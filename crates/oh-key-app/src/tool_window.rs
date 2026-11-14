//! What the macro editor and the converter share: a dark framed window drawn with Direct2D in
//! the control panel's look, the quiet text buttons, the outcome mark, and the Win32 EDIT
//! controls that stand in for text inputs. The look itself comes from `style` and `palette`.

use std::ffi::c_void;
use std::sync::Once;
use std::sync::atomic::{AtomicIsize, Ordering};

use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, RECT, WPARAM};
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
    AddFontMemResourceEx, CLEARTYPE_QUALITY, CLIP_DEFAULT_PRECIS, CreateFontW, DEFAULT_CHARSET,
    DEFAULT_PITCH, FF_DONTCARE, FW_NORMAL, GetMonitorInfoW, HFONT, InvalidateRect,
    MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromPoint, OUT_DEFAULT_PRECIS, UpdateWindow,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::HiDpi::{AdjustWindowRectExForDpi, GetDpiForWindow};
use windows::Win32::UI::Input::KeyboardAndMouse::{SetFocus, VK_ESCAPE, VK_RETURN, VK_TAB};
use windows::Win32::UI::Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, ES_AUTOHSCROLL, GWLP_USERDATA, GetClientRect, GetCursorPos,
    GetWindowLongPtrW, GetWindowTextLengthW, GetWindowTextW, HMENU, IsWindow, PostMessageW,
    RegisterClassExW, SW_SHOWNORMAL, SWP_NOZORDER, SendMessageW, SetForegroundWindow, SetWindowPos,
    SetWindowTextW, ShowWindow, WINDOW_EX_STYLE, WM_APP, WM_CHAR, WM_CLOSE, WM_KEYDOWN,
    WM_NCDESTROY, WM_SETFONT, WNDCLASSEXW, WNDPROC, WS_CAPTION, WS_CHILD, WS_CLIPCHILDREN,
    WS_EX_LEFT, WS_MINIMIZEBOX, WS_OVERLAPPED, WS_SYSMENU, WS_TABSTOP, WS_VISIBLE,
};
use windows::core::{BOOL, HSTRING, Interface, PCWSTR, Result, w};

use crate::graphics::Graphics;
use crate::layout::{self, Rect};
use crate::logo;
use crate::palette::{self, Rgba};
use crate::{mode_icon, style};

/// Posted by an edit control to its window when Enter is pressed in it.
pub const MESSAGE_EDIT_ENTER: u32 = WM_APP + 10;
/// Posted by an edit control when Tab is pressed in it; `wparam` is the edit's handle.
pub const MESSAGE_EDIT_TAB: u32 = WM_APP + 11;

const FONT_BYTES: &[u8] = include_bytes!("../assets/fonts/JetBrainsMono-Regular.ttf");
const EDIT_SUBCLASS_ID: usize = 1;
/// EM_SETLIMITTEXT, which lives in a feature this crate does not otherwise need.
const EDIT_SET_LIMIT: u32 = 0x00C5;

pub const ICON_CHECK: char = '\u{E73E}';
pub const ICON_CANCEL: char = '\u{E711}';

/// The ink with the hover wash laid over it: the fill of a text input.
pub const INK_PANEL: Rgba = blend(palette::INK, palette::TEXT, palette::HOVER.alpha);
/// The one colour the control panel does not have: a finished action needs a mark that is not
/// the accent, so a success and a failure differ in colour as well as in shape.
pub const SUCCESS: Rgba = palette::rgb(0x8F, 0xB5, 0x7A, 1.0);

const fn blend(base: Rgba, over: Rgba, amount: f32) -> Rgba {
    Rgba {
        red: base.red + (over.red - base.red) * amount,
        green: base.green + (over.green - base.green) * amount,
        blue: base.blue + (over.blue - base.blue) * amount,
        alpha: 1.0,
    }
}

fn colorref(color: Rgba) -> COLORREF {
    let channel = |value: f32| (value * 255.0).round() as u32;
    COLORREF(channel(color.red) | channel(color.green) << 8 | channel(color.blue) << 16)
}

pub fn panel_colorref() -> COLORREF {
    colorref(INK_PANEL)
}

pub fn text_colorref() -> COLORREF {
    colorref(palette::TEXT)
}

/// What a finished action reports.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    Success(String),
    Failure(String),
}

/// Set while a window is open, so a second request brings it forward.
pub struct OpenWindow(AtomicIsize);

impl OpenWindow {
    pub const fn new() -> Self {
        Self(AtomicIsize::new(0))
    }

    pub fn set(&self, hwnd: HWND) {
        self.0.store(hwnd.0 as isize, Ordering::Relaxed);
    }

    pub fn clear(&self) {
        self.0.store(0, Ordering::Relaxed);
    }

    /// Brings the open window forward; false when there is none.
    pub fn bring_forward(&self) -> bool {
        let raw = self.0.load(Ordering::Relaxed);
        if raw == 0 {
            return false;
        }
        let hwnd = HWND(raw as *mut _);
        if !unsafe { IsWindow(Some(hwnd)) }.as_bool() {
            return false;
        }
        unsafe {
            let _ = SetForegroundWindow(hwnd);
        }
        true
    }
}

pub struct Frame {
    pub hwnd: HWND,
    /// Pixels per device-independent pixel.
    pub scale: f32,
}

pub fn register_class(
    graphics: &Graphics,
    class_name: PCWSTR,
    window_proc: WNDPROC,
    registered: &Once,
) {
    registered.call_once(|| {
        let icon = mode_icon::icon_with(graphics, true);
        let class = WNDCLASSEXW {
            cbSize: size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: window_proc,
            hInstance: unsafe { GetModuleHandleW(None) }.unwrap_or_default().into(),
            hIcon: icon,
            lpszClassName: class_name,
            ..Default::default()
        };
        unsafe { RegisterClassExW(&class) };
    });
}

/// Creates the window with a client area of `size` device-independent pixels, centred on the
/// monitor under the cursor, scaled to that monitor's DPI or less when it would not fit.
pub fn create_frame(
    class_name: PCWSTR,
    title: PCWSTR,
    size: (f32, f32),
    owner: Option<HWND>,
) -> Result<Frame> {
    let instance = unsafe { GetModuleHandleW(None)? };
    let style = WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_MINIMIZEBOX | WS_CLIPCHILDREN;
    let hwnd = unsafe {
        CreateWindowExW(
            WS_EX_LEFT,
            class_name,
            title,
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
    let scale = layout::fitted_scale(dpi as f32 / 96.0, size, room);
    let mut frame = RECT {
        left: 0,
        top: 0,
        right: (size.0 * scale).round() as i32,
        bottom: (size.1 * scale).round() as i32,
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
    Ok(Frame { hwnd, scale })
}

pub fn show(hwnd: HWND) {
    unsafe {
        let _ = ShowWindow(hwnd, SW_SHOWNORMAL);
        let _ = SetForegroundWindow(hwnd);
    }
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

pub fn create_target(graphics: &Graphics, hwnd: HWND, scale: f32) -> Result<ID2D1RenderTarget> {
    let mut client = RECT::default();
    unsafe { GetClientRect(hwnd, &mut client)? };
    let dpi = scale * 96.0;
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

/// The window's state behind its user data, once the window has set it.
pub fn state_of<State>(hwnd: HWND) -> Option<&'static mut State> {
    let pointer = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } as *mut State;
    unsafe { pointer.as_mut() }
}

/// Handles a message for a window whose state is not set yet, or is being torn down.
/// Returns the result when the message is settled here.
pub fn default_while_stateless(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    has_state: bool,
) -> Option<LRESULT> {
    if !has_state || message == WM_NCDESTROY {
        return Some(unsafe { DefWindowProcW(hwnd, message, wparam, lparam) });
    }
    None
}

pub fn pointer_in_dips(target: &ID2D1RenderTarget, lparam: LPARAM) -> (f32, f32) {
    let pixel_x = (lparam.0 & 0xFFFF) as u16 as i16 as i32;
    let pixel_y = ((lparam.0 >> 16) & 0xFFFF) as u16 as i16 as i32;
    let mut dpi_x = 96.0;
    let mut dpi_y = 96.0;
    unsafe { target.GetDpi(&mut dpi_x, &mut dpi_y) };
    layout::pixels_to_dips((pixel_x, pixel_y), (dpi_x, dpi_y))
}

pub fn to_pixels(rect: Rect, scale: f32) -> RECT {
    RECT {
        left: (rect.left * scale).round() as i32,
        top: (rect.top * scale).round() as i32,
        right: (rect.right * scale).round() as i32,
        bottom: (rect.bottom * scale).round() as i32,
    }
}

pub struct Brushes {
    pub text: ID2D1SolidColorBrush,
    pub dim: ID2D1SolidColorBrush,
    pub accent: ID2D1SolidColorBrush,
    pub accent_soft: ID2D1SolidColorBrush,
    pub hover: ID2D1SolidColorBrush,
    pub panel: ID2D1SolidColorBrush,
    pub success: ID2D1SolidColorBrush,
}

pub fn create_brushes(target: &ID2D1RenderTarget) -> Option<Brushes> {
    let brush = |color: D2D1_COLOR_F| unsafe { target.CreateSolidColorBrush(&color, None) }.ok();
    Some(Brushes {
        text: brush(style::TEXT)?,
        dim: brush(style::TEXT_DIM)?,
        accent: brush(style::ACCENT)?,
        accent_soft: brush(style::ACCENT_SOFT)?,
        hover: brush(style::HOVER)?,
        panel: brush(style::direct2d(INK_PANEL))?,
        success: brush(style::direct2d(SUCCESS))?,
    })
}

/// Clears the window to the ink and draws the logo and the title, as the control panel does.
pub fn begin_window(
    target: &ID2D1RenderTarget,
    graphics: &Graphics,
    brushes: &Brushes,
    title: &str,
) {
    unsafe {
        target.BeginDraw();
        target.Clear(Some(&style::INK));
    }
    let logo_area = logo::square(layout::PADDING, 36.0, style::LOGO_SIZE);
    logo::draw(target, &graphics.d2d, logo_area);
    graphics.draw_label(
        target,
        title,
        layout::PADDING + 28.0,
        27.0,
        400.0,
        &brushes.text,
    );
}

pub fn end_window(target: &ID2D1RenderTarget) {
    unsafe {
        let _ = target.EndDraw(None, None);
    }
}

/// A group title in the accent with its icon, as in the control panel.
pub fn draw_section_header(
    target: &ID2D1RenderTarget,
    graphics: &Graphics,
    brushes: &Brushes,
    icon: char,
    title: &str,
    top: f32,
    right: f32,
) {
    let left = layout::PADDING;
    let icon_area = Rect {
        left: left - 2.0,
        top: top + 10.0,
        right: left + 20.0,
        bottom: top + 34.0,
    };
    graphics.draw_icon(target, icon, icon_area, &brushes.accent);
    graphics.draw_label(
        target,
        title,
        left + layout::ICON_SLOT,
        top + 14.0,
        right,
        &brushes.accent,
    );
}

/// A row label with its dim icon, vertically centred in a row that starts at `top`.
pub fn draw_row_label(
    target: &ID2D1RenderTarget,
    graphics: &Graphics,
    brushes: &Brushes,
    icon: char,
    label: &str,
    top: f32,
    right: f32,
) {
    let left = layout::PADDING;
    let (_, text_height) = graphics.measure(label);
    let icon_area = Rect {
        left: left - 2.0,
        top,
        right: left + 20.0,
        bottom: top + layout::ROW_HEIGHT,
    };
    graphics.draw_icon(target, icon, icon_area, &brushes.dim);
    graphics.draw_label(
        target,
        label,
        left + layout::ICON_SLOT,
        top + (layout::ROW_HEIGHT - text_height) / 2.0,
        right,
        &brushes.text,
    );
}

pub fn draw_label_centred(
    target: &ID2D1RenderTarget,
    graphics: &Graphics,
    text: &str,
    rect: Rect,
    left_inset: f32,
    brush: &ID2D1SolidColorBrush,
) {
    let (_, text_height) = graphics.measure(text);
    graphics.draw_label(
        target,
        text,
        rect.left + left_inset,
        rect.top + (rect.bottom - rect.top - text_height) / 2.0,
        rect.right,
        brush,
    );
}

fn fill_rounded(target: &ID2D1RenderTarget, rect: Rect, brush: &ID2D1SolidColorBrush) {
    let shape = D2D1_ROUNDED_RECT {
        rect: rect.into(),
        radiusX: style::BUTTON_RADIUS,
        radiusY: style::BUTTON_RADIUS,
    };
    unsafe { target.FillRoundedRectangle(&shape, brush) };
}

/// A quiet text button: no fill until hovered or selected.
pub fn draw_text_button(
    target: &ID2D1RenderTarget,
    graphics: &Graphics,
    brushes: &Brushes,
    label: &str,
    rect: Rect,
    hovered: bool,
    selected: bool,
) {
    if selected {
        fill_rounded(target, rect, &brushes.accent_soft);
    } else if hovered {
        fill_rounded(target, rect, &brushes.hover);
    }
    let brush = if selected {
        &brushes.accent
    } else {
        &brushes.text
    };
    draw_label_centred(
        target,
        graphics,
        label,
        rect,
        layout::BUTTON_PADDING / 2.0,
        brush,
    );
}

/// The fill behind a text input; the EDIT control sits on top of it.
pub fn draw_input_panel(target: &ID2D1RenderTarget, brushes: &Brushes, rect: Rect) {
    fill_rounded(target, rect, &brushes.panel);
}

/// A mark in the status colour (check for success, cross for failure), then the message.
pub fn draw_outcome(
    target: &ID2D1RenderTarget,
    graphics: &Graphics,
    brushes: &Brushes,
    outcome: &Outcome,
    top: f32,
    right: f32,
) {
    let (icon, mark_brush, message) = match outcome {
        Outcome::Success(message) => (ICON_CHECK, &brushes.success, message),
        Outcome::Failure(message) => (ICON_CANCEL, &brushes.accent, message),
    };
    let left = layout::PADDING;
    let icon_area = Rect {
        left: left - 2.0,
        top,
        right: left + 20.0,
        bottom: top + layout::ROW_HEIGHT,
    };
    graphics.draw_icon(target, icon, icon_area, mark_brush);
    let (_, text_height) = graphics.measure(message);
    graphics.draw_label(
        target,
        message,
        left + layout::ICON_SLOT,
        top + (layout::ROW_HEIGHT - text_height) / 2.0,
        right,
        &brushes.text,
    );
}

/// Right-aligned text buttons on one row; the rectangles come back in label order.
pub fn button_row(graphics: &Graphics, labels: &[&str], top: f32, right_edge: f32) -> Vec<Rect> {
    let widths: Vec<f32> = labels
        .iter()
        .map(|label| graphics.measure(label).0)
        .collect();
    layout::button_rects(top, right_edge, &widths)
}

/// The bundled mono font as a GDI font for the EDIT controls, which cannot use DirectWrite's
/// private collection. The font is added to this process only.
pub fn create_input_font(scale: f32) -> HFONT {
    static FONT_ADDED: Once = Once::new();
    FONT_ADDED.call_once(|| {
        let added = 0u32;
        unsafe {
            AddFontMemResourceEx(
                FONT_BYTES.as_ptr() as *const c_void,
                FONT_BYTES.len() as u32,
                None,
                &added,
            );
        }
    });
    let pixel_height = (style::LABEL_SIZE * scale).round() as i32;
    unsafe {
        CreateFontW(
            -pixel_height,
            0,
            0,
            0,
            FW_NORMAL.0 as i32,
            0,
            0,
            0,
            DEFAULT_CHARSET,
            OUT_DEFAULT_PRECIS,
            CLIP_DEFAULT_PRECIS,
            CLEARTYPE_QUALITY,
            (DEFAULT_PITCH.0 | FF_DONTCARE.0) as u32,
            &HSTRING::from(style::MONO_FAMILY),
        )
    }
}

/// A borderless single-line EDIT control over `area` (device-independent pixels). Enter, Tab
/// and Escape are forwarded to `parent` instead of beeping.
pub fn create_input(
    parent: HWND,
    id: isize,
    area: Rect,
    scale: f32,
    font: HFONT,
    character_limit: usize,
) -> Result<HWND> {
    let instance = unsafe { GetModuleHandleW(None)? };
    let pixels = to_pixels(area, scale);
    let edit = unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE(0),
            w!("EDIT"),
            PCWSTR::null(),
            WS_CHILD
                | WS_VISIBLE
                | WS_TABSTOP
                | windows::Win32::UI::WindowsAndMessaging::WINDOW_STYLE(ES_AUTOHSCROLL as u32),
            pixels.left,
            pixels.top,
            pixels.right - pixels.left,
            pixels.bottom - pixels.top,
            Some(parent),
            Some(HMENU(id as *mut _)),
            Some(instance.into()),
            None,
        )?
    };
    unsafe {
        SendMessageW(
            edit,
            WM_SETFONT,
            Some(WPARAM(font.0 as usize)),
            Some(LPARAM(1)),
        );
        SendMessageW(edit, EDIT_SET_LIMIT, Some(WPARAM(character_limit)), None);
        let _ = SetWindowSubclass(
            edit,
            Some(input_subclass),
            EDIT_SUBCLASS_ID,
            parent.0 as usize,
        );
    }
    Ok(edit)
}

unsafe extern "system" fn input_subclass(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _subclass_id: usize,
    parent_handle: usize,
) -> LRESULT {
    let parent = HWND(parent_handle as *mut _);
    let key = wparam.0 as u32;
    match message {
        WM_KEYDOWN if key == VK_RETURN.0 as u32 => {
            let _ = unsafe { PostMessageW(Some(parent), MESSAGE_EDIT_ENTER, WPARAM(0), LPARAM(0)) };
            LRESULT(0)
        }
        WM_KEYDOWN if key == VK_TAB.0 as u32 => {
            let _ = unsafe {
                PostMessageW(
                    Some(parent),
                    MESSAGE_EDIT_TAB,
                    WPARAM(hwnd.0 as usize),
                    LPARAM(0),
                )
            };
            LRESULT(0)
        }
        WM_KEYDOWN if key == VK_ESCAPE.0 as u32 => {
            let _ = unsafe { PostMessageW(Some(parent), WM_CLOSE, WPARAM(0), LPARAM(0)) };
            LRESULT(0)
        }
        WM_CHAR if matches!(key, 0x0D | 0x09 | 0x1B) => LRESULT(0),
        WM_NCDESTROY => {
            unsafe {
                let _ = RemoveWindowSubclass(hwnd, Some(input_subclass), EDIT_SUBCLASS_ID);
            }
            unsafe { DefSubclassProc(hwnd, message, wparam, lparam) }
        }
        _ => unsafe { DefSubclassProc(hwnd, message, wparam, lparam) },
    }
}

pub fn redraw_now(hwnd: HWND) {
    unsafe {
        let _ = InvalidateRect(Some(hwnd), None, false);
        let _ = UpdateWindow(hwnd);
    }
}

pub fn focus(hwnd: HWND) {
    unsafe {
        let _ = SetFocus(Some(hwnd));
    }
}

pub fn input_text(edit: HWND) -> String {
    let length = unsafe { GetWindowTextLengthW(edit) }.max(0) as usize;
    let mut buffer = vec![0u16; length + 1];
    let copied = unsafe { GetWindowTextW(edit, &mut buffer) }.max(0) as usize;
    String::from_utf16_lossy(&buffer[..copied])
}

pub fn set_input_text(edit: HWND, text: &str) {
    unsafe {
        let _ = SetWindowTextW(edit, &HSTRING::from(text));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_input_panel_is_the_ink_lifted_by_the_hover_wash() {
        let expected = 0x1F as f32 / 255.0 + (0xF0 - 0x1F) as f32 / 255.0 * 0.07;
        assert!((INK_PANEL.red - expected).abs() < 1e-6);
        assert_eq!(INK_PANEL.alpha, 1.0);
    }

    #[test]
    fn a_colour_packs_with_red_in_the_low_byte() {
        let packed = colorref(palette::rgb(0x11, 0x22, 0x33, 1.0));
        assert_eq!(packed.0, 0x0033_2211);
    }

    #[test]
    fn a_rectangle_scales_to_pixels() {
        let rect = Rect {
            left: 10.0,
            top: 20.0,
            right: 30.0,
            bottom: 41.0,
        };
        let pixels = to_pixels(rect, 1.5);
        assert_eq!(
            (pixels.left, pixels.top, pixels.right, pixels.bottom),
            (15, 30, 45, 62)
        );
    }
}
