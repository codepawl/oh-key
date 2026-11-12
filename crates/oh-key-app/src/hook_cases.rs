//! The end-to-end cases of the keyboard hook, shared by the `hook_suite` example (real windows,
//! needs a foreground) and the headless test in `hook_headless.rs` (no window), so the two
//! cannot drift apart.
//!
//! Key script: plain characters are typed; `{BS}` `{ENTER}` `{TAB}` `{ESC}` `{CAPS}` press that
//! key, `{CTRLSHIFT}` and `{ALTZ}` press the toggle chords, `{WAIT}` pauses so the autocomplete
//! suggestion lands. An `expected` starting with `mode:` compares the Vietnamese/English mode.
//! `hotkey` is 0 for Ctrl+Shift, 1 for Alt+Z, 2 for none.

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Control {
    Plain,
    Rich,
    AddressBar,
}

impl Control {
    pub fn label(self) -> &'static str {
        match self {
            Control::Plain => "edit",
            Control::Rich => "rich",
            Control::AddressBar => "addr",
        }
    }
}

pub struct Case {
    pub name: &'static str,
    pub control: Control,
    pub keys: &'static str,
    pub expected: &'static str,
    pub key_delay_ms: u64,
    pub hotkey: u8,
}

pub const fn case(
    name: &'static str,
    control: Control,
    keys: &'static str,
    expected: &'static str,
) -> Case {
    Case {
        name,
        control,
        keys,
        expected,
        key_delay_ms: 25,
        hotkey: 0,
    }
}

const fn fast(mut base: Case) -> Case {
    base.key_delay_ms = 5;
    base
}

const fn with_hotkey(mut base: Case, hotkey: u8) -> Case {
    base.hotkey = hotkey;
    base
}

const HARD_RHYMES_KEYS: &str = "nguwowif khuyru thuowr quoocs giuwx nguyeenx dduowngf ";
const HARD_RHYMES_TEXT: &str = "người khuỷu thuở quốc giữ nguyễn đường ";

pub fn all_cases() -> Vec<Case> {
    use Control::{AddressBar, Plain, Rich};
    vec![
        case("basic-sentence", Plain, "Vieetj Nam ", "Việt Nam "),
        case(
            "hard-rhymes-edit",
            Plain,
            HARD_RHYMES_KEYS,
            HARD_RHYMES_TEXT,
        ),
        case("hard-rhymes-rich", Rich, HARD_RHYMES_KEYS, HARD_RHYMES_TEXT),
        case(
            "hard-rhymes-addr",
            AddressBar,
            HARD_RHYMES_KEYS,
            HARD_RHYMES_TEXT,
        ),
        fast(case(
            "fast-5ms-edit",
            Plain,
            HARD_RHYMES_KEYS,
            HARD_RHYMES_TEXT,
        )),
        fast(case(
            "fast-5ms-rich",
            Rich,
            HARD_RHYMES_KEYS,
            HARD_RHYMES_TEXT,
        )),
        fast(case(
            "fast-5ms-addr",
            AddressBar,
            HARD_RHYMES_KEYS,
            HARD_RHYMES_TEXT,
        )),
        case("uppercase-shift", Plain, "VIEETJ ", "VIỆT "),
        case("capitalised", Plain, "Nguwowif ", "Người "),
        case(
            "caps-lock-edit",
            Plain,
            "{CAPS}vieetj {CAPS}vieetj",
            "VIỆT việt",
        ),
        case("caps-lock-rich", Rich, "{CAPS}vieetj {CAPS}", "VIỆT "),
        case("backspace-then-retype", Plain, "vieet{BS}tj ", "việt "),
        case("backspace-to-empty", Plain, "toois{BS}{BS}{BS}toi ", "toi "),
        case("backspace-mid-word-rich", Rich, "chaof{BS}o ", "chào "),
        case(
            "backspace-then-retype-addr",
            AddressBar,
            "vieet{BS}tj ",
            "việt ",
        ),
        case("backspace-plain-letters", Plain, "abc{BS}d ", "abd "),
        case(
            "english-words",
            Plain,
            "text window coffee ",
            "text window coffee ",
        ),
        case(
            "english-words-rich",
            Rich,
            "text window coffee ",
            "text window coffee ",
        ),
        case("toggle-ctrl-shift", Plain, "{CTRLSHIFT}", "mode:english"),
        case(
            "toggle-ctrl-shift-twice",
            Plain,
            "{CTRLSHIFT}{CTRLSHIFT}",
            "mode:vietnamese",
        ),
        case(
            "toggle-ctrl-shift-after-typing",
            Plain,
            "vieetj {CTRLSHIFT}",
            "mode:english",
        ),
        with_hotkey(case("toggle-alt-z", Plain, "{ALTZ}", "mode:english"), 1),
        with_hotkey(
            case(
                "toggle-alt-z-after-typing",
                Plain,
                "vieetj {ALTZ}",
                "mode:english",
            ),
            1,
        ),
        with_hotkey(
            case(
                "toggle-alt-z-keeps-typing-working",
                Plain,
                "{ALTZ}{ALTZ}vieetj ",
                "mode:vietnamese",
            ),
            1,
        ),
        case(
            "digits-punctuation",
            Plain,
            "vieetj, nam. 2026 vieetj1",
            "việt, nam. 2026 việt1",
        ),
        case(
            "enter-commits-rich",
            Rich,
            "vieetj{ENTER}nam{ENTER}",
            "việt\r\nnam\r\n",
        ),
        case("tab-commits-edit", Plain, "vieetj{TAB}", "việt"),
        case("english-then-enter-rich", Rich, "text{ENTER}", "text\r\n"),
        case(
            "addr-suggestion-then-circumflex",
            AddressBar,
            "vie{WAIT}e",
            "viê",
        ),
        case(
            "addr-suggestion-then-tone",
            AddressBar,
            "vie{WAIT}ej",
            "việ",
        ),
        case("addr-suggestion-d-bar", AddressBar, "d{WAIT}di", "đi"),
        case(
            "addr-suggestion-backspace",
            AddressBar,
            "vie{WAIT}{BS}",
            "vi",
        ),
        case(
            "addr-word-then-suggestion",
            AddressBar,
            "coffee{WAIT}",
            "coffee",
        ),
        case("addr-circumflex-word", AddressBar, "vie{WAIT}etj ", "việt "),
    ]
}
