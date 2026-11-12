//! Types keys through the installed oh-key text service into a private window
//! and prints what the window ends up holding.
//!
//! oh-key is activated for this process only (`TF_IPPMF_FORPROCESS`), so the
//! keyboard of every other application stays as it is.
//!
//! ```text
//! cargo run -p oh-key-windows --example probe --target x86_64-pc-windows-msvc -- edit "Vieetj Nam "
//! cargo run -p oh-key-windows --example probe --target x86_64-pc-windows-msvc -- richedit "Vieetj Nam "
//! ```

use std::time::{Duration, Instant};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx,
};
use windows::Win32::System::LibraryLoader::{GetModuleHandleW, LoadLibraryW};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    INPUT, INPUT_0, INPUT_KEYBOARD, KEYBD_EVENT_FLAGS, KEYBDINPUT, KEYEVENTF_KEYUP,
    MAP_VIRTUAL_KEY_TYPE, MapVirtualKeyW, SendInput, SetFocus, VIRTUAL_KEY, VkKeyScanW,
};
use windows::Win32::UI::TextServices::{
    CLSID_TF_InputProcessorProfiles, ITfInputProcessorProfileMgr, TF_IPPMF_FORPROCESS,
    TF_PROFILETYPE_INPUTPROCESSOR,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DispatchMessageW, ES_AUTOHSCROLL, GetWindowTextLengthW,
    GetWindowTextW, MSG, PM_REMOVE, PeekMessageW, RegisterClassW, SW_SHOW, SendMessageW,
    SetForegroundWindow, ShowWindow, TranslateMessage, WINDOW_EX_STYLE, WINDOW_STYLE, WNDCLASSW,
    WS_CHILD, WS_OVERLAPPEDWINDOW, WS_VISIBLE,
};
use windows::core::{GUID, PCWSTR, w};

const OHKEY_CLSID: GUID = GUID::from_u128(0x6d5a7f1e_3b2c_4e8a_9c41_7a1e5b0d2f63);
const OHKEY_PROFILE_GUID: GUID = GUID::from_u128(0xb8e0c4a2_5f17_4d93_a6b1_2c9e7d4f8a15);
const VIETNAMESE: u16 = 0x042A;
const EM_SETEDITSTYLE: u32 = 0x04CC;
const SES_USECTF: usize = 0x0001_0000;

fn main() -> windows::core::Result<()> {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    let control_kind = arguments.first().map(String::as_str).unwrap_or("edit");
    let keys = arguments
        .get(1)
        .map(String::as_str)
        .unwrap_or("Vieetj Nam ");

    unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED).ok()? };
    let (frame, editor) = create_window(control_kind)?;
    activate_oh_key_for_this_process()?;

    unsafe {
        let _ = ShowWindow(frame, SW_SHOW);
        let _ = SetForegroundWindow(frame);
        let _ = SetFocus(Some(editor));
    }
    pump_messages(Duration::from_millis(400));
    for character in keys.chars() {
        press(character);
        pump_messages(Duration::from_millis(40));
    }
    pump_messages(Duration::from_millis(600));

    let text = read_text(editor);
    println!("control: {control_kind}");
    println!("text: [{text}]");
    Ok(())
}

fn activate_oh_key_for_this_process() -> windows::core::Result<()> {
    let profiles: ITfInputProcessorProfileMgr =
        unsafe { CoCreateInstance(&CLSID_TF_InputProcessorProfiles, None, CLSCTX_INPROC_SERVER)? };
    unsafe {
        profiles.ActivateProfile(
            TF_PROFILETYPE_INPUTPROCESSOR,
            VIETNAMESE,
            &OHKEY_CLSID,
            &OHKEY_PROFILE_GUID,
            Default::default(),
            TF_IPPMF_FORPROCESS,
        )
    }
}

fn create_window(control_kind: &str) -> windows::core::Result<(HWND, HWND)> {
    let instance = unsafe { GetModuleHandleW(None)? };
    let class_name = w!("OhKeyProbe");
    let class = WNDCLASSW {
        lpfnWndProc: Some(window_procedure),
        hInstance: instance.into(),
        lpszClassName: class_name,
        ..Default::default()
    };
    unsafe { RegisterClassW(&class) };
    let frame = unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE(0),
            class_name,
            w!("oh-key probe"),
            WS_OVERLAPPEDWINDOW,
            200,
            200,
            520,
            140,
            None,
            None,
            Some(instance.into()),
            None,
        )?
    };
    let editor_class: PCWSTR = if control_kind == "richedit" {
        unsafe { LoadLibraryW(w!("Msftedit.dll"))? };
        w!("RICHEDIT50W")
    } else {
        w!("EDIT")
    };
    let editor = unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE(0),
            editor_class,
            w!(""),
            WS_CHILD | WS_VISIBLE | WINDOW_STYLE(ES_AUTOHSCROLL as u32),
            10,
            10,
            480,
            40,
            Some(frame),
            None,
            Some(instance.into()),
            None,
        )?
    };
    if control_kind == "richedit" {
        unsafe {
            SendMessageW(
                editor,
                EM_SETEDITSTYLE,
                Some(WPARAM(SES_USECTF)),
                Some(LPARAM(SES_USECTF as isize)),
            )
        };
    }
    Ok((frame, editor))
}

extern "system" fn window_procedure(
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

fn press(character: char) {
    let scan = unsafe { VkKeyScanW(character as u16) };
    let virtual_key = (scan as u16) & 0xFF;
    let needs_shift = (scan as u16) & 0x100 != 0;
    if needs_shift {
        send_key(0x10, false);
    }
    send_key(virtual_key, false);
    send_key(virtual_key, true);
    if needs_shift {
        send_key(0x10, true);
    }
}

fn send_key(virtual_key: u16, key_up: bool) {
    let scan_code = unsafe { MapVirtualKeyW(u32::from(virtual_key), MAP_VIRTUAL_KEY_TYPE(0)) };
    let flags = if key_up {
        KEYEVENTF_KEYUP
    } else {
        KEYBD_EVENT_FLAGS(0)
    };
    let input = INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: VIRTUAL_KEY(virtual_key),
                wScan: scan_code as u16,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    };
    unsafe { SendInput(&[input], std::mem::size_of::<INPUT>() as i32) };
}

fn read_text(editor: HWND) -> String {
    let length = unsafe { GetWindowTextLengthW(editor) };
    let mut buffer = vec![0u16; length as usize + 1];
    let copied = unsafe { GetWindowTextW(editor, &mut buffer) };
    String::from_utf16_lossy(&buffer[..copied as usize])
}
