//! Output: one AudioUnit on the built-in speakers, mixing recorded or synthetic switch sounds. It is
//! started by the first sound and stopped by the caller once idle, so silence costs nothing.

use std::cell::UnsafeCell;
use std::ffi::{c_char, c_void};
use std::path::Path;
use std::ptr::{null, null_mut};
use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering::*};

/// Switch packs: (menu title, directory under `sounds/`). Recordings from kbsim, see
/// sounds/LICENSE.md; the last three have no directory and are rendered by `synth`.
pub const PACKS: [(&str, &str); 16] = [
    ("Alpaca", "alpaca"),
    ("Gateron Black Ink", "blackink"),
    ("Gateron Red Ink", "redink"),
    ("NovelKeys Cream", "cream"),
    ("Cherry MX Black", "mxblack"),
    ("Turquoise Tealios", "turquoise"),
    ("Cherry MX Brown", "mxbrown"),
    ("Holy Panda", "holypanda"),
    ("Topre", "topre"),
    ("Alps SKCM Blue", "bluealps"),
    ("Cherry MX Blue", "mxblue"),
    ("Kailh Box Navy", "boxnavy"),
    ("Buckling Spring", "buckling"),
    ("Synthetic Linear", "synth-linear"),
    ("Synthetic Tactile", "synth-tactile"),
    ("Synthetic Clicky", "synth-clicky"),
];

/// Menu sections of `PACKS`, in order: (title, number of packs).
pub const GROUPS: [(&str, usize); 4] = [("Linear", 6), ("Tactile", 3), ("Clicky", 4), ("Synthetic", 3)];

/// Mouse packs: (menu title, directory under `sounds/mouse/`). Recordings from
/// keyboardsounds-pro, see sounds/mouse/LICENSE.
pub const MICE: [(&str, &str); 7] = [
    ("Apple Magic Mouse", "apple-magic-mouse"),
    ("Logitech G502 X", "g502x-wireless"),
    ("Logitech G502 Lightspeed", "logitech-g502-lightspeed"),
    ("Logitech M570 Trackball", "logitech-m570-trackball"),
    ("Razer Mamba Elite", "razer-mamba-elite"),
    ("Corsair M65 Elite", "corsair-m65-elite"),
    ("Amazon Gaming Mouse", "amazon-gaming-mouse"),
];

// Bank slots.
pub const PRESS: u32 = 0; // + 0..=4 for the row, 5 space, 6 enter, 7 backspace
pub const RELEASE: u32 = 8; // + 0 generic, 1 space, 2 enter, 3 backspace
pub const MOUSE: u32 = 12; // + 0 left, 2 right, 4 wheel button; + 1 for its release
pub const SCROLL: u32 = 18;
const SLOTS: usize = 19;

/// The system volume, about half-way on the slider, at which the menu volumes play as
/// set. Above it Keycaps turns itself down and below it up, so that it stays as loud.
const REFERENCE_DB: f32 = -18.0;
/// Nothing is played with more gain than this, so that a few overlapping keys do not
/// clip. It is what ends the range over which a low system volume can be undone.
const MAX_GAIN: f32 = 0.8;

/// The recording behind each slot: keys relative to the switch pack directory, mouse
/// buttons relative to the mouse pack directory, where "scroll" is the wheel button.
// shortcut: there are no open recordings of a wheel turning, so the scroll tick (last) is
// the shortest click there is, the G502 X left button; replace it if a source turns up.
pub(crate) const FILES: [&str; SLOTS] = [
    "press/GENERIC_R0",
    "press/GENERIC_R1",
    "press/GENERIC_R2",
    "press/GENERIC_R3",
    "press/GENERIC_R4",
    "press/SPACE",
    "press/ENTER",
    "press/BACKSPACE",
    "release/GENERIC",
    "release/SPACE",
    "release/ENTER",
    "release/BACKSPACE",
    "left-down",
    "left-up",
    "right-down",
    "right-up",
    "scroll-down",
    "scroll-up",
    "left-down",
];

#[derive(Clone, Copy)]
struct Voice {
    at: usize,
    end: usize,
    frac: f32, // position between `at` and `at + 1`
    step: f32, // playback rate
    l: f32,
    r: f32,
}
const IDLE: Voice = Voice { at: 0, end: 0, frac: 0.0, step: 1.0, l: 0.0, r: 0.0 };

/// Touched by the render thread while the unit runs, by the main thread only while it is stopped.
struct Bank {
    pcm: Vec<f32>,
    slot: [(usize, usize); SLOTS],
    voices: [Voice; 16],
}

/// Main thread only.
struct Unit {
    unit: *mut c_void,
    running: bool,
    rate: f32,
}

/// A global whose access rules are documented at its use instead of enforced by a lock.
pub struct Shared<T>(pub UnsafeCell<T>);
unsafe impl<T> Sync for Shared<T> {}

static BANK: Shared<Bank> = Shared(UnsafeCell::new(Bank { pcm: Vec::new(), slot: [(0, 0); SLOTS], voices: [IDLE; 16] }));
static UNIT: Shared<Unit> = Shared(UnsafeCell::new(Unit { unit: null_mut(), running: false, rate: 48000.0 }));

// Triggers, main thread -> render thread: slot | pan << 8 | rate << 16.
static RING: [AtomicU32; 64] = [const { AtomicU32::new(0) }; 64];
static HEAD: AtomicUsize = AtomicUsize::new(0);
static TAIL: AtomicUsize = AtomicUsize::new(0);
static VOLUME: [AtomicU32; 2] = [const { AtomicU32::new(0) }; 2]; // keyboard, mouse
static COMPENSATION: AtomicU32 = AtomicU32::new(1f32.to_bits()); // undoes the system volume

#[repr(C)]
struct Buffer {
    channels: u32,
    size: u32,
    data: *mut c_void,
}
#[repr(C)]
struct BufferList {
    n: u32,
    bufs: [Buffer; 2],
}
#[repr(C)]
#[derive(Default)]
struct Format {
    rate: f64,
    id: u32,
    flags: u32,
    bytes_per_packet: u32,
    frames_per_packet: u32,
    bytes_per_frame: u32,
    channels: u32,
    bits: u32,
    reserved: u32,
}
type Render = extern "C" fn(*mut c_void, *mut u32, *const c_void, u32, u32, *mut BufferList) -> i32;
#[repr(C)]
struct Callback {
    f: Render,
    ctx: *mut c_void,
}

#[link(name = "AudioToolbox", kind = "framework")]
#[link(name = "CoreAudio", kind = "framework")]
#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    fn AudioObjectGetPropertyData(obj: u32, addr: *const [u32; 3], qualifier_size: u32, qualifier: *const c_void, size: *mut u32, data: *mut c_void) -> i32;
    fn AudioObjectAddPropertyListener(obj: u32, addr: *const [u32; 3], listener: extern "C" fn(u32, u32, *const c_void, *mut c_void) -> i32, ctx: *mut c_void) -> i32;
    fn CFStringCreateWithCString(alloc: *const c_void, s: *const c_char, encoding: u32) -> *mut c_void;
    fn CFURLCreateFromFileSystemRepresentation(alloc: *const c_void, path: *const u8, len: isize, is_dir: bool) -> *mut c_void;
    fn CFRelease(obj: *mut c_void);
    fn ExtAudioFileOpenURL(url: *mut c_void, out: *mut *mut c_void) -> i32;
    fn ExtAudioFileSetProperty(file: *mut c_void, id: u32, size: u32, data: *const c_void) -> i32;
    fn ExtAudioFileRead(file: *mut c_void, frames: *mut u32, data: *mut BufferList) -> i32;
    fn ExtAudioFileDispose(file: *mut c_void) -> i32;
    fn AudioComponentFindNext(after: *mut c_void, desc: *const u32) -> *mut c_void;
    fn AudioComponentInstanceNew(comp: *mut c_void, out: *mut *mut c_void) -> i32;
    fn AudioUnitGetProperty(u: *mut c_void, id: u32, scope: u32, el: u32, data: *mut c_void, size: *mut u32) -> i32;
    fn AudioUnitSetProperty(u: *mut c_void, id: u32, scope: u32, el: u32, data: *const c_void, size: u32) -> i32;
    fn AudioUnitInitialize(u: *mut c_void) -> i32;
    fn AudioOutputUnitStart(u: *mut c_void) -> i32;
    fn AudioOutputUnitStop(u: *mut c_void) -> i32;
}

const STREAM_FORMAT: u32 = 8;
const RENDER_CALLBACK: u32 = 23;
const CURRENT_DEVICE: u32 = 2000;
const SCOPE_GLOBAL: u32 = 0;
const SCOPE_INPUT: u32 = 1;
const SCOPE_OUTPUT: u32 = 2;
const OUTPUT_IS_SILENCE: u32 = 1 << 4;

const fn fourcc(s: &[u8; 4]) -> u32 {
    u32::from_be_bytes(*s)
}

/// 32-bit float PCM, packed; `planar` gives each channel its own buffer.
fn pcm_format(rate: f64, channels: u32, planar: bool) -> Format {
    Format { rate, id: fourcc(b"lpcm"), flags: 1 | 8 | if planar { 32 } else { 0 }, bytes_per_packet: 4, frames_per_packet: 1, bytes_per_frame: 4, channels, bits: 32, reserved: 0 }
}

extern "C" fn render(_: *mut c_void, flags: *mut u32, _: *const c_void, _: u32, frames: u32, io: *mut BufferList) -> i32 {
    let (b, io, n) = unsafe { (&mut *BANK.0.get(), &mut *io, frames as usize) };
    if io.n != 2 {
        return 0;
    }
    let (l, r) = unsafe {
        (std::slice::from_raw_parts_mut(io.bufs[0].data as *mut f32, n), std::slice::from_raw_parts_mut(io.bufs[1].data as *mut f32, n))
    };
    l.fill(0.0);
    r.fill(0.0);

    let undo = f32::from_bits(COMPENSATION.load(Relaxed));
    let vol = VOLUME.each_ref().map(|v| (f32::from_bits(v.load(Relaxed)) * undo).min(MAX_GAIN));
    let (mut t, h) = (TAIL.load(Relaxed), HEAD.load(Acquire));
    while t != h {
        let m = RING[t % RING.len()].load(Relaxed);
        t = t.wrapping_add(1);
        let slot = (m & 0xFF) as usize % SLOTS;
        let (at, end) = b.slot[slot];
        if at + 1 >= end {
            continue; // recording missing
        }
        let vol = vol[(slot >= MOUSE as usize) as usize];
        let pan = (m >> 8 & 0xFF) as f32 / 255.0;
        // A free voice if there is one, else the one closest to finishing.
        let v = b.voices.iter_mut().min_by_key(|v| v.end.saturating_sub(v.at)).unwrap();
        // Equal power: as loud at any position, and silent in the far speaker at an edge.
        *v = Voice { at, end, frac: 0.0, step: (m >> 16) as f32 / 16384.0, l: vol * (1.0 - pan).sqrt(), r: vol * pan.sqrt() };
    }
    TAIL.store(t, Release);

    let mut silent = true;
    for v in &mut b.voices {
        // Linear interpolation between neighbours, so a voice needs two samples left.
        for i in 0..n {
            if v.at + 1 >= v.end {
                break;
            }
            silent = false;
            let x = b.pcm[v.at] + (b.pcm[v.at + 1] - b.pcm[v.at]) * v.frac;
            l[i] += x * v.l;
            r[i] += x * v.r;
            v.frac += v.step;
            let whole = v.frac as usize;
            v.at += whole;
            v.frac -= whole as f32;
        }
    }
    if silent {
        unsafe { *flags |= OUTPUT_IS_SILENCE };
    }
    0
}

/// The gain that undoes a system volume of `db` decibels.
fn compensation(db: f32) -> f32 {
    10f32.powf((REFERENCE_DB - db) / 20.0)
}

/// Called by the system when the output device's volume changes, and once at start:
/// stores the gain that undoes it. A device without a main volume is left alone.
extern "C" fn volume_changed(device: u32, _: u32, _: *const c_void, _: *mut c_void) -> i32 {
    let (mut db, mut size) = (0f32, 4u32);
    let main_volume_db = [fourcc(b"vold"), fourcc(b"outp"), 0];
    let known = unsafe { AudioObjectGetPropertyData(device, &main_volume_db, 0, null(), &mut size, &raw mut db as _) } == 0;
    COMPENSATION.store(if known { compensation(db) } else { 1.0 }.to_bits(), Relaxed);
    0
}

/// The built-in speakers' device id.
// shortcut: found by their Apple-silicon UID, so an Intel Mac falls back to whatever
// output was the default at launch; enumerate the devices to support those.
fn speakers() -> Option<u32> {
    unsafe {
        let uid = CFStringCreateWithCString(null(), c"BuiltInSpeakerDevice".as_ptr(), 0x0800_0100); // UTF-8
        let (mut dev, mut size) = (0u32, 4u32);
        // On the system object, 'uidd' translates a UID into a device id (0 = no such device).
        let err = AudioObjectGetPropertyData(1, &[fourcc(b"uidd"), fourcc(b"glob"), 0], 8, &raw const uid as _, &mut size, &raw mut dev as _);
        CFRelease(uid);
        (err == 0 && dev != 0).then_some(dev)
    }
}

/// Opens the built-in speakers, whatever the system output is. On failure the app stays silent.
pub fn init() {
    unsafe {
        let desc = [fourcc(b"auou"), fourcc(b"ahal"), fourcc(b"appl"), 0, 0];
        let comp = AudioComponentFindNext(null_mut(), desc.as_ptr());
        let mut unit = null_mut();
        if comp.is_null() || AudioComponentInstanceNew(comp, &mut unit) != 0 {
            return eprintln!("keycaps: no audio output");
        }
        match speakers() {
            Some(dev) if AudioUnitSetProperty(unit, CURRENT_DEVICE, SCOPE_GLOBAL, 0, &raw const dev as _, 4) == 0 => {}
            _ => eprintln!("keycaps: built-in speakers not available, using the default output"),
        }
        // Mix at the device rate so the unit does not have to resample.
        let mut dev = Format::default();
        let mut size = size_of::<Format>() as u32;
        AudioUnitGetProperty(unit, STREAM_FORMAT, SCOPE_OUTPUT, 0, &raw mut dev as _, &mut size);
        let rate = if dev.rate >= 8000.0 { dev.rate } else { 48000.0 };
        let fmt = pcm_format(rate, 2, true);
        let cb = Callback { f: render, ctx: null_mut() };
        if AudioUnitSetProperty(unit, STREAM_FORMAT, SCOPE_INPUT, 0, &raw const fmt as _, size_of::<Format>() as u32) != 0
            || AudioUnitSetProperty(unit, RENDER_CALLBACK, SCOPE_INPUT, 0, &raw const cb as _, size_of::<Callback>() as u32) != 0
            || AudioUnitInitialize(unit) != 0
        {
            return eprintln!("keycaps: audio output setup failed");
        }
        // Loudness is set by the menu volumes alone: follow the device's volume to undo it.
        let (mut device, mut size) = (0u32, 4u32);
        if AudioUnitGetProperty(unit, CURRENT_DEVICE, SCOPE_GLOBAL, 0, &raw mut device as _, &mut size) == 0 {
            AudioObjectAddPropertyListener(device, &[fourcc(b"volm"), fourcc(b"outp"), 0], volume_changed, null_mut());
            volume_changed(device, 0, null(), null_mut());
        }
        *UNIT.0.get() = Unit { unit, running: false, rate: rate as f32 };
    }
}

/// Decodes any file Core Audio can read to mono at `rate`. Empty if it is missing or unreadable.
pub(crate) fn decode(path: &Path, rate: f64) -> Vec<f32> {
    let mut out = Vec::new();
    let bytes = path.as_os_str().as_encoded_bytes();
    unsafe {
        let url = CFURLCreateFromFileSystemRepresentation(null(), bytes.as_ptr(), bytes.len() as isize, false);
        if url.is_null() {
            return out;
        }
        let mut file = null_mut();
        if ExtAudioFileOpenURL(url, &mut file) == 0 {
            let fmt = pcm_format(rate, 1, false);
            if ExtAudioFileSetProperty(file, fourcc(b"cfmt"), size_of::<Format>() as u32, &raw const fmt as _) == 0 {
                let mut chunk = [0f32; 4096];
                loop {
                    let mut frames = chunk.len() as u32;
                    let mut io = BufferList { n: 1, bufs: [Buffer { channels: 1, size: 4 * frames, data: chunk.as_mut_ptr() as _ }, Buffer { channels: 0, size: 0, data: null_mut() }] };
                    if ExtAudioFileRead(file, &mut frames, &mut io) != 0 || frames == 0 {
                        break;
                    }
                    out.extend_from_slice(&chunk[..frames as usize]);
                }
            }
            ExtAudioFileDispose(file);
        }
        CFRelease(url);
    }
    out
}

/// Appends `x` to `out`, cut to start 2 ms before its hit and to end where it has died
/// away, with a fade-out of up to 5 ms, and returns its peak. The recordings lead in with
/// up to 90 ms of travel, which would be heard as lag, and trail off into room noise or
/// stop abruptly. Appends nothing and returns 0 for silence (some packs ship placeholders).
///
/// "Died away" is the last sample above 2% of the peak. With `gate` it is instead the
/// first 5 ms after the hit that stay under twice the recording's own noise floor, like a
/// noise gate closing: several mouse recordings hiss at -15 to -35 dB, and the 10 ms click
/// would otherwise drag 100 ms of that along. The floor is the level of the typical 5 ms
/// of the file, which is hiss when the file is mostly hiss around a click.
pub(crate) fn shape(x: &[f32], rate: f32, gate: bool, out: &mut Vec<f32>) -> f32 {
    let level = |c: &[f32]| c.iter().fold(0f32, |m, v| m.max(v.abs()));
    let peak = level(x);
    if peak < 1e-4 {
        return 0.0;
    }
    let onset = x.iter().position(|v| v.abs() >= 0.2 * peak).unwrap_or(0);
    let lead = onset.min((0.002 * rate) as usize);
    let end = if gate {
        let chunk = (0.005 * rate) as usize;
        let mut levels: Vec<f32> = x.chunks_exact(chunk).map(level).collect();
        let middle = levels.len() / 2;
        let floor = if levels.is_empty() { 0.0 } else { *levels.select_nth_unstable_by(middle, f32::total_cmp).1 };
        let quiet = (2.0 * floor).clamp(0.02 * peak, 0.25 * peak);
        let hit = x.iter().position(|v| v.abs() == peak).unwrap_or(onset);
        (hit + x[hit..].chunks(chunk).take_while(|c| level(c) >= quiet).count() * chunk).min(x.len())
    } else {
        x.iter().rposition(|v| v.abs() >= 0.02 * peak).map_or(x.len(), |i| i + 1)
    };
    let x = &x[onset - lead..end];
    let tail = (0.005 * rate).min(x.len() as f32 / 4.0);
    out.extend(x.iter().enumerate().map(|(n, v)| v * ((n + 1) as f32 / lead as f32).min(1.0) * ((x.len() - n) as f32 / tail).min(1.0)));
    peak
}

/// Fills the bank with switch pack `dir` (decoded from `root/dir`, or rendered if it is
/// a synthetic one) and mouse pack `mouse` (from `root/mouse`). False if the switch
/// recordings are missing. Stops output first.
pub fn load(root: &Path, dir: &str, mouse: &str) -> bool {
    stop();
    let (b, rate) = unsafe { (&mut *BANK.0.get(), (*UNIT.0.get()).rate) };
    b.pcm.clear();
    b.voices = [IDLE; 16];
    let mut peaks = [0f32; SLOTS];
    for (i, file) in FILES.iter().enumerate() {
        let x = if i < MOUSE as usize {
            crate::synth::render(dir, i, rate).unwrap_or_else(|| decode(&root.join(dir).join(format!("{file}.mp3")), rate as f64))
        } else {
            let from = root.join("mouse").join(if i < SCROLL as usize { mouse } else { "g502x-wireless" });
            Some(decode(&from.join(format!("{file}.mp3")), rate as f64)).filter(|x| !x.is_empty()).unwrap_or_else(|| decode(&from.join(format!("{file}.wav")), rate as f64))
        };
        let at = b.pcm.len();
        peaks[i] = shape(&x, rate, i >= MOUSE as usize, &mut b.pcm);
        b.slot[i] = (at, b.pcm.len());
    }
    // The loudest letter row peaks at 0.6 and the rest keep their level relative to it,
    // within bounds: the recordings are levelled unevenly (a release 6x louder than its
    // press in one pack, 20x quieter in another).
    // The mouse buttons likewise keep their level relative to the loudest of them.
    let rows = 0.6 / peaks[..5].iter().fold(1e-9f32, |m, p| m.max(*p));
    let clicks = 0.5 / peaks[MOUSE as usize..SCROLL as usize].iter().fold(1e-9f32, |m, p| m.max(*p));
    for i in 0..SLOTS {
        let (lo, hi, reference) = match i as u32 {
            0..=4 => (0.0, 0.6, rows),
            5..=7 => (0.6, 0.9, rows),   // space, enter, backspace: at least as loud as a letter
            8..=11 => (0.15, 0.4, rows), // releases: always quieter than a press
            SCROLL => (0.2, 0.2, rows),
            _ => (0.25, 0.5, clicks),
        };
        let gain = (peaks[i] * reference).clamp(lo, hi) / peaks[i];
        let (at, end) = b.slot[i];
        b.pcm[at..end].iter_mut().for_each(|v| *v *= gain);
    }
    // A switch pack without its own space, enter or backspace uses the home row for them,
    // and a mouse pack without its own right or wheel button uses the left one.
    for i in 0..SCROLL as usize {
        if b.slot[i].0 == b.slot[i].1 {
            b.slot[i] = b.slot[match i as u32 {
                0..=7 => PRESS as usize + 2,
                8..=11 => RELEASE as usize,
                m => (MOUSE + (m - MOUSE) % 2) as usize,
            }];
        }
    }
    b.pcm.shrink_to_fit();
    b.slot[PRESS as usize + 2].0 != b.slot[PRESS as usize + 2].1
}

/// Queues a sound (`pan` -1 left speaker only .. 1 right speaker only, `rate` 0.25..4,
/// 1 = as recorded) and makes sure output is running. Main thread.
pub fn play(slot: u32, pan: f32, rate: f32) {
    let h = HEAD.load(Relaxed);
    if h.wrapping_sub(TAIL.load(Acquire)) < RING.len() {
        RING[h % RING.len()].store(slot | (((pan.clamp(-1.0, 1.0) + 1.0) * 127.5).round() as u32) << 8 | ((rate * 16384.0) as u32).clamp(4096, 0xFFFF) << 16, Relaxed);
        HEAD.store(h.wrapping_add(1), Release);
    }
    let u = unsafe { &mut *UNIT.0.get() };
    if !u.running && !u.unit.is_null() {
        u.running = unsafe { AudioOutputUnitStart(u.unit) } == 0;
    }
}

/// Stops output; returns only once the render thread is out of the callback.
pub fn stop() {
    let u = unsafe { &mut *UNIT.0.get() };
    if u.running {
        unsafe { AudioOutputUnitStop(u.unit) };
        u.running = false;
    }
}

pub fn running() -> bool {
    unsafe { (*UNIT.0.get()).running }
}

/// Keyboard and mouse volume, each 0..=1. The 0.6 leaves room to undo a low system volume.
pub fn set_volume(keys: f32, mouse: f32) {
    VOLUME[0].store((keys * 0.6).to_bits(), Relaxed);
    VOLUME[1].store((mouse * 0.6).to_bits(), Relaxed);
}

/// True once the render thread has picked up every queued sound.
pub fn drained() -> bool {
    HEAD.load(Relaxed) == TAIL.load(Relaxed)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn peak(v: &[f32]) -> f32 {
        v.iter().fold(0f32, |m, x| m.max(x.abs()))
    }

    /// Renders `frames` of stereo output.
    fn mix(frames: usize) -> (Vec<f32>, Vec<f32>, u32) {
        let (mut l, mut r) = (vec![1f32; frames], vec![1f32; frames]);
        let buf = |v: &mut Vec<f32>| Buffer { channels: 1, size: 4 * frames as u32, data: v.as_mut_ptr() as _ };
        let mut io = BufferList { n: 2, bufs: [buf(&mut l), buf(&mut r)] };
        let mut flags = 0;
        render(null_mut(), &mut flags, null(), 0, frames as u32, &mut io);
        (l, r, flags)
    }

    /// How much louder left is than right, in dB.
    fn balance(l: &[f32], r: &[f32]) -> f32 {
        let power = |v: &[f32]| v.iter().map(|x| x * x).sum::<f32>();
        10.0 * (power(l) / power(r)).log10()
    }

    #[test]
    fn compensation_undoes_the_system_volume() {
        assert!((compensation(REFERENCE_DB) - 1.0).abs() < 1e-6);
        assert!((compensation(0.0) - 0.1259).abs() < 1e-3); // Mac at full volume: 18 dB down
        assert!((compensation(-24.0) - 1.995).abs() < 1e-2); // Mac 6 dB below the reference: 6 dB up
    }

    #[test]
    fn shaping_cuts_the_lead_in_and_a_hissing_tail() {
        let rate = 48000.0;
        let mut seed = 7u32;
        let mut hiss = |n: usize| -> Vec<f32> {
            (0..n).map(|_| {
                seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
                (seed >> 8) as f32 / 8388608.0 * 0.03 - 0.03 // within 3% of full scale, about -30 dB
            }).collect()
        };
        // 30 ms of hiss, a 5 ms click dying away from full scale, 60 ms of hiss.
        let click = (0..240).map(|i| if i % 2 == 0 { 1.0 } else { -1.0 } * (1.0 - i as f32 / 300.0));
        let x: Vec<f32> = hiss(1440).into_iter().chain(click).chain(hiss(2880)).collect();
        let (mut gated, mut plain, mut none) = (Vec::new(), Vec::new(), Vec::new());
        assert_eq!(shape(&x, rate, true, &mut gated), 1.0);
        shape(&x, rate, false, &mut plain);
        // Gated: the 2 ms before the hit plus the click. Not gated: the hiss stays, as it is above 2%.
        assert!((240..400).contains(&gated.len()), "gated to {} samples", gated.len());
        assert!(plain.len() > 2880, "plain kept {} samples", plain.len());
        assert!(gated[0].abs() < 0.05 && gated.last().unwrap().abs() < 0.05);
        assert_eq!(shape(&[0.0; 480], rate, true, &mut none), 0.0);
        assert!(none.is_empty());
    }

    // One test: the bank is a global.
    #[test]
    fn every_pack_loads_trimmed_and_bounded_then_mixes_in_place() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("sounds");
        assert_eq!(GROUPS.iter().map(|g| g.1).sum::<usize>(), PACKS.len());
        assert!(!load(&root, "no-such-pack", MICE[0].1));
        // Every switch pack with the first mouse pack, then every mouse pack.
        let switches = PACKS.iter().map(|p| (p.1, MICE[0].1, 0..MOUSE as usize));
        let mice = MICE.iter().map(|m| ("cream", m.1, MOUSE as usize..SLOTS));
        for (dir, mouse, slots) in switches.chain(mice) {
            assert!(load(&root, dir, mouse), "{dir} did not load");
            let b = unsafe { &*BANK.0.get() };
            if slots.start == MOUSE as usize {
                let ms: Vec<u32> = slots.clone().map(|i| ((b.slot[i].1 - b.slot[i].0) as f32 / 48.0).round() as u32).collect();
                println!("{mouse}: left {}/{} ms, right {}/{} ms, wheel {}/{} ms, tick {} ms", ms[0], ms[1], ms[2], ms[3], ms[4], ms[5], ms[6]);
            }
            for i in slots {
                let x = &b.pcm[b.slot[i].0..b.slot[i].1];
                // At least 10 ms for a key, 2 ms for a mouse button.
                assert!(x.len() > if i < MOUSE as usize { 480 } else { 96 }, "{dir}/{mouse} slot {i} has {} samples", x.len());
                assert!(x.iter().all(|v| v.is_finite()) && peak(x) > 0.14 && peak(x) <= 0.91, "{dir}/{mouse} slot {i} peaks at {}", peak(x));
                let onset = x.iter().position(|v| v.abs() >= 0.2 * peak(x)).unwrap();
                assert!(onset <= 120, "{dir}/{mouse} slot {i} starts {onset} samples late"); // 2.5 ms at 48 kHz
                assert!(x.last().unwrap().abs() < 0.02, "{dir}/{mouse} slot {i} ends abruptly");
            }
        }

        // A key in the middle is the same on both sides.
        set_volume(1.0, 1.0);
        play(PRESS + 2, 0.0, 1.0);
        let (l, r, flags) = mix(4096);
        assert!(drained() && peak(&l) > 0.1 && flags & OUTPUT_IS_SILENCE == 0);
        assert!(balance(&l, &r).abs() < 0.1, "centre: {} dB", balance(&l, &r));
        // It runs out, and silence is flagged.
        let (l, _, flags) = (0..20).map(|_| mix(4096)).last().unwrap();
        assert!(peak(&l) == 0.0 && flags & OUTPUT_IS_SILENCE != 0);

        // Four fifths of the way to the left: 9.5 dB louder there (1.8 against 0.2 in power).
        play(PRESS + 2, -0.8, 1.0);
        let (l, r, _) = mix(4096);
        assert!((balance(&l, &r) - 9.5).abs() < 0.3, "four fifths left: {} dB", balance(&l, &r));
        // All the way, and beyond: nothing at all from the right speaker.
        for pan in [-1.0, -1.2] {
            (0..20).for_each(|_| drop(mix(4096)));
            play(PRESS + 2, pan, 1.0);
            let (l, r, _) = mix(4096);
            assert!(peak(&l) > 0.1 && peak(&r) == 0.0, "pan {pan}: right peaks at {}", peak(&r));
        }
        (0..20).for_each(|_| drop(mix(4096)));

        // The mouse has its own volume.
        let click = |mouse_volume| {
            (0..20).for_each(|_| drop(mix(4096))); // let the last sound run out
            set_volume(1.0, mouse_volume);
            play(MOUSE, 0.0, 1.0);
            peak(&mix(4096).0)
        };
        let (full, quarter) = (click(1.0), click(0.25));
        assert!(full > 0.05 && (quarter / full - 0.25).abs() < 0.01, "mouse at full {full}, at a quarter {quarter}");

        // The system volume is undone, up to the most gain anything may have.
        let key = |undo: f32| {
            (0..20).for_each(|_| drop(mix(4096)));
            COMPENSATION.store(undo.to_bits(), Relaxed);
            play(PRESS + 2, 0.0, 1.0);
            peak(&mix(4096).0)
        };
        let (as_set, halved, capped) = (key(1.0), key(0.5), key(100.0));
        assert!((halved / as_set - 0.5).abs() < 0.01 && (capped / as_set - MAX_GAIN / 0.6).abs() < 0.01, "{as_set} {halved} {capped}");
        COMPENSATION.store(1f32.to_bits(), Relaxed);

        // More simultaneous sounds than voices, at the extreme rates, must not overrun anything.
        (0..40).for_each(|i| play(i % SLOTS as u32, 0.0, if i % 2 == 0 { 0.01 } else { 3.99 }));
        let (l, _, _) = (0..100).map(|_| mix(512)).last().unwrap();
        assert!(drained() && l.iter().all(|x| x.is_finite()));
    }
}
