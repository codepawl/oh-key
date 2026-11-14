//! What an IBus key event means to the typing engine: keysym and modifier bits in, a kind of key
//! out. Pure numbers, so it is tested on every system.

pub const STATE_SHIFT: u32 = 1 << 0;
pub const STATE_CONTROL: u32 = 1 << 2;
pub const STATE_ALT: u32 = 1 << 3;
pub const STATE_SUPER_MOD4: u32 = 1 << 6;
pub const STATE_SUPER: u32 = 1 << 26;
pub const STATE_META: u32 = 1 << 28;
pub const STATE_RELEASE: u32 = 1 << 30;

const SHORTCUT_MODIFIERS: u32 =
    STATE_CONTROL | STATE_ALT | STATE_SUPER_MOD4 | STATE_SUPER | STATE_META;

pub const KEYSYM_BACKSPACE: u32 = 0xff08;
const KEYSYM_SHIFT_LEFT: u32 = 0xffe1;
const KEYSYM_SHIFT_RIGHT: u32 = 0xffe2;
const KEYSYM_CONTROL_LEFT: u32 = 0xffe3;
const KEYSYM_CONTROL_RIGHT: u32 = 0xffe4;
const KEYSYM_FIRST_MODIFIER: u32 = 0xffe1;
const KEYSYM_LAST_MODIFIER: u32 = 0xffee;
const KEYSYM_KEYPAD_FIRST: u32 = 0xffaa;
const KEYSYM_KEYPAD_LAST: u32 = 0xffb9;
const KEYSYM_KEYPAD_EQUAL: u32 = 0xffbd;
const KEYSYM_UNICODE_FIRST: u32 = 0x0100_0100;
const KEYSYM_UNICODE_OFFSET: u32 = 0x0100_0000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ModifierKey {
    Shift,
    Control,
    Other,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyKind {
    /// Shift, Control, Alt, Super, Caps Lock and the like: never part of a word.
    Modifier(ModifierKey),
    Backspace,
    Printable(char),
    /// Enter, Tab, Escape, arrows and every other key that ends the word without text.
    WordBreak,
}

pub fn classify_keysym(keysym: u32) -> KeyKind {
    if keysym == KEYSYM_BACKSPACE {
        return KeyKind::Backspace;
    }
    if (KEYSYM_FIRST_MODIFIER..=KEYSYM_LAST_MODIFIER).contains(&keysym) {
        return KeyKind::Modifier(modifier_key(keysym));
    }
    match printable_character(keysym) {
        Some(character) => KeyKind::Printable(character),
        None => KeyKind::WordBreak,
    }
}

fn modifier_key(keysym: u32) -> ModifierKey {
    match keysym {
        KEYSYM_SHIFT_LEFT | KEYSYM_SHIFT_RIGHT => ModifierKey::Shift,
        KEYSYM_CONTROL_LEFT | KEYSYM_CONTROL_RIGHT => ModifierKey::Control,
        _ => ModifierKey::Other,
    }
}

fn printable_character(keysym: u32) -> Option<char> {
    match keysym {
        // Latin-1 keysyms equal their Unicode scalar values.
        0x20..=0x7e | 0xa0..=0xff => char::from_u32(keysym),
        KEYSYM_UNICODE_FIRST..=0x0110_ffff => char::from_u32(keysym - KEYSYM_UNICODE_OFFSET),
        KEYSYM_KEYPAD_FIRST..=KEYSYM_KEYPAD_LAST => keypad_character(keysym),
        KEYSYM_KEYPAD_EQUAL => Some('='),
        _ => None,
    }
}

fn keypad_character(keysym: u32) -> Option<char> {
    match keysym {
        0xffaa => Some('*'),
        0xffab => Some('+'),
        0xffad => Some('-'),
        0xffae => Some('.'),
        0xffaf => Some('/'),
        0xffb0..=0xffb9 => char::from_u32('0' as u32 + (keysym - 0xffb0)),
        _ => None,
    }
}

/// Control, Alt or Super held: the key is a shortcut, not text.
pub fn is_shortcut(state: u32) -> bool {
    state & SHORTCUT_MODIFIERS != 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn letters_and_digits_are_printable() {
        assert_eq!(classify_keysym('a' as u32), KeyKind::Printable('a'));
        assert_eq!(classify_keysym(0x20), KeyKind::Printable(' '));
        assert_eq!(classify_keysym(0xffb7), KeyKind::Printable('7'));
    }

    #[test]
    fn unicode_keysyms_map_to_their_character() {
        assert_eq!(classify_keysym(0x0100_1ec7), KeyKind::Printable('ệ'));
    }

    #[test]
    fn navigation_keys_end_the_word() {
        for keysym in [0xff0d, 0xff09, 0xff1b, 0xff51, 0xffff] {
            assert_eq!(classify_keysym(keysym), KeyKind::WordBreak);
        }
    }

    #[test]
    fn modifiers_and_backspace_are_recognised() {
        assert_eq!(
            classify_keysym(0xffe1),
            KeyKind::Modifier(ModifierKey::Shift)
        );
        assert_eq!(
            classify_keysym(0xffe4),
            KeyKind::Modifier(ModifierKey::Control)
        );
        assert_eq!(
            classify_keysym(0xffe9),
            KeyKind::Modifier(ModifierKey::Other)
        );
        assert_eq!(classify_keysym(KEYSYM_BACKSPACE), KeyKind::Backspace);
    }

    #[test]
    fn shortcut_needs_control_alt_or_super_but_not_shift() {
        assert!(is_shortcut(STATE_CONTROL));
        assert!(is_shortcut(STATE_ALT | STATE_SHIFT));
        assert!(!is_shortcut(STATE_SHIFT));
        assert!(!is_shortcut(0));
    }
}
