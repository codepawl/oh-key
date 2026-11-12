//! Saving a settings change and telling the running hook about it.

use oh_key_ipc::{Settings, SharedState, save_settings};

/// Writes `config.toml` and publishes the same settings into the shared state.
pub fn save_and_publish(settings: &Settings) {
    let _ = save_settings(settings);
    publish(settings);
}

pub fn publish(settings: &Settings) {
    if let Ok(shared) = SharedState::open() {
        shared.publish_settings(settings);
    }
}
