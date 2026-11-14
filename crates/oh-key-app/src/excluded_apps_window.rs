//! The list of apps where oh-key stays off, opened from the control panel.
//! It looks like the control panel: ink background, mono labels, quiet text buttons.
//! Left, the apps on the list and a field to add one by name; right, the windows open now,
//! each with the process name and the window title. Every change is saved and published at once.

use std::ops::Range;
use std::path::Path;
use std::sync::atomic::{AtomicIsize, Ordering};

use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, TRUE, WPARAM};
use windows::Win32::Graphics::Direct2D::Common::{
    D2D_SIZE_U, D2D1_ALPHA_MODE_PREMULTIPLIED, D2D1_COLOR_F, D2D1_PIXEL_FORMAT,
};
use windows::Win32::Graphics::Direct2D::{
    D2D1_FEATURE_LEVEL_DEFAULT, D2D1_HWND_RENDER_TARGET_PROPERTIES, D2D1_PRESENT_OPTIONS_NONE,
    D2D1_RENDER_TARGET_PROPERTIES, D2D1_RENDER_TARGET_TYPE_DEFAULT, D2D1_RENDER_TARGET_USAGE_NONE,
    D2D1_ROUNDED_RECT, ID2D1HwndRenderTarget, ID2D1RenderTarget, ID2D1SolidColorBrush,
};
use windows::Win32::Graphics::Dwm::{
    DWMWINDOWATTRIBUTE, DwmGetWindowAttribute, DwmSetWindowAttribute,
};
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;
use windows::Win32::Graphics::Gdi::{
    GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromPoint, ValidateRect,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::GetCurrentProcessId;
use windows::Win32::UI::HiDpi::{AdjustWindowRectExForDpi, GetDpiForWindow};
use windows::Win32::UI::Input::KeyboardAndMouse::VK_ESCAPE;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, EnumWindows, GW_OWNER, GWL_EXSTYLE,
    GWLP_USERDATA, GetClientRect, GetCursorPos, GetWindow, GetWindowLongPtrW, GetWindowTextW,
    IDC_ARROW, IDC_HAND, IsWindow, IsWindowVisible, LoadCursorW, RegisterClassExW, SW_SHOWNORMAL,
    SWP_NOZORDER, SetCursor, SetForegroundWindow, SetWindowLongPtrW, SetWindowPos, ShowWindow,
    WM_ACTIVATE, WM_CHAR, WM_KEYDOWN, WM_LBUTTONUP, WM_MOUSEMOVE, WM_MOUSEWHEEL, WM_NCDESTROY,
    WM_PAINT, WM_SETCURSOR, WNDCLASSEXW, WS_CAPTION, WS_EX_LEFT, WS_EX_TOOLWINDOW, WS_MINIMIZEBOX,
    WS_OVERLAPPED, WS_SYSMENU,
};
use windows::core::{BOOL, Interface, Result, w};

use crate::graphics::Graphics;
use crate::hook::{process_exe_name, window_exe_name};
use crate::layout::{self, Rect};
use crate::logo;
use crate::offscreen::Offscreen;
use crate::{icons, mode_icon, persistence, settings_window, style};

const WIDTH: f32 = 960.0;
const APP_ROWS_VISIBLE: usize = 8;
const RUNNING_ROWS_VISIBLE: usize = 10;
const RUNNING_NAME_WIDTH: f32 = 170.0;
const LONGEST_TYPED_NAME: usize = 60;
const WHEEL_STEP: i32 = 120;
const DWMWA_CLOAKED: DWMWINDOWATTRIBUTE = DWMWINDOWATTRIBUTE(14);

const TITLE: &str = "ứng dụng tắt oh-key";
const SECTION_ADD: &str = "Thêm theo tên";
const SECTION_APPS: &str = "Đang tắt oh-key";
const SECTION_RUNNING: &str = "Đang chạy";
const PLACEHOLDER: &str = "tên chương trình, ví dụ notepad.exe";
const NO_APPS: &str = "chưa có ứng dụng nào";
const NO_RUNNING: &str = "không có cửa sổ nào để thêm";
const BUTTON_ADD: &str = "thêm";
const BUTTON_REMOVE: &str = "xóa";

// ---------------------------------------------------------------------------------------
// The list as plain data, so every rule is unit tested without a window.
// ---------------------------------------------------------------------------------------

/// One open window's program: the executable name and a window title to recognise it by.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RunningApp {
    pub exe: String,
    pub title: String,
}

/// The name as `excluded_apps` keeps it: a lower case file name, `.exe` when no extension was typed.
pub fn normalize_app_name(input: &str) -> Option<String> {
    let file_name = input.trim().rsplit(['\\', '/']).next()?.trim();
    if file_name.is_empty() {
        return None;
    }
    let lower = file_name.to_lowercase();
    if lower.contains('.') {
        Some(lower)
    } else {
        Some(format!("{lower}.exe"))
    }
}

/// Adds `name` unless it is already listed. Returns whether the list changed.
pub fn add_app(apps: &mut Vec<String>, name: String) -> bool {
    if apps.contains(&name) {
        return false;
    }
    apps.push(name);
    true
}

/// One entry per program (the topmost window's title), sorted by name.
pub fn one_per_program(windows_top_first: Vec<RunningApp>) -> Vec<RunningApp> {
    let mut programs: Vec<RunningApp> = Vec::new();
    for window in windows_top_first {
        if !programs.iter().any(|program| program.exe == window.exe) {
            programs.push(window);
        }
    }
    programs.sort_by(|first, second| first.exe.cmp(&second.exe));
    programs
}

/// The running programs that are not on the list yet.
pub fn programs_to_offer(running: &[RunningApp], apps: &[String]) -> Vec<RunningApp> {
    running
        .iter()
        .filter(|program| !apps.contains(&program.exe))
        .cloned()
        .collect()
}

/// The first row shown after scrolling `offset` rows, kept inside the list.
pub fn clamp_scroll(offset: usize, total: usize, capacity: usize) -> usize {
    offset.min(total.saturating_sub(capacity))
}

pub fn scrolled(offset: usize, wheel_notches: i32, total: usize, capacity: usize) -> usize {
    let moved = (offset as i64 - i64::from(wheel_notches)).max(0) as usize;
    clamp_scroll(moved, total, capacity)
}

pub fn visible_rows(offset: usize, total: usize, capacity: usize) -> Range<usize> {
    let first = clamp_scroll(offset, total, capacity);
    first..(first + capacity).min(total)
}

fn column_width() -> f32 {
    (WIDTH - 2.0 * layout::PADDING - layout::COLUMN_GAP) / 2.0
}

fn column_left(column_index: usize) -> f32 {
    layout::PADDING + column_index as f32 * (column_width() + layout::COLUMN_GAP)
}

fn window_height() -> f32 {
    let left_column = 2.0 * layout::SECTION_HEIGHT
        + (1 + APP_ROWS_VISIBLE) as f32 * layout::ROW_HEIGHT
        + layout::SECTION_GAP;
    let right_column = layout::SECTION_HEIGHT + RUNNING_ROWS_VISIBLE as f32 * layout::ROW_HEIGHT;
    layout::TITLE_HEIGHT + left_column.max(right_column) + layout::PADDING
}

// ---------------------------------------------------------------------------------------
// The window
// ---------------------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Hit {
    AddTyped,
    Remove(usize),
    AddRunning(usize),
}

/// What the window shows, apart from the drawing surface.
struct View<'a> {
    apps: &'a [String],
    offered: &'a [RunningApp],
    input: &'a str,
    apps_scroll: usize,
    running_scroll: usize,
}

/// Set while the window is open, so a second request brings it forward.
static OPEN_WINDOW: AtomicIsize = AtomicIsize::new(0);

struct ExcludedAppsWindow {
    // The render target goes before the factories it came from.
    target: ID2D1RenderTarget,
    graphics: Graphics,
    hwnd: HWND,
    apps: Vec<String>,
    running: Vec<RunningApp>,
    input: String,
    apps_scroll: usize,
    running_scroll: usize,
    pointer: (f32, f32),
    hits: Vec<(Hit, Rect)>,
}

fn open_window_handle() -> Option<HWND> {
    let raw = OPEN_WINDOW.load(Ordering::Relaxed);
    if raw == 0 {
        return None;
    }
    let hwnd = HWND(raw as *mut _);
    unsafe { IsWindow(Some(hwnd)) }.as_bool().then_some(hwnd)
}

/// Opens the window, or brings the open one forward.
pub fn open(owner: Option<HWND>) {
    if let Some(existing) = open_window_handle() {
        unsafe {
            let _ = SetForegroundWindow(existing);
        }
        return;
    }
    let _ = try_open(owner);
}

fn try_open(owner: Option<HWND>) -> Result<()> {
    let graphics = Graphics::new()?;
    register_class(&graphics);
    let instance = unsafe { GetModuleHandleW(None)? };
    let style = WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_MINIMIZEBOX;
    let hwnd = unsafe {
        CreateWindowExW(
            WS_EX_LEFT,
            w!("oh-key.excluded-apps"),
            w!("oh-key ứng dụng tắt oh-key"),
            style,
            0,
            0,
            100,
            100,
            owner,
            None,
            Some(instance.into()),
            None,
        )?
    };
    let scale = place_window(hwnd, style);
    dark_frame(hwnd);
    let target = create_target(&graphics, hwnd, scale * 96.0)?;
    let window = Box::new(ExcludedAppsWindow {
        target,
        graphics,
        hwnd,
        apps: oh_key_ipc::load_settings().excluded_apps,
        running: running_programs(),
        input: String::new(),
        apps_scroll: 0,
        running_scroll: 0,
        pointer: (-1.0, -1.0),
        hits: Vec::new(),
    });
    unsafe {
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, Box::into_raw(window) as _);
        OPEN_WINDOW.store(hwnd.0 as isize, Ordering::Relaxed);
        let _ = ShowWindow(hwnd, SW_SHOWNORMAL);
        let _ = SetForegroundWindow(hwnd);
    }
    Ok(())
}

/// Sizes and centres the window for the monitor under the cursor; returns the scale used.
fn place_window(hwnd: HWND, style: windows::Win32::UI::WindowsAndMessaging::WINDOW_STYLE) -> f32 {
    let dpi = unsafe { GetDpiForWindow(hwnd) }.max(96);
    let mut cursor = Default::default();
    unsafe {
        let _ = GetCursorPos(&mut cursor);
    }
    let monitor = unsafe { MonitorFromPoint(cursor, MONITOR_DEFAULTTONEAREST) };
    let mut monitor_info = MONITORINFO {
        cbSize: size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    unsafe {
        let _ = GetMonitorInfoW(monitor, &mut monitor_info);
    }
    let work = monitor_info.rcWork;
    let mut borders = RECT::default();
    unsafe {
        let _ = AdjustWindowRectExForDpi(&mut borders, style, false, WS_EX_LEFT, dpi);
    }
    let room = (
        (work.right - work.left - (borders.right - borders.left)) as f32,
        (work.bottom - work.top - (borders.bottom - borders.top)) as f32,
    );
    let height_dips = window_height();
    let scale = layout::fitted_scale(dpi as f32 / 96.0, (WIDTH, height_dips), room);
    let mut frame = RECT {
        left: 0,
        top: 0,
        right: (WIDTH * scale).round() as i32,
        bottom: (height_dips * scale).round() as i32,
    };
    unsafe {
        let _ = AdjustWindowRectExForDpi(&mut frame, style, false, WS_EX_LEFT, dpi);
    }
    let width = frame.right - frame.left;
    let height = frame.bottom - frame.top;
    unsafe {
        let _ = SetWindowPos(
            hwnd,
            None,
            work.left + (work.right - work.left - width) / 2,
            work.top + (work.bottom - work.top - height) / 2,
            width,
            height,
            SWP_NOZORDER,
        );
    }
    scale
}

fn register_class(graphics: &Graphics) {
    let icon = mode_icon::icon_with(graphics, true);
    let class = WNDCLASSEXW {
        cbSize: size_of::<WNDCLASSEXW>() as u32,
        lpfnWndProc: Some(window_proc),
        hInstance: unsafe { GetModuleHandleW(None) }.unwrap_or_default().into(),
        hIcon: icon,
        lpszClassName: w!("oh-key.excluded-apps"),
        ..Default::default()
    };
    unsafe { RegisterClassExW(&class) };
}

fn dark_frame(hwnd: HWND) {
    let dark = BOOL(1);
    // The caption colour is a COLORREF, blue in the high byte: the ink colour 1F1E1D.
    let caption: u32 = 0x001D_1E1F;
    unsafe {
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWINDOWATTRIBUTE(20),
            &dark as *const BOOL as *const _,
            size_of::<BOOL>() as u32,
        );
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWINDOWATTRIBUTE(35),
            &caption as *const u32 as *const _,
            size_of::<u32>() as u32,
        );
    }
}

fn create_target(graphics: &Graphics, hwnd: HWND, dpi: f32) -> Result<ID2D1RenderTarget> {
    let mut client = RECT::default();
    unsafe { GetClientRect(hwnd, &mut client)? };
    let properties = D2D1_RENDER_TARGET_PROPERTIES {
        r#type: D2D1_RENDER_TARGET_TYPE_DEFAULT,
        pixelFormat: D2D1_PIXEL_FORMAT {
            format: DXGI_FORMAT_B8G8R8A8_UNORM,
            alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
        },
        dpiX: dpi,
        dpiY: dpi,
        usage: D2D1_RENDER_TARGET_USAGE_NONE,
        minLevel: D2D1_FEATURE_LEVEL_DEFAULT,
    };
    let hwnd_properties = D2D1_HWND_RENDER_TARGET_PROPERTIES {
        hwnd,
        pixelSize: D2D_SIZE_U {
            width: client.right.max(1) as u32,
            height: client.bottom.max(1) as u32,
        },
        presentOptions: D2D1_PRESENT_OPTIONS_NONE,
    };
    let target: ID2D1HwndRenderTarget = unsafe {
        graphics
            .d2d
            .CreateHwndRenderTarget(&properties, &hwnd_properties)?
    };
    target.cast()
}

unsafe extern "system" fn window_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    let window_pointer =
        unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } as *mut ExcludedAppsWindow;
    if window_pointer.is_null() {
        return unsafe { DefWindowProcW(hwnd, message, wparam, lparam) };
    }
    if message == WM_NCDESTROY {
        drop(unsafe { Box::from_raw(window_pointer) });
        unsafe { SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0) };
        OPEN_WINDOW.store(0, Ordering::Relaxed);
        return unsafe { DefWindowProcW(hwnd, message, wparam, lparam) };
    }
    let window = unsafe { &mut *window_pointer };
    match message {
        WM_PAINT => {
            window.paint();
            unsafe {
                let _ = ValidateRect(Some(hwnd), None);
            }
            LRESULT(0)
        }
        WM_SETCURSOR => {
            let shape = if window.hit_at(window.pointer).is_some() {
                IDC_HAND
            } else {
                IDC_ARROW
            };
            if let Ok(cursor) = unsafe { LoadCursorW(None, shape) } {
                unsafe { SetCursor(Some(cursor)) };
                return LRESULT(1);
            }
            unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
        }
        WM_MOUSEMOVE => {
            window.pointer = window.to_dips(lparam);
            window.paint();
            LRESULT(0)
        }
        WM_LBUTTONUP => {
            window.pointer = window.to_dips(lparam);
            if let Some(hit) = window.hit_at(window.pointer) {
                window.choose(hit);
            }
            window.paint();
            LRESULT(0)
        }
        WM_MOUSEWHEEL => {
            let wheel_delta = ((wparam.0 >> 16) & 0xFFFF) as u16 as i16 as i32;
            window.scroll(wheel_delta / WHEEL_STEP);
            window.paint();
            LRESULT(0)
        }
        WM_CHAR => {
            window.type_character(wparam.0 as u32);
            window.paint();
            LRESULT(0)
        }
        WM_ACTIVATE if wparam.0 & 0xFFFF != 0 => {
            // Back from another window: the programs open now may have changed.
            window.running = running_programs();
            window.paint();
            LRESULT(0)
        }
        WM_KEYDOWN if wparam.0 as u16 == VK_ESCAPE.0 => {
            unsafe {
                let _ = DestroyWindow(hwnd);
            }
            LRESULT(0)
        }
        _ => unsafe { DefWindowProcW(hwnd, message, wparam, lparam) },
    }
}

impl ExcludedAppsWindow {
    fn to_dips(&self, lparam: LPARAM) -> (f32, f32) {
        let pixel_x = (lparam.0 & 0xFFFF) as u16 as i16 as i32;
        let pixel_y = ((lparam.0 >> 16) & 0xFFFF) as u16 as i16 as i32;
        let mut dpi_x = 96.0;
        let mut dpi_y = 96.0;
        unsafe { self.target.GetDpi(&mut dpi_x, &mut dpi_y) };
        layout::pixels_to_dips((pixel_x, pixel_y), (dpi_x, dpi_y))
    }

    fn hit_at(&self, point: (f32, f32)) -> Option<Hit> {
        layout::hit_test(&self.hits, point)
    }

    fn offered(&self) -> Vec<RunningApp> {
        programs_to_offer(&self.running, &self.apps)
    }

    fn choose(&mut self, hit: Hit) {
        let changed = match hit {
            Hit::AddTyped => self.add_typed_name(),
            Hit::Remove(index) => {
                self.apps.remove(index);
                true
            }
            Hit::AddRunning(index) => {
                let offered = self.offered();
                offered
                    .get(index)
                    .is_some_and(|program| add_app(&mut self.apps, program.exe.clone()))
            }
        };
        if changed {
            self.keep_scroll_inside_lists();
            self.save();
        }
    }

    fn add_typed_name(&mut self) -> bool {
        let Some(name) = normalize_app_name(&self.input) else {
            return false;
        };
        self.input.clear();
        add_app(&mut self.apps, name)
    }

    fn type_character(&mut self, code: u32) {
        const BACKSPACE: u32 = 0x08;
        const ENTER: u32 = 0x0D;
        match code {
            BACKSPACE => {
                self.input.pop();
            }
            ENTER => {
                if self.add_typed_name() {
                    self.keep_scroll_inside_lists();
                    self.save();
                }
            }
            _ => {
                let Some(character) = char::from_u32(code) else {
                    return;
                };
                if !character.is_control() && self.input.chars().count() < LONGEST_TYPED_NAME {
                    self.input.push(character);
                }
            }
        }
    }

    fn scroll(&mut self, wheel_notches: i32) {
        if self.pointer.0 < column_left(1) - layout::COLUMN_GAP / 2.0 {
            self.apps_scroll = scrolled(
                self.apps_scroll,
                wheel_notches,
                self.apps.len(),
                APP_ROWS_VISIBLE,
            );
        } else {
            self.running_scroll = scrolled(
                self.running_scroll,
                wheel_notches,
                self.offered().len(),
                RUNNING_ROWS_VISIBLE,
            );
        }
    }

    fn keep_scroll_inside_lists(&mut self) {
        self.apps_scroll = clamp_scroll(self.apps_scroll, self.apps.len(), APP_ROWS_VISIBLE);
        self.running_scroll = clamp_scroll(
            self.running_scroll,
            self.offered().len(),
            RUNNING_ROWS_VISIBLE,
        );
    }

    /// Writes the list into `config.toml` and publishes it, so the hook leaves or enters that app at once.
    fn save(&self) {
        let mut settings = oh_key_ipc::load_settings();
        settings.excluded_apps = self.apps.clone();
        persistence::save_and_publish(&settings);
        settings_window::refresh();
    }

    fn paint(&mut self) {
        let target = self.target.clone();
        let offered = self.offered();
        let hovered = self.hit_at(self.pointer);
        let view = View {
            apps: &self.apps,
            offered: &offered,
            input: &self.input,
            apps_scroll: self.apps_scroll,
            running_scroll: self.running_scroll,
        };
        self.hits = draw(&target, &self.graphics, &view, hovered);
        let _ = self.hwnd;
    }
}

// ---------------------------------------------------------------------------------------
// Which programs are open
// ---------------------------------------------------------------------------------------

/// Programs with a visible window of their own, each with the title of its topmost window.
fn running_programs() -> Vec<RunningApp> {
    let mut windows: Vec<RunningApp> = Vec::new();
    unsafe {
        let _ = EnumWindows(
            Some(collect_window),
            LPARAM(&mut windows as *mut _ as isize),
        );
    }
    let own_name = process_exe_name(unsafe { GetCurrentProcessId() }).unwrap_or_default();
    let programs = one_per_program(windows);
    programs
        .into_iter()
        .filter(|program| program.exe != own_name)
        .collect()
}

unsafe extern "system" fn collect_window(hwnd: HWND, lparam: LPARAM) -> BOOL {
    let windows = unsafe { &mut *(lparam.0 as *mut Vec<RunningApp>) };
    if let Some(program) = describe_window(hwnd) {
        windows.push(program);
    }
    TRUE
}

fn describe_window(hwnd: HWND) -> Option<RunningApp> {
    if !unsafe { IsWindowVisible(hwnd) }.as_bool() || is_cloaked(hwnd) {
        return None;
    }
    if unsafe { GetWindow(hwnd, GW_OWNER) }.is_ok_and(|owner| !owner.is_invalid()) {
        return None;
    }
    let extended_style = unsafe { GetWindowLongPtrW(hwnd, GWL_EXSTYLE) } as u32;
    if extended_style & WS_EX_TOOLWINDOW.0 != 0 {
        return None;
    }
    let mut buffer = [0u16; 256];
    let length = unsafe { GetWindowTextW(hwnd, &mut buffer) }.max(0) as usize;
    if length == 0 {
        return None;
    }
    Some(RunningApp {
        exe: window_exe_name(hwnd)?,
        title: String::from_utf16_lossy(&buffer[..length]),
    })
}

/// Windows 10 and 11 keep windows of suspended apps alive but hidden from the user.
fn is_cloaked(hwnd: HWND) -> bool {
    let mut cloaked = 0u32;
    let result = unsafe {
        DwmGetWindowAttribute(
            hwnd,
            DWMWA_CLOAKED,
            &mut cloaked as *mut u32 as *mut _,
            size_of::<u32>() as u32,
        )
    };
    result.is_ok() && cloaked != 0
}

// ---------------------------------------------------------------------------------------
// Drawing
// ---------------------------------------------------------------------------------------

struct Brushes {
    text: ID2D1SolidColorBrush,
    dim: ID2D1SolidColorBrush,
    accent: ID2D1SolidColorBrush,
    hover: ID2D1SolidColorBrush,
}

fn create_brushes(target: &ID2D1RenderTarget) -> Option<Brushes> {
    let brush = |color: D2D1_COLOR_F| unsafe { target.CreateSolidColorBrush(&color, None) }.ok();
    Some(Brushes {
        text: brush(style::TEXT)?,
        dim: brush(style::TEXT_DIM)?,
        accent: brush(style::ACCENT)?,
        hover: brush(style::HOVER)?,
    })
}

struct Canvas<'a> {
    target: &'a ID2D1RenderTarget,
    graphics: &'a Graphics,
    brushes: Brushes,
    hovered: Option<Hit>,
    hits: Vec<(Hit, Rect)>,
}

fn draw(
    target: &ID2D1RenderTarget,
    graphics: &Graphics,
    view: &View,
    hovered: Option<Hit>,
) -> Vec<(Hit, Rect)> {
    let Some(brushes) = create_brushes(target) else {
        return Vec::new();
    };
    unsafe {
        target.BeginDraw();
        target.Clear(Some(&style::INK));
    }
    let mut canvas = Canvas {
        target,
        graphics,
        brushes,
        hovered,
        hits: Vec::new(),
    };
    canvas.draw_title();
    canvas.draw_left_column(view);
    canvas.draw_right_column(view);
    unsafe {
        let _ = target.EndDraw(None, None);
    }
    canvas.hits
}

impl Canvas<'_> {
    fn draw_title(&self) {
        let logo_area = logo::square(layout::PADDING, 36.0, style::LOGO_SIZE);
        logo::draw(self.target, &self.graphics.d2d, logo_area);
        self.graphics.draw_label(
            self.target,
            TITLE,
            layout::PADDING + 28.0,
            27.0,
            400.0,
            &self.brushes.text,
        );
    }

    fn draw_section(&self, column_index: usize, top: f32, icon: char, title: &str, note: &str) {
        let left = column_left(column_index);
        let right = left + column_width();
        let icon_area = Rect {
            left: left - 2.0,
            top: top + 10.0,
            right: left + 20.0,
            bottom: top + 34.0,
        };
        self.graphics
            .draw_icon(self.target, icon, icon_area, &self.brushes.accent);
        self.graphics.draw_label(
            self.target,
            title,
            left + layout::ICON_SLOT,
            top + 14.0,
            right,
            &self.brushes.accent,
        );
        if !note.is_empty() {
            let note_width = self.graphics.measure(note).0;
            self.graphics.draw_label(
                self.target,
                note,
                right - note_width - 4.0,
                top + 14.0,
                right,
                &self.brushes.dim,
            );
        }
    }

    fn text_top(&self, row_top: f32) -> f32 {
        let text_height = self.graphics.measure("x").1;
        row_top + (layout::ROW_HEIGHT - text_height) / 2.0
    }

    /// A quiet text button at the right edge of a row. A `None` hit draws it unavailable.
    fn draw_button(
        &mut self,
        column_index: usize,
        row_top: f32,
        label: &str,
        hit: Option<Hit>,
    ) -> Rect {
        let right_edge = column_left(column_index) + column_width();
        let width = self.graphics.measure(label).0;
        let button = layout::button_rects(row_top, right_edge, &[width])[0];
        if hit.is_some() && self.hovered == hit {
            let shape = D2D1_ROUNDED_RECT {
                rect: button.into(),
                radiusX: style::BUTTON_RADIUS,
                radiusY: style::BUTTON_RADIUS,
            };
            unsafe {
                self.target
                    .FillRoundedRectangle(&shape, &self.brushes.hover)
            };
        }
        let brush = if hit.is_some() {
            &self.brushes.text
        } else {
            &self.brushes.dim
        };
        self.graphics.draw_label(
            self.target,
            label,
            button.left + layout::BUTTON_PADDING / 2.0,
            self.text_top(row_top),
            button.right,
            brush,
        );
        if let Some(hit) = hit {
            self.hits.push((hit, button));
        }
        button
    }

    fn draw_empty_row(&self, column_index: usize, row_top: f32, text: &str) {
        let left = column_left(column_index);
        self.graphics.draw_label(
            self.target,
            text,
            left + layout::ICON_SLOT,
            self.text_top(row_top),
            left + column_width(),
            &self.brushes.dim,
        );
    }

    fn draw_row_icon(&self, column_index: usize, row_top: f32) {
        let left = column_left(column_index);
        let icon_area = Rect {
            left: left - 2.0,
            top: row_top,
            right: left + 20.0,
            bottom: row_top + layout::ROW_HEIGHT,
        };
        self.graphics
            .draw_icon(self.target, icons::WINDOW, icon_area, &self.brushes.dim);
    }

    fn draw_left_column(&mut self, view: &View) {
        let left = column_left(0);
        let mut top = layout::TITLE_HEIGHT;
        self.draw_section(0, top, icons::EDIT, SECTION_ADD, "");
        top += layout::SECTION_HEIGHT;
        self.draw_input_row(view, top);
        top += layout::ROW_HEIGHT + layout::SECTION_GAP;

        let rows = visible_rows(view.apps_scroll, view.apps.len(), APP_ROWS_VISIBLE);
        let note = scroll_note(&rows, view.apps.len());
        self.draw_section(0, top, icons::WINDOW, SECTION_APPS, &note);
        top += layout::SECTION_HEIGHT;
        if view.apps.is_empty() {
            self.draw_empty_row(0, top, NO_APPS);
        }
        for index in rows {
            self.draw_row_icon(0, top);
            let button = self.draw_button(0, top, BUTTON_REMOVE, Some(Hit::Remove(index)));
            self.graphics.draw_label(
                self.target,
                &view.apps[index],
                left + layout::ICON_SLOT,
                self.text_top(top),
                button.left - 8.0,
                &self.brushes.text,
            );
            top += layout::ROW_HEIGHT;
        }
    }

    fn draw_input_row(&mut self, view: &View, row_top: f32) {
        let left = column_left(0);
        let addable = normalize_app_name(view.input).is_some();
        let hit = addable.then_some(Hit::AddTyped);
        let button = self.draw_button(0, row_top, BUTTON_ADD, hit);
        let field = Rect {
            left: left + layout::ICON_SLOT - 8.0,
            top: row_top + (layout::ROW_HEIGHT - layout::BUTTON_HEIGHT) / 2.0,
            right: button.left - layout::BUTTON_GAP - 8.0,
            bottom: row_top + (layout::ROW_HEIGHT + layout::BUTTON_HEIGHT) / 2.0,
        };
        let shape = D2D1_ROUNDED_RECT {
            rect: field.into(),
            radiusX: style::BUTTON_RADIUS,
            radiusY: style::BUTTON_RADIUS,
        };
        unsafe {
            self.target
                .FillRoundedRectangle(&shape, &self.brushes.hover)
        };
        let text_left = field.left + layout::BUTTON_PADDING / 2.0;
        let text_top = self.text_top(row_top);
        if view.input.is_empty() {
            self.graphics.draw_label(
                self.target,
                PLACEHOLDER,
                text_left,
                text_top,
                field.right - 8.0,
                &self.brushes.dim,
            );
        } else {
            self.graphics.draw_label(
                self.target,
                view.input,
                text_left,
                text_top,
                field.right - 8.0,
                &self.brushes.text,
            );
        }
        let typed_width = self.graphics.measure(view.input).0;
        let caret_left = (text_left + typed_width).min(field.right - 8.0);
        let caret = D2D1_ROUNDED_RECT {
            rect: Rect {
                left: caret_left,
                top: field.top + 6.0,
                right: caret_left + 1.5,
                bottom: field.bottom - 6.0,
            }
            .into(),
            radiusX: 0.75,
            radiusY: 0.75,
        };
        unsafe {
            self.target
                .FillRoundedRectangle(&caret, &self.brushes.accent)
        };
    }

    fn draw_right_column(&mut self, view: &View) {
        let left = column_left(1);
        let mut top = layout::TITLE_HEIGHT;
        let rows = visible_rows(
            view.running_scroll,
            view.offered.len(),
            RUNNING_ROWS_VISIBLE,
        );
        let note = scroll_note(&rows, view.offered.len());
        self.draw_section(1, top, icons::SYNC, SECTION_RUNNING, &note);
        top += layout::SECTION_HEIGHT;
        if view.offered.is_empty() {
            self.draw_empty_row(1, top, NO_RUNNING);
        }
        for index in rows {
            let program = &view.offered[index];
            self.draw_row_icon(1, top);
            let button = self.draw_button(1, top, BUTTON_ADD, Some(Hit::AddRunning(index)));
            let name_left = left + layout::ICON_SLOT;
            let title_left = name_left + RUNNING_NAME_WIDTH;
            self.graphics.draw_label(
                self.target,
                &program.exe,
                name_left,
                self.text_top(top),
                title_left - 8.0,
                &self.brushes.text,
            );
            self.graphics.draw_label(
                self.target,
                &program.title,
                title_left,
                self.text_top(top),
                button.left - 8.0,
                &self.brushes.dim,
            );
            top += layout::ROW_HEIGHT;
        }
    }
}

/// "3–10 / 14" when the list is longer than its rows, else nothing.
fn scroll_note(rows: &Range<usize>, total: usize) -> String {
    if rows.len() >= total {
        return String::new();
    }
    format!("{}–{} / {}", rows.start + 1, rows.end, total)
}

/// `oh-key.exe --render-png excluded-apps out.png`: the window drawn into a file, with the
/// settings' list and the programs open now.
/// Draws sample apps, never this machine's settings or open windows: the render ships in the
/// docs, and window titles carry private text.
pub fn render_png(graphics: &Graphics, path: &Path) -> Result<()> {
    let offscreen = Offscreen::new(graphics, WIDTH as u32, window_height().round() as u32)?;
    let apps = vec!["code.exe".to_owned(), "windowsterminal.exe".to_owned()];
    let running = sample_running_programs();
    let offered = programs_to_offer(&running, &apps);
    let view = View {
        apps: &apps,
        offered: &offered,
        input: "",
        apps_scroll: 0,
        running_scroll: 0,
    };
    draw(&offscreen.target, graphics, &view, None);
    offscreen.save_png(path)
}

fn sample_running_programs() -> Vec<RunningApp> {
    [
        ("notepad.exe", "Untitled - Notepad"),
        ("msedge.exe", "New tab - Microsoft Edge"),
        ("winword.exe", "Document1 - Word"),
        ("explorer.exe", "File Explorer"),
    ]
    .into_iter()
    .map(|(exe, title)| RunningApp {
        exe: exe.to_owned(),
        title: title.to_owned(),
    })
    .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn program(exe: &str, title: &str) -> RunningApp {
        RunningApp {
            exe: exe.to_owned(),
            title: title.to_owned(),
        }
    }

    #[test]
    fn a_typed_name_becomes_a_lower_case_file_name_with_an_extension() {
        assert_eq!(
            normalize_app_name(" Notepad "),
            Some("notepad.exe".to_owned())
        );
        assert_eq!(normalize_app_name("Code.EXE"), Some("code.exe".to_owned()));
        assert_eq!(
            normalize_app_name("C:\\Games\\Quake\\Quake.exe"),
            Some("quake.exe".to_owned())
        );
    }

    #[test]
    fn an_empty_name_is_not_a_program() {
        assert_eq!(normalize_app_name(""), None);
        assert_eq!(normalize_app_name("   "), None);
        assert_eq!(normalize_app_name("C:\\Games\\"), None);
    }

    #[test]
    fn a_program_is_listed_once() {
        let mut apps = vec!["notepad.exe".to_owned()];
        assert!(!add_app(&mut apps, "notepad.exe".to_owned()));
        assert!(add_app(&mut apps, "code.exe".to_owned()));
        assert_eq!(apps, ["notepad.exe", "code.exe"]);
    }

    #[test]
    fn one_entry_per_program_keeps_the_topmost_title_and_sorts_by_name() {
        let windows = vec![
            program("zed.exe", "main.rs"),
            program("chrome.exe", "Top tab"),
            program("chrome.exe", "Older window"),
        ];
        assert_eq!(
            one_per_program(windows),
            [
                program("chrome.exe", "Top tab"),
                program("zed.exe", "main.rs")
            ]
        );
    }

    #[test]
    fn listed_programs_are_not_offered_again() {
        let running = vec![program("a.exe", "A"), program("b.exe", "B")];
        let offered = programs_to_offer(&running, &["a.exe".to_owned()]);
        assert_eq!(offered, [program("b.exe", "B")]);
    }

    #[test]
    fn scrolling_stays_inside_the_list() {
        assert_eq!(clamp_scroll(5, 3, 8), 0);
        assert_eq!(clamp_scroll(9, 12, 8), 4);
        assert_eq!(scrolled(0, 1, 12, 8), 0);
        assert_eq!(scrolled(0, -1, 12, 8), 1);
        assert_eq!(scrolled(4, -3, 12, 8), 4);
        assert_eq!(scrolled(2, 1, 12, 8), 1);
    }

    #[test]
    fn visible_rows_are_the_window_into_the_list() {
        assert_eq!(visible_rows(0, 3, 8), 0..3);
        assert_eq!(visible_rows(2, 12, 8), 2..10);
        assert_eq!(visible_rows(99, 12, 8), 4..12);
        assert_eq!(visible_rows(0, 0, 8), 0..0);
    }

    #[test]
    fn the_scroll_note_appears_only_when_rows_are_hidden() {
        assert_eq!(scroll_note(&(0..3), 3), "");
        assert_eq!(scroll_note(&(2..10), 12), "3–10 / 12");
    }

    #[test]
    fn both_columns_fit_inside_the_window() {
        let rows_bottom = layout::TITLE_HEIGHT
            + layout::SECTION_HEIGHT
            + RUNNING_ROWS_VISIBLE as f32 * layout::ROW_HEIGHT;
        assert!(rows_bottom <= window_height() - layout::PADDING);
        assert!(column_left(1) + column_width() <= WIDTH - layout::PADDING + 0.01);
    }
}
