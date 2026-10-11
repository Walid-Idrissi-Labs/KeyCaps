//! Output on Linux: the PulseAudio native protocol, spoken directly over the sound server's
//! socket. PulseAudio answers it and so does PipeWire. The server asks for audio when it
//! wants some and plays silence by itself otherwise, so Keycaps writes only while a sound
//! lasts, on the main thread, and nothing runs in between.

use super::Shared;
use std::cell::UnsafeCell;
use std::io::{Error, ErrorKind, Read, Result, Write};
use std::os::fd::{AsRawFd, RawFd};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::time::{Duration, Instant};

const RATE: u32 = 48000;
/// Bytes per frame: two channels of 32-bit float.
const FRAME: usize = 8;
/// How much audio the server keeps queued, which is what delays a key press: 20 ms.
const TARGET: usize = RATE as usize / 50 * FRAME;

// From pulsecore/native-common.h.
const ERROR: u32 = 0;
const REPLY: u32 = 2;
const CREATE_PLAYBACK_STREAM: u32 = 3;
const AUTH: u32 = 8;
const SET_CLIENT_NAME: u32 = 9;
const REQUEST: u32 = 61;
const PLAYBACK_STREAM_KILLED: u32 = 64;
/// The protocol version spoken, which fixes the fields of every message below. 16 is
/// from 2009; it is the first where a stream can leave its volume to the server.
const VERSION: u32 = 16;
/// The channel of a packet that is a message and not audio.
const CONTROL: u32 = u32::MAX;

struct Stream {
    sock: UnixStream,
    channel: Option<u32>, // none until the server has opened the stream
    wanted: usize,        // bytes the server has asked for and not yet received
    input: Vec<u8>,
    asked: Instant, // when the stream was asked for
}

/// Main thread only.
struct Output {
    stream: Option<Stream>,
    retry: Option<Instant>, // after a failed connection: when to try the next one
}

static OUT: Shared<Output> = Shared(UnsafeCell::new(Output { stream: None, retry: None }));

unsafe extern "C" {
    fn getuid() -> u32;
}

/// A message being written: tagged values, after its command and sequence number.
struct Tags(Vec<u8>);

impl Tags {
    fn new(command: u32, sequence: u32) -> Tags {
        let mut t = Tags(Vec::new());
        t.u32(command).u32(sequence);
        t
    }
    fn u32(&mut self, v: u32) -> &mut Tags {
        self.0.push(b'L');
        self.0.extend(v.to_be_bytes());
        self
    }
    fn bool(&mut self, v: bool) -> &mut Tags {
        self.0.push(if v { b'1' } else { b'0' });
        self
    }
    fn bytes(&mut self, v: &[u8]) -> &mut Tags {
        self.0.push(b'x');
        self.0.extend((v.len() as u32).to_be_bytes());
        self.0.extend(v);
        self
    }
    /// A property list: each value is a string that carries its closing zero.
    fn properties(&mut self, list: &[(&str, &str)]) -> &mut Tags {
        self.0.push(b'P');
        for (key, value) in list {
            self.0.push(b't');
            self.0.extend(key.as_bytes());
            self.0.push(0);
            self.u32(value.len() as u32 + 1).bytes(&[value.as_bytes(), &[0]].concat());
        }
        self.0.push(b'N');
        self
    }
}

/// Sends one packet: a header of five big-endian words (length, channel, and three that
/// stay zero here), then the content.
fn send(sock: &mut UnixStream, channel: u32, content: &[u8]) -> Result<()> {
    let mut packet = Vec::with_capacity(20 + content.len());
    packet.extend((content.len() as u32).to_be_bytes());
    packet.extend(channel.to_be_bytes());
    packet.extend([0; 12]);
    packet.extend(content);
    sock.write_all(&packet)
}

/// The next tagged 32-bit value of a message.
fn take(message: &mut &[u8]) -> Option<u32> {
    let (value, rest) = message.strip_prefix(b"L")?.split_first_chunk::<4>()?;
    *message = rest;
    Some(u32::from_be_bytes(*value))
}

/// Waits for the answer to message `sequence` and returns its values.
fn answer(sock: &mut UnixStream, sequence: u32) -> Result<Vec<u8>> {
    loop {
        let mut header = [0u8; 20];
        sock.read_exact(&mut header)?;
        let mut content = vec![0u8; u32::from_be_bytes(header[..4].try_into().unwrap()) as usize];
        sock.read_exact(&mut content)?;
        let mut message = &content[..];
        match (take(&mut message), take(&mut message)) {
            (Some(REPLY), Some(s)) if s == sequence => return Ok(message.to_vec()),
            (Some(ERROR), Some(s)) if s == sequence => return Err(Error::other(format!("the sound server refused (error {})", take(&mut message).unwrap_or(0)))),
            _ => {} // something the server says unasked
        }
    }
}

/// The server's socket: where $PULSE_SERVER says, else the usual place for this user.
fn server() -> PathBuf {
    let named = std::env::var("PULSE_SERVER").unwrap_or_default();
    let named = named.strip_prefix("unix:").unwrap_or(&named);
    if named.starts_with('/') {
        return named.into();
    }
    let runtime = std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| format!("/run/user/{}", unsafe { getuid() }));
    PathBuf::from(runtime).join("pulse/native")
}

/// The 256 bytes that prove to PulseAudio that this is the same user. A server that
/// recognises the user by the socket does not look at them.
fn cookie() -> Vec<u8> {
    let home = PathBuf::from(std::env::var_os("HOME").unwrap_or_default());
    let config = std::env::var_os("XDG_CONFIG_HOME").map_or(home.join(".config"), PathBuf::from);
    let places = [std::env::var_os("PULSE_COOKIE").map(PathBuf::from), Some(config.join("pulse/cookie")), Some(home.join(".pulse-cookie"))];
    places.into_iter().flatten().find_map(|p| std::fs::read(p).ok().filter(|c| c.len() == 256)).unwrap_or(vec![0; 256])
}

/// Connects and asks for a stereo float stream that starts playing whatever it is given.
/// The server opens it once its device is ready, which `feed` waits for without blocking:
/// a device that was asleep can take a second.
fn connect() -> Result<Stream> {
    let mut sock = UnixStream::connect(server())?;
    // A server that does not answer must not hang the app.
    sock.set_read_timeout(Some(Duration::from_millis(500)))?;
    sock.set_write_timeout(Some(Duration::from_millis(500)))?;

    send(&mut sock, CONTROL, &Tags::new(AUTH, 0).u32(VERSION).bytes(&cookie()).0)?;
    let theirs = take(&mut &answer(&mut sock, 0)?[..]).unwrap_or(0) & 0xFFFF;
    if theirs < VERSION {
        return Err(Error::other(format!("the sound server speaks protocol {theirs}, older than {VERSION}")));
    }
    send(&mut sock, CONTROL, &Tags::new(SET_CLIENT_NAME, 1).properties(&[("application.name", "Keycaps"), ("application.icon_name", "input-keyboard")]).0)?;
    answer(&mut sock, 1)?;

    let mut t = Tags::new(CREATE_PLAYBACK_STREAM, 2);
    t.0.extend([b'a', 5, 2]); // sample format: 32-bit float little-endian, two channels
    t.0.extend(RATE.to_be_bytes());
    t.0.extend([b'm', 2, 1, 2]); // channel map: front left, front right
    t.u32(u32::MAX); // sink: the default one
    t.0.push(b'N'); // and no sink name
    t.u32(u32::MAX); // buffer limit: the server's
    t.bool(false); // not paused
    t.u32(TARGET as u32); // how much to keep queued
    // How much to wait for before playing: one frame. With none at all, the server keeps
    // the stream's clock running through a pause and throws away what arrives after it.
    t.u32(FRAME as u32);
    t.u32(u32::MAX); // how little to ask for at a time: the server's choice
    t.u32(0); // no synchronised group
    t.0.extend([b'v', 2, 0, 1, 0, 0, 0, 1, 0, 0]); // a volume of 100% for both channels, which the flag below leaves unused
    for _ in 0..7 {
        t.bool(false); // no_remap, no_remix, fix_format, fix_rate, fix_channels, no_move, variable_rate
    }
    t.bool(false); // not muted
    t.bool(true); // the queue above is the whole delay: the server sets up the device to match
    t.properties(&[("media.name", "Keys")]);
    t.bool(false); // no volume given: the server restores the one the user gave Keycaps in the mixer
    t.bool(false); // no early requests
    for _ in 0..3 {
        t.bool(false); // muted_set, dont_inhibit_auto_suspend, fail_on_suspend
    }
    send(&mut sock, CONTROL, &t.0)?;
    sock.set_nonblocking(true)?;
    Ok(Stream { sock, channel: None, wanted: 0, input: Vec::new(), asked: Instant::now() })
}

/// Notes what the server has asked for since the last look, then mixes that much, for as
/// long as something sounds.
fn feed(s: &mut Stream) -> Result<()> {
    let mut chunk = [0u8; 4096];
    loop {
        match s.sock.read(&mut chunk) {
            Ok(0) => return Err(ErrorKind::UnexpectedEof.into()),
            Ok(n) => {
                s.input.extend_from_slice(&chunk[..n]);
                if n < chunk.len() {
                    break; // that was all: no need to ask again only to be told so
                }
            }
            Err(e) if e.kind() == ErrorKind::WouldBlock => break,
            Err(e) => return Err(e),
        }
    }
    while s.input.len() >= 20 {
        let length = 20 + u32::from_be_bytes(s.input[..4].try_into().unwrap()) as usize;
        if s.input.len() < length {
            break;
        }
        let mut message = &s.input[20..length];
        if s.input[4..8] == CONTROL.to_be_bytes() {
            match (take(&mut message), take(&mut message), take(&mut message)) {
                // The stream is open: its channel, then (skipped) its number, then how much it wants.
                (Some(REPLY), Some(2), channel) => {
                    (s.channel, s.wanted) = (channel, take(&mut message).and_then(|_| take(&mut message)).unwrap_or(0) as usize);
                    if s.asked.elapsed() > Duration::from_millis(250) {
                        super::discard(); // what was pressed while waiting would all come out at once
                    }
                }
                (Some(ERROR), Some(2), code) => return Err(Error::other(format!("the sound server refused the stream (error {})", code.unwrap_or(0)))),
                (Some(REQUEST), _, _) => s.wanted += take(&mut message).unwrap_or(0) as usize,
                (Some(PLAYBACK_STREAM_KILLED), _, _) => return Err(ErrorKind::ConnectionAborted.into()),
                _ => {}
            }
        }
        s.input.drain(..length);
    }
    let Some(channel) = s.channel else {
        return if s.asked.elapsed() > Duration::from_secs(5) { Err(ErrorKind::TimedOut.into()) } else { Ok(()) };
    };

    const FRAMES: usize = 480; // 10 ms at a time
    let (mut l, mut r, mut packet) = ([0f32; FRAMES], [0f32; FRAMES], [0u8; 20 + FRAMES * FRAME]);
    while s.wanted >= FRAME {
        let n = (s.wanted / FRAME).min(FRAMES);
        if super::mix(&mut l[..n], &mut r[..n]) {
            break; // nothing sounds: the server plays silence by itself
        }
        packet[..4].copy_from_slice(&((n * FRAME) as u32).to_be_bytes());
        packet[4..8].copy_from_slice(&channel.to_be_bytes());
        for (i, frame) in packet[20..20 + n * FRAME].chunks_exact_mut(FRAME).enumerate() {
            frame[..4].copy_from_slice(&l[i].to_le_bytes());
            frame[4..].copy_from_slice(&r[i].to_le_bytes());
        }
        s.sock.write_all(&packet[..20 + n * FRAME])?;
        s.wanted -= n * FRAME;
    }
    Ok(())
}

/// Keeps the sound server fed: connects when a sound is waiting, and gives the server
/// what it asks for while sounds last. To be called after every event.
pub fn pump() {
    let o = unsafe { &mut *OUT.0.get() };
    if o.stream.is_none() {
        if super::drained() || o.retry.is_some_and(|t| Instant::now() < t) {
            return super::discard();
        }
        o.stream = connect().map_err(|e| unavailable(o, e)).ok();
    }
    if let Some(Err(e)) = o.stream.as_mut().map(feed) {
        // A server that went away while playing is simply connected to again by the next
        // sound. One that never opened the stream is left alone for a while.
        if o.stream.take().is_some_and(|s| s.channel.is_none()) {
            unavailable(o, e);
        } else {
            o.retry = None;
        }
    } else if o.stream.as_ref().is_some_and(|s| s.channel.is_some()) {
        o.retry = None;
    }
}

/// Notes that there is no sound to be had: says so once, drops what is queued and waits
/// five seconds before the next try.
fn unavailable(o: &mut Output, e: Error) {
    if o.retry.is_none() {
        eprintln!("keycaps: no sound: {e} ({})", server().display());
    }
    o.retry = Some(Instant::now() + Duration::from_secs(5));
    super::discard();
}

/// The socket to wait on for the server asking for more.
pub fn fd() -> Option<RawFd> {
    unsafe { (*OUT.0.get()).stream.as_ref().map(|s| s.sock.as_raw_fd()) }
}

/// Nothing to open: the first sound connects.
pub fn init() {}

/// The rate the bank has to be in. The server converts if its device runs at another.
pub(super) fn rate() -> f32 {
    RATE as f32
}

/// Nothing to do here: `pump` is what gets a queued sound out.
pub(super) fn start() {}

/// Leaves the sound server, which ends the stream there.
pub fn stop() {
    unsafe { (*OUT.0.get()).stream = None }
}

pub fn running() -> bool {
    unsafe { (*OUT.0.get()).stream.is_some() }
}
