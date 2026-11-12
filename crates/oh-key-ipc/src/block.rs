//! The fixed-size memory block behind `SharedState`, free of any system call.
//!
//! The settings text sits behind a seqlock: a writer makes the sequence odd,
//! fills the payload and makes it even again; a reader keeps a copy only if the
//! sequence was even and unchanged around the copy. Every field is atomic, so
//! a read racing a write is defined behaviour and simply gets retried.

use crate::settings::{Settings, format_settings, parse_settings};
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering, fence};

pub(crate) const PAYLOAD_BYTES: usize = 8 * 1024;
const WORD_BYTES: usize = size_of::<u64>();
const WORD_COUNT: usize = PAYLOAD_BYTES / WORD_BYTES;
const READ_ATTEMPTS: usize = 10_000;
const WRITE_ATTEMPTS: usize = 100_000;

#[repr(C)]
pub(crate) struct StateBlock {
    sequence: AtomicU64,
    generation: AtomicU64,
    mode_counter: AtomicU64,
    /// Stored inverted so a zeroed block starts in Vietnamese mode.
    english: AtomicU32,
    reserved: AtomicU32,
    payload_length: AtomicU64,
    payload: [AtomicU64; WORD_COUNT],
}

impl StateBlock {
    #[cfg(test)]
    pub(crate) fn new() -> Self {
        StateBlock {
            sequence: AtomicU64::new(0),
            generation: AtomicU64::new(0),
            mode_counter: AtomicU64::new(0),
            english: AtomicU32::new(0),
            reserved: AtomicU32::new(0),
            payload_length: AtomicU64::new(0),
            payload: std::array::from_fn(|_| AtomicU64::new(0)),
        }
    }

    pub(crate) fn generation(&self) -> u64 {
        self.generation.load(Ordering::Acquire)
    }

    pub(crate) fn mode_counter(&self) -> u64 {
        self.mode_counter.load(Ordering::Acquire)
    }

    pub(crate) fn vietnamese(&self) -> bool {
        self.english.load(Ordering::Acquire) == 0
    }

    pub(crate) fn set_vietnamese(&self, on: bool) {
        self.english.store(u32::from(!on), Ordering::Release);
        self.mode_counter.fetch_add(1, Ordering::AcqRel);
    }

    pub(crate) fn publish_settings(&self, settings: &Settings) {
        let text = format_settings(settings);
        let bytes = fitting_prefix(&text, PAYLOAD_BYTES).as_bytes();
        let start_sequence = self.begin_write();
        // Keeps the payload stores below from becoming visible before the odd sequence.
        fence(Ordering::Release);
        self.payload_length
            .store(bytes.len() as u64, Ordering::Relaxed);
        for (word, chunk) in self.payload.iter().zip(bytes.chunks(WORD_BYTES)) {
            let mut buffer = [0u8; WORD_BYTES];
            buffer[..chunk.len()].copy_from_slice(chunk);
            word.store(u64::from_le_bytes(buffer), Ordering::Relaxed);
        }
        self.sequence
            .store(start_sequence.wrapping_add(2), Ordering::Release);
        self.generation.fetch_add(1, Ordering::AcqRel);
    }

    /// Takes the writer role by making the sequence odd. Returns the even
    /// value it had. A writer that died mid-write leaves the sequence odd
    /// for good, so after many attempts the next writer takes over.
    fn begin_write(&self) -> u64 {
        for _ in 0..WRITE_ATTEMPTS {
            let current = self.sequence.load(Ordering::Acquire);
            if current.is_multiple_of(2)
                && self
                    .sequence
                    .compare_exchange(
                        current,
                        current.wrapping_add(1),
                        Ordering::AcqRel,
                        Ordering::Relaxed,
                    )
                    .is_ok()
            {
                return current;
            }
            std::hint::spin_loop();
        }
        let abandoned = self.sequence.load(Ordering::Acquire);
        let even = abandoned & !1;
        self.sequence.store(even.wrapping_add(1), Ordering::Release);
        even
    }

    /// `None` until something was published, or when writers never let a
    /// consistent copy through.
    pub(crate) fn read_settings(&self) -> Option<Settings> {
        for _ in 0..READ_ATTEMPTS {
            let before = self.sequence.load(Ordering::Acquire);
            if before == 0 {
                return None;
            }
            if !before.is_multiple_of(2) {
                std::hint::spin_loop();
                continue;
            }
            let copy = self.copy_payload();
            fence(Ordering::Acquire);
            if self.sequence.load(Ordering::Relaxed) == before {
                return Some(parse_settings(&String::from_utf8_lossy(&copy)));
            }
        }
        None
    }

    fn copy_payload(&self) -> Vec<u8> {
        let length = (self.payload_length.load(Ordering::Relaxed) as usize).min(PAYLOAD_BYTES);
        let mut bytes = Vec::with_capacity(length.next_multiple_of(WORD_BYTES));
        for word in self.payload.iter().take(length.div_ceil(WORD_BYTES)) {
            bytes.extend_from_slice(&word.load(Ordering::Relaxed).to_le_bytes());
        }
        bytes.truncate(length);
        bytes
    }
}

/// The longest prefix within `limit` bytes that ends on a line break, so a
/// too-long file loses whole trailing settings rather than half a line.
fn fitting_prefix(text: &str, limit: usize) -> &str {
    if text.len() <= limit {
        return text;
    }
    let mut end = limit;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    match text[..end].rfind('\n') {
        Some(line_end) => &text[..=line_end],
        None => &text[..end],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicBool;

    #[test]
    fn reads_nothing_before_the_first_publish() {
        let block = StateBlock::new();
        assert!(block.read_settings().is_none());
        assert_eq!(block.generation(), 0);
    }

    #[test]
    fn published_settings_come_back_and_bump_the_generation() {
        let block = StateBlock::new();
        let settings = Settings {
            excluded_apps: vec!["game.exe".to_owned()],
            ..Settings::default()
        };
        block.publish_settings(&settings);
        assert_eq!(block.generation(), 1);
        assert_eq!(block.read_settings(), Some(settings));
    }

    #[test]
    fn mode_changes_bump_only_the_mode_counter() {
        let block = StateBlock::new();
        assert!(block.vietnamese());
        block.set_vietnamese(false);
        assert!(!block.vietnamese());
        assert_eq!(block.mode_counter(), 1);
        assert_eq!(block.generation(), 0);
    }

    #[test]
    fn oversized_settings_are_cut_on_a_line_boundary() {
        let block = StateBlock::new();
        let settings = Settings {
            excluded_apps: (0..2000).map(|index| format!("app{index}.exe")).collect(),
            ..Settings::default()
        };
        block.publish_settings(&settings);
        let read = block.read_settings().expect("published");
        assert_eq!(read.engine, settings.engine);
        assert!(read.excluded_apps.is_empty());
    }

    #[test]
    fn a_read_during_concurrent_writes_is_never_torn() {
        let block = StateBlock::new();
        let stop = AtomicBool::new(false);
        std::thread::scope(|scope| {
            scope.spawn(|| {
                for round in 0..3000 {
                    let name = format!("app{}.exe", round % 7);
                    let settings = Settings {
                        excluded_apps: vec![name; 20 + round % 40],
                        ..Settings::default()
                    };
                    block.publish_settings(&settings);
                }
                stop.store(true, Ordering::Release);
            });
            let mut good_reads = 0;
            while !stop.load(Ordering::Acquire) {
                let Some(settings) = block.read_settings() else {
                    continue;
                };
                let first = settings
                    .excluded_apps
                    .first()
                    .expect("excluded apps present");
                assert!(settings.excluded_apps.iter().all(|name| name == first));
                good_reads += 1;
            }
            assert!(good_reads > 0);
        });
    }
}
