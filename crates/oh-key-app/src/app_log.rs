//! A small append-only log in `%LOCALAPPDATA%\oh-key\oh-key.log`, for the tray app that has no
//! console. One line per event: `<unix seconds> <message>`.
//!
//! Privacy: this app sees every key, so nothing typed may reach the log. Callers pass only
//! fixed words, counts and versions.

use std::io::Write;
use std::path::PathBuf;

const LOG_FILE_NAME: &str = "oh-key.log";
/// Past this size the log is deleted and starts again.
const MAX_LOG_BYTES: u64 = 1_000_000;
/// Overrides the data folder, so a test run never touches the real one.
const DATA_FOLDER_VARIABLE: &str = "OH_KEY_DATA_DIR";

/// Where the log, the install id, the report state and `last-crash.txt` live.
pub fn data_folder() -> PathBuf {
    if let Some(folder) = std::env::var_os(DATA_FOLDER_VARIABLE) {
        return PathBuf::from(folder);
    }
    let base = std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    base.join("oh-key")
}

pub fn log_path() -> PathBuf {
    data_folder().join(LOG_FILE_NAME)
}

/// One log line without line breaks, so a message can never fake a second entry.
pub fn format_line(unix_seconds: u64, message: &str) -> String {
    let single_line: String = message
        .chars()
        .map(|character| {
            if character.is_control() {
                ' '
            } else {
                character
            }
        })
        .collect();
    format!("{unix_seconds} {single_line}")
}

pub fn exceeds_size_cap(log_bytes: u64) -> bool {
    log_bytes > MAX_LOG_BYTES
}

pub fn unix_seconds_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs())
        .unwrap_or_default()
}

pub fn write(message: &str) {
    let folder = data_folder();
    if std::fs::create_dir_all(&folder).is_err() {
        return;
    }
    let path = folder.join(LOG_FILE_NAME);
    if let Ok(metadata) = std::fs::metadata(&path)
        && exceeds_size_cap(metadata.len())
    {
        let _ = std::fs::remove_file(&path);
    }
    let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
    else {
        return;
    };
    let _ = writeln!(file, "{}", format_line(unix_seconds_now(), message));
}

pub fn read_all() -> String {
    std::fs::read_to_string(log_path()).unwrap_or_default()
}

/// Opens the folder that holds the log in Explorer.
pub fn open_folder() {
    use windows::Win32::UI::Shell::ShellExecuteW;
    use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
    use windows::core::{HSTRING, w};
    let folder = data_folder();
    let _ = std::fs::create_dir_all(&folder);
    unsafe {
        ShellExecuteW(
            None,
            w!("open"),
            &HSTRING::from(folder.as_os_str()),
            None,
            None,
            SW_SHOWNORMAL,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_line_is_the_time_then_the_message() {
        assert_eq!(format_line(1700000000, "started"), "1700000000 started");
    }

    #[test]
    fn line_breaks_in_a_message_cannot_make_a_second_entry() {
        let line = format_line(5, "first\r\n6 forged entry");
        assert_eq!(line.lines().count(), 1);
        assert_eq!(line, "5 first  6 forged entry");
    }

    #[test]
    fn the_log_restarts_only_past_one_million_bytes() {
        assert!(!exceeds_size_cap(1_000_000));
        assert!(exceeds_size_cap(1_000_001));
    }
}
