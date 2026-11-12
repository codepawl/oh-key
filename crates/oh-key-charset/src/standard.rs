//! The hub every charset converts through: Unikey's "standard Vietnamese
//! character set", 213 entries.
//!
//! Indexes 0..186 are the Vietnamese-relevant letters. Upper case sits at even
//! indexes, lower case at odd ones, and each vowel family takes 12 slots in the
//! tone order none, acute, grave, hook, tilde, dot (upper and lower each).
//! Indexes 186..213 are Western symbols that legacy charsets re-purpose.

use std::collections::HashMap;
use std::sync::OnceLock;

use crate::tables::{UNICODE_DECOMPOSED, UNICODE_PRECOMPOSED};

/// Index of the first Western symbol in the standard set.
pub(crate) const FIRST_SYMBOL_INDEX: u8 = 186;
/// Symbols whose single-byte fallback is a plain ASCII look-alike.
pub(crate) const START_QUOTE_INDEX: u8 = 201;
pub(crate) const END_QUOTE_INDEX: u8 = 202;
pub(crate) const ELLIPSIS_INDEX: u8 = 190;

/// One unit of text in the hub: either a standard character or anything else.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Glyph {
    Standard(u8),
    Other(char),
}

const COMBINING_MARKS: [u16; 5] = [0x0300, 0x0301, 0x0303, 0x0309, 0x0323];

fn index_by_unicode() -> &'static HashMap<u16, u8> {
    static MAP: OnceLock<HashMap<u16, u8>> = OnceLock::new();
    MAP.get_or_init(|| {
        UNICODE_PRECOMPOSED
            .iter()
            .enumerate()
            .map(|(index, &code_point)| (code_point, index as u8))
            .collect()
    })
}

/// Map from (base letter, combining mark) to the standard index.
fn index_by_base_and_mark() -> &'static HashMap<(u16, u16), u8> {
    static MAP: OnceLock<HashMap<(u16, u16), u8>> = OnceLock::new();
    MAP.get_or_init(|| {
        UNICODE_DECOMPOSED
            .iter()
            .enumerate()
            .filter(|&(_, &composite)| composite >> 16 != 0)
            .map(|(index, &composite)| {
                let base = (composite & 0xFFFF) as u16;
                let mark = (composite >> 16) as u16;
                ((base, mark), index as u8)
            })
            .collect()
    })
}

pub(crate) fn glyph_from_char(character: char) -> Glyph {
    let index = u16::try_from(u32::from(character))
        .ok()
        .and_then(|code_point| index_by_unicode().get(&code_point));
    match index {
        Some(&index) => Glyph::Standard(index),
        None => Glyph::Other(character),
    }
}

pub(crate) fn char_from_glyph(glyph: Glyph) -> char {
    match glyph {
        Glyph::Standard(index) => {
            char::from_u32(u32::from(UNICODE_PRECOMPOSED[usize::from(index)])).unwrap_or('\u{FFFD}')
        }
        Glyph::Other(character) => character,
    }
}

/// Glyphs for Unicode text. A letter followed by a combining tone mark is
/// joined, so decomposed input reaches the hub the same way as precomposed.
pub(crate) fn glyphs_from_text(text: &str) -> Vec<Glyph> {
    let characters: Vec<char> = text.chars().collect();
    let mut glyphs = Vec::with_capacity(characters.len());
    let mut position = 0;
    while position < characters.len() {
        let character = characters[position];
        if let Some(index) = joined_with_mark(character, characters.get(position + 1).copied()) {
            glyphs.push(Glyph::Standard(index));
            position += 2;
        } else {
            glyphs.push(glyph_from_char(character));
            position += 1;
        }
    }
    glyphs
}

fn joined_with_mark(base: char, mark: Option<char>) -> Option<u8> {
    let mark = u16::try_from(u32::from(mark?)).ok()?;
    if !COMBINING_MARKS.contains(&mark) {
        return None;
    }
    let base = u16::try_from(u32::from(base)).ok()?;
    index_by_base_and_mark().get(&(base, mark)).copied()
}

pub(crate) fn text_from_glyphs(glyphs: &[Glyph]) -> String {
    glyphs.iter().copied().map(char_from_glyph).collect()
}
