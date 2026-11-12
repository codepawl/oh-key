//! Replays the `bench/unikey-compare` corpus through oh-key and times it.
//!
//! `cargo run --release -p oh-key-engine --example compare -- <keys-file> <text-out> <timing-out>`
//!
//! The text output is what an application would show after the keys. The
//! timing file holds `median p10 p90` in nanoseconds per key over 20 replays
//! that follow a warmup.

use std::fs;
use std::hint::black_box;
use std::time::Instant;

use oh_key_engine::{Config, Engine, KeyResult};

const WARMUP_REPLAYS: usize = 3;
const TIMED_REPLAYS: usize = 20;

/// Feeds every key to the engine and applies the result to `text`, as a frontend does.
fn replay(engine: &mut Engine, keys: &[char], text: &mut String) {
    text.clear();
    engine.reset();
    for &key in keys {
        match engine.process_key(black_box(key)) {
            KeyResult::Composing => {}
            KeyResult::Commit(committed) => text.push_str(&committed),
            KeyResult::PassThrough => text.push(key),
        }
    }
    text.push_str(&engine.commit());
}

fn timed_nanoseconds_per_key(engine: &mut Engine, keys: &[char], text: &mut String) -> f64 {
    let started = Instant::now();
    replay(engine, keys, text);
    black_box(&text);
    started.elapsed().as_nanos() as f64 / keys.len() as f64
}

fn percentile(sorted: &[f64], percent: usize) -> f64 {
    sorted[(sorted.len() - 1) * percent / 100]
}

fn median(sorted: &[f64]) -> f64 {
    let middle = sorted.len() / 2;
    if sorted.len().is_multiple_of(2) {
        (sorted[middle - 1] + sorted[middle]) / 2.0
    } else {
        sorted[middle]
    }
}

fn main() {
    let arguments: Vec<String> = std::env::args().collect();
    let [_, keys_path, text_path, timing_path] = &arguments[..] else {
        eprintln!("usage: compare <keys-file> <text-out> <timing-out>");
        std::process::exit(2);
    };
    let keys: Vec<char> = fs::read_to_string(keys_path)
        .expect("read keys file")
        .chars()
        .collect();
    let mut engine = Engine::new(Config::default());
    let mut text = String::with_capacity(keys.len() * 2);
    for _ in 0..WARMUP_REPLAYS {
        replay(&mut engine, &keys, &mut text);
    }
    let mut samples: Vec<f64> = (0..TIMED_REPLAYS)
        .map(|_| timed_nanoseconds_per_key(&mut engine, &keys, &mut text))
        .collect();
    samples.sort_by(f64::total_cmp);
    fs::write(text_path, &text).expect("write text output");
    let timing = format!(
        "{:.2} {:.2} {:.2}\n",
        median(&samples),
        percentile(&samples, 10),
        percentile(&samples, 90)
    );
    fs::write(timing_path, timing).expect("write timing output");
    println!("oh-key replayed {} keys {TIMED_REPLAYS} times", keys.len());
}
