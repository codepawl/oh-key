//! The mode switch sound: two soft mallet notes, rising for Vietnamese and falling for
//! English. Synthesised once at first use, so no audio file ships, as in oh-snap.

use std::sync::OnceLock;

use windows::Win32::Media::Audio::{PlaySoundW, SND_ASYNC, SND_MEMORY, SND_NODEFAULT};
use windows::core::PCWSTR;

const SAMPLE_RATE: u32 = 44_100;
/// G4 and C5: a fourth apart, low enough that nothing lands in the shrill 2 to 6 kHz band.
const LOW_NOTE_HZ: f32 = 392.0;
const HIGH_NOTE_HZ: f32 = 523.25;
const SECOND_NOTE_SECONDS: f32 = 0.065;
const LENGTH_SECONDS: f32 = 0.34;
const FADE_OUT_SECONDS: f32 = 0.03;
/// Peak level; about -13 dBFS, quieter than the Windows notification sounds.
const PEAK: f32 = 0.22;

static VIETNAMESE_WAV: OnceLock<Vec<u8>> = OnceLock::new();
static ENGLISH_WAV: OnceLock<Vec<u8>> = OnceLock::new();

/// Plays without blocking; a sound still playing is cut off by the new one.
pub fn play_mode_switch(vietnamese: bool) {
    let wav = if vietnamese {
        VIETNAMESE_WAV.get_or_init(|| build_mode_wav(true))
    } else {
        ENGLISH_WAV.get_or_init(|| build_mode_wav(false))
    };
    unsafe {
        let _ = PlaySoundW(
            PCWSTR(wav.as_ptr() as *const u16),
            None,
            SND_MEMORY | SND_ASYNC | SND_NODEFAULT,
        );
    }
}

/// For checking by ear or with a spectrogram: `oh-key --write-sounds <folder>` writes
/// `mode-vietnamese.wav` and `mode-english.wav`.
pub fn write_mode_sounds(folder: &std::path::Path) -> std::io::Result<()> {
    std::fs::create_dir_all(folder)?;
    std::fs::write(folder.join("mode-vietnamese.wav"), build_mode_wav(true))?;
    std::fs::write(folder.join("mode-english.wav"), build_mode_wav(false))
}

fn build_mode_wav(vietnamese: bool) -> Vec<u8> {
    wav_bytes(&to_pcm(&mode_samples(vietnamese)))
}

fn mode_samples(vietnamese: bool) -> Vec<f32> {
    let (first_hz, second_hz) = if vietnamese {
        (LOW_NOTE_HZ, HIGH_NOTE_HZ)
    } else {
        (HIGH_NOTE_HZ, LOW_NOTE_HZ)
    };
    let length = (SAMPLE_RATE as f32 * LENGTH_SECONDS) as usize;
    let mut samples = vec![0.0f32; length];
    add_mallet_note(&mut samples, 0.0, first_hz, 0.8);
    add_mallet_note(&mut samples, SECOND_NOTE_SECONDS, second_hz, 1.0);
    fade_out(&mut samples, FADE_OUT_SECONDS);
    normalise(&mut samples, PEAK);
    samples
}

/// A wooden mallet: a sine with a quiet octave that dies faster, a 4 ms attack so it does
/// not click, and an exponential decay.
fn add_mallet_note(samples: &mut [f32], start_seconds: f32, frequency_hz: f32, loudness: f32) {
    let start = (start_seconds * SAMPLE_RATE as f32) as usize;
    let attack_seconds = 0.004;
    for (offset, sample) in samples.iter_mut().skip(start).enumerate() {
        let time = offset as f32 / SAMPLE_RATE as f32;
        let attack = (time / attack_seconds).min(1.0);
        let phase = std::f32::consts::TAU * frequency_hz * time;
        let body = phase.sin() * (-time * 16.0).exp();
        let octave = (2.0 * phase).sin() * (-time * 45.0).exp() * 0.3;
        *sample += (body + octave) * attack * loudness;
    }
}

/// Ramps the last `seconds` down to zero, so the file never ends on a click.
fn fade_out(samples: &mut [f32], seconds: f32) {
    let fade_length = ((seconds * SAMPLE_RATE as f32) as usize).min(samples.len());
    let fade_start = samples.len() - fade_length;
    for (index, sample) in samples[fade_start..].iter_mut().enumerate() {
        *sample *= 1.0 - index as f32 / fade_length as f32;
    }
}

fn normalise(samples: &mut [f32], peak: f32) {
    let loudest = samples
        .iter()
        .fold(0.0f32, |loudest, sample| loudest.max(sample.abs()));
    if loudest <= 0.0 {
        return;
    }
    let gain = peak / loudest;
    for sample in samples.iter_mut() {
        *sample *= gain;
    }
}

fn to_pcm(samples: &[f32]) -> Vec<i16> {
    samples
        .iter()
        .map(|sample| (sample.clamp(-1.0, 1.0) * i16::MAX as f32) as i16)
        .collect()
}

fn wav_bytes(pcm: &[i16]) -> Vec<u8> {
    let data_size = (pcm.len() * 2) as u32;
    let mut bytes = Vec::with_capacity(44 + data_size as usize);
    bytes.extend_from_slice(b"RIFF");
    bytes.extend_from_slice(&(36 + data_size).to_le_bytes());
    bytes.extend_from_slice(b"WAVEfmt ");
    bytes.extend_from_slice(&16u32.to_le_bytes());
    bytes.extend_from_slice(&1u16.to_le_bytes());
    bytes.extend_from_slice(&1u16.to_le_bytes());
    bytes.extend_from_slice(&SAMPLE_RATE.to_le_bytes());
    bytes.extend_from_slice(&(SAMPLE_RATE * 2).to_le_bytes());
    bytes.extend_from_slice(&2u16.to_le_bytes());
    bytes.extend_from_slice(&16u16.to_le_bytes());
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&data_size.to_le_bytes());
    for sample in pcm {
        bytes.extend_from_slice(&sample.to_le_bytes());
    }
    bytes
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_wav_header_matches_its_data() {
        let wav = build_mode_wav(true);
        assert_eq!(&wav[0..4], b"RIFF");
        let data_size = u32::from_le_bytes([wav[40], wav[41], wav[42], wav[43]]) as usize;
        assert_eq!(wav.len(), 44 + data_size);
    }

    #[test]
    fn the_sound_peaks_at_the_set_level_and_ends_silent() {
        for vietnamese in [true, false] {
            let samples = mode_samples(vietnamese);
            let loudest = samples.iter().fold(0.0f32, |max, s| max.max(s.abs()));
            assert!((loudest - PEAK).abs() < 1e-4);
            let tail = samples[samples.len() - 100..]
                .iter()
                .fold(0.0f32, |max, s| max.max(s.abs()));
            assert!(tail < PEAK * 0.02, "tail {tail}");
        }
    }

    #[test]
    fn the_two_modes_sound_different() {
        assert_ne!(mode_samples(true), mode_samples(false));
    }
}
