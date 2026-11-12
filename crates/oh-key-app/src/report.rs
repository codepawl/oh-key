//! Crash and error reports, sent automatically unless the user turns `send_reports` off.
//!
//! PRIVACY: this app sees every key. A report holds only: app name and version, Windows
//! version and architecture, a random install id, the kind of event, a short message and
//! source location from our own code, function names, counts and the last few log lines, all
//! scrubbed of the user's home folder and name. It never holds typed text, key sequences,
//! the clipboard, window titles, file contents or the name of the program being typed into.
//!
//! Events go to a Cloudflare Worker that dedupes them and files issues. They are handed to a
//! background thread, so neither the keyboard hook nor the window thread ever waits on disk
//! or network; a failed send is only logged.

use std::path::PathBuf;
use std::sync::OnceLock;
use std::sync::mpsc::{Sender, channel};

use crate::{app_log, http};

/// The one place that names the Worker. The placeholder is replaced after deploying it; while
/// it is a placeholder nothing is sent.
const REPORT_ENDPOINT: &str = "https://oh-key-report.codepawl.com/v1/report";

pub const MAX_REPORTS_PER_DAY: u32 = 5;
/// Noisy kinds may use only this many of the day's reports, so a flood of them can never
/// leave no room for a crash.
pub const LOW_PRIORITY_REPORTS_PER_DAY: u32 = 3;
pub const DEDUPE_SECONDS: u64 = 24 * 60 * 60;
const SECONDS_PER_DAY: u64 = 24 * 60 * 60;
const MAX_LOG_LINES: usize = 8;
const MAX_LINE_CHARACTERS: usize = 280;
const MAX_MESSAGE_CHARACTERS: usize = 300;
const STATE_FILE: &str = "report-state.txt";
const INSTALL_ID_FILE: &str = "install-id.txt";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EventKind {
    Crash,
    Panic,
    Error,
    UpdateFailed,
    HookReinstalled,
    BreakerTripped,
}

impl EventKind {
    pub fn name(self) -> &'static str {
        match self {
            EventKind::Crash => "crash",
            EventKind::Panic => "panic",
            EventKind::Error => "error",
            EventKind::UpdateFailed => "update-failed",
            EventKind::HookReinstalled => "hook-reinstalled",
            EventKind::BreakerTripped => "breaker-tripped",
        }
    }

    fn daily_limit(self) -> u32 {
        match self {
            EventKind::Crash | EventKind::Panic | EventKind::UpdateFailed => MAX_REPORTS_PER_DAY,
            EventKind::Error | EventKind::HookReinstalled | EventKind::BreakerTripped => {
                LOW_PRIORITY_REPORTS_PER_DAY
            }
        }
    }
}

#[derive(Clone, Debug)]
pub struct ReportEvent {
    pub kind: EventKind,
    /// A short message from our own code.
    pub message: String,
    /// Function names of ours, newest first.
    pub frames: Vec<String>,
    pub count: Option<u32>,
    /// Whether to also write the event to the log (a crash was logged when it happened).
    pub write_to_log: bool,
}

impl ReportEvent {
    pub fn new(kind: EventKind, message: &str) -> ReportEvent {
        ReportEvent {
            kind,
            message: message.to_string(),
            frames: Vec::new(),
            count: None,
            write_to_log: true,
        }
    }
}

// ---- scrubbing -------------------------------------------------------------------------

/// What identifies the person in a path or message.
#[derive(Clone, Debug, Default)]
pub struct Identity {
    pub home: Option<String>,
    pub user_name: Option<String>,
    pub computer_name: Option<String>,
}

impl Identity {
    pub fn from_environment() -> Identity {
        let home = std::env::var("USERPROFILE")
            .or_else(|_| std::env::var("HOME"))
            .ok();
        let user_name = std::env::var("USERNAME").ok().or_else(|| {
            home.as_deref()
                .and_then(|home| home.rsplit(['\\', '/']).next())
                .map(str::to_string)
        });
        Identity {
            home,
            user_name,
            computer_name: std::env::var("COMPUTERNAME").ok(),
        }
    }
}

/// One line of text with the home folder, user name and computer name taken out, cut to
/// `max_characters`. A name shorter than three characters would hit ordinary words, and the
/// home folder is replaced as a whole before that.
pub fn scrub_text(text: &str, identity: &Identity, max_characters: usize) -> String {
    let mut scrubbed: String = text
        .chars()
        .take(4000)
        .map(|character| {
            if character.is_control() {
                ' '
            } else {
                character
            }
        })
        .collect();
    if let Some(home) = identity.home.as_deref().filter(|home| home.len() > 3) {
        let home = home.trim_start_matches(r"\\?\");
        scrubbed = replace_ignoring_case(&scrubbed, home, "~");
        scrubbed = replace_ignoring_case(&scrubbed, &home.replace('\\', "/"), "~");
    }
    if let Some(short_name) = identity.user_name.as_deref().and_then(short_folder_name) {
        scrubbed = replace_ignoring_case(&scrubbed, &short_name, "user");
    }
    for (name, replacement) in [
        (identity.user_name.as_deref(), "user"),
        (identity.computer_name.as_deref(), "pc"),
    ] {
        if let Some(name) = name.filter(|name| name.chars().count() >= 3) {
            scrubbed = replace_ignoring_case(&scrubbed, name, replacement);
        }
    }
    scrubbed.chars().take(max_characters).collect()
}

/// The 8.3 form Windows may use for a long profile folder: `NXAN2~1` for `nxan2long`. Only a
/// name that is too long or holds a space gets one.
fn short_folder_name(user_name: &str) -> Option<String> {
    let needs_short_form = user_name.chars().count() > 8 || user_name.contains(' ');
    if !needs_short_form {
        return None;
    }
    let stem: String = user_name
        .chars()
        .filter(|character| character.is_ascii_alphanumeric())
        .take(6)
        .collect();
    (stem.len() >= 2).then(|| format!("{stem}~1"))
}

/// Windows paths match in any case, and so do names with non-ASCII letters. Each character is
/// compared by its lowercase form, which keeps the walk aligned character by character.
fn replace_ignoring_case(text: &str, needle: &str, replacement: &str) -> String {
    let needle_characters: Vec<char> = needle.chars().map(lowercase_character).collect();
    if needle_characters.is_empty() {
        return text.to_string();
    }
    let characters: Vec<char> = text.chars().collect();
    let lowered: Vec<char> = characters
        .iter()
        .copied()
        .map(lowercase_character)
        .collect();
    let mut replaced = String::new();
    let mut index = 0;
    while index < characters.len() {
        if lowered[index..].starts_with(&needle_characters) {
            replaced.push_str(replacement);
            index += needle_characters.len();
        } else {
            replaced.push(characters[index]);
            index += 1;
        }
    }
    replaced
}

fn lowercase_character(character: char) -> char {
    character.to_lowercase().next().unwrap_or(character)
}

/// A drive or network path keeps only its last part, so a folder name that happens to be the
/// person's (`D:\\work\\an\\notes.txt`) does not travel.
pub fn hide_paths(text: &str) -> String {
    text.split(' ')
        .map(|word| {
            if looks_like_path(word) {
                word.rsplit(['\\', '/']).next().unwrap_or("")
            } else {
                word
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn looks_like_path(word: &str) -> bool {
    let bytes = word.as_bytes();
    let drive_path = bytes.len() > 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && (bytes[2] == b'\\' || bytes[2] == b'/');
    drive_path || word.starts_with("\\\\")
}

/// What the Worker accepts: printable ASCII. A localized system message or a letter that slipped
/// in becomes `?`.
pub fn ascii_only(text: &str) -> String {
    text.chars()
        .map(|character| {
            if character.is_ascii_graphic() || character == ' ' {
                character
            } else {
                '?'
            }
        })
        .collect()
}

/// The newest log lines about a crash or a failure, scrubbed.
pub fn problem_lines(log: &str, identity: &Identity) -> Vec<String> {
    let problems: Vec<&str> = log.lines().filter(|line| is_problem(line)).collect();
    let first_kept = problems.len().saturating_sub(MAX_LOG_LINES);
    problems[first_kept..]
        .iter()
        .map(|line| shape_log_line(line, identity))
        .collect()
}

/// The openings of log lines that may be sent. Every other line that matched `is_problem` is
/// replaced by a marker, so the privacy of a report does not depend on what a future
/// `app_log::write` call says.
pub const KNOWN_LOG_PREFIXES: [&str; 7] = [
    "crash in ",
    "update check failed: ",
    "report failed: ",
    "update-failed: ",
    "error: ",
    "update to ",
    "mode effect failed: ",
];

const WITHHELD_LINE: &str = "(line withheld)";

/// `<unix seconds> <message>` kept when the message opens as a known line does, else withheld.
pub fn shape_log_line(line: &str, identity: &Identity) -> String {
    let (timestamp, message) = line.split_once(' ').unwrap_or((line, ""));
    let timestamp_is_plain = !timestamp.is_empty()
        && timestamp.len() <= 11
        && timestamp.bytes().all(|byte| byte.is_ascii_digit());
    if !timestamp_is_plain {
        return WITHHELD_LINE.to_string();
    }
    let known = KNOWN_LOG_PREFIXES
        .iter()
        .any(|prefix| message.starts_with(prefix));
    if !known {
        return format!("{timestamp} {WITHHELD_LINE}");
    }
    let scrubbed = scrub_text(message, identity, MAX_LINE_CHARACTERS);
    format!("{timestamp} {}", ascii_only(&hide_paths(&scrubbed)))
}

fn is_problem(line: &str) -> bool {
    ["crash in", "failed", "error"]
        .iter()
        .any(|word| line.contains(word))
}

/// `crates/oh-key-app/src/hook.rs:12:5` from a build machine's absolute source path: the part
/// from `crates` on, else the last two folders. A path can hold the builder's user name.
pub fn short_source_path(path: &str) -> String {
    let parts: Vec<&str> = path
        .split(['\\', '/'])
        .filter(|part| !part.is_empty())
        .collect();
    let start = parts
        .iter()
        .rposition(|part| *part == "crates")
        .unwrap_or_else(|| parts.len().saturating_sub(3));
    parts[start..].join("/")
}

/// Function names of ours from a printed backtrace, without addresses, hash suffixes or the
/// file paths on the `at` lines.
pub fn own_function_names(backtrace_text: &str) -> Vec<String> {
    backtrace_text
        .lines()
        .filter_map(|line| {
            let (_, name) = line.trim().split_once(": ")?;
            let name = strip_hash_suffix(name.trim());
            (name.starts_with("oh_key") && is_plain_function_path(name)).then(|| name.to_string())
        })
        .take(12)
        .collect()
}

/// `oh_key_app::hook::run::{{closure}}`: segments of letters, digits, `_`, `{` and `}` only.
fn is_plain_function_path(name: &str) -> bool {
    name.split("::").all(|segment| {
        !segment.is_empty()
            && segment
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '{' | '}'))
    })
}

fn strip_hash_suffix(name: &str) -> &str {
    match name.rsplit_once("::h") {
        Some((head, hash)) if hash.len() == 16 && hash.chars().all(|c| c.is_ascii_hexdigit()) => {
            head
        }
        _ => name,
    }
}

// ---- rate limit and dedupe -------------------------------------------------------------

#[derive(Debug, PartialEq, Eq)]
pub enum Decision {
    Send,
    Duplicate,
    DailyLimit,
}

/// At most five reports a day, and the same signature at most once in 24 hours. Kept in a
/// small text file so a restart does not reset it.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct RateLimiter {
    day: u64,
    sent_today: u32,
    recent: Vec<(String, u64)>,
}

impl RateLimiter {
    pub fn parse(text: &str) -> RateLimiter {
        let mut limiter = RateLimiter::default();
        for line in text.lines() {
            let parts: Vec<&str> = line.split_whitespace().collect();
            match parts.as_slice() {
                ["day", day, sent] => {
                    limiter.day = day.parse().unwrap_or(0);
                    limiter.sent_today = sent.parse().unwrap_or(0);
                }
                ["sig", signature, sent_at] => {
                    if let Ok(sent_at) = sent_at.parse() {
                        limiter.recent.push((signature.to_string(), sent_at));
                    }
                }
                _ => {}
            }
        }
        limiter
    }

    pub fn serialize(&self) -> String {
        let mut text = format!("day {} {}\n", self.day, self.sent_today);
        for (signature, sent_at) in &self.recent {
            text.push_str(&format!("sig {signature} {sent_at}\n"));
        }
        text
    }

    pub fn try_acquire(&mut self, now_seconds: u64, signature: &str, daily_limit: u32) -> Decision {
        let today = now_seconds / SECONDS_PER_DAY;
        if today != self.day {
            self.day = today;
            self.sent_today = 0;
        }
        self.recent
            .retain(|(_, sent_at)| now_seconds.saturating_sub(*sent_at) < DEDUPE_SECONDS);
        if self.recent.iter().any(|(known, _)| known == signature) {
            return Decision::Duplicate;
        }
        if self.sent_today >= daily_limit {
            return Decision::DailyLimit;
        }
        self.sent_today += 1;
        self.recent.push((signature.to_string(), now_seconds));
        Decision::Send
    }

    /// A send that failed on the network does not use up the day's reports.
    pub fn refund(&mut self, signature: &str) {
        let before = self.recent.len();
        self.recent.retain(|(known, _)| known != signature);
        if self.recent.len() < before {
            self.sent_today = self.sent_today.saturating_sub(1);
        }
    }
}

/// FNV-1a over the version, kind and message: the same fault in the same version always
/// gets the same signature.
pub fn signature_of(version: &str, kind: EventKind, message: &str) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    let text = format!("{version}|{}|{message}", kind.name());
    for byte in text.bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    format!("{hash:016x}")
}

// ---- JSON ------------------------------------------------------------------------------

pub fn json_string(text: &str) -> String {
    let mut quoted = String::with_capacity(text.len() + 2);
    quoted.push('"');
    for character in text.chars() {
        match character {
            '"' => quoted.push_str("\\\""),
            '\\' => quoted.push_str("\\\\"),
            '\n' => quoted.push_str("\\n"),
            '\r' => quoted.push_str("\\r"),
            '\t' => quoted.push_str("\\t"),
            control if (control as u32) < 0x20 => {
                quoted.push_str(&format!("\\u{:04x}", control as u32));
            }
            other => quoted.push(other),
        }
    }
    quoted.push('"');
    quoted
}

fn json_string_list(items: &[String]) -> String {
    let quoted: Vec<String> = items.iter().map(|item| json_string(item)).collect();
    format!("[{}]", quoted.join(","))
}

/// Everything about this machine and build that a report may carry.
pub struct Environment {
    pub version: String,
    pub os_version: String,
    pub architecture: String,
    pub install_id: String,
}

/// A panic summary reads `panic at crates/x.rs:1:2: message`; the Worker takes the place and the
/// message as two fields. A place outside our own `crates/` folder is dropped.
pub fn split_source_location(message: &str) -> (String, Option<String>) {
    let Some(rest) = message.strip_prefix("panic at ") else {
        return (message.to_string(), None);
    };
    let Some((place, text)) = rest.split_once(": ") else {
        return (message.to_string(), None);
    };
    let place = place.starts_with("crates/").then(|| place.to_string());
    (format!("panic: {text}"), place)
}

/// The one report format both platforms send and the Worker accepts (the same field names as
/// `server/report-worker/src/schema.ts`; `fixtures/windows-report.json` pins it on both sides).
pub fn build_payload(
    event: &ReportEvent,
    environment: &Environment,
    recent_log: &[String],
) -> String {
    let (message, source_location) = split_source_location(&event.message);
    let mut fields = vec![
        format!("\"app\":{}", json_string("oh-key")),
        format!("\"app_version\":{}", json_string(&environment.version)),
        format!("\"platform\":{}", json_string("windows")),
        format!("\"os_version\":{}", json_string(&environment.os_version)),
        format!("\"arch\":{}", json_string(&environment.architecture)),
        format!("\"install_id\":{}", json_string(&environment.install_id)),
        format!("\"kind\":{}", json_string(event.kind.name())),
        format!("\"message\":{}", json_string(&message)),
    ];
    if let Some(place) = source_location {
        fields.push(format!("\"source_location\":{}", json_string(&place)));
    }
    if !event.frames.is_empty() {
        fields.push(format!("\"backtrace\":{}", json_string_list(&event.frames)));
    }
    if let Some(count) = event.count {
        fields.push(format!("\"counts\":{{\"count\":{count}}}"));
    }
    if !recent_log.is_empty() {
        fields.push(format!("\"log_lines\":{}", json_string_list(recent_log)));
    }
    format!("{{{}}}", fields.join(","))
}

// ---- install id ------------------------------------------------------------------------

/// A random (version 4) UUID from 16 random bytes.
pub fn uuid_v4_from_bytes(mut bytes: [u8; 16]) -> String {
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    let hex: Vec<String> = bytes.iter().map(|byte| format!("{byte:02x}")).collect();
    format!(
        "{}-{}-{}-{}-{}",
        hex[0..4].concat(),
        hex[4..6].concat(),
        hex[6..8].concat(),
        hex[8..10].concat(),
        hex[10..16].concat()
    )
}

fn looks_like_uuid(text: &str) -> bool {
    text.len() == 36
        && text.char_indices().all(|(index, character)| {
            if matches!(index, 8 | 13 | 18 | 23) {
                character == '-'
            } else {
                character.is_ascii_hexdigit()
            }
        })
}

/// The id kept in the data folder; deleting the file gives the install a new one.
fn install_id() -> Option<String> {
    let path = app_log::data_folder().join(INSTALL_ID_FILE);
    if let Ok(text) = std::fs::read_to_string(&path)
        && looks_like_uuid(text.trim())
    {
        return Some(text.trim().to_ascii_lowercase());
    }
    let bytes = http::random_bytes::<16>()?;
    let id = uuid_v4_from_bytes(bytes);
    let _ = std::fs::create_dir_all(app_log::data_folder());
    let _ = std::fs::write(&path, &id);
    Some(id)
}

fn windows_version() -> String {
    use windows::Wdk::System::SystemServices::RtlGetVersion;
    use windows::Win32::System::SystemInformation::OSVERSIONINFOW;
    let mut info = OSVERSIONINFOW {
        dwOSVersionInfoSize: size_of::<OSVERSIONINFOW>() as u32,
        ..Default::default()
    };
    let status = unsafe { RtlGetVersion(&mut info) };
    if status.is_ok() {
        format!(
            "Windows {}.{}.{}",
            info.dwMajorVersion, info.dwMinorVersion, info.dwBuildNumber
        )
    } else {
        "Windows".to_string()
    }
}

// ---- the reporter thread ---------------------------------------------------------------

static SENDER: OnceLock<Sender<ReportEvent>> = OnceLock::new();

/// Starts the thread that logs and sends events. Call once.
pub fn start() {
    let (sender, receiver) = channel::<ReportEvent>();
    if SENDER.set(sender).is_err() {
        return;
    }
    let spawned = std::thread::Builder::new()
        .name("oh-key-report".to_owned())
        .spawn(move || {
            while let Ok(event) = receiver.recv() {
                handle(&event);
            }
        });
    if spawned.is_err() {
        app_log::write("report thread did not start");
    }
}

/// Hands an event to the reporter. Never blocks; without a started reporter it is dropped.
pub fn submit(event: ReportEvent) {
    if let Some(sender) = SENDER.get() {
        let _ = sender.send(event);
    }
}

/// Logs and sends one event on the calling thread, for a process that is about to exit.
pub fn send_now(event: ReportEvent) {
    handle(&event);
}

fn handle(event: &ReportEvent) {
    let identity = Identity::from_environment();
    let scrubbed = scrub_text(&event.message, &identity, MAX_MESSAGE_CHARACTERS);
    let message = ascii_only(&hide_paths(&scrubbed));
    if event.write_to_log {
        app_log::write(&format!("{}: {message}", event.kind.name()));
    }
    if !oh_key_ipc::reports_allowed() {
        return;
    }
    if !endpoint_is_configured() {
        return;
    }
    let event = ReportEvent {
        message,
        ..event.clone()
    };
    send(&event, &identity);
}

fn endpoint_is_configured() -> bool {
    is_configured(REPORT_ENDPOINT)
}

fn is_configured(endpoint: &str) -> bool {
    endpoint.starts_with("https://") && !endpoint.contains("REPLACE_ME")
}

fn send(event: &ReportEvent, identity: &Identity) {
    let version = env!("CARGO_PKG_VERSION");
    let signature = signature_of(version, event.kind, &event.message);
    let now = app_log::unix_seconds_now();
    let state_path = app_log::data_folder().join(STATE_FILE);
    let mut limiter = RateLimiter::parse(&std::fs::read_to_string(&state_path).unwrap_or_default());
    match limiter.try_acquire(now, &signature, event.kind.daily_limit()) {
        Decision::Send => {}
        Decision::Duplicate => {
            app_log::write("report not sent: same problem already reported today");
            return;
        }
        Decision::DailyLimit => {
            app_log::write("report not sent: daily limit reached");
            return;
        }
    }
    save_state(&state_path, &limiter);
    let Some(install_id) = install_id() else {
        app_log::write("report not sent: no install id could be made");
        limiter.refund(&signature);
        save_state(&state_path, &limiter);
        return;
    };
    let environment = Environment {
        version: version.to_string(),
        os_version: windows_version(),
        architecture: std::env::consts::ARCH.to_string(),
        install_id,
    };
    let recent_log = problem_lines(&app_log::read_all(), identity);
    let payload = build_payload(event, &environment, &recent_log);
    match http::post_json(REPORT_ENDPOINT, &payload) {
        Ok(response) if (200..300).contains(&response.status) => {
            app_log::write("report sent");
        }
        Ok(response) => {
            app_log::write(&format!(
                "report failed: the server answered {}",
                response.status
            ));
            if worth_retrying(response.status) {
                limiter.refund(&signature);
                save_state(&state_path, &limiter);
            }
        }
        Err(error) => {
            app_log::write(&format!("report failed: {error}"));
            limiter.refund(&signature);
            save_state(&state_path, &limiter);
        }
    }
}

/// Only a server fault gives the report back to the day's budget. A 4xx answer (a refused body,
/// or 429 when the server is full) would refuse the same report again, so retrying it on every
/// later event only adds load.
pub fn worth_retrying(status: u32) -> bool {
    status >= 500
}

fn save_state(path: &PathBuf, limiter: &RateLimiter) {
    let _ = std::fs::create_dir_all(app_log::data_folder());
    let _ = std::fs::write(path, limiter.serialize());
}

/// Lets the keyboard hook report what it notices without knowing about the reporter.
pub fn connect_hook() {
    crate::hook::set_diagnostic_handler(forward_hook_diagnostic);
}

/// One reinstall report per run tells the story; the rest of a run's reinstalls are the same
/// problem and stay in the log only.
static HOOK_REINSTALL_REPORTED: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

fn forward_hook_diagnostic(diagnostic: crate::hook::HookDiagnostic) {
    use crate::hook::HookDiagnostic;
    use std::sync::atomic::Ordering;
    if matches!(diagnostic, HookDiagnostic::Reinstalled)
        && HOOK_REINSTALL_REPORTED.swap(true, Ordering::Relaxed)
    {
        app_log::write("hook reinstalled again; not reported");
        return;
    }
    let event = match diagnostic {
        HookDiagnostic::Reinstalled => ReportEvent::new(
            EventKind::HookReinstalled,
            "the keyboard hook was reinstalled",
        ),
        HookDiagnostic::BreakerTripped { total } => ReportEvent {
            count: Some(total.min(u32::MAX as usize) as u32),
            ..ReportEvent::new(
                EventKind::BreakerTripped,
                "the circuit breaker stopped an edit loop",
            )
        },
        HookDiagnostic::SendInputShort { sent, expected } => ReportEvent {
            count: Some(sent as u32),
            ..ReportEvent::new(
                EventKind::Error,
                &format!("SendInput took {sent} of {expected} events"),
            )
        },
    };
    submit(event);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn someone() -> Identity {
        Identity {
            home: Some(r"C:\Users\Someone".to_string()),
            user_name: Some("Someone".to_string()),
            computer_name: Some("DESKTOP-ABC123".to_string()),
        }
    }

    #[test]
    fn only_a_server_fault_is_retried() {
        assert!(worth_retrying(500));
        assert!(worth_retrying(503));
        assert!(!worth_retrying(429));
        assert!(!worth_retrying(400));
        assert!(!worth_retrying(413));
    }

    #[test]
    fn the_home_folder_never_reaches_a_report() {
        let line = r"7 could not edit C:\Users\Someone\Pictures\a.png, kept c:\users\someone\b.png";
        assert_eq!(
            scrub_text(line, &someone(), 300),
            r"7 could not edit ~\Pictures\a.png, kept ~\b.png"
        );
        assert_eq!(scrub_text("C:/Users/Someone/x", &someone(), 300), "~/x");
    }

    #[test]
    fn the_user_and_computer_names_are_taken_out_wherever_they_sit() {
        let scrubbed = scrub_text(
            r"8 failed: D:\work\C--Users-Someone-Documents on DESKTOP-ABC123",
            &someone(),
            300,
        );
        assert_eq!(scrubbed, r"8 failed: D:\work\C--Users-user-Documents on pc");
        assert!(!scrubbed.to_ascii_lowercase().contains("someone"));
    }

    #[test]
    fn a_very_short_name_is_left_alone_so_ordinary_words_survive() {
        let identity = Identity {
            home: None,
            user_name: Some("an".to_string()),
            computer_name: None,
        };
        assert_eq!(
            scrub_text("a panic in the main thread", &identity, 300),
            "a panic in the main thread"
        );
    }

    #[test]
    fn scrubbing_cuts_long_text_and_flattens_line_breaks() {
        let scrubbed = scrub_text(&format!("a\nb{}", "x".repeat(500)), &someone(), 50);
        assert_eq!(scrubbed.chars().count(), 50);
        assert!(!scrubbed.contains('\n'));
    }

    #[test]
    fn only_problem_lines_are_kept_and_only_the_newest() {
        let mut log = String::from("1 started\n2 updating to 0.13.0\n");
        for number in 0..12 {
            log.push_str(&format!("{number} update check failed: reason {number}\n"));
        }
        log.push_str("99 crash in 0.1.0: exception 0xc0000005 at oh-key.exe+0x1234\n");
        let lines = problem_lines(&log, &Identity::default());
        assert_eq!(lines.len(), MAX_LOG_LINES);
        assert_eq!(lines[0], "5 update check failed: reason 5");
        assert!(lines[MAX_LOG_LINES - 1].starts_with("99 crash in"));
    }

    #[test]
    fn a_source_path_loses_the_build_machines_folders() {
        assert_eq!(
            short_source_path(r"C:\Users\Builder\code\oh-key\crates\oh-key-app\src\hook.rs"),
            "crates/oh-key-app/src/hook.rs"
        );
        assert_eq!(
            short_source_path(r"C:\Users\Builder\.cargo\registry\src\x\lib.rs"),
            "src/x/lib.rs"
        );
    }

    #[test]
    fn a_backtrace_is_reduced_to_our_own_function_names() {
        let backtrace = "   0: std::backtrace::Backtrace::force_capture\n\
                         \x20  1: oh_key_app::hook::handle_event::h0123456789abcdef\n\
                         \x20            at C:\\Users\\Someone\\oh-key\\crates\\hook.rs:12\n\
                         \x20  2: oh_key_engine::session::push\n\
                         \x20  3: core::ops::function::FnOnce::call_once\n";
        assert_eq!(
            own_function_names(backtrace),
            [
                "oh_key_app::hook::handle_event",
                "oh_key_engine::session::push"
            ]
        );
    }

    #[test]
    fn the_limiter_allows_five_a_day_then_stops() {
        let mut limiter = RateLimiter::default();
        let now = 20_000 * SECONDS_PER_DAY + 100;
        for number in 0..5 {
            assert_eq!(
                limiter.try_acquire(now + number, &format!("sig{number}"), MAX_REPORTS_PER_DAY),
                Decision::Send
            );
        }
        assert_eq!(
            limiter.try_acquire(now + 10, "sig-new", MAX_REPORTS_PER_DAY),
            Decision::DailyLimit
        );
    }

    #[test]
    fn the_next_day_starts_a_fresh_count() {
        let mut limiter = RateLimiter::default();
        let now = 20_000 * SECONDS_PER_DAY + 100;
        for number in 0..5 {
            limiter.try_acquire(now, &format!("sig{number}"), MAX_REPORTS_PER_DAY);
        }
        let tomorrow = now + SECONDS_PER_DAY;
        assert_eq!(
            limiter.try_acquire(tomorrow, "fresh", MAX_REPORTS_PER_DAY),
            Decision::Send
        );
    }

    #[test]
    fn the_same_signature_is_sent_once_in_24_hours() {
        let mut limiter = RateLimiter::default();
        let now = 20_000 * SECONDS_PER_DAY + 100;
        assert_eq!(limiter.try_acquire(now, "same", 5), Decision::Send);
        assert_eq!(
            limiter.try_acquire(now + 60, "same", 5),
            Decision::Duplicate
        );
        assert_eq!(
            limiter.try_acquire(now + DEDUPE_SECONDS - 1, "same", 5),
            Decision::Duplicate
        );
        assert_eq!(
            limiter.try_acquire(now + DEDUPE_SECONDS, "same", 5),
            Decision::Send
        );
    }

    #[test]
    fn a_duplicate_does_not_use_up_the_day() {
        let mut limiter = RateLimiter::default();
        let now = 20_000 * SECONDS_PER_DAY + 100;
        limiter.try_acquire(now, "same", 5);
        for _ in 0..10 {
            limiter.try_acquire(now, "same", 5);
        }
        assert_eq!(limiter.sent_today, 1);
    }

    #[test]
    fn noisy_kinds_leave_room_for_a_crash() {
        let mut limiter = RateLimiter::default();
        let now = 20_000 * SECONDS_PER_DAY + 100;
        let noisy = EventKind::BreakerTripped.daily_limit();
        for number in 0..noisy {
            assert_eq!(
                limiter.try_acquire(now, &format!("noise{number}"), noisy),
                Decision::Send
            );
        }
        assert_eq!(
            limiter.try_acquire(now, "more-noise", noisy),
            Decision::DailyLimit
        );
        assert_eq!(
            limiter.try_acquire(now, "crash", EventKind::Crash.daily_limit()),
            Decision::Send
        );
    }

    #[test]
    fn a_failed_send_is_refunded() {
        let mut limiter = RateLimiter::default();
        let now = 20_000 * SECONDS_PER_DAY + 100;
        limiter.try_acquire(now, "offline", 5);
        limiter.refund("offline");
        assert_eq!(limiter.sent_today, 0);
        assert_eq!(limiter.try_acquire(now + 5, "offline", 5), Decision::Send);
    }

    #[test]
    fn the_limiter_state_survives_a_round_trip_through_its_file() {
        let mut limiter = RateLimiter::default();
        let now = 20_000 * SECONDS_PER_DAY + 100;
        limiter.try_acquire(now, "abc", 5);
        limiter.try_acquire(now + 1, "def", 5);
        assert_eq!(RateLimiter::parse(&limiter.serialize()), limiter);
        assert_eq!(
            RateLimiter::parse("garbage\nsig only\nday x y\n"),
            RateLimiter::default()
        );
    }

    #[test]
    fn the_signature_is_stable_and_depends_on_version_kind_and_message() {
        let base = signature_of("0.1.0", EventKind::Crash, "boom");
        assert_eq!(base, signature_of("0.1.0", EventKind::Crash, "boom"));
        assert_ne!(base, signature_of("0.1.1", EventKind::Crash, "boom"));
        assert_ne!(base, signature_of("0.1.0", EventKind::Panic, "boom"));
        assert_ne!(base, signature_of("0.1.0", EventKind::Crash, "bang"));
        assert_eq!(base.len(), 16);
    }

    #[test]
    fn json_strings_escape_quotes_slashes_and_control_characters() {
        assert_eq!(json_string("a\"b\\c\nd\u{1}"), r#""a\"b\\c\nd\u0001""#);
        assert_eq!(json_string("tiếng Việt"), "\"tiếng Việt\"");
    }

    #[test]
    fn the_payload_is_the_wire_format_the_worker_checks() {
        let fixture = include_str!("../../../server/report-worker/fixtures/windows-report.json");
        let event = ReportEvent {
            frames: vec!["oh_key_app::hook::run".to_string()],
            count: Some(2),
            ..ReportEvent::new(
                EventKind::Panic,
                "panic at crates/oh-key-app/src/hook.rs:12:5: index out of bounds",
            )
        };
        let environment = Environment {
            version: "0.1.0".to_string(),
            os_version: "Windows 10.0.26300".to_string(),
            architecture: "x86_64".to_string(),
            install_id: "11111111-2222-4333-8444-555555555555".to_string(),
        };
        let payload = build_payload(
            &event,
            &environment,
            &["1700000000 crash in 0.1.0: exception 0xc0000005 at oh-key.exe+0x10".to_string()],
        );
        assert_eq!(payload, fixture.trim());
    }

    #[test]
    fn a_panic_summary_splits_into_message_and_place() {
        assert_eq!(
            split_source_location("panic at crates/a/src/b.rs:1:2: index out of bounds"),
            (
                "panic: index out of bounds".to_string(),
                Some("crates/a/src/b.rs:1:2".to_string())
            )
        );
        assert_eq!(
            split_source_location("panic at library/core/src/x.rs:1:2: capacity overflow"),
            ("panic: capacity overflow".to_string(), None)
        );
        assert_eq!(
            split_source_location("exception 0x1 at oh-key.exe+0x2"),
            ("exception 0x1 at oh-key.exe+0x2".to_string(), None)
        );
    }

    #[test]
    fn only_lines_of_a_known_shape_leave_the_machine() {
        let identity = someone();
        assert_eq!(
            shape_log_line(
                "1700000000 crash in 0.1.0: panic: index out of bounds",
                &identity
            ),
            "1700000000 crash in 0.1.0: panic: index out of bounds"
        );
        assert_eq!(
            shape_log_line("1700000000 something failed with xin chao", &identity),
            "1700000000 (line withheld)"
        );
        assert_eq!(
            shape_log_line("no timestamp failed", &identity),
            "(line withheld)"
        );
        assert_eq!(
            shape_log_line(
                "1700000000 update check failed: Kh\u{f4}ng k\u{1ebf}t n\u{1ed1}i",
                &identity
            ),
            "1700000000 update check failed: Kh?ng k?t n?i"
        );
    }

    #[test]
    fn a_path_in_a_log_line_keeps_only_its_file_name() {
        assert_eq!(
            hide_paths(r"could not open D:\work\an\notes.txt now"),
            "could not open notes.txt now"
        );
        assert_eq!(hide_paths(r"\\server\share\a.log"), "a.log");
        assert_eq!(hide_paths("a plain sentence"), "a plain sentence");
    }

    #[test]
    fn names_are_matched_whatever_the_case_or_script() {
        let identity = Identity {
            home: Some("C:\\Users\\Nguy\u{1ec5}n An".to_string()),
            user_name: Some("Nguy\u{1ec5}n An".to_string()),
            computer_name: None,
        };
        assert_eq!(
            scrub_text("by NGUY\u{1ec4}N AN today", &identity, 100),
            "by user today"
        );
    }

    #[test]
    fn the_short_form_of_a_long_profile_folder_is_scrubbed() {
        let identity = Identity {
            home: None,
            user_name: Some("Nguyenxuanan".to_string()),
            computer_name: None,
        };
        assert_eq!(
            scrub_text(r"C:\Users\NGUYEN~1\a.txt", &identity, 100),
            r"C:\Users\user\a.txt"
        );
    }

    #[test]
    fn a_backtrace_keeps_only_plain_paths_of_ours() {
        assert!(is_plain_function_path("oh_key_app::hook::run::{{closure}}"));
        assert!(!is_plain_function_path("oh_key_app::hook::run<T>"));
        assert!(!is_plain_function_path("oh_key_app::"));
    }

    #[test]
    fn every_log_call_that_can_carry_a_problem_word_has_a_known_shape() {
        // A call whose text is a variable cannot be read here. The count below is the number of
        // such calls that exist; a new one fails this test until its text has been checked.
        const CALLS_WITH_A_VARIABLE_TEXT: usize = 3;
        let needle = concat!("app_log::", "write(");
        let folder = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut variable_calls = 0;
        for entry in std::fs::read_dir(folder).unwrap() {
            let path = entry.unwrap().path();
            if path.extension().is_none_or(|extension| extension != "rs") {
                continue;
            }
            let source = std::fs::read_to_string(&path).unwrap();
            for (index, _) in source.match_indices(needle) {
                match leading_literal(&source[index + needle.len()..]) {
                    None => variable_calls += 1,
                    Some(text) if is_problem(&without_placeholders(&text)) => assert!(
                        KNOWN_LOG_PREFIXES
                            .iter()
                            .any(|prefix| text.starts_with(prefix)),
                        "{} writes a problem line with no known opening: {text}",
                        path.display()
                    ),
                    Some(_) => {}
                }
            }
        }
        assert_eq!(variable_calls, CALLS_WITH_A_VARIABLE_TEXT);
    }

    /// `"failed: {error}"` -> `"failed: "`: a placeholder's name is not the logged text.
    fn without_placeholders(text: &str) -> String {
        let mut kept = String::new();
        let mut inside = false;
        for character in text.chars() {
            match character {
                '{' => inside = true,
                '}' => inside = false,
                other if !inside => kept.push(other),
                _ => {}
            }
        }
        kept
    }

    /// The string literal a call starts with, `"text"` or `&format!("text", ..)`.
    fn leading_literal(call: &str) -> Option<String> {
        let call = call.trim_start();
        let call = call.strip_prefix("&format!(").unwrap_or(call).trim_start();
        let call = call.strip_prefix('"')?;
        let mut text = String::new();
        let mut characters = call.chars();
        while let Some(character) = characters.next() {
            match character {
                '"' => return Some(text),
                '\\' => {
                    characters.next();
                }
                other => text.push(other),
            }
        }
        None
    }

    #[test]
    fn the_install_id_is_a_version_4_uuid() {
        let id = uuid_v4_from_bytes([0xff; 16]);
        assert_eq!(id, "ffffffff-ffff-4fff-bfff-ffffffffffff");
        assert!(looks_like_uuid(&id));
        assert!(!looks_like_uuid("not-a-uuid"));
    }

    #[test]
    fn nothing_is_sent_to_a_placeholder_endpoint() {
        assert!(!is_configured(
            "https://oh-key-report.REPLACE_ME.workers.dev/v1/report"
        ));
        assert!(!is_configured("http://example.com/v1/report"));
        assert!(is_configured(
            "https://oh-key-report.example.workers.dev/v1/report"
        ));
        assert!(REPORT_ENDPOINT.starts_with("https://"));
    }
}
