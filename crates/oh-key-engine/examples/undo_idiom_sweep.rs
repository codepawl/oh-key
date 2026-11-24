//! Sweeps an English word list for words that Telex typing loses. Each word is typed
//! - plainly,
//! - the way a person fixes a mangled letter: pressing the mark key again as soon as it
//!   changes a letter ("pas" shows "pá", so "pass" then "te" for "paste"),
//! - with a third press where the word has the letter twice ("pas", "pass" on screen, so the
//!   keys are "passs"),
//!
//! in lower case, Capitalised and UPPER CASE, ended by a space, by punctuation and by the
//! host committing the word (Enter, a click), and once more with the last letter deleted and
//! typed again. Every way must give the word.
//!
//! `cargo run -p oh-key-engine --example undo_idiom_sweep -- <frequency list> [limit]`
//! The list has one `word count` pair per line (the count may be missing), most common
//! first; it is never committed. Failures go to stdout as tab-separated rows, the totals per
//! way of typing to stderr.

use std::collections::BTreeMap;
use std::collections::HashMap;

use oh_key_engine::{Engine, KeyResult};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Typing {
    Plain,
    MarksUndone,
    ThirdPress,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Case {
    Lower,
    Capitalised,
    Upper,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Ending {
    Space,
    FullStop,
    Comma,
    Committed,
    /// The last letter deleted and typed again, then a space.
    RetypedLastLetter,
}

const TYPINGS: [Typing; 3] = [Typing::Plain, Typing::MarksUndone, Typing::ThirdPress];
const CASES: [Case; 3] = [Case::Lower, Case::Capitalised, Case::Upper];
const ENDINGS: [Ending; 5] = [
    Ending::Space,
    Ending::FullStop,
    Ending::Comma,
    Ending::Committed,
    Ending::RetypedLastLetter,
];

fn feed(engine: &mut Engine, visible: &mut String, key: char) {
    match engine.process_key(key) {
        KeyResult::Composing => {}
        KeyResult::Commit(text) => visible.push_str(&text),
        KeyResult::PassThrough => visible.push(key),
    }
}

/// What a text field shows after the keys and the ending.
fn typed(keys: &str, ending: Ending) -> String {
    let mut engine = Engine::default();
    let mut visible = String::new();
    for key in keys.chars() {
        feed(&mut engine, &mut visible, key);
    }
    match ending {
        Ending::Space => feed(&mut engine, &mut visible, ' '),
        Ending::FullStop => feed(&mut engine, &mut visible, '.'),
        Ending::Comma => feed(&mut engine, &mut visible, ','),
        Ending::Committed => {}
        Ending::RetypedLastLetter => {
            engine.backspace();
            if let Some(last_key) = keys.chars().last() {
                feed(&mut engine, &mut visible, last_key);
            }
            feed(&mut engine, &mut visible, ' ');
        }
    }
    visible.push_str(&engine.commit());
    visible
}

fn expected_text(word: &str, ending: Ending) -> String {
    let tail = match ending {
        Ending::Space | Ending::RetypedLastLetter => " ",
        Ending::FullStop => ".",
        Ending::Comma => ",",
        Ending::Committed => "",
    };
    format!("{word}{tail}")
}

/// The keys of a person who presses a key again the moment it changes a letter instead of
/// typing itself. A doubled letter of the word already is that second press; with
/// `Typing::ThirdPress` the person presses once more to see both letters.
fn keys_for(word: &str, typing: Typing) -> String {
    if typing == Typing::Plain {
        return word.to_owned();
    }
    let letters: Vec<char> = word.chars().collect();
    let mut engine = Engine::default();
    let mut keys = String::new();
    let mut index = 0;
    while index < letters.len() {
        let key = letters[index];
        let mut unchanged = engine.preedit().to_owned();
        unchanged.push(key);
        engine.process_key(key);
        keys.push(key);
        let changed_a_letter = engine.preedit() != unchanged;
        let same_key_follows = letters
            .get(index + 1)
            .is_some_and(|next| next.eq_ignore_ascii_case(&key));
        if changed_a_letter && same_key_follows {
            let second_key = letters[index + 1];
            engine.process_key(second_key);
            keys.push(second_key);
            index += 1;
            let shows_one_letter = !engine.preedit().ends_with([key, second_key]);
            if typing == Typing::ThirdPress && shows_one_letter {
                engine.process_key(second_key);
                keys.push(second_key);
            }
        } else if changed_a_letter && !engine.preedit().is_ascii() {
            engine.process_key(key);
            keys.push(key);
        }
        index += 1;
    }
    keys
}

fn in_case(word: &str, case: Case) -> String {
    match case {
        Case::Lower => word.to_owned(),
        Case::Upper => word.to_ascii_uppercase(),
        Case::Capitalised => {
            let mut letters = word.chars();
            let first = letters.next().map(|letter| letter.to_ascii_uppercase());
            first.into_iter().chain(letters).collect()
        }
    }
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
            let mut parts = line.split_whitespace();
            let word = parts.next()?;
            let count = parts.next().and_then(|text| text.parse().ok()).unwrap_or(0);
            let is_word = word.len() > 1 && word.chars().all(|letter| letter.is_ascii_lowercase());
            is_word.then_some((word, count))
        })
        .take(limit)
        .collect();
    let counts: HashMap<&str, u64> = entries.iter().copied().collect();
    let mut failures: BTreeMap<(Typing, Case, Ending), usize> = BTreeMap::new();
    for &(lowercase_word, count) in &entries {
        for case in CASES {
            let word = in_case(lowercase_word, case);
            for typing in TYPINGS {
                let keys = keys_for(&word, typing);
                if typing != Typing::Plain && keys == keys_for(&word, Typing::Plain) {
                    continue;
                }
                let same_as_marks_undone =
                    typing == Typing::ThirdPress && keys == keys_for(&word, Typing::MarksUndone);
                if same_as_marks_undone {
                    continue;
                }
                for ending in ENDINGS {
                    let got = typed(&keys, ending);
                    if got == expected_text(&word, ending) {
                        continue;
                    }
                    *failures.entry((typing, case, ending)).or_default() += 1;
                    let got_word = got.trim_end_matches([' ', '.', ',']).to_ascii_lowercase();
                    let got_count = counts.get(got_word.as_str()).copied().unwrap_or(0);
                    println!(
                        "{typing:?}\t{case:?}\t{ending:?}\t{word}\t{count}\t{keys}\t{got}\t{got_count}"
                    );
                }
            }
        }
    }
    eprintln!("{} words", entries.len());
    for ((typing, case, ending), total) in &failures {
        eprintln!("{total:>7}  {typing:?} {case:?} {ending:?}");
    }
}
