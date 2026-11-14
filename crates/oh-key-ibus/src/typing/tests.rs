//! Keys in, commit and preedit out, against a simulated text field.

use std::sync::atomic::{AtomicUsize, Ordering};

use super::*;
use crate::keys::{STATE_ALT, STATE_CONTROL, STATE_RELEASE, STATE_SHIFT};

const KEYSYM_ENTER: u32 = 0xff0d;
const KEYSYM_BACKSPACE: u32 = 0xff08;
const KEYSYM_SHIFT: u32 = 0xffe1;
const KEYSYM_CONTROL: u32 = 0xffe3;

/// What an application shows: committed text plus the underlined preedit after it.
struct Field {
    engine: KeyboardEngine,
    committed: String,
    preedit: String,
}

impl Field {
    fn new() -> Self {
        Field::with_engine(KeyboardEngine::new(None))
    }

    fn with_settings(settings_text: &str) -> Self {
        static NEXT_FOLDER: AtomicUsize = AtomicUsize::new(0);
        let folder = std::env::temp_dir().join(format!(
            "oh-key-ibus-test-{}-{}",
            std::process::id(),
            NEXT_FOLDER.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(folder.join("config.toml"), settings_text).unwrap();
        let engine = KeyboardEngine::new(Some(folder.clone()));
        std::fs::remove_dir_all(&folder).unwrap();
        Field::with_engine(engine)
    }

    fn with_engine(engine: KeyboardEngine) -> Self {
        Field {
            engine,
            committed: String::new(),
            preedit: String::new(),
        }
    }

    fn apply(&mut self, reaction: &Reaction) {
        self.committed.push_str(&reaction.commit);
        match &reaction.preedit {
            PreeditChange::Keep => {}
            PreeditChange::Show(word) => self.preedit = word.clone(),
            PreeditChange::Hide => self.preedit.clear(),
        }
    }

    /// Presses a key; a key the engine does not handle reaches the application.
    fn press(&mut self, keysym: u32, state: u32) -> Reaction {
        let reaction = self.engine.handle_key(keysym, state);
        self.apply(&reaction);
        if !reaction.handled {
            match classify_keysym(keysym) {
                KeyKind::Printable(character) => self.committed.push(character),
                KeyKind::WordBreak if keysym == KEYSYM_ENTER => self.committed.push('\n'),
                _ => {}
            }
        }
        reaction
    }

    fn release(&mut self, keysym: u32, state: u32) {
        let reaction = self.engine.handle_key(keysym, state | STATE_RELEASE);
        self.apply(&reaction);
    }

    fn type_text(&mut self, text: &str) {
        for character in text.chars() {
            self.press(character as u32, 0);
        }
    }

    fn shown(&self) -> String {
        format!("{}{}", self.committed, self.preedit)
    }
}

#[test]
fn telex_word_stays_in_the_preedit_until_the_space() {
    let mut field = Field::new();
    field.type_text("vieetj");
    assert_eq!(field.preedit, "việt");
    assert_eq!(field.committed, "");
    field.type_text(" ");
    assert_eq!(field.committed, "việt ");
    assert_eq!(field.preedit, "");
}

#[test]
fn a_sentence_comes_out_with_its_tones() {
    let mut field = Field::new();
    field.type_text("Tooi yeeu Vieetj Nam ");
    assert_eq!(field.shown(), "Tôi yêu Việt Nam ");
}

#[test]
fn backspace_shortens_the_word_and_finally_clears_it() {
    let mut field = Field::new();
    field.type_text("vieet");
    field.press(KEYSYM_BACKSPACE, 0);
    assert_eq!(field.preedit, "viê");
    for _ in 0..3 {
        field.press(KEYSYM_BACKSPACE, 0);
    }
    assert_eq!(field.shown(), "");
}

#[test]
fn backspace_with_no_word_goes_to_the_application() {
    let mut field = Field::new();
    let reaction = field.press(KEYSYM_BACKSPACE, 0);
    assert!(!reaction.handled);
}

#[test]
fn enter_commits_the_word_and_passes_through() {
    let mut field = Field::new();
    field.type_text("tooi");
    let reaction = field.press(KEYSYM_ENTER, 0);
    assert!(!reaction.handled);
    assert_eq!(field.shown(), "tôi\n");
}

#[test]
fn a_shortcut_commits_the_word_and_passes_through() {
    let mut field = Field::new();
    field.type_text("tooi");
    let reaction = field.press('c' as u32, STATE_CONTROL);
    assert!(!reaction.handled);
    assert_eq!(field.committed, "tôic");
    assert_eq!(field.preedit, "");
}

#[test]
fn key_releases_are_never_handled() {
    let mut field = Field::new();
    field.type_text("a");
    let reaction = field.engine.handle_key('a' as u32, STATE_RELEASE);
    assert!(!reaction.handled);
}

#[test]
fn vni_comes_from_the_config_file() {
    let mut field = Field::with_settings("input_method = \"vni\"\n");
    field.type_text("Vie6t5 ");
    assert_eq!(field.shown(), "Việt ");
}

#[test]
fn english_mode_passes_every_key_through() {
    let mut field = Field::with_settings("start_vietnamese = false\n");
    field.type_text("viet");
    assert_eq!(field.shown(), "viet");
    assert!(!field.engine.is_vietnamese());
}

#[test]
fn alt_z_toggles_when_it_is_the_hotkey() {
    let mut field = Field::with_settings("toggle_hotkey = \"alt+z\"\n");
    let toggle = field.press('z' as u32, STATE_ALT);
    assert!(toggle.handled);
    field.type_text("vieetj");
    assert_eq!(field.shown(), "vieetj");
    field.press('z' as u32, STATE_ALT);
    field.type_text(" vieetj");
    assert_eq!(field.shown(), "vieetj việt");
}

#[test]
fn toggling_in_the_middle_of_a_word_commits_it() {
    let mut field = Field::with_settings("toggle_hotkey = \"alt+z\"\n");
    field.type_text("tooi");
    field.press('z' as u32, STATE_ALT);
    assert_eq!(field.committed, "tôi");
    assert_eq!(field.preedit, "");
}

#[test]
fn ctrl_shift_toggles_on_release_when_no_key_came_between() {
    let mut field = Field::new();
    field.press(KEYSYM_CONTROL, 0);
    field.press(KEYSYM_SHIFT, STATE_CONTROL);
    assert!(field.engine.is_vietnamese());
    field.release(KEYSYM_SHIFT, STATE_CONTROL | STATE_SHIFT);
    assert!(!field.engine.is_vietnamese());
    field.release(KEYSYM_CONTROL, STATE_CONTROL);
    assert!(!field.engine.is_vietnamese());
}

#[test]
fn ctrl_shift_with_a_key_between_does_not_toggle() {
    let mut field = Field::new();
    field.press(KEYSYM_CONTROL, 0);
    field.press(KEYSYM_SHIFT, STATE_CONTROL);
    field.press('t' as u32, STATE_CONTROL | STATE_SHIFT);
    field.release(KEYSYM_SHIFT, STATE_CONTROL | STATE_SHIFT);
    assert!(field.engine.is_vietnamese());
}

#[test]
fn losing_focus_hides_the_preedit_and_forgets_the_word() {
    let mut field = Field::new();
    field.type_text("vie");
    let reaction = field.engine.abandon_word();
    field.apply(&reaction);
    assert_eq!(field.preedit, "");
    field.type_text("e");
    assert_eq!(field.preedit, "e");
}

#[test]
fn the_config_file_is_read_again_when_it_changes() {
    let folder = std::env::temp_dir().join(format!("oh-key-ibus-reload-{}", std::process::id()));
    std::fs::create_dir_all(&folder).unwrap();
    let mut field = Field::with_engine(KeyboardEngine::new(Some(folder.clone())));
    field.type_text("aa ");
    assert_eq!(field.shown(), "â ");
    std::fs::write(folder.join("config.toml"), "input_method = \"vni\"\n").unwrap();
    field.engine.reload_if_changed();
    field.type_text("a6 ");
    assert_eq!(field.shown(), "â â ");
    std::fs::remove_dir_all(&folder).unwrap();
}
