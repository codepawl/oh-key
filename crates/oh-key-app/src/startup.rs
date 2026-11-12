//! "Start with Windows": a value in the current user's Run key that points at this exe.

use std::path::Path;

use windows::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, KEY_SET_VALUE, REG_SZ, RegCloseKey, RegDeleteValueW, RegOpenKeyExW,
    RegSetValueExW,
};
use windows::core::w;

const RUN_KEY: windows::core::PCWSTR = w!("Software\\Microsoft\\Windows\\CurrentVersion\\Run");
const VALUE_NAME: windows::core::PCWSTR = w!("oh-key");

/// The path in quotes, so a folder with spaces still starts the right program.
pub fn run_command(executable: &Path) -> String {
    format!("\"{}\"", executable.display())
}

/// Adds or removes the Run value for the running exe. Failures are ignored: the setting
/// stays in the config file either way, and a locked-down machine simply keeps it off.
pub fn set_start_at_sign_in(enabled: bool) {
    let mut key = HKEY::default();
    let opened =
        unsafe { RegOpenKeyExW(HKEY_CURRENT_USER, RUN_KEY, None, KEY_SET_VALUE, &mut key) };
    if opened.is_err() {
        return;
    }
    if enabled {
        write_command(key);
    } else {
        let _ = unsafe { RegDeleteValueW(key, VALUE_NAME) };
    }
    let _ = unsafe { RegCloseKey(key) };
}

fn write_command(key: HKEY) {
    let Ok(executable) = std::env::current_exe() else {
        return;
    };
    let command = run_command(&executable);
    let bytes: Vec<u8> = command
        .encode_utf16()
        .chain(std::iter::once(0u16))
        .flat_map(u16::to_le_bytes)
        .collect();
    let _ = unsafe { RegSetValueExW(key, VALUE_NAME, None, REG_SZ, Some(&bytes)) };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_run_command_quotes_the_path() {
        let command = run_command(Path::new("C:\\Program Files\\oh-key\\oh-key.exe"));
        assert_eq!(command, "\"C:\\Program Files\\oh-key\\oh-key.exe\"");
    }
}
