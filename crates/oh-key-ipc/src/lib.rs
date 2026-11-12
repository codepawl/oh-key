//! What the parts of the oh-key app share.
//!
//! `settings` is the config file format; `SharedState` (Windows only) is a
//! named memory block that carries the settings and the Vietnamese/English
//! mode between the tray, the keyboard hook and a second launch of the app.

mod block;
mod settings;
#[cfg(windows)]
mod shared_state;

pub use settings::{
    Settings, ToggleHotkey, config_path, format_settings, load_settings, macros_path,
    parse_settings, save_settings,
};
#[cfg(windows)]
pub use shared_state::SharedState;
