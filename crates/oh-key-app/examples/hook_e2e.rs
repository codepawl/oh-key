//! End-to-end check of the keyboard hook, against this process's own window only.
//!
//! It opens a window with an EDIT control, runs with no exclusions (in memory,
//! nothing is saved), types "Vieetj Nam " through `SendInput` and reads the text back; the
//! hook has to turn it into "Việt Nam ". It types nothing unless its own window is the
//! foreground window at that moment, and it exits within 30 seconds whatever happens.
//!
//! Run: `cargo run -p oh-key-app --example hook_e2e --target x86_64-pc-windows-msvc`
//! Exit code: 0 passed, 1 wrong text, 2 could not get the foreground (nothing was typed),
//! 3 timed out, 4 the hook could not start.

#[cfg(windows)]
#[allow(dead_code)]
#[path = "../src/hook.rs"]
mod hook;

#[cfg(windows)]
mod e2e {
    use std::time::{Duration, Instant};

    use oh_key_engine::Config;
    use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows::Win32::System::Threading::{AttachThreadInput, GetCurrentThreadId};
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP, SendInput, SetFocus,
        VIRTUAL_KEY, VK_SHIFT, VkKeyScanW,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, ES_AUTOHSCROLL,
        GetForegroundWindow, GetWindowTextW, GetWindowThreadProcessId, MSG, PM_REMOVE,
        PeekMessageW, RegisterClassExW, SW_SHOW, SetForegroundWindow, ShowWindow, TranslateMessage,
        WINDOW_STYLE, WNDCLASSEXW, WS_BORDER, WS_CHILD, WS_OVERLAPPEDWINDOW, WS_VISIBLE,
    };
    use windows::core::w;

    use crate::hook::{self, HookInputs, HookOptions};

    const KEYS_TO_TYPE: &str = "Vieetj Nam ";
    const EXPECTED_TEXT: &str = "Việt Nam ";
    const WATCHDOG: Duration = Duration::from_secs(25);

    unsafe extern "system" fn window_proc(
        window: HWND,
        message: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        unsafe { DefWindowProcW(window, message, wparam, lparam) }
    }

    fn pump_messages(duration: Duration) {
        let deadline = Instant::now() + duration;
        let mut message = MSG::default();
        while Instant::now() < deadline {
            while unsafe { PeekMessageW(&mut message, None, 0, 0, PM_REMOVE) }.as_bool() {
                unsafe {
                    let _ = TranslateMessage(&message);
                    DispatchMessageW(&message);
                }
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    fn create_windows() -> windows::core::Result<(HWND, HWND)> {
        let instance = unsafe { GetModuleHandleW(None)? };
        let class = WNDCLASSEXW {
            cbSize: size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(window_proc),
            hInstance: instance.into(),
            lpszClassName: w!("oh-key.hook-e2e"),
            ..Default::default()
        };
        unsafe { RegisterClassExW(&class) };
        let window = unsafe {
            CreateWindowExW(
                Default::default(),
                w!("oh-key.hook-e2e"),
                w!("oh-key hook e2e"),
                WS_OVERLAPPEDWINDOW,
                200,
                200,
                520,
                120,
                None,
                None,
                Some(instance.into()),
                None,
            )?
        };
        let edit = unsafe {
            CreateWindowExW(
                Default::default(),
                w!("EDIT"),
                w!(""),
                WS_CHILD | WS_VISIBLE | WS_BORDER | WINDOW_STYLE(ES_AUTOHSCROLL as u32),
                10,
                10,
                480,
                28,
                Some(window),
                None,
                Some(instance.into()),
                None,
            )?
        };
        Ok((window, edit))
    }

    /// Brings `window` to the front even though this process was started in the background.
    fn take_foreground(window: HWND) -> bool {
        let current_thread = unsafe { GetCurrentThreadId() };
        let front = unsafe { GetForegroundWindow() };
        let front_thread = unsafe { GetWindowThreadProcessId(front, None) };
        unsafe {
            let _ = AttachThreadInput(current_thread, front_thread, true);
            let _ = ShowWindow(window, SW_SHOW);
            let _ = SetForegroundWindow(window);
            let _ = AttachThreadInput(current_thread, front_thread, false);
        }
        for _ in 0..40 {
            pump_messages(Duration::from_millis(50));
            if unsafe { GetForegroundWindow() } == window {
                return true;
            }
        }
        false
    }

    fn key_event(virtual_key: u16, release: bool) -> INPUT {
        INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: VIRTUAL_KEY(virtual_key),
                    wScan: 0,
                    dwFlags: if release {
                        KEYEVENTF_KEYUP
                    } else {
                        Default::default()
                    },
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        }
    }

    /// Presses and releases the key that types `character` on the current layout.
    fn type_character(character: char) {
        let scan = unsafe { VkKeyScanW(character as u16) };
        let virtual_key = (scan & 0xFF) as u16;
        let needs_shift = (scan >> 8) & 1 != 0;
        let mut events = Vec::new();
        if needs_shift {
            events.push(key_event(VK_SHIFT.0, false));
        }
        events.push(key_event(virtual_key, false));
        events.push(key_event(virtual_key, true));
        if needs_shift {
            events.push(key_event(VK_SHIFT.0, true));
        }
        unsafe { SendInput(&events, size_of::<INPUT>() as i32) };
    }

    fn edit_text(edit: HWND) -> String {
        let mut buffer = [0u16; 256];
        let length = unsafe { GetWindowTextW(edit, &mut buffer) }.max(0) as usize;
        String::from_utf16_lossy(&buffer[..length])
    }

    pub fn run() -> i32 {
        std::thread::spawn(|| {
            std::thread::sleep(WATCHDOG);
            eprintln!("hook_e2e: timed out");
            std::process::exit(3);
        });
        let Ok((window, edit)) = create_windows() else {
            return 2;
        };
        let mut delivered = false;
        let provider = Box::new(move || {
            if delivered {
                return None;
            }
            delivered = true;
            Some(HookInputs {
                excluded_apps: Vec::new(),
                vietnamese: true,
                engine: Config::default(),
                toggle_hotkey: oh_key_ipc::ToggleHotkey::Disabled,
                macros: None,
            })
        });
        let options = HookOptions {
            accept_foreign_injected_keys: true,
            ..HookOptions::default()
        };
        let Some(hook_handle) = hook::start(provider, Box::new(|| {}), options) else {
            eprintln!("hook_e2e: the hook could not start");
            return 4;
        };
        let exit_code = type_into_window(window, edit);
        drop(hook_handle);
        unsafe {
            let _ = DestroyWindow(window);
        }
        exit_code
    }

    fn type_into_window(window: HWND, edit: HWND) -> i32 {
        if !take_foreground(window) {
            eprintln!("hook_e2e: could not get the foreground, nothing was typed");
            return 2;
        }
        unsafe {
            let _ = SetFocus(Some(edit));
        }
        // The hook notices the foreground change at the start of the next key event.
        pump_messages(Duration::from_millis(400));
        for character in KEYS_TO_TYPE.chars() {
            if unsafe { GetForegroundWindow() } != window {
                eprintln!("hook_e2e: lost the foreground, stopped typing");
                return 2;
            }
            type_character(character);
            pump_messages(Duration::from_millis(40));
        }
        pump_messages(Duration::from_millis(300));
        let text = edit_text(edit);
        if text == EXPECTED_TEXT {
            println!("hook_e2e: PASS, typed {KEYS_TO_TYPE:?} and the window holds {text:?}");
            0
        } else {
            println!("hook_e2e: FAIL, expected {EXPECTED_TEXT:?} but the window holds {text:?}");
            1
        }
    }
}

#[cfg(windows)]
fn main() {
    std::process::exit(e2e::run());
}

#[cfg(not(windows))]
fn main() {
    eprintln!("hook_e2e is Windows only");
}
