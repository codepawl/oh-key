//! Finds and watches `config.toml` and `macros.txt` in `$XDG_CONFIG_HOME/oh-key/`.
//!
//! The files are read when a text field gets focus and their modification time changed, so an
//! edited setting applies without restarting IBus.

use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

const FOLDER_NAME: &str = "oh-key";
const SETTINGS_FILE: &str = "config.toml";
const MACROS_FILE: &str = "macros.txt";

/// `$XDG_CONFIG_HOME/oh-key`, or `~/.config/oh-key` when the variable is unset or empty.
pub fn config_folder(xdg_config_home: Option<OsString>, home: Option<OsString>) -> Option<PathBuf> {
    let base = match xdg_config_home.filter(|value| !value.is_empty()) {
        Some(xdg) => PathBuf::from(xdg),
        None => PathBuf::from(home.filter(|value| !value.is_empty())?).join(".config"),
    };
    Some(base.join(FOLDER_NAME))
}

pub fn config_folder_from_environment() -> Option<PathBuf> {
    config_folder(
        std::env::var_os("XDG_CONFIG_HOME"),
        std::env::var_os("HOME"),
    )
}

/// The contents of both files; a missing file reads as empty, which means the defaults.
pub struct ConfigText {
    pub settings: String,
    pub macros: String,
}

type Stamp = Option<SystemTime>;

pub struct ConfigFiles {
    folder: PathBuf,
    seen: Option<(Stamp, Stamp)>,
}

fn modification_time(path: &Path) -> Stamp {
    fs::metadata(path)
        .and_then(|metadata| metadata.modified())
        .ok()
}

fn read_or_empty(path: &Path) -> String {
    fs::read_to_string(path).unwrap_or_default()
}

impl ConfigFiles {
    pub fn new(folder: PathBuf) -> Self {
        ConfigFiles { folder, seen: None }
    }

    /// The new text when either file changed since the last call (always on the first call).
    pub fn changed_text(&mut self) -> Option<ConfigText> {
        let settings_path = self.folder.join(SETTINGS_FILE);
        let macros_path = self.folder.join(MACROS_FILE);
        let stamps = (
            modification_time(&settings_path),
            modification_time(&macros_path),
        );
        if self.seen == Some(stamps) {
            return None;
        }
        self.seen = Some(stamps);
        Some(ConfigText {
            settings: read_or_empty(&settings_path),
            macros: read_or_empty(&macros_path),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn xdg_config_home_wins_over_home() {
        let folder = config_folder(Some("/x".into()), Some("/home/an".into()));
        assert_eq!(folder, Some(PathBuf::from("/x").join("oh-key")));
    }

    #[test]
    fn empty_xdg_falls_back_to_dot_config() {
        let folder = config_folder(Some("".into()), Some("/home/an".into()));
        assert_eq!(
            folder,
            Some(PathBuf::from("/home/an").join(".config").join("oh-key"))
        );
    }

    #[test]
    fn no_variables_means_no_folder() {
        assert_eq!(config_folder(None, None), None);
    }

    #[test]
    fn a_change_is_reported_once() {
        let folder =
            std::env::temp_dir().join(format!("oh-key-ibus-config-{}", std::process::id()));
        fs::create_dir_all(&folder).unwrap();
        let mut files = ConfigFiles::new(folder.clone());
        let first = files.changed_text().expect("first call always reports");
        assert_eq!(first.settings, "");
        assert!(files.changed_text().is_none());
        fs::write(folder.join(SETTINGS_FILE), "input_method = \"vni\"\n").unwrap();
        let second = files.changed_text().expect("a new file is a change");
        assert!(second.settings.contains("vni"));
        assert!(files.changed_text().is_none());
        fs::remove_dir_all(&folder).unwrap();
    }
}
