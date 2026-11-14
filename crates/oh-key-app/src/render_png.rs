//! `oh-key.exe --render-png <settings|settings-dropdown|settings-focus|tray-menu-preview|...> <out.png>`: draws a window's content
//! offscreen to a PNG, so the look can be checked without opening anything on the desktop.

use std::path::Path;

use windows::Win32::Graphics::Direct2D::Common::D2D1_COLOR_F;
use windows::core::Result;

use crate::graphics::Graphics;
use crate::offscreen::Offscreen;
use crate::settings_window::Preview;
use crate::{
    converter_window, excluded_apps_window, macros_window, mode_flash, mode_icon, settings_window,
    style, tray,
};

const PREVIEW_WIDTH: u32 = 520;
const PREVIEW_HEIGHT: u32 = 400;
const PREVIEW_ICON_SIZE: f32 = 64.0;

pub fn render(graphics: &Graphics, window: &str, path: &Path) -> Result<()> {
    match window {
        "settings" => render_settings(graphics, Preview::Plain, path),
        "settings-dropdown" => render_settings(graphics, Preview::Dropdown, path),
        "settings-focus" => render_settings(graphics, Preview::Focus, path),
        "excluded-apps" => excluded_apps_window::render_png(graphics, path),
        "tray-menu-preview" => render_tray_preview(graphics, path),
        "macros" => macros_window::render_png(graphics, path),
        "converter" => converter_window::render_png(graphics, path),
        "mode-flash" => mode_flash::render_frames(graphics, path),
        "logo" => crate::logo::render_pngs(graphics, path),
        _ => Err(windows::core::Error::new(
            windows::Win32::Foundation::E_INVALIDARG,
            "window must be settings, settings-dropdown, settings-focus, excluded-apps, macros, converter, mode-flash, logo or tray-menu-preview",
        )),
    }
}

fn render_settings(graphics: &Graphics, preview: Preview, path: &Path) -> Result<()> {
    let settings = oh_key_ipc::load_settings();
    settings_window::render_png(graphics, &settings, preview, path)
}

/// The two tray icons at 64 px beside the menu's entries. The real menu is a native popup,
/// which cannot be drawn offscreen, so this lists what it holds.
fn render_tray_preview(graphics: &Graphics, path: &Path) -> Result<()> {
    let offscreen = Offscreen::new(graphics, PREVIEW_WIDTH, PREVIEW_HEIGHT)?;
    let target = &offscreen.target;
    let brush = |color: D2D1_COLOR_F| unsafe { target.CreateSolidColorBrush(&color, None) };
    let (text, dim, accent) = (
        brush(style::TEXT)?,
        brush(style::TEXT_DIM)?,
        brush(style::ACCENT)?,
    );
    unsafe {
        target.BeginDraw();
        target.Clear(Some(&style::INK));
    }
    mode_icon::draw(target, graphics, true, 32.0, 32.0, PREVIEW_ICON_SIZE);
    mode_icon::draw(target, graphics, false, 128.0, 32.0, PREVIEW_ICON_SIZE);
    graphics.draw_label(target, tray::LABEL_VIETNAMESE, 32.0, 108.0, 200.0, &accent);
    graphics.draw_label(target, tray::LABEL_ENGLISH, 128.0, 108.0, 300.0, &dim);

    let labels = [
        tray::LABEL_VIETNAMESE,
        tray::LABEL_ENGLISH,
        tray::LABEL_INPUT_METHOD,
        tray::LABEL_SETTINGS,
        tray::LABEL_MACROS,
        tray::LABEL_CONVERTER,
        tray::LABEL_CHECK_UPDATES,
        tray::LABEL_OPEN_LOG,
        tray::LABEL_QUIT,
    ];
    let mut top = 160.0;
    for label in labels {
        graphics.draw_label(target, label, 32.0, top, 300.0, &text);
        top += 24.0;
    }
    unsafe { target.EndDraw(None, None)? };
    offscreen.save_png(path)
}
