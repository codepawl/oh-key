//! What the settings window shows: the two mode buttons, two cards of rows (each a label and a
//! dropdown, a chip pair or a switch) and three tool buttons, and what a choice does to the
//! settings. No drawing here, so it is all unit tested.

use oh_key_engine::{InputMethod, ToneStyle};
use oh_key_ipc::{Settings, ToggleHotkey, UpdateMode};

use crate::icons;

pub const INPUT_METHOD_CHOICES: [(InputMethod, &str); 4] = [
    (InputMethod::Telex, "Telex"),
    (InputMethod::Vni, "VNI"),
    (InputMethod::Viqr, "VIQR"),
    (InputMethod::SimpleTelex, "Telex đơn giản"),
];
/// The new style puts the mark on the second vowel of "oa", "oe" and "uy", the old on the first.
const TONE_STYLE_CHOICES: [(ToneStyle, &str); 2] =
    [(ToneStyle::Modern, "hoà"), (ToneStyle::Classic, "hòa")];
const HOTKEY_CHOICES: [(ToggleHotkey, &str); 3] = [
    (ToggleHotkey::CtrlShift, "Ctrl + Shift"),
    (ToggleHotkey::AltZ, "Alt + Z"),
    (ToggleHotkey::Disabled, "Tắt"),
];
const UPDATE_CHOICES: [(UpdateMode, &str); 3] = [
    (UpdateMode::Auto, "Tự động"),
    (UpdateMode::Notify, "Chỉ báo"),
    (UpdateMode::Off, "Tắt"),
];

/// A switch's two options, in the order `apply_choice` reads them.
pub const SWITCH_OFF: usize = 0;
pub const SWITCH_ON: usize = 1;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Field {
    InputMethod,
    ToggleHotkey,
    ToneStyle,
    FreeTonePlacement,
    SpellCheck,
    RestoreInvalidWord,
    MacroEnabled,
    StartVietnamese,
    StartAtSignIn,
    Updates,
    SendReports,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tool {
    Macros,
    ExcludedApps,
    Converter,
}

/// What the window does after a click.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Effect {
    Nothing,
    Save,
    /// Saved, and the Windows start-up entry has to follow the new value.
    SaveAndApplyStartup,
}

/// One of the two big buttons at the top; clicking one sets the global mode.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ModeButton {
    pub vietnamese: bool,
    pub tile: &'static str,
    pub label: &'static str,
    pub active: bool,
}

pub fn mode_buttons(vietnamese: bool) -> [ModeButton; 2] {
    [
        ModeButton {
            vietnamese: true,
            tile: "V",
            label: "Tiếng Việt",
            active: vietnamese,
        },
        ModeButton {
            vietnamese: false,
            tile: "E",
            label: "English",
            active: !vietnamese,
        },
    ]
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Control {
    /// One value picked from a popup list.
    Dropdown {
        options: Vec<String>,
        selected: usize,
    },
    /// A few values side by side, one of them chosen.
    Chips {
        options: Vec<String>,
        selected: usize,
    },
    Switch {
        on: bool,
    },
}

impl Control {
    /// The option that is chosen now; a switch is option `SWITCH_ON` when on.
    pub fn selected(&self) -> usize {
        match self {
            Control::Dropdown { selected, .. } | Control::Chips { selected, .. } => *selected,
            Control::Switch { on } => {
                if *on {
                    SWITCH_ON
                } else {
                    SWITCH_OFF
                }
            }
        }
    }

    pub fn option_count(&self) -> usize {
        match self {
            Control::Dropdown { options, .. } | Control::Chips { options, .. } => options.len(),
            Control::Switch { .. } => 2,
        }
    }

    /// The option Space or Enter moves to: the other side of a switch, the next chip.
    pub fn next_option(&self) -> usize {
        (self.selected() + 1) % self.option_count().max(1)
    }

    /// The option an arrow key moves to, or None at the end of the list.
    pub fn stepped_option(&self, forward: bool) -> Option<usize> {
        let selected = self.selected();
        if forward {
            (selected + 1 < self.option_count()).then_some(selected + 1)
        } else {
            selected.checked_sub(1)
        }
    }
}

pub struct Row {
    pub field: Field,
    pub label: &'static str,
    pub control: Control,
}

pub struct ActionButton {
    pub tool: Tool,
    pub icon: char,
    pub label: &'static str,
}

fn labels(choices: impl Iterator<Item = &'static str>) -> Vec<String> {
    choices.map(str::to_string).collect()
}

fn dropdown_row(field: Field, label: &'static str, options: Vec<String>, selected: usize) -> Row {
    Row {
        field,
        label,
        control: Control::Dropdown { options, selected },
    }
}

fn switch_row(field: Field, label: &'static str, on: bool) -> Row {
    Row {
        field,
        label,
        control: Control::Switch { on },
    }
}

fn choice_index<Value: PartialEq + Copy>(choices: &[(Value, &str)], value: Value) -> usize {
    choices
        .iter()
        .position(|(choice, _)| *choice == value)
        .unwrap_or(0)
}

fn typing_card(settings: &Settings) -> Vec<Row> {
    let engine = &settings.engine;
    vec![
        dropdown_row(
            Field::InputMethod,
            "Bảng gõ",
            labels(INPUT_METHOD_CHOICES.iter().map(|(_, label)| *label)),
            choice_index(&INPUT_METHOD_CHOICES, engine.input_method),
        ),
        dropdown_row(
            Field::ToggleHotkey,
            "Phím chuyển",
            labels(HOTKEY_CHOICES.iter().map(|(_, label)| *label)),
            choice_index(&HOTKEY_CHOICES, settings.toggle_hotkey),
        ),
        Row {
            field: Field::ToneStyle,
            label: "Đặt dấu",
            control: Control::Chips {
                options: labels(TONE_STYLE_CHOICES.iter().map(|(_, label)| *label)),
                selected: choice_index(&TONE_STYLE_CHOICES, engine.tone_style),
            },
        },
    ]
}

fn switches_card(settings: &Settings) -> Vec<Row> {
    let engine = &settings.engine;
    vec![
        switch_row(
            Field::FreeTonePlacement,
            "Bỏ dấu tự do",
            engine.free_tone_placement,
        ),
        switch_row(Field::SpellCheck, "Kiểm tra chính tả", engine.spell_check),
        switch_row(
            Field::RestoreInvalidWord,
            "Giữ nguyên từ tiếng Anh",
            engine.restore_invalid_word,
        ),
        switch_row(Field::MacroEnabled, "Gõ tắt", engine.macro_enabled),
        switch_row(
            Field::StartVietnamese,
            "Bắt đầu ở tiếng Việt",
            settings.start_vietnamese,
        ),
        switch_row(
            Field::StartAtSignIn,
            "Khởi động cùng Windows",
            settings.start_at_sign_in,
        ),
    ]
}

fn system_card(settings: &Settings) -> Vec<Row> {
    vec![
        dropdown_row(
            Field::Updates,
            "Cập nhật",
            labels(UPDATE_CHOICES.iter().map(|(_, label)| *label)),
            choice_index(&UPDATE_CHOICES, settings.updates),
        ),
        switch_row(
            Field::SendReports,
            "Gửi báo lỗi tự động",
            settings.send_reports,
        ),
    ]
}

/// The three cards in reading order, built from the current settings.
pub fn cards(settings: &Settings) -> [Vec<Row>; 3] {
    [
        typing_card(settings),
        switches_card(settings),
        system_card(settings),
    ]
}

pub fn card_row_counts(cards: &[Vec<Row>]) -> Vec<usize> {
    cards.iter().map(Vec::len).collect()
}

pub fn actions() -> [ActionButton; 3] {
    [
        ActionButton {
            tool: Tool::Macros,
            icon: icons::LIST,
            label: "Gõ tắt",
        },
        ActionButton {
            tool: Tool::ExcludedApps,
            icon: icons::WINDOW,
            label: "Ứng dụng",
        },
        ActionButton {
            tool: Tool::Converter,
            icon: icons::SYNC,
            label: "Chuyển mã",
        },
    ]
}

/// Applies option `option` of `field`; for a switch, `SWITCH_ON` or `SWITCH_OFF`.
pub fn apply_choice(settings: &mut Settings, field: Field, option: usize) -> Effect {
    let enabled = option == SWITCH_ON;
    match field {
        Field::InputMethod => match INPUT_METHOD_CHOICES.get(option) {
            Some((method, _)) => settings.engine.input_method = *method,
            None => return Effect::Nothing,
        },
        Field::ToneStyle => match TONE_STYLE_CHOICES.get(option) {
            Some((style, _)) => settings.engine.tone_style = *style,
            None => return Effect::Nothing,
        },
        Field::ToggleHotkey => match HOTKEY_CHOICES.get(option) {
            Some((hotkey, _)) => settings.toggle_hotkey = *hotkey,
            None => return Effect::Nothing,
        },
        Field::FreeTonePlacement => settings.engine.free_tone_placement = enabled,
        Field::SpellCheck => settings.engine.spell_check = enabled,
        Field::RestoreInvalidWord => settings.engine.restore_invalid_word = enabled,
        Field::MacroEnabled => settings.engine.macro_enabled = enabled,
        Field::StartVietnamese => settings.start_vietnamese = enabled,
        Field::StartAtSignIn => {
            settings.start_at_sign_in = enabled;
            return Effect::SaveAndApplyStartup;
        }
        Field::Updates => match UPDATE_CHOICES.get(option) {
            Some((mode, _)) => settings.updates = *mode,
            None => return Effect::Nothing,
        },
        Field::SendReports => settings.send_reports = enabled,
    }
    Effect::Save
}

#[cfg(test)]
mod tests {
    use super::*;

    fn all_rows(settings: &Settings) -> Vec<Row> {
        cards(settings).into_iter().flatten().collect()
    }

    fn find_row(settings: &Settings, field: Field) -> Row {
        all_rows(settings)
            .into_iter()
            .find(|row| row.field == field)
            .unwrap()
    }

    #[test]
    fn every_row_shows_the_option_that_was_just_chosen() {
        for field in all_rows(&Settings::default()).iter().map(|row| row.field) {
            let option_count = find_row(&Settings::default(), field).control.option_count();
            for option in 0..option_count {
                let mut settings = Settings::default();
                apply_choice(&mut settings, field, option);
                let row = find_row(&settings, field);
                assert_eq!(row.control.selected(), option, "{field:?} option {option}");
            }
        }
    }

    #[test]
    fn every_boolean_setting_has_a_switch() {
        let switch_fields: Vec<Field> = all_rows(&Settings::default())
            .iter()
            .filter(|row| matches!(row.control, Control::Switch { .. }))
            .map(|row| row.field)
            .collect();
        for field in [
            Field::FreeTonePlacement,
            Field::SpellCheck,
            Field::RestoreInvalidWord,
            Field::MacroEnabled,
            Field::StartVietnamese,
            Field::StartAtSignIn,
            Field::SendReports,
        ] {
            assert!(switch_fields.contains(&field), "{field:?}");
        }
        assert_eq!(switch_fields.len(), 7);
    }

    #[test]
    fn the_input_method_dropdown_lists_every_method() {
        let row = find_row(&Settings::default(), Field::InputMethod);
        let Control::Dropdown { options, selected } = row.control else {
            panic!("the input method is a dropdown");
        };
        assert_eq!(options, ["Telex", "VNI", "VIQR", "Telex đơn giản"]);
        assert_eq!(selected, 0);
    }

    #[test]
    fn choosing_the_input_method_changes_the_engine_config() {
        let mut settings = Settings::default();
        assert_eq!(
            apply_choice(&mut settings, Field::InputMethod, 1),
            Effect::Save
        );
        assert_eq!(settings.engine.input_method, InputMethod::Vni);
        apply_choice(&mut settings, Field::InputMethod, 3);
        assert_eq!(settings.engine.input_method, InputMethod::SimpleTelex);
    }

    #[test]
    fn the_two_chips_are_the_new_and_the_old_tone_style() {
        let row = find_row(&Settings::default(), Field::ToneStyle);
        let Control::Chips { options, selected } = row.control else {
            panic!("the tone style is a chip pair");
        };
        assert_eq!(options, ["hoà", "hòa"]);
        assert_eq!(selected, 0);
        let mut settings = Settings::default();
        apply_choice(&mut settings, Field::ToneStyle, 1);
        assert_eq!(settings.engine.tone_style, ToneStyle::Classic);
    }

    #[test]
    fn a_switch_is_on_for_the_second_option_and_off_for_the_first() {
        let mut settings = Settings::default();
        apply_choice(&mut settings, Field::SpellCheck, SWITCH_OFF);
        assert!(!settings.engine.spell_check);
        apply_choice(&mut settings, Field::SpellCheck, SWITCH_ON);
        assert!(settings.engine.spell_check);
    }

    #[test]
    fn the_hotkey_dropdown_reaches_all_three_values() {
        let mut settings = Settings::default();
        apply_choice(&mut settings, Field::ToggleHotkey, 1);
        assert_eq!(settings.toggle_hotkey, ToggleHotkey::AltZ);
        apply_choice(&mut settings, Field::ToggleHotkey, 2);
        assert_eq!(settings.toggle_hotkey, ToggleHotkey::Disabled);
        apply_choice(&mut settings, Field::ToggleHotkey, 0);
        assert_eq!(settings.toggle_hotkey, ToggleHotkey::CtrlShift);
    }

    #[test]
    fn start_at_sign_in_asks_for_the_startup_entry_to_follow() {
        let mut settings = Settings::default();
        assert_eq!(
            apply_choice(&mut settings, Field::StartAtSignIn, SWITCH_ON),
            Effect::SaveAndApplyStartup
        );
        assert!(settings.start_at_sign_in);
    }

    #[test]
    fn the_start_mode_switch_follows_the_setting() {
        let mut settings = Settings::default();
        apply_choice(&mut settings, Field::StartVietnamese, SWITCH_OFF);
        assert!(!settings.start_vietnamese);
        let row = find_row(&settings, Field::StartVietnamese);
        assert_eq!(row.control, Control::Switch { on: false });
    }

    #[test]
    fn an_option_past_the_end_changes_nothing() {
        let mut settings = Settings::default();
        for field in [
            Field::InputMethod,
            Field::ToneStyle,
            Field::ToggleHotkey,
            Field::Updates,
        ] {
            assert_eq!(apply_choice(&mut settings, field, 9), Effect::Nothing);
        }
        assert_eq!(settings, Settings::default());
    }

    #[test]
    fn exactly_one_mode_button_is_active() {
        let [vietnamese, english] = mode_buttons(true);
        assert!(vietnamese.active && !english.active);
        assert_eq!((vietnamese.tile, english.tile), ("V", "E"));
        let [vietnamese, english] = mode_buttons(false);
        assert!(!vietnamese.active && english.active);
    }

    #[test]
    fn the_tool_buttons_open_the_three_windows() {
        let tools: Vec<Tool> = actions().iter().map(|action| action.tool).collect();
        assert_eq!(tools, [Tool::Macros, Tool::ExcludedApps, Tool::Converter]);
    }

    #[test]
    fn space_flips_a_switch_and_cycles_chips() {
        assert_eq!(Control::Switch { on: true }.next_option(), SWITCH_OFF);
        assert_eq!(Control::Switch { on: false }.next_option(), SWITCH_ON);
        let chips = Control::Chips {
            options: vec!["a".into(), "b".into()],
            selected: 1,
        };
        assert_eq!(chips.next_option(), 0);
    }

    #[test]
    fn arrows_stop_at_the_ends_of_a_dropdown() {
        let dropdown = |selected| Control::Dropdown {
            options: vec!["a".into(), "b".into(), "c".into()],
            selected,
        };
        assert_eq!(dropdown(0).stepped_option(false), None);
        assert_eq!(dropdown(0).stepped_option(true), Some(1));
        assert_eq!(dropdown(2).stepped_option(true), None);
        assert_eq!(dropdown(2).stepped_option(false), Some(1));
    }

    #[test]
    fn the_cards_hold_three_six_and_two_rows() {
        let cards = cards(&Settings::default());
        assert_eq!(card_row_counts(&cards), [3, 6, 2]);
    }

    #[test]
    fn the_update_dropdown_reaches_all_three_modes() {
        let row = find_row(&Settings::default(), Field::Updates);
        let Control::Dropdown { options, selected } = row.control else {
            panic!("updates is a dropdown");
        };
        assert_eq!(options, ["Tự động", "Chỉ báo", "Tắt"]);
        assert_eq!(selected, 0);
        let mut settings = Settings::default();
        assert_eq!(apply_choice(&mut settings, Field::Updates, 1), Effect::Save);
        assert_eq!(settings.updates, UpdateMode::Notify);
        apply_choice(&mut settings, Field::Updates, 2);
        assert_eq!(settings.updates, UpdateMode::Off);
        apply_choice(&mut settings, Field::Updates, 0);
        assert_eq!(settings.updates, UpdateMode::Auto);
    }

    #[test]
    fn the_report_switch_is_on_by_default_and_follows_the_setting() {
        assert_eq!(
            find_row(&Settings::default(), Field::SendReports).control,
            Control::Switch { on: true }
        );
        let mut settings = Settings::default();
        assert_eq!(
            apply_choice(&mut settings, Field::SendReports, SWITCH_OFF),
            Effect::Save
        );
        assert!(!settings.send_reports);
    }
}
