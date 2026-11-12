//! C interface to the oh-key typing engine, for the macOS input method (and any other host).
//!
//! A host creates one session per text field, feeds it keys and applies the result: delete
//! `backspaces` characters before the cursor, insert `text`, and swallow the key when `consume`
//! is set. Strings in a result are UTF-8, not NUL terminated, and owned by the session: they
//! stay valid until the next call on the same session.
//!
//! The settings use the `config.toml` text of the Windows app, so one file format serves both.

mod session;

use std::ffi::c_char;
use std::slice;

use oh_key_engine::InputMethod;
use oh_key_ipc::{Settings, ToggleHotkey};
use session::{Outcome, TypingSession};

pub const OH_KEY_INPUT_TELEX: u32 = 0;
pub const OH_KEY_INPUT_VNI: u32 = 1;
pub const OH_KEY_INPUT_VIQR: u32 = 2;
pub const OH_KEY_INPUT_SIMPLE_TELEX: u32 = 3;

pub const OH_KEY_HOTKEY_CTRL_SHIFT: u32 = 0;
pub const OH_KEY_HOTKEY_ALT_Z: u32 = 1;
pub const OH_KEY_HOTKEY_DISABLED: u32 = 2;

/// The session behind the opaque pointer the host holds.
pub struct OhKeySession {
    typing: TypingSession,
    outcome: Outcome,
    settings_text: String,
}

#[repr(C)]
pub struct OhKeyResult {
    pub backspaces: u32,
    pub text: *const u8,
    pub text_length: usize,
    /// Nonzero: the application must not handle the key.
    pub consume: u8,
    /// Nonzero: a word is still being composed.
    pub composing: u8,
    /// The whole word as it stands, for a host that uses marked text.
    pub preedit: *const u8,
    pub preedit_length: usize,
    /// The complete text of a word that ended with this key (boundary included).
    pub finished_text: *const u8,
    pub finished_text_length: usize,
}

fn input_method_from_code(code: u32) -> Option<InputMethod> {
    match code {
        OH_KEY_INPUT_TELEX => Some(InputMethod::Telex),
        OH_KEY_INPUT_VNI => Some(InputMethod::Vni),
        OH_KEY_INPUT_VIQR => Some(InputMethod::Viqr),
        OH_KEY_INPUT_SIMPLE_TELEX => Some(InputMethod::SimpleTelex),
        _ => None,
    }
}

fn input_method_code(input_method: InputMethod) -> u32 {
    match input_method {
        InputMethod::Telex => OH_KEY_INPUT_TELEX,
        InputMethod::Vni => OH_KEY_INPUT_VNI,
        InputMethod::Viqr => OH_KEY_INPUT_VIQR,
        InputMethod::SimpleTelex => OH_KEY_INPUT_SIMPLE_TELEX,
    }
}

fn write_result(session: &mut OhKeySession, outcome: Outcome, result: *mut OhKeyResult) {
    session.outcome = outcome;
    let outcome = &session.outcome;
    let filled = OhKeyResult {
        backspaces: outcome.backspaces as u32,
        text: outcome.text.as_ptr(),
        text_length: outcome.text.len(),
        consume: u8::from(outcome.consume),
        composing: u8::from(outcome.composing),
        preedit: outcome.preedit.as_ptr(),
        preedit_length: outcome.preedit.len(),
        finished_text: outcome.finished_text.as_ptr(),
        finished_text_length: outcome.finished_text.len(),
    };
    if !result.is_null() {
        // SAFETY: the caller passes a pointer to a writable OhKeyResult (checked non-null).
        unsafe { result.write(filled) };
    }
}

/// Bytes of a host string; invalid UTF-8 is replaced rather than rejected.
///
/// # Safety
/// `text` has `length` readable bytes, or is null.
unsafe fn host_text(text: *const c_char, length: usize) -> String {
    if text.is_null() || length == 0 {
        return String::new();
    }
    // SAFETY: the caller guarantees `length` readable bytes at `text`.
    let bytes = unsafe { slice::from_raw_parts(text.cast::<u8>(), length) };
    String::from_utf8_lossy(bytes).into_owned()
}

/// Creates a session with the default settings (Telex, Vietnamese on). Free it with
/// `oh_key_session_free`.
#[unsafe(no_mangle)]
pub extern "C" fn oh_key_session_new() -> *mut OhKeySession {
    let session = OhKeySession {
        typing: TypingSession::new(Settings::default()),
        outcome: Outcome::default(),
        settings_text: String::new(),
    };
    Box::into_raw(Box::new(session))
}

/// # Safety
/// `session` comes from `oh_key_session_new` and is not used afterwards.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn oh_key_session_free(session: *mut OhKeySession) {
    if !session.is_null() {
        // SAFETY: see above.
        drop(unsafe { Box::from_raw(session) });
    }
}

/// Replaces the settings with the parsed `config.toml` text and forgets the current word.
///
/// # Safety
/// `session` is valid; `text` has `length` readable bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn oh_key_session_load_settings(
    session: *mut OhKeySession,
    text: *const c_char,
    length: usize,
) {
    // SAFETY: see above.
    let (session, text) = unsafe { (&mut *session, host_text(text, length)) };
    session.typing.load_settings_text(&text);
}

/// Replaces the macro table with the contents of `macros.txt`.
///
/// # Safety
/// `session` is valid; `text` has `length` readable bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn oh_key_session_load_macros(
    session: *mut OhKeySession,
    text: *const c_char,
    length: usize,
) {
    // SAFETY: see above.
    let (session, text) = unsafe { (&mut *session, host_text(text, length)) };
    session.typing.load_macros_text(&text);
}

/// Writes the current settings as `config.toml` text. The text stays valid until the next call
/// on this session.
///
/// # Safety
/// `session` is valid; `text` and `length` are writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn oh_key_session_settings_text(
    session: *mut OhKeySession,
    text: *mut *const u8,
    length: *mut usize,
) {
    // SAFETY: see above.
    let session = unsafe { &mut *session };
    session.settings_text = session.typing.settings_text();
    // SAFETY: see above.
    unsafe {
        text.write(session.settings_text.as_ptr());
        length.write(session.settings_text.len());
    }
}

/// Selects Telex, VNI, VIQR or simple Telex; an unknown code changes nothing.
///
/// # Safety
/// `session` is valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn oh_key_session_set_input_method(session: *mut OhKeySession, code: u32) {
    // SAFETY: see above.
    let session = unsafe { &mut *session };
    if let Some(input_method) = input_method_from_code(code) {
        session.typing.set_input_method(input_method);
    }
}

/// # Safety
/// `session` is valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn oh_key_session_input_method(session: *const OhKeySession) -> u32 {
    // SAFETY: see above.
    let session = unsafe { &*session };
    input_method_code(session.typing.settings().engine.input_method)
}

/// Whether the application (its lower case bundle identifier) is on the "oh-key off" list.
///
/// # Safety
/// `session` is valid; `name` has `length` readable bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn oh_key_session_is_app_excluded(
    session: *const OhKeySession,
    name: *const c_char,
    length: usize,
) -> u8 {
    // SAFETY: see above.
    let (session, name) = unsafe { (&*session, host_text(name, length)) };
    u8::from(session.typing.is_app_excluded(&name))
}

/// Whether oh-key should start in Vietnamese mode, from the settings.
///
/// # Safety
/// `session` is valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn oh_key_session_starts_vietnamese(session: *const OhKeySession) -> u8 {
    // SAFETY: see above.
    let session = unsafe { &*session };
    u8::from(session.typing.settings().start_vietnamese)
}

/// Which key combination toggles Vietnamese and English: an `OH_KEY_HOTKEY_*` code.
///
/// # Safety
/// `session` is valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn oh_key_session_toggle_hotkey(session: *const OhKeySession) -> u32 {
    // SAFETY: see above.
    let session = unsafe { &*session };
    match session.typing.settings().toggle_hotkey {
        ToggleHotkey::CtrlShift => OH_KEY_HOTKEY_CTRL_SHIFT,
        ToggleHotkey::AltZ => OH_KEY_HOTKEY_ALT_Z,
        ToggleHotkey::Disabled => OH_KEY_HOTKEY_DISABLED,
    }
}

/// Feeds one printable character (a Unicode scalar value). A value that is not a scalar value
/// is treated as a word break.
///
/// # Safety
/// `session` is valid; `result` is writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn oh_key_session_process_key(
    session: *mut OhKeySession,
    character: u32,
    result: *mut OhKeyResult,
) {
    // SAFETY: see above.
    let session = unsafe { &mut *session };
    let outcome = match char::from_u32(character) {
        Some(character) => session.typing.process_key(character),
        None => session.typing.finish_word(),
    };
    write_result(session, outcome, result);
}

/// Backspace: shortens the word. When nothing is composing, the key is not consumed.
///
/// # Safety
/// `session` is valid; `result` is writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn oh_key_session_backspace(
    session: *mut OhKeySession,
    result: *mut OhKeyResult,
) {
    // SAFETY: see above.
    let session = unsafe { &mut *session };
    let outcome = session.typing.backspace();
    write_result(session, outcome, result);
}

/// Enter, Tab, an arrow or a click: finish the word in place; the key itself is not consumed.
///
/// # Safety
/// `session` is valid; `result` is writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn oh_key_session_finish_word(
    session: *mut OhKeySession,
    result: *mut OhKeyResult,
) {
    // SAFETY: see above.
    let session = unsafe { &mut *session };
    let outcome = session.typing.finish_word();
    write_result(session, outcome, result);
}

/// A shortcut or focus change: forget the word and leave the text as it is.
///
/// # Safety
/// `session` is valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn oh_key_session_reset(session: *mut OhKeySession) {
    // SAFETY: see above.
    let session = unsafe { &mut *session };
    session.typing.reset();
}

#[cfg(test)]
mod tests;
