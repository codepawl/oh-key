//! The oh-key mark, the same as the website's: a rounded accent tile with a dark V. Drawn
//! from vectors so the title bars, the PNG exports and the exe icon stay sharp at any size.

use std::path::Path;

use windows::Win32::Graphics::Direct2D::Common::{D2D1_FIGURE_BEGIN_HOLLOW, D2D1_FIGURE_END_OPEN};
use windows::Win32::Graphics::Direct2D::{
    D2D1_CAP_STYLE_ROUND, D2D1_DASH_STYLE_SOLID, D2D1_LINE_JOIN_ROUND,
    D2D1_STROKE_STYLE_PROPERTIES, ID2D1Factory, ID2D1PathGeometry, ID2D1RenderTarget,
    ID2D1StrokeStyle,
};
use windows::core::Result;
use windows_numerics::Vector2;

use crate::graphics::{Graphics, fill_rounded};
use crate::layout::Rect;
use crate::offscreen::Offscreen;
use crate::style;

/// The website's favicon is drawn on a 32 unit square; these are its numbers.
const DESIGN_SIZE: f32 = 32.0;
const CORNER: f32 = 8.0;
const STROKE_WIDTH: f32 = 3.2;
const V_POINTS: [(f32, f32); 3] = [(9.5, 9.5), (16.0, 23.0), (22.5, 9.5)];

/// The sizes packed into `assets/oh-key.ico`.
pub const ICON_SIZES: [u32; 7] = [16, 20, 24, 32, 48, 64, 256];

/// Draws the mark filling the square `area`.
pub fn draw(target: &ID2D1RenderTarget, factory: &ID2D1Factory, area: Rect) {
    let size = area.right - area.left;
    let unit = size / DESIGN_SIZE;
    let Ok((tile_brush, mark_brush)) = (unsafe {
        target
            .CreateSolidColorBrush(&style::ACCENT, None)
            .and_then(|tile| Ok((tile, target.CreateSolidColorBrush(&style::INK, None)?)))
    }) else {
        return;
    };
    fill_rounded(target, area, CORNER * unit, &tile_brush);
    let (Ok(path), Ok(stroke)) = (v_path(factory, area, unit), round_stroke(factory)) else {
        return;
    };
    unsafe { target.DrawGeometry(&path, &mark_brush, STROKE_WIDTH * unit, &stroke) };
}

/// The mark as a square in a title bar: `size` wide, its left edge at `left`, centred on
/// `centre_y`.
pub fn square(left: f32, centre_y: f32, size: f32) -> Rect {
    Rect {
        left,
        top: centre_y - size / 2.0,
        right: left + size,
        bottom: centre_y + size / 2.0,
    }
}

fn v_path(factory: &ID2D1Factory, area: Rect, unit: f32) -> Result<ID2D1PathGeometry> {
    let point = |(x, y): (f32, f32)| Vector2 {
        X: area.left + x * unit,
        Y: area.top + y * unit,
    };
    unsafe {
        let path = factory.CreatePathGeometry()?;
        let sink = path.Open()?;
        sink.BeginFigure(point(V_POINTS[0]), D2D1_FIGURE_BEGIN_HOLLOW);
        sink.AddLine(point(V_POINTS[1]));
        sink.AddLine(point(V_POINTS[2]));
        sink.EndFigure(D2D1_FIGURE_END_OPEN);
        sink.Close()?;
        Ok(path)
    }
}

fn round_stroke(factory: &ID2D1Factory) -> Result<ID2D1StrokeStyle> {
    let properties = D2D1_STROKE_STYLE_PROPERTIES {
        startCap: D2D1_CAP_STYLE_ROUND,
        endCap: D2D1_CAP_STYLE_ROUND,
        dashCap: D2D1_CAP_STYLE_ROUND,
        lineJoin: D2D1_LINE_JOIN_ROUND,
        miterLimit: 10.0,
        dashStyle: D2D1_DASH_STYLE_SOLID,
        dashOffset: 0.0,
    };
    unsafe { factory.CreateStrokeStyle(&properties, None) }
}

/// `oh-key --render-png logo <folder>`: one transparent PNG per icon size, named
/// `logo-<size>.png`, which `tools/make-icon.py` packs into the exe icon.
pub fn render_pngs(graphics: &Graphics, folder: &Path) -> Result<()> {
    // A folder that cannot be made shows up as the first PNG failing to save.
    let _ = std::fs::create_dir_all(folder);
    for size in ICON_SIZES {
        let offscreen = Offscreen::new(graphics, size, size)?;
        let target = &offscreen.target;
        let edge = size as f32;
        unsafe {
            target.BeginDraw();
            target.Clear(None);
        }
        draw(target, &graphics.d2d, square(0.0, edge / 2.0, edge));
        unsafe { target.EndDraw(None, None)? };
        offscreen.save_png(&folder.join(format!("logo-{size}.png")))?;
    }
    Ok(())
}
