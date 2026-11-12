//! Pure translation from a Windows key event to a decision for the engine.
//!
//! Nothing here touches COM, so the rules for shortcuts, boundaries and the
//! language toggle chord are covered by plain unit tests.

pub const VK_BACK: u32 = 0x08;
pub const VK_TAB: u32 = 0x09;
pub const VK_RETURN: u32 = 0x0D;
pub const VK_SHIFT: u32 = 0x10;
pub const VK_CONTROL: u32 = 0x11;
pub const VK_MENU: u32 = 0x12;
pub const VK_ESCAPE: u32 = 0x1B;
pub const VK_LSHIFT: u32 = 0xA0;
pub const VK_RSHIFT: u32 = 0xA1;
pub const VK_LCONTROL: u32 = 0xA2;
pub const VK_RCONTROL: u32 = 0xA3;
pub const VK_LMENU: u32 = 0xA4;
pub const VK_RMENU: u32 = 0xA5;
/// Page Up, Page Down, End, Home, the arrows, Select, Print, Execute, Snapshot, Insert, Delete.
const FIRST_NAVIGATION_KEY: u32 = 0x21;
const LAST_NAVIGATION_KEY: u32 = 0x2E;
const FIRST_FUNCTION_KEY: u32 = 0x70;
const LAST_FUNCTION_KEY: u32 = 0x87;

/// Everything the decision needs about one key press.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct KeyEvent {
    pub virtual_key: u32,
    /// The character the layout produces for this press, if it is printable.
    pub character: Option<char>,
    pub ctrl: bool,
    pub alt: bool,
    /// Right Alt on many layouts reports Ctrl and Alt together.
    pub alt_gr: bool,
    pub windows_key: bool,
}

/// What the text service does with a key.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyDecision {
    /// Not ours and nothing to commit: the application gets the key.
    PassThrough,
    /// Commit the composing word, then let the application have the key.
    CommitThenPassThrough,
    /// Feed this character to the engine.
    Process(char),
    /// Feed a backspace to the engine.
    Backspace,
}

pub fn decide_key(event: &KeyEvent, composing: bool) -> KeyDecision {
    if is_modifier_key(event.virtual_key) {
        return KeyDecision::PassThrough;
    }
    let shortcut_held = (event.ctrl || event.alt) && !event.alt_gr;
    if shortcut_held || event.windows_key {
        return commit_if_composing(composing);
    }
    if event.virtual_key == VK_BACK {
        return if composing {
            KeyDecision::Backspace
        } else {
            KeyDecision::PassThrough
        };
    }
    if closes_word_without_text(event.virtual_key) {
        return commit_if_composing(composing);
    }
    match event.character {
        Some(character) if !character.is_control() => KeyDecision::Process(character),
        _ => commit_if_composing(composing),
    }
}

/// Whether `OnTestKeyDown` should claim the key before the engine ran.
///
/// With no word open, only letters can start one; everything else goes to the
/// application untouched.
pub fn will_consume(decision: KeyDecision, composing: bool) -> bool {
    match decision {
        KeyDecision::Process(character) => composing || character.is_alphabetic(),
        KeyDecision::Backspace => true,
        KeyDecision::PassThrough | KeyDecision::CommitThenPassThrough => false,
    }
}

fn commit_if_composing(composing: bool) -> KeyDecision {
    if composing {
        KeyDecision::CommitThenPassThrough
    } else {
        KeyDecision::PassThrough
    }
}

fn closes_word_without_text(virtual_key: u32) -> bool {
    matches!(virtual_key, VK_RETURN | VK_TAB | VK_ESCAPE)
        || (FIRST_NAVIGATION_KEY..=LAST_NAVIGATION_KEY).contains(&virtual_key)
        || (FIRST_FUNCTION_KEY..=LAST_FUNCTION_KEY).contains(&virtual_key)
}

pub fn is_modifier_key(virtual_key: u32) -> bool {
    matches!(
        virtual_key,
        VK_SHIFT
            | VK_CONTROL
            | VK_MENU
            | VK_LSHIFT
            | VK_RSHIFT
            | VK_LCONTROL
            | VK_RCONTROL
            | VK_LMENU
            | VK_RMENU
    )
}

fn is_shift_key(virtual_key: u32) -> bool {
    matches!(virtual_key, VK_SHIFT | VK_LSHIFT | VK_RSHIFT)
}

fn is_control_key(virtual_key: u32) -> bool {
    matches!(virtual_key, VK_CONTROL | VK_LCONTROL | VK_RCONTROL)
}

/// Detects the Ctrl+Shift chord: both pressed and released with no other key
/// between, the same behaviour Unikey users know.
#[derive(Debug, Default)]
pub struct ToggleChordTracker {
    control_seen: bool,
    shift_seen: bool,
    chord_broken: bool,
}

impl ToggleChordTracker {
    pub fn key_down(&mut self, virtual_key: u32) {
        if is_control_key(virtual_key) {
            self.control_seen = true;
        } else if is_shift_key(virtual_key) {
            self.shift_seen = true;
        } else {
            self.chord_broken = true;
        }
    }

    /// Returns true once, when the chord completes on release.
    pub fn key_up(&mut self, virtual_key: u32) -> bool {
        if !is_control_key(virtual_key) && !is_shift_key(virtual_key) {
            return false;
        }
        let completed = self.control_seen && self.shift_seen && !self.chord_broken;
        *self = ToggleChordTracker::default();
        completed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn letter(character: char) -> KeyEvent {
        KeyEvent {
            virtual_key: u32::from(character.to_ascii_uppercase()),
            character: Some(character),
            ..KeyEvent::default()
        }
    }

    #[test]
    fn letters_go_to_the_engine() {
        assert_eq!(decide_key(&letter('a'), false), KeyDecision::Process('a'));
        assert_eq!(decide_key(&letter('A'), true), KeyDecision::Process('A'));
    }

    #[test]
    fn shortcuts_commit_first_and_pass_through() {
        let copy = KeyEvent {
            ctrl: true,
            ..letter('c')
        };
        assert_eq!(decide_key(&copy, true), KeyDecision::CommitThenPassThrough);
        assert_eq!(decide_key(&copy, false), KeyDecision::PassThrough);
        let alt_tab = KeyEvent {
            virtual_key: VK_TAB,
            alt: true,
            ..KeyEvent::default()
        };
        assert_eq!(
            decide_key(&alt_tab, true),
            KeyDecision::CommitThenPassThrough
        );
    }

    #[test]
    fn alt_gr_still_types_characters() {
        let event = KeyEvent {
            ctrl: true,
            alt: true,
            alt_gr: true,
            ..letter('e')
        };
        assert_eq!(decide_key(&event, false), KeyDecision::Process('e'));
    }

    #[test]
    fn enter_arrows_and_escape_commit_without_consuming() {
        for virtual_key in [VK_RETURN, VK_TAB, VK_ESCAPE, 0x25, 0x24, 0x71] {
            let event = KeyEvent {
                virtual_key,
                ..KeyEvent::default()
            };
            assert_eq!(decide_key(&event, true), KeyDecision::CommitThenPassThrough);
            assert_eq!(decide_key(&event, false), KeyDecision::PassThrough);
        }
    }

    #[test]
    fn backspace_belongs_to_the_engine_only_while_composing() {
        let event = KeyEvent {
            virtual_key: VK_BACK,
            ..KeyEvent::default()
        };
        assert_eq!(decide_key(&event, true), KeyDecision::Backspace);
        assert_eq!(decide_key(&event, false), KeyDecision::PassThrough);
    }

    #[test]
    fn modifiers_alone_never_interrupt_a_word() {
        let event = KeyEvent {
            virtual_key: VK_SHIFT,
            ..KeyEvent::default()
        };
        assert_eq!(decide_key(&event, true), KeyDecision::PassThrough);
    }

    #[test]
    fn punctuation_is_a_boundary_only_while_composing() {
        let comma = KeyEvent {
            virtual_key: 0xBC,
            character: Some(','),
            ..KeyEvent::default()
        };
        let decision = decide_key(&comma, true);
        assert_eq!(decision, KeyDecision::Process(','));
        assert!(will_consume(decision, true));
        assert!(!will_consume(decision, false));
    }

    #[test]
    fn only_letters_start_a_word() {
        assert!(will_consume(KeyDecision::Process('a'), false));
        assert!(!will_consume(KeyDecision::Process('1'), false));
        assert!(will_consume(KeyDecision::Backspace, true));
        assert!(!will_consume(KeyDecision::CommitThenPassThrough, true));
    }

    #[test]
    fn dead_keys_without_a_character_close_the_word() {
        let event = KeyEvent {
            virtual_key: 0xDE,
            character: None,
            ..KeyEvent::default()
        };
        assert_eq!(decide_key(&event, true), KeyDecision::CommitThenPassThrough);
    }

    #[test]
    fn ctrl_shift_chord_toggles_once_on_release() {
        let mut tracker = ToggleChordTracker::default();
        tracker.key_down(VK_CONTROL);
        tracker.key_down(VK_SHIFT);
        assert!(tracker.key_up(VK_SHIFT));
        assert!(!tracker.key_up(VK_CONTROL));
    }

    #[test]
    fn another_key_between_cancels_the_chord() {
        let mut tracker = ToggleChordTracker::default();
        tracker.key_down(VK_CONTROL);
        tracker.key_down(VK_SHIFT);
        tracker.key_down(u32::from(b'Z'));
        assert!(!tracker.key_up(VK_SHIFT));
    }

    #[test]
    fn one_modifier_alone_does_not_toggle() {
        let mut tracker = ToggleChordTracker::default();
        tracker.key_down(VK_SHIFT);
        assert!(!tracker.key_up(VK_SHIFT));
    }
}
