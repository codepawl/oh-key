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

/// What the hook's output does to the field: the edit goes in as keystrokes, in one batch.
struct SimulatedOutput {
    field: SimulatedField,
    use_selection_guard: bool,
}

impl OutputSink for SimulatedOutput {
    fn send_edit(&mut self, edit: &TextEdit) {
        let strokes = if self.use_selection_guard {
            strokes_for_sending(edit)
        } else {
            strokes_for(edit)
        };
        for stroke in strokes {
            self.field.apply_stroke(stroke);
        }
    }

    fn cancel_menu_activation(&mut self) {}
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
            output: SimulatedOutput {
                field: SimulatedField::new(control),
                use_selection_guard: true,
            },
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

    fn key_down(&mut self, virtual_key: u32) {
        let press = self.key_press(virtual_key);
        let outcome = self.flow.key_down(
            virtual_key,
            self.alt_down,
            self.ctrl_down,
            || press,
            &mut self.output,
        );
        self.count_toggle(outcome);
        if !outcome.swallow {
            self.deliver_to_application(&press);
        }
        self.remember_modifier(virtual_key, true);
    }

    fn key_up(&mut self, virtual_key: u32) {
        let outcome = self.flow.key_up(virtual_key);
        self.count_toggle(outcome);
        self.remember_modifier(virtual_key, false);
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
    let mut output = SimulatedOutput {
        field: SimulatedField::new(Control::Plain),
        use_selection_guard: true,
    };
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
