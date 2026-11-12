//! What happens when the mode switches, beyond the tray icon: the sound and the badge, each
//! behind its own setting.
//!
//! Full screen apps still get both: that is where the tray icon is hidden and the user most
//! needs to see the switch. Windows reports any window covering the monitor (a maximised
//! browser, a borderless game) as "busy", so that state cannot mean "quiet" (with only ordinary
//! apps open it can report "busy", which hid the effect at random).
//! Only two states hold something back: a Direct3D game in exclusive full screen keeps the sound
//! but loses the badge, since a topmost window can knock it out of exclusive mode, and
//! Windows' presentation mode silences both. The badge also follows Windows' "Animation
//! effects" switch.

use oh_key_ipc::load_settings;
use windows::Win32::UI::Shell::{
    QUERY_USER_NOTIFICATION_STATE, QUNS_PRESENTATION_MODE, QUNS_RUNNING_D3D_FULL_SCREEN,
    SHQueryUserNotificationState,
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
    let allowed = allowed_effects(notification_state());
    if settings.sound_effects && allowed.sound {
        sound::play_mode_switch(vietnamese);
    }
    if settings.visual_effects && allowed.badge && animations_enabled() {
        mode_flash::show(vietnamese);
    }
}

#[derive(Debug, PartialEq, Eq)]
struct AllowedEffects {
    sound: bool,
    badge: bool,
}

fn notification_state() -> Option<QUERY_USER_NOTIFICATION_STATE> {
    unsafe { SHQueryUserNotificationState() }.ok()
}

fn allowed_effects(state: Option<QUERY_USER_NOTIFICATION_STATE>) -> AllowedEffects {
    match state {
        Some(QUNS_PRESENTATION_MODE) => AllowedEffects {
            sound: false,
            badge: false,
        },
        Some(QUNS_RUNNING_D3D_FULL_SCREEN) => AllowedEffects {
            sound: true,
            badge: false,
        },
        _ => AllowedEffects {
            sound: true,
            badge: true,
        },
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::UI::Shell::{QUNS_ACCEPTS_NOTIFICATIONS, QUNS_BUSY};

    #[test]
    fn a_window_covering_the_screen_still_gets_both() {
        let both = AllowedEffects {
            sound: true,
            badge: true,
        };
        assert_eq!(allowed_effects(Some(QUNS_BUSY)), both);
        assert_eq!(allowed_effects(Some(QUNS_ACCEPTS_NOTIFICATIONS)), both);
        assert_eq!(allowed_effects(None), both);
    }

    #[test]
    fn an_exclusive_full_screen_game_gets_the_sound_only() {
        let allowed = allowed_effects(Some(QUNS_RUNNING_D3D_FULL_SCREEN));
        assert!(allowed.sound);
        assert!(!allowed.badge);
    }

    #[test]
    fn presentation_mode_gets_nothing() {
        let allowed = allowed_effects(Some(QUNS_PRESENTATION_MODE));
        assert!(!allowed.sound);
        assert!(!allowed.badge);
    }
}
