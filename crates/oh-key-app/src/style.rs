//! The look in Direct2D terms. The colours themselves are in `palette`.

use windows::Win32::Graphics::Direct2D::Common::D2D1_COLOR_F;

use crate::palette::{self, Rgba};

pub const INK: D2D1_COLOR_F = direct2d(palette::INK);
pub const TEXT: D2D1_COLOR_F = direct2d(palette::TEXT);
pub const TEXT_DIM: D2D1_COLOR_F = direct2d(palette::TEXT_DIM);
pub const HOVER: D2D1_COLOR_F = direct2d(palette::HOVER);
pub const ACCENT: D2D1_COLOR_F = direct2d(palette::ACCENT);
pub const ACCENT_SOFT: D2D1_COLOR_F = direct2d(palette::ACCENT_SOFT);

/// The bundled JetBrains Mono (SIL OFL, `assets/fonts`), loaded as a private font.
pub const MONO_FAMILY: &str = "JetBrains Mono";
pub const LABEL_SIZE: f32 = 13.0;
/// Claude Code's spinner glyph, the window's only mark.
pub const SPARK: &str = "✻";
pub const SPARK_SIZE: f32 = 19.0;
pub const ICON_SIZE: f32 = 16.0;

pub const BUTTON_RADIUS: f32 = 4.0;

pub const fn direct2d(color: Rgba) -> D2D1_COLOR_F {
    D2D1_COLOR_F {
        r: color.red,
        g: color.green,
        b: color.blue,
        a: color.alpha,
    }
}
