//! Legacy 8-bit charsets: one byte per character (TCVN3, VPS, VISCII, BK HCM 1,
//! Vietware-F, ISC) or a letter byte plus an optional tone byte (VNI, BK HCM 2,
//! Vietware-X, CP-1258).

use std::collections::HashMap;

use crate::standard::{
    ELLIPSIS_INDEX, END_QUOTE_INDEX, FIRST_SYMBOL_INDEX, Glyph, START_QUOTE_INDEX,
};
use crate::tables::STANDARD_CHARACTER_COUNT;

/// Written for a character the charset cannot represent.
const PAD_BYTE: u8 = b'#';

type SingleByteTable = [u8; STANDARD_CHARACTER_COUNT];
type WordTable = [u16; STANDARD_CHARACTER_COUNT];

/// Some tables put a printable ASCII filler (VNI-Mac uses `?`) in the slot of
/// a symbol they cannot hold. Decoding it as that symbol would turn every
/// ordinary `?` into a symbol, so such entries are write-only.
fn is_ascii_filler_for_symbol(index: usize, byte: u8) -> bool {
    index >= usize::from(FIRST_SYMBOL_INDEX) && (0x20..0x7F).contains(&byte)
}

fn pad_byte_for_missing_symbol(index: u8) -> u8 {
    match index {
        START_QUOTE_INDEX | END_QUOTE_INDEX => b'"',
        ELLIPSIS_INDEX => b'.',
        _ => PAD_BYTE,
    }
}

pub(crate) struct SingleByteCodec {
    byte_for_index: &'static SingleByteTable,
    index_for_byte: [Option<u8>; 256],
}

impl SingleByteCodec {
    pub(crate) fn new(byte_for_index: &'static SingleByteTable) -> Self {
        let mut index_for_byte = [None; 256];
        for (index, &byte) in byte_for_index.iter().enumerate() {
            let shares_byte_with_next = byte_for_index.get(index + 1) == Some(&byte);
            // Charsets that have no upper-case toned letters repeat the lower
            // case byte for the upper case slot: decode to the lower case one.
            let is_decodable = byte != 0 && !is_ascii_filler_for_symbol(index, byte);
            if is_decodable && !shares_byte_with_next {
                index_for_byte[usize::from(byte)] = Some(index as u8);
            }
        }
        Self {
            byte_for_index,
            index_for_byte,
        }
    }

    pub(crate) fn encode(&self, glyphs: &[Glyph]) -> Vec<u8> {
        glyphs
            .iter()
            .map(|&glyph| self.encode_glyph(glyph))
            .collect()
    }

    fn encode_glyph(&self, glyph: Glyph) -> u8 {
        match glyph {
            Glyph::Standard(index) => match self.byte_for_index[usize::from(index)] {
                0 => pad_byte_for_missing_symbol(index),
                byte => byte,
            },
            Glyph::Other(character) => match u8::try_from(u32::from(character)) {
                Ok(byte) if self.index_for_byte[usize::from(byte)].is_none() => byte,
                _ => PAD_BYTE,
            },
        }
    }

    pub(crate) fn decode(&self, bytes: &[u8]) -> Vec<Glyph> {
        bytes
            .iter()
            .map(|&byte| match self.index_for_byte[usize::from(byte)] {
                Some(index) => Glyph::Standard(index),
                None => Glyph::Other(char::from(byte)),
            })
            .collect()
    }
}

/// A letter byte optionally followed by a tone byte. Windows CP-1258 uses the
/// same shape, with extra precomposed letters accepted on input only.
pub(crate) struct TwoByteCodec {
    word_for_index: &'static WordTable,
    index_for_single_byte: [Option<u8>; 256],
    index_for_pair: HashMap<u16, u8>,
    is_tone_byte: [bool; 256],
}

impl TwoByteCodec {
    pub(crate) fn new(word_for_index: &'static WordTable) -> Self {
        Self::with_extra_input_tables(word_for_index, &[])
    }

    pub(crate) fn with_extra_input_tables(
        word_for_index: &'static WordTable,
        extra_input_tables: &[&'static WordTable],
    ) -> Self {
        let mut codec = Self {
            word_for_index,
            index_for_single_byte: [None; 256],
            index_for_pair: HashMap::new(),
            is_tone_byte: [false; 256],
        };
        codec.learn_table(word_for_index, None);
        for extra_table in extra_input_tables {
            codec.learn_table(extra_table, Some(word_for_index));
        }
        codec
    }

    /// Records how to decode every entry of `table`, skipping entries equal to
    /// the same slot of `already_known`.
    fn learn_table(&mut self, table: &WordTable, already_known: Option<&WordTable>) {
        for (index, &word) in table.iter().enumerate() {
            if already_known.is_some_and(|known| known[index] == word) {
                continue;
            }
            let [low_byte, high_byte] = word.to_le_bytes();
            if high_byte != 0 {
                self.is_tone_byte[usize::from(high_byte)] = true;
                self.index_for_pair.entry(word).or_insert(index as u8);
            } else if !is_ascii_filler_for_symbol(index, low_byte) {
                let index = index as u8;
                self.index_for_single_byte[usize::from(low_byte)].get_or_insert(index);
            }
        }
    }

    pub(crate) fn encode(&self, glyphs: &[Glyph]) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(glyphs.len() * 2);
        for &glyph in glyphs {
            match glyph {
                Glyph::Standard(index) => self.push_standard(&mut bytes, index),
                Glyph::Other(character) => bytes.push(self.encode_other(character)),
            }
        }
        bytes
    }

    fn push_standard(&self, bytes: &mut Vec<u8>, index: u8) {
        let [low_byte, high_byte] = self.word_for_index[usize::from(index)].to_le_bytes();
        if high_byte != 0 {
            bytes.push(low_byte);
            bytes.push(high_byte);
        } else if self.is_tone_byte[usize::from(low_byte)] {
            bytes.push(PAD_BYTE);
        } else {
            bytes.push(low_byte);
        }
    }

    fn encode_other(&self, character: char) -> u8 {
        match u8::try_from(u32::from(character)) {
            Ok(byte)
                if self.index_for_single_byte[usize::from(byte)].is_none()
                    && !self.is_tone_byte[usize::from(byte)] =>
            {
                byte
            }
            _ => PAD_BYTE,
        }
    }

    pub(crate) fn decode(&self, bytes: &[u8]) -> Vec<Glyph> {
        let mut glyphs = Vec::with_capacity(bytes.len());
        let mut position = 0;
        while position < bytes.len() {
            let (glyph, length) = self.decode_at(bytes, position);
            glyphs.push(glyph);
            position += length;
        }
        glyphs
    }

    fn decode_at(&self, bytes: &[u8], position: usize) -> (Glyph, usize) {
        let low_byte = bytes[position];
        if let Some(&high_byte) = bytes.get(position + 1) {
            let word = u16::from_le_bytes([low_byte, high_byte]);
            if let Some(&index) = self.index_for_pair.get(&word) {
                return (Glyph::Standard(index), 2);
            }
        }
        match self.index_for_single_byte[usize::from(low_byte)] {
            Some(index) => (Glyph::Standard(index), 1),
            None => (Glyph::Other(char::from(low_byte)), 1),
        }
    }
}
