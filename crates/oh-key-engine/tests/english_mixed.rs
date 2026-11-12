//! English and code typed in Vietnamese Telex mode: keys in, text out.
//! The rows come from the mixed corpus of `bench/unikey-compare`.

use oh_key_engine::{Config, Engine, KeyResult};

fn typed(keys: &str) -> String {
    let mut engine = Engine::new(Config::default());
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

fn check(name: &str, cases: &[(&str, &str)]) {
    let failures: Vec<String> = cases
        .iter()
        .filter_map(|&(keys, expected)| {
            let actual = typed(keys);
            (actual != expected)
                .then(|| format!("  {keys:?}: expected {expected:?}, got {actual:?}"))
        })
        .collect();
    assert!(
        failures.is_empty(),
        "{name}: {} of {} rows failed\n{}",
        failures.len(),
        cases.len(),
        failures.join("\n")
    );
}

#[test]
fn english_words_with_doubled_keys_stay_as_typed() {
    check(
        "doubled keys",
        &[
            ("Russia", "Russia"),
            ("Russian", "Russian"),
            ("boss", "boss"),
            ("coffee", "coffee"),
            ("issue", "issue"),
            ("password", "password"),
            ("classroom", "classroom"),
            ("successfully", "successfully"),
            ("missing", "missing"),
            ("passion", "passion"),
            ("OFF", "OFF"),
            ("BOSS", "BOSS"),
            ("ISSUE", "ISSUE"),
            ("different", "different"),
        ],
    );
}

#[test]
fn bracket_keys_after_a_symbol_or_digit_are_typed_as_they_are() {
    check(
        "brackets in code",
        &[
            ("item[0]", "item[0]"),
            ("a[i]", "a[i]"),
            ("if(x>0){", "ì(x>0){"),
            ("foo(){}", "foo(){}"),
            ("x=y[1];", "x=y[1];"),
            ("map{k}", "map{k}"),
        ],
    );
}

#[test]
fn bracket_keys_still_make_horned_letters_after_a_space() {
    check(
        "brackets in prose",
        &[
            ("[i", "ơi"),
            ("] ", "ư "),
            ("xin [i ]", "xin ơi ư"),
            ("[[", "["),
            ("]]", "]"),
        ],
    );
}

#[test]
fn code_and_tech_tokens_survive() {
    check(
        "tech tokens",
        &[
            ("snake_case", "snake_case"),
            ("camelCase", "camelCase"),
            (
                "https://github.com/codepawl/oh-key",
                "https://github.com/codepawl/oh-key",
            ),
            ("mai.lan@gmail.com", "mai.lan@gmail.com"),
            ("v1.2.3", "v1.2.3"),
            ("--release", "--release"),
            ("package.json", "package.json"),
            ("localhost:8080", "localhost:8080"),
            ("node_modules", "node_modules"),
            ("Cargo.toml", "Cargo.toml"),
        ],
    );
}

#[test]
fn mixed_sentences_keep_both_languages() {
    check(
        "mixed sentences",
        &[
            ("deadline thuws hai", "deadline thứ hai"),
            ("check mail ddi", "check mail đi"),
            ("hopj online lucs 3h", "họp online lúc 3h"),
            ("file excel bij looxi", "file excel bị lỗi"),
            ("gui file pdf, fix bug roofi", "gui file pdf, fix bug rồi"),
        ],
    );
}

/// English words whose Telex reading is a real Vietnamese syllable. The engine
/// turns them into Vietnamese; this table pins that policy so a change shows up.
#[test]
fn ambiguous_english_words_read_as_vietnamese() {
    check(
        "ambiguous words",
        &[
            ("as", "á"),
            ("is", "í"),
            ("of", "ò"),
            ("or", "ỏ"),
            ("if", "ì"),
            ("to", "to"),
            ("me", "me"),
            ("he", "he"),
            ("an", "an"),
            ("on", "on"),
            ("phone", "phone"),
            ("box", "bõ"),
            ("see", "sê"),
            ("how", "hơ"),
        ],
    );
}
