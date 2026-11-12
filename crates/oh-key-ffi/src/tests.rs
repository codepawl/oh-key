//! Keys in, text out, through the C interface, against a simulated text field.

use std::slice;

use super::*;

/// A text field that applies results the way the macOS input method does.
struct Field {
    session: *mut OhKeySession,
    text: String,
}

fn bytes_to_string(pointer: *const u8, length: usize) -> String {
    if length == 0 {
        return String::new();
    }
    // SAFETY: the session owns `length` bytes at `pointer` until its next call.
    let bytes = unsafe { slice::from_raw_parts(pointer, length) };
    String::from_utf8(bytes.to_vec()).expect("the engine only produces UTF-8")
}

fn empty_result() -> OhKeyResult {
    OhKeyResult {
        backspaces: 0,
        text: std::ptr::null(),
        text_length: 0,
        consume: 0,
        composing: 0,
        preedit: std::ptr::null(),
        preedit_length: 0,
        finished_text: std::ptr::null(),
        finished_text_length: 0,
    }
}

impl Field {
    fn new() -> Self {
        Field {
            session: oh_key_session_new(),
            text: String::new(),
        }
    }

    fn with_settings(settings_text: &str) -> Self {
        let field = Field::new();
        // SAFETY: the session is live and the text outlives the call.
        unsafe {
            oh_key_session_load_settings(
                field.session,
                settings_text.as_ptr().cast(),
                settings_text.len(),
            );
        }
        field
    }

    /// Applies one result: delete, insert, and the key itself when it was not consumed.
    fn apply(&mut self, result: &OhKeyResult, key: Option<char>) {
        for _ in 0..result.backspaces {
            self.text.pop();
        }
        self.text
            .push_str(&bytes_to_string(result.text, result.text_length));
        if result.consume == 0 {
            self.text.extend(key);
        }
    }

    fn press(&mut self, key: char) -> OhKeyResult {
        let mut result = empty_result();
        // SAFETY: the session is live and `result` is writable.
        unsafe { oh_key_session_process_key(self.session, key as u32, &mut result) };
        self.apply(&result, Some(key));
        result
    }

    fn type_keys(&mut self, keys: &str) {
        for key in keys.chars() {
            self.press(key);
        }
    }

    fn backspace(&mut self) -> OhKeyResult {
        let mut result = empty_result();
        // SAFETY: the session is live and `result` is writable.
        unsafe { oh_key_session_backspace(self.session, &mut result) };
        let was_consumed = result.consume != 0;
        self.apply(&result, None);
        if !was_consumed {
            self.text.pop();
        }
        result
    }

    fn finish_word_then_pass(&mut self, passed_key: char) {
        let mut result = empty_result();
        // SAFETY: the session is live and `result` is writable.
        unsafe { oh_key_session_finish_word(self.session, &mut result) };
        self.apply(&result, Some(passed_key));
    }

    fn typed(keys: &str) -> String {
        let mut field = Field::new();
        field.type_keys(keys);
        field.text.clone()
    }
}

impl Drop for Field {
    fn drop(&mut self) {
        // SAFETY: the session was created by `oh_key_session_new` and is not used again.
        unsafe { oh_key_session_free(self.session) };
    }
}

#[test]
fn telex_words_come_out_with_marks() {
    assert_eq!(Field::typed("Vieetj Nam "), "Việt Nam ");
    assert_eq!(Field::typed("xin chaof cacs banj "), "xin chào các bạn ");
    assert_eq!(Field::typed("dduwowcj "), "được ");
}

#[test]
fn the_field_always_shows_the_word_so_far() {
    let mut field = Field::new();
    for key in "nguwowif".chars() {
        let result = field.press(key);
        let preedit = bytes_to_string(result.preedit, result.preedit_length);
        assert_eq!(field.text, preedit, "after {key}");
    }
    assert_eq!(field.text, "người");
}

#[test]
fn a_plain_letter_is_not_consumed() {
    let mut field = Field::new();
    let result = field.press('b');
    assert_eq!(result.consume, 0);
    assert_eq!(result.backspaces, 0);
    assert_eq!(result.text_length, 0);
    assert_eq!(result.composing, 1);
}

#[test]
fn a_mark_replaces_only_the_changed_tail() {
    let mut field = Field::new();
    field.type_keys("to");
    let result = field.press('o');
    assert_eq!(result.consume, 1);
    assert_eq!(result.backspaces, 1);
    assert_eq!(bytes_to_string(result.text, result.text_length), "ô");
    assert_eq!(field.text, "tô");
}

#[test]
fn a_late_tone_moves_to_the_right_vowel() {
    let mut field = Field::new();
    field.type_keys("hoa");
    let result = field.press('f');
    assert_eq!(field.text, "hoà");
    assert_eq!(result.backspaces, 1);
    assert_eq!(bytes_to_string(result.text, result.text_length), "à");
}

#[test]
fn the_word_break_commits_and_reports_the_finished_word() {
    let mut field = Field::new();
    field.type_keys("vieejt");
    let result = field.press(' ');
    assert_eq!(field.text, "việt ");
    assert_eq!(result.composing, 0);
    assert_eq!(
        bytes_to_string(result.finished_text, result.finished_text_length),
        "việt "
    );
}

#[test]
fn english_words_are_given_back_as_typed() {
    assert_eq!(Field::typed("text "), "text ");
    assert_eq!(Field::typed("window "), "window ");
    assert_eq!(Field::typed("github "), "github ");
    assert_eq!(Field::typed("fix the bug "), "fix the bug ");
}

#[test]
fn restore_can_be_switched_off() {
    let mut field = Field::with_settings("restore_invalid_word = false\n");
    field.type_keys("text ");
    assert_ne!(field.text, "text ");
}

#[test]
fn pressing_a_mark_key_twice_types_the_letter() {
    assert_eq!(Field::typed("ddd "), "dd ");
    assert_eq!(Field::typed("aaa "), "aa ");
}

#[test]
fn vni_comes_from_the_settings() {
    let mut field = Field::with_settings("input_method = \"vni\"\n");
    field.type_keys("Vie6t5 Nam ");
    assert_eq!(field.text, "Việt Nam ");
}

#[test]
fn the_input_method_can_be_changed_through_the_c_interface() {
    let mut field = Field::new();
    // SAFETY: the session is live.
    unsafe { oh_key_session_set_input_method(field.session, OH_KEY_INPUT_VNI) };
    // SAFETY: the session is live.
    let code = unsafe { oh_key_session_input_method(field.session) };
    assert_eq!(code, OH_KEY_INPUT_VNI);
    field.type_keys("a1 ");
    assert_eq!(field.text, "á ");
}

#[test]
fn backspace_edits_the_word_and_then_passes_through() {
    let mut field = Field::new();
    field.type_keys("tooi");
    assert_eq!(field.text, "tôi");
    let result = field.backspace();
    assert_eq!(result.consume, 1);
    assert_eq!(field.text, "tô");
    field.backspace();
    field.backspace();
    assert_eq!(field.text, "");
    field.type_keys("a ");
    let result = field.backspace();
    assert_eq!(result.consume, 0);
    assert_eq!(field.text, "a");
}

#[test]
fn finishing_a_word_keeps_it_and_lets_the_key_through() {
    let mut field = Field::new();
    field.type_keys("vieejt");
    field.finish_word_then_pass('\n');
    assert_eq!(field.text, "việt\n");
}

#[test]
fn finishing_with_nothing_composing_changes_nothing() {
    let mut field = Field::new();
    let mut result = empty_result();
    // SAFETY: the session is live and `result` is writable.
    unsafe { oh_key_session_finish_word(field.session, &mut result) };
    assert_eq!(
        (result.backspaces, result.text_length, result.consume),
        (0, 0, 0)
    );
    field.finish_word_then_pass('\t');
    assert_eq!(field.text, "\t");
}

#[test]
fn reset_forgets_the_word_without_touching_the_text() {
    let mut field = Field::new();
    field.type_keys("vie");
    // SAFETY: the session is live.
    unsafe { oh_key_session_reset(field.session) };
    field.type_keys("e");
    assert_eq!(field.text, "viee");
}

#[test]
fn macros_expand_when_enabled() {
    let mut field = Field::with_settings(
        "macro_enabled = true
",
    );
    let mut table = oh_key_macro::MacroTable::new();
    table.insert("vn", "Việt Nam").expect("a short macro fits");
    let macros = table.serialize();
    // SAFETY: the session is live and the text outlives the call.
    unsafe { oh_key_session_load_macros(field.session, macros.as_ptr().cast(), macros.len()) };
    field.type_keys("vn ");
    assert_eq!(field.text, "Việt Nam ");
}

#[test]
fn settings_round_trip_through_the_c_interface() {
    let field =
        Field::with_settings("input_method = \"vni\"\nexcluded_apps = \"com.apple.Terminal\"\n");
    let mut text: *const u8 = std::ptr::null();
    let mut length = 0usize;
    // SAFETY: the session is live and both outputs are writable.
    unsafe { oh_key_session_settings_text(field.session, &mut text, &mut length) };
    let written = bytes_to_string(text, length);
    assert!(written.contains("input_method = \"vni\""));
    let name = "com.apple.terminal";
    // SAFETY: the session is live and the name outlives the call.
    let excluded =
        unsafe { oh_key_session_is_app_excluded(field.session, name.as_ptr().cast(), name.len()) };
    assert_eq!(excluded, 1);
    // SAFETY: the session is live.
    assert_eq!(
        unsafe { oh_key_session_starts_vietnamese(field.session) },
        1
    );
}

#[test]
fn a_value_that_is_not_a_character_ends_the_word() {
    let mut field = Field::new();
    field.type_keys("vieejt");
    let mut result = empty_result();
    // SAFETY: the session is live and `result` is writable.
    unsafe { oh_key_session_process_key(field.session, 0xD800, &mut result) };
    assert_eq!(result.composing, 0);
}
