//! oh-key typing engine: turns keystrokes into Vietnamese text.
//!
//! The engine keeps the word being composed as letters plus one tone and
//! derives the displayed text from them after every key, so a tone can follow
//! its vowel in any order and moves by itself when the vowels change. The
//! engine never touches the operating system: a frontend feeds keys in and
//! shows `preedit()` or the committed text.

#![forbid(unsafe_code)]

mod english;
mod letter;
mod method;
mod tables;
mod undo_idioms;
mod word;

use letter::{Letter, Mark};
use method::{Action, HookKind, action_for};
use word::{Validity, Word};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum InputMethod {
    Telex,
    Vni,
    Viqr,
    /// Telex without the `w` shortcut for ư and without the bracket keys.
    SimpleTelex,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ToneStyle {
    /// The tone goes on the second vowel of oa, oe, uy: "hoà", "thuý".
    Modern,
    /// The tone goes on the first vowel of an open syllable: "hòa", "thúy".
    Classic,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Config {
    pub input_method: InputMethod,
    pub tone_style: ToneStyle,
    /// Marks may be typed after the letters they belong to ("hoaf", "dod" -> "đo").
    pub free_tone_placement: bool,
    pub spell_check: bool,
    /// Give back the typed keys when the word is not Vietnamese ("text").
    pub restore_invalid_word: bool,
    pub macro_enabled: bool,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            input_method: InputMethod::Telex,
            tone_style: ToneStyle::Modern,
            free_tone_placement: true,
            spell_check: true,
            restore_invalid_word: true,
            macro_enabled: false,
        }
    }
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum KeyResult {
    /// The composing word changed; show `preedit()`.
    Composing,
    /// A word boundary was reached: commit this text, boundary included.
    Commit(String),
    /// The engine did not use the key and nothing is composing.
    PassThrough,
}

/// A word never grows past this; a longer run is committed as it is.
const MAX_WORD_LETTERS: usize = 48;
const LONGEST_ENGLISH_WORD: usize = 24;
/// Bytes kept free after a committed word for the key that ended it.
const BOUNDARY_ROOM: usize = 4;
/// Backspacing over more boundary keys than this ("word.!?…") no longer reaches the word.
const MAX_BOUNDARIES_AFTER_WORD: u8 = 8;

/// A copy of up to `MAX_WORD_LETTERS` keys on the stack, so the word can be rebuilt from them
/// without allocating.
fn copy_keys(keys: &[char]) -> ([char; MAX_WORD_LETTERS], usize) {
    let mut copy = ['\0'; MAX_WORD_LETTERS];
    let count = keys.len().min(MAX_WORD_LETTERS);
    copy[..count].copy_from_slice(&keys[..count]);
    (copy, count)
}

/// Asks an English word rule about these letters, in any case. Works on the stack: this runs
/// when a word ends.
fn english_rule_holds(letters: impl Iterator<Item = char>, rule: fn(&[u8]) -> bool) -> bool {
    let mut lowercase = [0u8; LONGEST_ENGLISH_WORD];
    let mut count = 0;
    for letter in letters {
        if count == LONGEST_ENGLISH_WORD || !letter.is_ascii_alphabetic() {
            return false;
        }
        lowercase[count] = letter.to_ascii_lowercase() as u8;
        count += 1;
    }
    rule(&lowercase[..count])
}

pub struct Engine {
    config: Config,
    word: Word,
    /// Every key typed into the current word, used to restore it.
    raw_keys: Vec<char>,
    preedit: String,
    /// The word is not Vietnamese: later letters are typed as they are.
    frozen: bool,
    /// Some key already changed the word, so restoring it can matter.
    transformed: bool,
    /// A mark key was pressed twice and its second press typed the letter.
    key_undone: bool,
    /// Index in `raw_keys` of a tone key pressed twice to type its letter ("miss"): a later
    /// letter means the word is foreign ("missa"), so it is given back as typed.
    tone_undo_key_index: Option<usize>,
    /// A mark was undone by a key typed apart from the one that set it ("posts", "seemed"):
    /// nobody undoes a mark that way, so the word is foreign and is given back as typed.
    undo_apart: bool,
    escape_pending: bool,
    /// Key that produced a fixed letter on the previous press (ww, ]]).
    last_mapped_key: Option<char>,
    /// The last word boundary was a symbol or digit, not a space: `a[0]`, `){`, `#ff`.
    after_symbol: bool,
    /// An escape key was used in the word, so its raw keys no longer replay to what is shown.
    escaped: bool,
    /// A backspace left the word showing exactly its typed keys (a word given back as typed):
    /// the next key that edits it first rebuilds the word from those keys, so a tone typed
    /// after backspacing a wrong key still lands ("vieetk", backspace, "s" gives "viết").
    replay_pending: bool,
    /// The last word finished by a boundary key, kept so that backspacing over the boundary
    /// picks it back up: its keys, its text without the boundary, and `after_symbol` at its start.
    previous_word_keys: Vec<char>,
    previous_word_text: String,
    previous_word_after_symbol: bool,
    /// Boundary keys typed since the previous word; 0 when there is no word to pick up.
    boundaries_after_previous_word: u8,
    macros: Vec<(String, String)>,
}

impl Default for Engine {
    fn default() -> Self {
        Engine::new(Config::default())
    }
}

impl Engine {
    pub fn new(config: Config) -> Self {
        Engine {
            config,
            word: Word::default(),
            raw_keys: Vec::with_capacity(MAX_WORD_LETTERS),
            preedit: String::with_capacity(MAX_WORD_LETTERS * 3),
            frozen: false,
            transformed: false,
            key_undone: false,
            tone_undo_key_index: None,
            undo_apart: false,
            escape_pending: false,
            last_mapped_key: None,
            after_symbol: false,
            escaped: false,
            replay_pending: false,
            previous_word_keys: Vec::with_capacity(MAX_WORD_LETTERS),
            previous_word_text: String::with_capacity(MAX_WORD_LETTERS * 3),
            previous_word_after_symbol: false,
            boundaries_after_previous_word: 0,
            macros: Vec::new(),
        }
    }

    pub fn set_config(&mut self, config: Config) {
        self.config = config;
        self.refresh_preedit();
    }

    pub fn preedit(&self) -> &str {
        &self.preedit
    }

    pub fn is_composing(&self) -> bool {
        !self.word.letters.is_empty()
    }

    pub fn reset(&mut self) {
        self.word.clear();
        self.raw_keys.clear();
        self.preedit.clear();
        self.frozen = false;
        self.transformed = false;
        self.key_undone = false;
        self.tone_undo_key_index = None;
        self.undo_apart = false;
        self.escape_pending = false;
        self.last_mapped_key = None;
        self.after_symbol = false;
        self.escaped = false;
        self.replay_pending = false;
        self.boundaries_after_previous_word = 0;
    }

    /// Returns the composing word and clears it.
    pub fn commit(&mut self) -> String {
        if !self.is_composing() {
            self.reset();
            return String::new();
        }
        self.finish_word()
    }

    /// Adds a text shortcut, used when `macro_enabled` and the word ends.
    pub fn add_macro(&mut self, shortcut: &str, replacement: &str) {
        self.macros
            .push((shortcut.to_lowercase(), replacement.to_owned()));
    }

    pub fn clear_macros(&mut self) {
        self.macros.clear();
    }

    /// Deletes the last letter of the composing word. With no word composing the host deletes
    /// the character itself (`PassThrough`); when that character was the last boundary after a
    /// finished word, the word is picked back up and `is_composing()` turns true, with
    /// `preedit()` equal to the word's text now before the cursor.
    pub fn backspace(&mut self) -> KeyResult {
        let Some(last_index) = self.word.letters.len().checked_sub(1) else {
            self.step_back_toward_previous_word();
            return KeyResult::PassThrough;
        };
        let shows_typed_keys = (self.frozen || self.replay_pending)
            && !self.escaped
            && self.preedit.chars().eq(self.raw_keys.iter().copied());
        let deletes_tone_carrier =
            self.word.tone_position(self.config.tone_style) == Some(last_index);
        self.word.letters.pop();
        if deletes_tone_carrier {
            self.word.tone = 0;
        }
        self.last_mapped_key = None;
        self.escape_pending = false;
        self.tone_undo_key_index = None;
        self.undo_apart = false;
        if self.word.letters.is_empty() {
            self.reset();
            return KeyResult::Composing;
        }
        self.drop_tone_without_vowels();
        self.refresh_preedit();
        // What is left is what the user sees; it becomes the text to restore.
        self.raw_keys.clear();
        self.raw_keys.extend(self.preedit.chars());
        self.frozen = self.validity() == Validity::Invalid;
        self.replay_pending = shows_typed_keys;
        KeyResult::Composing
    }

    pub fn process_key(&mut self, key: char) -> KeyResult {
        if self.replay_pending {
            self.replay_pending = false;
            if self.key_edits_word(key) {
                self.replay_raw_keys();
            }
        }
        let previously_mapped = self.last_mapped_key.take();
        if self.word.letters.len() >= MAX_WORD_LETTERS {
            return self.finish_with_boundary(key);
        }
        if self.escape_pending {
            self.escape_pending = false;
            return self.append_literal(key);
        }
        if self.frozen {
            if !key.is_alphabetic() {
                return self.finish_with_boundary(key);
            }
            return self.append_literal(key);
        }
        let action = action_for(self.config.input_method, key);
        if action == Action::Escape {
            return self.start_escape(key);
        }
        let consumed = self.apply_action(action, key, previously_mapped);
        if !consumed {
            if !key.is_alphabetic() {
                return self.finish_with_boundary(key);
            }
            self.word.letters.push(Letter::plain(key));
        }
        self.raw_keys.push(key);
        self.after_change();
        KeyResult::Composing
    }

    fn start_escape(&mut self, key: char) -> KeyResult {
        if !self.is_composing() {
            return self.finish_with_boundary(key);
        }
        self.escape_pending = true;
        self.escaped = true;
        KeyResult::Composing
    }

    /// A letter, or a key the input method turns into a mark or tone; not a boundary.
    fn key_edits_word(&self, key: char) -> bool {
        let action = action_for(self.config.input_method, key);
        key.is_alphabetic() || !matches!(action, Action::None | Action::Escape)
    }

    fn replay_raw_keys(&mut self) {
        let (keys, count) = copy_keys(&self.raw_keys);
        let after_symbol = self.after_symbol;
        self.replay_keys(&keys[..count], after_symbol);
    }

    /// Rebuilds the word as if `keys` had just been typed. A Vietnamese character, which the
    /// keys hold once a backspace has turned them into the shown text, goes back in as its
    /// letter and tone. Returns false when a key would have ended the word.
    fn replay_keys(&mut self, keys: &[char], after_symbol: bool) -> bool {
        self.reset();
        self.after_symbol = after_symbol;
        for &key in keys {
            if !self.replay_key(key) {
                self.reset();
                return false;
            }
        }
        true
    }

    fn replay_key(&mut self, key: char) -> bool {
        let decomposed = if key.is_ascii() {
            None
        } else {
            Letter::from_vietnamese_char(key)
        };
        let Some((letter, tone)) = decomposed else {
            if !self.is_composing() && !self.key_edits_word(key) {
                return false;
            }
            return self.process_key(key) == KeyResult::Composing;
        };
        self.word.letters.push(letter);
        if tone != 0 {
            self.word.tone = tone;
        }
        self.raw_keys.push(key);
        self.transformed |= letter.mark != Mark::None || tone != 0;
        self.after_change();
        true
    }

    /// The host is deleting the character before the cursor while no word is composing.
    fn step_back_toward_previous_word(&mut self) {
        match self.boundaries_after_previous_word {
            0 => {}
            1 => self.resume_previous_word(),
            count => self.boundaries_after_previous_word = count - 1,
        }
    }

    /// Makes the previous word the composing word again, exactly as it was shown.
    fn resume_previous_word(&mut self) {
        let (keys, count) = copy_keys(&self.previous_word_keys);
        let keys = &keys[..count];
        let replayed = self.replay_keys(keys, self.previous_word_after_symbol);
        if replayed && self.preedit == self.previous_word_text {
            return;
        }
        if self.previous_word_text.chars().eq(keys.iter().copied()) {
            // Given back as typed when it was finished ("coffee"): show it as typed, and let
            // the next key that edits it rebuild it from the keys.
            self.show_keys_as_typed(keys);
            return;
        }
        // A macro replaced the word; its text is not something the keys can rebuild.
        self.reset();
    }

    fn show_keys_as_typed(&mut self, keys: &[char]) {
        let after_symbol = self.previous_word_after_symbol;
        self.reset();
        self.after_symbol = after_symbol;
        self.raw_keys.extend_from_slice(keys);
        self.word
            .letters
            .extend(keys.iter().map(|&key| Letter::plain(key)));
        self.refresh_preedit();
        self.frozen = self.validity() == Validity::Invalid;
        self.replay_pending = true;
    }

    /// Keeps the word being finished so a backspace over its boundary can pick it back up.
    /// Called before `finish_word`, which clears the keys; the text is added after.
    fn remember_keys_of_finishing_word(&mut self) -> bool {
        let can_rebuild = !self.escaped && self.raw_keys.len() <= MAX_WORD_LETTERS;
        self.previous_word_keys.clear();
        if can_rebuild {
            self.previous_word_keys.extend_from_slice(&self.raw_keys);
            self.previous_word_after_symbol = self.after_symbol;
        }
        can_rebuild
    }

    fn append_literal(&mut self, key: char) -> KeyResult {
        self.word.letters.push(Letter::plain(key));
        self.raw_keys.push(key);
        self.frozen = true;
        self.restore_if_doubled_key_word();
        self.refresh_preedit();
        KeyResult::Composing
    }

    fn finish_with_boundary(&mut self, key: char) -> KeyResult {
        if !self.is_composing() {
            let boundaries = self.boundaries_after_previous_word;
            self.reset();
            self.after_symbol = !key.is_whitespace();
            if boundaries > 0 && boundaries < MAX_BOUNDARIES_AFTER_WORD {
                self.boundaries_after_previous_word = boundaries + 1;
            }
            return KeyResult::PassThrough;
        }
        let remembered = self.remember_keys_of_finishing_word();
        let mut text = self.finish_word();
        if remembered {
            self.previous_word_text.clear();
            self.previous_word_text.push_str(&text);
            self.boundaries_after_previous_word = 1;
        }
        text.push(key);
        self.after_symbol = !key.is_whitespace();
        KeyResult::Commit(text)
    }

    fn finish_word(&mut self) -> String {
        // Room for the boundary key the caller appends, so the text is allocated once.
        let mut text = String::with_capacity(self.preedit.len() + BOUNDARY_ROOM);
        if self.should_restore_unfinished_word() || self.should_give_back_english_word() {
            text.extend(self.raw_keys.iter());
        } else {
            text.push_str(&self.preedit);
        }
        if self.config.macro_enabled
            && let Some(expanded) = self.expand_macro(&text)
        {
            text = expanded;
        }
        self.reset();
        text
    }

    /// A word that carries marks but stops short of a Vietnamese word, like
    /// "tién" for "tiens", is given back as typed.
    fn should_restore_unfinished_word(&self) -> bool {
        self.config.restore_invalid_word
            && self.config.spell_check
            && self.transformed
            && self.word.has_marks()
            && self.word.analyze() != Validity::Valid
    }

    fn expand_macro(&self, typed: &str) -> Option<String> {
        let lowercase = typed.to_lowercase();
        let (_, replacement) = self
            .macros
            .iter()
            .find(|(shortcut, _)| *shortcut == lowercase)?;
        let letter_count = typed
            .chars()
            .filter(|character| character.is_alphabetic())
            .count();
        let all_upper = typed
            .chars()
            .filter(|character| character.is_alphabetic())
            .all(char::is_uppercase);
        if letter_count > 1 && all_upper {
            return Some(replacement.to_uppercase());
        }
        Some(replacement.clone())
    }

    fn validity(&self) -> Validity {
        if self.config.spell_check {
            self.word.analyze()
        } else {
            Validity::Valid
        }
    }

    fn refresh_preedit(&mut self) {
        self.preedit.clear();
        self.word.render(self.config.tone_style, &mut self.preedit);
    }

    fn drop_tone_without_vowels(&mut self) {
        if self.word.tone != 0 && !self.word.has_vowels() {
            self.word.tone = 0;
        }
    }

    /// ưo and uơ followed by a final consonant, or by i or u, are always ươ.
    fn complete_horn_pair(&mut self) {
        let layout = self.word.layout();
        let has_final = layout.vowel_end < self.word.letters.len();
        let glide_follows = layout.vowel_len() == 3
            && matches!(self.word.letters[layout.vowel_start + 2].base, 'i' | 'u');
        let completes = match layout.vowel_len() {
            2 => has_final,
            3 => glide_follows,
            _ => false,
        };
        if !completes {
            return;
        }
        let pair = &mut self.word.letters[layout.vowel_start..layout.vowel_start + 2];
        let is_u_o = pair[0].base == 'u' && pair[1].base == 'o';
        let one_horned = (pair[0].mark == Mark::Hook) != (pair[1].mark == Mark::Hook);
        if is_u_o && one_horned && pair[1].mark != Mark::Roof {
            pair[0].mark = Mark::Hook;
            pair[1].mark = Mark::Hook;
        }
    }

    fn after_change(&mut self) {
        self.complete_horn_pair();
        self.drop_tone_without_vowels();
        if self.config.spell_check {
            self.freeze_or_restore_if_needed();
        }
        self.refresh_preedit();
    }

    fn freeze_or_restore_if_needed(&mut self) {
        if self.restore_if_doubled_key_word() {
            self.frozen = true;
            return;
        }
        if self.word.analyze() == Validity::Invalid {
            if self.config.restore_invalid_word && self.word.has_marks() {
                self.restore_typed_keys();
            }
            self.frozen = true;
        }
    }

    /// "pass" reaches the engine as "pas" after the second s undoes the tone;
    /// when the raw keys spell a known English word, or a letter follows a z that removed a
    /// tone ("arizona"), give the keys back.
    fn restore_if_doubled_key_word(&mut self) -> bool {
        let applies =
            self.config.restore_invalid_word && self.config.spell_check && self.key_undone;
        if !applies {
            return false;
        }
        // A word on the English list is only given back when it ends (`finish_word`): while
        // typing, "mess" then "a" is "mesa", the doubled key just taking its mark off.
        let foreign = self.undo_apart || self.letter_follows_doubled_tone_key();
        if !foreign {
            return false;
        }
        self.restore_typed_keys();
        true
    }

    /// "pass" shows "pas" while typing (the second s undoes the tone); when the word ends
    /// and its keys are on the English list, they are given back as typed.
    fn should_give_back_english_word(&self) -> bool {
        self.config.restore_invalid_word
            && self.config.spell_check
            && self.key_undone
            && english_rule_holds(
                self.raw_keys.iter().copied(),
                english::is_doubled_key_word_form,
            )
            && !self.shows_doubled_key_word()
    }

    /// The word on screen already starts with such a word: the key was pressed a third time
    /// to put the letter back ("passs" shows "pass", "masssa" shows "massa"), so the keys
    /// hold one letter too many.
    fn shows_doubled_key_word(&self) -> bool {
        english_rule_holds(self.preedit.chars(), english::begins_with_a_listed_word)
    }

    /// A letter typed after a doubled tone key, other than another tone key: "ss" then "a"
    /// is a foreign word, while "asss" and "assf" keep undoing and toning as before.
    fn letter_follows_doubled_tone_key(&self) -> bool {
        let Some(undo_index) = self.tone_undo_key_index else {
            return false;
        };
        let Some(&last_key) = self.raw_keys.last() else {
            return false;
        };
        let typed_after_undo = self.raw_keys.len() > undo_index + 1;
        let is_tone_key = matches!(
            action_for(self.config.input_method, last_key),
            Action::Tone(_)
        );
        typed_after_undo && last_key.is_alphabetic() && !is_tone_key
    }

    fn restore_typed_keys(&mut self) {
        self.word.letters.clear();
        self.word
            .letters
            .extend(self.raw_keys.iter().map(|&key| Letter::plain(key)));
        self.word.tone = 0;
    }

    fn apply_action(&mut self, action: Action, key: char, previously_mapped: Option<char>) -> bool {
        let consumed = match action {
            Action::Tone(tone) => self.apply_tone(tone, key),
            Action::Roof(target) => self.apply_roof(target, key),
            Action::Hook(kind) => self.apply_hook(kind, key),
            Action::Stroke => self.apply_stroke(key),
            Action::TelexW => self.apply_telex_w(key, previously_mapped),
            Action::MapLetter { base, upper } => {
                self.apply_mapped_letter(key, base, upper, previously_mapped)
            }
            Action::Escape | Action::None => false,
        };
        self.transformed |= consumed;
        consumed
    }

    /// Undoing a mark types the key that asked for it.
    fn undo_with_literal(&mut self, key: char) {
        self.key_undone = true;
        let follows_same_key = self
            .raw_keys
            .last()
            .is_some_and(|previous| previous.to_lowercase().eq(key.to_lowercase()));
        self.undo_apart |= !follows_same_key;
        self.word.letters.push(Letter::plain(key));
    }

    fn apply_tone(&mut self, tone: u8, key: char) -> bool {
        if !self.word.has_vowels() {
            return false;
        }
        if tone == 0 {
            let had_tone = self.word.tone != 0;
            self.word.tone = 0;
            if had_tone {
                // Like a doubled tone key: a letter after it means the z was a letter of a
                // foreign word ("arizona", "isuzu").
                self.key_undone = true;
                self.tone_undo_key_index = Some(self.raw_keys.len());
            }
            return had_tone;
        }
        let refuses_incomplete = self.config.spell_check && !self.config.free_tone_placement;
        if refuses_incomplete && self.validity() == Validity::Incomplete {
            return false;
        }
        if self.word.tone == tone {
            self.word.tone = 0;
            // As in Unikey, the doubled key stays one letter whatever follows: "tassk" is
            // "task". Only words on the English list come back with both ("lesson").
            self.undo_with_literal(key);
            return true;
        }
        let previous_tone = self.word.tone;
        self.word.tone = tone;
        if self.validity() == Validity::Invalid {
            self.word.tone = previous_tone;
            return false;
        }
        true
    }

    /// With free placement any letter may be marked; without it only the last.
    fn may_mark(&self, index: usize) -> bool {
        self.config.free_tone_placement || index + 1 == self.word.letters.len()
    }

    fn apply_roof(&mut self, target: Option<char>, key: char) -> bool {
        let layout = self.word.layout();
        let wanted = |letter: Letter| {
            matches!(letter.base, 'a' | 'e' | 'o') && target.is_none_or(|base| base == letter.base)
        };
        for index in layout.vowel_start..layout.vowel_end {
            let letter = self.word.letters[index];
            if wanted(letter)
                && letter.mark != Mark::Roof
                && self.may_mark(index)
                && self.try_add_roof(index, layout.vowel_start)
            {
                return true;
            }
        }
        for index in layout.vowel_start..layout.vowel_end {
            let letter = self.word.letters[index];
            if wanted(letter) && letter.mark == Mark::Roof {
                self.word.letters[index].mark = Mark::None;
                self.undo_with_literal(key);
                return true;
            }
        }
        false
    }

    /// Puts a circumflex on the letter at `index`; ưô becomes uô.
    fn try_add_roof(&mut self, index: usize, vowel_start: usize) -> bool {
        let saved_letter = self.word.letters[index];
        let previous_index = index
            .checked_sub(1)
            .filter(|&previous| previous >= vowel_start);
        let saved_previous = previous_index.map(|previous| self.word.letters[previous]);
        self.word.letters[index].mark = Mark::Roof;
        if let Some(previous) = previous_index {
            let horned_u = self.word.letters[previous].base == 'u'
                && self.word.letters[previous].mark == Mark::Hook;
            if saved_letter.base == 'o' && horned_u {
                self.word.letters[previous].mark = Mark::None;
            }
        }
        if self.validity() != Validity::Invalid {
            return true;
        }
        self.word.letters[index] = saved_letter;
        if let (Some(previous), Some(letter)) = (previous_index, saved_previous) {
            self.word.letters[previous] = letter;
        }
        false
    }

    fn apply_hook(&mut self, kind: HookKind, key: char) -> bool {
        let layout = self.word.layout();
        if kind != HookKind::BreveOnly
            && self.is_u_o_pair(layout)
            && self.apply_hook_to_u_o(layout, key)
        {
            return true;
        }
        let takes_hook = |letter: Letter| match kind {
            HookKind::All => matches!(letter.base, 'a' | 'o' | 'u'),
            HookKind::HornOnly => matches!(letter.base, 'o' | 'u'),
            HookKind::BreveOnly => letter.base == 'a',
        } && letter.is_vowel();
        for index in layout.vowel_start..layout.vowel_end {
            let letter = self.word.letters[index];
            if !takes_hook(letter) || letter.mark == Mark::Hook || !self.may_mark(index) {
                continue;
            }
            self.word.letters[index].mark = Mark::Hook;
            if self.validity() != Validity::Invalid {
                return true;
            }
            self.word.letters[index] = letter;
        }
        for index in layout.vowel_start..layout.vowel_end {
            let letter = self.word.letters[index];
            if takes_hook(letter) && letter.mark == Mark::Hook {
                self.word.letters[index].mark = Mark::None;
                self.undo_with_literal(key);
                return true;
            }
        }
        false
    }

    fn is_u_o_pair(&self, layout: word::Layout) -> bool {
        if layout.vowel_len() < 2 {
            return false;
        }
        let first = self.word.letters[layout.vowel_start];
        let second = self.word.letters[layout.vowel_start + 1];
        first.base == 'u' && second.base == 'o'
    }

    /// The u-o pair takes its horn on both letters (ươ), or on the o alone in
    /// "thuơ"; a second press removes both.
    fn apply_hook_to_u_o(&mut self, layout: word::Layout, key: char) -> bool {
        let first_index = layout.vowel_start;
        let saved = [
            self.word.letters[first_index],
            self.word.letters[first_index + 1],
        ];
        let horned = [saved[0].mark == Mark::Hook, saved[1].mark == Mark::Hook];
        if horned == [true, true] {
            self.word.letters[first_index].mark = Mark::None;
            self.word.letters[first_index + 1].mark = Mark::None;
            self.undo_with_literal(key);
            return true;
        }
        if !self.may_mark(first_index + 1) {
            return false;
        }
        let horn_on_o_only = layout.vowel_len() == 2
            && layout.vowel_end == self.word.letters.len()
            && self.has_initial_th(layout)
            && horned == [false, false];
        self.word.letters[first_index + 1].mark = Mark::Hook;
        if !horn_on_o_only {
            self.word.letters[first_index].mark = Mark::Hook;
        }
        if self.validity() != Validity::Invalid {
            return true;
        }
        self.word.letters[first_index] = saved[0];
        self.word.letters[first_index + 1] = saved[1];
        false
    }

    fn has_initial_th(&self, layout: word::Layout) -> bool {
        let initial = &self.word.letters[..layout.vowel_start];
        initial.len() == 2 && initial[0].base == 't' && initial[1].base == 'h'
    }

    fn apply_stroke(&mut self, key: char) -> bool {
        let Some(&first) = self.word.letters.first() else {
            return false;
        };
        let is_lone_initial_d = first.base == 'd'
            && matches!(first.mark, Mark::None | Mark::Stroke)
            && self.word.layout().vowel_start == 1;
        if !is_lone_initial_d {
            return false;
        }
        if first.mark == Mark::Stroke {
            self.word.letters[0].mark = Mark::None;
            // A late d undoes đ from anywhere in the word ("ddad" gives "dad"), so it is not
            // a sign of a foreign word the way a far-off tone or roof key is.
            let undo_apart_before = self.undo_apart;
            self.undo_with_literal(key);
            self.undo_apart = undo_apart_before;
            return true;
        }
        if !self.config.free_tone_placement && self.word.letters.len() != 1 {
            return false;
        }
        self.word.letters[0].mark = Mark::Stroke;
        true
    }

    fn apply_telex_w(&mut self, key: char, previously_mapped: Option<char>) -> bool {
        if self.undo_mapped_letter(key, 'u', previously_mapped) {
            return true;
        }
        if self.apply_hook(HookKind::All, key) {
            return true;
        }
        self.push_mapped_letter(key, 'u', key.is_uppercase())
    }

    fn apply_mapped_letter(
        &mut self,
        key: char,
        base: char,
        upper: bool,
        previously_mapped: Option<char>,
    ) -> bool {
        if self.after_symbol && !self.is_composing() {
            // "a[0]" and "){" are code, not ơ and ư.
            return false;
        }
        if self.undo_mapped_letter(key, base, previously_mapped) {
            return true;
        }
        self.push_mapped_letter(key, base, upper)
    }

    /// Pressing a mapped key twice turns the mapped letter back into the key.
    fn undo_mapped_letter(
        &mut self,
        key: char,
        base: char,
        previously_mapped: Option<char>,
    ) -> bool {
        let same_key = previously_mapped == key.to_lowercase().next();
        let Some(&last) = self.word.letters.last() else {
            return false;
        };
        if !same_key || last.base != base || last.mark != Mark::Hook {
            return false;
        }
        self.word.letters.pop();
        self.undo_with_literal(key);
        true
    }

    fn push_mapped_letter(&mut self, key: char, base: char, upper: bool) -> bool {
        self.word.letters.push(Letter::new(base, Mark::Hook, upper));
        if self.validity() == Validity::Invalid {
            self.word.letters.pop();
            return false;
        }
        self.last_mapped_key = key.to_lowercase().next();
        true
    }
}
