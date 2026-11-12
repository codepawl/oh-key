//! Vietnamese syllable tables, ported as facts about the language: which vowel
//! sequences exist, which consonants may start or end a syllable and which
//! vowel/final pairs are legal. Vowel sequences are written with the codes of
//! `letter::VOWEL_SEQUENCE_CODES` (A = â, B = ă, E = ê, O = ô, P = ơ, U = ư).
//!
//! The vowel sequence lists are expanded into lookup arrays at compile time, so
//! checking a word costs an array read instead of a long chain of comparisons.

pub(crate) const FINAL_C: u8 = 1;
pub(crate) const FINAL_CH: u8 = 2;
pub(crate) const FINAL_M: u8 = 4;
pub(crate) const FINAL_N: u8 = 8;
pub(crate) const FINAL_NG: u8 = 16;
pub(crate) const FINAL_NH: u8 = 32;
pub(crate) const FINAL_P: u8 = 64;
pub(crate) const FINAL_T: u8 = 128;
const ANY_FINAL: u8 = 255;

/// Finals that refuse the hỏi, ngã and huyền tones.
pub(crate) const STOP_FINALS: u8 = FINAL_C | FINAL_CH | FINAL_P | FINAL_T;

const NO_NG: u8 = FINAL_C | FINAL_CH | FINAL_M | FINAL_N | FINAL_NH | FINAL_P | FINAL_T;
const ROUND: u8 = FINAL_C | FINAL_M | FINAL_N | FINAL_NG | FINAL_P | FINAL_T;
const NO_P: u8 = FINAL_C | FINAL_M | FINAL_N | FINAL_NG | FINAL_T;

/// Vowel sequences that are words as they stand.
const COMPLETE_SEQUENCES: &[&[u8]] = &[
    b"a", b"A", b"B", b"e", b"E", b"i", b"o", b"O", b"P", b"u", b"U", b"y", b"iE", b"oa", b"oB",
    b"oe", b"ua", b"uA", b"uE", b"uO", b"uP", b"uy", b"UP", b"yE", b"uyE", b"ai", b"ao", b"au",
    b"ay", b"Au", b"Ay", b"eo", b"Eu", b"ia", b"iu", b"oi", b"Oi", b"Pi", b"ui", b"Ua", b"Ui",
    b"Uu", b"iEu", b"oai", b"oay", b"oeo", b"uAy", b"uOi", b"uya", b"uyu", b"UPi", b"UPu", b"yEu",
];

/// Intermediate forms that only become words once a mark is added.
const INCOMPLETE_SEQUENCES: &[&[u8]] = &[
    b"ie", b"ue", b"uo", b"Uo", b"ye", b"uye", b"eu", b"uu", b"ieu", b"uay", b"uoi", b"uou",
    b"uPi", b"uPu", b"Uoi", b"Uou", b"yeu",
];

/// Finals that may follow each group of vowel sequences.
const FINALS_AFTER_SEQUENCES: &[(&[&[u8]], u8)] = &[
    (&[b"a", b"e", b"oa"], ANY_FINAL),
    (
        &[
            b"A", b"B", b"o", b"O", b"u", b"ie", b"iE", b"uo", b"Uo", b"UP",
        ],
        ROUND,
    ),
    (&[b"E", b"i"], NO_NG),
    (&[b"P"], FINAL_M | FINAL_N | FINAL_P | FINAL_T),
    (&[b"U", b"uO", b"oB"], NO_P),
    (&[b"y"], FINAL_T),
    (&[b"oe"], FINAL_N | FINAL_T),
    (&[b"ua", b"uA"], FINAL_N | FINAL_NG | FINAL_T),
    (&[b"ue", b"uE"], FINAL_C | FINAL_CH | FINAL_N | FINAL_NH),
    (
        &[b"uy"],
        FINAL_C | FINAL_CH | FINAL_N | FINAL_NH | FINAL_P | FINAL_T,
    ),
    (&[b"ye"], FINAL_M | FINAL_N | FINAL_NG | FINAL_P | FINAL_T),
    (&[b"yE"], FINAL_M | FINAL_N | FINAL_NG | FINAL_T),
    (&[b"uye", b"uyE"], FINAL_N | FINAL_T),
];

const VOWEL_CODES: &[u8; 12] = b"aABeEioOPuUy";
/// Codes are ranked 1 to 12, so a sequence reads as a base 13 number and no
/// two sequences share a slot. The longest sequence has three vowels.
const SEQUENCE_SLOTS: usize = 13 * 13 * 13;
const NOT_A_SEQUENCE: u8 = 0;
const COMPLETE: u8 = 1;
const INCOMPLETE: u8 = 2;

const fn code_rank(code: u8) -> usize {
    let mut index = 0;
    while index < VOWEL_CODES.len() {
        if VOWEL_CODES[index] == code {
            return index + 1;
        }
        index += 1;
    }
    0
}

const fn sequence_slot(sequence: &[u8]) -> usize {
    let mut slot = 0;
    let mut index = 0;
    while index < sequence.len() {
        slot = slot * 13 + code_rank(sequence[index]);
        index += 1;
    }
    slot
}

const fn completeness_table() -> [u8; SEQUENCE_SLOTS] {
    let mut table = [NOT_A_SEQUENCE; SEQUENCE_SLOTS];
    let mut index = 0;
    while index < COMPLETE_SEQUENCES.len() {
        table[sequence_slot(COMPLETE_SEQUENCES[index])] = COMPLETE;
        index += 1;
    }
    index = 0;
    while index < INCOMPLETE_SEQUENCES.len() {
        table[sequence_slot(INCOMPLETE_SEQUENCES[index])] = INCOMPLETE;
        index += 1;
    }
    table
}

const fn allowed_finals_table() -> [u8; SEQUENCE_SLOTS] {
    let mut table = [0; SEQUENCE_SLOTS];
    let mut group = 0;
    while group < FINALS_AFTER_SEQUENCES.len() {
        let (sequences, finals) = FINALS_AFTER_SEQUENCES[group];
        let mut index = 0;
        while index < sequences.len() {
            table[sequence_slot(sequences[index])] = finals;
            index += 1;
        }
        group += 1;
    }
    table
}

static COMPLETENESS: [u8; SEQUENCE_SLOTS] = completeness_table();
static ALLOWED_FINALS: [u8; SEQUENCE_SLOTS] = allowed_finals_table();

/// True when `sequence` is a vowel sequence that is complete as it stands.
/// `None` means the sequence does not exist in Vietnamese at all.
pub(crate) fn vowel_sequence_is_complete(sequence: &[u8]) -> Option<bool> {
    match COMPLETENESS[sequence_slot(sequence)] {
        COMPLETE => Some(true),
        INCOMPLETE => Some(false),
        _ => None,
    }
}

/// Bit set of the finals that may follow `sequence`.
pub(crate) fn allowed_finals(sequence: &[u8]) -> u8 {
    ALLOWED_FINALS[sequence_slot(sequence)]
}

/// Bit for a final consonant cluster, 0 when it cannot end a syllable.
pub(crate) fn final_bit(cluster: &[u8]) -> u8 {
    match cluster {
        [b'c'] => FINAL_C,
        [b'c', b'h'] => FINAL_CH,
        [b'm'] => FINAL_M,
        [b'n'] => FINAL_N,
        [b'n', b'g'] => FINAL_NG,
        [b'n', b'h'] => FINAL_NH,
        [b'p'] => FINAL_P,
        [b't'] => FINAL_T,
        _ => 0,
    }
}

/// Initial consonant clusters; `D` stands for the stroked d.
/// Matched by length first, so a lookup compares at most a handful of bytes.
pub(crate) fn is_initial_cluster(cluster: &[u8]) -> bool {
    match cluster {
        [letter] => matches!(
            letter,
            b'b' | b'c'
                | b'd'
                | b'D'
                | b'g'
                | b'h'
                | b'k'
                | b'l'
                | b'm'
                | b'n'
                | b'p'
                | b'q'
                | b'r'
                | b's'
                | b't'
                | b'v'
                | b'x'
        ),
        [first, second] => matches!(
            (first, second),
            (b'c', b'h')
                | (b'd', b'z')
                | (b'g', b'h')
                | (b'g', b'i')
                | (b'k', b'h')
                | (b'n', b'g')
                | (b'n', b'h')
                | (b'p', b'h')
                | (b'q', b'u')
                | (b't', b'h')
                | (b't', b'r')
        ),
        [b'n', b'g', b'h'] => true,
        _ => false,
    }
}

/// The initial/vowel pairs that never occur: no "gii", no "quu", and `k`
/// only before e, ê, i, y and the diphthongs that start with them.
pub(crate) fn initial_accepts_vowels(initial: &[u8], sequence: &[u8]) -> bool {
    match initial {
        b"gi" => sequence.first() != Some(&b'i'),
        b"qu" => sequence.first() != Some(&b'u'),
        b"q" => false,
        b"k" => matches!(
            sequence,
            b"e" | b"i"
                | b"y"
                | b"E"
                | b"eo"
                | b"eu"
                | b"Eu"
                | b"ia"
                | b"ie"
                | b"iE"
                | b"ieu"
                | b"iEu"
        ),
        _ => true,
    }
}

/// Pairs that fail the vowel/final table but are still words: quynh, quỵch, giêng; quyc is the prefix of quych.
pub(crate) fn is_vowel_final_exception(initial: &[u8], sequence: &[u8], final_bit: u8) -> bool {
    match initial {
        b"qu" => sequence == b"y" && final_bit & (FINAL_C | FINAL_CH | FINAL_N | FINAL_NH) != 0,
        b"gi" => matches!(sequence, b"e" | b"E") && final_bit & (FINAL_N | FINAL_NG) != 0,
        _ => false,
    }
}
