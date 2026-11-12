//! Unicode-based charsets: UTF-16LE (precomposed and decomposed), UTF-8,
//! numeric character references and C string escapes.

use crate::standard::{Glyph, char_from_glyph, glyphs_from_text};
use crate::tables::UNICODE_DECOMPOSED;

pub(crate) fn encode_utf16_little_endian(glyphs: &[Glyph]) -> Vec<u8> {
    let text: String = glyphs.iter().copied().map(char_from_glyph).collect();
    text.encode_utf16().flat_map(u16::to_le_bytes).collect()
}

/// Letter first, then the combining tone mark, as Unikey writes "UNI-COMP".
pub(crate) fn encode_utf16_decomposed(glyphs: &[Glyph]) -> Vec<u8> {
    let mut units: Vec<u16> = Vec::with_capacity(glyphs.len() * 2);
    for &glyph in glyphs {
        match glyph {
            Glyph::Standard(index) => {
                let composite = UNICODE_DECOMPOSED[usize::from(index)];
                units.push((composite & 0xFFFF) as u16);
                if composite >> 16 != 0 {
                    units.push((composite >> 16) as u16);
                }
            }
            Glyph::Other(character) => {
                let mut buffer = [0u16; 2];
                units.extend_from_slice(character.encode_utf16(&mut buffer));
            }
        }
    }
    units.into_iter().flat_map(u16::to_le_bytes).collect()
}

pub(crate) fn decode_utf16_little_endian(bytes: &[u8]) -> Vec<Glyph> {
    let units = bytes
        .as_chunks::<2>()
        .0
        .iter()
        .map(|&pair| u16::from_le_bytes(pair));
    let text: String = char::decode_utf16(units)
        .map(|unit| unit.unwrap_or('\u{FFFD}'))
        .collect();
    glyphs_from_text(&text)
}

pub(crate) fn encode_utf8(glyphs: &[Glyph]) -> Vec<u8> {
    let text: String = glyphs.iter().copied().map(char_from_glyph).collect();
    text.into_bytes()
}

pub(crate) fn decode_utf8(bytes: &[u8]) -> Vec<Glyph> {
    glyphs_from_text(&String::from_utf8_lossy(bytes))
}

/// `&#7879;` for anything outside ASCII.
pub(crate) fn encode_decimal_references(glyphs: &[Glyph]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(glyphs.len());
    for &glyph in glyphs {
        let character = char_from_glyph(glyph);
        if character.is_ascii() {
            bytes.push(character as u8);
        } else {
            bytes.extend_from_slice(format!("&#{};", u32::from(character)).as_bytes());
        }
    }
    bytes
}

/// `&#x1EC7;` for anything above U+00FF; U+0080..U+00FF go out as one raw byte.
pub(crate) fn encode_hexadecimal_references(glyphs: &[Glyph]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(glyphs.len());
    for &glyph in glyphs {
        let character = char_from_glyph(glyph);
        match u8::try_from(u32::from(character)) {
            Ok(byte) => bytes.push(byte),
            Err(_) => {
                bytes.extend_from_slice(format!("&#x{:X};", u32::from(character)).as_bytes());
            }
        }
    }
    bytes
}

/// Decodes decimal and hexadecimal references. Other bytes are Latin-1.
pub(crate) fn decode_references(bytes: &[u8]) -> Vec<Glyph> {
    let mut glyphs = Vec::with_capacity(bytes.len());
    let mut position = 0;
    while position < bytes.len() {
        let (character, length) =
            reference_at(bytes, position).unwrap_or((char::from(bytes[position]), 1));
        glyphs.push(crate::standard::glyph_from_char(character));
        position += length;
    }
    glyphs
}

fn reference_at(bytes: &[u8], position: usize) -> Option<(char, usize)> {
    let rest = bytes[position..].strip_prefix(b"&#")?;
    let (radix, digits_start) = match rest.first()? {
        b'x' | b'X' => (16, 1),
        _ => (10, 0),
    };
    let digits = &rest[digits_start..];
    let digit_count = digits
        .iter()
        .take_while(|&&byte| char::from(byte).is_digit(radix))
        .count();
    if digit_count == 0 || digit_count > 7 || digits.get(digit_count) != Some(&b';') {
        return None;
    }
    let digit_text = std::str::from_utf8(&digits[..digit_count]).ok()?;
    let character = char::from_u32(u32::from_str_radix(digit_text, radix).ok()?)?;
    Some((character, 2 + digits_start + digit_count + 1))
}

/// `\x1EC7` escapes. Hex digits, `x` and `\` are escaped too, so that an
/// escape can never run into the text after it.
pub(crate) fn encode_c_string(glyphs: &[Glyph]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(glyphs.len());
    for &glyph in glyphs {
        let character = char_from_glyph(glyph);
        let needs_escape = !character.is_ascii()
            || character.is_ascii_hexdigit()
            || matches!(character, 'x' | 'X' | '\\');
        if needs_escape {
            bytes.extend_from_slice(format!("\\x{:X}", u32::from(character)).as_bytes());
        } else {
            bytes.push(character as u8);
        }
    }
    bytes
}

pub(crate) fn decode_c_string(bytes: &[u8]) -> Vec<Glyph> {
    let mut glyphs = Vec::with_capacity(bytes.len());
    let mut position = 0;
    while position < bytes.len() {
        let (character, length) =
            c_escape_at(bytes, position).unwrap_or((char::from(bytes[position]), 1));
        glyphs.push(crate::standard::glyph_from_char(character));
        position += length;
    }
    glyphs
}

fn c_escape_at(bytes: &[u8], position: usize) -> Option<(char, usize)> {
    let rest = bytes[position..]
        .strip_prefix(b"\\x")
        .or_else(|| bytes[position..].strip_prefix(b"\\X"))?;
    let digit_count = rest
        .iter()
        .take_while(|byte| byte.is_ascii_hexdigit())
        .count()
        .min(6);
    if digit_count == 0 {
        return None;
    }
    let digit_text = std::str::from_utf8(&rest[..digit_count]).ok()?;
    let character = char::from_u32(u32::from_str_radix(digit_text, 16).ok()?)?;
    Some((character, 2 + digit_count))
}
