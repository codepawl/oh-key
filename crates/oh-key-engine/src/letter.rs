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

const NOT_A_VOWEL: u8 = u8::MAX;

/// Vowel index for each ASCII base letter and mark (None, Roof, Hook, Stroke),
/// so asking whether a letter is a vowel is one array read.
const VOWEL_INDEX_BY_BASE: [[u8; 4]; 128] = vowel_index_table();

const fn vowel_index_table() -> [[u8; 4]; 128] {
    let mut table = [[NOT_A_VOWEL; 4]; 128];
    table[b'a' as usize] = [0, 1, 2, NOT_A_VOWEL];
    table[b'e' as usize] = [3, 4, NOT_A_VOWEL, NOT_A_VOWEL];
    table[b'i' as usize][0] = 5;
    table[b'o' as usize] = [6, 7, 8, NOT_A_VOWEL];
    table[b'u' as usize][0] = 9;
    table[b'u' as usize][2] = 10;
    table[b'y' as usize][0] = 11;
    table
}

impl Letter {
    pub fn new(base: char, mark: Mark, upper: bool) -> Letter {
        Letter { base, mark, upper }
    }

    /// A letter exactly as typed, without any mark.
    pub fn plain(key: char) -> Letter {
        if key.is_ascii() {
            return Letter::new(
                key.to_ascii_lowercase(),
                Mark::None,
                key.is_ascii_uppercase(),
            );
        }
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
        let base = self.base as usize;
        if base >= VOWEL_INDEX_BY_BASE.len() {
            return None;
        }
        match VOWEL_INDEX_BY_BASE[base][self.mark as usize] {
            NOT_A_VOWEL => None,
            index => Some(usize::from(index)),
        }
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
