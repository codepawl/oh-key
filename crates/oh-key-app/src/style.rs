//! The look in Direct2D terms. The colours themselves are in `palette`.

use windows::Win32::Graphics::Direct2D::Common::D2D1_COLOR_F;

use crate::palette::{self, Rgba};

pub const INK: D2D1_COLOR_F = direct2d(palette::INK);
pub const INK_PANEL: D2D1_COLOR_F = direct2d(palette::INK_PANEL);
pub const RAISED: D2D1_COLOR_F = direct2d(palette::RAISED);
pub const RULE: D2D1_COLOR_F = direct2d(palette::RULE);
pub const TEXT: D2D1_COLOR_F = direct2d(palette::TEXT);
pub const TEXT_DIM: D2D1_COLOR_F = direct2d(palette::TEXT_DIM);
pub const HOVER: D2D1_COLOR_F = direct2d(palette::HOVER);
pub const ACCENT: D2D1_COLOR_F = direct2d(palette::ACCENT);
pub const ACCENT_SOFT: D2D1_COLOR_F = direct2d(palette::ACCENT_SOFT);

/// The bundled JetBrains Mono (SIL OFL, `assets/fonts`), loaded as a private font.
pub const MONO_FAMILY: &str = "JetBrains Mono";
pub const LABEL_SIZE: f32 = 13.0;
/// The oh-key mark beside a window title.
pub const LOGO_SIZE: f32 = 18.0;
pub const ICON_SIZE: f32 = 16.0;

pub const BUTTON_RADIUS: f32 = 4.0;
/// The settings window's rounder shapes.
pub const CARD_RADIUS: f32 = 10.0;
pub const CONTROL_RADIUS: f32 = 7.0;
pub const TILE_RADIUS: f32 = 8.0;
pub const OUTLINE_WIDTH: f32 = 1.0;
pub const FOCUS_RING_WIDTH: f32 = 2.0;
/// Clear space between a control and its ring, so a ring on an accent control still shows.
pub const FOCUS_RING_GAP: f32 = 2.0;

pub const fn direct2d(color: Rgba) -> D2D1_COLOR_F {
    D2D1_COLOR_F {
        r: color.red,
        g: color.green,
        b: color.blue,
        a: color.alpha,
    }
}
