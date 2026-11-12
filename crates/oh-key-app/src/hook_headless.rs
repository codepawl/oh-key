//! Replays every case of the hook suite without a window, a focus or a real keyboard.
//!
//! The suite (`examples/hook_suite.rs`) types through the real hook into real EDIT controls.
//! Here the same case table (`hook_cases.rs`) goes through `KeyFlow`, the very type that
//! `HookState::handle_event` drives: the Ctrl+Shift chord, the Alt+Z press, `hook_is_active`,
//! `classify_key`, `Session` and the swallowed key. The output lands in a simulated text field
//! that edits like an EDIT control: a backspace deletes the selection when there is one, a typed
//! character replaces it, and the address bar's inline autocomplete selects a suggestion after the
//! typed text, either at each `{WAIT}` in a script or after every key (a suggestion can land at
//! any moment, so every address bar case runs in both timings).
//!
//! What stays CI-only (the `hook-e2e` job): the Win32 glue (`SetWindowsHookEx`, `SendInput`
//! timing, `ToUnicodeEx` with the real layout and Caps Lock state, foreground switches), the real
//! RICHEDIT and the real shell autocomplete object. Caps Lock and the autocomplete selection are
//! modelled here; the real thing is not.

use std::collections::VecDeque;

use super::*;

#[path = "hook_cases.rs"]
mod hook_cases;

use hook_cases::{Case, Control, all_cases};

const SUITE_EXE: &str = "hook_suite.exe";
const VK_SPACE: u32 = 0x20;
const VK_COMMA: u32 = 0xBC;
const VK_PERIOD: u32 = 0xBE;

const SUGGESTIONS: [&str; 7] = [
    "vietnam.example.com",
    "viet-nam.example.com",
    "text.example.com",
    "coffee.example.com",
    "dien.example.com",
    "vid.example.com",
    "view.example.com",
];

/// A text field that edits the way an EDIT control does, with the caret at the end.
struct SimulatedField {
    control: Control,
    units: Vec<u16>,
    /// Where the selected inline suggestion starts; it runs to the end of the text.
    selection_start: Option<usize>,
}

impl SimulatedField {
    fn new(control: Control) -> Self {
        SimulatedField {
            control,
            units: Vec::new(),
            selection_start: None,
        }
    }

    fn delete_selection(&mut self) -> bool {
        match self.selection_start.take() {
            Some(start) => {
                self.units.truncate(start);
                true
            }
            None => false,
        }
    }

    fn insert_unit(&mut self, unit: u16) {
        self.delete_selection();
        self.units.push(unit);
    }

    fn backspace(&mut self) {
        if !self.delete_selection() {
            self.units.pop();
        }
    }

    fn enter(&mut self) {
        if self.control == Control::Rich {
            self.insert_unit(u16::from(b'\r'));
            self.insert_unit(u16::from(b'\n'));
        }
    }

    fn apply_stroke(&mut self, stroke: Stroke) {
        match stroke {
            Stroke::Backspace => self.backspace(),
            Stroke::Unit(unit) => self.insert_unit(unit),
            Stroke::Key(virtual_key) if u32::from(virtual_key) == VK_RETURN => self.enter(),
            Stroke::Key(_) => {}
        }
    }

    /// The suggestion lands a moment after typing: the rest of the first matching entry is
    /// appended and selected.
    fn let_suggestion_arrive(&mut self) {
        if self.control != Control::AddressBar || self.selection_start.is_some() {
            return;
        }
        let typed = String::from_utf16_lossy(&self.units).to_lowercase();
        if typed.is_empty() {
            return;
        }
        let Some(suggestion) = SUGGESTIONS
            .iter()
            .find(|suggestion| suggestion.starts_with(&typed))
        else {
            return;
        };
        self.selection_start = Some(self.units.len());
        self.units.extend(suggestion[typed.len()..].encode_utf16());
    }

    /// What `hook_suite` reads back: the whole text, or for the address bar what is before the
    /// selected suggestion.
    fn visible_text(&self) -> String {
        let end = match (self.control, self.selection_start) {
            (Control::AddressBar, Some(start)) => start,
            _ => self.units.len(),
        };
        String::from_utf16_lossy(&self.units[..end])
    }
}

/// When the address bar's suggestion arrives.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum SuggestionTiming {
    /// Only at a `{WAIT}` in the key script.
    AtWait,
    /// After every key, the worst case for the hook.
    AfterEveryKey,
}

/// One entry of the simulated input queue, which both the keyboard and our own `SendInput`
/// batches join at the back.
#[derive(Clone, Copy, Debug)]
enum QueuedEvent {
    KeyDown(u32),
    KeyUp(u32),
    /// A key event of our own edit; it applies to the field when it reaches the front.
    Own {
        stroke: Stroke,
        release: bool,
    },
}

/// What the hook's output does to the field: the edit goes in as keystrokes, in one batch.
/// Without a queue the batch lands at once and its events come back through the hook at once;
/// with one it lands behind whatever the queue already holds, like on a loaded machine.
struct SimulatedOutput {
    field: SimulatedField,
    use_selection_guard: bool,
    queue: Option<VecDeque<QueuedEvent>>,
    events_to_echo: usize,
    /// `SendInput` takes at most this many events of a batch (blocked input).
    send_limit: Option<usize>,
    now_milliseconds: u64,
}

impl SimulatedOutput {
    fn new(control: Control) -> Self {
        SimulatedOutput {
            field: SimulatedField::new(control),
            use_selection_guard: true,
            queue: None,
            events_to_echo: 0,
            send_limit: None,
            now_milliseconds: 0,
        }
    }
}

impl SimulatedOutput {
    /// One key event of ours: it joins the queue, or lands at once when there is no queue.
    /// Returns false when `SendInput` would refuse it.
    fn emit(&mut self, stroke: Stroke, release: bool, accepted_so_far: usize) -> bool {
        if self
            .send_limit
            .is_some_and(|limit| accepted_so_far >= limit)
        {
            return false;
        }
        match self.queue.as_mut() {
            Some(queue) => queue.push_back(QueuedEvent::Own { stroke, release }),
            None => {
                if !release {
                    self.field.apply_stroke(stroke);
                }
                self.events_to_echo += 1;
            }
        }
        true
    }
}

impl OutputSink for SimulatedOutput {
    fn send_edit(&mut self, edit: &TextEdit) -> usize {
        let strokes = if self.use_selection_guard {
            strokes_for_sending(edit)
        } else {
            strokes_for(edit)
        };
        let mut accepted = 0;
        for stroke in strokes {
            for release in [false, true] {
                if !self.emit(stroke, release, accepted) {
                    return accepted;
                }
                accepted += 1;
            }
        }
        if self.use_selection_guard {
            return accepted;
        }
        // Without the guard fewer events go out, but the flow counts the guarded batch.
        let reported = edit_event_count(edit);
        self.events_to_echo += reported - accepted;
        reported
    }

    fn cancel_menu_activation(&mut self) {}

    fn send_key(&mut self, virtual_key: u32) -> usize {
        let stroke = Stroke::Key(virtual_key as u16);
        let mut accepted = 0;
        for release in [false, true] {
            if !self.emit(stroke, release, accepted) {
                return accepted;
            }
            accepted += 1;
        }
        accepted
    }

    fn now_milliseconds(&self) -> u64 {
        self.now_milliseconds
    }
}

/// The real `KeyFlow` with the operating system replaced by plain fields.
struct SimulatedHook {
    flow: KeyFlow,
    output: SimulatedOutput,
    timing: SuggestionTiming,
    ctrl_down: bool,
    shift_down: bool,
    alt_down: bool,
    caps_lock_on: bool,
    toggles: usize,
}

impl SimulatedHook {
    fn new(control: Control, toggle_hotkey: ToggleHotkey, excluded_apps: Vec<String>) -> Self {
        let mut flow = KeyFlow::new(Config::default(), toggle_hotkey);
        flow.set_mode_and_apps(true, excluded_apps, toggle_hotkey);
        flow.set_foreground_exe(SUITE_EXE.to_owned());
        SimulatedHook {
            flow,
            output: SimulatedOutput::new(control),
            timing: SuggestionTiming::AtWait,
            ctrl_down: false,
            shift_down: false,
            alt_down: false,
            caps_lock_on: false,
            toggles: 0,
        }
    }

    fn character_for_layout(&self, virtual_key: u32) -> Option<char> {
        let letter = u8::try_from(virtual_key)
            .ok()
            .filter(|byte| byte.is_ascii_uppercase());
        if let Some(byte) = letter {
            let upper = self.shift_down != self.caps_lock_on;
            let character = char::from(byte);
            return Some(if upper {
                character
            } else {
                character.to_ascii_lowercase()
            });
        }
        match virtual_key {
            VK_SPACE => Some(' '),
            VK_COMMA => Some(','),
            VK_PERIOD => Some('.'),
            0x30..=0x39 => char::from_u32(virtual_key),
            _ => None,
        }
    }

    fn key_press(&self, virtual_key: u32) -> KeyPress {
        let shortcut_held = self.ctrl_down || self.alt_down;
        let character = if shortcut_held {
            None
        } else {
            self.character_for_layout(virtual_key)
        };
        KeyPress {
            virtual_key,
            character,
            ctrl: self.ctrl_down,
            alt: self.alt_down,
            ..KeyPress::default()
        }
    }

    /// What the application does with a key the hook let through.
    fn deliver_to_application(&mut self, press: &KeyPress) {
        let field = &mut self.output.field;
        if let Some(character) = press.character {
            let mut units = [0u16; 2];
            for unit in character.encode_utf16(&mut units) {
                field.insert_unit(*unit);
            }
            return;
        }
        match press.virtual_key {
            VK_BACK => field.backspace(),
            VK_RETURN => field.enter(),
            _ => {}
        }
    }

    /// What the hook decides for a key press, without the application's share of it.
    fn decide_key_down(&mut self, virtual_key: u32) -> (KeyOutcome, KeyPress) {
        let press = self.key_press(virtual_key);
        let outcome = self.flow.key_down(
            virtual_key,
            self.alt_down,
            self.ctrl_down,
            || press,
            &mut self.output,
        );
        self.count_toggle(outcome);
        self.remember_modifier(virtual_key, true);
        (outcome, press)
    }

    fn key_down(&mut self, virtual_key: u32) {
        let (outcome, press) = self.decide_key_down(virtual_key);
        if !outcome.swallow {
            self.deliver_to_application(&press);
        }
        self.settle_echoes();
    }

    fn key_up(&mut self, virtual_key: u32) {
        let outcome = self.flow.key_up(virtual_key);
        self.count_toggle(outcome);
        self.remember_modifier(virtual_key, false);
        self.settle_echoes();
    }

    /// Without a queue, our own events come back through the hook right after they were sent.
    fn settle_echoes(&mut self) {
        while self.output.events_to_echo > 0 {
            self.output.events_to_echo -= 1;
            self.flow.on_own_echo(&mut self.output);
        }
    }

    fn count_toggle(&mut self, outcome: KeyOutcome) {
        if outcome.toggled {
            self.toggles += 1;
        }
    }

    fn remember_modifier(&mut self, virtual_key: u32, down: bool) {
        match virtual_key {
            VK_CONTROL => self.ctrl_down = down,
            VK_SHIFT => self.shift_down = down,
            VK_MENU => self.alt_down = down,
            _ => {}
        }
    }

    /// Presses the keys in order and releases them in reverse, like `press_chord` in the suite.
    fn press_chord(&mut self, virtual_keys: &[u32]) {
        for virtual_key in virtual_keys {
            self.key_down(*virtual_key);
        }
        for virtual_key in virtual_keys.iter().rev() {
            self.key_up(*virtual_key);
        }
        if self.timing == SuggestionTiming::AfterEveryKey {
            self.output.field.let_suggestion_arrive();
        }
    }

    fn type_character(&mut self, character: char) {
        let virtual_key = virtual_key_for(character);
        if character.is_ascii_uppercase() {
            self.press_chord(&[VK_SHIFT, virtual_key]);
        } else {
            self.press_chord(&[virtual_key]);
        }
    }

    fn press_caps_lock(&mut self) {
        self.press_chord(&[VK_CAPITAL]);
        self.caps_lock_on = !self.caps_lock_on;
    }

    fn run_script(&mut self, script: &str) {
        let mut characters = script.chars();
        while let Some(character) = characters.next() {
            if character != '{' {
                self.type_character(character);
                continue;
            }
            let name: String = characters
                .by_ref()
                .take_while(|next| *next != '}')
                .collect();
            self.run_token(&name);
        }
    }

    fn run_token(&mut self, name: &str) {
        match name {
            "BS" => self.press_chord(&[VK_BACK]),
            "ENTER" => self.press_chord(&[VK_RETURN]),
            "TAB" => self.press_chord(&[VK_TAB]),
            "ESC" => self.press_chord(&[VK_ESCAPE]),
            "CAPS" => self.press_caps_lock(),
            "CTRLSHIFT" => self.press_chord(&[VK_CONTROL, VK_SHIFT]),
            "ALTZ" => self.press_chord(&[VK_MENU, VK_Z]),
            "WAIT" => self.output.field.let_suggestion_arrive(),
            other => panic!("unknown key script token {other}"),
        }
    }
}

fn virtual_key_for(character: char) -> u32 {
    match character {
        ' ' => VK_SPACE,
        ',' => VK_COMMA,
        '.' => VK_PERIOD,
        '0'..='9' => u32::from(character),
        letter if letter.is_ascii_alphabetic() => u32::from(letter.to_ascii_uppercase()),
        other => panic!("no virtual key known for {other:?}"),
    }
}

fn hotkey_from_code(code: u8) -> ToggleHotkey {
    match code {
        0 => ToggleHotkey::CtrlShift,
        1 => ToggleHotkey::AltZ,
        _ => ToggleHotkey::Disabled,
    }
}

/// What the suite would read back after the case: the mode for `mode:` cases, else the text.
fn outcome_of(case: &Case, hook: &SimulatedHook) -> String {
    if case.expected.starts_with("mode:") {
        let mode = if hook.flow.is_vietnamese() {
            "vietnamese"
        } else {
            "english"
        };
        return format!("mode:{mode}");
    }
    hook.output.field.visible_text()
}

fn replay_case(case: &Case, timing: SuggestionTiming) -> String {
    let mut hook = SimulatedHook::new(case.control, hotkey_from_code(case.hotkey), Vec::new());
    hook.timing = timing;
    hook.run_script(case.keys);
    outcome_of(case, &hook)
}

fn failures_with(timing: SuggestionTiming) -> Vec<String> {
    all_cases()
        .iter()
        .filter_map(|case| {
            let actual = replay_case(case, timing);
            (actual != case.expected).then(|| {
                format!(
                    "{} ({}, {:?}): expected {:?}, got {:?}",
                    case.name,
                    case.control.label(),
                    timing,
                    case.expected,
                    actual
                )
            })
        })
        .collect()
}

#[test]
fn every_hook_suite_case_passes_without_a_window() {
    let case_count = all_cases().len();
    assert!(case_count >= 34, "the case table shrank to {case_count}");
    let failures = failures_with(SuggestionTiming::AtWait);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn every_case_passes_when_the_suggestion_lands_after_any_key() {
    let failures = failures_with(SuggestionTiming::AfterEveryKey);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn the_simulated_address_bar_shows_why_the_selection_guard_exists() {
    let case = all_cases()
        .into_iter()
        .find(|case| case.name == "addr-suggestion-then-circumflex")
        .expect("case present");
    let mut hook = SimulatedHook::new(case.control, hotkey_from_code(case.hotkey), Vec::new());
    hook.output.use_selection_guard = false;
    hook.run_script(case.keys);
    // The first backspace only deletes the selected suggestion, so the old letter stays.
    assert_eq!(hook.output.field.visible_text(), "vieê");
}

#[test]
fn an_excluded_program_gets_its_keys_untouched() {
    let mut hook = SimulatedHook::new(
        Control::Plain,
        ToggleHotkey::CtrlShift,
        vec![SUITE_EXE.to_owned()],
    );
    hook.run_script("vieetj ");
    assert_eq!(hook.output.field.visible_text(), "vieetj ");
}

#[test]
fn the_hotkey_does_nothing_when_it_is_disabled() {
    let mut hook = SimulatedHook::new(Control::Plain, ToggleHotkey::Disabled, Vec::new());
    hook.run_script("{CTRLSHIFT}{ALTZ}vieetj ");
    assert_eq!(hook.toggles, 0);
    assert_eq!(hook.output.field.visible_text(), "việt ");
}

#[test]
fn a_swallowed_press_swallows_its_release_and_a_toggle_is_reported_once() {
    let mut flow = KeyFlow::new(Config::default(), ToggleHotkey::AltZ);
    flow.set_mode_and_apps(true, Vec::new(), ToggleHotkey::AltZ);
    flow.set_foreground_exe(SUITE_EXE.to_owned());
    let mut output = SimulatedOutput::new(Control::Plain);
    let press = flow.key_down(VK_Z, true, false, KeyPress::default, &mut output);
    assert_eq!(
        press,
        KeyOutcome {
            swallow: true,
            toggled: true
        }
    );
    assert!(!flow.is_vietnamese());
    let release = flow.key_up(VK_Z);
    assert_eq!(
        release,
        KeyOutcome {
            swallow: true,
            toggled: false
        }
    );
}

// ---------------------------------------------------------------------------------------
// A loaded machine: queued keys, late callbacks, blocked input
// ---------------------------------------------------------------------------------------

/// How long Windows lets a hook callback run before it passes the key on without waiting.
const SIMULATED_HOOK_TIMEOUT_MILLISECONDS: u64 = 300;

/// The simulated operating system: one input queue that the keyboard and our `SendInput`
/// batches share, the hook called for each entry in order, and a clock that a slow callback moves.
struct LagSystem {
    hook: SimulatedHook,
    /// A key whose callback takes `milliseconds` longer than usual.
    slow_key: Option<(u32, u64)>,
}

impl LagSystem {
    fn new(control: Control, mitigations: Mitigations) -> Self {
        let mut hook = SimulatedHook::new(control, ToggleHotkey::CtrlShift, Vec::new());
        hook.flow.set_mitigations(mitigations);
        hook.output.queue = Some(VecDeque::new());
        LagSystem {
            hook,
            slow_key: None,
        }
    }

    fn queue(&mut self) -> &mut VecDeque<QueuedEvent> {
        self.hook
            .output
            .queue
            .as_mut()
            .expect("the lag system has a queue")
    }

    fn press_keys(&mut self, virtual_keys: &[u32]) {
        for virtual_key in virtual_keys {
            self.queue().push_back(QueuedEvent::KeyDown(*virtual_key));
        }
        for virtual_key in virtual_keys.iter().rev() {
            self.queue().push_back(QueuedEvent::KeyUp(*virtual_key));
        }
    }

    /// The whole script reaches the queue before the hook has handled the first key.
    fn type_in_one_burst(&mut self, script: &str) {
        let mut characters = script.chars();
        while let Some(character) = characters.next() {
            if character != '{' {
                self.press_character(character);
                continue;
            }
            let name: String = characters
                .by_ref()
                .take_while(|next| *next != '}')
                .collect();
            match name.as_str() {
                "BS" => self.press_keys(&[VK_BACK]),
                "ENTER" => self.press_keys(&[VK_RETURN]),
                "TAB" => self.press_keys(&[VK_TAB]),
                other => panic!("the lag system does not model {other}"),
            }
        }
    }

    fn press_character(&mut self, character: char) {
        let virtual_key = virtual_key_for(character);
        if character.is_ascii_uppercase() {
            self.press_keys(&[VK_SHIFT, virtual_key]);
        } else {
            self.press_keys(&[virtual_key]);
        }
    }

    fn run_until_idle(&mut self) {
        while let Some(event) = self.queue().pop_front() {
            self.handle(event);
        }
    }

    fn handle(&mut self, event: QueuedEvent) {
        let now = self.hook.output.now_milliseconds;
        self.hook.flow.begin_event(now);
        match event {
            QueuedEvent::KeyDown(virtual_key) => self.handle_key_down(virtual_key),
            QueuedEvent::KeyUp(virtual_key) => self.hook.key_up_without_settling(virtual_key),
            QueuedEvent::Own { stroke, release } => {
                self.hook.flow.on_own_echo(&mut self.hook.output);
                if !release {
                    self.hook.output.field.apply_stroke(stroke);
                }
            }
        }
    }

    fn handle_key_down(&mut self, virtual_key: u32) {
        let extra_milliseconds = match self.slow_key {
            Some((slow, milliseconds)) if slow == virtual_key => milliseconds,
            _ => 0,
        };
        self.hook.output.now_milliseconds += extra_milliseconds;
        let (outcome, press) = self.hook.decide_key_down(virtual_key);
        let timed_out = extra_milliseconds > SIMULATED_HOOK_TIMEOUT_MILLISECONDS;
        // Past the timeout the system passes the key on whatever the hook answered.
        if !outcome.swallow || timed_out {
            self.hook.deliver_to_application(&press);
        }
    }

    fn visible_text(&self) -> String {
        self.hook.output.field.visible_text()
    }
}

impl SimulatedHook {
    fn key_up_without_settling(&mut self, virtual_key: u32) {
        let outcome = self.flow.key_up(virtual_key);
        self.count_toggle(outcome);
        self.remember_modifier(virtual_key, false);
    }
}

/// The cases a burst can reproduce: plain typing in a plain field, no hotkeys, no autocomplete.
fn burst_cases() -> Vec<Case> {
    let supported = ["{BS}", "{ENTER}", "{TAB}"];
    all_cases()
        .into_iter()
        .filter(|case| case.control == Control::Plain && !case.expected.starts_with("mode:"))
        .filter(|case| {
            let mut rest = case.keys;
            while let Some(start) = rest.find('{') {
                let Some(length) = rest[start..].find('}') else {
                    return false;
                };
                if !supported.contains(&&rest[start..start + length + 1]) {
                    return false;
                }
                rest = &rest[start + length + 1..];
            }
            true
        })
        .collect()
}

fn burst_failures(mitigations: Mitigations) -> Vec<String> {
    burst_cases()
        .iter()
        .filter_map(|case| {
            let mut system = LagSystem::new(case.control, mitigations);
            system.type_in_one_burst(case.keys);
            system.run_until_idle();
            let actual = system.visible_text();
            (actual != case.expected).then(|| {
                format!(
                    "{}: expected {:?}, got {:?}",
                    case.name, case.expected, actual
                )
            })
        })
        .collect()
}

#[test]
fn the_burst_covers_a_useful_part_of_the_case_table() {
    let count = burst_cases().len();
    assert!(count >= 10, "only {count} cases");
}

#[test]
fn legacy_keys_queued_behind_an_edit_garble_the_text() {
    // The symptom behind repeated letters and missing tones: with no gating, a key that was
    // already waiting reaches the application before the edit of the key before it.
    let failures = burst_failures(Mitigations::none());
    assert!(
        failures.len() >= 5,
        "the old logic should visibly fail under a burst, failed only: {failures:?}"
    );
}

#[test]
fn fixed_keys_queued_behind_an_edit_wait_for_it() {
    let failures = burst_failures(echo_gated());
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn legacy_a_callback_past_the_timeout_garbles_the_word() {
    // Only the budget is off, so the result shows the late callback alone.
    let mitigations = Mitigations {
        callback_budget_milliseconds: None,
        ..Mitigations::default()
    };
    let mut system = LagSystem::new(Control::Plain, mitigations);
    system.slow_key = Some((u32::from(b'S'), SIMULATED_HOOK_TIMEOUT_MILLISECONDS + 50));
    system.type_in_one_burst("viets");
    system.run_until_idle();
    // The system passed the raw "s" on at once, and the edit that was meant to replace it lands
    // afterwards: its backspaces eat the "s" and the "t" that were already there.
    assert_eq!(system.visible_text(), "vieét");
}

#[test]
fn fixed_a_late_callback_sends_nothing_and_the_key_stands() {
    let mut system = LagSystem::new(Control::Plain, Mitigations::default());
    system.slow_key = Some((u32::from(b'S'), SIMULATED_HOOK_TIMEOUT_MILLISECONDS + 50));
    system.type_in_one_burst("viets");
    system.run_until_idle();
    assert_eq!(system.visible_text(), "viets");
    assert_eq!(system.hook.flow.late_edits(), 1);
}

#[test]
fn a_blocked_send_input_leaves_the_key_as_typed_and_the_word_restarts() {
    let mut hook = SimulatedHook::new(Control::Plain, ToggleHotkey::CtrlShift, Vec::new());
    hook.output.send_limit = Some(0);
    hook.run_script("viets");
    // The tone edit could not go out, so the "s" must not be swallowed.
    assert_eq!(hook.output.field.visible_text(), "viets");
    hook.output.send_limit = None;
    hook.run_script(" viets");
    assert_eq!(hook.output.field.visible_text(), "viets viét");
}

#[test]
fn a_partial_send_input_does_not_leave_the_engine_ahead_of_the_screen() {
    let mut hook = SimulatedHook::new(Control::Plain, ToggleHotkey::CtrlShift, Vec::new());
    hook.run_script("viet");
    hook.output.send_limit = Some(2);
    hook.run_script("s");
    // After the short count the word starts over, so the next letter is typed as it is.
    hook.output.send_limit = None;
    hook.run_script("a");
    let text = hook.output.field.visible_text();
    assert!(text.ends_with('a'), "{text:?}");
}

/// Records what the flow sends and lets a test move the clock.
#[derive(Default)]
struct RecordingSink {
    edits: Vec<TextEdit>,
    keys: Vec<u32>,
    now_milliseconds: u64,
}

impl OutputSink for RecordingSink {
    fn send_edit(&mut self, edit: &TextEdit) -> usize {
        self.edits.push(edit.clone());
        edit_event_count(edit)
    }

    fn send_key(&mut self, virtual_key: u32) -> usize {
        self.keys.push(virtual_key);
        2
    }

    fn cancel_menu_activation(&mut self) {}

    fn now_milliseconds(&self) -> u64 {
        self.now_milliseconds
    }
}

fn active_flow() -> KeyFlow {
    let mut flow = KeyFlow::new(Config::default(), ToggleHotkey::Disabled);
    flow.set_mode_and_apps(true, Vec::new(), ToggleHotkey::Disabled);
    flow.set_foreground_exe(SUITE_EXE.to_owned());
    flow
}

/// Echo gating is off by default (it held keys forever on real Windows); its own tests turn it on.
fn echo_gated() -> Mitigations {
    Mitigations {
        echo_gating: true,
        ..Mitigations::default()
    }
}

fn echo_gated_flow() -> KeyFlow {
    let mut flow = active_flow();
    flow.set_mitigations(echo_gated());
    flow
}

fn press_letter_on(flow: &mut KeyFlow, sink: &mut RecordingSink, letter: char) -> KeyOutcome {
    let press = KeyPress {
        virtual_key: u32::from(letter.to_ascii_uppercase()),
        character: Some(letter),
        ..KeyPress::default()
    };
    flow.key_down(press.virtual_key, false, false, || press, sink)
}

fn type_until_the_tone_edit(flow: &mut KeyFlow, sink: &mut RecordingSink) {
    for letter in "viet".chars() {
        press_letter_on(flow, sink, letter);
    }
    // The tone key is swallowed and its edit is now in flight.
    assert!(press_letter_on(flow, sink, 's').swallow);
    assert_eq!(sink.edits.len(), 1);
}

#[test]
fn a_key_typed_while_an_edit_is_in_flight_waits_and_goes_out_after_the_echo() {
    let mut flow = echo_gated_flow();
    let mut sink = RecordingSink::default();
    type_until_the_tone_edit(&mut flow, &mut sink);
    sink.now_milliseconds = 10;
    let outcome = press_letter_on(&mut flow, &mut sink, 'j');
    assert!(
        outcome.swallow,
        "the key must wait, not reach the application early"
    );
    assert_eq!(sink.edits.len(), 1);
    let events = edit_event_count(&sink.edits[0]);
    for _ in 0..events {
        flow.on_own_echo(&mut sink);
    }
    assert_eq!(
        sink.edits.len(),
        2,
        "the waiting key goes out once the edit has landed"
    );
    assert!(
        flow.key_up(u32::from(b'J')).swallow,
        "its release is swallowed too"
    );
}

#[test]
fn a_key_after_the_echo_deadline_is_not_held_forever() {
    let mut flow = active_flow();
    let mut sink = RecordingSink::default();
    type_until_the_tone_edit(&mut flow, &mut sink);
    // The echo never comes (a lost event): the key is handled now, not held back for it.
    sink.now_milliseconds = 500;
    flow.begin_event(500);
    press_letter_on(&mut flow, &mut sink, 'j');
    assert_eq!(sink.edits.len(), 2, "the key was held back for a lost echo");
    assert!(sink.keys.is_empty());
}

#[test]
fn enter_while_an_edit_is_in_flight_ends_the_wait_and_keeps_the_waiting_letters() {
    let mut flow = echo_gated_flow();
    let mut sink = RecordingSink::default();
    type_until_the_tone_edit(&mut flow, &mut sink);
    sink.now_milliseconds = 10;
    assert!(press_letter_on(&mut flow, &mut sink, 'j').swallow);
    let enter = KeyPress {
        virtual_key: VK_RETURN,
        ..KeyPress::default()
    };
    let outcome = flow.key_down(VK_RETURN, false, false, || enter, &mut sink);
    assert!(
        outcome.swallow,
        "Enter must not overtake the waiting letter"
    );
    let events = edit_event_count(&sink.edits[0]);
    for _ in 0..events {
        flow.on_own_echo(&mut sink);
    }
    // The letter goes through the engine first, then Enter is pressed again; its release is
    // swallowed because the application never saw the press.
    assert!(sink.edits.len() >= 2, "the waiting letter was lost");
    assert_eq!(sink.keys, vec![VK_RETURN]);
    assert!(flow.key_up(VK_RETURN).swallow);
}

fn press_shortcut_c(flow: &mut KeyFlow, sink: &mut RecordingSink) -> KeyOutcome {
    let shortcut = KeyPress {
        virtual_key: u32::from(b'C'),
        ctrl: true,
        ..KeyPress::default()
    };
    flow.key_down(u32::from(b'C'), false, true, || shortcut, sink)
}

fn land_the_first_edit(flow: &mut KeyFlow, sink: &mut RecordingSink) {
    let events = edit_event_count(&sink.edits[0]);
    for _ in 0..events {
        flow.on_own_echo(sink);
    }
}

#[test]
fn a_shortcut_while_an_edit_is_in_flight_waits_behind_the_waiting_letters() {
    let mut flow = echo_gated_flow();
    let mut sink = RecordingSink::default();
    type_until_the_tone_edit(&mut flow, &mut sink);
    sink.now_milliseconds = 10;
    assert!(press_letter_on(&mut flow, &mut sink, 'j').swallow);
    let outcome = press_shortcut_c(&mut flow, &mut sink);
    assert!(
        outcome.swallow,
        "the shortcut must not overtake the waiting letter"
    );
    assert!(sink.keys.is_empty());
    land_the_first_edit(&mut flow, &mut sink);
    assert_eq!(sink.edits.len(), 2, "the waiting letter was lost");
    assert_eq!(sink.keys, vec![u32::from(b'C')]);
}

#[test]
fn a_waiting_key_that_the_system_passed_on_is_not_typed_again() {
    let mut flow = echo_gated_flow();
    let mut sink = RecordingSink::default();
    type_until_the_tone_edit(&mut flow, &mut sink);
    sink.now_milliseconds = 10;
    assert!(press_letter_on(&mut flow, &mut sink, 'j').swallow);
    flow.release_swallow(u32::from(b'J'));
    land_the_first_edit(&mut flow, &mut sink);
    assert_eq!(sink.edits.len(), 1, "the passed-on key was typed twice");
}

#[test]
fn a_window_switch_drops_the_keys_that_waited_for_the_old_window() {
    let mut flow = active_flow();
    let mut sink = RecordingSink::default();
    type_until_the_tone_edit(&mut flow, &mut sink);
    sink.now_milliseconds = 10;
    assert!(press_letter_on(&mut flow, &mut sink, 'j').swallow);
    flow.deactivate();
    assert!(flow.deferred_keys.is_empty());
    assert_eq!(flow.echoes_outstanding, 0);
}

#[test]
fn held_key_autorepeat_never_grows_the_waiting_list_past_its_capacity() {
    let mut flow = active_flow();
    let mut sink = RecordingSink::default();
    type_until_the_tone_edit(&mut flow, &mut sink);
    sink.now_milliseconds = 10;
    for _ in 0..(KEY_BUFFER_CAPACITY * 3) {
        press_letter_on(&mut flow, &mut sink, 'j');
        assert!(flow.deferred_keys.len() <= KEY_BUFFER_CAPACITY);
    }
}

#[test]
fn the_menu_cancel_keys_are_ignored_but_are_not_the_echo_of_an_edit() {
    assert!(should_ignore_event(false, OWN_MENU_CANCEL_MARKER, false));
    assert_ne!(OWN_MENU_CANCEL_MARKER, OWN_OUTPUT_MARKER);
}

#[test]
fn the_late_swallow_cutoff_follows_the_hook_timeout() {
    assert_eq!(late_swallow_cutoff_for_timeout(300), 250);
    assert_eq!(late_swallow_cutoff_for_timeout(100), 83);
    assert!(late_swallow_cutoff_for_timeout(100) > budget_for_timeout(100));
}

#[test]
fn the_event_count_matches_what_send_input_gets() {
    let edits = [
        TextEdit::default(),
        TextEdit {
            backspaces: 0,
            text: "a".to_owned(),
        },
        TextEdit {
            backspaces: 3,
            text: "việt".to_owned(),
        },
    ];
    for edit in &edits {
        let expected = if edit.is_empty() {
            0
        } else {
            strokes_for_sending(edit).len() * 2
        };
        assert_eq!(edit_event_count(edit), expected);
    }
}

#[test]
fn the_budget_is_half_the_system_timeout_within_limits() {
    assert_eq!(budget_for_timeout(300), 150);
    assert_eq!(budget_for_timeout(200), 100);
    assert_eq!(budget_for_timeout(1000), 150);
    assert_eq!(budget_for_timeout(10), 20);
}

#[test]
fn a_call_past_the_timeout_is_noticed() {
    // The callback itself ran too long.
    assert!(call_overran(1_000, 1_400, 350, 300));
    // The callback was quick, but it started long after Windows stamped the key.
    assert!(call_overran(1_000, 1_320, 5, 300));
    assert!(!call_overran(1_000, 1_100, 5, 300));
    // The tick counter wraps around after 49 days.
    assert!(!call_overran(u32::MAX - 50, 50, 5, 300));
    assert!(call_overran(u32::MAX - 50, 400, 5, 300));
    // A key with no time stamp only counts the callback's own duration.
    assert!(!call_overran(0, 900_000, 5, 300));
    assert!(call_overran(0, 900_000, 300, 300));
}

#[test]
fn an_injected_key_without_the_marker_is_still_ignored() {
    assert!(should_ignore_event(true, 0, false));
    assert!(should_ignore_event(false, OWN_OUTPUT_MARKER, false));
    assert!(!should_ignore_event(false, 0, false));
}

// ---------------------------------------------------------------------------------------
// Endless loops and stuck keys (owner report: "uôn" typed again and again until oh-key was killed)
// ---------------------------------------------------------------------------------------

/// What a loop run came to: how many events the system handled, whether it stopped on its own,
/// and how many times the circuit breaker fired.
struct LoopRun {
    events_handled: usize,
    ended_by_itself: bool,
    breaker_trips: usize,
}

const LOOP_EVENT_CAP: usize = 20_000;

/// A key that seems stuck: whenever the hook answers a key with output, the system delivers
/// that same key once more. Without a guard the hook feeds itself for as long as it edits.
fn run_with_a_feedback_key(mitigations: Mitigations) -> LoopRun {
    let mut system = LagSystem::new(Control::Plain, mitigations);
    system.type_in_one_burst("ua");
    system.run_until_idle();
    // Tone f and tone s replace each other, so every answer asks for the next.
    let feedback_keys = [virtual_key_for('s'), virtual_key_for('f')];
    let mut events_handled = 0;
    let mut feedback_count = 0;
    let mut ended_by_itself = false;
    while events_handled < LOOP_EVENT_CAP {
        system.press_keys(&[feedback_keys[feedback_count % 2]]);
        feedback_count += 1;
        // One round of the loop takes a millisecond: the hook answers as fast as it can.
        system.hook.output.now_milliseconds += 1;
        let mut produced_output = false;
        while let Some(event) = system.queue().pop_front() {
            let queued_before = system.queue().len();
            system.handle(event);
            events_handled += 1;
            produced_output |= system.queue().len() > queued_before;
        }
        if !produced_output {
            ended_by_itself = true;
            break;
        }
    }
    LoopRun {
        events_handled,
        ended_by_itself,
        breaker_trips: system.hook.flow.breaker_trips(),
    }
}

#[test]
fn legacy_a_feedback_key_makes_the_hook_edit_without_end() {
    let run = run_with_a_feedback_key(Mitigations::none());
    assert!(
        !run.ended_by_itself,
        "the old logic stopped after {} events",
        run.events_handled
    );
    assert!(run.events_handled >= LOOP_EVENT_CAP);
    assert_eq!(run.breaker_trips, 0);
}

#[test]
fn fixed_the_circuit_breaker_ends_a_feedback_loop() {
    let run = run_with_a_feedback_key(Mitigations::default());
    assert!(
        run.ended_by_itself,
        "still looping after {} events, {} trips",
        run.events_handled, run.breaker_trips
    );
    assert!(run.breaker_trips >= 1);
    assert!(run.events_handled < LOOP_EVENT_CAP / 10);
}

fn flow_without_gating() -> (KeyFlow, SimulatedOutput) {
    let mut flow = KeyFlow::new(Config::default(), ToggleHotkey::CtrlShift);
    flow.set_mode_and_apps(true, Vec::new(), ToggleHotkey::CtrlShift);
    flow.set_foreground_exe(SUITE_EXE.to_owned());
    flow.set_mitigations(Mitigations {
        echo_gating: false,
        ..Mitigations::default()
    });
    (flow, SimulatedOutput::new(Control::Plain))
}

fn press_of(character: char) -> KeyPress {
    KeyPress {
        virtual_key: u32::from(character.to_ascii_uppercase()),
        character: Some(character),
        ..KeyPress::default()
    }
}

#[test]
fn a_release_is_swallowed_only_when_its_press_was() {
    let (mut flow, mut output) = flow_without_gating();
    let virtual_key = u32::from(b'E');
    let first = flow.key_down(virtual_key, false, false, || press_of('e'), &mut output);
    assert!(!first.swallow);
    assert!(!flow.key_up(virtual_key).swallow);
    let second = flow.key_down(virtual_key, false, false, || press_of('e'), &mut output);
    assert!(second.swallow, "the circumflex edit replaces the second e");
    assert!(flow.key_up(virtual_key).swallow);
}

#[test]
fn a_press_that_passes_clears_a_stale_pending_release() {
    let (mut flow, mut output) = flow_without_gating();
    let virtual_key = u32::from(b'E');
    for _ in 0..2 {
        flow.key_down(virtual_key, false, false, || press_of('e'), &mut output);
    }
    // The release of that swallowed press never reached the flow (the hook was away).
    flow.session.reset();
    let fresh = flow.key_down(virtual_key, false, false, || press_of('e'), &mut output);
    assert!(!fresh.swallow);
    assert!(
        !flow.key_up(virtual_key).swallow,
        "the application saw this press, so it sees the release"
    );
}

#[test]
fn a_swallow_that_windows_ignored_does_not_swallow_the_release() {
    let (mut flow, mut output) = flow_without_gating();
    let virtual_key = u32::from(b'E');
    flow.key_down(virtual_key, false, false, || press_of('e'), &mut output);
    flow.key_up(virtual_key);
    let swallowed = flow.key_down(virtual_key, false, false, || press_of('e'), &mut output);
    assert!(swallowed.swallow);
    flow.release_swallow(virtual_key);
    assert!(!flow.key_up(virtual_key).swallow);
}
