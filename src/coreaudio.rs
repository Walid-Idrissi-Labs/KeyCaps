//! Output on macOS: one AudioUnit on the built-in speakers, which pulls from the mixer on
//! Core Audio's render thread. Core Audio also reads the recordings.

use super::{COMPENSATION, Shared, compensation};
use std::cell::UnsafeCell;
use std::ffi::{c_char, c_void};
use std::path::Path;
use std::ptr::{null, null_mut};
use std::sync::atomic::Ordering::Relaxed;

/// Main thread only.
struct Unit {
    unit: *mut c_void,
    running: bool,
    rate: f32,
}

static UNIT: Shared<Unit> = Shared(UnsafeCell::new(Unit { unit: null_mut(), running: false, rate: 48000.0 }));

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
    let (io, n) = unsafe { (&mut *io, frames as usize) };
    if io.n != 2 {
        return 0;
    }
    let (l, r) = unsafe {
        (std::slice::from_raw_parts_mut(io.bufs[0].data as *mut f32, n), std::slice::from_raw_parts_mut(io.bufs[1].data as *mut f32, n))
    };
    if super::mix(l, r) {
        unsafe { *flags |= OUTPUT_IS_SILENCE };
    }
    0
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

/// The rate the bank has to be in: the speakers' own.
pub(super) fn rate() -> f32 {
    unsafe { (*UNIT.0.get()).rate }
}

/// Makes sure the unit is pulling. Main thread.
pub(super) fn start() {
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
