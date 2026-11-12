//! One letter of the word being composed: a base letter, a mark and a case.
//! The tone is not stored here: a Vietnamese word carries one tone, kept on the word.

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Mark {
    None,
    /// Circumflex: â ê ô.
    Roof,
    /// Horn or breve: ă ơ ư.
    Hook,
    /// Crossbar: đ.
    Stroke,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct Letter {
    /// Lowercase base letter without any mark.
    pub base: char,
    pub mark: Mark,
    pub upper: bool,
}

/// Index of every Vietnamese vowel: a â ă e ê i o ô ơ u ư y.
const VOWEL_TONED_CHARS: [[char; 6]; 12] = [
    ['a', 'á', 'à', 'ả', 'ã', 'ạ'],
    ['â', 'ấ', 'ầ', 'ẩ', 'ẫ', 'ậ'],
    ['ă', 'ắ', 'ằ', 'ẳ', 'ẵ', 'ặ'],
    ['e', 'é', 'è', 'ẻ', 'ẽ', 'ẹ'],
    ['ê', 'ế', 'ề', 'ể', 'ễ', 'ệ'],
    ['i', 'í', 'ì', 'ỉ', 'ĩ', 'ị'],
    ['o', 'ó', 'ò', 'ỏ', 'õ', 'ọ'],
    ['ô', 'ố', 'ồ', 'ổ', 'ỗ', 'ộ'],
    ['ơ', 'ớ', 'ờ', 'ở', 'ỡ', 'ợ'],
    ['u', 'ú', 'ù', 'ủ', 'ũ', 'ụ'],
    ['ư', 'ứ', 'ừ', 'ử', 'ữ', 'ự'],
    ['y', 'ý', 'ỳ', 'ỷ', 'ỹ', 'ỵ'],
];

/// One ASCII code per vowel index, used to look up vowel sequences:
/// a, A = â, B = ă, e, E = ê, i, o, O = ô, P = ơ, u, U = ư, y.
const VOWEL_SEQUENCE_CODES: [u8; 12] = *b"aABeEioOPuUy";

impl Letter {
    pub fn new(base: char, mark: Mark, upper: bool) -> Letter {
        Letter { base, mark, upper }
    }

    /// A letter exactly as typed, without any mark.
    pub fn plain(key: char) -> Letter {
        let mut lowercase = key.to_lowercase();
        let base = lowercase.next().unwrap_or(key);
        let case_survives_round_trip =
            lowercase.next().is_none() && base.to_uppercase().eq(key.to_uppercase());
        if key.is_uppercase() && case_survives_round_trip {
            return Letter::new(base, Mark::None, true);
        }
        // Lowercase letters, and letters like İ and ǅ that do not lowercase and
        // capitalize back to themselves, are kept exactly as typed.
        Letter::new(key, Mark::None, false)
    }

    pub fn vowel_index(self) -> Option<usize> {
        let index = match (self.base, self.mark) {
            ('a', Mark::None) => 0,
            ('a', Mark::Roof) => 1,
            ('a', Mark::Hook) => 2,
            ('e', Mark::None) => 3,
            ('e', Mark::Roof) => 4,
            ('i', Mark::None) => 5,
            ('o', Mark::None) => 6,
            ('o', Mark::Roof) => 7,
            ('o', Mark::Hook) => 8,
            ('u', Mark::None) => 9,
            ('u', Mark::Hook) => 10,
            ('y', Mark::None) => 11,
            _ => return None,
        };
        Some(index)
    }

    pub fn is_vowel(self) -> bool {
        self.vowel_index().is_some()
    }

    pub fn sequence_code(self) -> Option<u8> {
        self.vowel_index().map(|index| VOWEL_SEQUENCE_CODES[index])
    }

    /// True for the letters a Vietnamese word may contain.
    pub fn is_vietnamese(self) -> bool {
        if self.is_vowel() {
            return true;
        }
        match self.base {
            'd' => matches!(self.mark, Mark::None | Mark::Stroke),
            'b' | 'c' | 'g' | 'h' | 'k' | 'l' | 'm' | 'n' | 'p' | 'q' | 'r' | 's' | 't' | 'v'
            | 'x' => self.mark == Mark::None,
            _ => false,
        }
    }

    /// Byte used when looking up consonant clusters; the stroked d is `D`.
    pub fn cluster_code(self) -> u8 {
        if self.base == 'd' && self.mark == Mark::Stroke {
            b'D'
        } else {
            self.base as u8
        }
    }

    /// The NFC character for this letter carrying `tone` (0 = no tone).
    pub fn to_char(self, tone: u8) -> char {
        let lower = match self.vowel_index() {
            Some(index) => VOWEL_TONED_CHARS[index][usize::from(tone)],
            None if self.base == 'd' && self.mark == Mark::Stroke => 'đ',
            None => self.base,
        };
        if !self.upper {
            return lower;
        }
        lower.to_uppercase().next().unwrap_or(lower)
    }
}
