//! The macOS app: a menu bar item, a listen-only event tap that wakes the process per
//! key, and a timer that releases the audio device after `IDLE_SECS` of silence. Menu
//! and stats work happens only when the menu is opened.

use crate::audio::{self, Shared};
use crate::objc::*;
use crate::stats::{self, Stats};
use crate::{IDLE_SECS, WIDTH};
use std::cell::UnsafeCell;
use std::ffi::{c_char, c_void};
use std::path::PathBuf;
use std::ptr::{null, null_mut};

/// Main thread only. Each callback takes it once and passes it down.
struct State {
    on: bool,
    keyup: bool,
    scroll: bool,
    vol: usize,              // keyboard volume, percent
    mvol: usize,             // mouse volume, percent
    key: usize,              // index into audio::PACKS
    mouse: usize,            // 0 = off, else 1 + index into audio::MICE
    spatial: usize,          // index into WIDTH
    loaded: (usize, usize),  // the switch and mouse packs that are in the audio bank
    root: PathBuf,           // the bundle's sounds folder
    stats: Stats,
    dirty: bool,  // stats changed since they were saved
    quarter: i64, // quarter-hour of the last date check
    last: f64,    // time of the last sound
    last_scroll: f64,
    rng: u32,
    tap: *mut c_void,
    timer: *mut c_void,
    button: Id,
}

static ST: Shared<State> = Shared(UnsafeCell::new(State {
    on: true,
    keyup: true,
    scroll: false,
    vol: 60,
    mvol: 60,
    key: 0,
    mouse: 0,
    spatial: 2,
    loaded: (usize::MAX, usize::MAX),
    root: PathBuf::new(),
    stats: Stats { day: 0, n: 0 },
    dirty: false,
    quarter: 0,
    last: 0.0,
    last_scroll: 0.0,
    rng: 0x2545_F491,
    tap: null_mut(),
    timer: null_mut(),
    button: null_mut(),
}));

type TapFn = extern "C" fn(*mut c_void, u32, *mut c_void, *mut c_void) -> *mut c_void;
type HotKeyFn = extern "C" fn(*mut c_void, *mut c_void, *mut c_void) -> i32;
#[repr(C)]
struct HotKeyId {
    signature: u32,
    id: u32,
}

#[link(name = "CoreFoundation", kind = "framework")]
#[link(name = "CoreGraphics", kind = "framework")]
#[link(name = "Carbon", kind = "framework")]
unsafe extern "C" {
    static kCFRunLoopCommonModes: *const c_void;
    fn CFAbsoluteTimeGetCurrent() -> f64;
    fn CFRunLoopGetMain() -> *mut c_void;
    fn CFRunLoopAddSource(rl: *mut c_void, src: *mut c_void, mode: *const c_void);
    fn CFRunLoopAddTimer(rl: *mut c_void, timer: *mut c_void, mode: *const c_void);
    fn CFRunLoopTimerCreate(alloc: *const c_void, fire: f64, interval: f64, flags: usize, order: isize, f: extern "C" fn(*mut c_void, *mut c_void), ctx: *const c_void) -> *mut c_void;
    fn CFRunLoopTimerSetNextFireDate(timer: *mut c_void, fire: f64);
    fn CFMachPortCreateRunLoopSource(alloc: *const c_void, port: *mut c_void, order: isize) -> *mut c_void;
    fn CFMachPortInvalidate(port: *mut c_void);
    fn CFRelease(obj: *mut c_void);
    fn CGEventTapCreate(tap: u32, place: u32, options: u32, mask: u64, f: TapFn, ctx: *mut c_void) -> *mut c_void;
    fn CGEventTapEnable(tap: *mut c_void, on: bool);
    fn CGEventGetIntegerValueField(event: *mut c_void, field: u32) -> i64;
    fn CGEventGetFlags(event: *mut c_void) -> u64;
    fn CGPreflightListenEventAccess() -> bool;
    fn CGRequestListenEventAccess() -> bool;
    fn GetApplicationEventTarget() -> *mut c_void;
    fn InstallEventHandler(target: *mut c_void, f: HotKeyFn, n: usize, types: *const [u32; 2], ctx: *mut c_void, out: *mut *mut c_void) -> i32;
    fn RegisterEventHotKey(code: u32, mods: u32, id: HotKeyId, target: *mut c_void, options: u32, out: *mut *mut c_void) -> i32;
    fn posix_spawn(pid: *mut i32, path: *const c_char, actions: *const c_void, attributes: *const c_void, argv: *const *const c_char, env: *const *const c_char) -> i32;
    fn waitpid(pid: i32, status: *mut i32, options: i32) -> i32;
}

// CGEventType
const LEFT_DOWN: u32 = 1;
const LEFT_UP: u32 = 2;
const RIGHT_DOWN: u32 = 3;
const RIGHT_UP: u32 = 4;
const KEY_DOWN: u32 = 10;
const KEY_UP: u32 = 11;
const FLAGS_CHANGED: u32 = 12;
const SYSTEM_DEFINED: u32 = 14;
const SCROLL: u32 = 22;
const OTHER_DOWN: u32 = 25;
const OTHER_UP: u32 = 26;
// CGEventField
const AUTOREPEAT: u32 = 8;
const KEYCODE: u32 = 9;

// Menu item tags are `kind * 1000 + index`.
const T_ON: isize = 1;
const T_VOLUME: isize = 2;
const T_KEY: isize = 3;
const T_MOUSE: isize = 4;
const T_SCROLL: isize = 5;
const T_KEYUP: isize = 6;
const T_QUIT: isize = 7;
const T_ACCESS: isize = 8;
const T_INFO: isize = 9;
const T_SPATIAL: isize = 10;
const T_MVOLUME: isize = 11;

/// ANSI key codes, left to right, one row per line.
const ROWS: [&[u8]; 6] = [
    &[53, 122, 120, 99, 118, 96, 97, 98, 100, 101, 109, 103, 111],
    &[50, 18, 19, 20, 21, 23, 22, 26, 28, 25, 29, 27, 24, 51],
    &[48, 12, 13, 14, 15, 17, 16, 32, 34, 31, 35, 33, 30, 42],
    &[57, 0, 1, 2, 3, 5, 4, 38, 40, 37, 41, 39, 36],
    &[56, 6, 7, 8, 9, 11, 45, 46, 43, 47, 44, 60],
    &[63, 59, 58, 55, 49, 54, 61, 123, 125, 126, 124],
];

/// Per key code: position across the keyboard, -100 (left edge) to 100 (right edge), and
/// which recording it plays (`audio::PRESS` + 0..=4 by row, 5 space, 6 enter, 7 backspace).
const KEYS: [(i8, u8); 128] = {
    let mut t = [(0i8, 2u8); 128];
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
    t[49].1 = 5; // space
    t[36].1 = 6; // return
    t[76].1 = 6; // keypad enter
    t[51].1 = 7; // delete
    t
};

/// The per-key bit a modifier sets in the event flags while it is held.
fn modifier(code: usize) -> u64 {
    match code {
        59 => 0x1,       // left control
        56 => 0x2,       // left shift
        60 => 0x4,       // right shift
        55 => 0x8,       // left command
        54 => 0x10,      // right command
        58 => 0x20,      // left option
        61 => 0x40,      // right option
        62 => 0x2000,    // right control
        57 => 0x1_0000,  // caps lock
        63 => 0x80_0000, // fn
        _ => 0,
    }
}

/// The top-row keys without a key code (brightness, keyboard light, media, volume) arrive
/// as "system defined" events. Decodes such an event's `data1` into (press?, position
/// across the keyboard, recording). None for key repeats and for the power/lock button.
fn media_key(data1: isize) -> Option<(bool, i8, u8)> {
    let pan = match data1 >> 16 {
        4 | 6 => return None, // caps lock (already heard as a modifier) and power/lock
        2 | 3 => -90,         // brightness
        21..=23 => -40,       // keyboard light
        16..=20 => 25,        // previous, play, next
        0 | 1 | 7 => 75,      // volume, mute
        _ => 0,
    };
    match (data1 >> 8 & 0xFF, data1 & 1) {
        (0xA, 0) => Some((true, pan, 0)),
        (0xB, _) => Some((false, pan, 0)),
        _ => None,
    }
}

/// Plays a bank slot and makes sure the idle timer will release the audio device.
fn sound(s: &mut State, now: f64, slot: u32, pan: f32, rate: f32) {
    let was_running = audio::running();
    audio::play(slot, pan, rate);
    s.last = now;
    if !was_running {
        unsafe { CFRunLoopTimerSetNextFireDate(s.timer, now + IDLE_SECS) };
    }
}

extern "C" fn tapped(_: *mut c_void, kind: u32, event: *mut c_void, _: *mut c_void) -> *mut c_void {
    let s = unsafe { &mut *ST.0.get() };
    let now = unsafe { CFAbsoluteTimeGetCurrent() };
    let pitch = crate::pitch(&mut s.rng);
    match kind {
        KEY_DOWN | KEY_UP | FLAGS_CHANGED | SYSTEM_DEFINED => {
            let (down, pan, key) = if kind == SYSTEM_DEFINED {
                // Subtype 8 is the one that carries keys.
                let e: Id = unsafe { send1(class(c"NSEvent"), c"eventWithCGEvent:", event) };
                if e.is_null() || unsafe { send0::<i16>(e, c"subtype") } != 8 {
                    return event;
                }
                match media_key(unsafe { send0(e, c"data1") }) {
                    Some(key) => key,
                    None => return event,
                }
            } else {
                let code = unsafe { CGEventGetIntegerValueField(event, KEYCODE) } as usize;
                let down = match kind {
                    _ if code == 127 => return event, // power/lock button
                    KEY_DOWN if unsafe { CGEventGetIntegerValueField(event, AUTOREPEAT) } != 0 => return event,
                    FLAGS_CHANGED if modifier(code) == 0 => return event,
                    FLAGS_CHANGED => (unsafe { CGEventGetFlags(event) } & modifier(code)) != 0,
                    _ => kind == KEY_DOWN,
                };
                // Codes past the table are top-row keys such as Mission Control.
                let (pan, key) = KEYS.get(code).copied().unwrap_or((0, 0));
                (down, pan, key)
            };
            if down {
                // Local midnight always falls on a quarter-hour, so this is the only date check needed.
                let quarter = (now / 900.0) as i64;
                if quarter != s.quarter {
                    s.quarter = quarter;
                    s.stats.roll(stats::today());
                }
                s.stats.n += 1;
                s.dirty = true;
            } else if !s.keyup {
                return event;
            }
            let slot = if down { audio::PRESS + key as u32 } else { audio::RELEASE + (key as u32).saturating_sub(4) };
            sound(s, now, slot, pan as f32 / 100.0 * WIDTH[s.spatial].1, pitch);
        }
        LEFT_DOWN | LEFT_UP | RIGHT_DOWN | RIGHT_UP | OTHER_DOWN | OTHER_UP => {
            // Left, right and wheel button each have their own press and release.
            let button = match kind {
                LEFT_DOWN | LEFT_UP => 0,
                RIGHT_DOWN | RIGHT_UP => 2,
                _ => 4,
            };
            sound(s, now, audio::MOUSE + button + matches!(kind, LEFT_UP | RIGHT_UP | OTHER_UP) as u32, 0.0, pitch);
        }
        SCROLL if now - s.last_scroll >= 0.04 => {
            s.last_scroll = now;
            sound(s, now, audio::SCROLL, 0.0, pitch);
        }
        SCROLL => {}
        // The system disabled the tap (timeout or user input): turn it back on.
        _ => unsafe { CGEventTapEnable(s.tap, true) },
    }
    event
}

extern "C" fn idle(_: *mut c_void, _: *mut c_void) {
    let s = unsafe { &mut *ST.0.get() };
    // This fires at least every IDLE_SECS while sounds keep coming, so a kill or a
    // logout loses at most that much of the count.
    save_stats(s);
    let due = s.last + IDLE_SECS;
    if unsafe { CFAbsoluteTimeGetCurrent() } >= due {
        audio::stop();
    } else {
        unsafe { CFRunLoopTimerSetNextFireDate(s.timer, due) };
    }
}

fn defaults() -> Id {
    unsafe { send0(class(c"NSUserDefaults"), c"standardUserDefaults") }
}

/// Integers are stored +1 so that a missing key (0) falls back to `default`.
fn get(key: &str, default: i64, max: i64) -> i64 {
    let v: isize = unsafe { send1(defaults(), c"integerForKey:", ns(key)) };
    if v == 0 { default } else { (v as i64 - 1).clamp(0, max) }
}

fn put(key: &str, v: i64) {
    unsafe { send2::<isize, Id, ()>(defaults(), c"setInteger:forKey:", v as isize + 1, ns(key)) }
}

fn save_stats(s: &mut State) {
    if std::mem::take(&mut s.dirty) {
        put("day", s.stats.day);
        put("n", s.stats.n);
    }
}

fn save_settings(s: &State) {
    put("on", s.on as i64);
    put("keyup", s.keyup as i64);
    put("vol", s.vol as i64);
    put("mvol", s.mvol as i64);
    put("key", s.key as i64);
    put("mouse", s.mouse as i64);
    put("scroll", s.scroll as i64);
    put("spatial", s.spatial as i64);
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

/// Pushes the settings into the audio engine, the event tap and the menu bar icon.
/// The tap only subscribes to what is switched on; disabled means no tap at all.
fn apply(s: &mut State) {
    audio::set_volume(s.vol as f32 / 100.0, s.mvol as f32 / 100.0);
    let (key, mouse) = (s.key, s.mouse.saturating_sub(1));
    load_packs(s, key, mouse);
    unsafe {
        if !s.tap.is_null() {
            CFMachPortInvalidate(s.tap);
            CFRelease(s.tap);
            s.tap = null_mut();
        }
        if s.on && CGRequestListenEventAccess() {
            let mut mask = 1u64 << KEY_DOWN | 1 << FLAGS_CHANGED | 1 << SYSTEM_DEFINED;
            if s.keyup {
                mask |= 1 << KEY_UP;
            }
            if s.mouse != 0 {
                mask |= 1 << LEFT_DOWN | 1 << LEFT_UP | 1 << RIGHT_DOWN | 1 << RIGHT_UP | 1 << OTHER_DOWN | 1 << OTHER_UP;
            }
            if s.scroll {
                mask |= 1 << SCROLL;
            }
            // session tap, inserted at the head, listen-only
            s.tap = CGEventTapCreate(1, 0, 1, mask, tapped, null_mut());
            if !s.tap.is_null() {
                let src = CFMachPortCreateRunLoopSource(null(), s.tap, 0);
                CFRunLoopAddSource(CFRunLoopGetMain(), src, kCFRunLoopCommonModes);
                CFRelease(src);
            }
        }
        send1::<bool, ()>(s.button, c"setAppearsDisabled:", s.tap.is_null());
    }
}

/// Without an Apple Developer ID every update is a new identity to macOS: the old Input
/// Monitoring entry stays switched on in System Settings but applies to nothing, and
/// nobody is asked again. So the first launch of a build that finds itself without access
/// clears its entry, and the request in `apply` then asks.
fn forget_stale_permission() {
    let modified = std::env::current_exe().and_then(std::fs::metadata).and_then(|m| m.modified()).ok();
    let built = modified.and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map_or(0, |d| d.as_secs() as i64);
    if get("build", 0, i64::MAX) == built {
        return;
    }
    put("build", built);
    unsafe {
        // Run outside the app bundle there is no identifier, and no entry to clear.
        let id: Id = send0(send0(class(c"NSBundle"), c"mainBundle"), c"bundleIdentifier");
        if !id.is_null() && !CGPreflightListenEventAccess() {
            let argv = [c"tccutil".as_ptr(), c"reset".as_ptr(), c"ListenEvent".as_ptr(), send0(id, c"UTF8String"), null()];
            let mut pid = 0;
            if posix_spawn(&mut pid, c"/usr/bin/tccutil".as_ptr(), null(), null(), argv.as_ptr(), &null()) == 0 {
                waitpid(pid, null_mut(), 0);
            }
        }
    }
}

/// Makes ⇧⌘K toggle sound from anywhere: key code 40 with cmdKey | shiftKey, handled as
/// a 'keyb' / hot-key-pressed (5) event. False if the system refuses.
fn install_hotkey() -> bool {
    unsafe {
        let target = GetApplicationEventTarget();
        InstallEventHandler(target, hotkey, 1, &[u32::from_be_bytes(*b"keyb"), 5], null_mut(), null_mut()) == 0
            && RegisterEventHotKey(40, 0x100 | 0x200, HotKeyId { signature: u32::from_be_bytes(*b"kcap"), id: 1 }, target, 0, &mut null_mut()) == 0
    }
}

extern "C" fn hotkey(_: *mut c_void, _: *mut c_void, _: *mut c_void) -> i32 {
    let s = unsafe { &mut *ST.0.get() };
    s.on = !s.on;
    save_settings(s);
    apply(s);
    0
}

extern "C" fn act(_: Id, _: Sel, sender: Id) {
    let s = unsafe { &mut *ST.0.get() };
    let tag: isize = unsafe { send0(sender, c"tag") };
    let (kind, i) = (tag / 1000, (tag % 1000) as usize);
    match kind {
        T_ON => s.on = !s.on,
        T_VOLUME => s.vol = i,
        T_MVOLUME => s.mvol = i,
        T_KEY => s.key = i,
        T_MOUSE => s.mouse = i.min(audio::MICE.len()),
        T_SPATIAL => s.spatial = i.min(WIDTH.len() - 1),
        T_SCROLL => s.scroll = !s.scroll,
        T_KEYUP => s.keyup = !s.keyup,
        T_QUIT => unsafe {
            save_stats(s);
            return send1(send0(class(c"NSApplication"), c"sharedApplication"), c"terminate:", null_mut::<c_void>());
        },
        _ => unsafe {
            let url: Id = send1(class(c"NSURL"), c"URLWithString:", ns("x-apple.systempreferences:com.apple.preference.security?Privacy_ListenEvent"));
            send1::<Id, bool>(send0(class(c"NSWorkspace"), c"sharedWorkspace"), c"openURL:", url);
            return;
        },
    }
    save_settings(s);
    apply(s);
    // Let the new choice be heard.
    let now = unsafe { CFAbsoluteTimeGetCurrent() };
    match kind {
        T_VOLUME => sound(s, now, audio::PRESS + 2, 0.0, 1.0),
        T_MVOLUME => sound(s, now, audio::MOUSE, 0.0, 1.0),
        T_SCROLL if s.scroll => sound(s, now, audio::SCROLL, 0.0, 1.0),
        // A key at the left edge, so that the new width can be heard.
        T_SPATIAL => sound(s, now, audio::PRESS + 2, -WIDTH[s.spatial].1, 1.0),
        _ => {}
    }
}

/// `menu:willHighlightItem:` — hovering over a pack plays it without switching to it.
extern "C" fn highlighted(_: Id, _: Sel, _menu: Id, item: Id) {
    let s = unsafe { &mut *ST.0.get() };
    let tag: isize = if item.is_null() { 0 } else { unsafe { send0(item, c"tag") } };
    let (kind, i, key, mouse) = (tag / 1000, (tag % 1000) as usize, s.key, s.mouse.saturating_sub(1));
    let now = unsafe { CFAbsoluteTimeGetCurrent() };
    if kind == T_KEY && i < audio::PACKS.len() {
        load_packs(s, i, mouse);
        sound(s, now, audio::PRESS + 2, 0.0, 1.0);
    } else if kind == T_MOUSE && (1..=audio::MICE.len()).contains(&i) {
        load_packs(s, key, i - 1);
        sound(s, now, audio::MOUSE, 0.0, 1.0);
    }
}

/// `menuDidClose:` — back to the chosen packs if another one was only previewed.
extern "C" fn closed(_: Id, _: Sel, _menu: Id) {
    let s = unsafe { &mut *ST.0.get() };
    let (key, mouse) = (s.key, s.mouse.saturating_sub(1));
    load_packs(s, key, mouse);
}

/// `menuNeedsUpdate:` — the only place the menu is brought up to date.
extern "C" fn opened(_: Id, _: Sel, menu: Id) {
    let s = unsafe { &mut *ST.0.get() };
    if s.on && s.tap.is_null() {
        apply(s); // permission may have been granted since the last look
    }
    s.stats.roll(stats::today());
    refresh(s, menu);
}

fn refresh(s: &State, menu: Id) {
    unsafe {
        for n in 0..send0::<isize>(menu, c"numberOfItems") {
            let item: Id = send1(menu, c"itemAtIndex:", n);
            let tag: isize = send0(item, c"tag");
            let (kind, i) = (tag / 1000, (tag % 1000) as usize);
            let checked = match kind {
                T_ON => s.on,
                T_VOLUME => s.vol == i,
                T_MVOLUME => s.mvol == i,
                T_KEY => s.key == i,
                T_MOUSE => s.mouse == i,
                T_SPATIAL => s.spatial == i,
                T_SCROLL => s.scroll,
                T_KEYUP => s.keyup,
                _ => false,
            };
            send1::<isize, ()>(item, c"setState:", checked as isize);
            if kind == T_INFO {
                let info = format!("{} · {} keys today", audio::PACKS[s.key].0, s.stats.n);
                send1::<Id, ()>(item, c"setTitle:", ns(&info));
            }
            if kind == T_ACCESS {
                send1::<bool, ()>(item, c"setHidden:", !s.on || !s.tap.is_null());
            }
            let sub: Id = send0(item, c"submenu");
            if !sub.is_null() {
                refresh(s, sub);
            }
        }
    }
}

/// Appends an item. A null `target` makes it a greyed-out label.
fn item(menu: Id, target: Id, title: &str, tag: isize, key: &str) -> Id {
    unsafe {
        let action = if target.is_null() { null() } else { sel(c"act:") };
        let item: Id = send3(send0(class(c"NSMenuItem"), c"alloc"), c"initWithTitle:action:keyEquivalent:", ns(title), action, ns(key));
        send1::<Id, ()>(item, c"setTarget:", target);
        send1::<isize, ()>(item, c"setTag:", tag);
        send1::<Id, ()>(menu, c"addItem:", item);
        item
    }
}

fn submenu(menu: Id, title: &str) -> Id {
    unsafe {
        let sub: Id = send1(send0(class(c"NSMenu"), c"alloc"), c"initWithTitle:", ns(title));
        send1::<Id, ()>(item(menu, null_mut(), title, 0, ""), c"setSubmenu:", sub);
        sub
    }
}

fn separator(menu: Id) {
    unsafe { send1::<Id, ()>(menu, c"addItem:", send0::<Id>(class(c"NSMenuItem"), c"separatorItem")) }
}

fn build_menu(h: Id) -> Id {
    let menu: Id = unsafe { send1(send0(class(c"NSMenu"), c"alloc"), c"initWithTitle:", ns("Keycaps")) };
    unsafe { send1::<Id, ()>(menu, c"setDelegate:", h) };
    item(menu, null_mut(), "", T_INFO * 1000, "");
    item(menu, h, "Allow Input Monitoring, then reopen Keycaps…", T_ACCESS * 1000, "");
    separator(menu);
    item(menu, h, "Enable Keycaps", T_ON * 1000, "K"); // capital K shows as ⇧⌘K
    for (title, kind) in [("Keyboard Volume", T_VOLUME), ("Mouse Volume", T_MVOLUME)] {
        let volume = submenu(menu, title);
        for pct in [20, 40, 60, 80, 100] {
            item(volume, h, &format!("{pct}%"), kind * 1000 + pct, "");
        }
    }
    let keys = submenu(menu, "Switch Keys");
    unsafe { send1::<Id, ()>(keys, c"setDelegate:", h) }; // for the hover preview
    let mut i = 0;
    for (title, count) in audio::GROUPS {
        if i > 0 {
            separator(keys);
        }
        item(keys, null_mut(), title, 0, "");
        for (name, _) in &audio::PACKS[i..i + count] {
            item(keys, h, name, T_KEY * 1000 + i as isize, "");
            i += 1;
        }
    }
    let mice = submenu(menu, "Mouse Clicks");
    unsafe { send1::<Id, ()>(mice, c"setDelegate:", h) }; // for the hover preview
    item(mice, h, "Off", T_MOUSE * 1000, "");
    for (i, (name, _)) in audio::MICE.iter().enumerate() {
        item(mice, h, name, T_MOUSE * 1000 + i as isize + 1, "");
    }
    let spatial = submenu(menu, "Spatial Audio");
    for (i, (name, _)) in WIDTH.iter().enumerate() {
        item(spatial, h, name, T_SPATIAL * 1000 + i as isize, "");
    }
    item(menu, h, "Key-up sounds", T_KEYUP * 1000, "");
    item(menu, h, "Scroll ticks", T_SCROLL * 1000, "");
    separator(menu);
    item(menu, h, "Quit Keycaps", T_QUIT * 1000, "q");
    menu
}

/// The sounds folder inside the app bundle (Contents/Resources/sounds).
fn sounds() -> PathBuf {
    std::env::current_exe().ok().and_then(|exe| Some(exe.parent()?.parent()?.join("Resources/sounds"))).unwrap_or_default()
}

pub fn main() {
    if std::env::args().any(|a| a == "--demo") {
        crate::demo(&sounds(), |ms| std::thread::sleep(std::time::Duration::from_millis(ms)));
    }
    let s = unsafe { &mut *ST.0.get() };
    unsafe {
        let pool = objc_autoreleasePoolPush();
        let app: Id = send0(class(c"NSApplication"), c"sharedApplication");
        send1::<isize, bool>(app, c"setActivationPolicy:", 1); // accessory: no Dock icon

        s.on = get("on", 1, 1) == 1;
        s.keyup = get("keyup", 1, 1) == 1;
        s.vol = get("vol", 60, 100) as usize;
        s.mvol = get("mvol", 60, 100) as usize;
        s.key = get("key", 3, audio::PACKS.len() as i64 - 1) as usize;
        s.mouse = get("mouse", 0, audio::MICE.len() as i64) as usize;
        s.scroll = get("scroll", 0, 1) == 1;
        s.spatial = get("spatial", 2, WIDTH.len() as i64 - 1) as usize;
        s.root = sounds();
        s.stats = Stats { day: get("day", 0, i64::MAX), n: get("n", 0, i64::MAX) };
        s.stats.roll(stats::today());

        audio::init();
        let handler = object(
            c"KeycapsHandler",
            &[
                (c"act:", act as *const c_void, c"v@:@"),
                (c"menuNeedsUpdate:", opened as *const c_void, c"v@:@"),
                (c"menuDidClose:", closed as *const c_void, c"v@:@"),
                (c"menu:willHighlightItem:", highlighted as *const c_void, c"v@:@@"),
            ],
        );
        let status: Id = send1(send0(class(c"NSStatusBar"), c"systemStatusBar"), c"statusItemWithLength:", -2.0f64); // square
        send0::<Id>(status, c"retain");
        s.button = send0(status, c"button");
        let bundle: Id = send0(class(c"NSBundle"), c"mainBundle");
        let path: Id = send2(bundle, c"pathForResource:ofType:", ns("KeycapsMenuBar"), ns("pdf"));
        let mut icon: Id = if path.is_null() {
            null_mut()
        } else {
            send1(send0(class(c"NSImage"), c"alloc"), c"initWithContentsOfFile:", path)
        };
        if icon.is_null() {
            // `cargo run` has no bundle resources.
            icon = send2(class(c"NSImage"), c"imageWithSystemSymbolName:accessibilityDescription:", ns("keyboard"), ns("Keycaps"));
        } else {
            send1::<bool, ()>(icon, c"setTemplate:", true); // adapts to light and dark menu bars
            send1::<Id, ()>(icon, c"setAccessibilityDescription:", ns("Keycaps"));
            send0::<Id>(icon, c"autorelease");
        }
        send1::<Id, ()>(s.button, c"setImage:", icon);
        send1::<Id, ()>(status, c"setMenu:", build_menu(handler));

        // One timer for the whole app, parked far in the future until a sound arms it.
        s.timer = CFRunLoopTimerCreate(null(), CFAbsoluteTimeGetCurrent() + 1e10, 1e10, 0, 0, idle, null());
        CFRunLoopAddTimer(CFRunLoopGetMain(), s.timer, kCFRunLoopCommonModes);

        if !install_hotkey() {
            eprintln!("keycaps: could not register the shift-command-K shortcut");
        }

        forget_stale_permission();
        apply(s);
        objc_autoreleasePoolPop(pool);
        send0::<()>(app, c"run");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    unsafe extern "C" {
        fn CreateEvent(alloc: *const c_void, class: u32, kind: u32, when: f64, attributes: u32, out: *mut *mut c_void) -> i32;
        fn SendEventToEventTarget(event: *mut c_void, target: *mut c_void) -> i32;
        fn ReleaseEvent(event: *mut c_void);
    }

    /// The system accepts the ⇧⌘K registration, and a hot-key event reaching the app
    /// switches sound off. What it cannot do is press the keys.
    #[test]
    fn hot_key_event_toggles_sound() {
        let s = unsafe { &mut *ST.0.get() };
        s.on = true;
        s.loaded = (s.key, s.mouse.saturating_sub(1)); // keeps apply() away from the audio bank, which another test is using
        assert!(install_hotkey(), "the system refused the shortcut");
        unsafe {
            let mut event = null_mut();
            assert_eq!(CreateEvent(null(), u32::from_be_bytes(*b"keyb"), 5, 0.0, 0, &mut event), 0);
            assert_eq!(SendEventToEventTarget(event, GetApplicationEventTarget()), 0, "no handler took the event");
            ReleaseEvent(event);
            assert!(!(*ST.0.get()).on, "the handler did not switch sound off");
            // The handler saved that under this test binary's own name: remove it again.
            let name: Id = send0(send0(class(c"NSProcessInfo"), c"processInfo"), c"processName");
            send1::<Id, ()>(defaults(), c"removePersistentDomainForName:", name);
        }
    }

    #[test]
    fn keys_map_to_their_row_recording_and_side() {
        assert_eq!((KEYS[18].1, KEYS[12].1, KEYS[0].1, KEYS[6].1, KEYS[55].1), (0, 1, 2, 3, 4)); // 1, Q, A, Z, command
        assert_eq!((KEYS[49].1, KEYS[36].1, KEYS[76].1, KEYS[51].1), (5, 6, 6, 7)); // space, return, keypad enter, delete
        assert_eq!(KEYS[53], (-100, 0)); // escape: left edge, shares the number row
        assert!(KEYS[12].0 < 0 && KEYS[35].0 > 0 && KEYS[124].0 == 100); // Q left, P right, right arrow at the edge
    }

    #[test]
    fn media_keys_sound_but_lock_and_repeats_do_not() {
        let event = |key: isize, state: isize, repeat: isize| key << 16 | state << 8 | repeat;
        assert_eq!(media_key(event(0, 0xA, 0)), Some((true, 75, 0))); // volume up pressed
        assert_eq!(media_key(event(3, 0xB, 0)), Some((false, -90, 0))); // brightness down released
        assert_eq!(media_key(event(16, 0xA, 1)), None); // play held down
        assert_eq!(media_key(event(6, 0xA, 0)), None); // power/lock
        assert_eq!(media_key(event(4, 0xA, 0)), None); // caps lock
    }
}
