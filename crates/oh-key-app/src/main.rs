//! `oh-key`: the tray app. It shows the Vietnamese/English mode, hosts the control panel and
//! types Vietnamese in every app through its keyboard hook.

#![cfg_attr(windows, windows_subsystem = "windows")]

#[cfg(windows)]
mod converter_window;
#[cfg(windows)]
mod excluded_apps_window;
#[cfg(windows)]
mod graphics;
#[cfg(windows)]
mod hook;
#[cfg(windows)]
mod icons;
#[cfg(windows)]
mod layout;
#[cfg(windows)]
mod macros_window;
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
mod settings_model;
#[cfg(windows)]
mod settings_window;
#[cfg(windows)]
mod startup;
#[cfg(windows)]
mod style;
#[cfg(windows)]
mod tool_window;
#[cfg(windows)]
mod tray;

#[cfg(windows)]
mod app {
    use std::path::PathBuf;

    use oh_key_ipc::{SharedState, load_settings};
    use windows::Win32::Foundation::{ERROR_ALREADY_EXISTS, GetLastError, HANDLE};
    use windows::Win32::System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx};
    use windows::Win32::System::Threading::CreateMutexW;
    use windows::Win32::UI::HiDpi::{
        DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, SetProcessDpiAwarenessContext,
    };
    use windows::Win32::UI::WindowsAndMessaging::{FindWindowW, PostMessageW};
    use windows::core::{Result, w};

    use crate::graphics::Graphics;
    use crate::{hook, persistence, render_png, tray};

    pub fn run() -> Result<()> {
        unsafe {
            let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
            let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        }
        let arguments: Vec<String> = std::env::args().skip(1).collect();
        if let Some(position) = arguments
            .iter()
            .position(|argument| argument == "--render-png")
        {
            return render_png_command(&arguments[position + 1..]);
        }
        let Some(_single_instance) = claim_single_instance()? else {
            return Ok(());
        };
        let settings = load_settings();
        let shared = SharedState::open()?;
        persistence::publish(&settings);
        shared.set_vietnamese(settings.start_vietnamese);
        let toggle_state = SharedState::open()?;
        let _keyboard_hook = hook::start(
            hook::app_inputs_provider(SharedState::open()?),
            Box::new(move || toggle_state.set_vietnamese(!toggle_state.vietnamese())),
            hook::HookOptions::default(),
        );
        tray::run(Graphics::new()?, shared)
    }

    fn render_png_command(arguments: &[String]) -> Result<()> {
        let (Some(window), Some(output)) = (arguments.first(), arguments.get(1)) else {
            return Err(windows::Win32::Foundation::E_INVALIDARG.into());
        };
        render_png::render(&Graphics::new()?, window, &PathBuf::from(output))
    }

    /// The mutex handle while this is the only instance. A second launch asks the running
    /// one to open the control panel and gets `None`.
    fn claim_single_instance() -> Result<Option<HANDLE>> {
        let mutex = unsafe { CreateMutexW(None, true, w!("Local\\oh-key.app"))? };
        if unsafe { GetLastError() } != ERROR_ALREADY_EXISTS {
            return Ok(Some(mutex));
        }
        if let Ok(running) = unsafe { FindWindowW(tray::TRAY_CLASS, None) } {
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
    let _ = app::run();
}

#[cfg(not(windows))]
fn main() {
    eprintln!("oh-key: the tray app is Windows only for now");
}
