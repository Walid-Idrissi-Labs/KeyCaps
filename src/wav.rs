//! Reads the recordings on Linux, where no system library does it: 16-bit PCM WAV files,
//! brought to the output rate.

use std::f64::consts::PI;
use std::path::Path;

/// Decodes a 16-bit PCM WAV file to mono at `rate`. Empty if it is missing or anything else.
pub(crate) fn decode(path: &Path, rate: f64) -> Vec<f32> {
    let d = std::fs::read(path).unwrap_or_default();
    let u16_at = |i: usize| d.get(i..i + 2).map_or(0, |b| u16::from_le_bytes([b[0], b[1]])) as usize;
    let u32_at = |i: usize| d.get(i..i + 4).map_or(0, |b| u32::from_le_bytes([b[0], b[1], b[2], b[3]])) as usize;
    if d.get(..4) != Some(b"RIFF") || d.get(8..12) != Some(b"WAVE") {
        return Vec::new();
    }
    // Chunks: four bytes of name, four of length, then the content, padded to an even length.
    let (mut at, mut channels, mut from) = (12, 0, 0.0);
    while at + 8 <= d.len() {
        let (name, len, body) = (&d[at..at + 4], u32_at(at + 4), at + 8);
        if name == b"fmt " && u16_at(body) == 1 && u16_at(body + 14) == 16 {
            (channels, from) = (u16_at(body + 2), u32_at(body + 4) as f64);
        } else if name == b"data" && channels > 0 {
            let frames = d[body..(body + len).min(d.len())].chunks_exact(2 * channels);
            let mono: Vec<f32> = frames.map(|f| f.chunks_exact(2).map(|b| i16::from_le_bytes([b[0], b[1]]) as f32).sum::<f32>() / (32768.0 * channels as f32)).collect();
            return resample(&mono, from, rate);
        }
        at = body + len + (len & 1);
    }
    Vec::new()
}

/// Band-limited resampling: every output sample is a Hann-windowed sinc sum over the 32
/// input samples around it.
fn resample(x: &[f32], from: f64, to: f64) -> Vec<f32> {
    if from == to || from <= 0.0 {
        return x.to_vec();
    }
    const HALF: isize = 16;
    let cutoff = (to / from).min(1.0); // going down, also remove what the lower rate cannot hold
    let sample = |i: usize| {
        let t = i as f64 * from / to;
        let first = t.floor() as isize - HALF + 1;
        let tap = |k: isize| {
            let d = t - k as f64;
            let sinc = if d == 0.0 { 1.0 } else { (PI * d * cutoff).sin() / (PI * d * cutoff) };
            x[k as usize] as f64 * sinc * cutoff * (0.5 + 0.5 * (PI * d / HALF as f64).cos())
        };
        (first.max(0)..(first + 2 * HALF).min(x.len() as isize)).map(tap).sum::<f64>() as f32
    };
    (0..(x.len() as f64 * to / from) as usize).map(sample).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_wav_file_is_read_and_brought_to_the_output_rate() {
        // A tenth of a second of a 1 kHz tone at 44.1 kHz in stereo, the right channel
        // silent, behind a chunk of odd length that has to be skipped.
        let tone = |i: usize, rate: f64| (2.0 * PI * 1000.0 * i as f64 / rate).sin();
        let samples: Vec<u8> = (0..4410).flat_map(|i| [(tone(i, 44100.0) * 32767.0) as i16, 0]).flat_map(i16::to_le_bytes).collect();
        let mut file = b"RIFF\0\0\0\0WAVEjunk\x03\0\0\0abc\0fmt \x10\0\0\0\x01\0\x02\0".to_vec();
        file.extend([44100u32.to_le_bytes(), 176400u32.to_le_bytes(), [4, 0, 16, 0]].concat());
        file.extend(b"data");
        file.extend((samples.len() as u32).to_le_bytes());
        file.extend(samples);
        let path = std::env::temp_dir().join(format!("keycaps-test-{}.wav", std::process::id()));
        std::fs::write(&path, &file).unwrap();
        let y = decode(&path, 48000.0);
        // Not a WAV file at all, and one that is not 16-bit.
        std::fs::write(&path, &file[1..]).unwrap();
        let (garbage, missing) = (decode(&path, 48000.0), decode(Path::new("/no/such/file.wav"), 48000.0));
        file[46] = 24;
        std::fs::write(&path, &file).unwrap();
        let other = decode(&path, 48000.0);
        std::fs::remove_file(&path).unwrap();

        assert_eq!(y.len(), 4800);
        // Halved by the downmix; the ends lack neighbours to interpolate from.
        let worst = (100..4700).map(|i| (y[i] as f64 - 0.5 * tone(i, 48000.0)).abs()).fold(0.0, f64::max);
        assert!(worst < 0.002, "off by {worst}");
        assert!(garbage.is_empty() && missing.is_empty() && other.is_empty());
    }
}
