//! Adversarial tests: a Vietnamese syllable list built here, independent of
//! the engine's own tables, typed in Telex and VNI in several key orders and
//! compared with the NFC text a person expects.

use oh_key_engine::{Config, Engine, InputMethod, KeyResult, ToneStyle};

const BACKSPACE: char = '\u{8}';
const MAX_FAILURES_SHOWN: usize = 40;

fn config(input_method: InputMethod, tone_style: ToneStyle) -> Config {
    Config {
        input_method,
        tone_style,
        ..Config::default()
    }
}

fn typed_with(engine: &mut Engine, keys: &str) -> String {
    let mut visible = String::new();
    for key in keys.chars() {
        let result = if key == BACKSPACE {
            engine.backspace()
        } else {
            engine.process_key(key)
        };
        match result {
            KeyResult::Composing => {}
            KeyResult::Commit(text) => visible.push_str(&text),
            KeyResult::PassThrough if key != BACKSPACE => visible.push(key),
            KeyResult::PassThrough => {}
        }
    }
    visible.push_str(engine.preedit());
    visible
}

fn typed(config: Config, keys: &str) -> String {
    typed_with(&mut Engine::new(config), keys)
}

fn check(name: &str, config: Config, cases: &[(&str, &str)]) {
    let failures: Vec<String> = cases
        .iter()
        .filter_map(|&(keys, expected)| {
            let actual = typed(config, keys);
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

fn assert_no_failures(name: &str, failures: &[String], total: usize) {
    let step = (failures.len() / MAX_FAILURES_SHOWN).max(1);
    let shown: Vec<&str> = failures
        .iter()
        .step_by(step)
        .take(MAX_FAILURES_SHOWN)
        .map(String::as_str)
        .collect();
    assert!(
        failures.is_empty(),
        "{name}: {} of {total} failed, first {}:\n{}",
        failures.len(),
        shown.len(),
        shown.join("\n")
    );
}

// ---------------------------------------------------------------------------
// The oracle: what a Vietnamese reader expects, written without the engine.
// ---------------------------------------------------------------------------

const VOWEL_ROWS: &[&str] = &[
    "aáàảãạ",
    "ăắằẳẵặ",
    "âấầẩẫậ",
    "eéèẻẽẹ",
    "êếềểễệ",
    "iíìỉĩị",
    "oóòỏõọ",
    "ôốồổỗộ",
    "ơớờởỡợ",
    "uúùủũụ",
    "ưứừửữự",
    "yýỳỷỹỵ",
];

fn is_vowel(character: char) -> bool {
    VOWEL_ROWS.iter().any(|row| row.starts_with(character))
}

fn is_marked_vowel(character: char) -> bool {
    "ăâêôơư".contains(character)
}

fn with_tone(vowel: char, tone: usize) -> char {
    VOWEL_ROWS
        .iter()
        .find(|row| row.starts_with(vowel))
        .and_then(|row| row.chars().nth(tone))
        .unwrap_or(vowel)
}

#[derive(Clone, Copy)]
struct Syllable {
    onset: &'static str,
    rhyme: &'static str,
    tone: usize,
}

impl Syllable {
    fn letters(&self) -> Vec<char> {
        self.onset.chars().chain(self.rhyme.chars()).collect()
    }

    /// Letters from the first vowel of the rhyme up to the end of its vowel run.
    fn vowel_run(&self) -> (usize, usize) {
        let letters = self.letters();
        let start = self.onset.chars().count();
        let mut end = start;
        while end < letters.len() && is_vowel(letters[end]) {
            end += 1;
        }
        (start, end)
    }

    fn tone_carrier(&self, style: ToneStyle) -> Option<usize> {
        if self.tone == 0 {
            return None;
        }
        let letters = self.letters();
        let (start, end) = self.vowel_run();
        let run = &letters[start..end];
        let has_final = end < letters.len();
        let offset = if run.len() == 1 {
            0
        } else if let Some(position) = run.iter().rposition(|&letter| is_marked_vowel(letter)) {
            position
        } else if run.len() >= 3 {
            1
        } else {
            let pair: String = run.iter().collect();
            let modern_second =
                style == ToneStyle::Modern && matches!(pair.as_str(), "oa" | "oe" | "uy");
            usize::from(modern_second || has_final)
        };
        Some(start + offset)
    }

    fn expected(&self, style: ToneStyle) -> String {
        let carrier = self.tone_carrier(style);
        self.letters()
            .iter()
            .enumerate()
            .map(|(index, &letter)| {
                if Some(index) == carrier {
                    with_tone(letter, self.tone)
                } else {
                    letter
                }
            })
            .collect()
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mark {
    Roof,
    Horn,
    Breve,
    Stroke,
}

fn decompose(character: char) -> (char, Option<Mark>) {
    match character {
        'â' => ('a', Some(Mark::Roof)),
        'ă' => ('a', Some(Mark::Breve)),
        'ê' => ('e', Some(Mark::Roof)),
        'ô' => ('o', Some(Mark::Roof)),
        'ơ' => ('o', Some(Mark::Horn)),
        'ư' => ('u', Some(Mark::Horn)),
        'đ' => ('d', Some(Mark::Stroke)),
        other => (other, None),
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Method {
    Telex,
    Vni,
}

impl Method {
    fn input_method(self) -> InputMethod {
        match self {
            Method::Telex => InputMethod::Telex,
            Method::Vni => InputMethod::Vni,
        }
    }

    fn mark_key(self, base: char, mark: Mark) -> char {
        match (self, mark) {
            (Method::Telex, Mark::Roof) => base,
            (Method::Telex, Mark::Horn | Mark::Breve) => 'w',
            (Method::Telex, Mark::Stroke) => 'd',
            (Method::Vni, Mark::Roof) => '6',
            (Method::Vni, Mark::Horn) => '7',
            (Method::Vni, Mark::Breve) => '8',
            (Method::Vni, Mark::Stroke) => '9',
        }
    }

    fn tone_key(self, tone: usize) -> Option<char> {
        match (self, tone) {
            (_, 0) => None,
            (Method::Telex, tone) => ['s', 'f', 'r', 'x', 'j'].get(tone - 1).copied(),
            (Method::Vni, tone) => char::from_digit(tone as u32, 10),
        }
    }
}

#[derive(Clone, Copy)]
struct Order {
    /// Roof, horn and breve keys come after the final consonant.
    marks_after_final: bool,
    /// The tone key comes right after the vowels, before the final consonant.
    tone_after_vowels: bool,
}

const CANONICAL: Order = Order {
    marks_after_final: false,
    tone_after_vowels: false,
};
const FREE_ORDERS: &[Order] = &[
    Order {
        marks_after_final: false,
        tone_after_vowels: true,
    },
    Order {
        marks_after_final: true,
        tone_after_vowels: false,
    },
    Order {
        marks_after_final: true,
        tone_after_vowels: true,
    },
];

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum CaseStyle {
    Lower,
    Title,
    AllCaps,
}

fn keys_for(syllable: &Syllable, method: Method, order: Order, case_style: CaseStyle) -> String {
    let letters = syllable.letters();
    let (_, run_end) = syllable.vowel_run();
    let tone_key = method.tone_key(syllable.tone);
    let mut keys = String::new();
    let mut deferred_marks = String::new();
    let mut previous_deferred_horn = false;
    for (index, &letter) in letters.iter().enumerate() {
        if index == run_end && order.tone_after_vowels {
            keys.extend(tone_key);
        }
        let (base, mark) = decompose(letter);
        keys.push(base);
        previous_deferred_horn = match mark {
            Some(mark) if order.marks_after_final && mark != Mark::Stroke => {
                let is_horn = mark == Mark::Horn;
                if !(is_horn && previous_deferred_horn) {
                    deferred_marks.push(method.mark_key(base, mark));
                }
                is_horn
            }
            Some(mark) => {
                keys.push(method.mark_key(base, mark));
                false
            }
            None => false,
        };
    }
    if run_end == letters.len() && order.tone_after_vowels {
        keys.extend(tone_key);
    }
    keys.push_str(&deferred_marks);
    if !order.tone_after_vowels {
        keys.extend(tone_key);
    }
    apply_case_to_keys(&keys, case_style)
}

fn apply_case_to_keys(keys: &str, case_style: CaseStyle) -> String {
    match case_style {
        CaseStyle::Lower => keys.to_owned(),
        CaseStyle::AllCaps => keys.to_ascii_uppercase(),
        CaseStyle::Title => {
            let mut characters = keys.chars();
            let first = characters
                .next()
                .map(|character| character.to_ascii_uppercase());
            first.into_iter().chain(characters).collect()
        }
    }
}

fn expected_with_case(syllable: &Syllable, style: ToneStyle, case_style: CaseStyle) -> String {
    let expected = syllable.expected(style);
    match case_style {
        CaseStyle::Lower => expected,
        CaseStyle::AllCaps => expected.to_uppercase(),
        CaseStyle::Title => {
            let mut characters = expected.chars();
            let first: String = characters
                .next()
                .map(|character| character.to_uppercase().collect())
                .unwrap_or_default();
            first + characters.as_str()
        }
    }
}

// ---------------------------------------------------------------------------
// The syllable list.
// ---------------------------------------------------------------------------

const PLAIN_ONSETS: &[&str] = &[
    "b", "ch", "d", "đ", "h", "kh", "l", "m", "n", "nh", "ph", "r", "s", "t", "th", "tr", "v", "x",
];

const FRONT_ONSETS: &[&str] = &[
    "b", "ch", "d", "đ", "h", "kh", "l", "m", "n", "nh", "ph", "r", "s", "t", "th", "tr", "v", "x",
    "k", "gh", "ngh",
];
const FRONT_RHYMES: &[&str] = &[
    "e", "em", "en", "eng", "eo", "ep", "et", "ê", "êch", "êm", "ên", "ênh", "êp", "êt", "êu", "i",
    "ia", "ich", "im", "in", "inh", "ip", "it", "iu", "iêc", "iêm", "iên", "iêng", "iêp", "iêt",
];

const BACK_RHYMES: &[&str] = &[
    "a", "ac", "ach", "ai", "am", "an", "ang", "anh", "ao", "ap", "at", "au", "ay", "ăc", "ăm",
    "ăn", "ăng", "ăp", "ăt", "âc", "âm", "ân", "âng", "âp", "ât", "âu", "ây", "o", "oc", "oi",
    "om", "on", "ong", "op", "ot", "ô", "ôc", "ôi", "ôm", "ôn", "ông", "ôp", "ôt", "ơ", "ơi", "ơm",
    "ơn", "ơp", "ơt", "u", "uc", "ui", "um", "un", "ung", "up", "ut", "ư", "ưa", "ưc", "ưi", "ưng",
    "ưt", "ưu", "ua", "uôc", "uôi", "uôm", "uôn", "uông",
];
const BACK_ONSET_EXTRAS: &[&str] = &["c", "g", "ng"];
const BACK_RHYMES_WITH_HORN_PAIR: &[&str] = &[
    "uôt", "ươc", "ươi", "ươm", "ươn", "ương", "ươp", "ươt", "ươu",
];

const GI_RHYMES: &[&str] = &[
    "a", "ac", "am", "an", "ang", "ao", "ap", "at", "au", "ay", "ăn", "ăng", "ăt", "ân", "âng",
    "ât", "âu", "ên", "êng", "ôn", "ông", "ơ", "ơi", "ơn", "u", "ư",
];

const ROUND_ONSETS: &[&str] = &["h", "kh", "l", "ng", "x", "th", "t", "d", "ch", "đ", "s"];
const ROUND_RHYMES: &[&str] = &[
    "oa", "oac", "oach", "oai", "oam", "oan", "oang", "oanh", "oap", "oat", "oay", "oăc", "oăm",
    "oăn", "oăng", "oăt", "oe", "oen", "oeo", "oet", "uy", "uych", "uynh", "uyt", "uê", "uêch",
    "uênh", "uân", "uât", "uâng", "uây", "uyên", "uyêt", "uya", "uyu", "uơ",
];

const QU_RHYMES: &[&str] = &[
    "a", "ac", "ach", "ai", "am", "an", "ang", "anh", "ao", "ap", "at", "ay", "ăc", "ăn", "ăng",
    "ăt", "ân", "ât", "ây", "e", "en", "eo", "et", "ê", "êch", "ênh", "êt", "i", "in", "inh", "it",
    "ich", "y", "ynh", "ych", "yên", "yêt", "yêu", "ôc", "ơ",
];

const EMPTY_ONSET_RHYMES: &[&str] = &[
    "a", "ai", "an", "anh", "ao", "au", "ay", "ăn", "âm", "ân", "âu", "ây", "e", "em", "en", "eo",
    "ê", "êm", "êu", "i", "ia", "in", "inh", "it", "o", "oi", "om", "ong", "ô", "ôi", "ôm", "ông",
    "ơ", "ơi", "ơn", "u", "ưu", "ư", "ưa", "oa", "oan", "oanh", "oai", "oăn",
];
const EMPTY_ONSET_YE_RHYMES: &[&str] =
    &["uy", "uyên", "uynh", "uât", "uân", "y", "yên", "yêu", "yêm"];

fn tones_for(rhyme: &str) -> Vec<usize> {
    let is_stop_final = rhyme.ends_with('c')
        || rhyme.ends_with('p')
        || rhyme.ends_with('t')
        || rhyme.ends_with("ch");
    if is_stop_final {
        vec![0, 1, 5]
    } else {
        (0..=5).collect()
    }
}

fn build_syllables() -> Vec<Syllable> {
    let mut pairs: Vec<(&'static str, &'static str)> = Vec::new();
    for &onset in FRONT_ONSETS {
        let keeps_rhyme = |rhyme: &&&str| !(onset == "k" && **rhyme == "iu");
        pairs.extend(
            FRONT_RHYMES
                .iter()
                .filter(keeps_rhyme)
                .map(|&rhyme| (onset, rhyme)),
        );
    }
    for onset in PLAIN_ONSETS.iter().chain(BACK_ONSET_EXTRAS) {
        pairs.extend(
            BACK_RHYMES
                .iter()
                .chain(BACK_RHYMES_WITH_HORN_PAIR)
                .map(|&rhyme| (*onset, rhyme)),
        );
    }
    pairs.extend(GI_RHYMES.iter().map(|&rhyme| ("gi", rhyme)));
    for &onset in ROUND_ONSETS {
        pairs.extend(
            ROUND_RHYMES
                .iter()
                .filter(|&&rhyme| rhyme != "uơ")
                .map(|&rhyme| (onset, rhyme)),
        );
    }
    pairs.extend(QU_RHYMES.iter().map(|&rhyme| ("qu", rhyme)));
    pairs.extend(
        EMPTY_ONSET_RHYMES
            .iter()
            .chain(EMPTY_ONSET_YE_RHYMES)
            .map(|&rhyme| ("", rhyme)),
    );
    pairs
        .into_iter()
        .flat_map(|(onset, rhyme)| {
            tones_for(rhyme)
                .into_iter()
                .map(move |tone| Syllable { onset, rhyme, tone })
        })
        .collect()
}

fn run_syllables(
    method: Method,
    style: ToneStyle,
    orders: &[Order],
    case_styles: &[CaseStyle],
) -> (Vec<String>, usize) {
    let syllables = build_syllables();
    let engine_config = config(method.input_method(), style);
    let mut failures = Vec::new();
    let mut total = 0;
    for syllable in &syllables {
        for &order in orders {
            if order.marks_after_final && rhyme_is_ambiguous_with_late_marks(syllable.rhyme) {
                continue;
            }
            for &case_style in case_styles {
                total += 1;
                let keys = keys_for(syllable, method, order, case_style);
                let expected = expected_with_case(syllable, style, case_style);
                let actual = typed(engine_config, &keys);
                if actual != expected {
                    failures.push(format!("  {keys:?}: expected {expected:?}, got {actual:?}"));
                }
            }
        }
    }
    (failures, total)
}

/// A late w after "oan" may land on either vowel, so that order is not asserted.
fn rhyme_is_ambiguous_with_late_marks(rhyme: &str) -> bool {
    rhyme.starts_with("oă") || rhyme == "uơ"
}

const ALL_CASES: &[CaseStyle] = &[CaseStyle::Lower, CaseStyle::Title, CaseStyle::AllCaps];

#[test]
fn syllable_list_has_at_least_two_thousand_entries() {
    assert!(build_syllables().len() >= 2000);
}

#[test]
fn telex_canonical_order_every_rhyme() {
    let (failures, total) =
        run_syllables(Method::Telex, ToneStyle::Modern, &[CANONICAL], ALL_CASES);
    assert_no_failures("telex canonical", &failures, total);
}

#[test]
fn telex_free_orders_every_rhyme() {
    let (failures, total) = run_syllables(
        Method::Telex,
        ToneStyle::Modern,
        FREE_ORDERS,
        &[CaseStyle::Lower],
    );
    assert_no_failures("telex free order", &failures, total);
}

#[test]
fn vni_canonical_order_every_rhyme() {
    let (failures, total) = run_syllables(Method::Vni, ToneStyle::Modern, &[CANONICAL], ALL_CASES);
    assert_no_failures("vni canonical", &failures, total);
}

#[test]
fn vni_free_orders_every_rhyme() {
    let (failures, total) = run_syllables(
        Method::Vni,
        ToneStyle::Modern,
        FREE_ORDERS,
        &[CaseStyle::Lower],
    );
    assert_no_failures("vni free order", &failures, total);
}

#[test]
fn classic_tone_style_every_rhyme() {
    for method in [Method::Telex, Method::Vni] {
        let (failures, total) = run_syllables(
            method,
            ToneStyle::Classic,
            &[CANONICAL],
            &[CaseStyle::Lower],
        );
        assert_no_failures(&format!("{method:?} classic"), &failures, total);
    }
}

#[test]
fn simple_telex_every_rhyme() {
    let syllables = build_syllables();
    let engine_config = config(InputMethod::SimpleTelex, ToneStyle::Modern);
    let mut failures = Vec::new();
    for syllable in &syllables {
        let keys = keys_for(syllable, Method::Telex, CANONICAL, CaseStyle::Lower);
        let expected = syllable.expected(ToneStyle::Modern);
        let actual = typed(engine_config, &keys);
        if actual != expected {
            failures.push(format!("  {keys:?}: expected {expected:?}, got {actual:?}"));
        }
    }
    assert_no_failures("simple telex", &failures, syllables.len());
}

#[test]
fn words_in_a_sentence_keep_their_own_state() {
    let syllables = build_syllables();
    let engine_config = config(InputMethod::Telex, ToneStyle::Modern);
    let mut failures = Vec::new();
    for pair in syllables.chunks(2).step_by(5) {
        let [first, second] = pair else { continue };
        let keys = format!(
            "{} {}, {}",
            keys_for(first, Method::Telex, CANONICAL, CaseStyle::Lower),
            keys_for(second, Method::Telex, CANONICAL, CaseStyle::Lower),
            keys_for(first, Method::Telex, CANONICAL, CaseStyle::Title)
        );
        let expected = format!(
            "{} {}, {}",
            first.expected(ToneStyle::Modern),
            second.expected(ToneStyle::Modern),
            expected_with_case(first, ToneStyle::Modern, CaseStyle::Title)
        );
        let actual = typed(engine_config, &keys);
        if actual != expected {
            failures.push(format!("  {keys:?}: expected {expected:?}, got {actual:?}"));
        }
    }
    assert_no_failures("sentences", &failures, syllables.len() / 10);
}

// ---------------------------------------------------------------------------
// Backspace in the middle of a word, then retyping what was removed.
// ---------------------------------------------------------------------------

fn inline_keys_for_letter(letter: char, method: Method) -> String {
    let (base, mark) = decompose(letter);
    let mut keys = base.to_string();
    if let Some(mark) = mark {
        keys.push(method.mark_key(base, mark));
    }
    keys
}

#[test]
fn backspace_then_retype_restores_the_same_word() {
    let mut failures = Vec::new();
    let mut total = 0;
    for method in [Method::Telex, Method::Vni] {
        let engine_config = config(method.input_method(), ToneStyle::Modern);
        for syllable in build_syllables().iter().step_by(3) {
            let letters = syllable.letters();
            let groups: Vec<String> = letters
                .iter()
                .map(|&letter| inline_keys_for_letter(letter, method))
                .collect();
            let tone_key = method.tone_key(syllable.tone);
            let carrier = syllable.tone_carrier(ToneStyle::Modern);
            let expected = syllable.expected(ToneStyle::Modern);
            let full_word = keys_for(syllable, method, CANONICAL, CaseStyle::Lower);
            for removed in 1..=letters.len() {
                total += 1;
                let kept = letters.len() - removed;
                let mut retype = groups[kept..].concat();
                if carrier.is_some_and(|index| index >= kept) {
                    retype.extend(tone_key);
                }
                let keys = format!(
                    "{full_word}{}{retype}",
                    BACKSPACE.to_string().repeat(removed)
                );
                let actual = typed(engine_config, &keys);
                if actual != expected {
                    failures.push(format!("  {keys:?}: expected {expected:?}, got {actual:?}"));
                }
            }
        }
    }
    assert_no_failures("backspace and retype", &failures, total);
}

#[test]
fn backspace_shows_the_word_without_its_last_letters() {
    let backspace = BACKSPACE;
    let cases: Vec<(String, &str)> = vec![
        (format!("nguwowif{backspace}"), "ngườ"),
        (format!("nguwowif{backspace}{backspace}"), "ngư"),
        (format!("duwowngf{backspace}{backspace}"), "dườ"),
        (format!("khuyeenr{backspace}"), "khuyể"),
        (format!("quyeets{backspace}{backspace}"), "quy"),
        (format!("thuowr{backspace}"), "thu"),
        (format!("ddas{backspace}"), "đ"),
        (format!("gifn{backspace}"), "gì"),
        (format!("giaf{backspace}"), "gi"),
        (format!("giaf{backspace}a"), "gia"),
        (
            format!("Tieengs{backspace}{backspace}{backspace}{backspace}{backspace}o"),
            "o",
        ),
        (format!("hoafn{backspace}{backspace}{backspace}t"), "ht"),
        (format!("aa{backspace}a"), "a"),
        (format!("tooi{backspace}{backspace}{backspace}i"), "i"),
    ];
    let borrowed: Vec<(&str, &str)> = cases
        .iter()
        .map(|(keys, expected)| (keys.as_str(), *expected))
        .collect();
    check(
        "backspace states",
        config(InputMethod::Telex, ToneStyle::Modern),
        &borrowed,
    );
}

// ---------------------------------------------------------------------------
// Named words that Unikey users hit every day.
// ---------------------------------------------------------------------------

#[test]
fn named_words_in_telex() {
    check(
        "named words",
        config(InputMethod::Telex, ToneStyle::Modern),
        &[
            ("gifn", "gìn"),
            ("gin", "gin"),
            ("giuwx", "giữ"),
            ("guwx", "gữ"),
            ("gifn giuwx", "gìn giữ"),
            ("quoocs", "quốc"),
            ("quocos", "quốc"),
            ("quoocj", "quộc"),
            ("khuyur", "khuỷu"),
            ("khuyru", "khuỷu"),
            ("thuowr", "thuở"),
            ("thuwowr", "thưở"),
            ("nguwowif", "người"),
            ("nguoiwf", "người"),
            ("nguwoif", "người"),
            ("nguowif", "người"),
            ("nguoiff", "nguoif"),
            ("huwowu", "hươu"),
            ("huowu", "hươu"),
            ("huouw", "hươu"),
            ("ruwouj", "rượu"),
            ("ruouwj", "rượu"),
            ("nguyeenx", "nguyễn"),
            ("nguyenex", "nguyễn"),
            ("Nguyeenx", "Nguyễn"),
            ("NGUYEENX", "NGUYỄN"),
            ("dduwowngf", "đường"),
            ("duongwfd", "đường"),
            ("dduowngf", "đường"),
            ("dduwongf", "đường"),
            ("oawnr", "oẳn"),
            ("oanwr", "oẳn"),
            ("khuyeens", "khuyến"),
            ("khuyenes", "khuyến"),
            ("quyeets", "quyết"),
            ("quyetes", "quyết"),
            ("tuyeetj", "tuyệt"),
            ("chuyeenj", "chuyện"),
            ("xoays", "xoáy"),
            ("khoangr", "khoảng"),
            ("huynhf", "huỳnh"),
            ("ngoawnf", "ngoằn"),
            ("nguwowngf", "ngường"),
            ("uwowcs", "ước"),
            ("muwowi", "mươi"),
            ("muwoi", "mươi"),
            ("tuowngf", "tường"),
            ("thuowngf", "thường"),
            ("thuwowngf", "thường"),
            ("Vieetj", "Việt"),
            ("Haf Nooij", "Hà Nội"),
            ("yeeus", "yếu"),
            ("uyeenf", "uyền"),
            ("ys", "ý"),
            ("quys", "quý"),
            ("hoaf", "hoà"),
            ("loaf", "loà"),
        ],
    );
    check(
        "classic",
        config(InputMethod::Telex, ToneStyle::Classic),
        &[
            ("thuys", "thúy"),
            ("hoaf", "hòa"),
            ("khoer", "khỏe"),
            ("khuyur", "khuỷu"),
            ("hoafn", "hoàn"),
            ("giuwx", "giữ"),
        ],
    );
}

#[test]
fn named_words_in_vni() {
    check(
        "vni named words",
        config(InputMethod::Vni, ToneStyle::Modern),
        &[
            ("gi2n", "gìn"),
            ("giu73", "giử"),
            ("gi2n giu74", "gìn giữ"),
            ("quo6c1", "quốc"),
            ("khuy3u", "khuỷu"),
            ("thuo73", "thuở"),
            ("ngu7o7i2", "người"),
            ("ngu7oi2", "người"),
            ("ngu7o7i2", "người"),
            ("hu7o7u", "hươu"),
            ("d9u7o7ng2", "đường"),
            ("d9uong72", "đường"),
            ("oa8n3", "oẳn"),
            ("khuye6n1", "khuyến"),
            ("quye6t1", "quyết"),
            ("nguye6n4", "nguyễn"),
            ("ru7o7u5", "rượu"),
            ("to6i", "tôi"),
            ("Vie6t5", "Việt"),
            ("d9", "đ"),
            ("D9a1", "Đá"),
        ],
    );
}

// ---------------------------------------------------------------------------
// English and code must come back as typed.
// ---------------------------------------------------------------------------

#[test]
fn english_and_code_tokens_are_not_mangled() {
    let tokens: Vec<&str> = vec![
        "function",
        "const",
        "return",
        "struct",
        "impl",
        "enum",
        "mod",
        "crate",
        "print",
        "println!",
        "printf(",
        "std::vec::Vec",
        "HashMap",
        "String",
        "self.x",
        "snake_case",
        "camelCase",
        "PascalCase",
        "kebab-case",
        "--help",
        "-rf",
        "$HOME",
        "~/src",
        "C:\\Windows",
        "git commit -m",
        "git push",
        "cargo build",
        "npm install",
        "node_modules",
        "package.json",
        "localhost:3000",
        "127.0.0.1",
        "0xFF",
        "#include",
        "#define",
        "<div>",
        "</div>",
        "x==y",
        "a!=b",
        "a&&b",
        "a||b",
        "//TODO",
        "/*x*/",
        "SELECT * FROM users",
        "WHERE id=1",
        "width:100px",
        "rgba(0,0,0)",
        "www.google.com",
        "key=value",
        "JSON",
        "XML",
        "HTML",
        "CSS",
        "API",
        "URL",
        "UUID",
        "Linux",
        "Ubuntu",
        "macOS",
        "iOS",
        "Android",
        "Chrome",
        "Firefox",
        "Discord",
        "YouTube",
        "Twitter",
        "TikTok",
        "while",
        "for",
        "match",
        "async",
        "await",
        "export",
        "import",
        "default",
        "class",
        "extends",
        "interface",
        "private",
        "static",
        "public",
        "void",
        "switch",
        "case",
        "break",
        "continue",
        "throw",
        "catch",
        "finally",
        "yield",
        "true",
        "false",
        "null",
        "undefined",
        "super",
        "new",
        "delete",
        "typeof",
        "instanceof",
        "let",
        "fn",
        "pub",
        "use",
        "trait",
        "where",
        "move",
        "dyn",
        "unsafe",
        "extern",
        "type",
        "Result",
        "Option",
        "Some",
        "None",
        "Vec",
        "Rc",
        "Arc",
        "Mutex",
        "println",
        "format",
        "assert",
        "debug",
        "release",
        "target",
        "build",
        "feature",
        "branch",
        "merge",
        "rebase",
        "stash",
        "fetch",
        "pull",
        "commit",
        "origin",
        "remote",
        "config",
        "install",
        "update",
        "upgrade",
        "remove",
        "search",
        "status",
        "login",
        "logout",
        "user",
        "admin",
        "sudo",
        "chmod",
        "mkdir",
        "rmdir",
        "touch",
        "grep",
        "sed",
        "awk",
        "curl",
        "wget",
        "ssh",
        "scp",
        "docker",
        "kubectl",
        "redis",
        "mysql",
        "postgres",
        "apache",
        "python",
        "java",
        "ruby",
        "swift",
        "kotlin",
        "golang",
        "typescript",
        "javascript",
        "react",
        "vue",
        "angular",
        "svelte",
        "nextjs",
        "webpack",
        "vite",
        "eslint",
        "prettier",
        "jest",
        "mocha",
        "server",
        "client",
        "request",
        "response",
        "header",
        "cookie",
        "token",
        "session",
        "database",
        "table",
        "index",
        "query",
        "schema",
        "model",
        "view",
        "controller",
        "router",
        "handler",
        "service",
        "worker",
        "thread",
        "process",
        "memory",
        "buffer",
        "stream",
        "socket",
        "packet",
        "kernel",
        "driver",
        "module",
        "package",
        "library",
        "framework",
        "compiler",
        "linker",
        "debugger",
        "profiler",
        "terminal",
        "console",
        "editor",
        "browser",
        "window",
        "button",
        "dialog",
        "widget",
        "layout",
        "style",
        "color",
        "font",
        "image",
        "video",
        "audio",
        "download",
        "upload",
        "backup",
        "restore",
        "version",
        "license",
        "author",
        "readme",
        "changelog",
    ];
    let engine_config = config(InputMethod::Telex, ToneStyle::Modern);
    let failures: Vec<String> = tokens
        .iter()
        .filter_map(|&token| {
            let with_boundary = format!("{token} ");
            let actual = typed(engine_config, &with_boundary);
            (actual != with_boundary).then(|| format!("  {with_boundary:?}: got {actual:?}"))
        })
        .collect();
    assert_no_failures("english and code tokens", &failures, tokens.len());
}

#[test]
fn english_words_with_vietnamese_looking_starts_in_vni() {
    check(
        "vni digits after consonants",
        config(InputMethod::Vni, ToneStyle::Modern),
        &[
            ("2024", "2024"),
            ("h264", "h264"),
            ("mp3", "mp3"),
            ("x86", "x86"),
            ("utf8", "utf8"),
            ("3.14", "3.14"),
            ("100%", "100%"),
            ("1,000", "1,000"),
            ("#1", "#1"),
            ("v0.1.0", "v0.1.0"),
            ("sha256", "sha256"),
            ("md5", "md5"),
            ("b2b", "b2b"),
            ("c3po", "c3po"),
        ],
    );
}

// ---------------------------------------------------------------------------
// Mixed case.
// ---------------------------------------------------------------------------

#[test]
fn mixed_case_keeps_the_case_of_each_letter() {
    check(
        "mixed case",
        config(InputMethod::Telex, ToneStyle::Modern),
        &[
            ("Tieengs", "Tiếng"),
            ("TIEENGs", "TIẾNG"),
            ("tIEENGS", "tIẾNG"),
            ("tIEENG", "tIÊNG"),
            ("DDuwowngf", "Đường"),
            ("dDuwowngf", "đường"),
            ("DDuWOWNGF", "ĐưỜNG"),
            ("QUOOCS", "QUỐC"),
            ("QuoocS", "Quốc"),
            ("QUoocs", "QUốc"),
            ("nGUYEENX", "nGUYỄN"),
            ("uW", "ư"),
            ("UW", "Ư"),
            ("Uw", "Ư"),
            ("OW", "Ơ"),
            ("oW", "ơ"),
            ("AW", "Ă"),
            ("aW", "ă"),
            ("EE", "Ê"),
            ("Ee", "Ê"),
            ("eE", "ê"),
            ("OO", "Ô"),
            ("Oo", "Ô"),
            ("GIFN", "GÌN"),
            ("GiFn", "Gìn"),
            ("KHUYUR", "KHUỶU"),
            ("Thuowr", "Thuở"),
            ("THUOWR", "THUỞ"),
            ("HoaF", "Hoà"),
            ("hOaf", "hOà"),
            ("HOAf", "HOÀ"),
            ("VIEETj", "VIỆT"),
            ("ViEEtj", "ViỆt"),
            ("Dd", "Đ"),
            ("dD", "đ"),
            ("DD", "Đ"),
        ],
    );
}

// ---------------------------------------------------------------------------
// Undo chains.
// ---------------------------------------------------------------------------

#[test]
fn double_key_undo_chains() {
    check(
        "undo chains",
        config(InputMethod::Telex, ToneStyle::Modern),
        &[
            ("aaa", "aa"),
            ("aaaa", "aaa"),
            ("aaaaa", "aaaa"),
            ("ooo", "oo"),
            ("oooo", "ooo"),
            ("eee", "ee"),
            ("eeee", "eee"),
            ("ddd", "dd"),
            ("dddd", "ddd"),
            ("ddddd", "dddd"),
            ("www", "www"),
            ("uww", "uw"),
            ("uwww", "uww"),
            ("ass", "as"),
            ("asss", "ass"),
            ("assf", "asf"),
            ("asf", "à"),
            ("asff", "af"),
            ("asfs", "á"),
            ("asfsf", "à"),
            ("asfrxj", "ạ"),
            ("asfrxjj", "aj"),
            ("tass", "tas"),
            ("tasss", "tass"),
            ("hoaff", "hoaf"),
            ("hoafff", "hoaff"),
            ("hoaf", "hoà"),
            ("hoafz", "hoa"),
            ("toooi", "tooi"),
            ("tooi", "tôi"),
            ("tooii", "tooii"),
            ("daad", "đâ"),
            ("dadd", "dad"),
            ("ddad", "dad"),
            ("dd", "đ"),
            ("ddd d", "dd d"),
            ("ddaa", "đâ"),
            ("aaas", "aas"),
            ("aw", "ă"),
            ("aww", "aw"),
            ("awww", "aww"),
            ("awws", "aws"),
            ("ow", "ơ"),
            ("oww", "ow"),
            ("owww", "oww"),
            ("uow", "ươ"),
            ("uoww", "uow"),
            ("uowww", "uoww"),
            ("tieengss", "tieengss"),
            ("tieeng", "tiêng"),
            ("tieengs", "tiếng"),
            ("tieen", "tiên"),
            ("tieenn", "tieenn"),
            ("tieeen", "tieen"),
        ],
    );
    check(
        "vni undo chains",
        config(InputMethod::Vni, ToneStyle::Modern),
        &[
            ("a11", "a1"),
            ("a111", "a11"),
            ("a1", "á"),
            ("a12", "à"),
            ("a122", "a2"),
            ("a66", "a6"),
            ("a666", "a66"),
            ("a88", "a8"),
            ("o77", "o7"),
            ("d99", "d9"),
            ("d999", "d99"),
            ("uo77", "uo7"),
            ("u7o77", "uo7"),
            ("a10", "a"),
            ("a60", "â0"),
            ("a1066", "a6"),
        ],
    );
}

#[test]
fn stroke_typed_after_the_word_works_in_every_position() {
    check(
        "late d",
        config(InputMethod::Telex, ToneStyle::Modern),
        &[
            ("dod", "đo"),
            ("duongd", "đuong"),
            ("duowngd", "đương"),
            ("dduongd", "duongd"),
            ("dongdf", "đòng"),
            ("dongfd", "đòng"),
            ("dongd", "đong"),
            ("diid", "diid"),
        ],
    );
}

// ---------------------------------------------------------------------------
// Unusual characters.
// ---------------------------------------------------------------------------

#[test]
fn non_vietnamese_letters_come_back_unchanged() {
    check(
        "unicode",
        config(InputMethod::Telex, ToneStyle::Modern),
        &[
            ("café", "café"),
            ("naïve", "naïve"),
            ("İstanbul", "İstanbul"),
            ("İ", "İ"),
            ("straße", "straße"),
            ("Ǆ", "Ǆ"),
            ("ǅ", "ǅ"),
            ("ǆ", "ǆ"),
            ("日本語", "日本語"),
            ("Ünïcödé", "Ünïcödé"),
            ("Việt", "Việt"),
            ("tiếng", "tiếng"),
            ("ĐẤT", "ĐẤT"),
            ("ệ", "ệ"),
            ("Ệ", "Ệ"),
            ("ẤM", "ẤM"),
        ],
    );
}

#[test]
fn english_words_with_doubled_mark_keys_are_given_back() {
    let words = [
        "www",
        "assert",
        "asserted",
        "assertion",
        "embarrass",
        "embarrassed",
        "harass",
        "dispossess",
        "possession",
        "classes",
        "classify",
        "Mississippi",
        "address",
        "addressed",
        "misspell",
        "dessert",
        "desserts",
        "assistant",
        "associated",
        "assurance",
        "assassin",
        "assault",
        "tassel",
        "vessel",
        "lasso",
        "russet",
        "fossil",
        "gossip",
        "hassle",
        "jessie",
        "missing",
        "passing",
        "passive",
        "mussel",
        "muffin",
        "waffle",
        "puffin",
        "ruffle",
        "shuffle",
        "snuffle",
        "scoff",
        "sniff",
        "whiff",
        "payoff",
        "cutoff",
        "runoff",
        "kickoff",
        "tooth",
        "carrot",
        "parrot",
        "terrain",
        "terrace",
        "terrible",
        "territory",
        "interrupt",
        "irrigate",
        "irritate",
        "mirror",
        "currency",
        "currently",
        "occurrence",
        "recurrent",
        "turret",
        "ferris",
        "sorrel",
        "narrate",
        "corridor",
        "correlate",
        "correspond",
        "corrupt",
        "arrange",
        "arrest",
        "arrival",
        "borrowed",
        "burrito",
        "carriage",
        "carrier",
        "embroider",
        "furry",
        "hurricane",
        "marriage",
        "porridge",
        "surrender",
        "tomorrow",
        "worried",
    ];
    let engine_config = config(InputMethod::Telex, ToneStyle::Modern);
    let failures: Vec<String> = words
        .iter()
        .filter_map(|&word| {
            let with_boundary = format!("{word} ");
            let actual = typed(engine_config, &with_boundary);
            (actual != with_boundary).then(|| format!("  {with_boundary:?}: got {actual:?}"))
        })
        .collect();
    assert_no_failures("doubled mark keys", &failures, words.len());
}
