//! Prints the speed table and the word-by-word differences of the comparison run.
//!
//! `cargo run --release -p oh-key-engine --example compare_report -- <keys> <expected>
//!  <unikey-text> <unikey-timing> <ohkey-text> <ohkey-timing>`

use std::collections::HashMap;
use std::fs;

const LISTED_DIFFERENCES_PER_GROUP: [usize; 4] = [10, 10, 5, 5];

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
enum Group {
    OnlyOhKeyCorrect,
    OnlyUniKeyCorrect,
    EnginesDifferBothWrong,
    EnginesAgreeBothWrong,
}

impl Group {
    const ALL: [Group; 4] = [
        Group::OnlyOhKeyCorrect,
        Group::OnlyUniKeyCorrect,
        Group::EnginesDifferBothWrong,
        Group::EnginesAgreeBothWrong,
    ];

    fn title(self) -> &'static str {
        match self {
            Group::OnlyOhKeyCorrect => "oh-key matches the expected text, UniKey does not",
            Group::OnlyUniKeyCorrect => "UniKey matches the expected text, oh-key does not",
            Group::EnginesDifferBothWrong => {
                "both differ from the expected text and from each other"
            }
            Group::EnginesAgreeBothWrong => {
                "both agree with each other but not with the expected text"
            }
        }
    }

    fn position(self) -> usize {
        Group::ALL
            .iter()
            .position(|group| *group == self)
            .unwrap_or(0)
    }
}

struct Timing {
    median: f64,
    p10: f64,
    p90: f64,
}

fn read_timing(path: &str) -> Timing {
    let text = fs::read_to_string(path).expect("read timing file");
    let values: Vec<f64> = text
        .split_whitespace()
        .map(|value| value.parse().expect("timing number"))
        .collect();
    Timing {
        median: values[0],
        p10: values[1],
        p90: values[2],
    }
}

fn words(path: &str) -> Vec<String> {
    let text = fs::read_to_string(path).expect("read text file");
    text.split_whitespace().map(str::to_owned).collect()
}

fn classify(expected: &str, unikey: &str, ohkey: &str) -> Option<Group> {
    match (unikey == expected, ohkey == expected, unikey == ohkey) {
        (true, true, _) => None,
        (false, true, _) => Some(Group::OnlyOhKeyCorrect),
        (true, false, _) => Some(Group::OnlyUniKeyCorrect),
        (false, false, false) => Some(Group::EnginesDifferBothWrong),
        (false, false, true) => Some(Group::EnginesAgreeBothWrong),
    }
}

fn print_speed_table(unikey: &Timing, ohkey: &Timing) {
    println!("{:<22}{:>12}{:>12}", "ns per key", "UniKey", "oh-key");
    println!(
        "{:<22}{:>12.1}{:>12.1}",
        "median", unikey.median, ohkey.median
    );
    println!("{:<22}{:>12.1}{:>12.1}", "p10", unikey.p10, ohkey.p10);
    println!("{:<22}{:>12.1}{:>12.1}", "p90", unikey.p90, ohkey.p90);
    println!(
        "oh-key / UniKey median: {:.2}x\n",
        ohkey.median / unikey.median
    );
}

type DistinctKey = (String, String, String, String);

fn print_group(group: Group, total: usize, distinct: &HashMap<DistinctKey, usize>) {
    println!(
        "\n{} ({total} words, {} distinct)",
        group.title(),
        distinct.len()
    );
    let mut entries: Vec<(&DistinctKey, &usize)> = distinct.iter().collect();
    entries.sort_by(|left, right| right.1.cmp(left.1).then(left.0.cmp(right.0)));
    for ((keys, expected, unikey, ohkey), count) in entries
        .into_iter()
        .take(LISTED_DIFFERENCES_PER_GROUP[group.position()])
    {
        println!(
            "  {count:>5}x  keys {keys}  expected {expected}  UniKey {unikey}  oh-key {ohkey}"
        );
    }
}

fn main() {
    let arguments: Vec<String> = std::env::args().collect();
    let [
        _,
        keys_path,
        expected_path,
        unikey_text,
        unikey_timing,
        ohkey_text,
        ohkey_timing,
    ] = &arguments[..]
    else {
        eprintln!(
            "usage: compare_report <keys> <expected> <unikey-text> <unikey-timing> <ohkey-text> <ohkey-timing>"
        );
        std::process::exit(2);
    };
    print_speed_table(&read_timing(unikey_timing), &read_timing(ohkey_timing));

    let keys = words(keys_path);
    let expected = words(expected_path);
    let unikey = words(unikey_text);
    let ohkey = words(ohkey_text);
    println!(
        "words: corpus {}, UniKey output {}, oh-key output {}",
        keys.len(),
        unikey.len(),
        ohkey.len()
    );
    if [expected.len(), unikey.len(), ohkey.len()]
        .iter()
        .any(|count| *count != keys.len())
    {
        println!("word counts differ, so a word-by-word comparison is not possible");
        std::process::exit(1);
    }

    let mut group_totals: HashMap<Group, usize> = HashMap::new();
    let mut distinct_by_group: HashMap<Group, HashMap<DistinctKey, usize>> = HashMap::new();
    let mut differing_words = 0;
    for index in 0..keys.len() {
        if unikey[index] != ohkey[index] {
            differing_words += 1;
        }
        let Some(group) = classify(&expected[index], &unikey[index], &ohkey[index]) else {
            continue;
        };
        *group_totals.entry(group).or_default() += 1;
        let identity = (
            keys[index].clone(),
            expected[index].clone(),
            unikey[index].clone(),
            ohkey[index].clone(),
        );
        *distinct_by_group
            .entry(group)
            .or_default()
            .entry(identity)
            .or_default() += 1;
    }
    let wrong_unikey = (0..keys.len())
        .filter(|index| unikey[*index] != expected[*index])
        .count();
    let wrong_ohkey = (0..keys.len())
        .filter(|index| ohkey[*index] != expected[*index])
        .count();
    println!("words where UniKey and oh-key differ: {differing_words}");
    println!(
        "words that differ from the expected text: UniKey {wrong_unikey}, oh-key {wrong_ohkey}"
    );
    for group in Group::ALL {
        let total = group_totals.get(&group).copied().unwrap_or(0);
        let empty = HashMap::new();
        print_group(
            group,
            total,
            distinct_by_group.get(&group).unwrap_or(&empty),
        );
    }
}
