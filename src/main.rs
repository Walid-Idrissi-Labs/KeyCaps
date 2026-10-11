//! Keycaps: recorded mechanical-keyboard sounds from the menu bar (macOS) or the tray (Linux).
//!
//! Idle cost is the design constraint, so nothing polls: a key press wakes the process, a
//! sound is mixed, and `IDLE_SECS` after the last one the audio output is released again.

mod audio;
mod stats;
mod synth;

#[cfg(target_os = "macos")]
mod objc;

#[cfg(target_os = "macos")]
#[path = "mac.rs"]
mod app;
#[cfg(target_os = "linux")]
#[path = "linux.rs"]
mod app;
#[cfg(not(any(target_os = "macos", target_os = "linux")))]
compile_error!("Keycaps runs on macOS and Linux.");

/// Seconds of silence before the audio output is released. Getting it back delays the next
/// sound by 10-40 ms (measured on macOS).
const IDLE_SECS: f64 = 60.0;

/// The "Spatial Audio" levels: how far towards one speaker a key at the edge of the
/// keyboard is placed, 1 being that speaker alone. Subtle leaves an edge key 5 dB quieter
/// in the far speaker and Natural 13 dB. Wide overshoots, so that the outer two columns of
/// keys on each side are in one speaker only.
const WIDTH: [(&str, f32); 4] = [("Off", 0.0), ("Subtle", 0.5), ("Natural", 0.9), ("Wide", 1.25)];

/// The playback rate of the next stroke. No two sound identical: each is up to 3% off the
/// recorded pitch.
fn pitch(rng: &mut u32) -> f32 {
    *rng ^= *rng << 13;
    *rng ^= *rng >> 17;
    *rng ^= *rng << 5;
    0.97 + (*rng % 61) as f32 / 1000.0
}

/// `keycaps --demo`: plays every pack once, which needs no permission. `pause` waits that
/// many milliseconds while the sound goes out.
fn demo(root: &std::path::Path, pause: impl Fn(u64)) -> ! {
    audio::init();
    audio::set_volume(0.8, 0.8);
    let mut ok = true;
    for (i, (name, dir)) in audio::PACKS.iter().enumerate() {
        let (mouse, mouse_dir) = audio::MICE[i % audio::MICE.len()];
        println!("{name}, {mouse}");
        ok &= audio::load(root, dir, mouse_dir);
        // One key per row from the left edge to the right, then space, enter and backspace.
        for k in 0..8 {
            let pan = (k as f32 / 3.5 - 1.0) * WIDTH[2].1;
            audio::play(audio::PRESS + k, pan, 1.0);
            pause(60);
            audio::play(audio::RELEASE + k.saturating_sub(4), pan, 1.0);
            pause(70);
        }
        // Left, right and wheel button, then two scroll ticks.
        for slot in (audio::MOUSE..audio::SCROLL).chain([audio::SCROLL, audio::SCROLL]) {
            audio::play(slot, 0.0, 1.0);
            pause(90);
        }
        pause(250);
    }
    ok &= audio::running() && audio::drained();
    audio::stop();
    println!("{}", if ok { "ok: every pack loaded and reached the speakers" } else { "FAILED: recordings missing or the output did not render" });
    std::process::exit(!ok as i32)
}

fn main() {
    app::main()
}
