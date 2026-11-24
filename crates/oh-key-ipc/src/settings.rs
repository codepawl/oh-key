//! Reads and writes `%APPDATA%\oh-key\config.toml`.
//!
//! The file is a flat list of `key = value` lines, so a small parser is
//! enough and keeps the app free of a TOML dependency. Unknown keys and
//! malformed lines are ignored: a typo must never stop typing.

use oh_key_engine::{Config, InputMethod, ToneStyle};
use std::io;
use std::path::PathBuf;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToggleHotkey {
    CtrlShift,
    AltZ,
    Disabled,
}

/// What the app does when a newer release exists.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UpdateMode {
    /// Download, verify and install it once typing has paused.
    Auto,
    /// Only tell the user.
    Notify,
    /// Never check on its own; the manual check still works.
    Off,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Settings {
    pub engine: Config,
    pub toggle_hotkey: ToggleHotkey,
    /// Start in Vietnamese mode; false starts with the English passthrough.
    pub start_vietnamese: bool,
    /// Start the tray app at every Windows sign-in, like any other input method.
    pub start_at_sign_in: bool,
    pub updates: UpdateMode,
    /// Send crash and error reports (never typed text) to the developers.
    pub send_reports: bool,
    /// A short sound when the mode switches between Vietnamese and English.
    pub sound_effects: bool,
    /// A small badge with flying letters by the caret when the mode switches.
    pub visual_effects: bool,
    /// Lower case executable names of the apps where oh-key stays off.
    pub excluded_apps: Vec<String>,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            engine: Config::default(),
            toggle_hotkey: ToggleHotkey::CtrlShift,
            start_vietnamese: true,
            start_at_sign_in: true,
            updates: UpdateMode::Auto,
            send_reports: true,
            sound_effects: true,
            visual_effects: true,
            excluded_apps: Vec::new(),
        }
    }
}

fn oh_key_folder() -> Option<PathBuf> {
    let application_data = std::env::var_os("APPDATA")?;
    Some(PathBuf::from(application_data).join("oh-key"))
}

pub fn config_path() -> Option<PathBuf> {
    Some(oh_key_folder()?.join("config.toml"))
}

pub fn macros_path() -> Option<PathBuf> {
    Some(oh_key_folder()?.join("macros.txt"))
}

pub fn load_settings() -> Settings {
    let Some(path) = config_path() else {
        return Settings::default();
    };
    match std::fs::read_to_string(path) {
        Ok(text) => parse_settings(&text),
        Err(_) => Settings::default(),
    }
}

/// Whether a report may be sent. Unlike `load_settings`, this fails closed: a config file
/// that exists but cannot be read, or a `send_reports` value that is not understood, counts as
/// "no". Only a missing file (a first start) or a plain "on" allows reports.
pub fn reports_allowed() -> bool {
    let Some(path) = config_path() else {
        return false;
    };
    match std::fs::read_to_string(path) {
        Ok(text) => reports_allowed_by(&text),
        Err(error) => error.kind() == io::ErrorKind::NotFound,
    }
}

/// The reading of `send_reports` in a config text; the last line wins, as in `parse_settings`.
pub fn reports_allowed_by(text: &str) -> bool {
    let mut allowed = true;
    for line in text.lines() {
        let line = line.split('#').next().unwrap_or("").trim();
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        if key.trim() != "send_reports" {
            continue;
        }
        match parse_flag(value.trim().trim_matches('"')) {
            Some(flag) => allowed = flag,
            None => return false,
        }
    }
    allowed
}

/// Why `config.toml` could not be read, when it exists but is unreadable. A missing file is
/// normal (first start) and is not a problem.
pub fn settings_file_problem() -> Option<String> {
    let path = config_path()?;
    match std::fs::read(path) {
        Ok(_) => None,
        Err(error) if error.kind() == io::ErrorKind::NotFound => None,
        Err(error) => Some(format!("config.toml could not be read: {:?}", error.kind())),
    }
}

/// Writes through a temporary file so a reader never sees half a file.
pub fn save_settings(settings: &Settings) -> io::Result<()> {
    let path = config_path().ok_or_else(|| io::Error::other("APPDATA is not set"))?;
    let folder = path
        .parent()
        .ok_or_else(|| io::Error::other("config path has no folder"))?;
    std::fs::create_dir_all(folder)?;
    let temporary_path = path.with_extension("toml.tmp");
    std::fs::write(&temporary_path, format_settings(settings))?;
    std::fs::rename(&temporary_path, &path)
}

pub fn parse_settings(text: &str) -> Settings {
    let mut settings = Settings::default();
    for line in text.lines() {
        let line = line.split('#').next().unwrap_or("").trim();
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        apply_setting(&mut settings, key.trim(), value.trim().trim_matches('"'));
    }
    settings
}

pub fn format_settings(settings: &Settings) -> String {
    let engine = &settings.engine;
    let lines = [
        format!(
            "input_method = \"{}\"",
            input_method_name(engine.input_method)
        ),
        format!("tone_style = \"{}\"", tone_style_name(engine.tone_style)),
        format!("free_tone_placement = {}", engine.free_tone_placement),
        format!("spell_check = {}", engine.spell_check),
        format!("restore_invalid_word = {}", engine.restore_invalid_word),
        format!("macro_enabled = {}", engine.macro_enabled),
        format!(
            "toggle_hotkey = \"{}\"",
            toggle_hotkey_name(settings.toggle_hotkey)
        ),
        format!("start_vietnamese = {}", settings.start_vietnamese),
        format!("start_at_sign_in = {}", settings.start_at_sign_in),
        format!("updates = \"{}\"", update_mode_name(settings.updates)),
        format!("send_reports = {}", settings.send_reports),
        format!("sound_effects = {}", settings.sound_effects),
        format!("visual_effects = {}", settings.visual_effects),
        format!("excluded_apps = \"{}\"", settings.excluded_apps.join(", ")),
    ];
    let mut text = lines.join("\n");
    text.push('\n');
    text
}

fn apply_setting(settings: &mut Settings, key: &str, value: &str) {
    let engine = &mut settings.engine;
    match key {
        "input_method" => {
            if let Some(method) = parse_input_method(value) {
                engine.input_method = method;
            }
        }
        "tone_style" => match value.to_ascii_lowercase().as_str() {
            "modern" => engine.tone_style = ToneStyle::Modern,
            "classic" => engine.tone_style = ToneStyle::Classic,
            _ => {}
        },
        "free_tone_placement" => set_flag(&mut engine.free_tone_placement, value),
        "spell_check" => set_flag(&mut engine.spell_check, value),
        "restore_invalid_word" => set_flag(&mut engine.restore_invalid_word, value),
        "macro_enabled" => set_flag(&mut engine.macro_enabled, value),
        "start_vietnamese" => set_flag(&mut settings.start_vietnamese, value),
        "start_at_sign_in" => set_flag(&mut settings.start_at_sign_in, value),
        "send_reports" => set_flag(&mut settings.send_reports, value),
        "sound_effects" => set_flag(&mut settings.sound_effects, value),
        "visual_effects" => set_flag(&mut settings.visual_effects, value),
        "updates" => {
            if let Some(mode) = parse_update_mode(value) {
                settings.updates = mode;
            }
        }
        "toggle_hotkey" => {
            if let Some(hotkey) = parse_toggle_hotkey(value) {
                settings.toggle_hotkey = hotkey;
            }
        }
        "excluded_apps" => settings.excluded_apps = parse_excluded_apps(value),
        _ => {}
    }
}

fn parse_input_method(value: &str) -> Option<InputMethod> {
    match value.to_ascii_lowercase().as_str() {
        "telex" => Some(InputMethod::Telex),
        "vni" => Some(InputMethod::Vni),
        "viqr" => Some(InputMethod::Viqr),
        "simple_telex" | "simple-telex" | "simpletelex" => Some(InputMethod::SimpleTelex),
        _ => None,
    }
}

fn parse_toggle_hotkey(value: &str) -> Option<ToggleHotkey> {
    match value.to_ascii_lowercase().replace(' ', "").as_str() {
        "ctrl+shift" => Some(ToggleHotkey::CtrlShift),
        "alt+z" => Some(ToggleHotkey::AltZ),
        "none" | "off" | "" => Some(ToggleHotkey::Disabled),
        _ => None,
    }
}

fn parse_update_mode(value: &str) -> Option<UpdateMode> {
    match value.to_ascii_lowercase().as_str() {
        "auto" => Some(UpdateMode::Auto),
        "notify" => Some(UpdateMode::Notify),
        "off" => Some(UpdateMode::Off),
        _ => None,
    }
}

fn update_mode_name(mode: UpdateMode) -> &'static str {
    match mode {
        UpdateMode::Auto => "auto",
        UpdateMode::Notify => "notify",
        UpdateMode::Off => "off",
    }
}

fn parse_excluded_apps(value: &str) -> Vec<String> {
    value
        .split(',')
        .map(|name| name.trim().to_ascii_lowercase())
        .filter(|name| !name.is_empty())
        .collect()
}

fn parse_flag(value: &str) -> Option<bool> {
    match value.to_ascii_lowercase().as_str() {
        "true" | "yes" | "on" | "1" => Some(true),
        "false" | "no" | "off" | "0" => Some(false),
        _ => None,
    }
}

fn set_flag(target: &mut bool, value: &str) {
    if let Some(flag) = parse_flag(value) {
        *target = flag;
    }
}

fn input_method_name(method: InputMethod) -> &'static str {
    match method {
        InputMethod::Telex => "telex",
        InputMethod::Vni => "vni",
        InputMethod::Viqr => "viqr",
        InputMethod::SimpleTelex => "simple_telex",
    }
}

fn tone_style_name(style: ToneStyle) -> &'static str {
    match style {
        ToneStyle::Modern => "modern",
        ToneStyle::Classic => "classic",
    }
}

fn toggle_hotkey_name(hotkey: ToggleHotkey) -> &'static str {
    match hotkey {
        ToggleHotkey::CtrlShift => "ctrl+shift",
        ToggleHotkey::AltZ => "alt+z",
        ToggleHotkey::Disabled => "none",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_file_gives_defaults() {
        assert_eq!(parse_settings(""), Settings::default());
    }

    #[test]
    fn reads_every_known_key() {
        let settings = parse_settings(
            "input_method = \"vni\"\ntone_style = classic # comment\nspell_check = false\n\
             free_tone_placement = off\nrestore_invalid_word = 0\nmacro_enabled = true\n\
             toggle_hotkey = \"none\"\nstart_vietnamese = no\nstart_at_sign_in = yes\n\
             excluded_apps = \"Game.exe, , Other.EXE\"\n",
        );
        assert_eq!(settings.engine.input_method, InputMethod::Vni);
        assert_eq!(settings.engine.tone_style, ToneStyle::Classic);
        assert!(!settings.engine.spell_check);
        assert!(!settings.engine.free_tone_placement);
        assert!(!settings.engine.restore_invalid_word);
        assert!(settings.engine.macro_enabled);
        assert_eq!(settings.toggle_hotkey, ToggleHotkey::Disabled);
        assert!(!settings.start_vietnamese);
        assert!(settings.start_at_sign_in);
        assert_eq!(settings.excluded_apps, ["game.exe", "other.exe"]);
    }

    #[test]
    fn effects_are_on_by_default_and_each_can_be_turned_off() {
        let defaults = Settings::default();
        assert!(defaults.sound_effects);
        assert!(defaults.visual_effects);
        let settings = parse_settings("sound_effects = false\nvisual_effects = off\n");
        assert!(!settings.sound_effects);
        assert!(!settings.visual_effects);
        assert_eq!(parse_settings(&format_settings(&settings)), settings);
    }

    #[test]
    fn the_old_hook_apps_line_is_ignored() {
        let settings = parse_settings("hook_apps = \"game.exe\"\n");
        assert!(settings.excluded_apps.is_empty());
    }

    #[test]
    fn ignores_unknown_keys_and_bad_values() {
        let settings = parse_settings(
            "colour = red\ninput_method = klingon\nnot a line\nspell_check = maybe\n",
        );
        assert_eq!(settings, Settings::default());
    }

    #[test]
    fn format_then_parse_round_trips_every_enum_value() {
        let input_methods = [
            InputMethod::Telex,
            InputMethod::Vni,
            InputMethod::Viqr,
            InputMethod::SimpleTelex,
        ];
        let tone_styles = [ToneStyle::Modern, ToneStyle::Classic];
        let hotkeys = [
            ToggleHotkey::CtrlShift,
            ToggleHotkey::AltZ,
            ToggleHotkey::Disabled,
        ];
        for input_method in input_methods {
            for tone_style in tone_styles {
                for toggle_hotkey in hotkeys {
                    let settings = Settings {
                        engine: Config {
                            input_method,
                            tone_style,
                            free_tone_placement: false,
                            spell_check: false,
                            restore_invalid_word: false,
                            macro_enabled: true,
                        },
                        toggle_hotkey,
                        start_vietnamese: false,
                        start_at_sign_in: true,
                        updates: UpdateMode::Notify,
                        send_reports: false,
                        sound_effects: false,
                        visual_effects: false,
                        excluded_apps: vec!["game.exe".to_owned(), "terminal.exe".to_owned()],
                    };
                    assert_eq!(parse_settings(&format_settings(&settings)), settings);
                }
            }
        }
    }

    #[test]
    fn updates_default_to_auto_and_reports_to_on() {
        let settings = Settings::default();
        assert_eq!(settings.updates, UpdateMode::Auto);
        assert!(settings.send_reports);
    }

    #[test]
    fn a_new_install_starts_at_sign_in_and_a_saved_choice_wins() {
        assert!(Settings::default().start_at_sign_in);
        assert!(
            parse_settings(
                "input_method = \"vni\"
"
            )
            .start_at_sign_in
        );
        assert!(
            !parse_settings(
                "start_at_sign_in = false
"
            )
            .start_at_sign_in
        );
    }

    #[test]
    fn an_old_config_without_the_new_keys_keeps_the_defaults() {
        let settings = parse_settings(
            "input_method = \"vni\"
start_at_sign_in = true
excluded_apps = \"game.exe\"
",
        );
        assert_eq!(settings.updates, UpdateMode::Auto);
        assert!(settings.send_reports);
        assert_eq!(settings.excluded_apps, ["game.exe"]);
    }

    #[test]
    fn every_update_mode_round_trips_and_bad_values_are_ignored() {
        for mode in [UpdateMode::Auto, UpdateMode::Notify, UpdateMode::Off] {
            let settings = Settings {
                updates: mode,
                ..Settings::default()
            };
            assert_eq!(parse_settings(&format_settings(&settings)).updates, mode);
        }
        assert_eq!(
            parse_settings(
                "updates = sometimes
"
            )
            .updates,
            UpdateMode::Auto
        );
        assert_eq!(
            parse_settings(
                "updates = OFF
"
            )
            .updates,
            UpdateMode::Off
        );
    }

    #[test]
    fn reports_are_allowed_only_when_the_setting_is_plainly_on_or_absent() {
        assert!(reports_allowed_by(""));
        assert!(reports_allowed_by(
            "send_reports = true
"
        ));
        assert!(reports_allowed_by(
            "send_reports = \"on\"
"
        ));
        assert!(!reports_allowed_by(
            "send_reports = false
"
        ));
        assert!(!reports_allowed_by(
            "send_reports = off
"
        ));
    }

    #[test]
    fn a_value_that_is_not_understood_turns_reports_off() {
        assert!(!reports_allowed_by(
            "send_reports = disabled
"
        ));
        assert!(!reports_allowed_by(
            "send_reports = none
"
        ));
        assert!(!reports_allowed_by(
            "send_reports = opt-out
"
        ));
        assert!(!reports_allowed_by(
            "send_reports = true
send_reports = maybe
"
        ));
        assert!(!reports_allowed_by(
            "send_reports = true
send_reports = false
"
        ));
    }

    #[test]
    fn default_settings_round_trip() {
        let settings = Settings::default();
        assert_eq!(parse_settings(&format_settings(&settings)), settings);
    }
}
