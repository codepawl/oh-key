//! The keyboard hook, the Unikey way, on for every app that is not excluded.
//!
//! A `WH_KEYBOARD_LL` hook on its own thread feeds the keys to a private engine and writes
//! the result back by diffing what it already sent against the new word: only the
//! backspaces and the changed tail go out, in one `SendInput` batch, which keeps the
//! window for the race behind Unikey's doubled letters as small as it can be; see
//! `docs/research/bugs-and-peers.md`.
//!
//! The first half of the file is pure (the diff, the key decision, the typing session) and is
//! unit tested. The second half is the Windows glue.

use std::cell::{Cell, RefCell};
use std::sync::atomic::{AtomicU32, AtomicU64, AtomicUsize, Ordering};
use std::sync::mpsc;
use std::thread::JoinHandle;

use oh_key_engine::{Config, Engine, KeyResult};
use oh_key_ipc::{Settings, SharedState, ToggleHotkey, load_settings, macros_path};
use oh_key_macro::MacroTable;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, KEY_QUERY_VALUE, REG_DWORD, REG_VALUE_TYPE, RegCloseKey,
    RegOpenKeyExW, RegQueryValueExW,
};
use windows::Win32::System::SystemInformation::GetTickCount;
use windows::Win32::System::Threading::{
    GetCurrentProcess, GetCurrentThread, GetCurrentThreadId, OpenProcess, PROCESS_NAME_WIN32,
    PROCESS_POWER_THROTTLING_CURRENT_VERSION, PROCESS_POWER_THROTTLING_EXECUTION_SPEED,
    PROCESS_POWER_THROTTLING_STATE, PROCESS_QUERY_LIMITED_INFORMATION, ProcessPowerThrottling,
    QueryFullProcessImageNameW, SetProcessInformation, SetThreadPriority,
    THREAD_PRIORITY_TIME_CRITICAL,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, GetKeyState, GetKeyboardLayout, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT,
    KEYEVENTF_EXTENDEDKEY, KEYEVENTF_KEYUP, KEYEVENTF_UNICODE, SendInput, ToUnicodeEx, VIRTUAL_KEY,
};
use windows::Win32::UI::Input::{RAWINPUTDEVICE, RIDEV_INPUTSINK, RegisterRawInputDevices};
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW,
    GetForegroundWindow, GetMessageW, GetWindowThreadProcessId, HHOOK, HWND_MESSAGE,
    KBDLLHOOKSTRUCT, MSG, PM_NOREMOVE, PeekMessageW, PostMessageW, PostThreadMessageW,
    RegisterClassExW, SetWindowsHookExW, TranslateMessage, UnhookWindowsHookEx, WH_KEYBOARD_LL,
    WINDOW_EX_STYLE, WINDOW_STYLE, WM_APP, WM_INPUT, WM_KEYDOWN, WM_KEYUP, WM_QUIT, WM_SYSKEYDOWN,
    WM_SYSKEYUP, WM_USER, WNDCLASSEXW,
};
use windows::core::{Owned, PWSTR, w};

const VK_BACK: u32 = 0x08;
const VK_TAB: u32 = 0x09;
const VK_RETURN: u32 = 0x0D;
const VK_SHIFT: u32 = 0x10;
const VK_CONTROL: u32 = 0x11;
const VK_MENU: u32 = 0x12;
const VK_CAPITAL: u32 = 0x14;
const VK_ESCAPE: u32 = 0x1B;
const VK_LSHIFT: u32 = 0xA0;
const VK_RSHIFT: u32 = 0xA1;
const VK_LCONTROL: u32 = 0xA2;
const VK_RCONTROL: u32 = 0xA3;
const VK_LMENU: u32 = 0xA4;
const VK_RMENU: u32 = 0xA5;
const VK_PACKET: u32 = 0xE7;
const VK_Z: u32 = 0x5A;
/// Page Up, Page Down, End, Home, the arrows, Select, Print, Execute, Snapshot, Insert, Delete.
const FIRST_NAVIGATION_KEY: u32 = 0x21;
const LAST_NAVIGATION_KEY: u32 = 0x2E;
const FIRST_FUNCTION_KEY: u32 = 0x70;
const LAST_FUNCTION_KEY: u32 = 0x87;

/// Stamped on every key the hook sends, so it can tell its own output from anyone else's.
pub const OWN_OUTPUT_MARKER: usize = 0x6F68_6B79;
/// Stamped on the key pair that only cancels the menu bar: ignored like our other output, but
/// it is not part of an edit, so it must never count as the echo of one.
const OWN_MENU_CANCEL_MARKER: usize = 0x6F68_6B7A;
const LLKHF_INJECTED: u32 = 0x10;

// ---------------------------------------------------------------------------------------
// Pure part
// ---------------------------------------------------------------------------------------

/// What to send so the text on screen turns from what the hook sent into what the engine shows.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TextEdit {
    pub backspaces: usize,
    pub text: String,
}

impl TextEdit {
    pub fn is_empty(&self) -> bool {
        self.backspaces == 0 && self.text.is_empty()
    }
}

/// The smallest edit from `sent` to `wanted`: delete the tail that differs, type the new tail.
pub fn plan_edit(sent: &str, wanted: &str) -> TextEdit {
    let shared_characters = sent
        .chars()
        .zip(wanted.chars())
        .take_while(|(sent_character, wanted_character)| sent_character == wanted_character)
        .count();
    TextEdit {
        backspaces: sent.chars().count() - shared_characters,
        text: wanted.chars().skip(shared_characters).collect(),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stroke {
    Backspace,
    /// One UTF-16 unit, sent as a Unicode key event.
    Unit(u16),
    /// A key pressed again as it was (Enter, Tab, Escape), after it had to wait for an edit.
    Key(u16),
}

pub fn strokes_for(edit: &TextEdit) -> Vec<Stroke> {
    let mut strokes = Vec::with_capacity(edit.backspaces + edit.text.len());
    strokes.extend(std::iter::repeat_n(Stroke::Backspace, edit.backspaces));
    strokes.extend(edit.text.encode_utf16().map(Stroke::Unit));
    strokes
}

/// Typed first when letters are about to be deleted. An address bar selects its inline
/// suggestion after the caret, and a backspace would delete that selection instead of the
/// letter. Typing over the selection removes it, and the extra backspace removes this
/// invisible character again, so the count of backspaces stays right everywhere.
const ZERO_WIDTH_SPACE: u16 = 0x200B;

/// The strokes that really go out: the edit, behind the selection guard when it deletes.
pub fn strokes_for_sending(edit: &TextEdit) -> Vec<Stroke> {
    if edit.backspaces == 0 {
        return strokes_for(edit);
    }
    let mut strokes = vec![Stroke::Unit(ZERO_WIDTH_SPACE)];
    strokes.extend(strokes_for(&TextEdit {
        backspaces: edit.backspaces + 1,
        text: edit.text.clone(),
    }));
    strokes
}

/// Whether the hook leaves an event alone: its own output always, and keys other programs
/// injected unless the caller asked to see those (only the end-to-end test does).
pub fn should_ignore_event(
    injected: bool,
    extra_info: usize,
    accept_foreign_injected: bool,
) -> bool {
    extra_info == OWN_OUTPUT_MARKER
        || extra_info == OWN_MENU_CANCEL_MARKER
        || (injected && !accept_foreign_injected)
}

/// Everything the decision needs about one key press.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct KeyPress {
    pub virtual_key: u32,
    /// The character the layout produces for this press, if it is printable.
    pub character: Option<char>,
    pub ctrl: bool,
    pub alt: bool,
    /// Right Alt on many layouts reports Ctrl and Alt together.
    pub alt_gr: bool,
    pub windows_key: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyInput {
    /// Not for the engine and the word stays as it is.
    Ignore,
    Character(char),
    Backspace,
    /// The word ends here without text of its own (Enter, Tab, arrows): finish it first.
    FinishWord,
    /// A shortcut: forget the word without touching the text, since the shortcut may act on it.
    AbandonWord,
}

pub fn classify_key(press: &KeyPress) -> KeyInput {
    if press.virtual_key == VK_PACKET || is_modifier_key(press.virtual_key) {
        return KeyInput::Ignore;
    }
    let shortcut_held = (press.ctrl || press.alt) && !press.alt_gr;
    if shortcut_held || press.windows_key {
        return KeyInput::AbandonWord;
    }
    if press.virtual_key == VK_BACK {
        return KeyInput::Backspace;
    }
    if closes_word_without_text(press.virtual_key) {
        return KeyInput::FinishWord;
    }
    match press.character {
        Some(character) if !character.is_control() => KeyInput::Character(character),
        _ => KeyInput::FinishWord,
    }
}

fn closes_word_without_text(virtual_key: u32) -> bool {
    matches!(virtual_key, VK_RETURN | VK_TAB | VK_ESCAPE)
        || (FIRST_NAVIGATION_KEY..=LAST_NAVIGATION_KEY).contains(&virtual_key)
        || (FIRST_FUNCTION_KEY..=LAST_FUNCTION_KEY).contains(&virtual_key)
}

fn is_modifier_key(virtual_key: u32) -> bool {
    matches!(
        virtual_key,
        VK_SHIFT
            | VK_CONTROL
            | VK_MENU
            | VK_LSHIFT
            | VK_RSHIFT
            | VK_LCONTROL
            | VK_RCONTROL
            | VK_LMENU
            | VK_RMENU
    )
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Verdict {
    /// The application gets the key as it is.
    PassThrough,
    /// Send the edit first, then the application gets the key.
    EditThenPass(TextEdit),
    /// The application does not get the key; the edit stands in for it.
    Swallow(TextEdit),
}

/// The engine plus a record of the text the hook has put into the application for the word.
pub struct Session {
    engine: Engine,
    sent: String,
}

impl Session {
    pub fn new(config: Config) -> Self {
        Session {
            engine: Engine::new(config),
            sent: String::new(),
        }
    }

    pub fn set_config(&mut self, config: Config) {
        self.engine.set_config(config);
    }

    pub fn set_macros(&mut self, macros: &[(String, String)]) {
        self.engine.clear_macros();
        for (shortcut, replacement) in macros {
            self.engine.add_macro(shortcut, replacement);
        }
    }

    pub fn reset(&mut self) {
        self.engine.reset();
        self.sent.clear();
    }

    #[cfg(test)]
    pub fn sent(&self) -> &str {
        &self.sent
    }

    pub fn handle(&mut self, input: KeyInput) -> Verdict {
        match input {
            KeyInput::Ignore => Verdict::PassThrough,
            KeyInput::Character(character) => self.handle_character(character),
            KeyInput::Backspace => self.handle_backspace(),
            KeyInput::FinishWord => self.finish_word(),
            KeyInput::AbandonWord => {
                self.reset();
                Verdict::PassThrough
            }
        }
    }

    fn handle_character(&mut self, character: char) -> Verdict {
        let wanted = match self.engine.process_key(character) {
            KeyResult::Composing => self.engine.preedit().to_owned(),
            KeyResult::Commit(text) => text,
            KeyResult::PassThrough => {
                self.sent.clear();
                return Verdict::PassThrough;
            }
        };
        let edit = plan_edit(&self.sent, &wanted);
        self.sent = if self.engine.is_composing() {
            wanted
        } else {
            String::new()
        };
        let just_the_key = edit.backspaces == 0 && edit.text.chars().eq([character]);
        if just_the_key {
            Verdict::PassThrough
        } else {
            Verdict::Swallow(edit)
        }
    }

    fn handle_backspace(&mut self) -> Verdict {
        if !self.engine.is_composing() {
            self.sent.clear();
            return Verdict::PassThrough;
        }
        self.engine.backspace();
        let wanted = self.engine.preedit().to_owned();
        let edit = plan_edit(&self.sent, &wanted);
        self.sent = wanted;
        // Never passed on: an inline suggestion would take the application's backspace.
        Verdict::Swallow(edit)
    }

    fn finish_word(&mut self) -> Verdict {
        if !self.engine.is_composing() {
            self.reset();
            return Verdict::PassThrough;
        }
        let final_text = self.engine.commit();
        let edit = plan_edit(&self.sent, &final_text);
        self.sent.clear();
        if edit.is_empty() {
            Verdict::PassThrough
        } else {
            Verdict::EditThenPass(edit)
        }
    }
}

/// Detects the Ctrl+Shift chord: both pressed and released with no other key between.
#[derive(Debug, Default)]
pub struct ToggleChordTracker {
    control_seen: bool,
    shift_seen: bool,
    chord_broken: bool,
}

impl ToggleChordTracker {
    pub fn key_down(&mut self, virtual_key: u32) {
        let starts_new_chord = !self.control_seen && !self.shift_seen;
        if starts_new_chord && (is_control_key(virtual_key) || is_shift_key(virtual_key)) {
            // Letters typed before the modifiers went down belong to no chord.
            self.chord_broken = false;
        }
        if is_control_key(virtual_key) {
            self.control_seen = true;
        } else if is_shift_key(virtual_key) {
            self.shift_seen = true;
        } else {
            self.chord_broken = true;
        }
    }

    /// Returns true once, when the chord completes on release.
    pub fn key_up(&mut self, virtual_key: u32) -> bool {
        if !is_control_key(virtual_key) && !is_shift_key(virtual_key) {
            return false;
        }
        let completed = self.control_seen && self.shift_seen && !self.chord_broken;
        *self = ToggleChordTracker::default();
        completed
    }
}

fn is_control_key(virtual_key: u32) -> bool {
    matches!(virtual_key, VK_CONTROL | VK_LCONTROL | VK_RCONTROL)
}

fn is_shift_key(virtual_key: u32) -> bool {
    matches!(virtual_key, VK_SHIFT | VK_LSHIFT | VK_RSHIFT)
}

/// Whether this press is the one-key toggle, Alt+Z, which the hook must swallow.
pub fn is_alt_z_press(hotkey: ToggleHotkey, virtual_key: u32, alt: bool, ctrl: bool) -> bool {
    hotkey == ToggleHotkey::AltZ && virtual_key == VK_Z && alt && !ctrl
}

/// Whether the hook should type Vietnamese now. An unknown foreground process (usually an
/// elevated window, which `SendInput` cannot reach) is left alone.
pub fn hook_is_active(vietnamese: bool, foreground_exe: &str, excluded_apps: &[String]) -> bool {
    let excluded = excluded_apps.iter().any(|name| name == foreground_exe);
    vietnamese && !foreground_exe.is_empty() && !excluded
}

/// Where the key flow sends what it types: `SendInput` in the hook, a simulated field in tests.
pub trait OutputSink {
    /// Returns how many key events went in; the caller compares it with `edit_event_count`.
    fn send_edit(&mut self, edit: &TextEdit) -> usize;
    fn cancel_menu_activation(&mut self);
    /// Sends one key press and release again; returns how many events went in.
    fn send_key(&mut self, virtual_key: u32) -> usize {
        let _ = virtual_key;
        0
    }
    /// A monotonic clock for the budget and the echo deadline. Tests that do not model
    /// time keep the default.
    fn now_milliseconds(&self) -> u64 {
        0
    }
}

/// How many key events `send_edit` puts in for this edit (a press and a release per stroke).
pub fn edit_event_count(edit: &TextEdit) -> usize {
    if edit.is_empty() {
        return 0;
    }
    let text_units = edit.text.encode_utf16().count();
    let guard_strokes = if edit.backspaces == 0 { 0 } else { 2 };
    (edit.backspaces + text_units + guard_strokes) * 2
}

/// The switches for the fixes against the lag symptoms. All on in the app; the tests and the
/// suite's negative control turn them off to show the symptom.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Mitigations {
    /// Keys that arrive while an earlier edit is still on its way wait for it to land.
    pub echo_gating: bool,
    /// An edit started later than this many milliseconds after the key event began is not
    /// sent, because Windows may already have passed the key on.
    pub callback_budget_milliseconds: Option<u64>,
    /// Too many edits in a short time, or many edits without a new key, mean a loop (a key
    /// that seems stuck, our own output read as typing): the flow stops editing for a while.
    pub circuit_breaker: bool,
}

impl Mitigations {
    #[allow(dead_code)]
    pub fn none() -> Self {
        Mitigations {
            echo_gating: false,
            callback_budget_milliseconds: None,
            circuit_breaker: false,
        }
    }
}

impl Default for Mitigations {
    fn default() -> Self {
        Mitigations {
            // Off: on real Windows the echoes did not all come back, so held keys were never typed
            // (CI hook-e2e, run 43208642701: "Viet Nam " came out as "Vie").
            echo_gating: false,
            callback_budget_milliseconds: Some(DEFAULT_CALLBACK_BUDGET_MILLISECONDS),
            circuit_breaker: true,
        }
    }
}

/// Typing never produces more than about 30 edits a second; this many in one window is a loop.
const BREAKER_WINDOW_MILLISECONDS: u64 = 1000;
const BREAKER_MAXIMUM_EDITS_PER_WINDOW: u32 = 100;
/// One key press makes one edit, a replay after lag a few more; this many with no new key is a loop.
const BREAKER_MAXIMUM_EDITS_WITHOUT_NEW_KEY: u32 = 32;
/// While the breaker is open the hook only passes keys through.
const BREAKER_OPEN_MILLISECONDS: u64 = 1000;

/// Windows drops a low-level hook whose callback runs past `LowLevelHooksTimeout` (300 ms by
/// default). Half of it, at most this much, leaves room for scheduling delays after the send.
const DEFAULT_CALLBACK_BUDGET_MILLISECONDS: u64 = 120;
const MAXIMUM_CALLBACK_BUDGET_MILLISECONDS: u64 = 150;
const MINIMUM_CALLBACK_BUDGET_MILLISECONDS: u64 = 20;

pub fn budget_for_timeout(timeout_milliseconds: u64) -> u64 {
    (timeout_milliseconds / 2).clamp(
        MINIMUM_CALLBACK_BUDGET_MILLISECONDS,
        MAXIMUM_CALLBACK_BUDGET_MILLISECONDS,
    )
}

/// How long keys wait for our own edit to come back through the hook before the flow gives up
/// on it (an event was lost) and lets the keys through raw. A busy machine delays the echo by
/// the callbacks queued before it, so this is generous.
const ECHO_DEADLINE_MILLISECONDS: u64 = 250;

/// A key that arrived while an earlier edit was still on its way.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct DeferredKey {
    virtual_key: u32,
    input: KeyInput,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct KeyOutcome {
    /// The key must not reach the application.
    pub swallow: bool,
    /// The Vietnamese/English mode flipped during this event.
    pub toggled: bool,
}

/// Everything the hook decides for one key event, with the operating system taken out: the
/// chord tracking, the hotkey, whether the hook is active, the typing session and which key to
/// swallow. `HookState` and the headless tests both drive this one type.
pub struct KeyFlow {
    pub session: Session,
    chord: ToggleChordTracker,
    toggle_hotkey: ToggleHotkey,
    vietnamese: bool,
    excluded_apps: Vec<String>,
    foreground_exe: String,
    active: bool,
    /// The keys whose press was swallowed, so their release is swallowed too.
    swallowed_keys: Vec<u32>,
    mitigations: Mitigations,
    event_started_milliseconds: u64,
    /// Key events of our own edits that have not come back through the hook yet.
    echoes_outstanding: usize,
    echo_started_milliseconds: u64,
    deferred_keys: Vec<DeferredKey>,
    late_edits: usize,
    breaker_open_until_milliseconds: u64,
    breaker_window_started_milliseconds: u64,
    edits_in_breaker_window: u32,
    edits_since_new_key: u32,
    breaker_trips: usize,
    /// Replaying keys that waited: each was a key press of its own, not a loop.
    replaying_deferred_keys: bool,
}

/// More than a handful of keys at once is not typing; the capacity keeps the key path free of
/// allocations.
const KEY_BUFFER_CAPACITY: usize = 16;

impl KeyFlow {
    pub fn new(config: Config, toggle_hotkey: ToggleHotkey) -> Self {
        KeyFlow {
            session: Session::new(config),
            chord: ToggleChordTracker::default(),
            toggle_hotkey,
            vietnamese: false,
            excluded_apps: Vec::new(),
            foreground_exe: String::new(),
            active: false,
            swallowed_keys: Vec::with_capacity(KEY_BUFFER_CAPACITY),
            mitigations: Mitigations::default(),
            event_started_milliseconds: 0,
            echoes_outstanding: 0,
            echo_started_milliseconds: 0,
            deferred_keys: Vec::with_capacity(KEY_BUFFER_CAPACITY),
            late_edits: 0,
            breaker_open_until_milliseconds: 0,
            breaker_window_started_milliseconds: 0,
            edits_in_breaker_window: 0,
            edits_since_new_key: 0,
            breaker_trips: 0,
            replaying_deferred_keys: false,
        }
    }

    /// How many times the circuit breaker stopped a loop.
    pub fn breaker_trips(&self) -> usize {
        self.breaker_trips
    }

    /// The system passed this swallowed key on after all (the callback ran past the hook
    /// timeout), so its release must pass too, or the application would see a held key.
    pub fn release_swallow(&mut self, virtual_key: u32) {
        self.take_swallowed(virtual_key);
        // A key that waited and was passed on by the system must not be typed again by the replay.
        let waiting_position = self
            .deferred_keys
            .iter()
            .rposition(|key| key.virtual_key == virtual_key);
        if let Some(position) = waiting_position {
            self.deferred_keys.remove(position);
        }
    }

    fn breaker_is_open(&self, sink: &impl OutputSink) -> bool {
        sink.now_milliseconds() < self.breaker_open_until_milliseconds
    }

    fn count_edit_for_breaker(&mut self, sink: &mut impl OutputSink) {
        if !self.mitigations.circuit_breaker {
            return;
        }
        let now = sink.now_milliseconds();
        if now.saturating_sub(self.breaker_window_started_milliseconds)
            > BREAKER_WINDOW_MILLISECONDS
        {
            self.breaker_window_started_milliseconds = now;
            self.edits_in_breaker_window = 0;
        }
        self.edits_in_breaker_window += 1;
        if !self.replaying_deferred_keys {
            self.edits_since_new_key += 1;
        }
        let looping = self.edits_in_breaker_window > BREAKER_MAXIMUM_EDITS_PER_WINDOW
            || self.edits_since_new_key > BREAKER_MAXIMUM_EDITS_WITHOUT_NEW_KEY;
        if looping {
            self.trip_breaker(now, sink);
        }
    }

    /// Stops editing: the keys that waited are typed as they are, the word is dropped and the
    /// flow passes keys through until the breaker closes. Held-key pairing stays intact.
    fn trip_breaker(&mut self, now: u64, sink: &mut impl OutputSink) {
        self.breaker_trips += 1;
        self.breaker_open_until_milliseconds = now + BREAKER_OPEN_MILLISECONDS;
        self.edits_in_breaker_window = 0;
        self.edits_since_new_key = 0;
        self.echoes_outstanding = 0;
        self.session.reset();
        for deferred in &self.deferred_keys {
            let edit = raw_key_edit(deferred.input);
            sink.send_edit(&edit);
        }
        self.deferred_keys.clear();
    }

    pub fn set_mitigations(&mut self, mitigations: Mitigations) {
        self.mitigations = mitigations;
    }

    /// Edits refused because the callback had used up its budget.
    #[allow(dead_code)]
    #[cfg(test)]
    pub fn late_edits(&self) -> usize {
        self.late_edits
    }

    /// Called first for every hook callback, with the clock reading at its start.
    pub fn begin_event(&mut self, now_milliseconds: u64) {
        self.event_started_milliseconds = now_milliseconds;
    }

    #[cfg(test)]
    pub fn is_vietnamese(&self) -> bool {
        self.vietnamese
    }

    pub fn set_mode_and_apps(
        &mut self,
        vietnamese: bool,
        excluded_apps: Vec<String>,
        toggle_hotkey: ToggleHotkey,
    ) {
        self.vietnamese = vietnamese;
        self.excluded_apps = excluded_apps;
        self.toggle_hotkey = toggle_hotkey;
        self.refresh_active();
    }

    pub fn set_foreground_exe(&mut self, foreground_exe: String) {
        self.foreground_exe = foreground_exe;
        self.refresh_active();
    }

    fn refresh_active(&mut self) {
        let active = hook_is_active(self.vietnamese, &self.foreground_exe, &self.excluded_apps);
        if active != self.active {
            self.active = active;
            self.session.reset();
        }
    }

    /// The foreground window changed: drop the word and the chord. A pending release stays,
    /// because the application that gets it never saw the press.
    /// Keys that still waited were typed for the window left behind; they are dropped rather
    /// than typed into the new one.
    pub fn deactivate(&mut self) {
        self.active = false;
        self.chord = ToggleChordTracker::default();
        self.session.reset();
        self.deferred_keys.clear();
        self.echoes_outstanding = 0;
    }

    /// The hook was reinstalled after a lost state: no held key can be trusted. Keys that were
    /// swallowed while they waited for an echo that will not come are typed as they are.
    pub fn forget_held_keys(&mut self, sink: &mut impl OutputSink) {
        self.swallowed_keys.clear();
        for index in 0..self.deferred_keys.len() {
            let key = self.deferred_keys[index];
            self.send_raw(key, sink);
        }
        self.deferred_keys.clear();
        self.echoes_outstanding = 0;
        self.chord = ToggleChordTracker::default();
        self.session.reset();
    }

    fn toggle_mode(&mut self) {
        self.vietnamese = !self.vietnamese;
        self.refresh_active();
    }

    fn remember_swallowed(&mut self, virtual_key: u32) {
        if !self.swallowed_keys.contains(&virtual_key) {
            self.swallowed_keys.push(virtual_key);
        }
    }

    fn take_swallowed(&mut self, virtual_key: u32) -> bool {
        let Some(position) = self
            .swallowed_keys
            .iter()
            .position(|key| *key == virtual_key)
        else {
            return false;
        };
        self.swallowed_keys.swap_remove(position);
        true
    }

    /// The Ctrl+Shift chord completes on release; a swallowed key's release is swallowed too.
    pub fn key_up(&mut self, virtual_key: u32) -> KeyOutcome {
        let swallow = self.take_swallowed(virtual_key);
        let completed = self.chord.key_up(virtual_key);
        let toggled = completed && self.toggle_hotkey == ToggleHotkey::CtrlShift;
        if toggled {
            self.toggle_mode();
        }
        KeyOutcome { swallow, toggled }
    }

    /// `read_press` runs only when the hook is active, because reading the key state is costly.
    pub fn key_down(
        &mut self,
        virtual_key: u32,
        alt_held: bool,
        ctrl_held: bool,
        read_press: impl FnOnce() -> KeyPress,
        sink: &mut impl OutputSink,
    ) -> KeyOutcome {
        self.chord.key_down(virtual_key);
        // A press that is not swallowed again below must not leave an old pending release
        // behind, or the release of a key the application saw pressed would be swallowed.
        self.take_swallowed(virtual_key);
        self.edits_since_new_key = 0;
        if is_alt_z_press(self.toggle_hotkey, virtual_key, alt_held, ctrl_held) {
            self.toggle_mode();
            self.remember_swallowed(virtual_key);
            sink.cancel_menu_activation();
            return KeyOutcome {
                swallow: true,
                toggled: true,
            };
        }
        if !self.active || self.breaker_is_open(sink) {
            return KeyOutcome::default();
        }
        let input = classify_key(&read_press());
        if self.hold_back_for_edit_in_flight(virtual_key, input, sink) {
            return KeyOutcome {
                swallow: true,
                toggled: false,
            };
        }
        let verdict = self.session.handle(input);
        let swallow = self.carry_out(virtual_key, verdict, sink);
        KeyOutcome {
            swallow,
            toggled: false,
        }
    }

    fn carry_out(
        &mut self,
        virtual_key: u32,
        verdict: Verdict,
        sink: &mut impl OutputSink,
    ) -> bool {
        match verdict {
            Verdict::PassThrough => false,
            Verdict::EditThenPass(edit) => {
                self.deliver_edit(&edit, sink);
                false
            }
            Verdict::Swallow(edit) => {
                let delivered = self.deliver_edit(&edit, sink);
                if delivered {
                    self.remember_swallowed(virtual_key);
                }
                delivered
            }
        }
    }

    /// Sends the edit unless the callback is out of time. Returns whether the edit went in
    /// whole. When it did not, the word restarts and the key stands as the user typed it.
    fn deliver_edit(&mut self, edit: &TextEdit, sink: &mut impl OutputSink) -> bool {
        if self.over_budget(sink) {
            self.late_edits += 1;
            self.session.reset();
            return false;
        }
        self.send_counted(edit, sink)
    }

    fn over_budget(&self, sink: &impl OutputSink) -> bool {
        let Some(budget) = self.mitigations.callback_budget_milliseconds else {
            return false;
        };
        let elapsed = sink
            .now_milliseconds()
            .saturating_sub(self.event_started_milliseconds);
        elapsed > budget
    }

    fn send_counted(&mut self, edit: &TextEdit, sink: &mut impl OutputSink) -> bool {
        let expected = edit_event_count(edit);
        let sent = sink.send_edit(edit);
        self.count_edit_for_breaker(sink);
        if sent > 0 {
            if self.echoes_outstanding == 0 {
                self.echo_started_milliseconds = sink.now_milliseconds();
            }
            self.echoes_outstanding += sent;
        }
        if sent == expected {
            return true;
        }
        self.session.reset();
        false
    }

    /// A key typed while our earlier edit is still on its way would be measured against text
    /// that is not on screen yet. Characters, backspaces and Enter, Tab and Escape wait,
    /// swallowed, until the edit has come back through the hook. Any other key ends the wait:
    /// the keys that waited go through the engine now and the key reaches the application.
    fn hold_back_for_edit_in_flight(
        &mut self,
        virtual_key: u32,
        input: KeyInput,
        sink: &mut impl OutputSink,
    ) -> bool {
        let waiting = self.echoes_outstanding > 0 || !self.deferred_keys.is_empty();
        if !self.mitigations.echo_gating || !waiting || input == KeyInput::Ignore {
            return false;
        }
        // Measured at the start of this callback: time this callback spends itself is not time
        // the echo was missing, since the echo is queued behind it.
        let waited = self
            .event_started_milliseconds
            .saturating_sub(self.echo_started_milliseconds);
        if waited > ECHO_DEADLINE_MILLISECONDS {
            return self.stop_waiting_for_lost_echo(virtual_key, input, sink);
        }
        if self.deferred_keys.len() >= KEY_BUFFER_CAPACITY {
            // Held-key autorepeat: stay within the capacity, so the key path never allocates.
            self.replay_deferred_keys(sink);
        }
        if can_wait_for_echo(input) {
            self.deferred_keys.push(DeferredKey { virtual_key, input });
            self.remember_swallowed(virtual_key);
            return true;
        }
        false
    }

    /// Our edit never came back (an event was lost): what waits is typed as it is, the word
    /// starts over, and a character that arrives now joins the ones typed raw.
    fn stop_waiting_for_lost_echo(
        &mut self,
        virtual_key: u32,
        input: KeyInput,
        sink: &mut impl OutputSink,
    ) -> bool {
        self.echoes_outstanding = 0;
        if self.deferred_keys.is_empty() {
            return false;
        }
        let joins_the_raw_keys = matches!(input, KeyInput::Character(_) | KeyInput::Backspace);
        if joins_the_raw_keys {
            self.deferred_keys.push(DeferredKey { virtual_key, input });
            self.remember_swallowed(virtual_key);
        }
        self.session.reset();
        for index in 0..self.deferred_keys.len() {
            let key = self.deferred_keys[index];
            self.send_raw(key, sink);
        }
        self.deferred_keys.clear();
        joins_the_raw_keys
    }

    /// One of our own key events came back through the hook. When the last one of the batch
    /// lands, the keys that waited go through the engine, in order.
    pub fn on_own_echo(&mut self, sink: &mut impl OutputSink) {
        self.echoes_outstanding = self.echoes_outstanding.saturating_sub(1);
        if self.echoes_outstanding == 0 {
            self.replay_deferred_keys(sink);
        }
    }

    /// The engine's text stays right when the keys go through it in order, because it measures
    /// each edit against what the earlier edits will leave on screen.
    fn replay_deferred_keys(&mut self, sink: &mut impl OutputSink) {
        self.replaying_deferred_keys = true;
        // Sending can trip the circuit breaker, which empties the list under this loop.
        for index in 0..self.deferred_keys.len() {
            let Some(key) = self.deferred_keys.get(index).copied() else {
                break;
            };
            self.replay_deferred(key, sink);
        }
        self.replaying_deferred_keys = false;
        self.deferred_keys.clear();
    }

    fn replay_deferred(&mut self, key: DeferredKey, sink: &mut impl OutputSink) {
        match self.session.handle(key.input) {
            Verdict::PassThrough => self.send_raw(key, sink),
            Verdict::Swallow(edit) => {
                self.send_counted(&edit, sink);
            }
            Verdict::EditThenPass(edit) => {
                self.send_counted(&edit, sink);
                self.send_raw(key, sink);
            }
        }
    }

    /// The key itself, as the user pressed it.
    fn send_raw(&mut self, key: DeferredKey, sink: &mut impl OutputSink) {
        match key.input {
            KeyInput::Character(_) | KeyInput::Backspace => {
                self.send_counted(&raw_key_edit(key.input), sink);
            }
            _ => self.send_key_counted(key.virtual_key, sink),
        }
    }

    fn send_key_counted(&mut self, virtual_key: u32, sink: &mut impl OutputSink) {
        let sent = sink.send_key(virtual_key);
        if sent > 0 {
            if self.echoes_outstanding == 0 {
                self.echo_started_milliseconds = sink.now_milliseconds();
            }
            self.echoes_outstanding += sent;
        }
    }
}

/// Whether a key may wait for the edit before it. Every key but the ones the hook ignores:
/// a shortcut or an arrow that passed at once would act before the text it follows has landed.
/// The key is sent again after the edit, with the modifiers still held at that moment.
fn can_wait_for_echo(input: KeyInput) -> bool {
    !matches!(input, KeyInput::Ignore)
}

/// The key itself as an edit, for a key that was held back.
fn raw_key_edit(input: KeyInput) -> TextEdit {
    match input {
        KeyInput::Character(character) => TextEdit {
            backspaces: 0,
            text: character.to_string(),
        },
        KeyInput::Backspace => TextEdit {
            backspaces: 1,
            text: String::new(),
        },
        _ => TextEdit::default(),
    }
}

// ---------------------------------------------------------------------------------------
// Windows glue
// ---------------------------------------------------------------------------------------

/// What the hook thread asks the app for, at the start of a key event.
pub struct HookInputs {
    /// Lower case executable names.
    pub excluded_apps: Vec<String>,
    pub vietnamese: bool,
    pub engine: Config,
    pub toggle_hotkey: ToggleHotkey,
    /// The macros, only when they changed since the last call.
    pub macros: Option<Vec<(String, String)>>,
}

/// Called at the start of every key event, so it must be cheap when nothing changed: it returns
/// `None` then, and the full inputs only after the settings or the mode changed. Nothing runs
/// between keys, so an idle hook costs no wake-ups. The first call must return `Some`.
pub type InputsProvider = Box<dyn FnMut() -> Option<HookInputs> + Send>;

/// Flips the global Vietnamese/English mode when the hotkey is pressed.
pub type ToggleAction = Box<dyn FnMut() + Send>;

#[derive(Clone, Copy, Debug, Default)]
pub struct HookOptions {
    /// Also handle keys that other programs injected. Off in the app; the end-to-end
    /// example turns it on because `SendInput` is how it types.
    pub accept_foreign_injected_keys: bool,
    /// The fixes against the lag symptoms; the suite switches them off as a negative control.
    pub mitigations: Mitigations,
    /// Test only: make the callback sleep this long before it decides, on every Nth key event
    /// (zero for either value means never). Reproduces a loaded machine.
    pub stress_delay_milliseconds: u32,
    pub stress_every_keys: u32,
}

type MacroList = Vec<(String, String)>;

/// The app's source: published settings, the mode in the shared state and the macro file.
/// The generation counter (settings) and the mode counter are two atomic loads, so the common
/// key event reads nothing else. The macro file is read on a short-lived thread, never inside
/// the key callback: Windows drops a low-level hook whose callback runs too long. The macros
/// reach the hook with the first key after the read finishes.
pub fn app_inputs_provider(shared: SharedState) -> InputsProvider {
    let mut seen_stamp = None;
    let mut seen_generation = None;
    let mut settings: Option<Settings> = None;
    let mut pending_macros: Option<mpsc::Receiver<MacroList>> = None;
    Box::new(move || {
        let mut macros = take_finished_macros(&mut pending_macros);
        let generation = shared.generation();
        let stamp = (generation, shared.mode_counter());
        if seen_stamp == Some(stamp) && macros.is_none() {
            return None;
        }
        seen_stamp = Some(stamp);
        if seen_generation != Some(generation) {
            match shared.read_settings() {
                Some(published) => {
                    seen_generation = Some(generation);
                    let enabled = published.engine.macro_enabled;
                    settings = Some(published);
                    (macros, pending_macros) = start_macro_load(enabled);
                }
                None if settings.is_none() => {
                    let from_disk = load_settings();
                    seen_generation = Some(generation);
                    let enabled = from_disk.engine.macro_enabled;
                    settings = Some(from_disk);
                    (macros, pending_macros) = start_macro_load(enabled);
                }
                // A torn read: keep the settings in use and try again with the next key.
                None => seen_stamp = None,
            }
        }
        let current = settings.as_ref()?;
        Some(HookInputs {
            excluded_apps: current.excluded_apps.clone(),
            vietnamese: shared.vietnamese(),
            engine: current.engine,
            toggle_hotkey: current.toggle_hotkey,
            macros,
        })
    })
}

fn take_finished_macros(pending: &mut Option<mpsc::Receiver<MacroList>>) -> Option<MacroList> {
    let receiver = pending.as_ref()?;
    match receiver.try_recv() {
        Ok(macros) => {
            *pending = None;
            Some(macros)
        }
        Err(mpsc::TryRecvError::Empty) => None,
        Err(mpsc::TryRecvError::Disconnected) => {
            *pending = None;
            None
        }
    }
}

/// Macros that are off are known at once; otherwise the file is read in the background.
fn start_macro_load(macro_enabled: bool) -> (Option<MacroList>, Option<mpsc::Receiver<MacroList>>) {
    if !macro_enabled {
        return (Some(Vec::new()), None);
    }
    let (sender, receiver) = mpsc::channel();
    let spawned = std::thread::Builder::new()
        .name("oh-key-macros".to_owned())
        .spawn(move || {
            let _ = sender.send(read_macros(true));
        });
    match spawned {
        Ok(_) => (None, Some(receiver)),
        Err(_) => (None, None),
    }
}

fn read_macros(macro_enabled: bool) -> Vec<(String, String)> {
    if !macro_enabled {
        return Vec::new();
    }
    let Some(path) = macros_path() else {
        return Vec::new();
    };
    let Ok(loaded) = MacroTable::load(&path) else {
        return Vec::new();
    };
    loaded
        .table
        .items()
        .into_iter()
        .map(|(shortcut, replacement)| (shortcut.to_owned(), replacement.to_owned()))
        .collect()
}

/// Lower case file name of the executable that runs process `process_id`.
pub fn process_exe_name(process_id: u32) -> Option<String> {
    let process = unsafe {
        Owned::new(OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, process_id).ok()?)
    };
    let mut buffer = [0u16; 520];
    let mut length = buffer.len() as u32;
    unsafe {
        QueryFullProcessImageNameW(
            *process,
            PROCESS_NAME_WIN32,
            PWSTR(buffer.as_mut_ptr()),
            &mut length,
        )
        .ok()?;
    }
    let path = String::from_utf16_lossy(&buffer[..length as usize]);
    let file_name = path.rsplit(['\\', '/']).next().unwrap_or(&path);
    Some(file_name.to_lowercase())
}

pub fn window_exe_name(window: HWND) -> Option<String> {
    let mut process_id = 0u32;
    unsafe { GetWindowThreadProcessId(window, Some(&mut process_id)) };
    if process_id == 0 {
        return None;
    }
    process_exe_name(process_id)
}

/// The most program names remembered by process id; a full cache starts over.
const EXE_NAME_CACHE_LIMIT: usize = 64;

/// Something the hook noticed that is worth a log line or a report. Counts only: never a key,
/// a word or the program being typed into.
#[allow(dead_code)]
#[derive(Clone, Copy, Debug)]
pub enum HookDiagnostic {
    Reinstalled,
    BreakerTripped { total: usize },
    SendInputShort { sent: usize, expected: usize },
}

static DIAGNOSTIC_HANDLER: std::sync::OnceLock<fn(HookDiagnostic)> = std::sync::OnceLock::new();
/// A diagnostic of one kind is passed on at most this often, so a stuck loop cannot flood.
const DIAGNOSTIC_PAUSE_MILLISECONDS: i32 = 10_000;
static LAST_BREAKER_DIAGNOSTIC_TICK: AtomicU32 = AtomicU32::new(0);
static LAST_SHORT_SEND_DIAGNOSTIC_TICK: AtomicU32 = AtomicU32::new(0);
static BREAKER_TRIPS_SEEN: AtomicUsize = AtomicUsize::new(0);

/// The tray app sets this once; the examples never do, and then nothing is passed on.
#[allow(dead_code)]
pub fn set_diagnostic_handler(handler: fn(HookDiagnostic)) {
    let _ = DIAGNOSTIC_HANDLER.set(handler);
}

fn pass_on_diagnostic(diagnostic: HookDiagnostic) {
    if let Some(handler) = DIAGNOSTIC_HANDLER.get() {
        handler(diagnostic);
    }
}

fn pause_over(last_tick: &AtomicU32) -> bool {
    let now = unsafe { GetTickCount() };
    let last = last_tick.load(Ordering::Relaxed);
    if last != 0 && (now.wrapping_sub(last) as i32) < DIAGNOSTIC_PAUSE_MILLISECONDS {
        return false;
    }
    last_tick.store(now.max(1), Ordering::Relaxed);
    true
}

fn note_breaker_trips(total: usize) {
    if total == BREAKER_TRIPS_SEEN.swap(total, Ordering::Relaxed) {
        return;
    }
    if pause_over(&LAST_BREAKER_DIAGNOSTIC_TICK) {
        pass_on_diagnostic(HookDiagnostic::BreakerTripped { total });
    }
}

fn note_short_send(sent: usize, expected: usize) {
    if pause_over(&LAST_SHORT_SEND_DIAGNOSTIC_TICK) {
        pass_on_diagnostic(HookDiagnostic::SendInputShort { sent, expected });
    }
}

/// Sends the hook's output with `SendInput`.
struct SendInputSink;

impl OutputSink for SendInputSink {
    fn send_edit(&mut self, edit: &TextEdit) -> usize {
        send_edit(edit)
    }

    fn cancel_menu_activation(&mut self) {
        cancel_menu_activation();
    }

    fn send_key(&mut self, virtual_key: u32) -> usize {
        let stroke = Stroke::Key(virtual_key as u16);
        let events = [keyboard_event(stroke, false), keyboard_event(stroke, true)];
        unsafe { SendInput(&events, size_of::<INPUT>() as i32) as usize }
    }

    fn now_milliseconds(&self) -> u64 {
        monotonic_milliseconds()
    }
}

fn monotonic_milliseconds() -> u64 {
    static START: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
    let start = START.get_or_init(std::time::Instant::now);
    start.elapsed().as_millis() as u64
}

struct HookState {
    provider: InputsProvider,
    toggle_action: ToggleAction,
    options: HookOptions,
    flow: KeyFlow,
    config: Config,
    foreground: HWND,
    /// Program names by process id, so a window switch back to a known program asks nothing.
    exe_names: std::collections::HashMap<u32, String>,
    stress_counter: u32,
}

/// `LowLevelHooksTimeout` in the current user's desktop settings, when it is set.
fn low_level_hooks_timeout_milliseconds() -> Option<u64> {
    let mut key = HKEY::default();
    let opened = unsafe {
        RegOpenKeyExW(
            HKEY_CURRENT_USER,
            w!("Control Panel\\Desktop"),
            None,
            KEY_QUERY_VALUE,
            &mut key,
        )
    };
    if opened.is_err() {
        return None;
    }
    let mut value = 0u32;
    let mut size = size_of::<u32>() as u32;
    let mut value_type = REG_VALUE_TYPE::default();
    let queried = unsafe {
        RegQueryValueExW(
            key,
            w!("LowLevelHooksTimeout"),
            None,
            Some(&mut value_type),
            Some((&mut value as *mut u32).cast::<u8>()),
            Some(&mut size),
        )
    };
    let _ = unsafe { RegCloseKey(key) };
    (queried.is_ok() && value_type == REG_DWORD).then_some(u64::from(value))
}

/// The budget the user's own timeout allows, never above the configured one.
fn mitigations_for_this_machine(configured: Mitigations) -> Mitigations {
    let timeout = low_level_hooks_timeout_milliseconds();
    let cutoff =
        late_swallow_cutoff_for_timeout(timeout.unwrap_or(DEFAULT_HOOKS_TIMEOUT_MILLISECONDS));
    LATE_SWALLOW_IGNORED_MILLISECONDS.store(cutoff, Ordering::Relaxed);
    let Some(configured_budget) = configured.callback_budget_milliseconds else {
        return configured;
    };
    let allowed = timeout.map_or(configured_budget, |timeout| {
        budget_for_timeout(timeout).min(configured_budget)
    });
    Mitigations {
        callback_budget_milliseconds: Some(allowed),
        ..configured
    }
}

thread_local! {
    /// The hook callback has no user data, so the state lives on the hook thread.
    static STATE: RefCell<Option<HookState>> = const { RefCell::new(None) };
}

impl HookState {
    fn new(
        mut provider: InputsProvider,
        toggle_action: ToggleAction,
        options: HookOptions,
    ) -> Self {
        let inputs = provider().expect("the first call of an inputs provider returns the inputs");
        let mut state = HookState {
            provider,
            toggle_action,
            options,
            flow: KeyFlow::new(inputs.engine, inputs.toggle_hotkey),
            config: inputs.engine,
            foreground: HWND::default(),
            exe_names: std::collections::HashMap::new(),
            stress_counter: 0,
        };
        state
            .flow
            .set_mitigations(mitigations_for_this_machine(options.mitigations));
        state.apply_inputs(inputs);
        state.follow_foreground();
        state
    }

    /// Runs first in every key event. A window switch drops the word and resolves the new
    /// window's program; the provider says whether the settings or the mode changed.
    fn catch_up(&mut self) {
        let current = unsafe { GetForegroundWindow() };
        // Focus passes through no window at all while windows switch under load; that is not
        // a new program, and resetting the word there would restart it mid-syllable.
        if !current.is_invalid() && current != self.foreground {
            FOREGROUND_CHANGES.fetch_add(1, Ordering::Relaxed);
            self.flow.deactivate();
            self.follow_foreground();
        }
        if let Some(inputs) = (self.provider)() {
            // The settings changed (or the session came back): read the timeout again too.
            self.flow
                .set_mitigations(mitigations_for_this_machine(self.options.mitigations));
            self.apply_inputs(inputs);
        }
    }

    fn apply_inputs(&mut self, inputs: HookInputs) {
        if inputs.engine != self.config {
            self.config = inputs.engine;
            self.flow.session.set_config(inputs.engine);
        }
        if let Some(macros) = inputs.macros {
            self.flow.session.set_macros(&macros);
        }
        self.flow.set_mode_and_apps(
            inputs.vietnamese,
            inputs.excluded_apps,
            inputs.toggle_hotkey,
        );
    }

    fn follow_foreground(&mut self) {
        self.foreground = unsafe { GetForegroundWindow() };
        let exe_name = self.exe_name_of_foreground();
        self.flow.set_foreground_exe(exe_name);
    }

    /// Asking Windows for a program name costs a few system calls, so a known process id is
    /// answered from memory.
    fn exe_name_of_foreground(&mut self) -> String {
        let mut process_id = 0u32;
        unsafe { GetWindowThreadProcessId(self.foreground, Some(&mut process_id)) };
        if process_id == 0 {
            return String::new();
        }
        if let Some(known) = self.exe_names.get(&process_id) {
            return known.clone();
        }
        // A failed lookup (an elevated window, a process under memory pressure) is remembered
        // too: asking again on every key would put the slow calls back on the key path.
        let exe_name = process_exe_name(process_id).unwrap_or_default();
        if self.exe_names.len() >= EXE_NAME_CACHE_LIMIT {
            self.exe_names.clear();
        }
        self.exe_names.insert(process_id, exe_name.clone());
        exe_name
    }

    /// Takes effect at once; the next key event reads the same mode back from the shared state.
    fn run_toggle_action(&mut self, outcome: KeyOutcome) {
        if outcome.toggled {
            (self.toggle_action)();
        }
    }

    /// The stress knob of the end-to-end suite: a callback that runs long, on every Nth key.
    fn sleep_when_stressed(&mut self) {
        let every = self.options.stress_every_keys;
        let delay = self.options.stress_delay_milliseconds;
        if every == 0 || delay == 0 {
            return;
        }
        self.stress_counter += 1;
        if self.stress_counter.is_multiple_of(every) {
            std::thread::sleep(std::time::Duration::from_millis(u64::from(delay)));
        }
    }

    /// Returns whether the event is swallowed.
    fn handle_event(&mut self, message: u32, event: &KBDLLHOOKSTRUCT) -> bool {
        let injected = event.flags.0 & LLKHF_INJECTED != 0;
        self.flow.begin_event(monotonic_milliseconds());
        if event.dwExtraInfo == OWN_OUTPUT_MARKER {
            self.flow.on_own_echo(&mut SendInputSink);
        }
        if should_ignore_event(
            injected,
            event.dwExtraInfo,
            self.options.accept_foreign_injected_keys,
        ) {
            return false;
        }
        self.catch_up();
        self.sleep_when_stressed();
        if !injected {
            PHYSICAL_KEYS.fetch_add(1, Ordering::Relaxed);
        }
        let virtual_key = event.vkCode;
        let outcome = if matches!(message, WM_KEYUP | WM_SYSKEYUP) {
            self.flow.key_up(virtual_key)
        } else if matches!(message, WM_KEYDOWN | WM_SYSKEYDOWN) {
            self.flow.key_down(
                virtual_key,
                key_is_down(VK_MENU),
                key_is_down(VK_CONTROL),
                || read_key_press(event),
                &mut SendInputSink,
            )
        } else {
            KeyOutcome::default()
        };
        self.run_toggle_action(outcome);
        outcome.swallow
    }
}

fn key_is_down(virtual_key: u32) -> bool {
    let state = unsafe { GetAsyncKeyState(virtual_key as i32) };
    state < 0
}

fn read_key_press(event: &KBDLLHOOKSTRUCT) -> KeyPress {
    let ctrl = key_is_down(VK_CONTROL);
    let alt = key_is_down(VK_MENU);
    let alt_gr = key_is_down(VK_RMENU) && !key_is_down(VK_LMENU) && !key_is_down(VK_RCONTROL);
    let windows_key = key_is_down(0x5B) || key_is_down(0x5C);
    let shortcut_held = (ctrl || alt) && !alt_gr;
    let character = if shortcut_held || windows_key {
        None
    } else {
        character_for_key(event)
    };
    KeyPress {
        virtual_key: event.vkCode,
        character,
        ctrl,
        alt,
        alt_gr,
        windows_key,
    }
}

/// What the foreground window's layout types for this key with the modifiers held now.
/// The "do not change the keyboard state" flag keeps dead keys from being consumed here.
fn character_for_key(event: &KBDLLHOOKSTRUCT) -> Option<char> {
    const DO_NOT_CHANGE_KEYBOARD_STATE: u32 = 0x4;
    let mut state = [0u8; 256];
    if key_is_down(VK_SHIFT) {
        state[VK_SHIFT as usize] = 0x80;
    }
    if unsafe { GetKeyState(VK_CAPITAL as i32) } & 1 != 0 {
        state[VK_CAPITAL as usize] = 1;
    }
    if key_is_down(VK_RMENU) {
        state[VK_CONTROL as usize] = 0x80;
        state[VK_MENU as usize] = 0x80;
    }
    let foreground = unsafe { GetForegroundWindow() };
    let thread_id = unsafe { GetWindowThreadProcessId(foreground, None) };
    let layout = unsafe { GetKeyboardLayout(thread_id) };
    let mut buffer = [0u16; 4];
    let written = unsafe {
        ToUnicodeEx(
            event.vkCode,
            event.scanCode,
            &state,
            &mut buffer,
            DO_NOT_CHANGE_KEYBOARD_STATE,
            Some(layout),
        )
    };
    if written != 1 {
        return None;
    }
    char::from_u32(u32::from(buffer[0]))
}

/// Page Up/Down, End, Home, the arrows, Insert and Delete: sent again without the extended
/// flag they would act as their number pad twins.
fn is_extended_key(virtual_key: u16) -> bool {
    matches!(virtual_key, 0x21..=0x28 | 0x2D | 0x2E)
}

fn keyboard_event(stroke: Stroke, release: bool) -> INPUT {
    let release_flag = if release {
        KEYEVENTF_KEYUP
    } else {
        Default::default()
    };
    let (virtual_key, scan_code, flags) = match stroke {
        Stroke::Backspace => (VIRTUAL_KEY(VK_BACK as u16), 0, release_flag),
        Stroke::Unit(unit) => (VIRTUAL_KEY(0), unit, KEYEVENTF_UNICODE | release_flag),
        Stroke::Key(virtual_key) => {
            let extended = if is_extended_key(virtual_key) {
                KEYEVENTF_EXTENDEDKEY
            } else {
                Default::default()
            };
            (VIRTUAL_KEY(virtual_key), 0, release_flag | extended)
        }
    };
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: virtual_key,
                wScan: scan_code,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: OWN_OUTPUT_MARKER,
            },
        },
    }
}

/// Alt pressed and released alone opens the window's menu bar. With the Z swallowed, the
/// window saw exactly that, so an unassigned key goes in between to make it a different chord.
fn cancel_menu_activation() {
    const UNASSIGNED_KEY: u16 = 0xE8;
    let mut press = keyboard_event(Stroke::Backspace, false);
    let mut release = keyboard_event(Stroke::Backspace, true);
    press.Anonymous.ki.wVk = VIRTUAL_KEY(UNASSIGNED_KEY);
    release.Anonymous.ki.wVk = VIRTUAL_KEY(UNASSIGNED_KEY);
    press.Anonymous.ki.dwExtraInfo = OWN_MENU_CANCEL_MARKER;
    release.Anonymous.ki.dwExtraInfo = OWN_MENU_CANCEL_MARKER;
    unsafe { SendInput(&[press, release], size_of::<INPUT>() as i32) };
}

/// The whole edit in one `SendInput` call, so nothing the user types lands in the middle.
/// Returns how many events Windows took; fewer than sent means the input was blocked.
fn send_edit(edit: &TextEdit) -> usize {
    let mut events = Vec::with_capacity((edit.backspaces + edit.text.len() + 2) * 2);
    for stroke in strokes_for_sending(edit) {
        events.push(keyboard_event(stroke, false));
        events.push(keyboard_event(stroke, true));
    }
    if events.is_empty() {
        return 0;
    }
    let sent = unsafe { SendInput(&events, size_of::<INPUT>() as i32) as usize };
    if sent < events.len() {
        note_short_send(sent, events.len());
    }
    sent
}

const TIMING_CAPACITY: usize = 16384;
static CALLBACK_MICROSECONDS: [AtomicU32; TIMING_CAPACITY] =
    [const { AtomicU32::new(0) }; TIMING_CAPACITY];
static CALLBACK_COUNT: AtomicUsize = AtomicUsize::new(0);
/// How many times the foreground window changed (the hook drops its word then).
static FOREGROUND_CHANGES: AtomicUsize = AtomicUsize::new(0);
/// How many keys a person (not a program) pressed while the hook was watching the foreground.
static PHYSICAL_KEYS: AtomicUsize = AtomicUsize::new(0);

/// Grows whenever something other than the end-to-end suite touched the keyboard or the
/// foreground, so the suite can tell a disturbed run from a failed one.
#[allow(dead_code)]
pub fn outside_interference() -> usize {
    FOREGROUND_CHANGES.load(Ordering::Relaxed) + PHYSICAL_KEYS.load(Ordering::Relaxed)
}

fn record_callback_time(elapsed: std::time::Duration) {
    let slot = CALLBACK_COUNT.fetch_add(1, Ordering::Relaxed);
    if slot < TIMING_CAPACITY {
        let microseconds = elapsed.as_micros().min(u128::from(u32::MAX)) as u32;
        CALLBACK_MICROSECONDS[slot].store(microseconds, Ordering::Relaxed);
    }
}

/// How long each key callback took so far (the first 16384 keys), for the end-to-end suite.
#[allow(dead_code)]
pub fn callback_microseconds() -> Vec<u32> {
    let recorded = CALLBACK_COUNT.load(Ordering::Relaxed).min(TIMING_CAPACITY);
    CALLBACK_MICROSECONDS[..recorded]
        .iter()
        .map(|slot| slot.load(Ordering::Relaxed))
        .collect()
}

/// A callback this long is treated as already passed on by Windows. Derived from the user's
/// `LowLevelHooksTimeout`: the default timeout of 300 ms gives 250 ms.
static LATE_SWALLOW_IGNORED_MILLISECONDS: AtomicU64 = AtomicU64::new(250);
const DEFAULT_HOOKS_TIMEOUT_MILLISECONDS: u64 = 300;

/// Five sixths of the timeout leaves a margin for the clock and the scheduler.
pub fn late_swallow_cutoff_for_timeout(timeout_milliseconds: u64) -> u64 {
    timeout_milliseconds * 5 / 6
}

/// Callbacks that took longer than this are counted; they are what Windows may punish.
const SLOW_CALLBACK_MICROSECONDS: u32 = 50_000;
static SLOW_CALLBACKS: AtomicUsize = AtomicUsize::new(0);
/// `GetTickCount` at the start of the last hook callback, to notice a hook that Windows removed.
static LAST_CALLBACK_TICK: AtomicU32 = AtomicU32::new(0);
/// Counts hook callbacks, so the watchdog can tell whether one arrived since its last reinstall.
static CALLBACK_SERIAL: AtomicU32 = AtomicU32::new(0);
/// Keyboard keys the watchdog saw and hooks it reinstalled, for the end-to-end suite.
static WATCHDOG_KEYS: AtomicUsize = AtomicUsize::new(0);
static WATCHDOG_REINSTALLS: AtomicUsize = AtomicUsize::new(0);
/// Posted to the watch window, so a reinstall never runs inside `WM_INPUT`.
const WM_REINSTALL_HOOK: u32 = WM_APP + 1;

/// How many keyboard keys the watchdog saw and how many times it reinstalled the hook.
#[allow(dead_code)]
pub fn watchdog_counts() -> (usize, usize) {
    (
        WATCHDOG_KEYS.load(Ordering::Relaxed),
        WATCHDOG_REINSTALLS.load(Ordering::Relaxed),
    )
}

/// How many key callbacks ran longer than 50 ms so far.
#[allow(dead_code)]
pub fn slow_callback_count() -> usize {
    SLOW_CALLBACKS.load(Ordering::Relaxed)
}

/// A keyboard key is newer than the last callback by this much: the hook is gone.
const SILENT_HOOK_MILLISECONDS: i32 = 1500;
/// A reinstall is cheap but not free; never more often than this.
const REINSTALL_PAUSE_MILLISECONDS: i32 = 10_000;

/// Windows removes a low-level hook whose callback ran past its timeout, without telling the
/// app. A keyboard key that produced no callback for a while means the hook is gone. The key
/// tick comes from raw keyboard input only: mouse input never reaches a keyboard hook, so
/// counting it (as `GetLastInputInfo` does) took every mouse move for a removed hook.
pub fn hook_seems_removed(last_key_tick: u32, last_callback_tick: u32) -> bool {
    last_key_tick.wrapping_sub(last_callback_tick) as i32 > SILENT_HOOK_MILLISECONDS
}

/// What the watchdog knows when a keyboard key arrives.
#[derive(Clone, Copy, Debug)]
pub struct WatchdogFacts {
    pub key_tick: u32,
    pub last_callback_tick: u32,
    pub last_reinstall_tick: Option<u32>,
    /// A callback ran after the last reinstall.
    pub callback_since_reinstall: bool,
}

/// A reinstall that brought no callback back means the hook was blocked, not removed: an
/// elevated window has the keyboard, and Windows keeps a normal app's hook out of it. Trying
/// again would only repeat, so the watchdog waits for a callback before it may reinstall again.
pub fn should_reinstall(facts: WatchdogFacts) -> bool {
    if !hook_seems_removed(facts.key_tick, facts.last_callback_tick) {
        return false;
    }
    match facts.last_reinstall_tick {
        None => true,
        Some(last) => {
            facts.callback_since_reinstall
                && facts.key_tick.wrapping_sub(last) as i32 >= REINSTALL_PAUSE_MILLISECONDS
        }
    }
}

/// What the watchdog does with one keyboard key.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WatchdogStep {
    /// The hook answered, or may not be reinstalled now.
    Nothing,
    /// No callback came for this key; the callback count is kept to compare at the next key.
    Suspect(u32),
    Reinstall,
}

/// A key's raw input can be handled before that key's hook callback, so one key without a
/// callback proves nothing. A second key while the callback count has not moved since the
/// first one does.
pub fn watchdog_step(
    facts: WatchdogFacts,
    suspect_serial: Option<u32>,
    serial_now: u32,
) -> WatchdogStep {
    if !should_reinstall(facts) {
        return WatchdogStep::Nothing;
    }
    if suspect_serial == Some(serial_now) {
        WatchdogStep::Reinstall
    } else {
        WatchdogStep::Suspect(serial_now)
    }
}

unsafe extern "system" fn keyboard_hook(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    const HC_ACTION: i32 = 0;
    if code == HC_ACTION {
        let started = std::time::Instant::now();
        LAST_CALLBACK_TICK.store(unsafe { GetTickCount() }, Ordering::Relaxed);
        CALLBACK_SERIAL.fetch_add(1, Ordering::Relaxed);
        let event = unsafe { &*(lparam.0 as *const KBDLLHOOKSTRUCT) };
        let mut swallowed = STATE.with(|cell| match cell.try_borrow_mut() {
            Ok(mut guard) => guard.as_mut().is_some_and(|state| {
                let swallowed = state.handle_event(wparam.0 as u32, event);
                note_breaker_trips(state.flow.breaker_trips());
                swallowed
            }),
            Err(_) => false,
        });
        let elapsed = started.elapsed();
        record_callback_time(elapsed);
        let late_cutoff = LATE_SWALLOW_IGNORED_MILLISECONDS.load(Ordering::Relaxed);
        if swallowed && elapsed.as_millis() >= u128::from(late_cutoff) {
            // Windows has probably passed the key on already; its release must pass as well,
            // or the application would see a key that stays down and repeats.
            swallowed = false;
            STATE.with(|cell| {
                if let Ok(mut guard) = cell.try_borrow_mut()
                    && let Some(state) = guard.as_mut()
                {
                    state.flow.release_swallow(event.vkCode);
                }
            });
        }
        if elapsed.as_micros() > u128::from(SLOW_CALLBACK_MICROSECONDS) {
            SLOW_CALLBACKS.fetch_add(1, Ordering::Relaxed);
        }
        if swallowed {
            return LRESULT(1);
        }
    }
    unsafe { CallNextHookEx(None, code, wparam, lparam) }
}

/// The running hook thread. Dropping it removes the hook and joins the thread.
pub struct HookHandle {
    thread_id: u32,
    thread: Option<JoinHandle<()>>,
}

impl Drop for HookHandle {
    fn drop(&mut self) {
        let _ = unsafe { PostThreadMessageW(self.thread_id, WM_QUIT, WPARAM(0), LPARAM(0)) };
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// Starts the hook thread. `None` when Windows refused the hook.
pub fn start(
    provider: InputsProvider,
    toggle_action: ToggleAction,
    options: HookOptions,
) -> Option<HookHandle> {
    let (sender, receiver) = mpsc::channel();
    let thread = std::thread::Builder::new()
        .name("oh-key-hook".to_owned())
        .spawn(move || run_hook_thread(provider, toggle_action, options, sender))
        .ok()?;
    match receiver.recv() {
        Ok(Some(thread_id)) => Some(HookHandle {
            thread_id,
            thread: Some(thread),
        }),
        _ => {
            let _ = thread.join();
            None
        }
    }
}

thread_local! {
    /// The installed hook, the tick of the last reinstall and the callback count at that
    /// moment; only the hook thread touches them.
    static INSTALLED_HOOK: Cell<Option<HHOOK>> = const { Cell::new(None) };
    static LAST_REINSTALL_TICK: Cell<Option<u32>> = const { Cell::new(None) };
    static SERIAL_AT_REINSTALL: Cell<u32> = const { Cell::new(0) };
    /// The callback count at a key that came without a callback.
    static SUSPECT_SERIAL: Cell<Option<u32>> = const { Cell::new(None) };
}

fn run_hook_thread(
    provider: InputsProvider,
    toggle_action: ToggleAction,
    options: HookOptions,
    ready: mpsc::Sender<Option<u32>>,
) {
    STATE.with(|cell| *cell.borrow_mut() = Some(HookState::new(provider, toggle_action, options)));
    prefer_responsiveness();
    let Some(hook) = install_hook() else {
        let _ = ready.send(None);
        return;
    };
    INSTALLED_HOOK.with(|cell| cell.set(Some(hook)));
    LAST_CALLBACK_TICK.store(unsafe { GetTickCount() }, Ordering::Relaxed);
    // Event driven, so an idle app still has no timer: each keyboard key is the moment to check.
    let key_watch = watch_keyboard_keys();
    let mut message = MSG::default();
    // Creates this thread's message queue, so a quit posted right away is not lost.
    let _ = unsafe { PeekMessageW(&mut message, None, WM_USER, WM_USER, PM_NOREMOVE) };
    let _ = ready.send(Some(unsafe { GetCurrentThreadId() }));
    while unsafe { GetMessageW(&mut message, None, 0, 0) }.0 > 0 {
        unsafe {
            let _ = TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    }
    if let Some(window) = key_watch {
        let _ = unsafe { DestroyWindow(window) };
    }
    if let Some(current) = INSTALLED_HOOK.with(Cell::take) {
        let _ = unsafe { UnhookWindowsHookEx(current) };
    }
    STATE.with(|cell| *cell.borrow_mut() = None);
}

/// The hook thread must answer fast, and Windows must not park it on an efficiency core.
fn prefer_responsiveness() {
    unsafe {
        let _ = SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_TIME_CRITICAL);
        let state = PROCESS_POWER_THROTTLING_STATE {
            Version: PROCESS_POWER_THROTTLING_CURRENT_VERSION,
            ControlMask: PROCESS_POWER_THROTTLING_EXECUTION_SPEED,
            StateMask: 0,
        };
        let _ = SetProcessInformation(
            GetCurrentProcess(),
            ProcessPowerThrottling,
            (&state as *const PROCESS_POWER_THROTTLING_STATE).cast(),
            size_of::<PROCESS_POWER_THROTTLING_STATE>() as u32,
        );
    }
}

/// A message-only window that receives raw keyboard input in the background. Only the time of
/// each key is used, never which key it was. The hook callback for a key runs on this thread
/// before the key's raw input does, so a live hook has always stamped its tick first.
fn watch_keyboard_keys() -> Option<HWND> {
    let instance = unsafe { GetModuleHandleW(None) }.ok()?;
    let class_name = w!("oh-key-hook-watch");
    let class = WNDCLASSEXW {
        cbSize: size_of::<WNDCLASSEXW>() as u32,
        lpfnWndProc: Some(key_watch_proc),
        hInstance: instance.into(),
        lpszClassName: class_name,
        ..Default::default()
    };
    // Registering twice fails harmlessly; the class from the first call stays.
    unsafe { RegisterClassExW(&class) };
    let window = unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE(0),
            class_name,
            w!(""),
            WINDOW_STYLE(0),
            0,
            0,
            0,
            0,
            Some(HWND_MESSAGE),
            None,
            Some(instance.into()),
            None,
        )
    }
    .ok()?;
    // Generic desktop page, keyboard usage.
    let keyboard = RAWINPUTDEVICE {
        usUsagePage: 0x01,
        usUsage: 0x06,
        dwFlags: RIDEV_INPUTSINK,
        hwndTarget: window,
    };
    let registered =
        unsafe { RegisterRawInputDevices(&[keyboard], size_of::<RAWINPUTDEVICE>() as u32) };
    if registered.is_err() {
        let _ = unsafe { DestroyWindow(window) };
        return None;
    }
    Some(window)
}

unsafe extern "system" fn key_watch_proc(
    window: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if message == WM_INPUT {
        WATCHDOG_KEYS.fetch_add(1, Ordering::Relaxed);
        check_key(window, unsafe { GetTickCount() });
    }
    if message == WM_REINSTALL_HOOK {
        reinstall_hook_if_removed(wparam.0 as u32, lparam.0 as u32);
        return LRESULT(0);
    }
    // DefWindowProc frees the raw input buffer.
    unsafe { DefWindowProcW(window, message, wparam, lparam) }
}

fn watchdog_facts(key_tick: u32, serial_now: u32) -> WatchdogFacts {
    WatchdogFacts {
        key_tick,
        last_callback_tick: LAST_CALLBACK_TICK.load(Ordering::Relaxed),
        last_reinstall_tick: LAST_REINSTALL_TICK.with(Cell::get),
        callback_since_reinstall: SERIAL_AT_REINSTALL.with(Cell::get) != serial_now,
    }
}

fn check_key(window: HWND, key_tick: u32) {
    let serial_now = CALLBACK_SERIAL.load(Ordering::Relaxed);
    let facts = watchdog_facts(key_tick, serial_now);
    match watchdog_step(facts, SUSPECT_SERIAL.with(Cell::get), serial_now) {
        WatchdogStep::Nothing => SUSPECT_SERIAL.with(|cell| cell.set(None)),
        WatchdogStep::Suspect(serial) => SUSPECT_SERIAL.with(|cell| cell.set(Some(serial))),
        WatchdogStep::Reinstall => {
            SUSPECT_SERIAL.with(|cell| cell.set(None));
            // Windows may still be waiting on the old hook for this very key, so the swap
            // happens after this message, not inside it.
            let _ = unsafe {
                PostMessageW(
                    Some(window),
                    WM_REINSTALL_HOOK,
                    WPARAM(key_tick as usize),
                    LPARAM(serial_now as isize),
                )
            };
        }
    }
}

fn reinstall_hook_if_removed(key_tick: u32, serial_at_post: u32) {
    let serial_now = CALLBACK_SERIAL.load(Ordering::Relaxed);
    // A callback that arrived after the post, or a reinstall already done, cancels this one.
    if serial_now != serial_at_post || !should_reinstall(watchdog_facts(key_tick, serial_now)) {
        return;
    }
    if let Some(old) = INSTALLED_HOOK.with(Cell::take) {
        let _ = unsafe { UnhookWindowsHookEx(old) };
    }
    let installed = install_hook();
    // Only a successful install starts the pause, so a failed one is tried again at the next key.
    if installed.is_some() {
        LAST_REINSTALL_TICK.with(|cell| cell.set(Some(key_tick)));
        SERIAL_AT_REINSTALL.with(|cell| cell.set(serial_now));
        WATCHDOG_REINSTALLS.fetch_add(1, Ordering::Relaxed);
        pass_on_diagnostic(HookDiagnostic::Reinstalled);
    }
    INSTALLED_HOOK.with(|cell| cell.set(installed));
    // A key-up that arrived while the hook was gone never reached the flow, and keys that
    // waited for an echo that was lost with the hook are typed now.
    STATE.with(|cell| {
        if let Ok(mut guard) = cell.try_borrow_mut()
            && let Some(state) = guard.as_mut()
        {
            state.flow.forget_held_keys(&mut SendInputSink);
        }
    });
}

fn install_hook() -> Option<HHOOK> {
    let module = unsafe { GetModuleHandleW(None) }.ok()?;
    unsafe { SetWindowsHookExW(WH_KEYBOARD_LL, Some(keyboard_hook), Some(module.into()), 0) }.ok()
}

#[cfg(test)]
#[path = "hook_headless.rs"]
mod hook_headless;

#[cfg(test)]
mod tests {
    use super::*;

    fn chord_after(downs: &[u32], up: u32) -> bool {
        let mut tracker = ToggleChordTracker::default();
        for virtual_key in downs {
            tracker.key_down(*virtual_key);
        }
        tracker.key_up(up)
    }

    #[test]
    fn ctrl_then_shift_released_completes_the_chord() {
        assert!(chord_after(&[VK_LCONTROL, VK_LSHIFT], VK_LSHIFT));
        assert!(chord_after(&[VK_LSHIFT, VK_LCONTROL], VK_LCONTROL));
    }

    #[test]
    fn another_key_or_a_single_modifier_breaks_the_chord() {
        let letter_between = [VK_LCONTROL, u32::from(b'C'), VK_LSHIFT];
        assert!(!chord_after(&letter_between, VK_LSHIFT));
        assert!(!chord_after(&[VK_LSHIFT], VK_LSHIFT));
    }

    #[test]
    fn alt_z_is_recognised_only_when_configured_and_alone() {
        let z = u32::from(b'Z');
        assert!(is_alt_z_press(ToggleHotkey::AltZ, z, true, false));
        assert!(!is_alt_z_press(ToggleHotkey::CtrlShift, z, true, false));
        assert!(!is_alt_z_press(ToggleHotkey::AltZ, z, true, true));
        assert!(!is_alt_z_press(ToggleHotkey::AltZ, z, false, false));
    }

    fn telex_session() -> Session {
        Session::new(Config::default())
    }

    fn press_letter(character: char) -> KeyInput {
        classify_key(&KeyPress {
            virtual_key: u32::from(character.to_ascii_uppercase()),
            character: Some(character),
            ..KeyPress::default()
        })
    }

    fn type_text(session: &mut Session, text: &str) -> Vec<Verdict> {
        text.chars()
            .map(|character| session.handle(press_letter(character)))
            .collect()
    }

    /// A text field that obeys the verdicts: what the user would see after typing `keys`.
    fn screen_after_typing(keys: &str) -> String {
        let mut session = telex_session();
        let mut screen = String::new();
        for character in keys.chars() {
            let (edit, key_reaches_application) = match session.handle(press_letter(character)) {
                Verdict::PassThrough => (TextEdit::default(), true),
                Verdict::EditThenPass(edit) => (edit, true),
                Verdict::Swallow(edit) => (edit, false),
            };
            for _ in 0..edit.backspaces {
                screen.pop();
            }
            screen.push_str(&edit.text);
            if key_reaches_application {
                screen.push(character);
            }
        }
        screen
    }

    #[test]
    fn english_words_come_back_unchanged() {
        for words in ["text window coffee ", "window ", "windows ", "text "] {
            assert_eq!(screen_after_typing(words), words);
        }
    }

    #[test]
    fn a_circumflex_replaces_the_last_letter_with_one_backspace() {
        let edit = plan_edit("vie", "viê");
        assert_eq!(edit.backspaces, 1);
        assert_eq!(edit.text, "ê");
    }

    #[test]
    fn a_letter_added_to_the_word_needs_no_backspace() {
        let edit = plan_edit("Việ", "Việt");
        assert_eq!(edit.backspaces, 0);
        assert_eq!(edit.text, "t");
    }

    #[test]
    fn an_identical_word_needs_no_edit() {
        assert!(plan_edit("việt", "việt").is_empty());
    }

    #[test]
    fn a_tone_moving_to_another_vowel_rewrites_from_the_first_difference() {
        let edit = plan_edit("hoà", "hóa");
        assert_eq!(edit.backspaces, 2);
        assert_eq!(edit.text, "óa");
    }

    #[test]
    fn strokes_are_backspaces_then_utf16_units() {
        let strokes = strokes_for(&TextEdit {
            backspaces: 2,
            text: "ệ".to_owned(),
        });
        assert_eq!(
            strokes,
            [Stroke::Backspace, Stroke::Backspace, Stroke::Unit(0x1EC7)]
        );
    }

    #[test]
    fn plain_letters_pass_through_until_a_mark_key_changes_one() {
        let mut session = telex_session();
        let verdicts = type_text(&mut session, "viee");
        assert!(
            verdicts[..3]
                .iter()
                .all(|verdict| *verdict == Verdict::PassThrough)
        );
        assert_eq!(
            verdicts[3],
            Verdict::Swallow(TextEdit {
                backspaces: 1,
                text: "ê".to_owned()
            })
        );
        assert_eq!(session.sent(), "viê");
    }

    #[test]
    fn a_tone_key_rewrites_only_the_tail() {
        let mut session = telex_session();
        type_text(&mut session, "viee");
        let verdict = session.handle(press_letter('j'));
        assert_eq!(
            verdict,
            Verdict::Swallow(TextEdit {
                backspaces: 1,
                text: "ệ".to_owned()
            })
        );
        assert_eq!(type_text(&mut session, "t"), [Verdict::PassThrough]);
        assert_eq!(session.sent(), "việt");
    }

    #[test]
    fn a_space_ends_the_word_and_clears_the_record() {
        let mut session = telex_session();
        type_text(&mut session, "vieetj");
        assert_eq!(session.sent(), "việt");
        assert_eq!(type_text(&mut session, " "), [Verdict::PassThrough]);
        assert_eq!(session.sent(), "");
    }

    #[test]
    fn backspace_is_done_by_the_hook_so_a_suggestion_cannot_take_it() {
        let mut session = telex_session();
        type_text(&mut session, "vieet");
        let verdict = session.handle(KeyInput::Backspace);
        assert_eq!(
            verdict,
            Verdict::Swallow(TextEdit {
                backspaces: 1,
                text: String::new()
            })
        );
        assert_eq!(session.sent(), "viê");
    }

    #[test]
    fn deleting_edits_start_with_the_selection_guard() {
        let strokes = strokes_for_sending(&TextEdit {
            backspaces: 1,
            text: "ê".to_owned(),
        });
        assert_eq!(
            strokes,
            [
                Stroke::Unit(ZERO_WIDTH_SPACE),
                Stroke::Backspace,
                Stroke::Backspace,
                Stroke::Unit(0x00EA)
            ]
        );
        let only_text = TextEdit {
            backspaces: 0,
            text: "t".to_owned(),
        };
        assert_eq!(strokes_for_sending(&only_text), strokes_for(&only_text));
    }

    #[test]
    fn a_chord_after_typed_letters_still_counts() {
        let mut tracker = ToggleChordTracker::default();
        tracker.key_down(u32::from(b'V'));
        tracker.key_down(u32::from(b' '));
        tracker.key_down(VK_LCONTROL);
        tracker.key_down(VK_LSHIFT);
        assert!(tracker.key_up(VK_LSHIFT));
    }

    #[test]
    fn backspace_with_no_word_passes_through() {
        let mut session = telex_session();
        assert_eq!(session.handle(KeyInput::Backspace), Verdict::PassThrough);
    }

    #[test]
    fn enter_finishes_an_unchanged_word_without_sending_anything() {
        let mut session = telex_session();
        type_text(&mut session, "vieet");
        assert_eq!(session.handle(KeyInput::FinishWord), Verdict::PassThrough);
        assert_eq!(session.sent(), "");
    }

    #[test]
    fn a_shortcut_forgets_the_word_and_leaves_the_text() {
        let mut session = telex_session();
        type_text(&mut session, "vieet");
        assert_eq!(session.handle(KeyInput::AbandonWord), Verdict::PassThrough);
        assert_eq!(session.sent(), "");
    }

    #[test]
    fn ctrl_alt_and_windows_chords_abandon_the_word() {
        for press in [
            KeyPress {
                virtual_key: 0x43,
                character: Some('c'),
                ctrl: true,
                ..KeyPress::default()
            },
            KeyPress {
                virtual_key: 0x44,
                character: Some('d'),
                alt: true,
                ..KeyPress::default()
            },
            KeyPress {
                virtual_key: 0x45,
                character: Some('e'),
                windows_key: true,
                ..KeyPress::default()
            },
        ] {
            assert_eq!(classify_key(&press), KeyInput::AbandonWord);
        }
    }

    #[test]
    fn alt_gr_still_types_its_character() {
        let press = KeyPress {
            virtual_key: 0x45,
            character: Some('€'),
            ctrl: true,
            alt: true,
            alt_gr: true,
            ..KeyPress::default()
        };
        assert_eq!(classify_key(&press), KeyInput::Character('€'));
    }

    #[test]
    fn modifiers_and_packets_are_ignored_and_navigation_finishes_the_word() {
        let key = |virtual_key| KeyPress {
            virtual_key,
            ..KeyPress::default()
        };
        assert_eq!(classify_key(&key(VK_LSHIFT)), KeyInput::Ignore);
        assert_eq!(classify_key(&key(VK_PACKET)), KeyInput::Ignore);
        assert_eq!(classify_key(&key(0x25)), KeyInput::FinishWord);
        assert_eq!(classify_key(&key(VK_RETURN)), KeyInput::FinishWord);
        assert_eq!(classify_key(&key(VK_BACK)), KeyInput::Backspace);
    }

    #[test]
    fn the_hook_ignores_its_own_output_and_foreign_injected_keys() {
        assert!(should_ignore_event(true, OWN_OUTPUT_MARKER, true));
        assert!(should_ignore_event(false, OWN_OUTPUT_MARKER, false));
        assert!(should_ignore_event(true, 0, false));
        assert!(!should_ignore_event(true, 0, true));
        assert!(!should_ignore_event(false, 0, false));
    }

    #[test]
    fn the_hook_is_active_in_vietnamese_mode_except_in_excluded_apps() {
        let excluded = vec!["game.exe".to_owned()];
        assert!(hook_is_active(true, "notepad.exe", &excluded));
        assert!(hook_is_active(true, "code.exe", &[]));
        assert!(!hook_is_active(false, "notepad.exe", &excluded));
        assert!(!hook_is_active(true, "game.exe", &excluded));
        assert!(!hook_is_active(true, "", &excluded));
    }

    fn private_shared_state(label: &str) -> SharedState {
        let name = format!("Local\\oh-key-provider-test-{label}-{}", std::process::id());
        SharedState::open_named(&name).expect("create mapping")
    }

    fn settings_excluding(program: &str, macro_enabled: bool) -> Settings {
        let mut settings = Settings {
            excluded_apps: vec![program.to_owned()],
            ..Settings::default()
        };
        settings.engine.macro_enabled = macro_enabled;
        settings
    }

    #[test]
    fn the_provider_answers_only_when_the_settings_or_the_mode_changed() {
        let shared = private_shared_state("changes");
        let writer = private_shared_state("changes");
        writer.publish_settings(&settings_excluding("game.exe", false));
        let mut provider = app_inputs_provider(shared);

        let first = provider().expect("the first call returns the inputs");
        assert_eq!(first.excluded_apps, vec!["game.exe".to_owned()]);
        assert_eq!(first.macros, Some(Vec::new()));
        assert!(provider().is_none());

        writer.publish_settings(&settings_excluding("editor.exe", false));
        let after_publish = provider().expect("new settings are picked up by the next key");
        assert_eq!(after_publish.excluded_apps, vec!["editor.exe".to_owned()]);
        assert_eq!(after_publish.macros, Some(Vec::new()));
        assert!(provider().is_none());

        writer.set_vietnamese(false);
        let after_flip = provider().expect("a mode flip is picked up by the next key");
        assert!(!after_flip.vietnamese);
        assert_eq!(after_flip.macros, None);
        assert_eq!(after_flip.excluded_apps, vec!["editor.exe".to_owned()]);
        assert!(provider().is_none());
    }

    #[test]
    fn the_macro_file_is_read_off_the_key_path() {
        let shared = private_shared_state("macros");
        let writer = private_shared_state("macros");
        writer.publish_settings(&settings_excluding("game.exe", true));
        let mut provider = app_inputs_provider(shared);

        let first = provider().expect("the first call returns the inputs");
        assert_eq!(first.macros, None, "the file is still being read");
        let started = std::time::Instant::now();
        loop {
            if let Some(inputs) = provider() {
                assert!(inputs.macros.is_some());
                break;
            }
            assert!(started.elapsed().as_secs() < 5, "the macros never arrived");
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert!(provider().is_none());
    }
}
