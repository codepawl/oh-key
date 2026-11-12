//! What the settings window shows: groups of rows, each a label and a few choices, and what a
//! click on a choice does to the settings. No drawing here, so it is all unit tested.

use oh_key_engine::{InputMethod, ToneStyle};
use oh_key_ipc::{Settings, ToggleHotkey};

use crate::icons;

pub const INPUT_METHOD_CHOICES: [(InputMethod, &str); 4] = [
    (InputMethod::Telex, "Telex"),
    (InputMethod::Vni, "VNI"),
    (InputMethod::Viqr, "VIQR"),
    (InputMethod::SimpleTelex, "Telex đơn giản"),
];
const TONE_STYLE_CHOICES: [(ToneStyle, &str); 2] = [
    (ToneStyle::Modern, "kiểu mới"),
    (ToneStyle::Classic, "kiểu cũ"),
];
const HOTKEY_CHOICES: [(ToggleHotkey, &str); 3] = [
    (ToggleHotkey::CtrlShift, "Ctrl+Shift"),
    (ToggleHotkey::AltZ, "Alt+Z"),
    (ToggleHotkey::Disabled, "tắt"),
];
const ENABLED_CHOICES: [&str; 2] = ["bật", "tắt"];

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Field {
    InputMethod,
    ToneStyle,
    FreeTonePlacement,
    SpellCheck,
    RestoreInvalidWord,
    ToggleHotkey,
    MacroEnabled,
    MacroList,
    ExcludedApps,
    Converter,
    StartAtSignIn,
    Version,
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
    Open(Tool),
}

pub struct Row {
    pub field: Field,
    pub icon: char,
    pub label: &'static str,
    pub options: Vec<String>,
    /// None when the buttons are actions rather than a choice.
    pub selected: Option<usize>,
    /// Dim text between the label and the buttons.
    pub trailing: Option<String>,
}

pub struct Section {
    pub icon: char,
    pub title: &'static str,
    pub rows: Vec<Row>,
}

fn options(labels: &[&str]) -> Vec<String> {
    labels.iter().map(|label| label.to_string()).collect()
}

fn enabled_row(field: Field, icon: char, label: &'static str, enabled: bool) -> Row {
    Row {
        field,
        icon,
        label,
        options: options(&ENABLED_CHOICES),
        selected: Some(if enabled { 0 } else { 1 }),
        trailing: None,
    }
}

fn action_row(
    field: Field,
    icon: char,
    label: &'static str,
    action: &str,
    trailing: Option<String>,
) -> Row {
    Row {
        field,
        icon,
        label,
        options: options(&[action]),
        selected: None,
        trailing,
    }
}

fn choice_row(
    field: Field,
    icon: char,
    label: &'static str,
    labels: Vec<&str>,
    selected: Option<usize>,
) -> Row {
    Row {
        field,
        icon,
        label,
        options: options(&labels),
        selected,
        trailing: None,
    }
}

fn typing_section(settings: &Settings) -> Section {
    let engine = &settings.engine;
    let method_labels = INPUT_METHOD_CHOICES
        .iter()
        .map(|(_, label)| *label)
        .collect();
    let method_selected = INPUT_METHOD_CHOICES
        .iter()
        .position(|(method, _)| *method == engine.input_method);
    let tone_labels = TONE_STYLE_CHOICES.iter().map(|(_, label)| *label).collect();
    let tone_selected = TONE_STYLE_CHOICES
        .iter()
        .position(|(style, _)| *style == engine.tone_style);
    Section {
        icon: icons::KEYBOARD,
        title: "Gõ",
        rows: vec![
            choice_row(
                Field::InputMethod,
                icons::KEYBOARD,
                "Kiểu gõ",
                method_labels,
                method_selected,
            ),
            choice_row(
                Field::ToneStyle,
                icons::EDIT,
                "Đặt dấu",
                tone_labels,
                tone_selected,
            ),
            enabled_row(
                Field::FreeTonePlacement,
                icons::MARK,
                "Bỏ dấu tự do",
                engine.free_tone_placement,
            ),
            enabled_row(
                Field::SpellCheck,
                icons::TEXT,
                "Kiểm tra chính tả",
                engine.spell_check,
            ),
            enabled_row(
                Field::RestoreInvalidWord,
                icons::RETAKE,
                "Khôi phục từ tiếng Anh",
                engine.restore_invalid_word,
            ),
        ],
    }
}

fn hotkey_section(settings: &Settings) -> Section {
    let labels = HOTKEY_CHOICES.iter().map(|(_, label)| *label).collect();
    let selected = HOTKEY_CHOICES
        .iter()
        .position(|(hotkey, _)| *hotkey == settings.toggle_hotkey);
    Section {
        icon: icons::KEYBOARD,
        title: "Phím tắt",
        rows: vec![choice_row(
            Field::ToggleHotkey,
            icons::GLOBE,
            "Chuyển Việt/Anh",
            labels,
            selected,
        )],
    }
}

fn macro_section(settings: &Settings) -> Section {
    Section {
        icon: icons::SPARKLE,
        title: "Gõ tắt",
        rows: vec![
            enabled_row(
                Field::MacroEnabled,
                icons::SPARKLE,
                "Gõ tắt",
                settings.engine.macro_enabled,
            ),
            action_row(Field::MacroList, icons::CLIPBOARD, "Danh sách", "sửa", None),
        ],
    }
}

fn excluded_apps_section(settings: &Settings) -> Section {
    let count = format!("{} ứng dụng", settings.excluded_apps.len());
    Section {
        icon: icons::WINDOW,
        title: "Ứng dụng tắt oh-key",
        rows: vec![action_row(
            Field::ExcludedApps,
            icons::WINDOW,
            "Danh sách",
            "sửa",
            Some(count),
        )],
    }
}

fn tools_section() -> Section {
    Section {
        icon: icons::SYNC,
        title: "Công cụ",
        rows: vec![action_row(
            Field::Converter,
            icons::SYNC,
            "Chuyển mã",
            "mở",
            None,
        )],
    }
}

fn system_section(settings: &Settings) -> Section {
    Section {
        icon: icons::GEAR,
        title: "Hệ thống",
        rows: vec![
            enabled_row(
                Field::StartAtSignIn,
                icons::POWER,
                "Khởi động cùng Windows",
                settings.start_at_sign_in,
            ),
            Row {
                field: Field::Version,
                icon: icons::INFO,
                label: "Phiên bản",
                options: Vec::new(),
                selected: None,
                trailing: Some(format!("v{VERSION}")),
            },
        ],
    }
}

/// The two columns, groups and rows in reading order, built from the current settings.
pub fn columns(settings: &Settings) -> [Vec<Section>; 2] {
    [
        vec![
            typing_section(settings),
            hotkey_section(settings),
            tools_section(),
        ],
        vec![
            macro_section(settings),
            excluded_apps_section(settings),
            system_section(settings),
        ],
    ]
}

pub fn rows_per_section(sections: &[Section]) -> Vec<usize> {
    sections.iter().map(|section| section.rows.len()).collect()
}

/// Applies a click on choice `option` of `field`.
pub fn apply_choice(settings: &mut Settings, field: Field, option: usize) -> Effect {
    let enabled = option == 0;
    match field {
        Field::InputMethod => match INPUT_METHOD_CHOICES.get(option) {
            Some((method, _)) => settings.engine.input_method = *method,
            None => return Effect::Nothing,
        },
        Field::ToneStyle => match TONE_STYLE_CHOICES.get(option) {
            Some((style, _)) => settings.engine.tone_style = *style,
            None => return Effect::Nothing,
        },
        Field::FreeTonePlacement => settings.engine.free_tone_placement = enabled,
        Field::SpellCheck => settings.engine.spell_check = enabled,
        Field::RestoreInvalidWord => settings.engine.restore_invalid_word = enabled,
        Field::ToggleHotkey => match HOTKEY_CHOICES.get(option) {
            Some((hotkey, _)) => settings.toggle_hotkey = *hotkey,
            None => return Effect::Nothing,
        },
        Field::MacroEnabled => settings.engine.macro_enabled = enabled,
        Field::MacroList => return Effect::Open(Tool::Macros),
        Field::ExcludedApps => return Effect::Open(Tool::ExcludedApps),
        Field::Converter => return Effect::Open(Tool::Converter),
        Field::StartAtSignIn => {
            settings.start_at_sign_in = enabled;
            return Effect::SaveAndApplyStartup;
        }
        Field::Version => return Effect::Nothing,
    }
    Effect::Save
}

#[cfg(test)]
mod tests {
    use super::*;

    fn all_rows(settings: &Settings) -> Vec<Row> {
        columns(settings)
            .into_iter()
            .flatten()
            .flat_map(|section| section.rows)
            .collect()
    }

    fn find_row(settings: &Settings, field: Field) -> Row {
        all_rows(settings)
            .into_iter()
            .find(|row| row.field == field)
            .unwrap()
    }

    #[test]
    fn every_choice_row_shows_the_option_that_was_just_chosen() {
        let choice_fields = [
            Field::InputMethod,
            Field::ToneStyle,
            Field::FreeTonePlacement,
            Field::SpellCheck,
            Field::RestoreInvalidWord,
            Field::ToggleHotkey,
            Field::MacroEnabled,
            Field::StartAtSignIn,
        ];
        for field in choice_fields {
            let option_count = find_row(&Settings::default(), field).options.len();
            for option in 0..option_count {
                let mut settings = Settings::default();
                apply_choice(&mut settings, field, option);
                let row = find_row(&settings, field);
                assert_eq!(row.selected, Some(option), "{field:?} option {option}");
            }
        }
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
    fn the_second_tone_style_is_classic() {
        let mut settings = Settings::default();
        apply_choice(&mut settings, Field::ToneStyle, 1);
        assert_eq!(settings.engine.tone_style, ToneStyle::Classic);
    }

    #[test]
    fn on_is_the_first_option_and_off_the_second() {
        let mut settings = Settings::default();
        apply_choice(&mut settings, Field::SpellCheck, 1);
        assert!(!settings.engine.spell_check);
        apply_choice(&mut settings, Field::SpellCheck, 0);
        assert!(settings.engine.spell_check);
    }

    #[test]
    fn the_hotkey_row_cycles_through_all_three_values() {
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
            apply_choice(&mut settings, Field::StartAtSignIn, 0),
            Effect::SaveAndApplyStartup
        );
        assert!(settings.start_at_sign_in);
    }

    #[test]
    fn the_edit_and_open_buttons_open_their_windows_without_changing_settings() {
        let mut settings = Settings::default();
        assert_eq!(
            apply_choice(&mut settings, Field::MacroList, 0),
            Effect::Open(Tool::Macros)
        );
        assert_eq!(
            apply_choice(&mut settings, Field::ExcludedApps, 0),
            Effect::Open(Tool::ExcludedApps)
        );
        assert_eq!(
            apply_choice(&mut settings, Field::Converter, 0),
            Effect::Open(Tool::Converter)
        );
        assert_eq!(settings, Settings::default());
    }

    #[test]
    fn an_option_past_the_end_changes_nothing() {
        let mut settings = Settings::default();
        assert_eq!(
            apply_choice(&mut settings, Field::InputMethod, 9),
            Effect::Nothing
        );
        assert_eq!(
            apply_choice(&mut settings, Field::Version, 0),
            Effect::Nothing
        );
        assert_eq!(settings, Settings::default());
    }

    #[test]
    fn the_excluded_apps_row_counts_the_listed_apps() {
        let settings = Settings {
            excluded_apps: vec!["notepad.exe".into(), "code.exe".into()],
            ..Settings::default()
        };
        assert_eq!(
            find_row(&settings, Field::ExcludedApps).trailing.as_deref(),
            Some("2 ứng dụng")
        );
    }

    #[test]
    fn the_columns_hold_twelve_rows_in_six_groups() {
        let [left, right] = columns(&Settings::default());
        assert_eq!(left.len() + right.len(), 6);
        let total: usize = rows_per_section(&left)
            .iter()
            .chain(rows_per_section(&right).iter())
            .sum();
        assert_eq!(total, 12);
    }
}
