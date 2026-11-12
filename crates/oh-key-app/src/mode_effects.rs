//! What happens when the mode switches, beyond the tray icon: the sound and the badge, each
//! behind its own setting. Both stay quiet while a full-screen game or a presentation runs, and
//! the badge also follows Windows' "Animation effects" switch.

use oh_key_ipc::load_settings;
use windows::Win32::UI::Shell::{
    QUNS_BUSY, QUNS_PRESENTATION_MODE, QUNS_RUNNING_D3D_FULL_SCREEN, SHQueryUserNotificationState,
};
use windows::Win32::UI::WindowsAndMessaging::{
    SPI_GETCLIENTAREAANIMATION, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS, SystemParametersInfoW,
};
use windows::core::BOOL;

use crate::{mode_flash, sound};

/// Runs on the tray thread after the mode changed; returns at once.
pub fn announce(vietnamese: bool) {
    let settings = load_settings();
    if !settings.sound_effects && !settings.visual_effects {
        return;
    }
    if user_must_not_be_disturbed() {
        return;
    }
    if settings.sound_effects {
        sound::play_mode_switch(vietnamese);
    }
    if settings.visual_effects && animations_enabled() {
        mode_flash::show(vietnamese);
    }
}

fn user_must_not_be_disturbed() -> bool {
    let Ok(state) = (unsafe { SHQueryUserNotificationState() }) else {
        return false;
    };
    [
        QUNS_BUSY,
        QUNS_RUNNING_D3D_FULL_SCREEN,
        QUNS_PRESENTATION_MODE,
    ]
    .contains(&state)
}

fn animations_enabled() -> bool {
    let mut enabled = BOOL(1);
    let read = unsafe {
        SystemParametersInfoW(
            SPI_GETCLIENTAREAANIMATION,
            0,
            Some(&mut enabled as *mut BOOL as *mut core::ffi::c_void),
            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
        )
    };
    read.is_err() || enabled.as_bool()
}
