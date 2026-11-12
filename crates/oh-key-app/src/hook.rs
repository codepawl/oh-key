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

use std::cell::RefCell;
use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering};
use std::sync::mpsc;
use std::thread::JoinHandle;

use oh_key_engine::{Config, Engine, KeyResult};
use oh_key_ipc::{Settings, SharedState, ToggleHotkey, load_settings, macros_path};
use oh_key_macro::MacroTable;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::{
    GetCurrentThreadId, OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
    QueryFullProcessImageNameW,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, GetKeyState, GetKeyboardLayout, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT,
    KEYEVENTF_KEYUP, KEYEVENTF_UNICODE, SendInput, ToUnicodeEx, VIRTUAL_KEY,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, DispatchMessageW, GetForegroundWindow, GetMessageW, GetWindowThreadProcessId,
    HHOOK, KBDLLHOOKSTRUCT, MSG, PM_NOREMOVE, PeekMessageW, PostThreadMessageW, SetTimer,
    SetWindowsHookExW, TranslateMessage, UnhookWindowsHookEx, WH_KEYBOARD_LL, WM_KEYDOWN, WM_KEYUP,
    WM_QUIT, WM_SYSKEYDOWN, WM_SYSKEYUP, WM_USER,
};
use windows::core::{Owned, PWSTR};

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
const LLKHF_INJECTED: u32 = 0x10;
const REFRESH_MILLISECONDS: u32 = 50;

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
    extra_info == OWN_OUTPUT_MARKER || (injected && !accept_foreign_injected)
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

// ---------------------------------------------------------------------------------------
// Windows glue
// ---------------------------------------------------------------------------------------

/// What the hook thread asks the app for every few milliseconds.
pub struct HookInputs {
    /// Lower case executable names.
    pub excluded_apps: Vec<String>,
    pub vietnamese: bool,
    pub engine: Config,
    pub toggle_hotkey: ToggleHotkey,
    /// The macros, only when they changed since the last call.
    pub macros: Option<Vec<(String, String)>>,
}

pub type InputsProvider = Box<dyn FnMut() -> HookInputs + Send>;

/// Flips the global Vietnamese/English mode when the hotkey is pressed.
pub type ToggleAction = Box<dyn FnMut() + Send>;

#[derive(Clone, Copy, Debug, Default)]
pub struct HookOptions {
    /// Also handle keys that other programs injected. Off in the app; the end-to-end
    /// example turns it on because `SendInput` is how it types.
    pub accept_foreign_injected_keys: bool,
}

/// The app's source: published settings, the mode in the shared state and the macro file.
pub fn app_inputs_provider(shared: SharedState) -> InputsProvider {
    let mut seen_generation = None;
    let mut settings = Settings::default();
    Box::new(move || {
        let generation = shared.generation();
        let mut macros = None;
        if seen_generation != Some(generation) {
            seen_generation = Some(generation);
            settings = shared.read_settings().unwrap_or_else(load_settings);
            macros = Some(read_macros(settings.engine.macro_enabled));
        }
        HookInputs {
            excluded_apps: settings.excluded_apps.clone(),
            vietnamese: shared.vietnamese(),
            engine: settings.engine,
            toggle_hotkey: settings.toggle_hotkey,
            macros,
        }
    })
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

struct HookState {
    provider: InputsProvider,
    toggle_action: ToggleAction,
    options: HookOptions,
    session: Session,
    config: Config,
    excluded_apps: Vec<String>,
    toggle_hotkey: ToggleHotkey,
    chord: ToggleChordTracker,
    vietnamese: bool,
    foreground: HWND,
    foreground_exe: String,
    active: bool,
    /// The key whose press was swallowed, so its release is swallowed too.
    swallowed_key: Option<u32>,
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
        let inputs = provider();
        let mut state = HookState {
            provider,
            toggle_action,
            options,
            session: Session::new(inputs.engine),
            config: inputs.engine,
            excluded_apps: Vec::new(),
            toggle_hotkey: inputs.toggle_hotkey,
            chord: ToggleChordTracker::default(),
            vietnamese: false,
            foreground: HWND::default(),
            foreground_exe: String::new(),
            active: false,
            swallowed_key: None,
        };
        state.apply(inputs);
        state
    }

    /// Runs on a timer, never inside the key callback: it reads files and asks for process names.
    fn refresh(&mut self) {
        let inputs = (self.provider)();
        self.apply(inputs);
    }

    fn apply(&mut self, inputs: HookInputs) {
        if inputs.engine != self.config {
            self.config = inputs.engine;
            self.session.set_config(inputs.engine);
        }
        if let Some(macros) = inputs.macros {
            self.session.set_macros(&macros);
        }
        self.excluded_apps = inputs.excluded_apps;
        self.toggle_hotkey = inputs.toggle_hotkey;
        self.vietnamese = inputs.vietnamese;
        let foreground = unsafe { GetForegroundWindow() };
        if foreground != self.foreground {
            FOREGROUND_CHANGES.fetch_add(1, Ordering::Relaxed);
            self.foreground = foreground;
            self.foreground_exe = window_exe_name(foreground).unwrap_or_default();
        }
        self.refresh_active();
    }

    fn refresh_active(&mut self) {
        let active = hook_is_active(self.vietnamese, &self.foreground_exe, &self.excluded_apps);
        if active != self.active {
            self.active = active;
            self.session.reset();
        }
    }

    fn deactivate(&mut self) {
        FOREGROUND_CHANGES.fetch_add(1, Ordering::Relaxed);
        self.active = false;
        self.swallowed_key = None;
        self.chord = ToggleChordTracker::default();
        self.session.reset();
    }

    /// Takes effect at once; the next refresh reads the same mode back from the shared state.
    fn toggle_mode(&mut self) {
        (self.toggle_action)();
        self.vietnamese = !self.vietnamese;
        self.refresh_active();
    }

    /// Returns whether the press is the swallowed Alt+Z; the Ctrl+Shift chord completes on release.
    fn handle_toggle_press(&mut self, virtual_key: u32) -> bool {
        self.chord.key_down(virtual_key);
        let alt = key_is_down(VK_MENU);
        let ctrl = key_is_down(VK_CONTROL);
        if !is_alt_z_press(self.toggle_hotkey, virtual_key, alt, ctrl) {
            return false;
        }
        self.toggle_mode();
        self.swallowed_key = Some(virtual_key);
        cancel_menu_activation();
        true
    }

    fn handle_toggle_release(&mut self, virtual_key: u32) {
        let completed = self.chord.key_up(virtual_key);
        if completed && self.toggle_hotkey == ToggleHotkey::CtrlShift {
            self.toggle_mode();
        }
    }

    /// Returns whether the event is swallowed.
    fn handle_event(&mut self, message: u32, event: &KBDLLHOOKSTRUCT) -> bool {
        let injected = event.flags.0 & LLKHF_INJECTED != 0;
        if should_ignore_event(
            injected,
            event.dwExtraInfo,
            self.options.accept_foreign_injected_keys,
        ) {
            return false;
        }
        if unsafe { GetForegroundWindow() } != self.foreground {
            // The timer will resolve the new window; until then keys go through untouched.
            self.deactivate();
            return false;
        }
        if !injected {
            PHYSICAL_KEYS.fetch_add(1, Ordering::Relaxed);
        }
        let virtual_key = event.vkCode;
        if matches!(message, WM_KEYUP | WM_SYSKEYUP) {
            let was_swallowed = self.swallowed_key.take() == Some(virtual_key);
            self.handle_toggle_release(virtual_key);
            return was_swallowed;
        }
        if !matches!(message, WM_KEYDOWN | WM_SYSKEYDOWN) {
            return false;
        }
        if self.handle_toggle_press(virtual_key) {
            return true;
        }
        if !self.active {
            return false;
        }
        let press = read_key_press(event);
        match self.session.handle(classify_key(&press)) {
            Verdict::PassThrough => false,
            Verdict::EditThenPass(edit) => {
                send_edit(&edit);
                false
            }
            Verdict::Swallow(edit) => {
                send_edit(&edit);
                self.swallowed_key = Some(virtual_key);
                true
            }
        }
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

fn keyboard_event(stroke: Stroke, release: bool) -> INPUT {
    let release_flag = if release {
        KEYEVENTF_KEYUP
    } else {
        Default::default()
    };
    let (virtual_key, scan_code, flags) = match stroke {
        Stroke::Backspace => (VIRTUAL_KEY(VK_BACK as u16), 0, release_flag),
        Stroke::Unit(unit) => (VIRTUAL_KEY(0), unit, KEYEVENTF_UNICODE | release_flag),
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
    unsafe { SendInput(&[press, release], size_of::<INPUT>() as i32) };
}

/// The whole edit in one `SendInput` call, so nothing the user types lands in the middle.
fn send_edit(edit: &TextEdit) {
    let mut events = Vec::with_capacity((edit.backspaces + edit.text.len() + 2) * 2);
    for stroke in strokes_for_sending(edit) {
        events.push(keyboard_event(stroke, false));
        events.push(keyboard_event(stroke, true));
    }
    if events.is_empty() {
        return;
    }
    unsafe { SendInput(&events, size_of::<INPUT>() as i32) };
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

unsafe extern "system" fn keyboard_hook(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    const HC_ACTION: i32 = 0;
    if code == HC_ACTION {
        let started = std::time::Instant::now();
        let event = unsafe { &*(lparam.0 as *const KBDLLHOOKSTRUCT) };
        let swallowed = STATE.with(|cell| match cell.try_borrow_mut() {
            Ok(mut guard) => guard
                .as_mut()
                .is_some_and(|state| state.handle_event(wparam.0 as u32, event)),
            Err(_) => false,
        });
        record_callback_time(started.elapsed());
        if swallowed {
            return LRESULT(1);
        }
    }
    unsafe { CallNextHookEx(None, code, wparam, lparam) }
}

unsafe extern "system" fn refresh_timer(_window: HWND, _message: u32, _id: usize, _time: u32) {
    STATE.with(|cell| {
        if let Ok(mut guard) = cell.try_borrow_mut()
            && let Some(state) = guard.as_mut()
        {
            state.refresh();
        }
    });
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

fn run_hook_thread(
    provider: InputsProvider,
    toggle_action: ToggleAction,
    options: HookOptions,
    ready: mpsc::Sender<Option<u32>>,
) {
    STATE.with(|cell| *cell.borrow_mut() = Some(HookState::new(provider, toggle_action, options)));
    let Some(hook) = install_hook() else {
        let _ = ready.send(None);
        return;
    };
    let mut message = MSG::default();
    // Creates this thread's message queue, so a quit posted right away is not lost.
    let _ = unsafe { PeekMessageW(&mut message, None, WM_USER, WM_USER, PM_NOREMOVE) };
    unsafe { SetTimer(None, 0, REFRESH_MILLISECONDS, Some(refresh_timer)) };
    let _ = ready.send(Some(unsafe { GetCurrentThreadId() }));
    while unsafe { GetMessageW(&mut message, None, 0, 0) }.0 > 0 {
        unsafe {
            let _ = TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    }
    let _ = unsafe { UnhookWindowsHookEx(hook) };
    STATE.with(|cell| *cell.borrow_mut() = None);
}

fn install_hook() -> Option<HHOOK> {
    let module = unsafe { GetModuleHandleW(None) }.ok()?;
    unsafe { SetWindowsHookExW(WH_KEYBOARD_LL, Some(keyboard_hook), Some(module.into()), 0) }.ok()
}

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
}
