//! Prints per-category results of the mixed corpus as markdown tables.
//!
//! `cargo run --release -p oh-key-engine --example mixed_report -- <keys> <expected> <categories>
//!  <unikey-text> <ohkey-text> [timings: <unikey-timing> <ohkey-timing>]`

use std::collections::{BTreeMap, HashMap};
use std::fs;

const CATEGORY_NAMES: [(char, &str); 7] = [
    ('a', "a. English words that collide with Telex keys"),
    (
        'b',
        "b. English words that are also Telex Vietnamese (ambiguous)",
    ),
    ('c', "c. Brands and proper nouns"),
    ('d', "d. Code and tech tokens"),
    ('e', "e. Mixed Vietnamese / English sentences"),
    ('f', "f. All caps and capitalised English"),
    ('g', "g. English followed by punctuation"),
];
const DEFAULT_LISTED_FAILURES: usize = 8;

fn listed_failures() -> usize {
    std::env::var("MIXED_REPORT_ROWS")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(DEFAULT_LISTED_FAILURES)
}

fn words(path: &str) -> Vec<String> {
    let text = fs::read_to_string(path).expect("read text file");
    text.split_whitespace().map(str::to_owned).collect()
}

#[derive(Default)]
struct CategoryTotals {
    occurrences: usize,
    distinct_tokens: BTreeMap<String, ()>,
    unikey_wrong: usize,
    ohkey_wrong: usize,
    unikey_failures: HashMap<(String, String, String), usize>,
    ohkey_failures: HashMap<(String, String, String), usize>,
    ohkey_only_failures: HashMap<(String, String, String), usize>,
}

fn print_failures(title: &str, failures: &HashMap<(String, String, String), usize>) {
    let mut entries: Vec<_> = failures.iter().collect();
    entries.sort_by(|left, right| right.1.cmp(left.1).then(left.0.cmp(right.0)));
    println!("\n{title} ({} distinct wrong tokens)\n", entries.len());
    println!("| keys typed | expected | produced |\n|---|---|---|");
    for ((keys, expected, produced), _) in entries.into_iter().take(listed_failures()) {
        println!("| `{keys}` | `{expected}` | `{produced}` |");
    }
}

fn read_timing(path: &str) -> f64 {
    let text = fs::read_to_string(path).expect("read timing file");
    text.split_whitespace()
        .next()
        .and_then(|value| value.parse().ok())
        .expect("timing number")
}

fn main() {
    let arguments: Vec<String> = std::env::args().collect();
    if arguments.len() < 6 {
        eprintln!("usage: mixed_report <keys> <expected> <categories> <unikey> <ohkey> [t1 t2]");
        std::process::exit(2);
    }
    let keys = words(&arguments[1]);
    let expected = words(&arguments[2]);
    let categories = words(&arguments[3]);
    let unikey = words(&arguments[4]);
    let ohkey = words(&arguments[5]);
    let lengths = [
        keys.len(),
        expected.len(),
        categories.len(),
        unikey.len(),
        ohkey.len(),
    ];
    if lengths.iter().any(|length| *length != keys.len()) {
        println!("word counts differ: {lengths:?}");
        std::process::exit(1);
    }
    if arguments.len() >= 8 {
        println!(
            "Speed on this corpus (median ns/key): UniKey {:.1}, oh-key {:.1}\n",
            read_timing(&arguments[6]),
            read_timing(&arguments[7])
        );
    }
    let mut totals: BTreeMap<char, CategoryTotals> = BTreeMap::new();
    for index in 0..keys.len() {
        let letter = categories[index].chars().next().unwrap_or('?');
        let entry = totals.entry(letter).or_default();
        entry.occurrences += 1;
        entry.distinct_tokens.insert(keys[index].clone(), ());
        let identity = |produced: &str| {
            (
                keys[index].clone(),
                expected[index].clone(),
                produced.to_owned(),
            )
        };
        if unikey[index] != expected[index] {
            entry.unikey_wrong += 1;
            *entry
                .unikey_failures
                .entry(identity(&unikey[index]))
                .or_default() += 1;
        }
        if ohkey[index] != expected[index] {
            entry.ohkey_wrong += 1;
            if unikey[index] == expected[index] {
                *entry
                    .ohkey_only_failures
                    .entry(identity(&ohkey[index]))
                    .or_default() += 1;
            }
            *entry
                .ohkey_failures
                .entry(identity(&ohkey[index]))
                .or_default() += 1;
        }
    }
    println!(
        "| category | tokens | distinct | oh-key wrong | UniKey wrong | oh-key wrong (distinct) | UniKey wrong (distinct) |"
    );
    println!("|---|---:|---:|---:|---:|---:|---:|");
    for (letter, name) in CATEGORY_NAMES {
        let Some(entry) = totals.get(&letter) else {
            continue;
        };
        println!(
            "| {name} | {} | {} | {} | {} | {} | {} |",
            entry.occurrences,
            entry.distinct_tokens.len(),
            entry.ohkey_wrong,
            entry.unikey_wrong,
            entry.ohkey_failures.len(),
            entry.unikey_failures.len()
        );
    }
    for (letter, name) in CATEGORY_NAMES {
        let Some(entry) = totals.get(&letter) else {
            continue;
        };
        println!("\n### {name}");
        print_failures("oh-key wrong, UniKey right", &entry.ohkey_only_failures);
        print_failures("oh-key", &entry.ohkey_failures);
        print_failures("UniKey", &entry.unikey_failures);
    }
    print_ambiguous_table(&keys, &categories, &unikey, &ohkey);
}

/// Category b: every distinct word with what each engine produced.
fn print_ambiguous_table(
    keys: &[String],
    categories: &[String],
    unikey: &[String],
    ohkey: &[String],
) {
    let mut rows: BTreeMap<&str, (&str, &str)> = BTreeMap::new();
    for index in 0..keys.len() {
        if categories[index] == "b" {
            rows.insert(&keys[index], (&unikey[index], &ohkey[index]));
        }
    }
    println!("\n### Ambiguous words, full table\n");
    println!("| typed | UniKey | oh-key |\n|---|---|---|");
    for (typed, (unikey_text, ohkey_text)) in rows {
        let marker = |text: &str| {
            if text == typed {
                "same".to_owned()
            } else {
                format!("`{text}`")
            }
        };
        println!(
            "| `{typed}` | {} | {} |",
            marker(unikey_text),
            marker(ohkey_text)
        );
    }
}
