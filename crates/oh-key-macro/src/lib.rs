//! Unikey-compatible macro (gõ tắt) table.
//!
//! File format (version 1): an optional UTF-8 BOM, a header line containing
//! `*** version=1 ***`, then one `key:text` per line, split at the first `:`.
//! A file without the header is version 0: VIQR text, read as such.
//!
//! Differences from Unikey: a key typed twice keeps the later text only, and
//! the 128 KB memory cap is not modelled.

#![forbid(unsafe_code)]

use std::collections::HashMap;
use std::fmt;
use std::io;
use std::path::Path;

use oh_key_charset::{Charset, decode, normalize_to_precomposed};

pub const MAX_KEY_LENGTH: usize = 15;
pub const MAX_TEXT_LENGTH: usize = 1024;
pub const MAX_ITEMS: usize = 1024;

const FILE_VERSION: u32 = 1;
const BYTE_ORDER_MARK: char = '\u{FEFF}';
const HEADER_LINE: &str = ";DO NOT DELETE THIS LINE*** version=1 ***";

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MacroError {
    EmptyKey,
    TableFull,
}

impl fmt::Display for MacroError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MacroError::EmptyKey => write!(formatter, "macro key is empty"),
            MacroError::TableFull => {
                write!(formatter, "macro table holds {MAX_ITEMS} items already")
            }
        }
    }
}

impl std::error::Error for MacroError {}

#[derive(Clone, Debug, PartialEq, Eq)]
struct MacroItem {
    key: String,
    text: String,
}

/// A parsed macro file together with the version it was stored in.
#[derive(Debug)]
pub struct LoadedMacros {
    pub table: MacroTable,
    /// 0 for legacy VIQR files, which callers should save again as version 1.
    pub version: u32,
}

#[derive(Clone, Debug, Default)]
pub struct MacroTable {
    items_by_folded_key: HashMap<String, MacroItem>,
}

/// Lower case, with letters and combining marks joined, so that keys typed
/// by the engine and keys read from a file compare equal.
fn fold_key(key: &str) -> String {
    normalize_to_precomposed(key).to_lowercase()
}

impl MacroTable {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn len(&self) -> usize {
        self.items_by_folded_key.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items_by_folded_key.is_empty()
    }

    /// Adds or replaces a macro. The key is cut to [`MAX_KEY_LENGTH`] characters
    /// and the text to [`MAX_TEXT_LENGTH`].
    pub fn insert(&mut self, key: &str, text: &str) -> Result<(), MacroError> {
        let key: String = normalize_to_precomposed(key)
            .chars()
            .take(MAX_KEY_LENGTH)
            .collect();
        if key.is_empty() {
            return Err(MacroError::EmptyKey);
        }
        let folded_key = fold_key(&key);
        let is_replacement = self.items_by_folded_key.contains_key(&folded_key);
        if !is_replacement && self.len() >= MAX_ITEMS {
            return Err(MacroError::TableFull);
        }
        let text: String = normalize_to_precomposed(text)
            .chars()
            .take(MAX_TEXT_LENGTH)
            .collect();
        self.items_by_folded_key
            .insert(folded_key, MacroItem { key, text });
        Ok(())
    }

    pub fn remove(&mut self, key: &str) -> bool {
        self.items_by_folded_key.remove(&fold_key(key)).is_some()
    }

    /// The stored replacement for `key`, ignoring case, exactly as stored.
    pub fn lookup(&self, key: &str) -> Option<&str> {
        self.items_by_folded_key
            .get(&fold_key(key))
            .map(|item| item.text.as_str())
    }

    /// The replacement for what was just typed, adapted to its case like
    /// Unikey: a lower case first letter gives all lower case text; an
    /// all-capitals key gives all capitals; anything else leaves the text as
    /// stored.
    pub fn expand(&self, typed: &str) -> Option<String> {
        let stored = self.lookup(typed)?;
        Some(match case_of(typed) {
            TypedCase::Lower => stored.to_lowercase(),
            TypedCase::AllCapitals => stored.to_uppercase(),
            TypedCase::Unchanged => stored.to_owned(),
        })
    }

    /// Keys and texts in the order they are written to a file.
    pub fn items(&self) -> Vec<(&str, &str)> {
        let mut folded_items: Vec<(&String, &MacroItem)> =
            self.items_by_folded_key.iter().collect();
        folded_items.sort_by(|left, right| left.0.cmp(right.0));
        folded_items
            .into_iter()
            .map(|(_, item)| (item.key.as_str(), item.text.as_str()))
            .collect()
    }

    /// Parses the bytes of a macro file, version 0 or 1. Lines without a
    /// colon or with an empty key are skipped.
    pub fn parse(file_bytes: &[u8]) -> LoadedMacros {
        let (version, body) = split_header(file_bytes);
        let text = if version == FILE_VERSION {
            String::from_utf8_lossy(body).into_owned()
        } else {
            decode(body, Charset::Viqr)
        };
        let mut table = MacroTable::new();
        for line in text.split('\n') {
            let line = line.strip_suffix('\r').unwrap_or(line);
            if let Some((key, macro_text)) = line.split_once(':') {
                // A full table drops the remaining lines, like Unikey.
                let _ = table.insert(key, macro_text);
            }
        }
        LoadedMacros { table, version }
    }

    /// The version 1 file contents.
    pub fn serialize(&self) -> String {
        let mut contents = String::new();
        contents.push(BYTE_ORDER_MARK);
        contents.push_str(HEADER_LINE);
        contents.push('\n');
        let lines: Vec<String> = self
            .items()
            .into_iter()
            .map(|(key, text)| format!("{key}:{text}"))
            .collect();
        contents.push_str(&lines.join("\n"));
        contents
    }

    pub fn load(path: &Path) -> io::Result<LoadedMacros> {
        Ok(Self::parse(&std::fs::read(path)?))
    }

    pub fn save(&self, path: &Path) -> io::Result<()> {
        std::fs::write(path, self.serialize())
    }
}

enum TypedCase {
    Lower,
    AllCapitals,
    Unchanged,
}

fn case_of(typed: &str) -> TypedCase {
    let mut characters = typed.chars();
    match characters.next() {
        Some(first) if first.is_lowercase() => TypedCase::Lower,
        Some(first) if first.is_uppercase() => {
            if characters.any(char::is_lowercase) {
                TypedCase::Unchanged
            } else {
                TypedCase::AllCapitals
            }
        }
        _ => TypedCase::Unchanged,
    }
}

/// Splits a file into its version and the lines after the header. A file
/// without a header is version 0 and keeps all its lines.
fn split_header(file_bytes: &[u8]) -> (u32, &[u8]) {
    let first_line_end = file_bytes
        .iter()
        .position(|&byte| byte == b'\n')
        .map_or(file_bytes.len(), |position| position + 1);
    let first_line = String::from_utf8_lossy(&file_bytes[..first_line_end]);
    match version_in_header(first_line.trim_start_matches(BYTE_ORDER_MARK)) {
        Some(version) => (version, &file_bytes[first_line_end..]),
        None => (0, file_bytes),
    }
}

fn version_in_header(line: &str) -> Option<u32> {
    let after_stars = line.split_once("***")?.1.trim_start_matches(' ');
    let digits = after_stars.strip_prefix("version=")?;
    let digit_count = digits.chars().take_while(char::is_ascii_digit).count();
    digits[..digit_count].parse().ok()
}

#[cfg(test)]
mod tests;
