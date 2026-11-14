//! The IBus engine's key handling: one key event in, what IBus must commit or show as preedit out.
//!
//! The word being typed lives in the preedit, which every toolkit, browser and terminal that talks
//! to IBus understands; the finished word is committed when it ends. Same decisions as the macOS
//! frontend's marked text path.

use std::path::PathBuf;

use oh_key_ffi::session::{Outcome, TypingSession};
use oh_key_ipc::{Settings, ToggleHotkey};

use crate::config::{ConfigFiles, ConfigText};
use crate::keys::{
    KeyKind, ModifierKey, STATE_ALT, STATE_CONTROL, STATE_RELEASE, STATE_SHIFT, classify_keysym,
    is_shortcut,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PreeditChange {
    /// Leave the preedit as it is.
    Keep,
    /// Show this word, underlined, at the cursor.
    Show(String),
    Hide,
}

/// What IBus has to do for one key event. `commit` is applied before the key is handled further.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Reaction {
    /// The key is used up: IBus must not forward it to the application.
    pub handled: bool,
    /// Text to commit, empty for none.
    pub commit: String,
    pub preedit: PreeditChange,
}

impl Reaction {
    fn pass() -> Self {
        Reaction {
            handled: false,
            commit: String::new(),
            preedit: PreeditChange::Keep,
        }
    }

    fn handled() -> Self {
        Reaction {
            handled: true,
            ..Reaction::pass()
        }
    }
}

pub struct KeyboardEngine {
    session: TypingSession,
    vietnamese: bool,
    /// The word as the preedit shows it now; empty when no preedit is visible.
    shown_word: String,
    /// Both Ctrl and Shift are down with no other key between them: a release toggles the mode.
    chord_pending: bool,
    files: Option<ConfigFiles>,
    first_load_done: bool,
}

impl KeyboardEngine {
    /// `config_folder` is where `config.toml` and `macros.txt` live; `None` keeps the defaults.
    pub fn new(config_folder: Option<PathBuf>) -> Self {
        let mut engine = KeyboardEngine {
            session: TypingSession::new(Settings::default()),
            vietnamese: true,
            shown_word: String::new(),
            chord_pending: false,
            files: config_folder.map(ConfigFiles::new),
            first_load_done: false,
        };
        engine.reload_if_changed();
        engine
    }

    /// Reads the config files again when one changed. A word being typed is dropped.
    pub fn reload_if_changed(&mut self) {
        let Some(files) = self.files.as_mut() else {
            return;
        };
        let Some(ConfigText { settings, macros }) = files.changed_text() else {
            return;
        };
        self.session.load_settings_text(&settings);
        self.session.load_macros_text(&macros);
        self.shown_word.clear();
        if !self.first_load_done {
            self.vietnamese = self.session.settings().start_vietnamese;
            self.first_load_done = true;
        }
    }

    pub fn is_vietnamese(&self) -> bool {
        self.vietnamese
    }

    /// Focus left the field or IBus reset the engine: forget the word, hide the preedit.
    pub fn abandon_word(&mut self) -> Reaction {
        self.session.reset();
        self.chord_pending = false;
        let had_preedit = !self.shown_word.is_empty();
        self.shown_word.clear();
        Reaction {
            preedit: if had_preedit {
                PreeditChange::Hide
            } else {
                PreeditChange::Keep
            },
            ..Reaction::pass()
        }
    }

    pub fn handle_key(&mut self, keysym: u32, state: u32) -> Reaction {
        let kind = classify_keysym(keysym);
        if state & STATE_RELEASE != 0 {
            return self.handle_release(kind);
        }
        if let KeyKind::Modifier(modifier) = kind {
            self.note_modifier_press(modifier, state);
            return Reaction::pass();
        }
        self.chord_pending = false;
        if self.is_alt_z_toggle(kind, state) {
            let mut reaction = self.toggle_mode();
            reaction.handled = true;
            return reaction;
        }
        if !self.vietnamese {
            return Reaction::pass();
        }
        if is_shortcut(state) {
            let outcome = self.session.finish_word();
            return self.end_word_without_text(outcome);
        }
        match kind {
            KeyKind::Printable(character) => {
                let outcome = self.session.process_key(character);
                self.apply_outcome(outcome)
            }
            KeyKind::Backspace => {
                let outcome = self.session.backspace();
                self.apply_outcome(outcome)
            }
            KeyKind::WordBreak | KeyKind::Modifier(_) => {
                let outcome = self.session.finish_word();
                self.end_word_without_text(outcome)
            }
        }
    }

    fn hotkey(&self) -> ToggleHotkey {
        self.session.settings().toggle_hotkey
    }

    fn is_alt_z_toggle(&self, kind: KeyKind, state: u32) -> bool {
        let is_z = matches!(kind, KeyKind::Printable('z' | 'Z'));
        self.hotkey() == ToggleHotkey::AltZ && is_z && state & STATE_ALT != 0
    }

    fn note_modifier_press(&mut self, modifier: ModifierKey, state: u32) {
        let completes_chord = match modifier {
            ModifierKey::Shift => state & STATE_CONTROL != 0,
            ModifierKey::Control => state & STATE_SHIFT != 0,
            ModifierKey::Other => false,
        };
        self.chord_pending = completes_chord && self.hotkey() == ToggleHotkey::CtrlShift;
    }

    fn handle_release(&mut self, kind: KeyKind) -> Reaction {
        let is_chord_key = matches!(
            kind,
            KeyKind::Modifier(ModifierKey::Shift | ModifierKey::Control)
        );
        if is_chord_key && self.chord_pending {
            self.chord_pending = false;
            return self.toggle_mode();
        }
        Reaction::pass()
    }

    /// Switches Vietnamese and English; the word in progress is committed as it stands.
    fn toggle_mode(&mut self) -> Reaction {
        let outcome = self.session.finish_word();
        let reaction = self.end_word_without_text(outcome);
        self.vietnamese = !self.vietnamese;
        reaction
    }

    /// Enter, Tab, arrows and shortcuts: commit the word in its final form and pass the key on.
    fn end_word_without_text(&mut self, outcome: Outcome) -> Reaction {
        let previous_word = std::mem::take(&mut self.shown_word);
        if previous_word.is_empty() && outcome.finished_text.is_empty() {
            return Reaction::pass();
        }
        let commit = if outcome.finished_text.is_empty() {
            previous_word
        } else {
            outcome.finished_text
        };
        Reaction {
            handled: false,
            commit,
            preedit: PreeditChange::Hide,
        }
    }

    fn apply_outcome(&mut self, outcome: Outcome) -> Reaction {
        if outcome.composing {
            self.shown_word = outcome.preedit.clone();
            return Reaction {
                preedit: PreeditChange::Show(outcome.preedit),
                ..Reaction::handled()
            };
        }
        let previous_word = std::mem::take(&mut self.shown_word);
        if !outcome.finished_text.is_empty() {
            return Reaction {
                commit: outcome.finished_text,
                preedit: PreeditChange::Hide,
                ..Reaction::handled()
            };
        }
        if previous_word.is_empty() {
            return Reaction {
                handled: outcome.consume,
                ..Reaction::pass()
            };
        }
        if outcome.consume {
            // Backspace erased the whole word: drop the preedit.
            return Reaction {
                preedit: PreeditChange::Hide,
                ..Reaction::handled()
            };
        }
        // A pass-through key ends the word: commit it, then let the key reach the application.
        Reaction {
            handled: false,
            commit: previous_word,
            preedit: PreeditChange::Hide,
        }
    }
}

#[cfg(test)]
mod tests;
