//! The control panel: one small window with two mode buttons, two cards of dropdowns, chips and
//! switches, and three buttons that open the tool windows. A click saves at once and publishes
//! the change to the running hook; the mode buttons set the global mode like the tray does.

use std::path::Path;
use std::sync::atomic::{AtomicIsize, Ordering};

use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Direct2D::Common::{
    D2D_SIZE_U, D2D1_ALPHA_MODE_PREMULTIPLIED, D2D1_COLOR_F, D2D1_PIXEL_FORMAT,
};
use windows::Win32::Graphics::Direct2D::{
    D2D1_FEATURE_LEVEL_DEFAULT, D2D1_HWND_RENDER_TARGET_PROPERTIES, D2D1_PRESENT_OPTIONS_NONE,
    D2D1_RENDER_TARGET_PROPERTIES, D2D1_RENDER_TARGET_TYPE_DEFAULT, D2D1_RENDER_TARGET_USAGE_NONE,
    ID2D1HwndRenderTarget, ID2D1RenderTarget, ID2D1SolidColorBrush,
};
use windows::Win32::Graphics::Dwm::{DWMWINDOWATTRIBUTE, DwmSetWindowAttribute};
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;
use windows::Win32::Graphics::Gdi::{
    GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromPoint, ValidateRect,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::HiDpi::{AdjustWindowRectExForDpi, GetDpiForWindow};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetKeyState, TME_LEAVE, TRACKMOUSEEVENT, TrackMouseEvent, VIRTUAL_KEY, VK_DOWN, VK_ESCAPE,
    VK_LEFT, VK_RETURN, VK_RIGHT, VK_SHIFT, VK_SPACE, VK_TAB, VK_UP,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, GWLP_USERDATA, GetClientRect, GetCursorPos,
    GetWindowLongPtrW, IDC_ARROW, IDC_HAND, IsWindow, LoadCursorW, RegisterClassExW, SW_SHOWNORMAL,
    SWP_NOZORDER, SetCursor, SetForegroundWindow, SetWindowLongPtrW, SetWindowPos, ShowWindow,
    WM_KEYDOWN, WM_KILLFOCUS, WM_LBUTTONUP, WM_MOUSEMOVE, WM_NCDESTROY, WM_PAINT, WM_SETCURSOR,
    WNDCLASSEXW, WS_CAPTION, WS_EX_LEFT, WS_MINIMIZEBOX, WS_OVERLAPPED, WS_SYSMENU,
};
use windows::core::{BOOL, Interface, Result, w};

use crate::graphics::{self, Graphics};
use crate::layout::{self, Rect, settings as geometry};
use crate::logo;
use crate::offscreen::Offscreen;
use crate::settings_model::{self, Control, Effect, Field, Row, Tool};
use crate::{
    converter_window, excluded_apps_window, icons, macros_window, mode_icon, persistence, startup,
    style, update_service,
};
use oh_key_ipc::{Settings, SharedState};

/// Something the pointer can land on.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Target {
    Mode(bool),
    Dropdown(Field),
    Chip(Field, usize),
    Switch(Field),
    Action(Tool),
    /// The "Kiểm tra cập nhật" button in the title bar.
    CheckUpdates,
    PopupItem(Field, usize),
}

/// A stop of the Tab order.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Focus {
    /// The pair of mode buttons, ringed on the active one.
    Mode,
    Row(Field),
    Action(Tool),
    CheckUpdates,
}

/// The list opened from a dropdown, and which of its items is lit.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Popup {
    field: Field,
    highlighted: usize,
}

/// Everything one paint depends on.
struct View<'a> {
    settings: &'a Settings,
    vietnamese: bool,
    hovered: Option<Target>,
    /// Only set while the keyboard is driving, so a mouse click leaves no ring.
    focus_ring: Option<Focus>,
    popup: Option<Popup>,
    /// A manual update check is running, so the button says so.
    checking_for_updates: bool,
}

/// Set while the window is open, so a second request brings it forward.
static OPEN_WINDOW: AtomicIsize = AtomicIsize::new(0);

struct SettingsWindow {
    graphics: Graphics,
    hwnd: HWND,
    target: ID2D1RenderTarget,
    settings: Settings,
    /// Dropped with the window; None when the shared block could not be opened.
    shared: Option<SharedState>,
    pointer: (f32, f32),
    hits: Vec<(Target, Rect)>,
    focus: Option<Focus>,
    focus_visible: bool,
    popup: Option<Popup>,
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
    let cards = settings_model::cards(settings);
    geometry::frame(&settings_model::card_row_counts(&cards)).height
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
    let scale = layout::fitted_scale(dpi as f32 / 96.0, (geometry::WIDTH, height_dips), room);
    let mut frame = RECT {
        left: 0,
        top: 0,
        right: (geometry::WIDTH * scale).round() as i32,
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
        shared: SharedState::open().ok(),
        pointer: (-1.0, -1.0),
        hits: Vec::new(),
        focus: None,
        focus_visible: false,
        popup: None,
    });
    unsafe {
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, Box::into_raw(window) as _);
        OPEN_WINDOW.store(hwnd.0 as isize, Ordering::Relaxed);
        let _ = ShowWindow(hwnd, SW_SHOWNORMAL);
        let _ = SetForegroundWindow(hwnd);
    }
    Ok(())
}

/// Re-reads the settings into the open window, after something else changed them or the mode
/// changed.
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

/// Asks for a WM_MOUSELEAVE, so the hover state clears when the pointer leaves the window.
fn track_mouse_leave(hwnd: HWND) {
    let mut request = TRACKMOUSEEVENT {
        cbSize: size_of::<TRACKMOUSEEVENT>() as u32,
        dwFlags: TME_LEAVE,
        hwndTrack: hwnd,
        dwHoverTime: 0,
    };
    unsafe {
        let _ = TrackMouseEvent(&mut request);
    }
}

fn shift_is_down() -> bool {
    let state = unsafe { GetKeyState(VK_SHIFT.0 as i32) };
    state < 0
}

const HIT_TEST_CLIENT: isize = 1;
/// Sent after `TrackMouseEvent(TME_LEAVE)`, when the pointer leaves the client area.
const WM_MOUSELEAVE: u32 = 0x02A3;

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
        WM_SETCURSOR if (lparam.0 & 0xFFFF) == HIT_TEST_CLIENT => {
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
            track_mouse_leave(hwnd);
            let point = window.to_dips(lparam);
            window.move_pointer(point);
            LRESULT(0)
        }
        WM_MOUSELEAVE => {
            window.move_pointer((-1.0, -1.0));
            LRESULT(0)
        }
        WM_KILLFOCUS => {
            window.popup = None;
            window.paint();
            LRESULT(0)
        }
        WM_LBUTTONUP => {
            window.pointer = window.to_dips(lparam);
            window.click();
            window.paint();
            LRESULT(0)
        }
        WM_KEYDOWN => {
            let key = VIRTUAL_KEY(wparam.0 as u16);
            if key == VK_ESCAPE && window.popup.is_none() {
                unsafe {
                    let _ = DestroyWindow(hwnd);
                }
                return LRESULT(0);
            }
            window.key_down(key, shift_is_down());
            window.paint();
            LRESULT(0)
        }
        _ => unsafe { DefWindowProcW(hwnd, message, wparam, lparam) },
    }
}

/// The Tab order: the mode buttons, every row top to bottom, then the tool buttons.
fn focus_order(settings: &Settings) -> Vec<Focus> {
    let mut order = vec![Focus::Mode];
    for row in settings_model::cards(settings).iter().flatten() {
        order.push(Focus::Row(row.field));
    }
    for action in settings_model::actions() {
        order.push(Focus::Action(action.tool));
    }
    order.push(Focus::CheckUpdates);
    order
}

/// The stop after (or before) `current`, wrapping round; from nowhere, the first (or last).
fn neighbour_focus(order: &[Focus], current: Option<Focus>, backward: bool) -> Focus {
    let position = current.and_then(|focus| order.iter().position(|stop| *stop == focus));
    let last = order.len() - 1;
    let index = match (position, backward) {
        (None, false) => 0,
        (None, true) => last,
        (Some(0), true) => last,
        (Some(position), true) => position - 1,
        (Some(position), false) if position == last => 0,
        (Some(position), false) => position + 1,
    };
    order[index]
}

fn focus_of(target: Target) -> Option<Focus> {
    match target {
        Target::Mode(_) => Some(Focus::Mode),
        Target::Dropdown(field) | Target::Chip(field, _) | Target::Switch(field) => {
            Some(Focus::Row(field))
        }
        Target::Action(tool) => Some(Focus::Action(tool)),
        Target::CheckUpdates => Some(Focus::CheckUpdates),
        Target::PopupItem(..) => None,
    }
}

impl SettingsWindow {
    fn to_dips(&self, lparam: LPARAM) -> (f32, f32) {
        let pixel_x = (lparam.0 & 0xFFFF) as u16 as i16 as i32;
        let pixel_y = ((lparam.0 >> 16) & 0xFFFF) as u16 as i16 as i32;
        let mut dpi_x = 96.0;
        let mut dpi_y = 96.0;
        unsafe { self.target.GetDpi(&mut dpi_x, &mut dpi_y) };
        layout::pixels_to_dips((pixel_x, pixel_y), (dpi_x, dpi_y))
    }

    /// While a popup is open only its items can be hit; the popup's rectangles come first.
    fn hit_at(&self, point: (f32, f32)) -> Option<Target> {
        let hit = layout::hit_test(&self.hits, point)?;
        let blocked = self.popup.is_some() && !matches!(hit, Target::PopupItem(..));
        (!blocked).then_some(hit)
    }

    fn vietnamese(&self) -> bool {
        self.shared
            .as_ref()
            .map_or(self.settings.start_vietnamese, SharedState::vietnamese)
    }

    fn row_control(&self, field: Field) -> Option<Control> {
        settings_model::cards(&self.settings)
            .into_iter()
            .flatten()
            .find(|row| row.field == field)
            .map(|row| row.control)
    }

    /// Repaints only when the pointer changed what is lit.
    fn move_pointer(&mut self, point: (f32, f32)) {
        let before = self.hit_at(self.pointer);
        self.pointer = point;
        let after = self.hit_at(point);
        let mut highlight_changed = false;
        if let (Some(popup), Some(Target::PopupItem(_, item))) = (self.popup.as_mut(), after) {
            highlight_changed = popup.highlighted != item;
            popup.highlighted = item;
        }
        if before != after || highlight_changed {
            self.paint();
        }
    }

    fn click(&mut self) {
        let hit = self.hit_at(self.pointer);
        if let Some(popup) = self.popup.take() {
            if let Some(Target::PopupItem(field, option)) = hit {
                self.choose(field, option);
                self.focus = Some(Focus::Row(popup.field));
            }
            return;
        }
        let Some(hit) = hit else {
            return;
        };
        self.focus = focus_of(hit);
        self.focus_visible = false;
        match hit {
            Target::Mode(vietnamese) => self.set_mode(vietnamese),
            Target::Dropdown(field) => self.open_popup(field),
            Target::Chip(field, option) => self.choose(field, option),
            Target::Switch(field) => self.choose_next(field),
            Target::Action(tool) => self.open_tool(tool),
            Target::CheckUpdates => self.check_for_updates(),
            Target::PopupItem(..) => {}
        }
    }

    fn key_down(&mut self, key: VIRTUAL_KEY, shift: bool) {
        if self.popup.is_some() {
            self.popup_key(key);
            return;
        }
        match key {
            VK_TAB => self.move_focus(shift),
            VK_SPACE | VK_RETURN => self.activate_focus(),
            VK_LEFT | VK_UP => self.step_focus(false),
            VK_RIGHT | VK_DOWN => self.step_focus(true),
            _ => {}
        }
    }

    fn popup_key(&mut self, key: VIRTUAL_KEY) {
        let Some(popup) = self.popup else {
            return;
        };
        let item_count = self
            .row_control(popup.field)
            .map_or(0, |control| control.option_count());
        match key {
            VK_UP => self.set_highlight(popup.highlighted.saturating_sub(1)),
            VK_DOWN => {
                self.set_highlight((popup.highlighted + 1).min(item_count.saturating_sub(1)))
            }
            VK_RETURN | VK_SPACE => {
                self.popup = None;
                self.choose(popup.field, popup.highlighted);
            }
            VK_ESCAPE | VK_TAB => self.popup = None,
            _ => {}
        }
    }

    fn set_highlight(&mut self, highlighted: usize) {
        if let Some(popup) = self.popup.as_mut() {
            popup.highlighted = highlighted;
        }
    }

    fn move_focus(&mut self, backward: bool) {
        let order = focus_order(&self.settings);
        self.focus = Some(neighbour_focus(&order, self.focus, backward));
        self.focus_visible = true;
    }

    fn activate_focus(&mut self) {
        self.focus_visible = true;
        match self.focus {
            Some(Focus::Mode) => self.set_mode(!self.vietnamese()),
            Some(Focus::Row(field)) => self.activate_row(field),
            Some(Focus::Action(tool)) => self.open_tool(tool),
            Some(Focus::CheckUpdates) => self.check_for_updates(),
            None => {}
        }
    }

    fn activate_row(&mut self, field: Field) {
        if let Some(Control::Dropdown { .. }) = self.row_control(field) {
            self.open_popup(field);
        } else {
            self.choose_next(field);
        }
    }

    /// An arrow key on the focused control: the other mode, or the neighbouring option.
    fn step_focus(&mut self, forward: bool) {
        self.focus_visible = true;
        match self.focus {
            Some(Focus::Mode) => self.set_mode(!forward),
            Some(Focus::Row(field)) => {
                let stepped = self
                    .row_control(field)
                    .and_then(|control| control.stepped_option(forward));
                if let Some(option) = stepped {
                    self.choose(field, option);
                }
            }
            Some(Focus::Action(_) | Focus::CheckUpdates) | None => {}
        }
    }

    fn open_popup(&mut self, field: Field) {
        let Some(control) = self.row_control(field) else {
            return;
        };
        self.popup = Some(Popup {
            field,
            highlighted: control.selected(),
        });
    }

    fn set_mode(&mut self, vietnamese: bool) {
        if let Some(shared) = &self.shared {
            shared.set_vietnamese(vietnamese);
        }
    }

    fn choose_next(&mut self, field: Field) {
        if let Some(control) = self.row_control(field) {
            self.choose(field, control.next_option());
        }
    }

    /// Applies one choice, saves, and publishes it to the hook.
    fn choose(&mut self, field: Field, option: usize) {
        match settings_model::apply_choice(&mut self.settings, field, option) {
            Effect::Nothing => {}
            Effect::Save => persistence::save_and_publish(&self.settings),
            Effect::SaveAndApplyStartup => {
                persistence::save_and_publish(&self.settings);
                startup::set_start_at_sign_in(self.settings.start_at_sign_in);
            }
        }
    }

    /// The result comes back as a balloon from the tray, and the button repaints then.
    fn check_for_updates(&mut self) {
        update_service::check_now();
        self.paint();
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
        let view = View {
            settings: &self.settings,
            vietnamese: self.vietnamese(),
            hovered: self.hit_at(self.pointer),
            focus_ring: self.focus.filter(|_| self.focus_visible),
            popup: self.popup,
            checking_for_updates: update_service::checking(),
        };
        let hits = draw(&self.target, &self.graphics, &view);
        self.hits = hits;
    }
}

struct Brushes {
    ink: ID2D1SolidColorBrush,
    panel: ID2D1SolidColorBrush,
    raised: ID2D1SolidColorBrush,
    rule: ID2D1SolidColorBrush,
    text: ID2D1SolidColorBrush,
    dim: ID2D1SolidColorBrush,
    accent: ID2D1SolidColorBrush,
    accent_soft: ID2D1SolidColorBrush,
    hover: ID2D1SolidColorBrush,
}

fn create_brushes(target: &ID2D1RenderTarget) -> Option<Brushes> {
    let brush = |color: D2D1_COLOR_F| unsafe { target.CreateSolidColorBrush(&color, None) }.ok();
    Some(Brushes {
        ink: brush(style::INK)?,
        panel: brush(style::INK_PANEL)?,
        raised: brush(style::RAISED)?,
        rule: brush(style::RULE)?,
        text: brush(style::TEXT)?,
        dim: brush(style::TEXT_DIM)?,
        accent: brush(style::ACCENT)?,
        accent_soft: brush(style::ACCENT_SOFT)?,
        hover: brush(style::HOVER)?,
    })
}

/// Draws the whole window and returns where everything clickable is, popup first.
fn draw(target: &ID2D1RenderTarget, graphics: &Graphics, view: &View) -> Vec<(Target, Rect)> {
    let Some(brushes) = create_brushes(target) else {
        return Vec::new();
    };
    unsafe {
        target.BeginDraw();
        target.Clear(Some(&style::INK));
    }
    let cards = settings_model::cards(view.settings);
    let frame = geometry::frame(&settings_model::card_row_counts(&cards));
    let mut scene = Scene {
        target,
        graphics,
        brushes: &brushes,
        view,
        hits: Vec::new(),
        focus_shapes: Vec::new(),
    };
    scene.draw_title_bar();
    scene.draw_mode_buttons(&frame);
    for (rows, card) in cards.iter().zip(&frame.cards) {
        scene.draw_card(rows, *card);
    }
    scene.draw_actions(&frame);
    scene.draw_focus_ring();
    let mut hits = scene.draw_popup(&cards, frame.height);
    hits.append(&mut scene.hits);
    unsafe {
        let _ = target.EndDraw(None, None);
    }
    hits
}

/// The drawing state of one paint: where to draw, and what has been placed so far.
struct Scene<'a> {
    target: &'a ID2D1RenderTarget,
    graphics: &'a Graphics,
    brushes: &'a Brushes,
    view: &'a View<'a>,
    hits: Vec<(Target, Rect)>,
    /// Each focus stop's outline and corner radius, for the ring and the popup's anchor.
    focus_shapes: Vec<(Focus, Rect, f32)>,
}

impl Scene<'_> {
    fn fill(&self, rect: Rect, radius: f32, brush: &ID2D1SolidColorBrush) {
        graphics::fill_rounded(self.target, rect, radius, brush);
    }

    fn outline(&self, rect: Rect, radius: f32, brush: &ID2D1SolidColorBrush) {
        graphics::outline_rounded(self.target, rect, radius, style::OUTLINE_WIDTH, brush);
    }

    /// `text` starting at `left`, vertically centred in `top..bottom`.
    fn label_centred(
        &self,
        text: &str,
        left: f32,
        right: f32,
        (top, bottom): (f32, f32),
        brush: &ID2D1SolidColorBrush,
    ) {
        let (_, text_height) = self.graphics.measure(text);
        let text_top = top + (bottom - top - text_height) / 2.0;
        self.graphics
            .draw_label(self.target, text, left, text_top, right, brush);
    }

    fn is_hovered(&self, target: Target) -> bool {
        self.view.hovered == Some(target)
    }

    fn draw_title_bar(&mut self) {
        let logo_area = logo::square(
            geometry::MARGIN,
            geometry::TITLE_BAR_HEIGHT / 2.0,
            style::LOGO_SIZE,
        );
        logo::draw(self.target, &self.graphics.d2d, logo_area);
        let vertical = (0.0, geometry::TITLE_BAR_HEIGHT);
        let right_edge = geometry::WIDTH - geometry::MARGIN;
        self.label_centred(
            "oh-key",
            geometry::MARGIN + 28.0,
            right_edge,
            vertical,
            &self.brushes.text,
        );
        let version = format!("v{}", settings_model::VERSION);
        let version_width = self.graphics.measure(&version).0;
        let version_left = right_edge - version_width;
        self.label_centred(
            &version,
            version_left,
            geometry::WIDTH,
            vertical,
            &self.brushes.dim,
        );
        self.draw_check_updates_button(version_left);
    }

    /// A quiet text button left of the version.
    fn draw_check_updates_button(&mut self, version_left: f32) {
        let label = if self.view.checking_for_updates {
            "Đang kiểm tra…"
        } else {
            "Kiểm tra cập nhật"
        };
        let text_width = self.graphics.measure(label).0;
        let rect = geometry::title_button_rect(version_left, text_width);
        let brushes = self.brushes;
        let hovered = self.is_hovered(Target::CheckUpdates);
        if hovered {
            self.fill(rect, style::CONTROL_RADIUS, &brushes.hover);
        }
        let tone = if hovered { &brushes.text } else { &brushes.dim };
        self.label_centred(
            label,
            rect.left + geometry::TITLE_BUTTON_PADDING,
            rect.right,
            (rect.top, rect.bottom),
            tone,
        );
        self.hits.push((Target::CheckUpdates, rect));
        self.focus_shapes
            .push((Focus::CheckUpdates, rect, style::CONTROL_RADIUS));
    }

    fn draw_mode_buttons(&mut self, frame: &geometry::Frame) {
        let buttons = settings_model::mode_buttons(self.view.vietnamese);
        for (button, rect) in buttons.iter().zip(frame.mode_buttons) {
            let hit = Target::Mode(button.vietnamese);
            let brushes = self.brushes;
            if button.active {
                self.fill(rect, style::CARD_RADIUS, &brushes.accent_soft);
                self.outline(rect, style::CARD_RADIUS, &brushes.accent);
                self.focus_shapes
                    .push((Focus::Mode, rect, style::CARD_RADIUS));
            } else {
                if self.is_hovered(hit) {
                    self.fill(rect, style::CARD_RADIUS, &brushes.hover);
                }
                self.outline(rect, style::CARD_RADIUS, &brushes.rule);
            }
            let tile_size = 28.0;
            let tile_top = rect.top + (rect.bottom - rect.top - tile_size) / 2.0;
            let tile = Rect {
                left: rect.left + geometry::ROW_INSET,
                top: tile_top,
                right: rect.left + geometry::ROW_INSET + tile_size,
                bottom: tile_top + tile_size,
            };
            self.fill(tile, style::TILE_RADIUS, &brushes.ink);
            self.outline(tile, style::TILE_RADIUS, &brushes.rule);
            let tone = if button.active {
                &brushes.accent
            } else {
                &brushes.text
            };
            self.graphics
                .draw_centred_mono(self.target, button.tile, 16.0, tile, tone);
            self.label_centred(
                button.label,
                tile.right + 10.0,
                rect.right - 8.0,
                (rect.top, rect.bottom),
                tone,
            );
            self.hits.push((hit, rect));
        }
    }

    fn draw_card(&mut self, rows: &[Row], card: Rect) {
        self.fill(card, style::CARD_RADIUS, &self.brushes.panel);
        for (index, row) in rows.iter().enumerate() {
            let row_rect = geometry::row_rect(card, index);
            self.draw_row(row, row_rect);
        }
    }

    fn draw_row(&mut self, row: &Row, row_rect: Rect) {
        let label_right = row_rect.right - geometry::ROW_INSET - geometry::DROPDOWN_WIDTH;
        self.label_centred(
            row.label,
            row_rect.left + geometry::ROW_INSET,
            label_right,
            (row_rect.top, row_rect.bottom),
            &self.brushes.text,
        );
        match &row.control {
            Control::Dropdown { options, selected } => {
                self.draw_dropdown(row.field, &options[*selected], row_rect);
            }
            Control::Chips { options, selected } => {
                self.draw_chips(row.field, options, *selected, row_rect);
            }
            Control::Switch { on } => self.draw_switch(row.field, *on, row_rect),
        }
    }

    fn draw_dropdown(&mut self, field: Field, value: &str, row_rect: Rect) {
        let rect = geometry::dropdown_rect(row_rect);
        let brushes = self.brushes;
        let is_open = self.view.popup.is_some_and(|popup| popup.field == field);
        self.fill(rect, style::CONTROL_RADIUS, &brushes.ink);
        if self.is_hovered(Target::Dropdown(field)) {
            self.fill(rect, style::CONTROL_RADIUS, &brushes.hover);
        }
        let outline = if is_open {
            &brushes.accent
        } else {
            &brushes.rule
        };
        self.outline(rect, style::CONTROL_RADIUS, outline);
        let vertical = (rect.top, rect.bottom);
        self.label_centred(
            value,
            rect.left + 12.0,
            rect.right - 32.0,
            vertical,
            &brushes.text,
        );
        let chevron = Rect {
            left: rect.right - 30.0,
            top: rect.top,
            right: rect.right - 10.0,
            bottom: rect.bottom,
        };
        self.graphics
            .draw_icon(self.target, icons::CHEVRON_DOWN, chevron, &brushes.dim);
        self.hits.push((Target::Dropdown(field), rect));
        self.focus_shapes
            .push((Focus::Row(field), rect, style::CONTROL_RADIUS));
    }

    fn draw_chips(&mut self, field: Field, options: &[String], selected: usize, row_rect: Rect) {
        let widths: Vec<f32> = options
            .iter()
            .map(|option| self.graphics.measure(option).0)
            .collect();
        let rects = geometry::chip_rects(row_rect, &widths);
        let brushes = self.brushes;
        for (index, (option, rect)) in options.iter().zip(&rects).enumerate() {
            let hit = Target::Chip(field, index);
            let tone = if index == selected {
                self.fill(*rect, style::CONTROL_RADIUS, &brushes.accent_soft);
                self.outline(*rect, style::CONTROL_RADIUS, &brushes.accent);
                &brushes.accent
            } else {
                if self.is_hovered(hit) {
                    self.fill(*rect, style::CONTROL_RADIUS, &brushes.hover);
                }
                self.outline(*rect, style::CONTROL_RADIUS, &brushes.rule);
                &brushes.text
            };
            self.label_centred(
                option,
                rect.left + geometry::CHIP_TEXT_PADDING,
                rect.right,
                (rect.top, rect.bottom),
                tone,
            );
            self.hits.push((hit, *rect));
        }
        if let (Some(first), Some(last)) = (rects.first(), rects.last()) {
            let group = geometry::union(*first, *last);
            self.focus_shapes
                .push((Focus::Row(field), group, style::CONTROL_RADIUS));
        }
    }

    fn draw_switch(&mut self, field: Field, on: bool, row_rect: Rect) {
        let rect = geometry::switch_rect(row_rect);
        let brushes = self.brushes;
        let radius = geometry::SWITCH_HEIGHT / 2.0;
        let track = if on { &brushes.accent } else { &brushes.rule };
        self.fill(rect, radius, track);
        let hit = Target::Switch(field);
        if self.is_hovered(hit) {
            self.fill(rect, radius, &brushes.hover);
        }
        let knob_inset = 3.0;
        let knob_radius = radius - knob_inset;
        let knob_x = if on {
            rect.right - knob_inset - knob_radius
        } else {
            rect.left + knob_inset + knob_radius
        };
        let knob_brush = if on { &brushes.ink } else { &brushes.dim };
        let knob_y = (rect.top + rect.bottom) / 2.0;
        let knob = Rect {
            left: knob_x - knob_radius,
            top: knob_y - knob_radius,
            right: knob_x + knob_radius,
            bottom: knob_y + knob_radius,
        };
        self.fill(knob, knob_radius, knob_brush);
        // The whole row switches it, not only the small track.
        self.hits.push((hit, row_rect));
        self.focus_shapes.push((Focus::Row(field), rect, radius));
    }

    fn draw_actions(&mut self, frame: &geometry::Frame) {
        let icon_size = 16.0;
        let icon_gap = 8.0;
        let brushes = self.brushes;
        for (action, rect) in settings_model::actions().iter().zip(frame.actions) {
            let hit = Target::Action(action.tool);
            self.fill(rect, style::CONTROL_RADIUS, &brushes.raised);
            if self.is_hovered(hit) {
                self.fill(rect, style::CONTROL_RADIUS, &brushes.hover);
            }
            self.outline(rect, style::CONTROL_RADIUS, &brushes.rule);
            let text_width = self.graphics.measure(action.label).0;
            let group_width = icon_size + icon_gap + text_width;
            let left = rect.left + (rect.right - rect.left - group_width) / 2.0;
            let icon_area = Rect {
                left,
                top: rect.top,
                right: left + icon_size,
                bottom: rect.bottom,
            };
            self.graphics
                .draw_icon(self.target, action.icon, icon_area, &brushes.text);
            self.label_centred(
                action.label,
                left + icon_size + icon_gap,
                rect.right,
                (rect.top, rect.bottom),
                &brushes.text,
            );
            self.hits.push((hit, rect));
            self.focus_shapes
                .push((Focus::Action(action.tool), rect, style::CONTROL_RADIUS));
        }
    }

    fn draw_focus_ring(&self) {
        let Some(focus) = self.view.focus_ring else {
            return;
        };
        let Some((_, rect, radius)) = self.focus_shapes.iter().find(|(stop, ..)| *stop == focus)
        else {
            return;
        };
        let width = style::FOCUS_RING_WIDTH;
        let offset = style::FOCUS_RING_GAP + width;
        graphics::outline_rounded(
            self.target,
            geometry::grown(*rect, offset),
            radius + offset,
            width,
            &self.brushes.accent,
        );
    }

    /// Draws the open dropdown's list over everything and returns its items' hit rectangles.
    fn draw_popup(&self, cards: &[Vec<Row>], window_height: f32) -> Vec<(Target, Rect)> {
        let Some(popup) = self.view.popup else {
            return Vec::new();
        };
        let control = cards
            .iter()
            .flatten()
            .find(|row| row.field == popup.field)
            .map(|row| &row.control);
        let anchor = self
            .focus_shapes
            .iter()
            .find(|(stop, ..)| *stop == Focus::Row(popup.field))
            .map(|(_, rect, _)| *rect);
        let (Some(Control::Dropdown { options, selected }), Some(anchor)) = (control, anchor)
        else {
            return Vec::new();
        };
        let brushes = self.brushes;
        let list = geometry::popup_rect(anchor, options.len(), window_height);
        self.fill(list, style::CONTROL_RADIUS + 1.0, &brushes.raised);
        self.outline(list, style::CONTROL_RADIUS + 1.0, &brushes.rule);
        let mut hits = Vec::with_capacity(options.len());
        let item_rects = geometry::popup_item_rects(list, options.len());
        for (index, (option, item)) in options.iter().zip(item_rects).enumerate() {
            if index == popup.highlighted {
                self.fill(item, style::CONTROL_RADIUS - 2.0, &brushes.hover);
            }
            let tone = if index == *selected {
                &brushes.accent
            } else {
                &brushes.text
            };
            self.label_centred(
                option,
                item.left + 8.0,
                item.right - 8.0,
                (item.top, item.bottom),
                tone,
            );
            hits.push((Target::PopupItem(popup.field, index), item));
        }
        hits
    }
}

/// Which state of the window `--render-png` draws.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Preview {
    Plain,
    /// The input method list open with its second item lit.
    Dropdown,
    /// The keyboard ring on a switch, and the English button hovered.
    Focus,
}

/// `oh-key.exe --render-png settings out.png`: the window drawn into a file.
pub fn render_png(
    graphics: &Graphics,
    settings: &Settings,
    preview: Preview,
    path: &Path,
) -> Result<()> {
    let width = geometry::WIDTH as u32;
    let height = window_height(settings).round() as u32;
    let offscreen = Offscreen::new(graphics, width, height)?;
    let view = View {
        settings,
        vietnamese: settings.start_vietnamese,
        hovered: (preview == Preview::Focus).then_some(Target::Mode(false)),
        focus_ring: (preview == Preview::Focus).then_some(Focus::Row(Field::SpellCheck)),
        popup: (preview == Preview::Dropdown).then_some(Popup {
            field: Field::InputMethod,
            highlighted: 1,
        }),
        checking_for_updates: false,
    };
    draw(&offscreen.target, graphics, &view);
    offscreen.save_png(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_tab_order_visits_the_modes_every_row_then_the_tools() {
        let order = focus_order(&Settings::default());
        assert_eq!(order.len(), 1 + 14 + 3 + 1);
        assert_eq!(order[0], Focus::Mode);
        assert_eq!(order[1], Focus::Row(Field::InputMethod));
        assert_eq!(order[8], Focus::Row(Field::EnglishInGames));
        assert_eq!(order[10], Focus::Row(Field::StartAtSignIn));
        assert_eq!(order[11], Focus::Row(Field::Updates));
        assert_eq!(order[12], Focus::Row(Field::SoundEffects));
        assert_eq!(order[13], Focus::Row(Field::VisualEffects));
        assert_eq!(order[14], Focus::Row(Field::SendReports));
        assert_eq!(order[15], Focus::Action(Tool::Macros));
        assert_eq!(order[17], Focus::Action(Tool::Converter));
        assert_eq!(order[18], Focus::CheckUpdates);
    }

    #[test]
    fn tab_moves_forward_and_wraps_to_the_start() {
        let order = focus_order(&Settings::default());
        let last = *order.last().unwrap();
        assert_eq!(neighbour_focus(&order, None, false), order[0]);
        assert_eq!(neighbour_focus(&order, Some(order[0]), false), order[1]);
        assert_eq!(neighbour_focus(&order, Some(last), false), order[0]);
    }

    #[test]
    fn shift_tab_moves_back_and_wraps_to_the_end() {
        let order = focus_order(&Settings::default());
        let last = *order.last().unwrap();
        assert_eq!(neighbour_focus(&order, None, true), last);
        assert_eq!(neighbour_focus(&order, Some(order[0]), true), last);
        assert_eq!(neighbour_focus(&order, Some(order[2]), true), order[1]);
    }

    #[test]
    fn a_click_on_a_row_focuses_that_row_and_a_popup_item_focuses_nothing() {
        assert_eq!(
            focus_of(Target::Chip(Field::ToneStyle, 1)),
            Some(Focus::Row(Field::ToneStyle))
        );
        assert_eq!(focus_of(Target::Mode(false)), Some(Focus::Mode));
        assert_eq!(focus_of(Target::CheckUpdates), Some(Focus::CheckUpdates));
        assert_eq!(focus_of(Target::PopupItem(Field::InputMethod, 0)), None);
    }
}
