//! The mode switch effect: the V or E tile pops up by the caret, a ring spreads from it and a
//! handful of letters break off and fall, then everything fades, in under a second. Letters
//! with Vietnamese marks fly for Vietnamese and plain ones for English, like the particles in
//! oh-snap.
//!
//! The window is layered, click-through and never activated, so typing never loses focus. It
//! lives on its own short thread with its own Direct2D objects, gone again once the effect ends.

use std::sync::Mutex;
use std::sync::mpsc::{Receiver, Sender, TryRecvError, channel};
use std::time::{Duration, Instant};

use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, POINT, RECT, SIZE, WPARAM};
use windows::Win32::Graphics::Direct2D::Common::{D2D_RECT_F, D2D1_COLOR_F};
use windows::Win32::Graphics::Direct2D::{ID2D1RenderTarget, ID2D1SolidColorBrush};
use windows::Win32::Graphics::Gdi::{
    AC_SRC_ALPHA, AC_SRC_OVER, BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BLENDFUNCTION, ClientToScreen,
    CreateCompatibleDC, CreateDIBSection, DIB_RGB_COLORS, DeleteDC, DeleteObject, GetMonitorInfoW,
    HBITMAP, HDC, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromPoint, SelectObject,
};
use windows::Win32::System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx, CoUninitialize};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GUITHREADINFO, GetCursorPos,
    GetForegroundWindow, GetGUIThreadInfo, GetWindowThreadProcessId, MA_NOACTIVATE, MSG, PM_REMOVE,
    PeekMessageW, RegisterClassExW, SW_SHOWNOACTIVATE, ShowWindow, TranslateMessage, ULW_ALPHA,
    UpdateLayeredWindow, WM_MOUSEACTIVATE, WNDCLASSEXW, WS_EX_LAYERED, WS_EX_NOACTIVATE,
    WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_EX_TRANSPARENT, WS_POPUP,
};
use windows::core::{Result, w};

use crate::graphics::{self, Graphics};
use crate::layout::Rect;
use crate::offscreen::Offscreen;
use crate::{palette, style};

/// The canvas in logical pixels; the tile sits low in it so letters have room to fly up.
const CANVAS_WIDTH: f32 = 176.0;
const CANVAS_HEIGHT: f32 = 136.0;
const TILE_CENTRE: (f32, f32) = (88.0, 88.0);
const TILE_SIZE: f32 = 38.0;
/// From the caret's bottom left to the tile's centre.
const CARET_OFFSET: (f32, f32) = (30.0, 30.0);

pub const DURATION_SECONDS: f32 = 0.80;
const POP_SECONDS: f32 = 0.22;
const FADE_START_SECONDS: f32 = 0.50;
const RING_SECONDS: f32 = 0.42;
const RING_GROWTH: f32 = 30.0;
const BURST_AT_SECONDS: f32 = 0.03;
const PARTICLE_COUNT: usize = 22;
const GRAVITY: f32 = 900.0;
const CLAY_DEEP: D2D1_COLOR_F = style::direct2d(palette::rgb(0xB8, 0x5C, 0x3E, 1.0));
const ENGLISH_GREY: D2D1_COLOR_F = style::direct2d(palette::rgb(0x7F, 0x7A, 0x72, 1.0));
const ENGLISH_GREY_LIGHT: D2D1_COLOR_F = style::direct2d(palette::rgb(0x99, 0x94, 0x8A, 1.0));
/// A faint light edge, so the dark tile stands off a dark editor.
const TILE_EDGE: D2D1_COLOR_F = style::direct2d(palette::rgb(0xF0, 0xEE, 0xE6, 0.22));
const FRAME: Duration = Duration::from_millis(16);

const VIETNAMESE_GLYPHS: [&str; 12] = ["á", "ầ", "ả", "ỗ", "ự", "đ", "ê", "ơ", "ư", "ă", "ẽ", "ạ"];
const ENGLISH_GLYPHS: [&str; 10] = ["a", "e", "k", "w", "z", "f", "#", "*", "+", ":"];

/// Shows the effect for the new mode. Returns at once; a switch while one is playing restarts
/// it with the new mode instead of stacking a second window.
pub fn show(vietnamese: bool) {
    let Ok(mut slot) = RUNNING.lock() else {
        return;
    };
    if let Some(sender) = slot.as_ref()
        && sender.send(vietnamese).is_ok()
    {
        return;
    }
    let (sender, receiver) = channel();
    if sender.send(vietnamese).is_err() {
        return;
    }
    let spawned = std::thread::Builder::new()
        .name("oh-key-mode-flash".to_owned())
        .spawn(move || run_thread(receiver));
    if spawned.is_ok() {
        *slot = Some(sender);
    }
}

/// The channel to the thread that plays the effect, while one runs.
static RUNNING: Mutex<Option<Sender<bool>>> = Mutex::new(None);

fn run_thread(receiver: Receiver<bool>) {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
    }
    let Ok(mut vietnamese) = receiver.recv() else {
        return;
    };
    loop {
        let restart = play(vietnamese, &receiver).unwrap_or(None);
        if let Some(next) = restart {
            vietnamese = next;
            continue;
        }
        // `show` sends while holding the lock, so a switch is either seen here or starts a
        // new thread after the slot is cleared, never lost in between.
        let Ok(mut slot) = RUNNING.lock() else {
            break;
        };
        match receiver.try_recv() {
            Ok(next) => vietnamese = next,
            Err(_) => {
                *slot = None;
                break;
            }
        }
    }
    unsafe { CoUninitialize() };
}

/// Plays the effect once; `Some(mode)` when a new switch arrived mid-way.
fn play(vietnamese: bool, receiver: &Receiver<bool>) -> Result<Option<bool>> {
    let anchor = effect_anchor();
    let scale = monitor_scale(anchor);
    let pixel_width = (CANVAS_WIDTH * scale).ceil() as u32;
    let pixel_height = (CANVAS_HEIGHT * scale).ceil() as u32;
    let origin = canvas_origin(anchor, scale, pixel_width as i32, pixel_height as i32);

    let graphics = Graphics::new()?;
    let offscreen = Offscreen::new(&graphics, pixel_width, pixel_height)?;
    unsafe { offscreen.target.SetDpi(96.0 * scale, 96.0 * scale) };
    let surface = LayeredSurface::new(pixel_width, pixel_height)?;
    let window = create_window()?;
    let mut scene = Scene::new(vietnamese);
    let started = Instant::now();
    let mut shown = false;
    let outcome = loop {
        match receiver.try_recv() {
            Ok(next) => break Some(next),
            Err(TryRecvError::Disconnected) => break None,
            Err(TryRecvError::Empty) => {}
        }
        let elapsed = started.elapsed().as_secs_f32();
        if elapsed >= DURATION_SECONDS {
            break None;
        }
        scene.advance_to(elapsed);
        draw_frame(&offscreen.target, &graphics, &scene)?;
        surface.present(&offscreen, window, origin)?;
        if !shown {
            unsafe {
                let _ = ShowWindow(window, SW_SHOWNOACTIVATE);
            }
            shown = true;
        }
        pump_messages();
        std::thread::sleep(FRAME);
    };
    unsafe {
        let _ = DestroyWindow(window);
    }
    Ok(outcome)
}

/// Where the effect points: the caret of the window being typed in, else the mouse pointer.
/// Only the caret's rectangle is read, never the text around it.
fn effect_anchor() -> POINT {
    caret_bottom_left().unwrap_or_else(|| {
        let mut point = POINT::default();
        unsafe {
            let _ = GetCursorPos(&mut point);
        }
        point
    })
}

fn caret_bottom_left() -> Option<POINT> {
    let foreground = unsafe { GetForegroundWindow() };
    if foreground.is_invalid() {
        return None;
    }
    let thread_id = unsafe { GetWindowThreadProcessId(foreground, None) };
    let mut info = GUITHREADINFO {
        cbSize: size_of::<GUITHREADINFO>() as u32,
        ..Default::default()
    };
    unsafe { GetGUIThreadInfo(thread_id, &mut info) }.ok()?;
    let caret = info.rcCaret;
    if info.hwndCaret.is_invalid() || caret_is_empty(caret) {
        return None;
    }
    let mut point = POINT {
        x: caret.left,
        y: caret.bottom,
    };
    unsafe { ClientToScreen(info.hwndCaret, &mut point) }
        .as_bool()
        .then_some(point)
}

fn caret_is_empty(caret: RECT) -> bool {
    caret.bottom <= caret.top && caret.right <= caret.left
}

fn monitor_scale(point: POINT) -> f32 {
    let monitor = unsafe { MonitorFromPoint(point, MONITOR_DEFAULTTONEAREST) };
    let mut dpi_x = 96;
    let mut dpi_y = 96;
    let _ = unsafe { GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut dpi_x, &mut dpi_y) };
    (dpi_x.max(96) as f32) / 96.0
}

/// The canvas's top-left corner on screen: the tile just below and right of the caret, kept
/// inside the monitor's work area.
fn canvas_origin(anchor: POINT, scale: f32, width: i32, height: i32) -> POINT {
    let tile_left = anchor.x + (CARET_OFFSET.0 * scale) as i32;
    let tile_top = anchor.y + (CARET_OFFSET.1 * scale) as i32;
    let wanted = POINT {
        x: tile_left - (TILE_CENTRE.0 * scale) as i32,
        y: tile_top - (TILE_CENTRE.1 * scale) as i32,
    };
    let monitor = unsafe { MonitorFromPoint(anchor, MONITOR_DEFAULTTONEAREST) };
    let mut info = MONITORINFO {
        cbSize: size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    if !unsafe { GetMonitorInfoW(monitor, &mut info) }.as_bool() {
        return wanted;
    }
    clamp_into(wanted, width, height, info.rcWork)
}

fn clamp_into(wanted: POINT, width: i32, height: i32, area: RECT) -> POINT {
    POINT {
        x: wanted.x.min(area.right - width).max(area.left),
        y: wanted.y.min(area.bottom - height).max(area.top),
    }
}

fn create_window() -> Result<HWND> {
    let instance = unsafe { GetModuleHandleW(None)? };
    let class_name = w!("oh-key-mode-flash");
    let class = WNDCLASSEXW {
        cbSize: size_of::<WNDCLASSEXW>() as u32,
        lpfnWndProc: Some(window_proc),
        hInstance: instance.into(),
        lpszClassName: class_name,
        ..Default::default()
    };
    // Registering twice fails harmlessly; the class from the first call stays.
    unsafe { RegisterClassExW(&class) };
    unsafe {
        CreateWindowExW(
            WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_NOACTIVATE | WS_EX_TOPMOST | WS_EX_TOOLWINDOW,
            class_name,
            w!(""),
            WS_POPUP,
            0,
            0,
            0,
            0,
            None,
            None,
            Some(instance.into()),
            None,
        )
    }
}

unsafe extern "system" fn window_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if message == WM_MOUSEACTIVATE {
        return LRESULT(MA_NOACTIVATE as isize);
    }
    unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
}

fn pump_messages() {
    let mut message = MSG::default();
    while unsafe { PeekMessageW(&mut message, None, 0, 0, PM_REMOVE) }.as_bool() {
        unsafe {
            let _ = TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    }
}

/// A memory DC holding a top-down 32-bit DIB that `UpdateLayeredWindow` reads from.
struct LayeredSurface {
    device_context: HDC,
    bitmap: HBITMAP,
    bits: *mut u8,
    width: u32,
    height: u32,
}

impl LayeredSurface {
    fn new(width: u32, height: u32) -> Result<Self> {
        let header = BITMAPINFOHEADER {
            biSize: size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: width as i32,
            // Negative: rows run top to bottom, as WIC copies them.
            biHeight: -(height as i32),
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        };
        let info = BITMAPINFO {
            bmiHeader: header,
            ..Default::default()
        };
        let device_context = unsafe { CreateCompatibleDC(None) };
        let mut bits = std::ptr::null_mut();
        let bitmap = unsafe {
            CreateDIBSection(
                Some(device_context),
                &info,
                DIB_RGB_COLORS,
                &mut bits,
                None,
                0,
            )
        };
        let bitmap = match bitmap {
            Ok(bitmap) => bitmap,
            Err(error) => {
                unsafe {
                    let _ = DeleteDC(device_context);
                }
                return Err(error);
            }
        };
        unsafe { SelectObject(device_context, bitmap.into()) };
        Ok(Self {
            device_context,
            bitmap,
            bits: bits as *mut u8,
            width,
            height,
        })
    }

    fn present(&self, offscreen: &Offscreen, window: HWND, origin: POINT) -> Result<()> {
        let length = (self.width * self.height * 4) as usize;
        let pixels = unsafe { std::slice::from_raw_parts_mut(self.bits, length) };
        offscreen.copy_premultiplied_pixels(pixels)?;
        let size = SIZE {
            cx: self.width as i32,
            cy: self.height as i32,
        };
        let source = POINT::default();
        let blend = BLENDFUNCTION {
            BlendOp: AC_SRC_OVER as u8,
            BlendFlags: 0,
            SourceConstantAlpha: 255,
            AlphaFormat: AC_SRC_ALPHA as u8,
        };
        unsafe {
            UpdateLayeredWindow(
                window,
                None,
                Some(&origin),
                Some(&size),
                Some(self.device_context),
                Some(&source),
                COLORREF(0),
                Some(&blend),
                ULW_ALPHA,
            )
        }
    }
}

impl Drop for LayeredSurface {
    fn drop(&mut self) {
        unsafe {
            let _ = DeleteObject(self.bitmap.into());
            let _ = DeleteDC(self.device_context);
        }
    }
}

struct Particle {
    x: f32,
    y: f32,
    velocity_x: f32,
    velocity_y: f32,
    born: f32,
    lifetime: f32,
    glyph: Option<&'static str>,
    size: f32,
    bright: bool,
}

/// Everything the frame shows at one moment. Pure arithmetic, so it is tested without a window.
pub struct Scene {
    vietnamese: bool,
    elapsed: f32,
    particles: Vec<Particle>,
    burst_done: bool,
    random_state: u32,
}

impl Scene {
    pub fn new(vietnamese: bool) -> Self {
        Self {
            vietnamese,
            elapsed: 0.0,
            particles: Vec::with_capacity(PARTICLE_COUNT),
            burst_done: false,
            random_state: if vietnamese { 0x9E37_79B9 } else { 0x85EB_CA6B },
        }
    }

    /// Moves to `elapsed` seconds after the switch, in small steps so a late frame does not
    /// throw letters off their arc.
    pub fn advance_to(&mut self, elapsed: f32) {
        let step = 1.0 / 120.0;
        while self.elapsed < elapsed {
            let next = (self.elapsed + step).min(elapsed);
            let delta = next - self.elapsed;
            self.elapsed = next;
            if !self.burst_done && self.elapsed >= BURST_AT_SECONDS {
                self.burst();
            }
            for particle in &mut self.particles {
                if self.elapsed < particle.born {
                    continue;
                }
                particle.velocity_y += GRAVITY * delta;
                particle.x += particle.velocity_x * delta;
                particle.y += particle.velocity_y * delta;
            }
        }
    }

    fn burst(&mut self) {
        self.burst_done = true;
        for index in 0..PARTICLE_COUNT {
            // Spread around the top half of the tile, flung up and out.
            let angle = std::f32::consts::PI
                * (1.05 + 0.9 * (index as f32 + self.random()) / PARTICLE_COUNT as f32);
            let speed = 150.0 + self.random() * 170.0;
            let is_glyph = self.random() < 0.55;
            let glyph = is_glyph.then(|| self.pick_glyph());
            let lifetime = (0.40 + self.random() * 0.30).min(DURATION_SECONDS - BURST_AT_SECONDS);
            let particle = Particle {
                x: TILE_CENTRE.0 + angle.cos() * TILE_SIZE * 0.45,
                y: TILE_CENTRE.1 + angle.sin() * TILE_SIZE * 0.45,
                velocity_x: angle.cos() * speed,
                velocity_y: angle.sin() * speed,
                born: BURST_AT_SECONDS,
                lifetime,
                glyph,
                size: 3.0 + (self.random() * 3.0).floor(),
                bright: self.random() < 0.6,
            };
            self.particles.push(particle);
        }
    }

    fn pick_glyph(&mut self) -> &'static str {
        let glyphs: &[&'static str] = if self.vietnamese {
            &VIETNAMESE_GLYPHS
        } else {
            &ENGLISH_GLYPHS
        };
        glyphs[(self.random() * glyphs.len() as f32) as usize % glyphs.len()]
    }

    /// The tile's scale: pops past full size and settles, an ease-out with overshoot.
    pub fn tile_scale(&self) -> f32 {
        let progress = (self.elapsed / POP_SECONDS).min(1.0);
        let overshoot = 1.9;
        let shifted = progress - 1.0;
        let eased = 1.0 + (overshoot + 1.0) * shifted.powi(3) + overshoot * shifted.powi(2);
        0.55 + 0.45 * eased
    }

    pub fn tile_opacity(&self) -> f32 {
        let fade_in = (self.elapsed / 0.08).min(1.0);
        let fade_out = 1.0
            - ((self.elapsed - FADE_START_SECONDS) / (DURATION_SECONDS - FADE_START_SECONDS))
                .clamp(0.0, 1.0);
        fade_in * fade_out * fade_out.sqrt()
    }

    /// The tile drifts up a little while it fades.
    fn tile_lift(&self) -> f32 {
        let progress = ((self.elapsed - FADE_START_SECONDS)
            / (DURATION_SECONDS - FADE_START_SECONDS))
            .clamp(0.0, 1.0);
        -8.0 * progress
    }

    /// The ring's growth (0 to 1) and opacity, or `None` once it has gone.
    fn ring(&self) -> Option<(f32, f32)> {
        let progress = (self.elapsed - BURST_AT_SECONDS) / RING_SECONDS;
        if !(0.0..1.0).contains(&progress) {
            return None;
        }
        let eased = 1.0 - (1.0 - progress).powi(3);
        Some((eased, 0.6 * (1.0 - progress)))
    }

    #[cfg(test)]
    fn live_particles(&self) -> usize {
        self.particles
            .iter()
            .filter(|particle| self.particle_opacity(particle) > 0.0)
            .count()
    }

    fn particle_opacity(&self, particle: &Particle) -> f32 {
        let age = self.elapsed - particle.born;
        if age < 0.0 || age >= particle.lifetime {
            return 0.0;
        }
        let progress = age / particle.lifetime;
        1.0 - progress * progress
    }

    fn random(&mut self) -> f32 {
        self.random_state ^= self.random_state << 13;
        self.random_state ^= self.random_state >> 17;
        self.random_state ^= self.random_state << 5;
        self.random_state as f32 / u32::MAX as f32
    }
}

/// Clay for Vietnamese, a neutral grey for English: mid tones that read on a white page and on
/// a dark editor alike, which the ivory text colour does not.
fn particle_colors(vietnamese: bool) -> (D2D1_COLOR_F, D2D1_COLOR_F) {
    if vietnamese {
        (style::ACCENT, CLAY_DEEP)
    } else {
        (ENGLISH_GREY, ENGLISH_GREY_LIGHT)
    }
}

fn with_opacity(color: D2D1_COLOR_F, opacity: f32) -> D2D1_COLOR_F {
    D2D1_COLOR_F {
        a: color.a * opacity,
        ..color
    }
}

/// Draws the scene in logical pixels; the target's dpi carries the display scale.
pub fn draw_frame(target: &ID2D1RenderTarget, graphics: &Graphics, scene: &Scene) -> Result<()> {
    let accent = if scene.vietnamese {
        style::ACCENT
    } else {
        style::TEXT
    };
    let brush = unsafe { target.CreateSolidColorBrush(&accent, None)? };
    let (ring_color, _) = particle_colors(scene.vietnamese);
    unsafe {
        target.BeginDraw();
        target.Clear(None);
    }
    draw_ring(target, scene, ring_color, &brush);
    draw_particles(target, graphics, scene, &brush);
    draw_tile(target, graphics, scene, accent, &brush);
    unsafe { target.EndDraw(None, None) }
}

fn draw_ring(
    target: &ID2D1RenderTarget,
    scene: &Scene,
    accent: D2D1_COLOR_F,
    brush: &ID2D1SolidColorBrush,
) {
    let Some((growth, opacity)) = scene.ring() else {
        return;
    };
    let half = TILE_SIZE / 2.0 + growth * RING_GROWTH;
    let ring = centred_square(TILE_CENTRE.0, TILE_CENTRE.1, half);
    unsafe { brush.SetColor(&with_opacity(accent, opacity)) };
    let width = 2.0 * (1.0 - growth) + 1.0;
    graphics::outline_rounded(target, ring, half * 0.44, width, brush);
}

fn draw_particles(
    target: &ID2D1RenderTarget,
    graphics: &Graphics,
    scene: &Scene,
    brush: &ID2D1SolidColorBrush,
) {
    let (bright, soft) = particle_colors(scene.vietnamese);
    for particle in &scene.particles {
        let opacity = scene.particle_opacity(particle);
        if opacity <= 0.0 {
            continue;
        }
        let color = if particle.bright { bright } else { soft };
        unsafe { brush.SetColor(&with_opacity(color, opacity)) };
        match particle.glyph {
            Some(glyph) => {
                let left = particle.x - 4.0;
                let top = particle.y - 9.0;
                graphics.draw_label(target, glyph, left, top, left + 20.0, brush);
            }
            None => {
                // Whole pixels keep the squares crisp, like oh-snap's voxels.
                let left = particle.x.round();
                let top = particle.y.round();
                let square = D2D_RECT_F {
                    left,
                    top,
                    right: left + particle.size,
                    bottom: top + particle.size,
                };
                unsafe { target.FillRectangle(&square, brush) };
            }
        }
    }
}

fn draw_tile(
    target: &ID2D1RenderTarget,
    graphics: &Graphics,
    scene: &Scene,
    accent: D2D1_COLOR_F,
    brush: &ID2D1SolidColorBrush,
) {
    let opacity = scene.tile_opacity();
    if opacity <= 0.0 {
        return;
    }
    let half = TILE_SIZE * scene.tile_scale() / 2.0;
    let tile = centred_square(TILE_CENTRE.0, TILE_CENTRE.1 + scene.tile_lift(), half);
    let corner = half * 0.44;
    unsafe { brush.SetColor(&with_opacity(style::INK, opacity)) };
    graphics::fill_rounded(target, tile, corner, brush);
    unsafe { brush.SetColor(&with_opacity(TILE_EDGE, opacity)) };
    graphics::outline_rounded(target, tile, corner, style::OUTLINE_WIDTH, brush);
    unsafe { brush.SetColor(&with_opacity(accent, opacity)) };
    let letter = crate::mode_icon::letter(scene.vietnamese);
    graphics.draw_centred_mono(target, letter, half * 2.0 * 0.62, tile, brush);
}

fn centred_square(centre_x: f32, centre_y: f32, half: f32) -> Rect {
    Rect {
        left: centre_x - half,
        top: centre_y - half,
        right: centre_x + half,
        bottom: centre_y + half,
    }
}

/// `oh-key --render-png mode-flash <folder>`: every frame of both effects at 30 fps and 2x, as
/// `vietnamese-NN.png` and `english-NN.png`, to check the motion without opening a window.
pub fn render_frames(graphics: &Graphics, folder: &std::path::Path) -> Result<()> {
    let scale = 2.0;
    let width = (CANVAS_WIDTH * scale) as u32;
    let height = (CANVAS_HEIGHT * scale) as u32;
    std::fs::create_dir_all(folder).map_err(|_| windows::Win32::Foundation::E_FAIL)?;
    for vietnamese in [true, false] {
        let name = if vietnamese { "vietnamese" } else { "english" };
        let mut scene = Scene::new(vietnamese);
        let frame_count = (DURATION_SECONDS * 30.0).ceil() as usize;
        for frame in 0..=frame_count {
            let offscreen = Offscreen::new(graphics, width, height)?;
            unsafe { offscreen.target.SetDpi(96.0 * scale, 96.0 * scale) };
            scene.advance_to(frame as f32 / 30.0);
            draw_frame(&offscreen.target, graphics, &scene)?;
            offscreen.save_png(&folder.join(format!("{name}-{frame:02}.png")))?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_tile_pops_past_full_size_and_settles() {
        let mut scene = Scene::new(true);
        assert!(scene.tile_scale() < 0.6);
        let mut largest: f32 = 0.0;
        for step in 1..=22 {
            scene.advance_to(step as f32 * 0.01);
            largest = largest.max(scene.tile_scale());
        }
        assert!(largest > 1.02, "no overshoot: {largest}");
        scene.advance_to(0.3);
        assert!((scene.tile_scale() - 1.0).abs() < 1e-4);
    }

    #[test]
    fn everything_is_gone_when_the_effect_ends() {
        for vietnamese in [true, false] {
            let mut scene = Scene::new(vietnamese);
            scene.advance_to(0.1);
            assert!(scene.live_particles() > 0);
            assert!(scene.tile_opacity() > 0.9);
            scene.advance_to(DURATION_SECONDS);
            assert_eq!(scene.live_particles(), 0);
            assert!(scene.tile_opacity() < 1e-4);
            assert!(scene.ring().is_none());
        }
    }

    #[test]
    fn letters_fly_up_first_then_fall() {
        let mut scene = Scene::new(false);
        scene.advance_to(0.12);
        let highest = scene
            .particles
            .iter()
            .fold(f32::MAX, |highest, particle| highest.min(particle.y));
        assert!(highest < TILE_CENTRE.1 - 25.0, "highest {highest}");
        let vertical_speeds: Vec<f32> = scene.particles.iter().map(|p| p.velocity_y).collect();
        scene.advance_to(0.6);
        for (particle, earlier) in scene.particles.iter().zip(vertical_speeds) {
            assert!(particle.velocity_y > earlier);
        }
    }

    #[test]
    fn vietnamese_throws_marked_letters_and_english_plain_ones() {
        let mut vietnamese = Scene::new(true);
        vietnamese.advance_to(0.05);
        let mut english = Scene::new(false);
        english.advance_to(0.05);
        let glyphs = |scene: &Scene| -> Vec<&str> {
            scene.particles.iter().filter_map(|p| p.glyph).collect()
        };
        assert!(!glyphs(&vietnamese).is_empty());
        assert!(
            glyphs(&vietnamese)
                .iter()
                .all(|g| VIETNAMESE_GLYPHS.contains(g))
        );
        assert!(glyphs(&english).iter().all(|g| ENGLISH_GLYPHS.contains(g)));
    }

    #[test]
    fn the_canvas_stays_inside_the_work_area() {
        let area = RECT {
            left: 0,
            top: 0,
            right: 1920,
            bottom: 1040,
        };
        let corner = clamp_into(POINT { x: 1900, y: 1030 }, 176, 136, area);
        assert_eq!((corner.x, corner.y), (1920 - 176, 1040 - 136));
        let start = clamp_into(POINT { x: -40, y: -20 }, 176, 136, area);
        assert_eq!((start.x, start.y), (0, 0));
    }
}
