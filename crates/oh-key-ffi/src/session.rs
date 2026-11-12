//! The typing session behind the C interface: the engine plus a record of the text already
//! put into the application for the word being composed. Pure, so it is tested on every system.
//!
//! It makes the same decisions as the Windows hook: after each key only the backspaces and the
//! changed tail are sent, and a key that adds nothing but itself passes through untouched.

use oh_key_engine::{Config, Engine, InputMethod, KeyResult};
use oh_key_ipc::{Settings, format_settings, parse_settings};
use oh_key_macro::MacroTable;

/// What the application has to do for one key.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Outcome {
    /// Characters to delete before the cursor.
    pub backspaces: usize,
    /// Text to insert after the deletion.
    pub text: String,
    /// The application must not handle the key itself; the edit stands in for it.
    pub consume: bool,
    /// The whole word as it stands now, for a host that edits with marked text instead.
    pub preedit: String,
    /// The complete text of a word that ended with this key (boundary included), empty otherwise.
    pub finished_text: String,
    /// A word is still being composed after this key.
    pub composing: bool,
}

pub struct TypingSession {
    settings: Settings,
    engine: Engine,
    sent: String,
}

/// The smallest edit from `sent` to `wanted`: (characters to delete, text to type).
fn plan_edit(sent: &str, wanted: &str) -> (usize, String) {
    let shared_characters = sent
        .chars()
        .zip(wanted.chars())
        .take_while(|(sent_character, wanted_character)| sent_character == wanted_character)
        .count();
    let backspaces = sent.chars().count() - shared_characters;
    let text = wanted.chars().skip(shared_characters).collect();
    (backspaces, text)
}

impl TypingSession {
    pub fn new(settings: Settings) -> Self {
        TypingSession {
            engine: Engine::new(settings.engine),
            settings,
            sent: String::new(),
        }
    }

    pub fn settings(&self) -> &Settings {
        &self.settings
    }

    pub fn load_settings_text(&mut self, text: &str) {
        self.settings = parse_settings(text);
        self.engine.set_config(self.settings.engine);
        self.reset();
    }

    pub fn settings_text(&self) -> String {
        format_settings(&self.settings)
    }

    pub fn set_config(&mut self, config: Config) {
        self.settings.engine = config;
        self.engine.set_config(config);
    }

    pub fn set_input_method(&mut self, input_method: InputMethod) {
        let mut config = self.settings.engine;
        config.input_method = input_method;
        self.set_config(config);
    }

    pub fn is_app_excluded(&self, application_name: &str) -> bool {
        let lowered = application_name.to_lowercase();
        self.settings.excluded_apps.contains(&lowered)
    }

    pub fn load_macros_text(&mut self, text: &str) {
        self.engine.clear_macros();
        let loaded = MacroTable::parse(text.as_bytes());
        for (shortcut, replacement) in loaded.table.items() {
            self.engine.add_macro(shortcut, replacement);
        }
    }

    pub fn reset(&mut self) {
        self.engine.reset();
        self.sent.clear();
    }

    pub fn process_key(&mut self, character: char) -> Outcome {
        let wanted = match self.engine.process_key(character) {
            KeyResult::Composing => self.engine.preedit().to_owned(),
            KeyResult::Commit(text) => text,
            KeyResult::PassThrough => {
                self.sent.clear();
                return self.outcome(0, String::new(), false);
            }
        };
        let (backspaces, text) = plan_edit(&self.sent, &wanted);
        let wanted_for_finish = wanted.clone();
        self.sent = if self.engine.is_composing() {
            wanted
        } else {
            String::new()
        };
        let just_the_key = backspaces == 0 && text.chars().eq([character]);
        let text = if just_the_key { String::new() } else { text };
        let mut outcome = self.outcome(backspaces, text, !just_the_key);
        if !outcome.composing {
            outcome.finished_text = wanted_for_finish;
        }
        outcome
    }

    pub fn backspace(&mut self) -> Outcome {
        if !self.engine.is_composing() {
            self.sent.clear();
            return self.outcome(0, String::new(), false);
        }
        self.engine.backspace();
        let wanted = self.engine.preedit().to_owned();
        let (backspaces, text) = plan_edit(&self.sent, &wanted);
        self.sent = wanted;
        self.outcome(backspaces, text, true)
    }

    /// The word ends without text of its own (Enter, Tab, arrows): put its final form in place
    /// and let the key through.
    pub fn finish_word(&mut self) -> Outcome {
        if !self.engine.is_composing() {
            self.reset();
            return self.outcome(0, String::new(), false);
        }
        let final_text = self.engine.commit();
        let (backspaces, text) = plan_edit(&self.sent, &final_text);
        self.sent.clear();
        let mut outcome = self.outcome(backspaces, text, false);
        outcome.finished_text = final_text;
        outcome
    }

    fn outcome(&self, backspaces: usize, text: String, consume: bool) -> Outcome {
        Outcome {
            backspaces,
            text,
            consume,
            preedit: self.sent.clone(),
            finished_text: String::new(),
            composing: self.engine.is_composing(),
        }
    }
}
