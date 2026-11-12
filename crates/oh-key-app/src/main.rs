//! `oh-key`: the tray app. It shows the Vietnamese/English mode, hosts the control panel and
//! types Vietnamese in every app through its keyboard hook.

#![cfg_attr(windows, windows_subsystem = "windows")]

#[cfg(windows)]
mod app_log;
#[cfg(windows)]
mod converter_window;
#[cfg(windows)]
mod crash;
#[cfg(windows)]
mod excluded_apps_window;
#[cfg(windows)]
mod graphics;
#[cfg(windows)]
mod hook;
#[cfg(windows)]
mod http;
#[cfg(windows)]
mod icons;
#[cfg(windows)]
mod layout;
#[cfg(windows)]
mod macros_window;
#[cfg(windows)]
mod mode_effects;
#[cfg(windows)]
mod mode_flash;
#[cfg(windows)]
mod mode_icon;
#[cfg(windows)]
mod offscreen;
#[cfg(windows)]
mod palette;
#[cfg(windows)]
mod persistence;
#[cfg(windows)]
mod render_png;
#[cfg(windows)]
mod report;
#[cfg(windows)]
mod settings_model;
#[cfg(windows)]
mod settings_window;
#[cfg(windows)]
mod sound;
#[cfg(windows)]
mod startup;
#[cfg(windows)]
mod style;
#[cfg(windows)]
mod tool_window;
#[cfg(windows)]
mod tray;
#[cfg(windows)]
mod update_service;
#[cfg(windows)]
mod updater;

#[cfg(windows)]
mod app {
    use std::path::PathBuf;

    use oh_key_ipc::{SharedState, instance_suffix, load_settings};
    use windows::Win32::Foundation::{CloseHandle, ERROR_ALREADY_EXISTS, GetLastError, HANDLE};
    use windows::Win32::System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx};
    use windows::Win32::System::Threading::CreateMutexW;
    use windows::Win32::UI::HiDpi::{
        DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, SetProcessDpiAwarenessContext,
    };
    use windows::Win32::UI::WindowsAndMessaging::{FindWindowW, PostMessageW};
    use windows::core::{HSTRING, Result};

    use crate::graphics::Graphics;
    use crate::{app_log, crash, hook, persistence, render_png, report, tray, updater};

    /// A restart (after an update or a crash) waits this long for the old process to let go
    /// of the single-instance lock.
    const RESTART_WAIT_ATTEMPTS: u32 = 150;
    const RESTART_WAIT_MILLISECONDS: u64 = 100;

    pub fn run() -> Result<()> {
        updater::remember_exe_path();
        let arguments: Vec<String> = std::env::args().skip(1).collect();
        if arguments.iter().any(|argument| argument == "--version") {
            println!("oh-key {}", env!("CARGO_PKG_VERSION"));
            return Ok(());
        }
        if let Some(position) = arguments
            .iter()
            .position(|argument| argument == "--write-sounds")
        {
            let folder = arguments
                .get(position + 1)
                .map(String::as_str)
                .unwrap_or(".");
            if let Err(error) = crate::sound::write_mode_sounds(&PathBuf::from(folder)) {
                eprintln!("oh-key: {error}");
            }
            return Ok(());
        }
        if arguments.first().map(String::as_str) == Some("crash-test") {
            crash::install();
            crash::test(arguments.get(1).map(String::as_str));
        }
        unsafe {
            let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
            let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        }
        if let Some(position) = arguments
            .iter()
            .position(|argument| argument == "--render-png")
        {
            return render_png_command(&arguments[position + 1..]);
        }
        let started_after_update = arguments
            .iter()
            .any(|argument| argument == "--after-update");
        let started_after_crash = arguments.iter().any(|argument| argument == "--after-crash");
        let started_after_rollback = arguments
            .iter()
            .any(|argument| argument == "--after-rollback");
        let wait_for_old_process =
            started_after_update || started_after_crash || started_after_rollback;
        let Some(single_instance) = claim_single_instance(wait_for_old_process)? else {
            return Ok(());
        };
        if let Some(outcome) = updater::roll_back_unconfirmed_update(started_after_update) {
            match outcome {
                Ok(message) => return hand_over_to_old_version(single_instance, &message),
                Err(message) => report_update_problem(&message),
            }
        }
        start_diagnostics(started_after_update, started_after_crash);
        let settings = load_settings();
        let shared = SharedState::open()?;
        persistence::publish(&settings);
        shared.set_vietnamese(settings.start_vietnamese);
        let _keyboard_hook = start_keyboard_hook()?;
        let outcome = tray::run(shared, started_after_crash);
        if started_after_update && outcome.is_ok() {
            updater::confirm_update();
        }
        outcome
    }

    /// The new version did not survive its first minutes. The old program is back in place:
    /// let go of the lock so it can start, then tell the reporter.
    fn hand_over_to_old_version(single_instance: HANDLE, message: &str) -> Result<()> {
        let _ = unsafe { CloseHandle(single_instance) };
        updater::restart_after_rollback();
        report_update_problem(message);
        Ok(())
    }

    fn report_update_problem(message: &str) {
        report::send_now(report::ReportEvent::new(
            report::EventKind::UpdateFailed,
            message,
        ));
    }

    /// How long a fresh update must keep running before the old program is thrown away.
    const UPDATE_PROBATION: std::time::Duration = std::time::Duration::from_secs(120);

    fn start_probation_timer() {
        updater::begin_probation();
        let _ = std::thread::Builder::new()
            .name("oh-key-probation".to_owned())
            .spawn(|| {
                std::thread::sleep(UPDATE_PROBATION);
                updater::confirm_update();
            });
    }

    /// The crash handlers, the log and the reporter; and what the last run left behind.
    fn start_diagnostics(started_after_update: bool, started_after_crash: bool) {
        crash::install();
        crash::watch_tray(started_after_crash);
        report::start();
        report::connect_hook();
        app_log::write(&format!(
            "started oh-key {} on {}",
            env!("CARGO_PKG_VERSION"),
            std::env::consts::ARCH
        ));
        if started_after_update {
            app_log::write("started after an update");
            start_probation_timer();
        }
        updater::clean_up_old_exe(started_after_update);
        if let Some(problem) = oh_key_ipc::settings_file_problem() {
            app_log::write(&problem);
        }
        if let Some(crashed) = crash::take_pending() {
            report::submit(report::ReportEvent {
                frames: crashed.frames.clone(),
                write_to_log: false,
                ..report::ReportEvent::new(crashed.report_kind(), &crashed.summary)
            });
        }
    }

    /// `OHKEY_NO_HOOK=1` leaves the hook out, so a measurement build never touches typing.
    fn start_keyboard_hook() -> Result<Option<hook::HookHandle>> {
        if std::env::var_os("OHKEY_NO_HOOK").is_some_and(|value| value == "1") {
            return Ok(None);
        }
        let toggle_state = SharedState::open()?;
        Ok(hook::start(
            hook::app_inputs_provider(SharedState::open()?),
            Box::new(move || toggle_state.set_vietnamese(!toggle_state.vietnamese())),
            hook::HookOptions::default(),
        ))
    }

    fn render_png_command(arguments: &[String]) -> Result<()> {
        let (Some(window), Some(output)) = (arguments.first(), arguments.get(1)) else {
            return Err(windows::Win32::Foundation::E_INVALIDARG.into());
        };
        render_png::render(&Graphics::new()?, window, &PathBuf::from(output))
    }

    /// The mutex handle while this is the only instance. A second launch asks the running
    /// one to open the control panel and gets `None`. A restart after an update or a crash
    /// instead waits for the old process to go, and takes its place.
    fn claim_single_instance(wait_for_old_process: bool) -> Result<Option<HANDLE>> {
        let mutex_name = HSTRING::from(format!("Local\\oh-key.app{}", instance_suffix()));
        let mut attempts_left = if wait_for_old_process {
            RESTART_WAIT_ATTEMPTS
        } else {
            0
        };
        loop {
            let mutex = unsafe { CreateMutexW(None, true, &mutex_name)? };
            if unsafe { GetLastError() } != ERROR_ALREADY_EXISTS {
                return Ok(Some(mutex));
            }
            let _ = unsafe { CloseHandle(mutex) };
            if attempts_left == 0 {
                break;
            }
            attempts_left -= 1;
            std::thread::sleep(std::time::Duration::from_millis(RESTART_WAIT_MILLISECONDS));
        }
        if wait_for_old_process {
            app_log::write("a restart gave up waiting for the old process to exit");
            return Ok(None);
        }
        let title = tray::window_title();
        if let Ok(running) = unsafe { FindWindowW(tray::TRAY_CLASS, &title) } {
            let _ = unsafe {
                PostMessageW(
                    Some(running),
                    tray::MESSAGE_OPEN_SETTINGS,
                    Default::default(),
                    Default::default(),
                )
            };
        }
        Ok(None)
    }
}

#[cfg(windows)]
fn main() {
    if let Err(error) = app::run() {
        app_log::write(&format!("oh-key stopped: {}", error.message()));
    }
}

#[cfg(not(windows))]
fn main() {
    eprintln!("oh-key: the tray app is Windows only for now");
}
