//! The tray icon: an ink tile with a bold bright letter, "V" in the accent for Vietnamese and
//! "E" in the text colour for English. Drawn at run time so it follows the DPI of the display.

use windows::Win32::Graphics::Direct2D::D2D1_ROUNDED_RECT;
use windows::Win32::Graphics::Direct2D::ID2D1RenderTarget;
use windows::Win32::UI::WindowsAndMessaging::{CreateIcon, GetSystemMetrics, HICON, SM_CXSMICON};
use windows::core::Result;

use crate::graphics::Graphics;
use crate::layout::Rect;
use crate::offscreen::Offscreen;
use crate::style;

pub fn letter(vietnamese: bool) -> &'static str {
    if vietnamese { "V" } else { "E" }
}

/// Draws the icon for the mode into the square whose top-left corner is `left`, `top`.
pub fn draw(
    target: &ID2D1RenderTarget,
    graphics: &Graphics,
    vietnamese: bool,
    left: f32,
    top: f32,
    size: f32,
) {
    let area = Rect {
        left,
        top,
        right: left + size,
        bottom: top + size,
    };
    // Bright letters: the first icon used dim text and a regular weight, which vanished at 16 px.
    let letter_color = if vietnamese {
        style::ACCENT
    } else {
        style::TEXT
    };
    let corner = size * 0.22;
    unsafe {
        let (Ok(tile_brush), Ok(letter_brush)) = (
            target.CreateSolidColorBrush(&style::INK, None),
            target.CreateSolidColorBrush(&letter_color, None),
        ) else {
            return;
        };
        let tile = D2D1_ROUNDED_RECT {
            rect: area.into(),
            radiusX: corner,
            radiusY: corner,
        };
        target.FillRoundedRectangle(&tile, &tile_brush);
        graphics.draw_centred_mono(target, letter(vietnamese), size * 0.82, area, &letter_brush);
    }
}

/// The icon at the size Windows uses for small icons on this display. COM must be initialised.
pub fn create_icon(graphics: &Graphics, vietnamese: bool) -> Result<HICON> {
    let size = unsafe { GetSystemMetrics(SM_CXSMICON) }.max(16) as u32;
    let offscreen = Offscreen::new(graphics, size, size)?;
    unsafe {
        offscreen.target.BeginDraw();
        offscreen.target.Clear(None);
    }
    draw(
        &offscreen.target,
        graphics,
        vietnamese,
        0.0,
        0.0,
        size as f32,
    );
    unsafe { offscreen.target.EndDraw(None, None)? };
    let color = offscreen.straight_alpha_pixels()?;
    let mask = vec![0u8; (size * size / 8) as usize];
    unsafe {
        CreateIcon(
            None,
            size as i32,
            size as i32,
            1,
            32,
            mask.as_ptr(),
            color.as_ptr(),
        )
    }
}
