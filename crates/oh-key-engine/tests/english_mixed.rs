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
            ("OFFF", "OFF"),
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

#[test]
fn a_mark_key_typed_again_later_gives_the_word_back() {
    check(
        "mark key typed again later",
        &[
            ("posts", "posts"),
            ("basis", "basis"),
            ("horses", "horses"),
            ("versus", "versus"),
            ("seemed", "seemed"),
            ("keeper", "keeper"),
            ("downward", "downward"),
            ("Tests", "Tests"),
            ("arizona", "arizona"),
            ("isuzu", "isuzu"),
            ("lorenzo", "lorenzo"),
            ("daddy", "daddy"),
            ("cuff", "cuff"),
            ("puff", "puff"),
        ],
    );
}

#[test]
fn doubling_a_mark_key_still_undoes_it() {
    check(
        "undo idiom",
        &[
            ("ass", "as"),
            ("thiss", "this"),
            ("rooom", "room"),
            ("hoafz", "hoa"),
            ("ddad", "dad"),
            ("tieengs", "tiếng"),
            ("vieejtz", "viêt"),
        ],
    );
}

/// "off" is how Telex types the English "of" (the second f undoes the tone), so it is not
/// given back as "off" at the end of the word; "offf" types "off", and longer words stay listed.
#[test]
fn doubled_f_types_of_not_off() {
    check(
        "of through the undo idiom",
        &[
            ("off", "of"),
            ("off ", "of "),
            ("off the", "of the"),
            ("offf ", "off "),
            ("office ", "office "),
            ("offset ", "offset "),
        ],
    );
}

/// Common English words that plain Telex turns Vietnamese ("his" is "hí"), typed with the
/// doubled key the way a person fixes them, give the word, not the doubled keys.
#[test]
fn undo_idiom_types_common_english_words() {
    check(
        "undo idiom for common words",
        &[
            ("hiss ", "his "),
            ("burrns ", "burns "),
            ("burrst ", "burst "),
            ("burry ", "bury "),
            ("losser ", "loser "),
            ("lossers ", "losers "),
            ("passta ", "pasta "),
            ("bossom ", "bosom "),
            ("loss ", "loss "),
            ("pass ", "pass "),
            ("burrito ", "burrito "),
            ("hissing ", "hissing "),
        ],
    );
}

/// A key pressed again the moment it put a mark on ("pas" shows "pá", so "pass" then "te"):
/// the word on screen stays when it ends, although its keys start like a listed word.
#[test]
fn undo_idiom_in_the_middle_of_a_word_keeps_the_word() {
    check(
        "undo idiom before more letters",
        &[
            ("passte ", "paste "),
            ("passted ", "pasted "),
            ("bassic ", "basic "),
            ("bassically ", "basically "),
            ("bassed ", "based "),
            ("masster ", "master "),
            ("misstake ", "mistake "),
            ("lossing ", "losing "),
            ("purrpose ", "purpose "),
            ("mosstly ", "mostly "),
            ("guessts ", "guests "),
            ("Masster ", "Master "),
            ("BASSIC ", "BASIC "),
        ],
    );
}

/// The same keys are a word of their own when the doubled letter belongs to it.
#[test]
fn words_with_their_own_doubled_letter_still_come_back_as_typed() {
    check(
        "natural doubles",
        &[
            ("passing ", "passing "),
            ("password ", "password "),
            ("passion ", "passion "),
            ("missing ", "missing "),
            ("misspell ", "misspell "),
            ("possession ", "possession "),
            ("dessert ", "dessert "),
            ("losses ", "losses "),
            ("massive ", "massive "),
            ("bassist ", "bassist "),
            ("purring ", "purring "),
            ("guessing ", "guessing "),
        ],
    );
}
