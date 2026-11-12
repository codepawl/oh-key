//! Reads `%APPDATA%\oh-key\config.toml`.
//!
//! The file is a flat list of `key = value` lines, so a small parser is
//! enough and keeps the DLL free of a TOML dependency. Unknown keys and
//! malformed lines are ignored: a typo must never stop typing.

use oh_key_engine::{Config, InputMethod, ToneStyle};
use std::path::PathBuf;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToggleHotkey {
    CtrlShift,
    Disabled,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Settings {
    pub engine: Config,
    pub toggle_hotkey: ToggleHotkey,
    /// Start in Vietnamese mode; false starts with the English passthrough.
    pub start_vietnamese: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            engine: Config::default(),
            toggle_hotkey: ToggleHotkey::CtrlShift,
            start_vietnamese: true,
        }
    }
}

pub fn config_path() -> Option<PathBuf> {
    let application_data = std::env::var_os("APPDATA")?;
    Some(
        PathBuf::from(application_data)
            .join("oh-key")
            .join("config.toml"),
    )
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
        "toggle_hotkey" => match value.to_ascii_lowercase().replace(' ', "").as_str() {
            "ctrl+shift" => settings.toggle_hotkey = ToggleHotkey::CtrlShift,
            "none" | "off" | "" => settings.toggle_hotkey = ToggleHotkey::Disabled,
            _ => {}
        },
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

fn set_flag(target: &mut bool, value: &str) {
    match value.to_ascii_lowercase().as_str() {
        "true" | "yes" | "on" | "1" => *target = true,
        "false" | "no" | "off" | "0" => *target = false,
        _ => {}
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
             toggle_hotkey = \"none\"\nstart_vietnamese = no\n",
        );
        assert_eq!(settings.engine.input_method, InputMethod::Vni);
        assert_eq!(settings.engine.tone_style, ToneStyle::Classic);
        assert!(!settings.engine.spell_check);
        assert!(!settings.engine.free_tone_placement);
        assert!(!settings.engine.restore_invalid_word);
        assert!(settings.engine.macro_enabled);
        assert_eq!(settings.toggle_hotkey, ToggleHotkey::Disabled);
        assert!(!settings.start_vietnamese);
    }

    #[test]
    fn ignores_unknown_keys_and_bad_values() {
        let settings = parse_settings(
            "colour = red\ninput_method = klingon\nnot a line\nspell_check = maybe\n",
        );
        assert_eq!(settings, Settings::default());
    }
}
