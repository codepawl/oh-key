//! Writes the Telex keystroke corpus used by `bench/unikey-compare`.
//!
//! `cargo run --release -p oh-key-engine --example compare_corpus -- <keys-file> <expected-file> [seed]`
//!
//! The corpus is built from Vietnamese prose plus hard rhymes, capitals, numbers
//! and a few English words. It is fully deterministic: the same build always
//! writes the same bytes. The keys file is what both engines replay; the
//! expected file is the text a correct engine should produce, word for word.

use std::fs;

const TARGET_KEY_COUNT: usize = 300_000;
const RANDOM_SEED: u64 = 0x0123_4567_89AB_CDEF;

/// Modern tone placement (hoà, khoẻ, thuý), the oh-key default.
const PROSE: &[&str] = &[
    "Tôi là người Việt Nam, đang gõ tiếng Việt bằng một chương trình nhanh và không lỗi.",
    "Mùa thu Hà Nội có gió nhẹ, lá vàng rơi trên con đường nhỏ quen thuộc.",
    "Thuở nhỏ, tôi hay ngồi bên cửa sổ nhìn mưa rơi trên mái nhà.",
    "Quốc gia nào cũng cần giữ gìn tiếng nói và chữ viết của dân tộc mình.",
    "Nguyễn Du viết Truyện Kiều bằng thể thơ lục bát, câu chữ giàu hình ảnh.",
    "Cô giáo dạy chúng tôi cách viết đúng chính tả và cách chọn từ cho hay.",
    "Buổi sáng, mẹ dậy sớm nấu cơm, bố đọc báo, còn em gái thì chạy ra sân tập thể dục.",
    "Anh ấy bị đau khuỷu tay nên hôm nay không chơi bóng cùng cả đội được.",
    "Chúng ta cần hoà bình, cần sức khoẻ và cần một cuộc sống yên ổn.",
    "Người nông dân ra đồng từ lúc trời chưa sáng, mang theo nắm cơm và bình nước.",
    "Con thuyền nhỏ nghiêng nghiêng theo từng con sóng, chầm chậm vào bến.",
    "Câu chuyện hôm qua làm tôi suy nghĩ rất nhiều về cách chúng ta đối xử với nhau.",
    "Quyết định này không dễ, nhưng tuỳ vào hoàn cảnh mà mỗi người sẽ chọn khác nhau.",
    "Hôm nay trời nắng đẹp, chúng tôi rủ nhau đi chơi công viên rồi ăn kem.",
    "Nhà ga cũ nằm cuối phố, nơi có quán cà phê bán món bánh mì thơm nức.",
    "Thành phố lên đèn, dòng xe nối đuôi nhau chạy qua cầu dưới ánh hoàng hôn.",
    "Bạn có biết vì sao bầu trời lại xanh, còn hoàng hôn thì đỏ không?",
    "Xin chào, tôi muốn đặt hai vé đi Đà Nẵng vào sáng thứ sáu tuần sau.",
    "Giá vé là 450.000 đồng, khởi hành lúc 7:30 và đến nơi lúc 9:15.",
    "Cuốn sách này có 320 trang, tác giả mất hơn 5 năm để hoàn thành.",
    "Ông bà tôi sống ở quê, vườn nhà trồng nhiều cây ăn quả như bưởi, ổi, xoài và chuối.",
    "Mỗi khi gió mùa về, cả xóm lại quây quần bên bếp lửa nghe bà kể chuyện cổ tích.",
    "Học sinh cần chuyên cần, giữ lời hứa và biết giúp đỡ bạn bè khi gặp khó khăn.",
    "Đừng bao giờ bỏ cuộc khi bạn chưa thử hết sức, vì thành công đến với người kiên trì.",
    "Tiếng chuông chùa ngân vang trong sương sớm, nghe thật thanh tịnh và bình yên.",
    "Cả lớp im lặng lắng nghe thầy giảng bài về lịch sử các triều đại phong kiến.",
    "Sông Mê Công chảy qua nhiều nước rồi đổ ra biển, mang theo phù sa màu mỡ.",
    "Mẹ dặn: hãy mặc thêm áo ấm, ngoài trời lạnh lắm!",
    "Chị ấy làm việc ở một công ty phần mềm, chuyên viết ứng dụng cho điện thoại.",
    "Đội tuyển thắng ba trận liên tiếp, khán giả reo hò suốt buổi chiều.",
    "Món phở nóng hổi với hành, rau thơm và chút chanh luôn là bữa sáng yêu thích của tôi.",
    "Trong rừng sâu, tiếng chim hót vang lên giữa những tán cây cổ thụ rậm rạp.",
    "Họ đã đi một quãng đường dài, qua đèo, qua suối, rồi mới thấy làng hiện ra.",
    "Hãy thử nghĩ xem, điều gì sẽ xảy ra nếu mọi người cùng nói thật?",
    "Cậu bé chăm chú nhìn con diều bay cao, đuôi diều uốn lượn như một dải lụa.",
    "Các nhà khoa học đang nghiên cứu cách tạo ra năng lượng sạch từ ánh nắng và gió.",
    "Chuyến tàu đêm chở đầy hành khách, ai cũng mệt nhưng vẫn mỉm cười khi về gần nhà.",
    "Cửa hàng mở cửa từ 8 giờ sáng đến 10 giờ tối, kể cả ngày lễ.",
    "Người ta thường nói rằng thời gian sẽ chữa lành mọi vết thương, nhưng cũng cần cả lòng kiên nhẫn.",
    "Khuya rồi, ngoài hiên chỉ còn tiếng côn trùng rả rích và ánh trăng loang trên sân.",
    "Giờ nghỉ trưa, bọn trẻ chạy ào ra sân chơi nhảy dây, đá cầu và chơi trốn tìm.",
    "Anh giữ chìa khoá, em giữ bản đồ, còn bé thì giữ chiếc ô màu vàng.",
    "Ngày mai là sinh nhật bạn tôi, nên tôi sẽ mua một bó hoa và một chiếc bánh nhỏ.",
];

/// Words chosen for the rhymes and clusters that trip up Telex engines.
const HARD_WORDS: &[&str] = &[
    "người",
    "khuỷu",
    "thuở",
    "quốc",
    "giữ",
    "Nguyễn",
    "đường",
    "thuyền",
    "chuyện",
    "quyết",
    "khoẻ",
    "hoà",
    "thuý",
    "tuỳ",
    "nghiêng",
    "nguyên",
    "xuyên",
    "chuyển",
    "truyền",
    "huyền",
    "khuyến",
    "quyền",
    "duyệt",
    "tuyệt",
    "hoạt",
    "hoàng",
    "ngoằn",
    "ngoèo",
    "khoảng",
    "loắt",
    "choắt",
    "oanh",
    "uyển",
    "nghiệp",
    "nghĩa",
    "nghỉ",
    "ngược",
    "những",
    "nhưng",
    "mượn",
    "lượn",
    "hươu",
    "rượu",
    "bướm",
    "cười",
    "tươi",
    "lưỡi",
    "gương",
    "giường",
    "giống",
    "giếng",
    "gìn",
    "gì",
    "giàu",
    "giúp",
    "quả",
    "quạt",
    "quẹt",
    "quỳnh",
    "quanh",
    "quý",
    "phở",
    "phượng",
    "thương",
    "trường",
    "trượt",
    "chuông",
    "buồng",
    "muống",
    "luỹ",
    "đứng",
    "đuổi",
    "đũa",
    "đưa",
    "đẹp",
    "đêm",
    "điều",
    "ước",
    "yêu",
    "yếu",
    "ỷ",
    "ỉ",
    "ở",
    "ừ",
    "ơi",
    "ôi",
    "ảnh",
    "ếch",
    "ách",
    "ức",
    "ăn",
    "ắt",
    "oai",
    "ngoại",
    "khoái",
    "xoáy",
    "loạn",
    "toàn",
    "hoặc",
    "hoạch",
    "ngạc",
    "thuần",
    "thuận",
    "huỳnh",
    "tiếng",
    "việt",
    "biển",
    "miền",
    "kiến",
    "thiết",
    "chiến",
    "nghiền",
    "xiết",
    "tiệc",
    "diệu",
    "hiểu",
];

const ENGLISH_WORDS: &[&str] = &[
    "text",
    "window",
    "Windows",
    "coffee",
    "Facebook",
    "software",
    "YouTube",
    "email",
    "project",
    "status",
    "download",
    "iPhone",
    "world",
    "class",
    "press",
    "fix",
    "boss",
    "Google",
    "Microsoft",
    "laptop",
    "dashboard",
    "shift",
    "summer",
    "order",
    "access",
    "success",
    "first",
    "were",
    "after",
    "worker",
    "stress",
    "pass",
    "file",
    "next",
    "review",
    "default",
    "user",
    "express",
    "street",
    "service",
];

const NUMBER_WORDS: &[&str] = &[
    "2026", "15", "3,5", "100%", "10:30", "42", "7", "1.000", "2k",
];

/// Vowel letters of one family: the plain form, then sắc, huyền, hỏi, ngã, nặng.
struct VowelFamily {
    forms: &'static str,
    base_key: char,
    modifier_key: Option<char>,
}

const VOWEL_FAMILIES: [VowelFamily; 12] = [
    VowelFamily {
        forms: "aáàảãạ",
        base_key: 'a',
        modifier_key: None,
    },
    VowelFamily {
        forms: "ăắằẳẵặ",
        base_key: 'a',
        modifier_key: Some('w'),
    },
    VowelFamily {
        forms: "âấầẩẫậ",
        base_key: 'a',
        modifier_key: Some('a'),
    },
    VowelFamily {
        forms: "eéèẻẽẹ",
        base_key: 'e',
        modifier_key: None,
    },
    VowelFamily {
        forms: "êếềểễệ",
        base_key: 'e',
        modifier_key: Some('e'),
    },
    VowelFamily {
        forms: "iíìỉĩị",
        base_key: 'i',
        modifier_key: None,
    },
    VowelFamily {
        forms: "oóòỏõọ",
        base_key: 'o',
        modifier_key: None,
    },
    VowelFamily {
        forms: "ôốồổỗộ",
        base_key: 'o',
        modifier_key: Some('o'),
    },
    VowelFamily {
        forms: "ơớờởỡợ",
        base_key: 'o',
        modifier_key: Some('w'),
    },
    VowelFamily {
        forms: "uúùủũụ",
        base_key: 'u',
        modifier_key: None,
    },
    VowelFamily {
        forms: "ưứừửữự",
        base_key: 'u',
        modifier_key: Some('w'),
    },
    VowelFamily {
        forms: "yýỳỷỹỵ",
        base_key: 'y',
        modifier_key: None,
    },
];

const TONE_KEYS: [Option<char>; 6] = [None, Some('s'), Some('f'), Some('r'), Some('x'), Some('j')];

pub struct RandomSource {
    pub state: u64,
}

impl RandomSource {
    pub fn next_u64(&mut self) -> u64 {
        self.state ^= self.state >> 12;
        self.state ^= self.state << 25;
        self.state ^= self.state >> 27;
        self.state.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    pub fn below(&mut self, limit: usize) -> usize {
        (self.next_u64() % limit as u64) as usize
    }

    /// True about `percent` times in a hundred.
    pub fn chance(&mut self, percent: usize) -> bool {
        self.below(100) < percent
    }
}

/// One typed letter: base keys, the optional modifier key (circumflex, horn, breve,
/// the second d of đ), its tone and whether it is a vowel letter.
struct TypedLetter {
    base_keys: String,
    modifier_key: Option<char>,
    tone: usize,
    is_vowel: bool,
}

fn type_letter(letter: char, upper_case_all_keys: bool) -> TypedLetter {
    let is_upper = letter.is_uppercase();
    let lower = letter.to_lowercase().next().unwrap_or(letter);
    let mut base_key = lower;
    let mut modifier_key = None;
    let mut tone = 0;
    let mut is_vowel = matches!(lower, 'a' | 'e' | 'i' | 'o' | 'u' | 'y');
    if lower == 'đ' {
        base_key = 'd';
        modifier_key = Some('d');
    } else if let Some(family) = VOWEL_FAMILIES
        .iter()
        .find(|family| family.forms.contains(lower))
    {
        tone = family
            .forms
            .chars()
            .position(|form| form == lower)
            .unwrap_or(0);
        base_key = family.base_key;
        modifier_key = family.modifier_key;
        is_vowel = true;
    }
    let base_upper = upper_case_all_keys || is_upper;
    TypedLetter {
        base_keys: if base_upper {
            base_key.to_ascii_uppercase()
        } else {
            base_key
        }
        .to_string(),
        modifier_key: modifier_key.map(|key| {
            if upper_case_all_keys {
                key.to_ascii_uppercase()
            } else {
                key
            }
        }),
        tone,
        is_vowel,
    }
}

/// How one word is typed.
pub struct TypingStyle {
    /// The tone key follows the last vowel instead of ending the word.
    pub tone_after_vowels: bool,
    /// ươ is typed "uow" instead of "uwow".
    pub short_horn_pair: bool,
    /// Every modifier key is typed after the whole word, before the tone ("duongwd").
    pub deferred_modifiers: bool,
}

fn merge_horn_pair(pieces: &mut [TypedLetter]) {
    for index in 1..pieces.len() {
        let previous_is_horn_u = pieces[index - 1].base_keys.eq_ignore_ascii_case("u")
            && pieces[index - 1]
                .modifier_key
                .is_some_and(|key| key.eq_ignore_ascii_case(&'w'));
        let current_is_horn_o = pieces[index].base_keys.eq_ignore_ascii_case("o")
            && pieces[index]
                .modifier_key
                .is_some_and(|key| key.eq_ignore_ascii_case(&'w'));
        if previous_is_horn_u && current_is_horn_o {
            pieces[index - 1].modifier_key = None;
        }
    }
}

pub fn telex_keys_for_word(word: &str, style: &TypingStyle) -> String {
    let letters: Vec<char> = word.chars().collect();
    let upper_count = letters
        .iter()
        .filter(|letter| letter.is_uppercase())
        .count();
    let all_caps = upper_count >= 2 && upper_count == letters.len();
    let mut pieces: Vec<TypedLetter> = letters
        .iter()
        .map(|letter| type_letter(*letter, all_caps))
        .collect();
    let tone = pieces.iter().map(|piece| piece.tone).max().unwrap_or(0);
    let last_vowel = pieces.iter().rposition(|piece| piece.is_vowel);
    if style.short_horn_pair || style.deferred_modifiers {
        merge_horn_pair(&mut pieces);
    }
    let tone_key = TONE_KEYS[tone].map(|key| {
        if all_caps {
            key.to_ascii_uppercase()
        } else {
            key
        }
    });
    let tone_slot = match (style.tone_after_vowels, last_vowel) {
        (true, Some(index)) => index + 1,
        _ => pieces.len(),
    };
    let mut keys = String::new();
    let mut deferred = String::new();
    for (index, piece) in pieces.iter().enumerate() {
        if index == tone_slot {
            keys.extend(tone_key);
        }
        keys.push_str(&piece.base_keys);
        if style.deferred_modifiers {
            deferred.extend(piece.modifier_key);
        } else {
            keys.extend(piece.modifier_key);
        }
    }
    keys.push_str(&deferred);
    if tone_slot == pieces.len() {
        keys.extend(tone_key);
    }
    keys
}

/// Splits a token such as `"(người,` into leading marks, the word and trailing marks.
fn split_token(token: &str) -> (&str, &str, &str) {
    let word_start = token.find(char::is_alphabetic).unwrap_or(token.len());
    let word_end = token
        .rfind(char::is_alphabetic)
        .map(|index| index + token[index..].chars().next().map_or(0, char::len_utf8))
        .unwrap_or(word_start);
    (
        &token[..word_start],
        &token[word_start..word_end],
        &token[word_end..],
    )
}

fn capitalize_first(word: &str) -> String {
    let mut characters = word.chars();
    let first = characters.next().into_iter().flat_map(char::to_uppercase);
    first.chain(characters).collect()
}

enum CaseChoice {
    AsWritten,
    Capitalized,
    AllCaps,
}

fn pick_case(random: &mut RandomSource) -> CaseChoice {
    if random.chance(2) {
        CaseChoice::AllCaps
    } else if random.chance(4) {
        CaseChoice::Capitalized
    } else {
        CaseChoice::AsWritten
    }
}

fn apply_case(word: &str, choice: &CaseChoice) -> String {
    match choice {
        CaseChoice::AsWritten => word.to_owned(),
        CaseChoice::Capitalized => capitalize_first(word),
        CaseChoice::AllCaps => word.to_uppercase(),
    }
}

pub fn pick_style(random: &mut RandomSource) -> TypingStyle {
    TypingStyle {
        tone_after_vowels: random.chance(15),
        short_horn_pair: random.chance(20),
        deferred_modifiers: random.chance(8),
    }
}

/// Takes the next token of a sentence, sometimes swapping the word for a hard word,
/// an English word or a number. Returns (keys, expected text).
fn render_token(token: &str, random: &mut RandomSource) -> (String, String) {
    let (prefix, word, suffix) = split_token(token);
    if word.is_empty() {
        return (token.to_owned(), token.to_owned());
    }
    if random.chance(5) {
        let english = ENGLISH_WORDS[random.below(ENGLISH_WORDS.len())];
        return (
            format!("{prefix}{english}{suffix}"),
            format!("{prefix}{english}{suffix}"),
        );
    }
    if random.chance(3) {
        let number = NUMBER_WORDS[random.below(NUMBER_WORDS.len())];
        return (
            format!("{prefix}{number}{suffix}"),
            format!("{prefix}{number}{suffix}"),
        );
    }
    let chosen = if random.chance(8) {
        HARD_WORDS[random.below(HARD_WORDS.len())]
    } else {
        word
    };
    if chosen.is_empty() {
        return (token.to_owned(), token.to_owned());
    }
    let expected_word = apply_case(chosen, &pick_case(random));
    let keys_word = telex_keys_for_word(&expected_word, &pick_style(random));
    (
        format!("{prefix}{keys_word}{suffix}"),
        format!("{prefix}{expected_word}{suffix}"),
    )
}

fn render_sentence(sentence: &str, random: &mut RandomSource) -> (String, String) {
    let mut keys = Vec::new();
    let mut expected = Vec::new();
    for token in sentence.split(' ') {
        let (token_keys, token_expected) = render_token(token, random);
        keys.push(token_keys);
        expected.push(token_expected);
    }
    (keys.join(" "), expected.join(" "))
}

fn main() {
    let arguments: Vec<String> = std::env::args().collect();
    let (keys_path, expected_path, seed) = match &arguments[1..] {
        [keys, expected] => (keys, expected, RANDOM_SEED),
        [keys, expected, seed] => (keys, expected, seed.parse().expect("seed is a number")),
        _ => {
            eprintln!("usage: compare_corpus <keys-file> <expected-file> [seed]");
            std::process::exit(2);
        }
    };
    let mut random = RandomSource { state: seed };
    let mut all_keys = String::new();
    let mut all_expected = String::new();
    while all_keys.len() < TARGET_KEY_COUNT {
        let sentence = PROSE[random.below(PROSE.len())];
        let (keys, expected) = render_sentence(sentence, &mut random);
        let separator = if random.chance(30) { '\n' } else { ' ' };
        all_keys.push_str(&keys);
        all_keys.push(separator);
        all_expected.push_str(&expected);
        all_expected.push(separator);
    }
    fs::write(keys_path, &all_keys).expect("write keys file");
    fs::write(expected_path, &all_expected).expect("write expected file");
    println!("{} keystrokes written to {keys_path}", all_keys.len());
}
