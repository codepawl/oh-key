//! Fixing a word after a slip, the way UniKey allows: backspace into a word that was already
//! finished by a space or punctuation, or past a wrong key inside a word, then type the tone or
//! mark key; it applies to the word instead of being typed as a letter.

use oh_key_engine::{Config, Engine, InputMethod, KeyResult};

const BACKSPACE: char = '\u{8}';

/// Types the keys like a host that mirrors the text field: a backspace the engine passes on
/// deletes the character before the cursor, and when the engine picks the word before the
/// cursor back up, that word leaves the committed text and becomes the composing word.
fn screen_after(engine: &mut Engine, keys: &str) -> String {
    let mut committed = String::new();
    for key in keys.chars() {
        if key == BACKSPACE {
            apply_backspace(engine, &mut committed);
            continue;
        }
        match engine.process_key(key) {
            KeyResult::Composing => {}
            KeyResult::Commit(text) => committed.push_str(&text),
            KeyResult::PassThrough => committed.push(key),
        }
    }
    committed.push_str(engine.preedit());
    committed
}

fn apply_backspace(engine: &mut Engine, committed: &mut String) {
    if engine.is_composing() {
        engine.backspace();
        return;
    }
    let result = engine.backspace();
    assert_eq!(result, KeyResult::PassThrough);
    committed.pop();
    if engine.is_composing() {
        let resumed = engine.preedit();
        assert!(
            committed.ends_with(resumed),
            "resumed {resumed:?} is not the text before the cursor {committed:?}"
        );
        committed.truncate(committed.len() - resumed.len());
    }
}

fn telex(keys: &str) -> String {
    screen_after(&mut Engine::default(), keys)
}

fn with_method(input_method: InputMethod, keys: &str) -> String {
    let config = Config {
        input_method,
        ..Config::default()
    };
    screen_after(&mut Engine::new(config), keys)
}

fn check_table(table: &[(&str, &str)], typer: impl Fn(&str) -> String) {
    let mut failures = Vec::new();
    for (keys, expected) in table {
        let keys = keys.replace('<', "\u{8}");
        let actual = typer(&keys);
        if actual != *expected {
            failures.push(format!("{keys:?}: got {actual:?}, want {expected:?}"));
        }
    }
    assert!(failures.is_empty(), "\n{}", failures.join("\n"));
}

/// `<` in a key column is a backspace.
#[test]
fn a_tone_after_backspacing_over_the_space_lands_on_the_word() {
    check_table(
        &[
            ("vieet <j", "việt"),
            ("vieet <j ", "việt "),
            ("vieet <j nam", "việt nam"),
            ("vieet <s", "viết"),
            ("Vieet <j", "Việt"),
            ("toi <s", "tói"),
            ("chao <f", "chào"),
            ("ban <ws", "bắn"),
            ("tuong <w", "tương"),
            ("dung <d", "đung"),
            ("dung <<d", "đun"),
            ("vieet <<s", "viế"),
            ("vieejt <z", "viêt"),
            ("vieet <<ts", "viết"),
        ],
        telex,
    );
}

#[test]
fn a_tone_after_backspacing_over_punctuation_lands_on_the_word() {
    check_table(
        &[
            ("vieet.<j", "việt"),
            ("vieet,<j ", "việt "),
            ("vieet. <<j", "việt"),
            ("vieet!? <<<j", "việt"),
            ("viet. <j", "viet.j"),
        ],
        telex,
    );
}

#[test]
fn backspacing_past_the_word_boundary_does_not_resume_a_word() {
    check_table(
        &[
            ("viet  <j", "viet j"),
            ("viet <<<<<j", "j"),
            ("viet <<<<<<j", "j"),
            ("an viet <<<<<<s", "ans"),
            ("an em <<<s", "an s"),
            ("xin chao <<<<<<j", "xinj"),
            ("a <b", "ab"),
            ("1 <s", "1s"),
        ],
        telex,
    );
}

#[test]
fn letters_typed_after_resuming_extend_the_word() {
    check_table(
        &[
            ("viet <nam", "vietnam"),
            ("vie <et", "viêt"),
            ("tie <ng", "tieng"),
            ("vie <ej", "việ"),
        ],
        telex,
    );
}

#[test]
fn a_tone_after_backspacing_a_wrong_key_lands_on_the_word() {
    check_table(
        &[
            ("vieetk<s", "viết"),
            ("vieetk<s ", "viết "),
            ("vieetkl<<s", "viết"),
            ("vieetw<s", "viết"),
            ("nguwowik<f", "người"),
            ("vieen<tk<s", "viết"),
            ("vieetk<k", "vieetk"),
            ("vieetk< ", "vieet "),
            ("ddungk<f", "đùng"),
        ],
        telex,
    );
}

#[test]
fn vni_keys_apply_after_backspacing() {
    check_table(
        &[
            ("viet <65", "việt"),
            ("vie6tk<1", "viết"),
            ("dung <9", "đung"),
        ],
        |keys| with_method(InputMethod::Vni, keys),
    );
}

#[test]
fn english_words_still_come_back_as_typed() {
    check_table(
        &[
            ("coffee ", "coffee "),
            ("pics ", "pics "),
            ("coffee <", "coffee"),
            ("coffee < ", "coffee "),
            ("coffee <<", "coffe"),
            ("pics < ", "pics "),
            ("disc <<c ", "disc "),
            ("text <<t ", "text "),
            ("pass <<s ", "pass "),
            ("disc <, ", "disc, "),
        ],
        telex,
    );
}

#[test]
fn macro_expansions_are_not_resumed() {
    let mut engine = Engine::new(Config {
        macro_enabled: true,
        ..Config::default()
    });
    engine.add_macro("vn", "Việt Nam");
    assert_eq!(screen_after(&mut engine, "vn \u{8}s"), "Việt Nams");
}

/// Space, backspace, then a key gives the same text as the key typed straight after the word,
/// for Vietnamese words, words given back as English, and invalid runs alike.
#[test]
fn space_and_backspace_change_nothing_about_the_next_key() {
    let words = [
        "viet", "vieet", "vieejt", "nguwowi", "dduong", "tieng", "coffee", "misc", "pics", "text",
        "pass", "disc", "window", "thx", "Vieet", "VIEET",
    ];
    let next_keys = ["s", "f", "j", "z", "w", "d", "aa", "ee", "c", "h", "k"];
    let mut failures = Vec::new();
    for word in words {
        for next in next_keys {
            let straight = telex(&format!("{word}{next} "));
            let resumed = telex(&format!("{word} \u{8}{next} "));
            if straight != resumed {
                failures.push(format!(
                    "{word} + {next}: straight {straight:?}, resumed {resumed:?}"
                ));
            }
        }
    }
    assert!(failures.is_empty(), "\n{}", failures.join("\n"));
}

#[test]
fn reset_forgets_the_finished_word() {
    let mut engine = Engine::default();
    assert_eq!(screen_after(&mut engine, "viet "), "viet ");
    engine.reset();
    assert_eq!(engine.backspace(), KeyResult::PassThrough);
    assert!(!engine.is_composing());
}

#[test]
fn finishing_without_a_boundary_key_forgets_the_word() {
    let mut engine = Engine::default();
    for key in "viet".chars() {
        engine.process_key(key);
    }
    assert_eq!(engine.commit(), "viet");
    assert_eq!(engine.backspace(), KeyResult::PassThrough);
    assert!(!engine.is_composing());
}

/// Deleting the end of a word and typing it again keeps the letter the mark key undid:
/// "different" shows "diferent" until the space.
#[test]
fn retyping_the_end_of_a_doubled_letter_word_keeps_both_letters() {
    check_table(
        &[
            ("different<t ", "different "),
            ("different<<<ence ", "difference "),
            ("sorry<y ", "sorry "),
            ("office<e ", "office "),
            ("coffee<<ee ", "coffee "),
            ("Different<t.", "Different."),
            ("DIFFERENT<T ", "DIFFERENT "),
            ("passwotd<<rd ", "password "),
            ("differemt<<nt ", "different "),
            ("passt<<ss ", "pass "),
        ],
        telex,
    );
}

/// One letter too many, deleted: the word is the one typed before it.
#[test]
fn deleting_an_extra_letter_keeps_both_letters() {
    check_table(
        &[
            ("officee< ", "office "),
            ("sorryy< ", "sorry "),
            ("coffeex< ", "coffee "),
            ("differentm< ", "different "),
            ("differentlyy<<< ", "different "),
            ("passt< ", "pass "),
            ("passwordd<.", "password."),
            ("pastee< ", "paste "),
            ("passtee< ", "paste "),
        ],
        telex,
    );
}

/// A backspace that takes off the letter a third press put back leaves the word as shown;
/// so does a backspace on the doubled letter itself, and a word picked back up.
#[test]
fn a_backspace_on_the_doubled_letter_takes_it_off() {
    check_table(
        &[
            ("passs< ", "pas "),
            ("pass< ", "pa "),
            ("masssa< ", "mass "),
            ("masssa<< ", "mas "),
            ("passs<s ", "pass "),
            ("pass<s ", "pá "),
            ("passt<e ", "pase "),
            ("passs< < ", "pas "),
            ("pass < ", "pass "),
            ("different < ", "different "),
            ("different <<t ", "different "),
        ],
        telex,
    );
}
