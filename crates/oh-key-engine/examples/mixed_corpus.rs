//! Writes the mixed Vietnamese / English Telex corpus used by `bench/unikey-compare`.
//!
//! `cargo run --release -p oh-key-engine --example mixed_corpus -- <keys> <expected> <categories> [passes]`
//!
//! Seven categories of tokens, each with the text a correct engine should show:
//! English tokens come out as the exact keys typed, Vietnamese tokens as the
//! Vietnamese word. The categories file holds one letter per token, laid out
//! like the keys file, so the report can group results. Category `b` holds
//! English words that are also valid Telex Vietnamese: the report lists what
//! each engine does with them instead of calling one reading wrong.

use std::fs;

#[allow(dead_code)]
#[path = "compare_corpus.rs"]
mod telex_typing;

use telex_typing::{RandomSource, pick_style, telex_keys_for_word};

const TARGET_KEY_COUNT: usize = 300_000;
const RANDOM_SEED: u64 = 0x0FED_CBA9_8765_4321;
const WORDS_PER_LINE: usize = 6;

/// English words that collide with Telex keys: doubled letters and w/s/f/r/x/j/z/d.
const COLLIDING_ENGLISH: &[&str] = &[
    "book",
    "good",
    "too",
    "see",
    "issue",
    "status",
    "fix",
    "box",
    "tax",
    "max",
    "rows",
    "law",
    "saw",
    "new",
    "now",
    "how",
    "will",
    "off",
    "coffee",
    "pass",
    "boss",
    "address",
    "success",
    "google",
    "facebook",
    "github",
    "window",
    "words",
    "draw",
    "jazz",
    "quiz",
    "add",
    "odd",
    "dd",
    "feed",
    "seed",
    "need",
    "keep",
    "deep",
    "week",
    "street",
    "tree",
    "free",
    "three",
    "green",
    "moon",
    "room",
    "look",
    "took",
    "cool",
    "school",
    "football",
    "wood",
    "food",
    "door",
    "floor",
    "pool",
    "sleep",
    "sheep",
    "sweet",
    "meet",
    "email",
    "read",
    "reader",
    "user",
    "users",
    "size",
    "sort",
    "press",
    "class",
    "glass",
    "grass",
    "less",
    "mess",
    "miss",
    "kiss",
    "process",
    "access",
    "express",
    "progress",
    "dress",
    "stress",
    "wow",
    "flow",
    "show",
    "slow",
    "grow",
    "throw",
    "know",
    "follow",
    "yellow",
    "today",
    "way",
    "away",
    "day",
    "play",
    "stay",
    "say",
    "pay",
    "key",
    "money",
    "screen",
    "six",
    "mix",
    "fox",
    "next",
    "text",
    "exit",
    "exam",
    "example",
    "expand",
    "experience",
    "except",
    "wrote",
    "store",
    "firefox",
    "xbox",
    "ask",
    "task",
    "desk",
    "test",
    "best",
    "rest",
    "first",
    "worst",
    "jump",
    "just",
    "major",
    "project",
    "object",
    "reject",
    "json",
    "java",
    "request",
    "queue",
    "square",
    "quote",
    "file",
    "find",
    "fast",
    "far",
    "fire",
    "focus",
    "from",
    "front",
    "foot",
    "force",
    "format",
    "forward",
    "world",
    "word",
    "work",
    "worse",
    "doors",
    "dish",
    "wish",
    "wash",
    "push",
    "fresh",
    "flash",
    "flex",
    "software",
    "download",
    "dashboard",
    "shift",
    "summer",
    "order",
    "worker",
    "default",
    "service",
    "review",
    "twitter",
    "better",
    "letter",
    "little",
    "middle",
    "happy",
    "yes",
    "hello",
    "should",
    "would",
    "could",
    "through",
    "right",
    "night",
    "light",
    "sight",
    "again",
    "always",
    "because",
    "before",
    "between",
    "different",
    "everyone",
    "something",
    "without",
];

/// English words that are also valid Telex Vietnamese or become so with one tone key.
const AMBIGUOUS_ENGLISH: &[&str] = &[
    "as", "is", "to", "me", "he", "an", "on", "or", "of", "at", "so", "do", "go", "no", "la", "ma",
    "ta", "ban", "tin", "phone", "us", "if", "ox", "ex", "ax", "her", "per", "mar", "bar", "car",
    "jar", "war", "sir", "fur", "bus", "gas", "his", "has", "was", "mas", "pas", "mix", "tux",
    "lax", "wax", "sax", "mom", "moms", "ass", "bass", "tas", "does", "goes", "toes", "nor", "for",
    "our", "four", "hour", "your", "more", "sure", "care", "share", "here", "there", "were",
    "where", "one", "none", "tone", "done", "gone", "lone", "man", "men", "tan", "ten", "hen",
    "pen", "den", "son", "ton", "pan", "fan", "han", "can", "nam", "sam", "tam", "lam", "van",
    "win", "tim", "him", "dim", "rim", "sim", "ok", "oh", "uh", "ah", "eh", "hi",
];

const BRANDS_AND_NAMES: &[&str] = &[
    "iPhone",
    "YouTube",
    "Messi",
    "Samsung",
    "Windows",
    "Google",
    "Facebook",
    "Twitter",
    "Instagram",
    "TikTok",
    "Netflix",
    "Spotify",
    "Amazon",
    "Apple",
    "Microsoft",
    "Nvidia",
    "Intel",
    "Toshiba",
    "Sony",
    "Xiaomi",
    "Huawei",
    "Oppo",
    "Vivo",
    "Shopee",
    "Lazada",
    "Grab",
    "Zalo",
    "Tesla",
    "Ronaldo",
    "Neymar",
    "Mbappe",
    "Beckham",
    "Haaland",
    "Salah",
    "Mercedes",
    "Toyota",
    "Honda",
    "Yamaha",
    "Suzuki",
    "Nokia",
    "Adobe",
    "Photoshop",
    "Slack",
    "Discord",
    "Reddit",
    "WhatsApp",
    "Telegram",
    "Pinterest",
    "LinkedIn",
    "GitHub",
    "Wikipedia",
    "Python",
    "JavaScript",
    "TypeScript",
    "Rust",
    "Linux",
    "Ubuntu",
    "Android",
    "macOS",
    "iOS",
    "PlayStation",
    "Xbox",
    "Nintendo",
    "Minecraft",
    "Fortnite",
    "Zoom",
    "OpenAI",
    "ChatGPT",
    "Claude",
    "Gemini",
    "Anthropic",
    "Obama",
    "Trump",
    "Biden",
    "Putin",
    "Elon",
    "Musk",
    "Zuckerberg",
    "Newton",
    "Einstein",
    "Shakespeare",
    "Paris",
    "London",
    "Tokyo",
    "Seoul",
    "Berlin",
    "Madrid",
    "Texas",
    "Florida",
    "Boston",
    "Chicago",
    "Mexico",
    "Brazil",
    "Russia",
    "Japan",
    "Korea",
    "McDonald",
    "Starbucks",
    "Uber",
    "Airbnb",
    "Steam",
    "Docker",
    "Kubernetes",
];

const CODE_AND_TECH: &[&str] = &[
    "camelCase",
    "getUserName",
    "isValid",
    "XMLHttpRequest",
    "snake_case",
    "max_value",
    "user_id",
    "__init__",
    "https://github.com/codepawl/oh-key",
    "a.b@example.com",
    "an.nx@gmail.com",
    "C:\\Users\\an\\file.txt",
    "/usr/local/bin",
    "./src/main.rs",
    "v1.2.3",
    "2.0.1-beta",
    "0xFF",
    "0xdeadbeef",
    "#ff00aa",
    "--release",
    "--help",
    "-rf",
    "-v",
    "git",
    "npm",
    "cargo",
    "sudo",
    "ssh",
    "chmod",
    "console.log(\"hi\");",
    "foo(bar)",
    "x=1",
    "a+b",
    "i++",
    "if(x>0){",
    "std::vec::Vec",
    "user.name",
    "item[0]",
    "{key:value}",
    "SELECT",
    "FROM",
    "WHERE",
    "users",
    "localhost:8080",
    "192.168.1.1",
    "#include",
    "<div>",
    "</div>",
    "class=\"box\"",
    "printf(\"%d\")",
    "mp3",
    "h264",
    "win11",
    "4k",
    "2fa",
    "utf-8",
    "x86_64",
    "node_modules",
    "package.json",
    "Cargo.toml",
    "README.md",
    "main.rs",
    "index.html",
    "style.css",
    "docker-compose.yml",
    "foo.bar.baz",
    "$HOME",
    "%APPDATA%",
    "~/.ssh/id_rsa",
    "git@github.com:codepawl/oh-key.git",
    "fix/typo-in-readme",
    "feat:",
    "refactor(engine):",
    "TODO",
    "FIXME",
    "null",
    "undefined",
    "true",
    "false",
    "async",
    "await",
    "const",
    "let",
    "return",
    "function",
    "struct",
    "impl",
    "match",
    "self",
    "super",
    "crate",
    "mod",
    "pub",
];

/// Whole sentences as people write them. A token with a Vietnamese letter is typed in Telex.
const MIXED_SENTENCES: &[&str] = &[
    "deadline thứ hai",
    "check mail đi",
    "họp online lúc 3h",
    "file excel bị lỗi",
    "mai mình gửi report cho sếp nhé",
    "bug này fix xong rồi, em push code lên github đi",
    "anh ơi cho em xin link zoom với",
    "hôm nay làm việc ở nhà, work from home",
    "mình đang dùng windows 11 và chrome",
    "em cài lại driver cho card màn hình",
    "tối nay đi cafe không?",
    "quên password rồi, reset giúp mình với",
    "gửi file pdf qua gmail cho khách",
    "meeting dời sang thứ sáu nhé",
    "update app lên bản mới nhất đi",
    "nhớ backup dữ liệu trước khi cài lại windows",
    "đặt vé máy bay qua app vietjet",
    "đang chờ approve từ manager",
    "code review xong thì merge nhé",
    "server bị down từ sáng",
    "anh share screen giúp em",
    "wifi yếu quá, restart router đi",
    "thanks bạn nhiều nha, see you",
    "phim mới trên netflix hay lắm",
    "mua laptop dell hay macbook?",
    "em vừa deploy xong rồi, test lại giúp anh",
    "đổi mật khẩu wifi thành 12345678",
    "tôi dùng iPhone chứ không phải Samsung",
    "đây là task của sprint này",
    "cuối tuần chill ở coffee shop",
    "hồ sơ nộp qua email trước 5pm",
    "sếp bảo làm gấp, urgent lắm",
    "người dùng nhập sai username hoặc password",
    "vui lòng tải file về máy rồi mở bằng excel",
    "gõ lệnh git status để xem thay đổi",
    "phần mềm này free nhưng có quảng cáo",
    "mạng chậm nên video bị lag",
    "update status trên jira giúp mình",
    "lỗi 404 not found, trang không tồn tại",
    "đăng nhập bằng tài khoản google",
    "em gửi anh bản draft, anh góp ý giúp em nhé",
    "hôm qua team họp về roadmap quý sau",
    "mình thích dùng dark mode hơn light mode",
    "file này nặng quá, nén thành zip rồi gửi",
    "bạn có biết cách export ra pdf không?",
];

const CAPITALISED_AND_ALL_CAPS: &[&str] = &[
    "HELLO",
    "WORLD",
    "BOOK",
    "GOOD",
    "NEW",
    "NOW",
    "FIX",
    "STATUS",
    "ISSUE",
    "ASAP",
    "FAQ",
    "HTTP",
    "JSON",
    "HTML",
    "CSS",
    "API",
    "URL",
    "PDF",
    "USB",
    "WIFI",
    "OK",
    "NASA",
    "FBI",
    "IBM",
    "TOO",
    "SEE",
    "WILL",
    "OFF",
    "PASS",
    "BOSS",
    "DONE",
    "README",
    "TODO",
    "SQL",
    "GPU",
    "CPU",
    "RAM",
    "SSD",
    "AIRPORT",
    "WINDOW",
    "WORDS",
    "JAZZ",
    "QUIZ",
    "SUCCESS",
    "ADDRESS",
    "Book",
    "Good",
    "Status",
    "Issue",
    "Window",
    "Words",
    "Draw",
    "Jazz",
    "Quiz",
    "Boss",
    "Pass",
    "Address",
    "Success",
    "Monday",
    "Tuesday",
    "Friday",
    "January",
    "April",
    "Hello",
    "Thanks",
    "Please",
    "Sorry",
    "Welcome",
    "Download",
    "Settings",
    "Search",
    "Files",
    "Review",
    "Today",
    "Tomorrow",
    "Yesterday",
    "Weekend",
    "Summer",
    "Winter",
    "School",
    "Football",
    "Wood",
];

const ENGLISH_WITH_PUNCTUATION: &[&str] = &[
    "book,",
    "good.",
    "see!",
    "what?",
    "(window)",
    "\"status\"",
    "fix:",
    "issue;",
    "pass...",
    "off-line",
    "e-mail",
    "well-known",
    "don't",
    "it's",
    "can't",
    "won't",
    "I'm",
    "rock'n'roll",
    "[sic]",
    "word/words",
    "a/b",
    "yes/no",
    "$100",
    "50%",
    "3.5mm",
    "(see",
    "boss)",
    "Mr.",
    "Dr.",
    "etc.",
    "e.g.",
    "i.e.",
    "U.S.",
    "good,",
    "will:",
    "now!!!",
    "so?",
    "today,",
    "tomorrow.",
    "works!",
    "(fix)",
    "text:",
    "boxes;",
    "tax.",
    "max,",
    "rows)",
    "law?",
    "saw!",
    "new,",
    "how?",
    "off.",
    "pass,",
    "windows;",
    "wow!",
    "ok.",
    "yes!",
    "no?",
    "sure,",
    "thanks!",
    "class:",
    "file.",
    "index?",
    "user)",
    "\"words\"",
    "'draw'",
    "...status",
    "-fix",
    "+1",
    "@user",
    "#hashtag",
    "&more",
    "100%.",
    "next,",
    "then:",
    "first;",
    "last.",
    "also!",
];

#[derive(Clone, Copy)]
enum Category {
    CollidingEnglish,
    AmbiguousEnglish,
    BrandsAndNames,
    CodeAndTech,
    MixedSentence,
    CapitalisedOrAllCaps,
    EnglishWithPunctuation,
}

impl Category {
    fn letter(self) -> char {
        match self {
            Category::CollidingEnglish => 'a',
            Category::AmbiguousEnglish => 'b',
            Category::BrandsAndNames => 'c',
            Category::CodeAndTech => 'd',
            Category::MixedSentence => 'e',
            Category::CapitalisedOrAllCaps => 'f',
            Category::EnglishWithPunctuation => 'g',
        }
    }
}

struct Line {
    keys: Vec<String>,
    expected: Vec<String>,
    category: Category,
}

fn has_vietnamese_letter(token: &str) -> bool {
    !token.is_ascii()
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

fn keys_for_token(token: &str, random: &mut RandomSource) -> String {
    if !has_vietnamese_letter(token) {
        return token.to_owned();
    }
    let (prefix, word, suffix) = split_token(token);
    let typed = telex_keys_for_word(word, &pick_style(random));
    format!("{prefix}{typed}{suffix}")
}

fn shuffled(words: &[&'static str], random: &mut RandomSource) -> Vec<&'static str> {
    let mut remaining = words.to_vec();
    for index in (1..remaining.len()).rev() {
        remaining.swap(index, random.below(index + 1));
    }
    remaining
}

fn word_lines(words: &[&'static str], category: Category, random: &mut RandomSource) -> Vec<Line> {
    shuffled(words, random)
        .chunks(WORDS_PER_LINE)
        .map(|chunk| Line {
            keys: chunk.iter().map(|word| (*word).to_owned()).collect(),
            expected: chunk.iter().map(|word| (*word).to_owned()).collect(),
            category,
        })
        .collect()
}

fn sentence_lines(random: &mut RandomSource) -> Vec<Line> {
    shuffled(MIXED_SENTENCES, random)
        .into_iter()
        .map(|sentence| {
            let tokens: Vec<&str> = sentence.split(' ').collect();
            Line {
                keys: tokens
                    .iter()
                    .map(|token| keys_for_token(token, random))
                    .collect(),
                expected: tokens.iter().map(|token| (*token).to_owned()).collect(),
                category: Category::MixedSentence,
            }
        })
        .collect()
}

fn one_pass(random: &mut RandomSource) -> Vec<Line> {
    let mut lines = Vec::new();
    lines.extend(word_lines(
        COLLIDING_ENGLISH,
        Category::CollidingEnglish,
        random,
    ));
    lines.extend(word_lines(
        AMBIGUOUS_ENGLISH,
        Category::AmbiguousEnglish,
        random,
    ));
    lines.extend(word_lines(
        BRANDS_AND_NAMES,
        Category::BrandsAndNames,
        random,
    ));
    lines.extend(word_lines(CODE_AND_TECH, Category::CodeAndTech, random));
    lines.extend(sentence_lines(random));
    lines.extend(word_lines(
        CAPITALISED_AND_ALL_CAPS,
        Category::CapitalisedOrAllCaps,
        random,
    ));
    lines.extend(word_lines(
        ENGLISH_WITH_PUNCTUATION,
        Category::EnglishWithPunctuation,
        random,
    ));
    for index in (1..lines.len()).rev() {
        lines.swap(index, random.below(index + 1));
    }
    lines
}

fn main() {
    let arguments: Vec<String> = std::env::args().collect();
    let (keys_path, expected_path, categories_path, passes) = match &arguments[1..] {
        [keys, expected, categories] => (keys, expected, categories, None),
        [keys, expected, categories, passes] => (
            keys,
            expected,
            categories,
            Some(passes.parse::<usize>().expect("passes is a number")),
        ),
        _ => {
            eprintln!("usage: mixed_corpus <keys> <expected> <categories> [passes]");
            std::process::exit(2);
        }
    };
    let mut random = RandomSource { state: RANDOM_SEED };
    let mut all_keys = String::new();
    let mut all_expected = String::new();
    let mut all_categories = String::new();
    let mut finished_passes = 0;
    loop {
        for line in one_pass(&mut random) {
            all_keys.push_str(&line.keys.join(" "));
            all_keys.push('\n');
            all_expected.push_str(&line.expected.join(" "));
            all_expected.push('\n');
            let letter = line.category.letter().to_string();
            let letters: Vec<&str> = line.keys.iter().map(|_| letter.as_str()).collect();
            all_categories.push_str(&letters.join(" "));
            all_categories.push('\n');
        }
        finished_passes += 1;
        let done = match passes {
            Some(count) => finished_passes >= count,
            None => all_keys.len() >= TARGET_KEY_COUNT,
        };
        if done {
            break;
        }
    }
    fs::write(keys_path, &all_keys).expect("write keys file");
    fs::write(expected_path, &all_expected).expect("write expected file");
    fs::write(categories_path, &all_categories).expect("write categories file");
    println!(
        "{} keystrokes, {finished_passes} passes written to {keys_path}",
        all_keys.len()
    );
}
