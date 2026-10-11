//! Synthetic switch packs: noise shaped by a measured table, with no recording involved
//! when they play. For every sound, `synth.bin` holds the level of 12 frequency bands
//! over twenty 4 ms frames, measured once from one recorded pack per switch type by the
//! `measure` test below.

use std::f32::consts::TAU;

const BANDS: usize = 12;
const FRAMES: usize = 20;
const FRAME_SECS: f32 = 0.004;
/// The noise is always made at this rate and resampled for the device, so that every
/// device plays the very waveform the table was fitted to.
const RATE: f32 = 48000.0;
/// Sounds per pack: the key slots of `audio`, presses then releases.
const SOUNDS: usize = 12;

/// (directory name used in `audio::PACKS`, recorded pack it was measured from).
pub const PACKS: [(&str, &str); 3] = [("synth-linear", "cream"), ("synth-tactile", "holypanda"), ("synth-clicky", "mxblue")];

/// Levels as [pack][sound][band][frame], 0 = silent, then 1..=255 = -60..0 dB.
static TABLE: &[u8; PACKS.len() * SOUNDS * BANDS * FRAMES] = include_bytes!("synth.bin");

/// Band centres, half an octave apart from 180 Hz to 12 kHz.
fn centre(band: usize) -> f32 {
    180.0 * (12000.0f32 / 180.0).powf(band as f32 / (BANDS - 1) as f32)
}

/// Half-octave band-pass, in place: two RBJ biquads (Q 2) in series, which keeps a
/// neighbouring band 11 dB down while the bands still add up flat to within 1 dB.
fn band_pass(x: &mut [f32], f: f32) {
    let w = TAU * f / RATE;
    let alpha = w.sin() / (2.0 * 2.0);
    let (b0, a1, a2) = (alpha / (1.0 + alpha), -2.0 * w.cos() / (1.0 + alpha), (1.0 - alpha) / (1.0 + alpha));
    for _ in 0..2 {
        let (mut x1, mut x2, mut y1, mut y2) = (0f32, 0f32, 0f32, 0f32);
        for v in x.iter_mut() {
            let y = b0 * (*v - x2) - a1 * y1 - a2 * y2;
            (x2, x1, y2, y1) = (x1, *v, y1, y);
            *v = y;
        }
    }
}

/// Noise shaped to one sound's [band][frame] levels, at `RATE`. The same `sound` always
/// gives the same waveform.
fn noise_bands(table: &[u8], sound: usize) -> Vec<f32> {
    let frame = FRAME_SECS * RATE;
    let n = (FRAMES as f32 * frame) as usize;
    let warm = (0.01 * RATE) as usize; // lets each filter settle before the sound starts
    let mut noise = vec![0f32; warm + n];
    let mut out = vec![0f32; n];
    let mut seed = 0x9E37_79B9 ^ sound as u32;
    for band in 0..BANDS {
        for v in &mut noise {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            *v = seed as f32 / 2_147_483_648.0 - 1.0;
        }
        band_pass(&mut noise, centre(band));
        let y = &noise[warm..];
        let rms = (y.iter().map(|v| v * v).sum::<f32>() / n as f32).sqrt().max(1e-9);
        // Natural log of each frame's amplitude: 0..=255 is -60..0 dB.
        let level: [f32; FRAMES] = std::array::from_fn(|k| (table[band * FRAMES + k] as f32 / 255.0 - 1.0) * 3.0 * std::f32::consts::LN_10);
        for (i, o) in out.iter_mut().enumerate() {
            // Interpolate between frame centres in dB, which is how these sounds decay (a
            // straight line in amplitude smears a hit into the next frame). Before the
            // first centre the sound starts at full level, which is what makes it a click.
            let p = (i as f32 / frame - 0.5).max(0.0);
            let (a, b) = (level[(p as usize).min(FRAMES - 1)], level[(p as usize + 1).min(FRAMES - 1)]);
            *o += y[i] / rms * (a + (b - a) * p.fract()).exp();
        }
    }
    out
}

/// Renders sound `slot` of the synthetic pack `dir` at `sr`. None if `dir` is not one.
pub fn render(dir: &str, slot: usize, sr: f32) -> Option<Vec<f32>> {
    let sound = PACKS.iter().position(|p| p.0 == dir)? * SOUNDS + slot;
    let table = TABLE.get(sound * BANDS * FRAMES..)?.get(..BANDS * FRAMES).filter(|_| slot < SOUNDS)?;
    let x = noise_bands(table, sound);
    // Linear resampling: at most 1 dB down at 12 kHz, the top of what the table describes.
    let at = |i: usize| x.get(i).copied().unwrap_or(0.0);
    Some((0..(x.len() as f32 * sr / RATE) as usize).map(|i| i as f32 * RATE / sr).map(|p| at(p as usize) + (at(p as usize + 1) - at(p as usize)) * p.fract()).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::{FILES, decode, shape};

    /// The band levels of `x` over time, quantised like the table.
    fn analyse(x: &[f32]) -> [u8; BANDS * FRAMES] {
        let frame = (FRAME_SECS * RATE) as usize;
        let mut rms = [0f32; BANDS * FRAMES];
        for band in 0..BANDS {
            let mut y = x.to_vec();
            y.resize(FRAMES * frame, 0.0);
            band_pass(&mut y, centre(band));
            for k in 0..FRAMES {
                rms[band * FRAMES + k] = (y[k * frame..][..frame].iter().map(|v| v * v).sum::<f32>() / frame as f32).sqrt();
            }
        }
        let top = rms.iter().fold(1e-9f32, |m, v| m.max(*v));
        rms.map(|v| (255.0 * (1.0 + (v / top).log10() / 3.0)).clamp(0.0, 255.0).round() as u8)
    }

    /// The recording a synthetic sound imitates, as the app would play it. A pack without
    /// its own space, enter or backspace uses the home row, like `audio::load`.
    fn recording(from: &str, slot: usize) -> Vec<f32> {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("sounds").join(from);
        let read = |i: usize| decode(&dir.join(format!("{}.wav", FILES[i])), RATE as f64);
        let x = Some(read(slot)).filter(|x| !x.is_empty()).unwrap_or_else(|| read(if slot < 8 { 2 } else { 8 }));
        let mut out = Vec::new();
        shape(&x, RATE, false, &mut out);
        out
    }

    #[test]
    #[ignore = "regenerates src/synth.bin from the recordings: cargo test -- --ignored measure"]
    fn measure() {
        let mut bin = Vec::new();
        for (pack, (_, from)) in PACKS.iter().enumerate() {
            for slot in 0..SOUNDS {
                let want = analyse(&recording(from, slot));
                // The bands overlap and noise is uneven, so levels played back as measured
                // come out smeared. Nudge the table until the very waveform it produces
                // measures like the recording.
                let mut table = want;
                for _ in 0..24 {
                    let got = analyse(&noise_bands(&table, pack * SOUNDS + slot));
                    for (c, q) in table.iter_mut().enumerate().filter(|(c, _)| want[*c] > 0) {
                        *q = (*q as i32 + (want[c] as i32 - got[c] as i32) / 2).clamp(1, 255) as u8;
                    }
                }
                bin.extend(table);
            }
        }
        std::fs::write(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/synth.bin"), bin).unwrap();
    }

    /// Power in dB per frame (summed over bands) and per band (summed over frames).
    fn profile(t: &[u8]) -> (Vec<f32>, Vec<f32>) {
        let power = |q: u8| if q == 0 { 0.0 } else { 10f32.powf((q as f32 / 255.0 - 1.0) * 6.0) };
        let db = |p: f32| 10.0 * p.max(1e-9).log10();
        let frames = (0..FRAMES).map(|k| db((0..BANDS).map(|b| power(t[b * FRAMES + k])).sum())).collect();
        let bands = (0..BANDS).map(|b| db((0..FRAMES).map(|k| power(t[b * FRAMES + k])).sum())).collect();
        (frames, bands)
    }

    #[test]
    fn rendering_measures_like_the_recording() {
        assert!(render("cream", 0, RATE).is_none() && render(PACKS[0].0, SOUNDS, RATE).is_none());
        let mut worst = 0f32;
        for (dir, from) in PACKS {
            for slot in 0..SOUNDS {
                for sr in [8000.0, 44100.0, RATE] {
                    assert!(render(dir, slot, sr).unwrap().iter().all(|v| v.is_finite()), "{dir} {slot} @{sr}");
                }
                let (want, got) = (analyse(&recording(from, slot)), analyse(&render(dir, slot, RATE).unwrap()));
                // Shape over time and over frequency, wherever the sound is within 15 dB of
                // its loudest point: weaker bands sit on the splatter of the stronger ones
                // and come out up to 10 dB loud. Each analysis is relative to its own maximum.
                for (what, want, got) in [("frame", profile(&want).0, profile(&got).0), ("band", profile(&want).1, profile(&got).1)] {
                    let top = want.iter().fold(f32::MIN, |m, v| m.max(*v));
                    let near: Vec<(usize, f32, f32)> = want.iter().zip(&got).enumerate().filter(|(_, (w, _))| **w > top - 15.0).map(|(i, (w, g))| (i, *w, *g)).collect();
                    let offset = near.iter().map(|(_, w, g)| g - w).sum::<f32>() / near.len() as f32;
                    for (i, w, g) in near {
                        worst = worst.max((g - offset - w).abs());
                        assert!((g - offset - w).abs() < 6.0, "{dir} sound {slot} {what} {i}: recorded {w:.1} dB, rendered {:.1} dB", g - offset);
                    }
                }
            }
        }
        println!("worst deviation from the recordings: {worst:.1} dB");
    }
}
