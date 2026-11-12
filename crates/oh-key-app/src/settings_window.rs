//! The control panel: every option in two columns of groups, each a row of quiet text
//! buttons, as in oh-snap's settings window. A click saves at once and publishes the change
//! to the running hook.

use std::path::Path;
use std::sync::atomic::{AtomicIsize, Ordering};

use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Direct2D::Common::{
    D2D_SIZE_U, D2D1_ALPHA_MODE_PREMULTIPLIED, D2D1_COLOR_F, D2D1_PIXEL_FORMAT,
};
use windows::Win32::Graphics::Direct2D::{
    D2D1_FEATURE_LEVEL_DEFAULT, D2D1_HWND_RENDER_TARGET_PROPERTIES, D2D1_PRESENT_OPTIONS_NONE,
    D2D1_RENDER_TARGET_PROPERTIES, D2D1_RENDER_TARGET_TYPE_DEFAULT, D2D1_RENDER_TARGET_USAGE_NONE,
    D2D1_ROUNDED_RECT, ID2D1HwndRenderTarget, ID2D1RenderTarget, ID2D1SolidColorBrush,
};
use windows::Win32::Graphics::Dwm::{DWMWINDOWATTRIBUTE, DwmSetWindowAttribute};
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;
use windows::Win32::Graphics::Gdi::{
    GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromPoint, ValidateRect,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::HiDpi::{AdjustWindowRectExForDpi, GetDpiForWindow};
use windows::Win32::UI::Input::KeyboardAndMouse::VK_ESCAPE;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, GWLP_USERDATA, GetClientRect, GetCursorPos,
    GetWindowLongPtrW, IDC_ARROW, IDC_HAND, IsWindow, LoadCursorW, RegisterClassExW, SW_SHOWNORMAL,
    SWP_NOZORDER, SetCursor, SetForegroundWindow, SetWindowLongPtrW, SetWindowPos, ShowWindow,
    WM_KEYDOWN, WM_LBUTTONUP, WM_MOUSEMOVE, WM_NCDESTROY, WM_PAINT, WM_SETCURSOR, WNDCLASSEXW,
    WS_CAPTION, WS_EX_LEFT, WS_MINIMIZEBOX, WS_OVERLAPPED, WS_SYSMENU,
};
use windows::core::{BOOL, Interface, Result, w};

use crate::graphics::Graphics;
use crate::layout::{self, Rect};
use crate::offscreen::Offscreen;
use crate::settings_model::{self, Effect, Field, Row, Section, Tool};
use crate::{
    converter_window, excluded_apps_window, macros_window, mode_icon, persistence, startup, style,
};
use oh_key_ipc::Settings;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Hit {
    field: Field,
    option: usize,
}

/// Set while the window is open, so a second request brings it forward.
static OPEN_WINDOW: AtomicIsize = AtomicIsize::new(0);

struct SettingsWindow {
    graphics: Graphics,
    hwnd: HWND,
    target: ID2D1RenderTarget,
    settings: Settings,
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

fn window_height(settings: &Settings) -> f32 {
    let [left, right] = settings_model::columns(settings);
    layout::window_height(
        &settings_model::rows_per_section(&left),
        &settings_model::rows_per_section(&right),
    )
}

/// Opens the window, or brings the open one forward.
pub fn open() -> Result<()> {
    if let Some(existing) = open_window_handle() {
        unsafe {
            let _ = SetForegroundWindow(existing);
        }
        return Ok(());
    }
    let graphics = Graphics::new()?;
    register_class(&graphics);
    let settings = oh_key_ipc::load_settings();
    let instance = unsafe { GetModuleHandleW(None)? };
    let style = WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_MINIMIZEBOX;
    let hwnd = unsafe {
        CreateWindowExW(
            WS_EX_LEFT,
            w!("oh-key.settings"),
            w!("oh-key bảng điều khiển"),
            style,
            0,
            0,
            100,
            100,
            None,
            None,
            Some(instance.into()),
            None,
        )?
    };
    // Size and centre the window for the DPI of the monitor it opens on.
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
    let height_dips = window_height(&settings);
    let scale = layout::fitted_scale(dpi as f32 / 96.0, (layout::WIDTH, height_dips), room);
    let mut frame = RECT {
        left: 0,
        top: 0,
        right: (layout::WIDTH * scale).round() as i32,
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
    dark_frame(hwnd);
    let target = create_target(&graphics, hwnd, scale * 96.0)?;
    let window = Box::new(SettingsWindow {
        graphics,
        hwnd,
        target,
        settings,
        pointer: (-1.0, -1.0),
        hits: Vec::new(),
    });
    unsafe {
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, Box::into_raw(window) as isize);
        OPEN_WINDOW.store(hwnd.0 as isize, Ordering::Relaxed);
        let _ = ShowWindow(hwnd, SW_SHOWNORMAL);
        let _ = SetForegroundWindow(hwnd);
    }
    Ok(())
}

/// Re-reads the settings into the open window, after something else changed them.
pub fn refresh() {
    let Some(hwnd) = open_window_handle() else {
        return;
    };
    let window_pointer = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } as *mut SettingsWindow;
    if window_pointer.is_null() {
        return;
    }
    let window = unsafe { &mut *window_pointer };
    window.settings = oh_key_ipc::load_settings();
    window.paint();
}

fn register_class(graphics: &Graphics) {
    let icon = mode_icon::icon_with(graphics, true);
    let class = WNDCLASSEXW {
        cbSize: size_of::<WNDCLASSEXW>() as u32,
        lpfnWndProc: Some(window_proc),
        hInstance: unsafe { GetModuleHandleW(None) }.unwrap_or_default().into(),
        hIcon: icon,
        lpszClassName: w!("oh-key.settings"),
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
    let window_pointer = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } as *mut SettingsWindow;
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
        WM_KEYDOWN if wparam.0 as u16 == VK_ESCAPE.0 => {
            unsafe {
                let _ = DestroyWindow(hwnd);
            }
            LRESULT(0)
        }
        _ => unsafe { DefWindowProcW(hwnd, message, wparam, lparam) },
    }
}

impl SettingsWindow {
    fn graphics(&self) -> &Graphics {
        &self.graphics
    }

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

    /// Applies one click, saves, and publishes it to the hook.
    fn choose(&mut self, hit: Hit) {
        match settings_model::apply_choice(&mut self.settings, hit.field, hit.option) {
            Effect::Nothing => {}
            Effect::Save => persistence::save_and_publish(&self.settings),
            Effect::SaveAndApplyStartup => {
                persistence::save_and_publish(&self.settings);
                startup::set_start_at_sign_in(self.settings.start_at_sign_in);
            }
            Effect::Open(tool) => self.open_tool(tool),
        }
    }

    fn open_tool(&self, tool: Tool) {
        let owner = Some(self.hwnd);
        match tool {
            Tool::Macros => macros_window::open(owner),
            Tool::ExcludedApps => excluded_apps_window::open(owner),
            Tool::Converter => converter_window::open(owner),
        }
    }

    fn paint(&mut self) {
        let target = self.target.clone();
        let hovered = self.hit_at(self.pointer);
        self.hits = draw(&target, self.graphics(), &self.settings, hovered);
    }
}

struct Brushes {
    text: ID2D1SolidColorBrush,
    dim: ID2D1SolidColorBrush,
    accent: ID2D1SolidColorBrush,
    accent_soft: ID2D1SolidColorBrush,
    hover: ID2D1SolidColorBrush,
}

fn create_brushes(target: &ID2D1RenderTarget) -> Option<Brushes> {
    let brush = |color: D2D1_COLOR_F| unsafe { target.CreateSolidColorBrush(&color, None) }.ok();
    Some(Brushes {
        text: brush(style::TEXT)?,
        dim: brush(style::TEXT_DIM)?,
        accent: brush(style::ACCENT)?,
        accent_soft: brush(style::ACCENT_SOFT)?,
        hover: brush(style::HOVER)?,
    })
}

/// Draws the whole window and returns where every choice button is, for hit testing.
fn draw(
    target: &ID2D1RenderTarget,
    graphics: &Graphics,
    settings: &Settings,
    hovered: Option<Hit>,
) -> Vec<(Hit, Rect)> {
    let Some(brushes) = create_brushes(target) else {
        return Vec::new();
    };
    unsafe {
        target.BeginDraw();
        target.Clear(Some(&style::INK));
    }
    let spark = Rect {
        left: layout::PADDING - 8.0,
        top: 18.0,
        right: layout::PADDING + 22.0,
        bottom: 54.0,
    };
    graphics.draw_spark(target, spark, &brushes.accent);
    graphics.draw_label(
        target,
        "bảng điều khiển",
        layout::PADDING + 28.0,
        27.0,
        400.0,
        &brushes.text,
    );

    let mut hits = Vec::new();
    for (column_index, sections) in settings_model::columns(settings).iter().enumerate() {
        draw_column(
            target,
            graphics,
            &brushes,
            sections,
            column_index,
            hovered,
            &mut hits,
        );
    }
    unsafe {
        let _ = target.EndDraw(None, None);
    }
    hits
}

fn row_buttons(graphics: &Graphics, row: &Row, column_index: usize, top: f32) -> Vec<(Hit, Rect)> {
    let right_edge = layout::column_left(column_index) + layout::column_width();
    let widths: Vec<f32> = row
        .options
        .iter()
        .map(|option| graphics.measure(option).0)
        .collect();
    layout::button_rects(top, right_edge, &widths)
        .into_iter()
        .enumerate()
        .map(|(option, rect)| {
            (
                Hit {
                    field: row.field,
                    option,
                },
                rect,
            )
        })
        .collect()
}

fn draw_column(
    target: &ID2D1RenderTarget,
    graphics: &Graphics,
    brushes: &Brushes,
    sections: &[Section],
    column_index: usize,
    hovered: Option<Hit>,
    hits: &mut Vec<(Hit, Rect)>,
) {
    let left = layout::column_left(column_index);
    let right = left + layout::column_width();
    let mut top = layout::TITLE_HEIGHT;
    for section in sections {
        let icon_area = Rect {
            left: left - 2.0,
            top: top + 10.0,
            right: left + 20.0,
            bottom: top + 34.0,
        };
        graphics.draw_icon(target, section.icon, icon_area, &brushes.accent);
        graphics.draw_label(
            target,
            section.title,
            left + layout::ICON_SLOT,
            top + 14.0,
            right,
            &brushes.accent,
        );
        top += layout::SECTION_HEIGHT;
        for row in &section.rows {
            draw_row(
                target,
                graphics,
                brushes,
                row,
                column_index,
                top,
                hovered,
                hits,
            );
            top += layout::ROW_HEIGHT;
        }
        top += layout::SECTION_GAP;
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_row(
    target: &ID2D1RenderTarget,
    graphics: &Graphics,
    brushes: &Brushes,
    row: &Row,
    column_index: usize,
    top: f32,
    hovered: Option<Hit>,
    hits: &mut Vec<(Hit, Rect)>,
) {
    let left = layout::column_left(column_index);
    let (_, text_height) = graphics.measure(row.label);
    let text_top = top + (layout::ROW_HEIGHT - text_height) / 2.0;
    let icon_area = Rect {
        left: left - 2.0,
        top,
        right: left + 20.0,
        bottom: top + layout::ROW_HEIGHT,
    };
    graphics.draw_icon(target, row.icon, icon_area, &brushes.dim);
    graphics.draw_label(
        target,
        row.label,
        left + layout::ICON_SLOT,
        text_top,
        left + layout::LABEL_WIDTH,
        &brushes.text,
    );

    let buttons = row_buttons(graphics, row, column_index, top);
    let text_right = buttons
        .first()
        .map_or(left + layout::column_width(), |(_, button)| button.left);
    for (button_index, (hit, button)) in buttons.iter().enumerate() {
        let selected = row.selected == Some(button_index);
        if selected || hovered == Some(*hit) {
            let fill = if selected {
                &brushes.accent_soft
            } else {
                &brushes.hover
            };
            let shape = D2D1_ROUNDED_RECT {
                rect: (*button).into(),
                radiusX: style::BUTTON_RADIUS,
                radiusY: style::BUTTON_RADIUS,
            };
            unsafe { target.FillRoundedRectangle(&shape, fill) };
        }
        let brush = if selected {
            &brushes.accent
        } else {
            &brushes.text
        };
        let text_left = button.left + layout::BUTTON_PADDING / 2.0;
        graphics.draw_label(
            target,
            &row.options[button_index],
            text_left,
            text_top,
            button.right,
            brush,
        );
    }
    hits.extend(buttons);

    if let Some(trailing) = &row.trailing {
        graphics.draw_label(
            target,
            trailing,
            left + layout::LABEL_WIDTH,
            text_top,
            text_right - 8.0,
            &brushes.dim,
        );
    }
}

/// `oh-key.exe --render-png settings out.png`: the window drawn into a file.
pub fn render_png(graphics: &Graphics, settings: &Settings, path: &Path) -> Result<()> {
    let width = layout::WIDTH as u32;
    let height = window_height(settings).round() as u32;
    let offscreen = Offscreen::new(graphics, width, height)?;
    draw(&offscreen.target, graphics, settings, None);
    offscreen.save_png(path)
}
