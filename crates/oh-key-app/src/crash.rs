//! Leaves a line in the log when the program dies, so a report can say where: the tray app has
//! no console, and Windows often keeps no record of its own. A tray app that dies starts a
//! new one (`--after-crash`), which reports the crash if the user allows it.
//!
//! Release builds abort on a panic, so every panic is a crash. Panic messages are reduced to
//! text that cannot hold typed characters: a formatted message may carry the value that
//! failed, and the engine handles what people type.

use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use windows::Win32::System::Diagnostics::Debug::{EXCEPTION_POINTERS, SetUnhandledExceptionFilter};
use windows::Win32::System::LibraryLoader::{
    GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS, GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
    GetModuleFileNameW, GetModuleHandleExW,
};
use windows::core::PCWSTR;

use crate::{app_log, report};

const CONTINUE_SEARCH: i32 = 0;
/// A tray app that a crash started and that dies again this soon stays down: bringing it
/// back would loop.
const CRASH_LOOP_WINDOW: Duration = Duration::from_secs(60);
/// What the last crash was, kept until the next tray app reports it.
const PENDING_FILE: &str = "last-crash.txt";
const FRAMES_PREFIX: &str = "frames: ";
const MAX_SUMMARY_CHARACTERS: usize = 300;

/// Panic messages that name no data. The matched part is all that is kept.
const SAFE_PANIC_PREFIXES: [&str; 12] = [
    "called `Option::unwrap()` on a `None` value",
    "called `Result::unwrap()` on an `Err` value",
    "index out of bounds",
    "slice index starts at",
    "range end index",
    "range start index",
    "byte index",
    "attempt to ",
    "assertion failed",
    "internal error: entered unreachable code",
    "capacity overflow",
    "already borrowed",
];

/// When the tray app started, and whether a crash started it. Only the tray app sets this,
/// so only it comes back after a crash.
static TRAY_STARTED: OnceLock<(Instant, bool)> = OnceLock::new();
/// A panic can end in an exception too; the first report is the one that counts.
static ALREADY_RECORDED: AtomicBool = AtomicBool::new(false);

#[derive(Debug, PartialEq, Eq)]
pub struct CrashRecord {
    pub summary: String,
    pub frames: Vec<String>,
}

/// Called by the tray app: from here on a crash is kept for a report and the tray app comes
/// back by itself.
pub fn watch_tray(started_after_crash: bool) {
    let _ = TRAY_STARTED.set((Instant::now(), started_after_crash));
}

/// The crash the last tray app died of, once.
pub fn take_pending() -> Option<CrashRecord> {
    let path = app_log::data_folder().join(PENDING_FILE);
    let text = std::fs::read_to_string(&path).ok()?;
    let _ = std::fs::remove_file(&path);
    parse_pending(&text)
}

impl CrashRecord {
    /// A panic is reported as a panic, anything else (an exception) as a crash.
    pub fn report_kind(&self) -> report::EventKind {
        if self.summary.starts_with("panic") {
            report::EventKind::Panic
        } else {
            report::EventKind::Crash
        }
    }
}

pub fn format_pending(record: &CrashRecord) -> String {
    format!(
        "{}\n{FRAMES_PREFIX}{}\n",
        record.summary,
        record.frames.join(";")
    )
}

pub fn parse_pending(text: &str) -> Option<CrashRecord> {
    let mut lines = text.lines().map(str::trim).filter(|line| !line.is_empty());
    let summary = lines.next()?.to_string();
    let frames = lines
        .find_map(|line| line.strip_prefix(FRAMES_PREFIX))
        .map(|list| {
            list.split(';')
                .filter(|name| !name.is_empty())
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();
    Some(CrashRecord { summary, frames })
}

/// A restart after a crash is allowed unless this app is itself a restart that died at once.
pub fn restart_allowed(started_after_crash: bool, uptime: Duration) -> bool {
    !(started_after_crash && uptime < CRASH_LOOP_WINDOW)
}

fn record(summary: &str, frames: Vec<String>) {
    if ALREADY_RECORDED.swap(true, Ordering::SeqCst) {
        return;
    }
    let identity = report::Identity::from_environment();
    let summary = report::scrub_text(summary, &identity, MAX_SUMMARY_CHARACTERS);
    app_log::write(&format!(
        "crash in {}: {summary}",
        env!("CARGO_PKG_VERSION")
    ));
    let Some((started, started_after_crash)) = TRAY_STARTED.get() else {
        return;
    };
    let pending = CrashRecord { summary, frames };
    let _ = std::fs::write(
        app_log::data_folder().join(PENDING_FILE),
        format_pending(&pending),
    );
    if !restart_allowed(*started_after_crash, started.elapsed()) {
        app_log::write("crashed again right after a restart, staying down");
        return;
    }
    restart_tray();
}

/// Starts a new tray app. It waits for this one to let go of the single-instance lock.
fn restart_tray() {
    let Ok(exe) = std::env::current_exe() else {
        return;
    };
    if let Err(error) = std::process::Command::new(exe).arg("--after-crash").spawn() {
        app_log::write(&format!("could not restart after the crash: {error}"));
    }
}

/// Hooks everything this OS can report: native exceptions and panics.
pub fn install() {
    unsafe {
        let _ = SetUnhandledExceptionFilter(Some(on_exception));
    }
    std::panic::set_hook(Box::new(on_panic));
}

/// What a panic message may say: a literal written in our code as it is, a formatted one
/// only as far as a known safe opening.
pub fn describe_panic(literal: Option<&str>, formatted: Option<&str>) -> String {
    if let Some(text) = literal {
        return text.chars().take(200).collect();
    }
    if let Some(text) = formatted {
        return safe_panic_message(text);
    }
    "(message withheld)".to_string()
}

pub fn safe_panic_message(message: &str) -> String {
    SAFE_PANIC_PREFIXES
        .iter()
        .find(|prefix| message.starts_with(*prefix))
        .map_or_else(
            || "(message withheld)".to_string(),
            |prefix| prefix.to_string(),
        )
}

fn on_panic(info: &std::panic::PanicHookInfo<'_>) {
    let payload = info.payload();
    let message = describe_panic(
        payload.downcast_ref::<&str>().copied(),
        payload.downcast_ref::<String>().map(String::as_str),
    );
    let place = info.location().map_or_else(String::new, |location| {
        format!(
            " at {}:{}:{}",
            report::short_source_path(location.file()),
            location.line(),
            location.column()
        )
    });
    let backtrace = std::backtrace::Backtrace::force_capture().to_string();
    record(
        &format!("panic{place}: {message}"),
        report::own_function_names(&backtrace),
    );
}

/// `oh-key.exe+0x10` for a fault in our own program. Another module (an overlay, an antivirus
/// or an input tool injected into this process) is not named: its file name would list what
/// the person has installed.
pub fn exception_place(module_file_name: &str, offset: usize) -> String {
    let name = if module_file_name.eq_ignore_ascii_case("oh-key.exe") {
        "oh-key.exe"
    } else {
        "other-module"
    };
    format!("{name}+{offset:#x}")
}

unsafe extern "system" fn on_exception(info: *const EXCEPTION_POINTERS) -> i32 {
    let exception = unsafe { &*(*info).ExceptionRecord };
    let address = exception.ExceptionAddress as usize;
    let mut module = Default::default();
    let found = unsafe {
        GetModuleHandleExW(
            GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
            PCWSTR(exception.ExceptionAddress as *const u16),
            &mut module,
        )
    }
    .is_ok();
    let place = if found {
        let mut name = [0u16; 400];
        let length = unsafe { GetModuleFileNameW(Some(module), &mut name) } as usize;
        let path = String::from_utf16_lossy(&name[..length]);
        let file = path.rsplit('\\').next().unwrap_or(&path);
        exception_place(file, address - module.0 as usize)
    } else {
        format!("{address:#x}")
    };
    record(
        &format!(
            "exception {:#x} at {place}",
            exception.ExceptionCode.0 as u32
        ),
        Vec::new(),
    );
    CONTINUE_SEARCH
}

/// `oh-key crash-test [panic]`: dies on purpose, to prove that the log gets its line. It is
/// not the tray app, so nothing restarts and no report is kept.
pub fn test(kind: Option<&str>) -> ! {
    if kind == Some("panic") {
        panic!("crash test");
    }
    let nowhere = std::hint::black_box(std::ptr::null_mut::<u8>());
    unsafe { nowhere.write_volatile(1) };
    unreachable!("the write above faults");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_formatted_message_that_may_hold_typed_text_is_withheld() {
        let leaking = "byte index 1 is not a char boundary; it is inside 'ế' of `ếabc`";
        assert_eq!(safe_panic_message(leaking), "byte index");
        let debug_of_input = "called `Result::unwrap()` on an `Err` value: Parse(\"xin chào\")";
        assert_eq!(
            safe_panic_message(debug_of_input),
            "called `Result::unwrap()` on an `Err` value"
        );
        assert_eq!(
            safe_panic_message("assertion `left == right` failed\n  left: \"viet\""),
            "(message withheld)"
        );
        assert_eq!(
            safe_panic_message("something about the word xin"),
            "(message withheld)"
        );
    }

    #[test]
    fn a_literal_message_from_our_code_is_kept_and_cut() {
        assert_eq!(describe_panic(Some("crash test"), None), "crash test");
        assert_eq!(describe_panic(Some(&"x".repeat(500)), None).len(), 200);
        assert_eq!(
            describe_panic(
                None,
                Some("index out of bounds: the len is 3 but the index is 5")
            ),
            "index out of bounds"
        );
        assert_eq!(describe_panic(None, None), "(message withheld)");
    }

    #[test]
    fn the_pending_crash_round_trips_through_its_file() {
        let record = CrashRecord {
            summary: "panic at crates/oh-key-app/src/hook.rs:1:2: index out of bounds".into(),
            frames: vec!["oh_key_app::hook::run".into(), "oh_key_engine::push".into()],
        };
        assert_eq!(parse_pending(&format_pending(&record)), Some(record));
    }

    #[test]
    fn a_pending_file_without_frames_or_content_is_handled() {
        assert_eq!(
            parse_pending("exception 0xc0000005 at oh-key.exe+0x10\n"),
            Some(CrashRecord {
                summary: "exception 0xc0000005 at oh-key.exe+0x10".into(),
                frames: Vec::new()
            })
        );
        assert_eq!(parse_pending("  \n\n"), None);
    }

    #[test]
    fn a_panic_is_reported_as_a_panic_and_an_exception_as_a_crash() {
        let kind_of = |summary: &str| {
            CrashRecord {
                summary: summary.into(),
                frames: Vec::new(),
            }
            .report_kind()
        };
        assert_eq!(
            kind_of("panic at x.rs:1:1: crash test"),
            report::EventKind::Panic
        );
        assert_eq!(
            kind_of("exception 0xc0000005 at oh-key.exe+0x1"),
            report::EventKind::Crash
        );
    }

    #[test]
    fn only_our_own_module_is_named_in_an_exception() {
        assert_eq!(exception_place("oh-key.exe", 0x10), "oh-key.exe+0x10");
        assert_eq!(exception_place("OH-KEY.EXE", 0x10), "oh-key.exe+0x10");
        assert_eq!(
            exception_place("SomeOverlayHook64.dll", 0x20),
            "other-module+0x20"
        );
    }

    #[test]
    fn a_crash_within_a_minute_of_a_crash_restart_stays_down() {
        let soon = Duration::from_secs(59);
        let later = Duration::from_secs(61);
        assert!(!restart_allowed(true, soon));
        assert!(restart_allowed(true, later));
        assert!(restart_allowed(false, soon));
    }
}
