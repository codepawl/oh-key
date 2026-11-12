//! Opt-in diagnostics for debugging the text service inside real applications.
//!
//! Logging is on only while `C:\Users\Public\Documents\oh-key.log` exists, so a
//! developer turns it on by creating that file and off by deleting it. Entries
//! name events and HRESULTs only: typed text is never written.

use std::fs::OpenOptions;
use std::io::Write;
use std::sync::OnceLock;

const LOG_PATH: &str = r"C:\Users\Public\Documents\oh-key.log";

fn enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| std::path::Path::new(LOG_PATH).exists())
}

pub fn log(message: std::fmt::Arguments<'_>) {
    if !enabled() {
        return;
    }
    let Ok(mut file) = OpenOptions::new().append(true).open(LOG_PATH) else {
        return;
    };
    let process_id = std::process::id();
    let _ = writeln!(file, "[{process_id}] {message}");
}

/// `trace!("edit session failed: {error:?}")`
macro_rules! trace {
    ($($argument:tt)*) => {
        $crate::diagnostics::log(format_args!($($argument)*))
    };
}
pub(crate) use trace;
