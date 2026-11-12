//! The tray icon: shows the mode (V or E), toggles it on a left click and opens the menu on a
//! right click. It sleeps until a message or a mode change signalled by the hotkey arrives.

use oh_key_engine::InputMethod;
use oh_key_ipc::{SharedState, instance_suffix, load_settings};
use windows::Win32::Foundation::{HANDLE, HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::INFINITE;
use windows::Win32::UI::Shell::{
    NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NIM_MODIFY, NOTIFYICONDATAW,
    Shell_NotifyIconW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreatePopupMenu, CreateWindowExW, DefWindowProcW, DestroyMenu, DestroyWindow,
    DispatchMessageW, GWLP_USERDATA, GetCursorPos, GetWindowLongPtrW, HMENU, MF_CHECKED, MF_POPUP,
    MF_SEPARATOR, MF_STRING, MSG, MsgWaitForMultipleObjects, PM_REMOVE, PeekMessageW,
    PostQuitMessage, QS_ALLINPUT, RegisterClassExW, RegisterWindowMessageW, SetForegroundWindow,
    SetWindowLongPtrW, TPM_BOTTOMALIGN, TPM_RETURNCMD, TPM_RIGHTBUTTON, TrackPopupMenu,
    TranslateMessage, WM_APP, WM_DESTROY, WM_LBUTTONUP, WM_QUIT, WM_RBUTTONUP, WNDCLASSEXW,
    WS_EX_TOOLWINDOW, WS_OVERLAPPED,
};
use windows::core::{HSTRING, Result, w};

use crate::settings_model::INPUT_METHOD_CHOICES;
use crate::{converter_window, macros_window, mode_icon, persistence, settings_window};

pub const TRAY_CLASS: windows::core::PCWSTR = w!("oh-key.tray");
const MESSAGE_TRAY: u32 = WM_APP + 1;
/// Sent by a second launch of the app, so it opens the control panel of the running one.
pub const MESSAGE_OPEN_SETTINGS: u32 = WM_APP + 2;
/// Only when the mode event could not be created: check for a mode change this often.
const MODE_FALLBACK_POLL_MILLISECONDS: u32 = 1000;

const MENU_VIETNAMESE: usize = 1;
const MENU_ENGLISH: usize = 2;
const MENU_SETTINGS: usize = 3;
const MENU_MACROS: usize = 4;
const MENU_CONVERTER: usize = 5;
const MENU_QUIT: usize = 6;
const MENU_METHOD_FIRST: usize = 10;

pub const LABEL_VIETNAMESE: &str = "Tiếng Việt";
pub const LABEL_ENGLISH: &str = "English";
pub const LABEL_INPUT_METHOD: &str = "Kiểu gõ";
pub const LABEL_SETTINGS: &str = "Bảng điều khiển…";
pub const LABEL_MACROS: &str = "Gõ tắt…";
pub const LABEL_CONVERTER: &str = "Chuyển mã…";
pub const LABEL_QUIT: &str = "Thoát";

/// Everything the window procedure needs, owned by the window through its user data.
struct Tray {
    shared: SharedState,
    shown_mode_counter: u64,
    taskbar_created: u32,
}

/// The hidden window's title; a second launch finds the running one by it.
pub fn window_title() -> HSTRING {
    HSTRING::from(format!("oh-key{}", instance_suffix()))
}

fn method_menu_id(index: usize) -> usize {
    MENU_METHOD_FIRST + index
}

fn method_index_for_menu_id(menu_id: usize) -> Option<usize> {
    let index = menu_id.checked_sub(MENU_METHOD_FIRST)?;
    (index < INPUT_METHOD_CHOICES.len()).then_some(index)
}

fn tooltip(vietnamese: bool) -> &'static str {
    if vietnamese {
        "oh-key: Tiếng Việt"
    } else {
        "oh-key: English"
    }
}

/// Creates the hidden window and the tray icon, then runs the message loop until quit.
pub fn run(shared: SharedState) -> Result<()> {
    let instance = unsafe { GetModuleHandleW(None)? };
    let class = WNDCLASSEXW {
        cbSize: size_of::<WNDCLASSEXW>() as u32,
        lpfnWndProc: Some(window_proc),
        hInstance: instance.into(),
        lpszClassName: TRAY_CLASS,
        ..Default::default()
    };
    unsafe { RegisterClassExW(&class) };
    let tray = Box::new(Tray {
        shared,
        shown_mode_counter: 0,
        taskbar_created: unsafe { RegisterWindowMessageW(w!("TaskbarCreated")) },
    });
    let tray_pointer = Box::into_raw(tray);
    // A top-level window that is never shown: TaskbarCreated does not reach message-only windows.
    let hwnd = unsafe {
        CreateWindowExW(
            WS_EX_TOOLWINDOW,
            TRAY_CLASS,
            &window_title(),
            WS_OVERLAPPED,
            0,
            0,
            0,
            0,
            None,
            None,
            Some(instance.into()),
            None,
        )?
    };
    unsafe { SetWindowLongPtrW(hwnd, GWLP_USERDATA, tray_pointer as _) };
    let tray = unsafe { &mut *tray_pointer };
    tray.shown_mode_counter = tray.shared.mode_counter();
    tray.add_icon(hwnd);
    run_message_loop(tray, hwnd);
    drop(unsafe { Box::from_raw(tray_pointer) });
    Ok(())
}

/// Sleeps until a window message or the mode event arrives; no timer wakes it while idle.
fn run_message_loop(tray: &mut Tray, hwnd: HWND) {
    let mode_event = tray.shared.mode_event();
    let wake_sources: Vec<HANDLE> = mode_event.into_iter().collect();
    let timeout = if mode_event.is_some() {
        INFINITE
    } else {
        MODE_FALLBACK_POLL_MILLISECONDS
    };
    let mut message = MSG::default();
    loop {
        tray.follow_mode(hwnd);
        while unsafe { PeekMessageW(&mut message, None, 0, 0, PM_REMOVE) }.as_bool() {
            if message.message == WM_QUIT {
                return;
            }
            unsafe {
                let _ = TranslateMessage(&message);
                DispatchMessageW(&message);
            }
        }
        unsafe { MsgWaitForMultipleObjects(Some(&wake_sources), false, timeout, QS_ALLINPUT) };
    }
}

unsafe extern "system" fn window_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    let tray_pointer = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } as *mut Tray;
    if tray_pointer.is_null() {
        return unsafe { DefWindowProcW(hwnd, message, wparam, lparam) };
    }
    let tray = unsafe { &mut *tray_pointer };
    match message {
        MESSAGE_TRAY => {
            match (lparam.0 & 0xFFFF) as u32 {
                WM_LBUTTONUP => tray.toggle_mode(hwnd),
                WM_RBUTTONUP => tray.show_menu(hwnd),
                _ => {}
            }
            LRESULT(0)
        }
        MESSAGE_OPEN_SETTINGS => {
            let _ = settings_window::open();
            LRESULT(0)
        }
        WM_DESTROY => {
            tray.remove_icon(hwnd);
            unsafe { PostQuitMessage(0) };
            LRESULT(0)
        }
        _ if message == tray.taskbar_created => {
            tray.add_icon(hwnd);
            LRESULT(0)
        }
        _ => unsafe { DefWindowProcW(hwnd, message, wparam, lparam) },
    }
}

impl Tray {
    fn notify_data(&self, hwnd: HWND) -> NOTIFYICONDATAW {
        let vietnamese = self.shared.vietnamese();
        // The struct is packed on x86, so the tip is filled in a local buffer and moved in whole.
        let mut tip_buffer = [0u16; 128];
        for (slot, unit) in tip_buffer
            .iter_mut()
            .zip(tooltip(vietnamese).encode_utf16())
        {
            *slot = unit;
        }
        NOTIFYICONDATAW {
            cbSize: size_of::<NOTIFYICONDATAW>() as u32,
            hWnd: hwnd,
            uID: 1,
            uFlags: NIF_ICON | NIF_MESSAGE | NIF_TIP,
            uCallbackMessage: MESSAGE_TRAY,
            hIcon: mode_icon::icon(vietnamese),
            szTip: tip_buffer,
            ..Default::default()
        }
    }

    fn add_icon(&self, hwnd: HWND) {
        let _ = unsafe { Shell_NotifyIconW(NIM_ADD, &self.notify_data(hwnd)) };
    }

    fn update_icon(&self, hwnd: HWND) {
        let _ = unsafe { Shell_NotifyIconW(NIM_MODIFY, &self.notify_data(hwnd)) };
    }

    fn remove_icon(&self, hwnd: HWND) {
        let data = NOTIFYICONDATAW {
            cbSize: size_of::<NOTIFYICONDATAW>() as u32,
            hWnd: hwnd,
            uID: 1,
            ..Default::default()
        };
        let _ = unsafe { Shell_NotifyIconW(NIM_DELETE, &data) };
    }

    fn toggle_mode(&mut self, hwnd: HWND) {
        self.shared.set_vietnamese(!self.shared.vietnamese());
        self.follow_mode(hwnd);
    }

    /// Redraws the icon when the mode changed, here or by the hotkey inside an app.
    fn follow_mode(&mut self, hwnd: HWND) {
        let counter = self.shared.mode_counter();
        if counter != self.shown_mode_counter {
            self.shown_mode_counter = counter;
            self.update_icon(hwnd);
            settings_window::refresh();
        }
    }

    fn select_vietnamese(&mut self, hwnd: HWND, vietnamese: bool) {
        self.shared.set_vietnamese(vietnamese);
        self.follow_mode(hwnd);
    }

    fn select_input_method(&self, index: usize) {
        let mut settings = load_settings();
        settings.engine.input_method = INPUT_METHOD_CHOICES[index].0;
        persistence::save_and_publish(&settings);
        settings_window::refresh();
    }

    fn show_menu(&mut self, hwnd: HWND) {
        let vietnamese = self.shared.vietnamese();
        let current_method: InputMethod = load_settings().engine.input_method;
        let Ok(menu) = (unsafe { CreatePopupMenu() }) else {
            return;
        };
        build_menu(menu, vietnamese, current_method);
        let mut cursor = POINT::default();
        unsafe {
            let _ = GetCursorPos(&mut cursor);
            // The menu only closes on an outside click when its owner is foreground.
            let _ = SetForegroundWindow(hwnd);
        }
        let chosen = unsafe {
            TrackPopupMenu(
                menu,
                TPM_RETURNCMD | TPM_RIGHTBUTTON | TPM_BOTTOMALIGN,
                cursor.x,
                cursor.y,
                None,
                hwnd,
                None,
            )
        };
        unsafe {
            let _ = DestroyMenu(menu);
        }
        self.run_command(hwnd, chosen.0 as usize);
    }

    fn run_command(&mut self, hwnd: HWND, command: usize) {
        match command {
            MENU_VIETNAMESE => self.select_vietnamese(hwnd, true),
            MENU_ENGLISH => self.select_vietnamese(hwnd, false),
            MENU_SETTINGS => {
                let _ = settings_window::open();
            }
            MENU_MACROS => macros_window::open(None),
            MENU_CONVERTER => converter_window::open(None),
            MENU_QUIT => unsafe {
                let _ = DestroyWindow(hwnd);
            },
            other => {
                if let Some(index) = method_index_for_menu_id(other) {
                    self.select_input_method(index);
                }
            }
        }
    }
}

fn checked_if(checked: bool) -> windows::Win32::UI::WindowsAndMessaging::MENU_ITEM_FLAGS {
    if checked {
        MF_STRING | MF_CHECKED
    } else {
        MF_STRING
    }
}

fn append_item(
    menu: HMENU,
    flags: windows::Win32::UI::WindowsAndMessaging::MENU_ITEM_FLAGS,
    id: usize,
    label: &str,
) {
    let _ = unsafe { AppendMenuW(menu, flags, id, &HSTRING::from(label)) };
}

fn build_menu(menu: HMENU, vietnamese: bool, current_method: InputMethod) {
    append_item(
        menu,
        checked_if(vietnamese),
        MENU_VIETNAMESE,
        LABEL_VIETNAMESE,
    );
    append_item(menu, checked_if(!vietnamese), MENU_ENGLISH, LABEL_ENGLISH);
    if let Ok(methods) = unsafe { CreatePopupMenu() } {
        for (index, (method, label)) in INPUT_METHOD_CHOICES.iter().enumerate() {
            append_item(
                methods,
                checked_if(*method == current_method),
                method_menu_id(index),
                label,
            );
        }
        let _ = unsafe {
            AppendMenuW(
                menu,
                MF_POPUP,
                methods.0 as usize,
                &HSTRING::from(LABEL_INPUT_METHOD),
            )
        };
    }
    let _ = unsafe { AppendMenuW(menu, MF_SEPARATOR, 0, None) };
    append_item(menu, MF_STRING, MENU_SETTINGS, LABEL_SETTINGS);
    append_item(menu, MF_STRING, MENU_MACROS, LABEL_MACROS);
    append_item(menu, MF_STRING, MENU_CONVERTER, LABEL_CONVERTER);
    let _ = unsafe { AppendMenuW(menu, MF_SEPARATOR, 0, None) };
    append_item(menu, MF_STRING, MENU_QUIT, LABEL_QUIT);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_input_method_has_its_own_menu_id_and_maps_back() {
        for index in 0..INPUT_METHOD_CHOICES.len() {
            assert_eq!(method_index_for_menu_id(method_menu_id(index)), Some(index));
        }
    }

    #[test]
    fn other_menu_ids_are_not_input_methods() {
        assert_eq!(method_index_for_menu_id(MENU_QUIT), None);
        assert_eq!(
            method_index_for_menu_id(method_menu_id(INPUT_METHOD_CHOICES.len())),
            None
        );
    }

    #[test]
    fn menu_ids_do_not_collide_with_the_method_ids() {
        let fixed = [
            MENU_VIETNAMESE,
            MENU_ENGLISH,
            MENU_SETTINGS,
            MENU_MACROS,
            MENU_CONVERTER,
            MENU_QUIT,
        ];
        assert!(fixed.iter().all(|id| *id < MENU_METHOD_FIRST));
    }

    #[test]
    fn the_tooltip_names_the_mode() {
        assert!(tooltip(true).contains("Việt"));
        assert!(tooltip(false).contains("English"));
    }
}
