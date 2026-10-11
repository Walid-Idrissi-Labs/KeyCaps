//! The Linux app: one thread asleep in poll() on the keyboards and mice, the sound server,
//! the tray and a socket that takes the `keycaps <command>` calls. Keys are read from the
//! kernel's input devices (/dev/input), which works the same under X11 and Wayland and
//! needs the user to be in the `input` group.

#[path = "tray.rs"]
mod tray;

use crate::audio;
use crate::stats::{self, Stats};
use crate::{IDLE_SECS, WIDTH};
use std::ffi::{c_char, c_int, c_ulong};
use std::fs::File;
use std::io::{ErrorKind, Read, Write};
use std::os::fd::{AsRawFd, FromRawFd, RawFd};
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const USAGE: &str = "\
Keycaps plays the sound of a mechanical keyboard as you type.

  keycaps                      start it (it stays in the tray, where there is one)
  keycaps on | off | toggle    switch the sound on or off
  keycaps switch [NAME]        choose the switch, or list them
  keycaps mouse [NAME | off]   choose the mouse whose clicks to play, or list them
  keycaps volume 0-100         keyboard volume
  keycaps mouse-volume 0-100   mouse volume
  keycaps spatial off | subtle | natural | wide
  keycaps keyup on | off       the sound of a key coming back up
  keycaps scroll on | off      a tick when the wheel turns
  keycaps status               what is set, and whether keys can be heard
  keycaps quit
";

#[repr(C)]
struct PollFd {
    fd: RawFd,
    events: i16,
    revents: i16,
}

unsafe extern "C" {
    fn poll(fds: *mut PollFd, n: c_ulong, timeout: c_int) -> c_int;
    fn ioctl(fd: c_int, request: c_ulong, ...) -> c_int;
    fn inotify_init1(flags: c_int) -> c_int;
    fn inotify_add_watch(fd: c_int, path: *const c_char, mask: u32) -> c_int;
    fn getuid() -> u32;
}

// linux/input.h and linux/input-event-codes.h.
const EV_KEY: u16 = 1;
const EV_REL: u16 = 2;
const EV_ABS: u16 = 3;
const EV_MSC: u16 = 4;
const REL_WHEEL: u16 = 8;
const BTN_LEFT: u16 = 0x110;
const BTN_RIGHT: u16 = 0x111;
const BTN_TASK: u16 = 0x117; // the last of the mouse buttons
const KEY_FN: u16 = 0x1d0;
const EVIOCGBIT: c_ulong = 0x8000_4520; // + the event type, and the buffer length << 16
const EVIOCSMASK: c_ulong = 0x4010_4593;
const O_NONBLOCK: i32 = 0x800;
const O_CLOEXEC: i32 = 0x80000;
const IN_ATTRIB: u32 = 4;
const IN_CREATE: u32 = 0x100;
const POLLIN: i16 = 1;

/// Key codes, left to right, one row per line of a PC keyboard.
const ROWS: [&[u8]; 6] = [
    &[1, 59, 60, 61, 62, 63, 64, 65, 66, 67, 68, 87, 88],      // escape, F1 to F12
    &[41, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14],         // ` 1 to 0 - = backspace
    &[15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 43], // tab, Q to P, [ ] \
    &[58, 30, 31, 32, 33, 34, 35, 36, 37, 38, 39, 40, 28],     // caps lock, A to L, ; ' enter
    &[42, 86, 44, 45, 46, 47, 48, 49, 50, 51, 52, 53, 54],     // shift, the extra ISO key, Z to M, , . / shift
    &[29, 125, 56, 57, 100, 126, 127, 97],                     // control, super, alt, space, alt, super, menu, control
];

/// Keys right of the main block: arrows, the navigation cluster and the number pad.
const RIGHT: [u8; 31] = [
    105, 108, 103, 106, 110, 111, 102, 107, 104, 109, 99, 70, 119, // arrows, insert to page down, print, scroll lock, pause
    69, 98, 55, 74, 71, 72, 73, 78, 75, 76, 77, 79, 80, 81, 96, 82, 83, 117, // number pad
];

/// Per key code: position across the keyboard, -100 (left edge) to 100 (right edge), and
/// which recording it plays (`audio::PRESS` + 0..=4 by row, 5 space, 6 enter, 7 backspace).
const KEYS: [(i8, u8); 256] = {
    let mut t = [(0i8, 2u8); 256];
    let mut r = 0;
    while r < ROWS.len() {
        let mut i = 0;
        while i < ROWS[r].len() {
            // The recordings start at the number row, so the function row shares it.
            let pan = i as i32 * 200 / (ROWS[r].len() as i32 - 1) - 100;
            t[ROWS[r][i] as usize] = (pan as i8, if r == 0 { 0 } else { r as u8 - 1 });
            i += 1;
        }
        r += 1;
    }
    let mut i = 0;
    while i < RIGHT.len() {
        t[RIGHT[i] as usize] = (100, 2);
        i += 1;
    }
    // The keys laptops put on the top row sit where they do on a Mac's.
    let media: [(usize, i8); 12] = [(224, -90), (225, -90), (228, -40), (229, -40), (230, -40), (165, 25), (164, 25), (163, 25), (166, 25), (113, 75), (114, 75), (115, 75)];
    let mut i = 0;
    while i < media.len() {
        t[media[i].0] = (media[i].1, 0); // brightness, keyboard light, previous to stop, mute and volume
        i += 1;
    }
    t[57].1 = 5; // space
    t[28].1 = 6; // enter
    t[96].1 = 6; // number pad enter
    t[14].1 = 7; // backspace
    t
};

/// Keys that stay silent: power, sleep, wake, lock and suspend.
const SILENT: [u16; 5] = [116, 142, 143, 152, 205];

struct Device {
    number: u32, // the N of /dev/input/eventN
    file: File,
}

pub struct State {
    pub on: bool,
    pub keyup: bool,
    pub scroll: bool,
    pub vol: usize,  // keyboard volume, percent
    pub mvol: usize, // mouse volume, percent
    pub key: usize,  // index into audio::PACKS
    pub mouse: usize, // 0 = off, else 1 + index into audio::MICE
    pub spatial: usize, // index into WIDTH
    loaded: (usize, usize), // the switch and mouse packs that are in the audio bank
    root: PathBuf,   // the sounds folder
    pub stats: Stats,
    dirty: bool,  // stats changed since they were saved
    quarter: u64, // quarter-hour of the last date check
    last: Instant, // time of the last sound
    last_scroll: Instant,
    due: Option<Instant>, // while sound is in use: when to save the count and see whether to let go of the sound server
    rng: u32,
    devices: Vec<Device>,
    pub deaf: bool, // there are input devices, but none may be opened
    quit: bool,
}

impl State {
    fn new(root: PathBuf) -> State {
        let now = Instant::now();
        State {
            on: true,
            keyup: true,
            scroll: false,
            vol: 60,
            mvol: 60,
            key: 3,
            mouse: 0,
            spatial: 2,
            loaded: (usize::MAX, usize::MAX),
            root,
            stats: Stats { day: 0, n: 0 },
            dirty: false,
            quarter: 0,
            last: now,
            last_scroll: now,
            due: None,
            rng: 0x2545_F491,
            devices: Vec::new(),
            deaf: false,
            quit: false,
        }
    }
}

/// Plays a bank slot and makes sure the loop will come back to release the sound server.
fn sound(s: &mut State, slot: u32, pan: f32, rate: f32) {
    audio::play(slot, pan, rate);
    s.last = Instant::now();
    s.due.get_or_insert(s.last + Duration::from_secs_f64(IDLE_SECS));
}

/// One event of an input device.
fn event(s: &mut State, kind: u16, code: u16, value: i32) {
    let pitch = crate::pitch(&mut s.rng);
    match (kind, code, value) {
        // Left, right, and the wheel button for every other one. 2 would be a key repeating.
        (EV_KEY, BTN_LEFT..=BTN_TASK, 0 | 1) if s.mouse != 0 => {
            let button = match code {
                BTN_LEFT => 0,
                BTN_RIGHT => 2,
                _ => 4,
            };
            sound(s, audio::MOUSE + button + (value == 0) as u32, 0.0, pitch);
        }
        (EV_KEY, 1..=255 | KEY_FN, 0 | 1) if !SILENT.contains(&code) => {
            // Codes past the table are keys such as Fn, which sit on the top row.
            let (pan, key) = KEYS.get(code as usize).copied().unwrap_or((0, 0));
            if value == 1 {
                // Local midnight always falls on a quarter-hour, so this is the only date check needed.
                let quarter = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs() / 900);
                if quarter != s.quarter {
                    s.quarter = quarter;
                    s.stats.roll(stats::today());
                }
                s.stats.n += 1;
                s.dirty = true;
            } else if !s.keyup {
                return;
            }
            let slot = if value == 1 { audio::PRESS + key as u32 } else { audio::RELEASE + (key as u32).saturating_sub(4) };
            sound(s, slot, pan as f32 / 100.0 * WIDTH[s.spatial].1, pitch);
        }
        (EV_REL, REL_WHEEL, _) if s.scroll && s.last_scroll.elapsed() >= Duration::from_millis(40) => {
            s.last_scroll = Instant::now();
            sound(s, audio::SCROLL, 0.0, pitch);
        }
        _ => {}
    }
}

/// Reads what an input device has to say. False once the device is gone.
fn listen(s: &mut State, device: usize) -> bool {
    loop {
        // 24 bytes per event: 16 of time, then type, code and value.
        let mut events = [0u8; 24 * 32];
        match (&s.devices[device].file).read(&mut events) {
            Ok(n) if n >= 24 => {
                for e in events[..n].chunks_exact(24) {
                    event(s, u16::from_ne_bytes([e[16], e[17]]), u16::from_ne_bytes([e[18], e[19]]), i32::from_ne_bytes([e[20], e[21], e[22], e[23]]));
                }
            }
            Err(e) if e.kind() == ErrorKind::WouldBlock => return true,
            _ => return false, // unplugged
        }
    }
}

/// Whether `code` is set in the bits an input device reports for one event type.
fn has(bits: &[u8], code: u16) -> bool {
    bits[code as usize / 8] >> (code % 8) & 1 == 1
}

/// Opens the input devices that the settings call for and that are not open yet:
/// keyboards, and mice if their clicks or the wheel are to be heard. Nothing while off.
fn open_devices(s: &mut State) {
    let (mut found, mut denied) = (false, false);
    for entry in std::fs::read_dir("/dev/input").into_iter().flatten().flatten() {
        let name = entry.file_name();
        let Some(number) = name.to_str().and_then(|n| n.strip_prefix("event")).and_then(|n| n.parse().ok()) else { continue };
        found = true;
        if !s.on || s.devices.iter().any(|d| d.number == number) {
            continue;
        }
        let file = match File::options().read(true).custom_flags(O_NONBLOCK).open(entry.path()) {
            Ok(file) => file,
            Err(e) => {
                denied |= e.kind() == ErrorKind::PermissionDenied;
                continue;
            }
        };
        let fd = file.as_raw_fd();
        let (mut keys, mut axes) = ([0u8; 64], [0u8; 8]);
        unsafe {
            ioctl(fd, EVIOCGBIT + EV_KEY as c_ulong + ((keys.len() as c_ulong) << 16), keys.as_mut_ptr());
            ioctl(fd, EVIOCGBIT + EV_REL as c_ulong + ((axes.len() as c_ulong) << 16), axes.as_mut_ptr());
        }
        let keyboard = keys[0] & !1 != 0 || keys[1..32].iter().any(|b| *b != 0);
        let wheel = s.scroll && has(&axes, REL_WHEEL);
        if !(keyboard || wheel || s.mouse != 0 && has(&keys, BTN_LEFT)) {
            continue;
        }
        // Ask for nothing but keys, buttons and the wheel. Without this, every movement
        // of a mouse or a finger on a touchpad would wake the app. Kernels before 4.4
        // do not know the request, and then it is only that: woken more often.
        let wanted = [(wheel as u64) << REL_WHEEL, 0, 0];
        for (kind, codes) in [EV_REL, EV_ABS, EV_MSC].into_iter().zip(&wanted) {
            #[repr(C)]
            struct Mask {
                kind: u32,
                size: u32,
                codes: *const u64,
            }
            unsafe { ioctl(fd, EVIOCSMASK, &Mask { kind: kind as u32, size: 8, codes }) };
        }
        s.devices.push(Device { number, file });
    }
    s.deaf = s.on && found && denied && s.devices.is_empty();
}

/// Puts switch pack `key` and mouse pack `mouse` (an index into `audio::MICE`) into the
/// audio bank unless they are already there.
fn load_packs(s: &mut State, key: usize, mouse: usize) {
    if s.loaded != (key, mouse) {
        s.loaded = (key, mouse);
        if !audio::load(&s.root, audio::PACKS[key].1, audio::MICE[mouse].1) {
            eprintln!("keycaps: no recordings for {} in {}", audio::PACKS[key].1, s.root.display());
        }
    }
}

/// Plays a switch or a mouse without choosing it, as pointing at it in the tray menu
/// does. With neither, goes back to the chosen ones.
pub fn preview(s: &mut State, key: Option<usize>, mouse: Option<usize>) {
    let chosen = (s.key, s.mouse.saturating_sub(1));
    load_packs(s, key.unwrap_or(chosen.0), mouse.unwrap_or(chosen.1));
    match (key, mouse) {
        (Some(_), _) => sound(s, audio::PRESS + 2, 0.0, 1.0),
        (_, Some(_)) => sound(s, audio::MOUSE, 0.0, 1.0),
        _ => {}
    }
}

/// Pushes the settings into the audio engine and the input devices. Only what is switched
/// on is listened to; off means no device is open at all.
fn apply(s: &mut State) {
    audio::set_volume(s.vol as f32 / 100.0, s.mvol as f32 / 100.0);
    let (key, mouse) = (s.key, s.mouse.saturating_sub(1));
    load_packs(s, key, mouse);
    s.devices.clear();
    open_devices(s);
}

/// The folder for a kind of file ($XDG_CONFIG_HOME or $XDG_STATE_HOME, else under home).
fn folder(variable: &str, fallback: &str) -> PathBuf {
    let home = PathBuf::from(std::env::var_os("HOME").unwrap_or_default());
    std::env::var_os(variable).map(PathBuf::from).filter(|p| p.is_absolute()).unwrap_or(home.join(fallback)).join("keycaps")
}

/// Replaces a small file in one step, so that a crash cannot leave half of it.
fn write(folder: PathBuf, name: &str, content: String) {
    let _ = std::fs::create_dir_all(&folder);
    let partial = folder.join(format!("{name}.new"));
    if std::fs::write(&partial, content).and_then(|_| std::fs::rename(&partial, folder.join(name))).is_err() {
        eprintln!("keycaps: could not write {}", folder.join(name).display());
    }
}

fn save_stats(s: &mut State) {
    if std::mem::take(&mut s.dirty) {
        write(folder("XDG_STATE_HOME", ".local/state"), "today", format!("{} {}\n", s.stats.day, s.stats.n));
    }
}

/// The settings as the commands that set them, one per line.
fn settings(s: &State) -> String {
    let word = |on| if on { "on" } else { "off" };
    let mouse = s.mouse.checked_sub(1).map_or("off", |m| audio::MICE[m].1);
    format!(
        "{}\nswitch {}\nmouse {mouse}\nvolume {}\nmouse-volume {}\nspatial {}\nkeyup {}\nscroll {}\n",
        word(s.on), audio::PACKS[s.key].1, s.vol, s.mvol, WIDTH[s.spatial].0.to_lowercase(), word(s.keyup), word(s.scroll)
    )
}

/// Whether the keyboard can be heard, in words.
pub fn ears(s: &State) -> String {
    match (s.on, s.deaf, s.devices.len()) {
        (false, ..) => "Not listening while it is off.".into(),
        (_, true, _) => "Cannot hear the keyboard: this user may not read /dev/input.\nRun `sudo usermod -aG input $USER`, then log out and back in.".into(),
        (.., n) => format!("Listening to {n} input device{}.", if n == 1 { "" } else { "s" }),
    }
}

/// What `keycaps status` prints.
fn status(s: &State) -> String {
    format!("Keycaps is {}{} · {} keys today\n{}\n", settings(s), audio::PACKS[s.key].0, s.stats.n, ears(s))
}

/// Changes one setting as a command says, without acting on it yet. Ok carries text for
/// the user if the command was a question.
fn set(s: &mut State, verb: &str, value: Option<&str>) -> Result<String, String> {
    let switch = |now: bool| match value {
        Some("on") => Ok(true),
        Some("off") => Ok(false),
        None | Some("toggle") => Ok(!now),
        Some(other) => Err(format!("\"{other}\" is neither on nor off")),
    };
    let percent = || value.and_then(|v| v.trim_end_matches('%').parse().ok()).filter(|v| *v <= 100).ok_or("give a volume from 0 to 100".to_string());
    let list = |packs: &[(&str, &str)], chosen: Option<usize>| packs.iter().enumerate().map(|(i, p)| format!("{} {:26} {}\n", if chosen == Some(i) { '*' } else { ' ' }, p.1, p.0)).collect::<String>();
    let find = |packs: &[(&str, &str)], name: &str| packs.iter().position(|p| p.1 == name).ok_or(format!("there is no \"{name}\": run this without a name for the list"));
    match (verb, value) {
        ("on" | "off", _) => s.on = verb == "on",
        ("toggle", _) => s.on = !s.on,
        ("keyup", _) => s.keyup = switch(s.keyup)?,
        ("scroll", _) => s.scroll = switch(s.scroll)?,
        ("volume", _) => s.vol = percent()?,
        ("mouse-volume", _) => s.mvol = percent()?,
        ("switch", None) => return Ok(list(&audio::PACKS, Some(s.key))),
        ("switch", Some(name)) => s.key = find(&audio::PACKS, name)?,
        ("mouse", None) => return Ok(format!("{} off\n", if s.mouse == 0 { '*' } else { ' ' }) + &list(&audio::MICE, s.mouse.checked_sub(1))),
        ("mouse", Some("off")) => s.mouse = 0,
        ("mouse", Some(name)) => s.mouse = 1 + find(&audio::MICE, name)?,
        ("spatial", Some(name)) => s.spatial = WIDTH.iter().position(|w| w.0.eq_ignore_ascii_case(name)).ok_or("off, subtle, natural or wide")?,
        ("status", _) => return Ok(status(s)),
        ("quit", _) => s.quit = true,
        _ => return Err(format!("\"{verb}\" is not a command: see keycaps help")),
    }
    Ok(String::new())
}

/// Carries out a command from `keycaps <command>` or the tray, and lets the new choice be heard.
pub fn command(s: &mut State, line: &str) -> Result<String, String> {
    let mut words = line.split_whitespace();
    let verb = words.next().unwrap_or("status");
    let answer = set(s, verb, words.next())?;
    if answer.is_empty() {
        write(folder("XDG_CONFIG_HOME", ".config"), "settings", settings(s));
        apply(s);
        match verb {
            // A tray cannot play a switch while it is only pointed at, so choosing one does.
            "volume" | "switch" => sound(s, audio::PRESS + 2, 0.0, 1.0),
            "mouse-volume" => sound(s, audio::MOUSE, 0.0, 1.0),
            "mouse" if s.mouse != 0 => sound(s, audio::MOUSE, 0.0, 1.0),
            "scroll" if s.scroll => sound(s, audio::SCROLL, 0.0, 1.0),
            // A key at the left edge, so that the new width can be heard.
            "spatial" => sound(s, audio::PRESS + 2, -WIDTH[s.spatial].1, 1.0),
            _ => {}
        }
    }
    Ok(answer)
}

/// Where the running app takes commands.
fn socket() -> PathBuf {
    std::env::var_os("XDG_RUNTIME_DIR").map_or(format!("/tmp/keycaps-{}", unsafe { getuid() }).into(), PathBuf::from).join("keycaps.sock")
}

/// `keycaps <command>`: hands the command to the running app and prints its answer.
fn ask(command: &str) -> ! {
    let mut answer = String::new();
    let sent = UnixStream::connect(socket()).and_then(|mut app| {
        app.write_all(command.as_bytes())?;
        app.shutdown(std::net::Shutdown::Write)?;
        app.read_to_string(&mut answer)
    });
    if sent.is_err() {
        eprintln!("Keycaps is not running. Start it with: keycaps");
        std::process::exit(1);
    }
    print!("{answer}");
    std::process::exit(answer.starts_with("keycaps: ") as i32)
}

/// Answers the commands that are waiting on the socket. True if one of them changed a setting.
fn answer(s: &mut State, commands: &UnixListener) -> bool {
    let mut changed = false;
    while let Ok((mut caller, _)) = commands.accept() {
        let mut line = String::new();
        // A caller that connects and says nothing must not hold up the keys.
        let _ = caller.set_read_timeout(Some(Duration::from_millis(100)));
        let _ = (&caller).take(256).read_to_string(&mut line);
        let answer = command(s, &line).unwrap_or_else(|problem| format!("keycaps: {problem}\n"));
        changed |= answer.is_empty();
        let _ = caller.write_all(answer.as_bytes());
    }
    changed
}

/// The sounds folder: beside the program as unpacked, or under share/ as installed.
fn sounds() -> PathBuf {
    let program = std::env::current_exe().unwrap_or_default();
    let beside = program.parent().unwrap_or(&program);
    [beside.join("sounds"), beside.join("../share/keycaps/sounds")].into_iter().find(|p| p.is_dir()).unwrap_or_default()
}

pub fn main() {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    match arguments.first().map(String::as_str) {
        Some("--demo") => crate::demo(&sounds(), |ms| {
            (0..ms / 5).for_each(|_| {
                audio::pump();
                std::thread::sleep(Duration::from_millis(5));
            })
        }),
        Some("help" | "--help" | "-h") => return print!("{USAGE}"),
        Some("version" | "--version") => return println!("Keycaps {}", env!("CARGO_PKG_VERSION")),
        Some(_) => ask(&arguments.join(" ")),
        None => {}
    }
    if UnixStream::connect(socket()).is_ok() {
        eprintln!("Keycaps is already running. `keycaps help` lists what it can be told.");
        std::process::exit(1);
    }
    let _ = std::fs::DirBuilder::new().recursive(true).mode(0o700).create(socket().parent().unwrap());
    let _ = std::fs::remove_file(socket()); // left behind by a run that was killed
    let commands = UnixListener::bind(socket()).and_then(|l| l.set_nonblocking(true).map(|_| l));
    if let Err(e) = &commands {
        eprintln!("keycaps: no commands will be taken: {e} ({})", socket().display());
    }

    let mut s = State::new(sounds());
    for line in std::fs::read_to_string(folder("XDG_CONFIG_HOME", ".config").join("settings")).unwrap_or_default().lines() {
        let mut words = line.split_whitespace();
        if let Err(problem) = set(&mut s, words.next().unwrap_or(""), words.next()) {
            eprintln!("keycaps: in the settings file: {problem}");
        }
    }
    let saved = std::fs::read_to_string(folder("XDG_STATE_HOME", ".local/state").join("today")).unwrap_or_default();
    let mut saved = saved.split_whitespace().map(|n| n.parse().unwrap_or(0));
    s.stats = Stats { day: saved.next().unwrap_or(0), n: saved.next().unwrap_or(0) };
    s.stats.roll(stats::today());
    audio::init();
    apply(&mut s);
    if s.deaf {
        eprintln!("keycaps: {}", ears(&s));
    }

    // Keyboards and mice come and go: the folder is watched for new ones, and for the
    // moment a new one becomes readable.
    let watch = unsafe {
        let fd = inotify_init1(O_NONBLOCK | O_CLOEXEC);
        inotify_add_watch(fd, c"/dev/input".as_ptr(), IN_CREATE | IN_ATTRIB);
        (fd >= 0).then(|| File::from_raw_fd(fd))
    };
    let mut tray = tray::Tray::connect();
    let fd_of = |file: Option<RawFd>| PollFd { fd: file.unwrap_or(-1), events: POLLIN, revents: 0 };
    let mut fds = Vec::new();
    while !s.quit {
        fds.clear();
        fds.extend([fd_of(commands.as_ref().ok().map(|c| c.as_raw_fd())), fd_of(watch.as_ref().map(|w| w.as_raw_fd())), fd_of(audio::fd()), fd_of(tray.as_ref().map(|t| t.fd()))]);
        fds.extend(s.devices.iter().map(|d| fd_of(Some(d.file.as_raw_fd()))));
        // Asleep until something happens, or until it is time to look at the sound server.
        let wait = s.due.map_or(-1, |due| due.saturating_duration_since(Instant::now()).as_millis() as c_int + 1);
        if unsafe { poll(fds.as_mut_ptr(), fds.len() as c_ulong, wait) } < 0 {
            continue; // interrupted by a signal
        }
        // The devices first: the lists below are in step only until a command changes them.
        let mut gone = Vec::new();
        for device in 0..s.devices.len() {
            if fds[4 + device].revents != 0 && !listen(&mut s, device) {
                gone.push(s.devices[device].number);
            }
        }
        s.devices.retain(|d| !gone.contains(&d.number));
        if fds[1].revents != 0 {
            let mut news = [0u8; 4096];
            while watch.as_ref().is_some_and(|mut w| w.read(&mut news).is_ok_and(|n| n > 0)) {}
            open_devices(&mut s);
        }
        if let (true, Ok(commands)) = (fds[0].revents != 0, &commands) {
            if answer(&mut s, commands) && !s.quit {
                // What a command changed shows in the tray menu too. Without a tray, or once it is gone, life goes on.
                tray = tray.and_then(|mut t| t.changed().is_ok().then_some(t));
            }
        }
        if fds[3].revents != 0 {
            tray = tray.and_then(|mut t| t.serve(&mut s).then_some(t));
        }
        if s.due.is_some_and(|due| Instant::now() >= due) {
            // This comes round every IDLE_SECS while sounds keep coming, so a kill or a
            // logout loses at most that much of the count.
            save_stats(&mut s);
            let idle = s.last + Duration::from_secs_f64(IDLE_SECS);
            s.due = (Instant::now() < idle).then_some(idle);
            if s.due.is_none() {
                audio::stop();
            }
        }
        audio::pump();
    }
    save_stats(&mut s);
    let _ = std::fs::remove_file(socket());
}

#[cfg(test)]
mod tests {
    use super::*;

    pub fn state() -> State {
        State::new(PathBuf::new())
    }

    #[test]
    fn keys_map_to_their_row_recording_and_side() {
        assert_eq!((KEYS[2].1, KEYS[16].1, KEYS[30].1, KEYS[44].1, KEYS[29].1), (0, 1, 2, 3, 4)); // 1, Q, A, Z, control
        assert_eq!((KEYS[57].1, KEYS[28].1, KEYS[96].1, KEYS[14].1), (5, 6, 6, 7)); // space, enter, number pad enter, backspace
        assert_eq!(KEYS[1], (-100, 0)); // escape: left edge, shares the number row
        assert!(KEYS[16].0 < 0 && KEYS[25].0 > 0 && KEYS[106] == (100, 2)); // Q left, P right, right arrow at the edge
        assert_eq!((KEYS[224], KEYS[115]), ((-90, 0), (75, 0))); // brightness down, volume up
    }
}
