//! Sweeps an English word list for words that Telex typing loses: each word is typed plainly
//! and the way a person fixes a mangled letter (pressing the mark key again as soon as it
//! changes a letter), and must come out as the word both times.
//!
//! `cargo run -p oh-key-engine --example undo_idiom_sweep -- <frequency list> [limit]`
//! The list has one `word count` pair per line, most common first; it is never committed.

use std::collections::HashMap;

use oh_key_engine::{Engine, KeyResult};

fn typed(keys: &str) -> String {
    let mut engine = Engine::default();
    let mut visible = String::new();
    for key in keys.chars() {
        match engine.process_key(key) {
            KeyResult::Composing => {}
            KeyResult::Commit(text) => visible.push_str(&text),
            KeyResult::PassThrough => visible.push(key),
        }
    }
    visible.push_str(&engine.commit());
    visible
}

/// The keys of a person who presses a key again the moment it changes a letter instead of
/// typing itself. A doubled letter of the word itself already is that second press.
fn keys_with_marks_undone(word: &str) -> String {
    let letters: Vec<char> = word.chars().collect();
    let mut engine = Engine::default();
    let mut keys = String::new();
    let mut index = 0;
    while index < letters.len() {
        let key = letters[index];
        let mut expected = engine.preedit().to_owned();
        expected.push(key);
        engine.process_key(key);
        keys.push(key);
        let changed_a_letter = engine.preedit() != expected;
        let word_doubles_it = letters.get(index + 1) == Some(&key);
        if changed_a_letter && word_doubles_it {
            engine.process_key(key);
            keys.push(key);
            index += 1;
        } else if changed_a_letter && !engine.preedit().is_ascii() {
            engine.process_key(key);
            keys.push(key);
        }
        index += 1;
    }
    keys
}

fn main() {
    let mut arguments = std::env::args().skip(1);
    let path = arguments.next().expect("path of the frequency list");
    let limit: usize = arguments
        .next()
        .map_or(usize::MAX, |text| text.parse().expect("limit is a number"));
    let list = std::fs::read_to_string(path).expect("the frequency list is readable");
    let entries: Vec<(&str, u64)> = list
        .lines()
        .filter_map(|line| {
            let (word, count) = line.split_once(' ')?;
            let is_word = word.len() > 1 && word.chars().all(|letter| letter.is_ascii_lowercase());
            is_word.then_some((word, count.trim().parse().ok()?))
        })
        .take(limit)
        .collect();
    let counts: HashMap<&str, u64> = entries.iter().copied().collect();
    let mut plain_failures = 0;
    let mut undo_failures = 0;
    for &(word, count) in &entries {
        let expected = format!("{word} ");
        let plain = typed(&expected);
        if plain != expected {
            plain_failures += 1;
            println!("plain\t{word}\t{count}\t{word}\t{}", plain.trim_end());
        }
        let keys = keys_with_marks_undone(word);
        if keys == word {
            continue;
        }
        let undone = typed(&format!("{keys} "));
        if undone != expected {
            undo_failures += 1;
            let got = undone.trim_end();
            let got_count = counts.get(got).copied().unwrap_or(0);
            println!("undo\t{word}\t{count}\t{keys}\t{got}\t{got_count}");
        }
    }
    eprintln!(
        "{} words: {plain_failures} plain failures, {undo_failures} undo failures",
        entries.len()
    );
}
