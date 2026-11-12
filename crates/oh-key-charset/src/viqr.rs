//! VIQR (ASCII with tone marks after the vowel: `Vie^.t`) and UVIQR (UTF-8
//! text that may also contain VIQR sequences).
//!
//! Output uses the default Unikey options: a backslash escapes a mark that
//! would otherwise be read as part of the previous vowel, and escaping is
//! suspended inside e-mail and URL like text. Input accepts `dd` as đ only at
//! the start of a word.

use std::sync::OnceLock;

use crate::standard::{Glyph, glyph_from_char};
use crate::tables::VIQR;

/// Text after which marks are not escaped until the next white space.
const NO_ESCAPE_PATTERNS: [&[u8]; 8] = [
    b"://", b"/", b"@", b"mailto:", b"email:", b"news:", b"www", b"ftp",
];

const LONGEST_PATTERN_LENGTH: usize = 7;

/// Offset from a base letter's standard index for each VIQR mark.
fn mark_offset(byte: u8) -> u8 {
    match byte {
        b'\'' => 2,
        b'`' => 4,
        b'?' => 6,
        b'~' => 8,
        b'.' => 10,
        b'^' => 12,
        b'(' => 24,
        b'+' | b'*' => 26,
        _ => 0,
    }
}

fn is_vowel(byte: u8) -> bool {
    matches!(
        byte.to_ascii_lowercase(),
        b'a' | b'e' | b'i' | b'o' | b'u' | b'y'
    )
}

fn is_white_space(byte: u8) -> bool {
    matches!(byte, b' ' | b'\t' | b'\r' | b'\n')
}

/// Remembers recent bytes to notice the no-escape patterns.
#[derive(Default)]
struct PatternTail {
    recent: Vec<u8>,
}

impl PatternTail {
    fn reset(&mut self) {
        self.recent.clear();
    }

    /// Feeds one byte; true when a pattern has just been completed.
    fn feed(&mut self, byte: u8) -> bool {
        self.recent.push(byte);
        if self.recent.len() > LONGEST_PATTERN_LENGTH {
            self.recent.remove(0);
        }
        NO_ESCAPE_PATTERNS
            .iter()
            .any(|pattern| self.recent.ends_with(pattern))
    }
}

pub(crate) fn encode_viqr(glyphs: &[Glyph]) -> Vec<u8> {
    let mut encoder = ViqrEncoder::default();
    for &glyph in glyphs {
        encoder.push_glyph(glyph);
    }
    encoder.bytes
}

#[derive(Default)]
struct ViqrEncoder {
    bytes: Vec<u8>,
    patterns: PatternTail,
    escape_suppressed: bool,
    mark_needs_escape: bool,
    bowl_needs_escape: bool,
    hook_needs_escape: bool,
    roof_needs_escape: bool,
}

impl ViqrEncoder {
    fn push_glyph(&mut self, glyph: Glyph) {
        match glyph {
            Glyph::Standard(index) => self.push_standard(VIQR[usize::from(index)]),
            Glyph::Other(character) => self.push_other(character),
        }
    }

    fn push_byte(&mut self, byte: u8) {
        self.bytes.push(byte);
        if self.patterns.feed(byte) {
            self.escape_suppressed = true;
        }
        if self.escape_suppressed && is_white_space(byte) {
            self.escape_suppressed = false;
        }
    }

    fn clear_escape_flags(&mut self) {
        self.mark_needs_escape = false;
        self.bowl_needs_escape = false;
        self.hook_needs_escape = false;
        self.roof_needs_escape = false;
    }

    fn push_standard(&mut self, sequence: u32) {
        let [first, second, third, _] = sequence.to_le_bytes();
        self.push_byte(first);
        if second == 0 {
            let upper = first.to_ascii_uppercase();
            self.mark_needs_escape = is_vowel(first);
            self.bowl_needs_escape = upper == b'A';
            self.hook_needs_escape = matches!(upper, b'U' | b'O');
            self.roof_needs_escape = matches!(upper, b'A' | b'E' | b'O');
            return;
        }
        self.bytes.push(second);
        if third != 0 {
            self.bytes.push(third);
        }
        // After a hat, breve or horn only a tone mark can follow.
        self.clear_escape_flags();
        self.mark_needs_escape = third == 0 && matches!(mark_offset(second), 12 | 24 | 26);
        self.patterns.reset();
    }

    fn push_other(&mut self, character: char) {
        let Ok(byte) = u8::try_from(u32::from(character)) else {
            self.push_byte(b'#');
            self.clear_escape_flags();
            return;
        };
        if self.needs_escape(byte) {
            self.push_byte(b'\\');
        }
        self.push_byte(byte);
        self.clear_escape_flags();
    }

    fn needs_escape(&self, byte: u8) -> bool {
        if self.escape_suppressed {
            return false;
        }
        match mark_offset(byte) {
            _ if byte == b'\\' => true,
            2..=10 => self.mark_needs_escape,
            12 => self.roof_needs_escape,
            24 => self.bowl_needs_escape,
            26 => self.hook_needs_escape,
            _ => false,
        }
    }
}

/// Standard index of a plain VIQR letter or symbol byte, if it is one.
fn letter_index(byte: u8) -> Option<u8> {
    static INDEX_FOR_BYTE: OnceLock<[Option<u8>; 256]> = OnceLock::new();
    let index_for_byte = INDEX_FOR_BYTE.get_or_init(|| {
        let mut table = [None; 256];
        for (index, &sequence) in VIQR.iter().enumerate() {
            if let Ok(single_byte) = u8::try_from(sequence) {
                table[usize::from(single_byte)] = Some(index as u8);
            }
        }
        table
    });
    index_for_byte[usize::from(byte)]
}

pub(crate) fn decode_viqr(bytes: &[u8]) -> Vec<Glyph> {
    let mut decoder = ViqrDecoder::new();
    let mut glyphs = Vec::with_capacity(bytes.len());
    let mut position = 0;
    while position < bytes.len() {
        glyphs.push(decoder.next_glyph(bytes, &mut position));
    }
    glyphs
}

/// UTF-8 sequences are decoded as UTF-8; everything else as VIQR. Right after
/// a UTF-8 character, VIQR marks are only accepted when unambiguous.
pub(crate) fn decode_utf8_viqr(bytes: &[u8]) -> Vec<Glyph> {
    let mut decoder = ViqrDecoder::new();
    let mut glyphs = Vec::with_capacity(bytes.len());
    let mut position = 0;
    while position < bytes.len() {
        let lead = bytes[position];
        if (0xC0..0xFE).contains(&lead) {
            glyphs.push(decode_utf8_character(bytes, &mut position));
            decoder = ViqrDecoder::new();
            decoder.suspicious = true;
        } else {
            glyphs.push(decoder.next_glyph(bytes, &mut position));
        }
    }
    glyphs
}

fn decode_utf8_character(bytes: &[u8], position: &mut usize) -> Glyph {
    let length = match bytes[*position] {
        0xF0.. => 4,
        0xE0.. => 3,
        _ => 2,
    };
    let sequence = bytes.get(*position..*position + length);
    match sequence.and_then(|sequence| std::str::from_utf8(sequence).ok()) {
        Some(text) => {
            *position += length;
            glyph_from_char(text.chars().next().unwrap_or('\u{FFFD}'))
        }
        None => {
            *position += 1;
            Glyph::Other('\u{FFFD}')
        }
    }
}

struct ViqrDecoder {
    at_word_start: bool,
    has_mark_in_word: bool,
    suspicious: bool,
    escape_all: bool,
    patterns: PatternTail,
}

impl ViqrDecoder {
    fn new() -> Self {
        Self {
            at_word_start: true,
            has_mark_in_word: false,
            suspicious: false,
            escape_all: false,
            patterns: PatternTail::default(),
        }
    }

    fn next_glyph(&mut self, bytes: &[u8], position: &mut usize) -> Glyph {
        let mut first = bytes[*position];
        *position += 1;
        if self.patterns.feed(first) {
            self.escape_all = true;
        }
        if self.escape_all && is_white_space(first) {
            self.escape_all = false;
        }
        let mut base_index = letter_index(first);
        if first == b'\\' {
            // A backslash makes the next byte literal.
            if let Some(&escaped) = bytes.get(*position) {
                *position += 1;
                first = escaped;
                base_index = None;
            }
        }
        let Some(base_index) = base_index else {
            self.at_word_start = true;
            self.has_mark_in_word = false;
            return glyph_from_char(char::from(first));
        };
        let glyph_index = if self.escape_all {
            u16::from(base_index)
        } else {
            self.apply_marks(first, base_index, bytes, position)
        };
        self.at_word_start = false;
        Glyph::Standard(glyph_index as u8)
    }

    fn apply_marks(
        &mut self,
        letter: u8,
        base_index: u8,
        bytes: &[u8],
        position: &mut usize,
    ) -> u16 {
        let Some(&next) = bytes.get(*position) else {
            return u16::from(base_index);
        };
        let upper = letter.to_ascii_uppercase();
        if self.at_word_start && upper == b'D' && matches!(next, b'd' | b'D') {
            *position += 1;
            return u16::from(base_index) + 2;
        }
        let offset = mark_offset(next);
        if !self.accepts_mark(letter, offset) {
            return u16::from(base_index);
        }
        self.has_mark_in_word = true;
        *position += 1;
        let mut index = u16::from(base_index) + u16::from(self.effective_offset(letter, offset));
        let tone_after_hat = bytes
            .get(*position)
            .map(|&byte| mark_offset(byte))
            .filter(|&tone| offset > 10 && (1..=10).contains(&tone));
        if let Some(tone) = tone_after_hat {
            *position += 1;
            index += u16::from(tone);
        }
        index
    }

    /// The horn is stored in the breve slot (24); on u it is the roof slot.
    fn effective_offset(&self, letter: u8, offset: u8) -> u8 {
        let offset = if offset == 26 { 24 } else { offset };
        if offset == 24 && letter.eq_ignore_ascii_case(&b'u') {
            12
        } else {
            offset
        }
    }

    fn accepts_mark(&mut self, letter: u8, offset: u8) -> bool {
        if !is_vowel(letter) {
            return false;
        }
        let upper = letter.to_ascii_uppercase();
        let shape_marks = (offset == 12 && matches!(upper, b'A' | b'E' | b'O'))
            || (offset == 24 && upper == b'A')
            || (offset == 26 && matches!(upper, b'O' | b'U'));
        let tone_marks = if self.suspicious {
            // `?` and `.` are too common in prose to trust after UTF-8 text.
            matches!(offset, 2 | 4 | 8)
        } else {
            (1..=10).contains(&offset) && (!self.has_mark_in_word || (offset != 6 && offset != 10))
        };
        let accepted = shape_marks || tone_marks;
        if accepted && self.suspicious {
            self.suspicious = false;
        }
        accepted
    }
}
