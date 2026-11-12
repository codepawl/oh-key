//! The update checks, off the hook thread and off the window thread: a worker thread sleeps
//! on a channel until the first check (10 seconds after start), then until the next one (every
//! 6 hours) or a command from the menu or the settings window. It wakes for nothing else.
//!
//! The worker never touches a window. It queues a `Notice` and posts a message to the tray
//! window, which shows a balloon or restarts into the new version.

use std::sync::atomic::{AtomicBool, AtomicIsize, AtomicU32, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use oh_key_ipc::{UpdateMode, load_settings};
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::System::SystemInformation::GetTickCount;
use windows::Win32::UI::Input::KeyboardAndMouse::{GetLastInputInfo, LASTINPUTINFO};
use windows::Win32::UI::WindowsAndMessaging::PostMessageW;

use crate::report::{EventKind, ReportEvent};
use crate::{app_log, report, updater};

const FIRST_CHECK_DELAY: Duration = Duration::from_secs(10);
const CHECK_INTERVAL: Duration = Duration::from_secs(6 * 60 * 60);
/// The keyboard and mouse must have rested this long before the exe is swapped, so no word
/// is cut in half by the restart.
const QUIET_BEFORE_SWAP_MILLISECONDS: u32 = 5_000;
const QUIET_POLL: Duration = Duration::from_secs(2);
/// A person who types without a pause for this long is left alone until the next check.
const QUIET_WAIT_LIMIT: Duration = Duration::from_secs(3 * 60);

/// What the tray should tell the user, or do.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Notice {
    UpToDate,
    /// A newer version exists and was not installed (notify mode, or a manual check).
    Available(String),
    /// The new exe is in place; the tray restarts into it.
    Installed(String),
    Failed(String),
}

enum Command {
    CheckNow,
    InstallPending,
}

static COMMANDS: OnceLock<Sender<Command>> = OnceLock::new();
static NOTICES: Mutex<Vec<Notice>> = Mutex::new(Vec::new());
static PENDING: Mutex<Option<updater::Release>> = Mutex::new(None);
static TRAY_WINDOW: AtomicIsize = AtomicIsize::new(0);
static NOTICE_MESSAGE: AtomicU32 = AtomicU32::new(0);
/// A manual check was asked for and has not produced a notice yet.
static CHECKING: AtomicBool = AtomicBool::new(false);

/// Starts the worker. `notice_message` is posted to `tray_window` whenever a notice is queued.
pub fn start(tray_window: HWND, notice_message: u32) {
    let (sender, receiver) = mpsc::channel();
    if COMMANDS.set(sender).is_err() {
        return;
    }
    TRAY_WINDOW.store(tray_window.0 as isize, Ordering::Relaxed);
    NOTICE_MESSAGE.store(notice_message, Ordering::Relaxed);
    let spawned = std::thread::Builder::new()
        .name("oh-key-update".to_owned())
        .spawn(move || run(receiver));
    if spawned.is_err() {
        app_log::write("update thread did not start");
    }
}

/// A manual check, from the menu or the settings window.
pub fn check_now() {
    if COMMANDS.get().is_some() {
        CHECKING.store(true, Ordering::Relaxed);
    }
    send(Command::CheckNow);
}

/// Whether a manual check is still running, for the settings window's button.
pub fn checking() -> bool {
    CHECKING.load(Ordering::Relaxed)
}

/// Installs the release a notice announced.
pub fn install_pending() {
    send(Command::InstallPending);
}

fn send(command: Command) {
    if let Some(sender) = COMMANDS.get() {
        let _ = sender.send(command);
    }
}

/// The version waiting to be installed, for the tray menu.
pub fn pending_version() -> Option<String> {
    let pending = PENDING.lock().ok()?;
    pending.as_ref().map(|release| release.version.clone())
}

pub fn take_notices() -> Vec<Notice> {
    NOTICES
        .lock()
        .map(|mut notices| std::mem::take(&mut *notices))
        .unwrap_or_default()
}

fn post_notice(notice: Notice) {
    CHECKING.store(false, Ordering::Relaxed);
    if let Ok(mut notices) = NOTICES.lock() {
        notices.push(notice);
    }
    let raw = TRAY_WINDOW.load(Ordering::Relaxed);
    if raw == 0 {
        return;
    }
    let window = HWND(raw as *mut _);
    let _ = unsafe {
        PostMessageW(
            Some(window),
            NOTICE_MESSAGE.load(Ordering::Relaxed),
            WPARAM(0),
            LPARAM(0),
        )
    };
}

/// What to do about a newer release.
#[derive(Debug, PartialEq, Eq)]
enum Action {
    InstallNow,
    Announce,
    /// Already announced once; the user is not nagged every 6 hours.
    Stay,
}

fn action_for_available(mode: UpdateMode, manual: bool, already_announced: bool) -> Action {
    match mode {
        UpdateMode::Auto => Action::InstallNow,
        UpdateMode::Notify | UpdateMode::Off if manual || !already_announced => Action::Announce,
        UpdateMode::Notify | UpdateMode::Off => Action::Stay,
    }
}

/// The scheduled check respects the setting; only a manual check runs when it is off.
fn scheduled_check_wanted(mode: UpdateMode) -> bool {
    mode != UpdateMode::Off
}

/// True once the last keyboard or mouse input is at least `needed` milliseconds old. Tick
/// counts wrap after 49 days, which the subtraction handles.
fn quiet_for(now_tick: u32, last_input_tick: u32, needed: u32) -> bool {
    now_tick.wrapping_sub(last_input_tick) >= needed
}

fn input_is_quiet() -> bool {
    let mut info = LASTINPUTINFO {
        cbSize: size_of::<LASTINPUTINFO>() as u32,
        dwTime: 0,
    };
    if !unsafe { GetLastInputInfo(&mut info) }.as_bool() {
        return true;
    }
    quiet_for(
        unsafe { GetTickCount() },
        info.dwTime,
        QUIET_BEFORE_SWAP_MILLISECONDS,
    )
}

fn run(receiver: mpsc::Receiver<Command>) {
    let mut next_scheduled = Instant::now() + FIRST_CHECK_DELAY;
    let mut announced_version: Option<String> = None;
    loop {
        let wait = next_scheduled.saturating_duration_since(Instant::now());
        match receiver.recv_timeout(wait) {
            Ok(Command::CheckNow) => {
                check_and_act(true, &mut announced_version);
                next_scheduled = Instant::now() + CHECK_INTERVAL;
            }
            Ok(Command::InstallPending) => install_pending_release(),
            Err(RecvTimeoutError::Timeout) => {
                if scheduled_check_wanted(load_settings().updates) {
                    check_and_act(false, &mut announced_version);
                }
                next_scheduled = Instant::now() + CHECK_INTERVAL;
            }
            Err(RecvTimeoutError::Disconnected) => return,
        }
    }
}

fn check_and_act(manual: bool, announced_version: &mut Option<String>) {
    let release = match updater::check() {
        Ok(updater::Check::Available(release)) => release,
        Ok(updater::Check::UpToDate) => {
            clear_pending();
            if manual {
                app_log::write("update check: up to date");
                post_notice(Notice::UpToDate);
            }
            return;
        }
        Err(error) => {
            app_log::write(&format!("update check failed: {error}"));
            if manual {
                post_notice(Notice::Failed(error));
            }
            return;
        }
    };
    let already_announced = announced_version.as_deref() == Some(release.version.as_str());
    let mode = load_settings().updates;
    match action_for_available(mode, manual, already_announced) {
        Action::InstallNow => install_release(&release, manual),
        Action::Announce => {
            app_log::write(&format!("update available: {}", release.version));
            *announced_version = Some(release.version.clone());
            let version = release.version.clone();
            set_pending(Some(release));
            post_notice(Notice::Available(version));
        }
        Action::Stay => set_pending(Some(release)),
    }
}

fn install_pending_release() {
    let release = PENDING.lock().ok().and_then(|pending| pending.clone());
    if let Some(release) = release {
        install_release(&release, true);
    }
}

fn set_pending(release: Option<updater::Release>) {
    if let Ok(mut pending) = PENDING.lock() {
        *pending = release;
    }
}

fn clear_pending() {
    set_pending(None);
}

fn install_release(release: &updater::Release, tell_failure: bool) {
    app_log::write(&format!("updating to {}", release.version));
    match download_and_swap(release) {
        Ok(()) => {
            app_log::write(&format!("update to {} is in place", release.version));
            clear_pending();
            post_notice(Notice::Installed(release.version.clone()));
        }
        Err(error) => {
            let message = format!("update to {} failed: {error}", release.version);
            app_log::write(&message);
            report::submit(ReportEvent {
                write_to_log: false,
                ..ReportEvent::new(EventKind::UpdateFailed, &message)
            });
            if tell_failure {
                post_notice(Notice::Failed(error));
            }
        }
    }
}

fn download_and_swap(release: &updater::Release) -> Result<(), String> {
    let new_exe = updater::download(release)?;
    // The wait comes before the file is written, so it never sits on disk unwatched.
    if !wait_for_quiet_input() {
        return Err("typing did not pause; will try again later".into());
    }
    // Tried first: a bad release must not cost the running program.
    let tried = updater::try_new_exe(&new_exe, &release.version)?;
    updater::install(tried)
}

/// Waits for a pause in typing; false when there was none within the limit.
fn wait_for_quiet_input() -> bool {
    let started = Instant::now();
    while !input_is_quiet() {
        if started.elapsed() > QUIET_WAIT_LIMIT {
            return false;
        }
        std::thread::sleep(QUIET_POLL);
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auto_installs_and_the_other_modes_only_announce() {
        assert_eq!(
            action_for_available(UpdateMode::Auto, false, false),
            Action::InstallNow
        );
        assert_eq!(
            action_for_available(UpdateMode::Auto, true, true),
            Action::InstallNow
        );
        assert_eq!(
            action_for_available(UpdateMode::Notify, false, false),
            Action::Announce
        );
        assert_eq!(
            action_for_available(UpdateMode::Off, true, false),
            Action::Announce
        );
    }

    #[test]
    fn a_version_is_announced_once_unless_the_user_asks_again() {
        assert_eq!(
            action_for_available(UpdateMode::Notify, false, true),
            Action::Stay
        );
        assert_eq!(
            action_for_available(UpdateMode::Notify, true, true),
            Action::Announce
        );
    }

    #[test]
    fn only_off_stops_the_scheduled_check() {
        assert!(scheduled_check_wanted(UpdateMode::Auto));
        assert!(scheduled_check_wanted(UpdateMode::Notify));
        assert!(!scheduled_check_wanted(UpdateMode::Off));
    }

    #[test]
    fn the_swap_waits_for_five_quiet_seconds_even_across_a_tick_wrap() {
        assert!(!quiet_for(10_000, 6_000, 5_000));
        assert!(quiet_for(11_000, 6_000, 5_000));
        let just_before_wrap = u32::MAX - 1_000;
        assert!(!quiet_for(2_000, just_before_wrap, 5_000));
        assert!(quiet_for(5_000, just_before_wrap, 5_000));
    }
}
