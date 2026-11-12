//! What each key does in each input method.

use crate::InputMethod;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum HookKind {
    /// Horn on u and o, breve on a (Telex w).
    All,
    /// Horn on u and o only (VNI 7, VIQR + and *).
    HornOnly,
    /// Breve on a only (VNI 8, VIQR open parenthesis).
    BreveOnly,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Action {
    /// Set a tone; 0 removes the tone.
    Tone(u8),
    /// Circumflex on this vowel, or on whichever vowel takes one when `None`.
    Roof(Option<char>),
    Hook(HookKind),
    Stroke,
    /// Telex w: a hook if one fits, otherwise ư.
    TelexW,
    /// A key that stands for a fixed letter (Telex `[` `]`).
    MapLetter {
        base: char,
        upper: bool,
    },
    /// VIQR backslash: the next key is typed literally.
    Escape,
    None,
}

pub(crate) fn action_for(method: InputMethod, key: char) -> Action {
    match method {
        InputMethod::Telex => telex_action(key, true),
        InputMethod::SimpleTelex => telex_action(key, false),
        InputMethod::Vni => vni_action(key),
        InputMethod::Viqr => viqr_action(key),
    }
}

fn telex_action(key: char, full: bool) -> Action {
    let lower = if key.is_ascii() {
        key.to_ascii_lowercase()
    } else {
        key.to_lowercase().next().unwrap_or(key)
    };
    match lower {
        's' => Action::Tone(1),
        'f' => Action::Tone(2),
        'r' => Action::Tone(3),
        'x' => Action::Tone(4),
        'j' => Action::Tone(5),
        'z' => Action::Tone(0),
        'a' | 'e' | 'o' => Action::Roof(Some(lower)),
        'd' => Action::Stroke,
        'w' if full => Action::TelexW,
        'w' => Action::Hook(HookKind::All),
        '[' | '{' if full => Action::MapLetter {
            base: 'o',
            upper: key == '{',
        },
        ']' | '}' if full => Action::MapLetter {
            base: 'u',
            upper: key == '}',
        },
        _ => Action::None,
    }
}

fn vni_action(key: char) -> Action {
    match key {
        '0' => Action::Tone(0),
        '1'..='5' => Action::Tone(key as u8 - b'0'),
        '6' => Action::Roof(None),
        '7' => Action::Hook(HookKind::HornOnly),
        '8' => Action::Hook(HookKind::BreveOnly),
        '9' => Action::Stroke,
        _ => Action::None,
    }
}

fn viqr_action(key: char) -> Action {
    match key {
        '\'' => Action::Tone(1),
        '`' => Action::Tone(2),
        '?' => Action::Tone(3),
        '~' => Action::Tone(4),
        '.' => Action::Tone(5),
        '^' => Action::Roof(None),
        '+' | '*' => Action::Hook(HookKind::HornOnly),
        '(' => Action::Hook(HookKind::BreveOnly),
        'd' | 'D' => Action::Stroke,
        '\\' => Action::Escape,
        _ => Action::None,
    }
}
