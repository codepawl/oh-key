//! Table tests: keystrokes in, text out. Each table names its configuration;
//! a failing table reports every wrong row at once.

use oh_key_engine::{Config, Engine, InputMethod, KeyResult, ToneStyle};

const BACKSPACE: char = '\u{8}';

fn config(input_method: InputMethod, tone_style: ToneStyle) -> Config {
    Config {
        input_method,
        tone_style,
        ..Config::default()
    }
}

fn telex() -> Config {
    config(InputMethod::Telex, ToneStyle::Modern)
}

fn telex_classic() -> Config {
    config(InputMethod::Telex, ToneStyle::Classic)
}

fn vni() -> Config {
    config(InputMethod::Vni, ToneStyle::Modern)
}

fn viqr() -> Config {
    config(InputMethod::Viqr, ToneStyle::Modern)
}

fn simple_telex() -> Config {
    config(InputMethod::SimpleTelex, ToneStyle::Modern)
}

/// Types the keys and returns everything the user would see: committed text
/// followed by the word still being composed.
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

#[test]
fn telex_letters_and_marks() {
    check(
        "telex basics",
        telex(),
        &[
            ("a", "a"),
            ("aa", "â"),
            ("aw", "ă"),
            ("ee", "ê"),
            ("oo", "ô"),
            ("ow", "ơ"),
            ("uw", "ư"),
            ("dd", "đ"),
            ("w", "ư"),
            ("W", "Ư"),
            ("tw", "tư"),
            ("as", "á"),
            ("af", "à"),
            ("ar", "ả"),
            ("ax", "ã"),
            ("aj", "ạ"),
            ("aas", "ấ"),
            ("aws", "ắ"),
            ("ees", "ế"),
            ("oos", "ố"),
            ("ows", "ớ"),
            ("uws", "ứ"),
            ("ys", "ý"),
            ("is", "í"),
            ("os", "ó"),
            ("us", "ú"),
            ("es", "é"),
            ("ddas", "đá"),
            ("dduwowngf", "đường"),
            ("ba", "ba"),
            ("bas", "bá"),
            ("baf", "bà"),
            ("bar", "bả"),
            ("bax", "bã"),
            ("baj", "bạ"),
        ],
    );
}

#[test]
fn telex_words() {
    check(
        "telex words",
        telex(),
        &[
            ("vieetj", "việt"),
            ("Vieetj", "Việt"),
            ("tieengs", "tiếng"),
            ("nguoiwf", "người"),
            ("nguwowif", "người"),
            ("thuowng", "thương"),
            ("tuowng", "tương"),
            ("duwowcj", "dược"),
            ("duocwj", "dược"),
            ("ddaats", "đất"),
            ("ddaatj", "đật"),
            ("toans", "toán"),
            ("toanf", "toàn"),
            ("toafn", "toàn"),
            ("khoer", "khoẻ"),
            ("quyr", "quỷ"),
            ("quoocs", "quốc"),
            ("quas", "quá"),
            ("quys", "quý"),
            ("quynhf", "quỳnh"),
            ("giaf", "già"),
            ("gif", "gì"),
            ("gia", "gia"),
            ("gias", "giá"),
            ("giaos", "giáo"),
            ("giuwx", "giữ"),
            ("gins", "gín"),
            ("gieengs", "giếng"),
            ("nghieeng", "nghiêng"),
            ("chuyeenr", "chuyển"),
            ("khuyeens", "khuyến"),
            ("muoons", "muốn"),
            ("muas", "múa"),
            ("muaf", "mùa"),
            ("thuow", "thuơ"),
            ("thuowr", "thuở"),
            ("huow", "hươ"),
            ("thanhf", "thành"),
            ("chaof", "chào"),
            ("trawng", "trăng"),
            ("trangw", "trăng"),
            ("ddawngs", "đắng"),
            ("hoawcj", "hoặc"),
            ("toiw", "tơi"),
            ("tuwf", "từ"),
            ("boong", "bông"),
            ("coong", "công"),
            ("mootj", "một"),
            ("hocj", "học"),
            ("hocs", "hóc"),
            ("tachs", "tách"),
            ("tachj", "tạch"),
            ("cats", "cát"),
            ("catj", "cạt"),
            ("kimf", "kìm"),
            ("kes", "ké"),
            ("kee", "kê"),
            ("ddepj", "đẹp"),
            ("Ddaats", "Đất"),
            ("DDaats", "Đất"),
            ("ddoong", "đông"),
        ],
    );
}

#[test]
fn telex_out_of_order_typing() {
    check(
        "free tone placement",
        telex(),
        &[
            ("hoaf", "hoà"),
            ("hofa", "hoà"),
            ("hoaf", "hoà"),
            ("vietej", "việt"),
            ("dod", "đo"),
            ("dad", "đa"),
            ("toanf", "toàn"),
            ("tafn", "tàn"),
            ("tuyeenr", "tuyển"),
            ("tuyenre", "tuyển"),
            ("ngoafi", "ngoài"),
            ("nguwowfi", "người"),
        ],
    );
    check(
        "free placement off",
        Config {
            free_tone_placement: false,
            ..telex()
        },
        &[
            ("hoaf", "hoà"),
            ("vietej", "vietej"),
            ("dod", "dod"),
            ("dd", "đ"),
            ("tieengs", "tiếng"),
        ],
    );
}

#[test]
fn telex_double_key_undo() {
    check(
        "undo",
        telex(),
        &[
            ("ass", "as"),
            ("aaa", "aa"),
            ("aaaa", "aaa"),
            ("ooo", "oo"),
            ("eee", "ee"),
            ("dd", "đ"),
            ("ddd", "dd"),
            ("dddd", "ddd"),
            ("uww", "uw"),
            ("oww", "ow"),
            ("aww", "aw"),
            ("ww", "w"),
            ("tww", "tw"),
            ("tooo", "too"),
            ("rooom", "room"),
            ("hoff", "hof"),
            ("asf", "à"),
            ("asfrxj", "ạ"),
            ("asz", "a"),
            ("az", "az"),
            ("ss", "ss"),
            ("thiss", "this"),
            ("this", "thí"),
            ("boxx", "box"),
            ("tieengss", "tieengss"),
            ("passs", "passs"),
            ("missa", "missa"),
            ("lissa", "lissa"),
            ("Lissabon", "Lissabon"),
            ("tassa", "tassa"),
            ("lissf", "lisf"),
        ],
    );
}

#[test]
fn english_words_survive() {
    check(
        "english",
        telex(),
        &[
            ("text", "text"),
            ("next", "next"),
            ("sweet", "sweet"),
            ("window", "window"),
            ("Windows", "Windows"),
            ("windows", "windows"),
            ("class", "class"),
            ("coffee", "coffee"),
            ("pass", "pass"),
            ("password", "password"),
            ("Facebook", "Facebook"),
            ("facebook", "facebook"),
            ("http", "http"),
            ("https://", "https://"),
            ("email", "email"),
            ("an@gmail.com", "an@gmail.com"),
            ("google", "google"),
            ("hello", "hello"),
            ("world", "world"),
            ("code", "code"),
            ("jazz", "jazz"),
            ("zoo", "zoo"),
            ("user@example.com", "user@example.com"),
            ("error", "error"),
            ("office", "office"),
            ("staff", "staff"),
            ("Microsoft", "Microsoft"),
            ("keyboard", "keyboard"),
            ("github.com/oh-key", "github.com/oh-key"),
        ],
    );
}

#[test]
fn restore_can_be_switched_off() {
    check(
        "no restore",
        Config {
            restore_invalid_word: false,
            ..telex()
        },
        &[("text", "tẽt"), ("pass", "pas")],
    );
    check(
        "no spell check",
        Config {
            spell_check: false,
            ..telex()
        },
        &[("text", "tẽt"), ("class", "clas")],
    );
}

#[test]
fn invalid_words_come_back_as_typed_at_the_boundary() {
    check(
        "unfinished words",
        telex(),
        &[
            ("tiens ", "tiens "),
            ("user ", "user "),
            ("duocj ", "duocj "),
            ("tieens ", "tiến "),
        ],
    );
}

#[test]
fn a_c_that_only_starts_ich_or_ech_gives_the_word_back() {
    check(
        "c waiting for h",
        telex(),
        &[
            ("misc ", "míc "),
            ("mics ", "míc "),
            ("tics tawcs ", "tíc tắc "),
            ("disc ", "disc "),
            ("pics ", "pics "),
            ("meecs ", "meecs "),
            ("mecs ", "méc "),
            ("quycs ", "quycs "),
            ("tichs ", "tích "),
            ("eechs ", "ếch "),
            ("quychj ", "quỵch "),
            ("hics ", "híc "),
            ("hics hics ", "híc híc "),
            ("mic ", "mic "),
        ],
    );
}

#[test]
fn telex_brackets() {
    check(
        "brackets",
        telex(),
        &[
            ("[", "ơ"),
            ("]", "ư"),
            ("{", "Ơ"),
            ("}", "Ư"),
            ("]]", "]"),
            ("t]", "tư"),
            ("ba]", "ba]"),
            ("t[", "tơ"),
        ],
    );
}

#[test]
fn uppercase_and_caps_lock() {
    check(
        "caps",
        telex(),
        &[
            ("DDAATS", "ĐẤT"),
            ("VIEETJ", "VIỆT"),
            ("TOANS", "TOÁN"),
            ("HOAF", "HOÀ"),
            ("Hoaf", "Hoà"),
            ("Nguoiwf", "Người"),
            ("NGUOIWF", "NGƯỜI"),
            ("Tw", "Tư"),
            ("AA", "Â"),
            ("Aa", "Â"),
            ("DD", "Đ"),
            ("dD", "đ"),
            ("Dd", "Đ"),
            ("TIEENGS", "TIẾNG"),
            ("TieengS", "Tiếng"),
            ("THUOWNG", "THƯƠNG"),
            ("GIAF", "GIÀ"),
            ("QUOOCS", "QUỐC"),
        ],
    );
}

#[test]
fn sentences_and_punctuation() {
    check(
        "sentences",
        telex(),
        &[
            ("Xin chaof cacs banj", "Xin chào các bạn"),
            ("Tooi laf nguwowif Vieetj Nam", "Tôi là người Việt Nam"),
            ("Vieetj Nam ddepj", "Việt Nam đẹp"),
            ("Chaof, cacs banj!", "Chào, các bạn!"),
            ("(tieengs Vieetj)", "(tiếng Việt)"),
            ("abc123", "abc123"),
            ("a1", "a1"),
            ("2024", "2024"),
            ("xin chaof.\nhoc", "xin chào.\nhoc"),
            ("\"hay\"", "\"hay\""),
            ("tieengs\tVieetj", "tiếng\tViệt"),
        ],
    );
}

#[test]
fn backspace_behaviour() {
    let backspace = BACKSPACE;
    let cases: Vec<(String, &str)> = vec![
        (format!("tieengs{backspace}"), "tiến"),
        (format!("tieengs{backspace}{backspace}"), "tiế"),
        (format!("tieeng{backspace}{backspace}{backspace}"), "ti"),
        (format!("vieetj{backspace}"), "việ"),
        (format!("vieetj{backspace}{backspace}"), "vi"),
        (format!("hoafn{backspace}"), "hoà"),
        (format!("a{backspace}"), ""),
        (format!("{backspace}"), ""),
        (format!("{backspace}a"), "a"),
        (format!("dd{backspace}"), ""),
        (format!("ab{backspace}c"), "ac"),
        (format!("toans{backspace}"), "toá"),
        (format!("text{backspace}"), "tex"),
        (format!("text{backspace}t"), "text"),
        (format!("tieengs{backspace}s"), "tiếns"),
        (format!("xin{backspace}{backspace}{backspace}chaof"), "chào"),
        (format!("ab cd{backspace}e"), "ab ce"),
        (format!("aa{backspace}a"), "a"),
    ];
    let borrowed: Vec<(&str, &str)> = cases
        .iter()
        .map(|(keys, expected)| (keys.as_str(), *expected))
        .collect();
    check("backspace", telex(), &borrowed);
    let classic = format!("hoafn{backspace}");
    assert_eq!(typed(telex_classic(), &classic), "hòa");
}

#[test]
fn classic_tone_style() {
    check(
        "classic",
        telex_classic(),
        &[
            ("hoaf", "hòa"),
            ("hofa", "hòa"),
            ("khoer", "khỏe"),
            ("thuys", "thúy"),
            ("quys", "quý"),
            ("toans", "toán"),
            ("hoaf", "hòa"),
            ("loas", "lóa"),
            ("muas", "múa"),
            ("hoafn", "hoàn"),
            ("tuyeens", "tuyến"),
            ("thuowr", "thuở"),
        ],
    );
    check(
        "modern",
        telex(),
        &[
            ("thuys", "thuý"),
            ("khoer", "khoẻ"),
            ("loas", "loá"),
            ("hoafn", "hoàn"),
        ],
    );
}

#[test]
fn vni_method() {
    check(
        "vni",
        vni(),
        &[
            ("a1", "á"),
            ("a2", "à"),
            ("a3", "ả"),
            ("a4", "ã"),
            ("a5", "ạ"),
            ("a6", "â"),
            ("a8", "ă"),
            ("e6", "ê"),
            ("o6", "ô"),
            ("o7", "ơ"),
            ("u7", "ư"),
            ("d9", "đ"),
            ("D9", "Đ"),
            ("Vie6t5", "Việt"),
            ("tie6ng1", "tiếng"),
            ("ngu7o7i2", "người"),
            ("nguo7i2", "người"),
            ("thuo7ng", "thương"),
            ("tu7o7ng", "tương"),
            ("hoa2", "hoà"),
            ("ho2a", "hoà"),
            ("toa1n", "toán"),
            ("d9a6u1", "đấu"),
            ("a12", "à"),
            ("a11", "a1"),
            ("a66", "a6"),
            ("123", "123"),
            ("abc1", "abc1"),
            ("2024", "2024"),
            ("ba2", "bà"),
            ("so0", "so0"),
            ("sa10", "sa"),
            ("hoc2", "hoc2"),
            ("hoc5", "học"),
            ("thanh2", "thành"),
            ("tra8ng", "trăng"),
            ("Xin cha2o", "Xin chào"),
            ("text", "text"),
            ("nghie6ng", "nghiêng"),
            ("quo6c1", "quốc"),
            ("gia2", "già"),
            ("a7", "a7"),
            ("u8", "u8"),
        ],
    );
}

#[test]
fn viqr_method() {
    check(
        "viqr",
        viqr(),
        &[
            ("a'", "á"),
            ("a`", "à"),
            ("a?", "ả"),
            ("a~", "ã"),
            ("a.", "ạ"),
            ("a^", "â"),
            ("a(", "ă"),
            ("o+", "ơ"),
            ("o*", "ơ"),
            ("u+", "ư"),
            ("dd", "đ"),
            ("DD", "Đ"),
            ("Vie^.t", "Việt"),
            ("tie^'ng", "tiếng"),
            ("tu+o+ng", "tương"),
            ("ngu+o+`i", "người"),
            ("hoa`", "hoà"),
            ("a\\'", "a'"),
            ("ba, ", "ba, "),
            ("text", "text"),
            ("a''", "a'"),
            ("a^^", "a^"),
            ("d", "d"),
            ("dduwowngf", "dduwowngf"),
            ("Vie^.t Nam", "Việt Nam"),
            ("o+ ", "ơ "),
            ("1999", "1999"),
        ],
    );
}

#[test]
fn simple_telex_method() {
    check(
        "simple telex",
        simple_telex(),
        &[
            ("uw", "ư"),
            ("w", "w"),
            ("aw", "ă"),
            ("ow", "ơ"),
            ("uow", "ươ"),
            ("tieengs", "tiếng"),
            ("[", "["),
            ("]", "]"),
            ("ww", "ww"),
            ("uww", "uw"),
            ("nguowif", "người"),
            ("thuowng", "thương"),
            ("ddaats", "đất"),
            ("window", "window"),
        ],
    );
}

#[test]
fn macros_expand_at_word_end() {
    let mut engine = Engine::new(Config {
        macro_enabled: true,
        ..telex()
    });
    engine.add_macro("vn", "Việt Nam");
    engine.add_macro("tp", "thành phố");
    assert_eq!(typed_with(&mut engine, "vn "), "Việt Nam ");
    assert_eq!(typed_with(&mut engine, "VN "), "VIỆT NAM ");
    assert_eq!(typed_with(&mut engine, "tp."), "thành phố.");
    assert_eq!(typed_with(&mut engine, "vnx "), "vnx ");
    let mut disabled = Engine::new(telex());
    disabled.add_macro("vn", "Việt Nam");
    assert_eq!(typed_with(&mut disabled, "vn "), "vn ");
}

#[test]
fn engine_api_contract() {
    let mut engine = Engine::default();
    assert!(!engine.is_composing());
    assert_eq!(engine.process_key('1'), KeyResult::PassThrough);
    assert_eq!(engine.process_key(' '), KeyResult::PassThrough);
    assert_eq!(engine.backspace(), KeyResult::PassThrough);
    assert_eq!(engine.process_key('t'), KeyResult::Composing);
    assert_eq!(engine.process_key('o'), KeyResult::Composing);
    assert_eq!(engine.process_key('o'), KeyResult::Composing);
    assert_eq!(engine.preedit(), "tô");
    assert!(engine.is_composing());
    assert_eq!(engine.process_key(' '), KeyResult::Commit("tô ".to_owned()));
    assert!(!engine.is_composing());
    assert_eq!(engine.preedit(), "");
    engine.process_key('b');
    engine.process_key('a');
    engine.process_key('s');
    assert_eq!(engine.commit(), "bá");
    assert_eq!(engine.commit(), "");
    engine.process_key('x');
    engine.reset();
    assert!(!engine.is_composing());
    assert_eq!(engine.preedit(), "");
    engine.process_key('h');
    engine.process_key('o');
    engine.process_key('a');
    engine.process_key('f');
    assert_eq!(engine.preedit(), "hoà");
    engine.set_config(telex_classic());
    assert_eq!(engine.preedit(), "hòa");
    assert_eq!(engine.commit(), "hòa");
}

#[test]
fn long_runs_are_committed_instead_of_growing() {
    let mut engine = Engine::default();
    let mut visible = String::new();
    for _ in 0..500 {
        if let KeyResult::Commit(text) = engine.process_key('b') {
            visible.push_str(&text);
        }
    }
    visible.push_str(&engine.commit());
    assert_eq!(visible.len(), 500);
}

/// Tone tables written by hand, independent of the engine.
const TONED: &[(char, &str)] = &[
    ('a', "áàảãạ"),
    ('â', "ấầẩẫậ"),
    ('ă', "ắằẳẵặ"),
    ('e', "éèẻẽẹ"),
    ('ê', "ếềểễệ"),
    ('i', "íìỉĩị"),
    ('o', "óòỏõọ"),
    ('ô', "ốồổỗộ"),
    ('ơ', "ớờởỡợ"),
    ('u', "úùủũụ"),
    ('ư', "ứừửữự"),
    ('y', "ýỳỷỹỵ"),
];

fn with_tone(vowel: char, tone: usize) -> char {
    let (_, tones) = TONED
        .iter()
        .find(|(base, _)| *base == vowel)
        .expect("known vowel");
    tones.chars().nth(tone - 1).expect("tone 1..=5")
}

struct Cluster {
    telex: &'static str,
    vni: &'static str,
    viqr: &'static str,
    display: &'static str,
    modern_index: usize,
    classic_index: usize,
}

const fn cluster(
    telex: &'static str,
    vni: &'static str,
    viqr: &'static str,
    display: &'static str,
    modern_index: usize,
    classic_index: usize,
) -> Cluster {
    Cluster {
        telex,
        vni,
        viqr,
        display,
        modern_index,
        classic_index,
    }
}

const CLUSTERS: &[Cluster] = &[
    cluster("a", "a", "a", "a", 0, 0),
    cluster("aa", "a6", "a^", "â", 0, 0),
    cluster("aw", "a8", "a(", "ă", 0, 0),
    cluster("e", "e", "e", "e", 0, 0),
    cluster("ee", "e6", "e^", "ê", 0, 0),
    cluster("i", "i", "i", "i", 0, 0),
    cluster("o", "o", "o", "o", 0, 0),
    cluster("oo", "o6", "o^", "ô", 0, 0),
    cluster("ow", "o7", "o+", "ơ", 0, 0),
    cluster("u", "u", "u", "u", 0, 0),
    cluster("uw", "u7", "u+", "ư", 0, 0),
    cluster("y", "y", "y", "y", 0, 0),
    cluster("ai", "ai", "ai", "ai", 0, 0),
    cluster("ao", "ao", "ao", "ao", 0, 0),
    cluster("au", "au", "au", "au", 0, 0),
    cluster("ay", "ay", "ay", "ay", 0, 0),
    cluster("aau", "a6u", "a^u", "âu", 0, 0),
    cluster("aay", "a6y", "a^y", "ây", 0, 0),
    cluster("eo", "eo", "eo", "eo", 0, 0),
    cluster("eeu", "e6u", "e^u", "êu", 0, 0),
    cluster("ia", "ia", "ia", "ia", 0, 0),
    cluster("iee", "ie6", "ie^", "iê", 1, 1),
    cluster("iu", "iu", "iu", "iu", 0, 0),
    cluster("oa", "oa", "oa", "oa", 1, 0),
    cluster("oaw", "oa8", "oa(", "oă", 1, 1),
    cluster("oe", "oe", "oe", "oe", 1, 0),
    cluster("oi", "oi", "oi", "oi", 0, 0),
    cluster("ooi", "o6i", "o^i", "ôi", 0, 0),
    cluster("owi", "o7i", "o+i", "ơi", 0, 0),
    cluster("ua", "ua", "ua", "ua", 0, 0),
    cluster("uaa", "ua6", "ua^", "uâ", 1, 1),
    cluster("uee", "ue6", "ue^", "uê", 1, 1),
    cluster("ui", "ui", "ui", "ui", 0, 0),
    cluster("uoo", "uo6", "uo^", "uô", 1, 1),
    cluster("uy", "uy", "uy", "uy", 1, 0),
    cluster("uwa", "u7a", "u+a", "ưa", 0, 0),
    cluster("uwi", "u7i", "u+i", "ưi", 0, 0),
    cluster("uwu", "u7u", "u+u", "ưu", 0, 0),
    cluster("uow", "uo7", "uo+", "ươ", 1, 1),
    cluster("yee", "ye6", "ye^", "yê", 1, 1),
    cluster("ieeu", "ie6u", "ie^u", "iêu", 1, 1),
    cluster("oai", "oai", "oai", "oai", 1, 1),
    cluster("oay", "oay", "oay", "oay", 1, 1),
    cluster("oeo", "oeo", "oeo", "oeo", 1, 1),
    cluster("uaay", "ua6y", "ua^y", "uây", 1, 1),
    cluster("uooi", "uo6i", "uo^i", "uôi", 1, 1),
    cluster("uowi", "uo7i", "uo+i", "ươi", 1, 1),
    cluster("uowu", "uo7u", "uo+u", "ươu", 1, 1),
    cluster("uya", "uya", "uya", "uya", 1, 1),
    cluster("uyee", "uye6", "uye^", "uyê", 2, 2),
    cluster("uyu", "uyu", "uyu", "uyu", 1, 1),
    cluster("yeeu", "ye6u", "ye^u", "yêu", 1, 1),
];

const TELEX_TONE_KEYS: [char; 5] = ['s', 'f', 'r', 'x', 'j'];
const VNI_TONE_KEYS: [char; 5] = ['1', '2', '3', '4', '5'];
const VIQR_TONE_KEYS: [char; 5] = ['\'', '`', '?', '~', '.'];

fn expected_word(display: &str, tone_index: usize, tone: usize) -> String {
    let mut word = String::from("t");
    for (index, vowel) in display.chars().enumerate() {
        word.push(if index == tone_index {
            with_tone(vowel, tone)
        } else {
            vowel
        });
    }
    word
}

fn run_cluster_table(method: InputMethod, style: ToneStyle) {
    let mut failures = Vec::new();
    let mut rows = 0;
    for cluster in CLUSTERS {
        let (keys, tone_keys) = match method {
            InputMethod::Vni => (cluster.vni, VNI_TONE_KEYS),
            InputMethod::Viqr => (cluster.viqr, VIQR_TONE_KEYS),
            _ => (cluster.telex, TELEX_TONE_KEYS),
        };
        let tone_index = if style == ToneStyle::Modern {
            cluster.modern_index
        } else {
            cluster.classic_index
        };
        for (offset, tone_key) in tone_keys.iter().enumerate() {
            let tone = offset + 1;
            let typed_keys = format!("t{keys}{tone_key}");
            let expected = expected_word(cluster.display, tone_index, tone);
            let actual = typed(config(method, style), &typed_keys);
            rows += 1;
            if actual != expected {
                failures.push(format!(
                    "  {typed_keys:?}: expected {expected:?}, got {actual:?}"
                ));
            }
        }
    }
    assert!(
        failures.is_empty(),
        "{method:?} {style:?}: {} of {rows} failed\n{}",
        failures.len(),
        failures.join("\n")
    );
}

#[test]
fn every_vowel_cluster_times_every_tone_telex() {
    run_cluster_table(InputMethod::Telex, ToneStyle::Modern);
    run_cluster_table(InputMethod::Telex, ToneStyle::Classic);
    run_cluster_table(InputMethod::SimpleTelex, ToneStyle::Modern);
}

#[test]
fn every_vowel_cluster_times_every_tone_vni() {
    run_cluster_table(InputMethod::Vni, ToneStyle::Modern);
    run_cluster_table(InputMethod::Vni, ToneStyle::Classic);
}

#[test]
fn every_vowel_cluster_times_every_tone_viqr() {
    run_cluster_table(InputMethod::Viqr, ToneStyle::Modern);
    run_cluster_table(InputMethod::Viqr, ToneStyle::Classic);
}

#[test]
fn tone_typed_between_two_plain_vowels_lands_where_the_pair_puts_it() {
    let plain_pairs = CLUSTERS
        .iter()
        .filter(|cluster| cluster.telex.len() == 2 && cluster.display.is_ascii());
    for cluster in plain_pairs {
        let (first, second) = cluster.telex.split_at(1);
        for (style, index) in [
            (ToneStyle::Modern, cluster.modern_index),
            (ToneStyle::Classic, cluster.classic_index),
        ] {
            let sequence = format!("t{first}s{second}");
            let actual = typed(config(InputMethod::Telex, style), &sequence);
            assert_eq!(
                actual,
                expected_word(cluster.display, index, 1),
                "{sequence:?} {style:?}"
            );
        }
    }
}

#[test]
fn closed_syllables_keep_the_tone_on_the_second_vowel() {
    let both_styles = [telex(), telex_classic()];
    for style_config in both_styles {
        check(
            "closed syllables",
            style_config,
            &[
                ("toans", "toán"),
                ("quans", "quán"),
                ("hoangf", "hoàng"),
                ("khoanh", "khoanh"),
                ("tuanf", "tuàn"),
                ("tienj", "tiẹn"),
                ("mieengs", "miếng"),
                ("luoongs", "luống"),
            ],
        );
    }
}

/// A small deterministic generator so the property loop needs no crates.
struct XorShift(u64);

impl XorShift {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn below(&mut self, bound: usize) -> usize {
        (self.next() % bound as u64) as usize
    }
}

const KEY_POOL: &str = "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789 .,;:'`~?^+*(\\[]{}-_/@#!$%&=<>|\u{8}\u{8}aaeeoowwddssffrrxxjj";

fn is_valid_output_char(character: char) -> bool {
    if character.is_ascii() {
        return true;
    }
    let combining = ('\u{0300}'..='\u{036F}').contains(&character);
    let vietnamese = TONED.iter().any(|(base, tones)| {
        let lower = *base == character || tones.contains(character);
        let upper_base = base.to_uppercase().next() == Some(character);
        let upper_tone = tones
            .chars()
            .any(|tone| tone.to_uppercase().next() == Some(character));
        lower || upper_base || upper_tone
    }) || character == 'đ'
        || character == 'Đ';
    !combining && vietnamese
}

#[test]
fn random_keystrokes_never_panic_and_stay_nfc() {
    let pool: Vec<char> = KEY_POOL.chars().collect();
    let methods = [
        InputMethod::Telex,
        InputMethod::Vni,
        InputMethod::Viqr,
        InputMethod::SimpleTelex,
    ];
    let styles = [ToneStyle::Modern, ToneStyle::Classic];
    let mut random = XorShift(0x9E37_79B9_7F4A_7C15);
    for round in 0..4000 {
        let config = Config {
            input_method: methods[random.below(methods.len())],
            tone_style: styles[random.below(styles.len())],
            free_tone_placement: random.below(4) != 0,
            spell_check: random.below(5) != 0,
            restore_invalid_word: random.below(4) != 0,
            macro_enabled: false,
        };
        let mut engine = Engine::new(config);
        for step in 0..60 {
            let key = pool[random.below(pool.len())];
            let result = if key == BACKSPACE {
                engine.backspace()
            } else {
                engine.process_key(key)
            };
            let context = format!("round {round} step {step} key {key:?} config {config:?}");
            assert!(
                engine.preedit().chars().all(is_valid_output_char),
                "{context}: {:?}",
                engine.preedit()
            );
            assert_eq!(
                engine.is_composing(),
                !engine.preedit().is_empty(),
                "{context}"
            );
            if let KeyResult::Commit(text) = result {
                assert!(
                    text.chars()
                        .all(|c| is_valid_output_char(c) || !c.is_alphabetic()),
                    "{context}: {text:?}"
                );
                assert!(!engine.is_composing(), "{context}");
            }
        }
        let remainder = engine.commit();
        assert!(
            remainder
                .chars()
                .all(|c| is_valid_output_char(c) || !c.is_alphabetic())
        );
        assert_eq!(engine.preedit(), "");
    }
}

#[test]
fn committed_text_loses_no_letters_without_marks() {
    // Plain words with no mark keys come out unchanged in every method.
    let words = ["bcd", "xyz", "klmn", "prst", "qrv"];
    for method in [
        InputMethod::Telex,
        InputMethod::Vni,
        InputMethod::Viqr,
        InputMethod::SimpleTelex,
    ] {
        for word in words {
            let output = typed(config(method, ToneStyle::Modern), &format!("{word} "));
            assert_eq!(output, format!("{word} "), "{method:?} {word}");
        }
    }
}
