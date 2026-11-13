//! The word being composed: letters plus one tone, with the syllable analysis
//! (initial, vowels, final), validity and tone placement derived from them.

use crate::ToneStyle;
use crate::letter::{Letter, Mark};
use crate::tables;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Validity {
    Valid,
    /// A legal prefix that is not yet a word, such as "tie" waiting for its ê.
    Incomplete,
    Invalid,
}

/// Where the three parts of a syllable sit in the letters.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Layout {
    pub vowel_start: usize,
    pub vowel_end: usize,
}

impl Layout {
    pub fn vowel_len(self) -> usize {
        self.vowel_end - self.vowel_start
    }
}

/// Longest vowel run any Vietnamese syllable has.
const LONGEST_VOWEL_RUN: usize = 3;
const LONGEST_CLUSTER: usize = 3;

#[derive(Default)]
pub(crate) struct Word {
    pub letters: Vec<Letter>,
    pub tone: u8,
}

impl Word {
    pub fn clear(&mut self) {
        self.letters.clear();
        self.tone = 0;
    }

    pub fn has_marks(&self) -> bool {
        self.tone != 0 || self.letters.iter().any(|letter| letter.mark != Mark::None)
    }

    /// The u of "qu" and the i of "gi" belong to the initial, not the vowels.
    /// The i of "gi" only counts as a consonant when a vowel follows it.
    fn is_glide_consonant(&self, index: usize) -> bool {
        if index != 1 {
            return false;
        }
        let first = self.letters[0];
        let second = self.letters[1];
        if first.base == 'q' && second.base == 'u' && second.mark == Mark::None {
            return true;
        }
        let vowel_follows = self.letters.get(2).is_some_and(|letter| letter.is_vowel());
        first.base == 'g' && second.base == 'i' && vowel_follows
    }

    pub fn layout(&self) -> Layout {
        let letter_count = self.letters.len();
        let mut vowel_start = 0;
        while vowel_start < letter_count {
            let letter = self.letters[vowel_start];
            if letter.is_vowel() && !self.is_glide_consonant(vowel_start) {
                break;
            }
            vowel_start += 1;
        }
        let mut vowel_end = vowel_start;
        while vowel_end < letter_count && self.letters[vowel_end].is_vowel() {
            vowel_end += 1;
        }
        Layout {
            vowel_start,
            vowel_end,
        }
    }

    pub fn has_vowels(&self) -> bool {
        self.layout().vowel_len() > 0
    }

    /// Checks the word against the syllable tables.
    pub fn analyze(&self) -> Validity {
        if self.letters.iter().any(|letter| !letter.is_vietnamese()) {
            return Validity::Invalid;
        }
        let layout = self.layout();
        let mut initial = [0u8; LONGEST_CLUSTER];
        let Some(initial_len) = fill_cluster(&self.letters[..layout.vowel_start], &mut initial)
        else {
            return Validity::Invalid;
        };
        let initial = &initial[..initial_len];
        if initial_len > 0 && !tables::is_initial_cluster(initial) {
            return Validity::Invalid;
        }
        if layout.vowel_len() == 0 {
            return Validity::Valid;
        }
        self.analyze_vowels_and_final(layout, initial)
    }

    fn analyze_vowels_and_final(&self, layout: Layout, initial: &[u8]) -> Validity {
        if layout.vowel_len() > LONGEST_VOWEL_RUN {
            return Validity::Invalid;
        }
        let mut sequence = [0u8; LONGEST_VOWEL_RUN];
        for (slot, letter) in sequence
            .iter_mut()
            .zip(&self.letters[layout.vowel_start..layout.vowel_end])
        {
            *slot = letter.sequence_code().unwrap_or(0);
        }
        let sequence = &sequence[..layout.vowel_len()];
        let Some(complete) = tables::vowel_sequence_is_complete(sequence) else {
            return Validity::Invalid;
        };
        if !tables::initial_accepts_vowels(initial, sequence) {
            return Validity::Invalid;
        }
        let final_letters = &self.letters[layout.vowel_end..];
        if final_letters.is_empty() {
            return completeness(complete);
        }
        let mut final_cluster = [0u8; 2];
        let Some(final_len) = fill_cluster(final_letters, &mut final_cluster) else {
            return Validity::Invalid;
        };
        if final_letters.iter().any(|letter| letter.is_vowel()) {
            return Validity::Invalid;
        }
        let final_bit = tables::final_bit(&final_cluster[..final_len]);
        let allowed = tables::allowed_finals(sequence) & final_bit != 0;
        if final_bit == 0
            || (!allowed && !tables::is_vowel_final_exception(initial, sequence, final_bit))
        {
            return Validity::Invalid;
        }
        let refused_tone = matches!(self.tone, 2..=4);
        if refused_tone && final_bit & tables::STOP_FINALS != 0 {
            return Validity::Invalid;
        }
        if tables::final_waits_for_h(initial, sequence, final_bit) {
            return Validity::Incomplete;
        }
        completeness(complete)
    }

    /// Index of the letter that carries the tone, `None` when there is no tone
    /// or no vowel.
    pub fn tone_position(&self, style: ToneStyle) -> Option<usize> {
        if self.tone == 0 {
            return None;
        }
        let layout = self.layout();
        if layout.vowel_len() == 0 {
            return None;
        }
        let vowels = &self.letters[layout.vowel_start..layout.vowel_end];
        let offset = if vowels.len() == 1 {
            0
        } else {
            self.tone_offset_in(vowels, layout, style)
        };
        Some(layout.vowel_start + offset)
    }

    fn tone_offset_in(&self, vowels: &[Letter], layout: Layout, style: ToneStyle) -> usize {
        if let Some(roofed) = vowels.iter().position(|letter| letter.mark == Mark::Roof) {
            return roofed;
        }
        if let Some(hooked) = vowels.iter().position(|letter| letter.mark == Mark::Hook) {
            // In ươ the tone goes on the ơ.
            let horned_o = vowels
                .iter()
                .position(|letter| letter.base == 'o' && letter.mark == Mark::Hook);
            return horned_o.unwrap_or(hooked);
        }
        if vowels.len() == 3 {
            return 1;
        }
        let has_final = layout.vowel_end < self.letters.len();
        let pair = (vowels[0].base, vowels[1].base);
        let modern_second =
            style == ToneStyle::Modern && matches!(pair, ('o', 'a') | ('o', 'e') | ('u', 'y'));
        usize::from(modern_second || has_final)
    }

    /// Writes the word as NFC text.
    pub fn render(&self, style: ToneStyle, output: &mut String) {
        let tone_position = self.tone_position(style);
        for (index, letter) in self.letters.iter().enumerate() {
            let tone = if Some(index) == tone_position {
                self.tone
            } else {
                0
            };
            output.push(letter.to_char(tone));
        }
    }
}

fn completeness(complete: bool) -> Validity {
    if complete {
        Validity::Valid
    } else {
        Validity::Incomplete
    }
}

/// Copies the cluster codes into `buffer`; `None` when the cluster is too long.
fn fill_cluster(letters: &[Letter], buffer: &mut [u8]) -> Option<usize> {
    if letters.len() > buffer.len() {
        return None;
    }
    for (slot, letter) in buffer.iter_mut().zip(letters) {
        *slot = letter.cluster_code();
    }
    Some(letters.len())
}
