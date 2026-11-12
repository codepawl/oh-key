//! The tray icon: shows the mode (V or E), toggles it on a left click and opens the menu on a
//! right click. It sleeps until a message or a mode change signalled by the hotkey arrives.

use oh_key_engine::InputMethod;
use oh_key_ipc::{SharedState, instance_suffix, load_settings};
use windows::Win32::Foundation::{HANDLE, HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::INFINITE;
use windows::Win32::UI::Shell::{
    NIF_ICON, NIF_INFO, NIF_MESSAGE, NIF_TIP, NIIF_INFO, NIIF_WARNING, NIM_ADD, NIM_DELETE,
    NIM_MODIFY, NOTIFYICONDATAW, Shell_NotifyIconW,
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
use crate::update_service::{self, Notice};
use crate::{
    app_log, converter_window, macros_window, mode_icon, persistence, report, settings_window,
    updater,
};

pub const TRAY_CLASS: windows::core::PCWSTR = w!("oh-key.tray");
const MESSAGE_TRAY: u32 = WM_APP + 1;
/// Sent by a second launch of the app, so it opens the control panel of the running one.
pub const MESSAGE_OPEN_SETTINGS: u32 = WM_APP + 2;
/// Posted by the update thread when it queued a notice for the tray to show.
const MESSAGE_UPDATE_NOTICE: u32 = WM_APP + 3;
/// What the tray icon sends when its balloon is clicked.
const NOTIFICATION_BALLOON_CLICKED: u32 = 0x0405;
/// Only when the mode event could not be created: check for a mode change this often.
const MODE_FALLBACK_POLL_MILLISECONDS: u32 = 1000;

const MENU_VIETNAMESE: usize = 1;
const MENU_ENGLISH: usize = 2;
const MENU_SETTINGS: usize = 3;
const MENU_MACROS: usize = 4;
const MENU_CONVERTER: usize = 5;
const MENU_QUIT: usize = 6;
const MENU_CHECK_UPDATES: usize = 7;
const MENU_INSTALL_UPDATE: usize = 8;
const MENU_OPEN_LOG: usize = 9;
const MENU_METHOD_FIRST: usize = 10;

pub const LABEL_VIETNAMESE: &str = "Tiếng Việt";
pub const LABEL_ENGLISH: &str = "English";
pub const LABEL_INPUT_METHOD: &str = "Kiểu gõ";
pub const LABEL_SETTINGS: &str = "Bảng điều khiển…";
pub const LABEL_MACROS: &str = "Gõ tắt…";
pub const LABEL_CONVERTER: &str = "Chuyển mã…";
pub const LABEL_QUIT: &str = "Thoát";
pub const LABEL_CHECK_UPDATES: &str = "Kiểm tra cập nhật";
pub const LABEL_OPEN_LOG: &str = "Mở thư mục log";

pub fn label_install_update(version: &str) -> String {
    format!("Có bản mới {version} — Cập nhật")
}

/// What a balloon says for a notice: title, text and whether it is a warning. A restart into
/// a new version needs no balloon.
fn balloon_for(notice: &Notice) -> Option<(String, String, bool)> {
    match notice {
        Notice::UpToDate => Some((
            "oh-key".to_string(),
            "Bạn đang dùng bản mới nhất.".to_string(),
            false,
        )),
        Notice::Available(version) => Some((
            "Có bản mới".to_string(),
            format!("oh-key {version} đã có. Bấm vào đây để cập nhật."),
            false,
        )),
        Notice::Failed(reason) => Some((
            "Không cập nhật được".to_string(),
            reason.chars().take(200).collect(),
            true,
        )),
        Notice::Installed(_) => None,
    }
}

/// Everything the window procedure needs, owned by the window through its user data.
struct Tray {
    shared: SharedState,
    shown_mode_counter: u64,
    taskbar_created: u32,
    /// Whether the balloon on screen offers an update, so a click on it installs.
    balloon_offers_update: bool,
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
pub fn run(shared: SharedState, restarted_after_crash: bool) -> Result<()> {
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
        balloon_offers_update: false,
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
    if restarted_after_crash {
        tray.show_balloon(
            hwnd,
            "oh-key đã khởi động lại",
            "oh-key vừa dừng đột ngột và đã tự khởi động lại.",
            true,
        );
    }
    start_update_checks(hwnd);
    run_message_loop(tray, hwnd);
    drop(unsafe { Box::from_raw(tray_pointer) });
    Ok(())
}

/// Measurement builds (`OHKEY_NO_HOOK`, `OHKEY_NO_UPDATES`) never check for updates.
fn start_update_checks(hwnd: HWND) {
    let disabled = |name: &str| std::env::var_os(name).is_some_and(|value| value == "1");
    if disabled("OHKEY_NO_HOOK") || disabled("OHKEY_NO_UPDATES") {
        return;
    }
    update_service::start(hwnd, MESSAGE_UPDATE_NOTICE);
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
                NOTIFICATION_BALLOON_CLICKED => tray.balloon_clicked(),
                _ => {}
            }
            LRESULT(0)
        }
        MESSAGE_OPEN_SETTINGS => {
            let _ = settings_window::open();
            LRESULT(0)
        }
        MESSAGE_UPDATE_NOTICE => {
            tray.show_update_notices(hwnd);
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

    /// A balloon in the notification area. The fields are filled in local buffers and moved in
    /// whole, because the struct is packed on x86.
    fn show_balloon(&mut self, hwnd: HWND, title: &str, text: &str, warning: bool) {
        let mut title_buffer = [0u16; 64];
        let mut text_buffer = [0u16; 256];
        fill_wide(&mut title_buffer, title);
        fill_wide(&mut text_buffer, text);
        let data = NOTIFYICONDATAW {
            cbSize: size_of::<NOTIFYICONDATAW>() as u32,
            hWnd: hwnd,
            uID: 1,
            uFlags: NIF_INFO,
            szInfo: text_buffer,
            szInfoTitle: title_buffer,
            dwInfoFlags: if warning { NIIF_WARNING } else { NIIF_INFO },
            ..Default::default()
        };
        let _ = unsafe { Shell_NotifyIconW(NIM_MODIFY, &data) };
    }

    fn show_update_notices(&mut self, hwnd: HWND) {
        settings_window::refresh();
        for notice in update_service::take_notices() {
            if let Notice::Installed(version) = &notice {
                self.restart_into(hwnd, version);
                continue;
            }
            self.balloon_offers_update = matches!(notice, Notice::Available(_));
            if let Some((title, text, warning)) = balloon_for(&notice) {
                self.show_balloon(hwnd, &title, &text, warning);
            }
        }
    }

    /// The new exe is in place: start it and leave. If it cannot start, the files are put
    /// back and this version keeps running.
    fn restart_into(&mut self, hwnd: HWND, version: &str) {
        match updater::restart_into_new_version() {
            Ok(()) => {
                app_log::write(&format!("restarting into {version}"));
                unsafe {
                    let _ = DestroyWindow(hwnd);
                }
            }
            Err(error) => {
                let message = format!("update to {version} failed: {error}");
                app_log::write(&message);
                report::submit(report::ReportEvent {
                    write_to_log: false,
                    ..report::ReportEvent::new(report::EventKind::UpdateFailed, &message)
                });
                self.show_balloon(hwnd, "Không cập nhật được", &error, true);
            }
        }
    }

    fn balloon_clicked(&mut self) {
        if self.balloon_offers_update {
            self.balloon_offers_update = false;
            update_service::install_pending();
        }
    }

    fn show_menu(&mut self, hwnd: HWND) {
        let vietnamese = self.shared.vietnamese();
        let current_method: InputMethod = load_settings().engine.input_method;
        let Ok(menu) = (unsafe { CreatePopupMenu() }) else {
            return;
        };
        build_menu(
            menu,
            vietnamese,
            current_method,
            update_service::pending_version().as_deref(),
        );
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
            MENU_CHECK_UPDATES => update_service::check_now(),
            MENU_INSTALL_UPDATE => update_service::install_pending(),
            MENU_OPEN_LOG => app_log::open_folder(),
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

/// Copies `text` into a fixed buffer, leaving room for the closing zero.
fn fill_wide(buffer: &mut [u16], text: &str) {
    let usable = buffer.len() - 1;
    for (slot, unit) in buffer.iter_mut().take(usable).zip(text.encode_utf16()) {
        *slot = unit;
    }
}

fn build_menu(
    menu: HMENU,
    vietnamese: bool,
    current_method: InputMethod,
    pending_update: Option<&str>,
) {
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
    if let Some(version) = pending_update {
        append_item(
            menu,
            MF_STRING,
            MENU_INSTALL_UPDATE,
            &label_install_update(version),
        );
    }
    append_item(menu, MF_STRING, MENU_CHECK_UPDATES, LABEL_CHECK_UPDATES);
    append_item(menu, MF_STRING, MENU_OPEN_LOG, LABEL_OPEN_LOG);
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
            MENU_CHECK_UPDATES,
            MENU_INSTALL_UPDATE,
            MENU_OPEN_LOG,
        ];
        assert!(fixed.iter().all(|id| *id < MENU_METHOD_FIRST));
        let mut sorted = fixed.to_vec();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), fixed.len());
    }

    #[test]
    fn the_update_item_names_the_version() {
        assert_eq!(label_install_update("0.3.0"), "Có bản mới 0.3.0 — Cập nhật");
    }

    #[test]
    fn each_notice_has_a_balloon_except_the_restart() {
        let (_, _, warning) = balloon_for(&Notice::Failed("no network".into())).unwrap();
        assert!(warning);
        let (_, text, warning) = balloon_for(&Notice::Available("0.3.0".into())).unwrap();
        assert!(text.contains("0.3.0") && !warning);
        assert!(balloon_for(&Notice::UpToDate).is_some());
        assert!(balloon_for(&Notice::Installed("0.3.0".into())).is_none());
    }

    #[test]
    fn a_long_balloon_text_is_cut_and_the_last_slot_is_left_for_the_zero() {
        let mut buffer = [0xffffu16; 8];
        fill_wide(&mut buffer, "abcdefghijkl");
        assert_eq!(buffer[6], u16::from(b'g'));
        assert_eq!(buffer[7], 0xffff);
    }

    #[test]
    fn the_tooltip_names_the_mode() {
        assert!(tooltip(true).contains("Việt"));
        assert!(tooltip(false).contains("English"));
    }
}
