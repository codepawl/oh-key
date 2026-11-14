//! The C interface used by the IBus glue (`include/oh_key_ibus.h`).

use crate::config::config_folder_from_environment;
use crate::typing::{KeyboardEngine, PreeditChange, Reaction};

pub const OH_KEY_IBUS_PREEDIT_KEEP: u8 = 0;
pub const OH_KEY_IBUS_PREEDIT_SHOW: u8 = 1;
pub const OH_KEY_IBUS_PREEDIT_HIDE: u8 = 2;

/// The engine plus the strings of the last reaction, which stay valid until the next call.
pub struct OhKeyIbusEngine {
    typing: KeyboardEngine,
    commit: String,
    preedit: String,
}

#[repr(C)]
pub struct OhKeyIbusReaction {
    /// Nonzero: the key is used up and must not be forwarded.
    pub handled: u8,
    /// An `OH_KEY_IBUS_PREEDIT_*` value.
    pub preedit_change: u8,
    pub commit: *const u8,
    pub commit_length: usize,
    pub preedit: *const u8,
    pub preedit_length: usize,
}

fn write_reaction(engine: &mut OhKeyIbusEngine, reaction: Reaction, out: *mut OhKeyIbusReaction) {
    engine.commit = reaction.commit;
    engine.preedit.clear();
    let preedit_change = match reaction.preedit {
        PreeditChange::Keep => OH_KEY_IBUS_PREEDIT_KEEP,
        PreeditChange::Hide => OH_KEY_IBUS_PREEDIT_HIDE,
        PreeditChange::Show(word) => {
            engine.preedit = word;
            OH_KEY_IBUS_PREEDIT_SHOW
        }
    };
    let filled = OhKeyIbusReaction {
        handled: u8::from(reaction.handled),
        preedit_change,
        commit: engine.commit.as_ptr(),
        commit_length: engine.commit.len(),
        preedit: engine.preedit.as_ptr(),
        preedit_length: engine.preedit.len(),
    };
    if !out.is_null() {
        // SAFETY: the caller passes a pointer to a writable OhKeyIbusReaction (checked non-null).
        unsafe { out.write(filled) };
    }
}

/// Creates the engine and reads `$XDG_CONFIG_HOME/oh-key/`. Free it with
/// `oh_key_ibus_engine_free`.
#[unsafe(no_mangle)]
pub extern "C" fn oh_key_ibus_engine_new() -> *mut OhKeyIbusEngine {
    let engine = OhKeyIbusEngine {
        typing: KeyboardEngine::new(config_folder_from_environment()),
        commit: String::new(),
        preedit: String::new(),
    };
    Box::into_raw(Box::new(engine))
}

/// # Safety
/// `engine` comes from `oh_key_ibus_engine_new` and is not used afterwards.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn oh_key_ibus_engine_free(engine: *mut OhKeyIbusEngine) {
    if !engine.is_null() {
        // SAFETY: see above.
        drop(unsafe { Box::from_raw(engine) });
    }
}

/// Reads the config files again if they changed; call it when a text field gets focus.
///
/// # Safety
/// `engine` is valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn oh_key_ibus_engine_reload(engine: *mut OhKeyIbusEngine) {
    // SAFETY: see above.
    let engine = unsafe { &mut *engine };
    engine.typing.reload_if_changed();
}

/// Handles one key event: `keysym`, and `state` as IBus gives it (modifier bits, release bit).
///
/// # Safety
/// `engine` is valid; `out` is writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn oh_key_ibus_engine_process_key(
    engine: *mut OhKeyIbusEngine,
    keysym: u32,
    state: u32,
    out: *mut OhKeyIbusReaction,
) {
    // SAFETY: see above.
    let engine = unsafe { &mut *engine };
    let reaction = engine.typing.handle_key(keysym, state);
    write_reaction(engine, reaction, out);
}

/// Focus left the field or the engine was reset: forget the word and hide the preedit.
///
/// # Safety
/// `engine` is valid; `out` is writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn oh_key_ibus_engine_abandon_word(
    engine: *mut OhKeyIbusEngine,
    out: *mut OhKeyIbusReaction,
) {
    // SAFETY: see above.
    let engine = unsafe { &mut *engine };
    let reaction = engine.typing.abandon_word();
    write_reaction(engine, reaction, out);
}

/// Nonzero while Vietnamese typing is on.
///
/// # Safety
/// `engine` is valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn oh_key_ibus_engine_is_vietnamese(engine: *const OhKeyIbusEngine) -> u8 {
    // SAFETY: see above.
    let engine = unsafe { &*engine };
    u8::from(engine.typing.is_vietnamese())
}
