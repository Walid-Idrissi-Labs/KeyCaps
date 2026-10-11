//! The tray icon and its menu on Linux, spoken over D-Bus by hand: a StatusNotifierItem
//! (shown by KDE, by GNOME with the AppIndicator extension, Xfce, Cinnamon, Waybar and
//! others) whose menu is a com.canonical.dbusmenu object. Whoever hosts the tray draws
//! both; this side only answers questions about them and is told what was clicked.

use super::{State, command, ears, preview};
use crate::WIDTH;
use crate::audio::{GROUPS, MICE, PACKS};
use std::io::{Read, Result, Write};
use std::os::fd::{AsRawFd, RawFd};
use std::os::linux::net::SocketAddrExt;
use std::os::unix::net::{SocketAddr, UnixStream};
use std::time::Duration;

const ITEM: &str = "org.kde.StatusNotifierItem";
const MENU: &str = "com.canonical.dbusmenu";
const WATCHER: &str = "org.kde.StatusNotifierWatcher";
const PROPERTIES: &str = "org.freedesktop.DBus.Properties";
const ITEM_PATH: &str = "/StatusNotifierItem";
const MENU_PATH: &str = "/MenuBar";

// Menu item ids are `kind * 1000 + index`, for the items that do something.
const T_ON: i32 = 1;
const T_VOLUME: i32 = 2;
const T_KEY: i32 = 3;
const T_MOUSE: i32 = 4;
const T_SCROLL: i32 = 5;
const T_KEYUP: i32 = 6;
const T_QUIT: i32 = 7;
const T_SPATIAL: i32 = 10;
const T_MVOLUME: i32 = 11;

/// Values in D-Bus's wire format: each aligned to its size from the start of the message
/// body, which itself starts on a multiple of 8.
#[derive(Default)]
struct Body(Vec<u8>);

impl Body {
    fn pad(&mut self, to: usize) {
        self.0.resize(self.0.len().next_multiple_of(to), 0);
    }
    fn u32(&mut self, v: u32) {
        self.pad(4);
        self.0.extend(v.to_le_bytes());
    }
    /// Strings and object paths: length, text, a zero.
    fn string(&mut self, v: &str) {
        self.u32(v.len() as u32);
        self.0.extend(v.as_bytes());
        self.0.push(0);
    }
    fn signature(&mut self, v: &str) {
        self.0.push(v.len() as u8);
        self.0.extend(v.as_bytes());
        self.0.push(0);
    }
    /// An array whose elements align to `align`: its length in bytes, then what `fill` writes.
    fn array(&mut self, align: usize, fill: impl FnOnce(&mut Body)) {
        self.u32(0);
        let length = self.0.len() - 4;
        self.pad(align);
        let start = self.0.len();
        fill(self);
        let bytes = (self.0.len() - start) as u32;
        self.0[length..length + 4].copy_from_slice(&bytes.to_le_bytes());
    }
    /// One entry of a dictionary of variants: a name and a value with its type.
    fn entry(&mut self, name: &str, value: Value) {
        self.pad(8);
        self.string(name);
        self.variant(value);
    }
    fn variant(&mut self, value: Value) {
        match value {
            Value::Text(v) => {
                self.signature("s");
                self.string(v);
            }
            Value::Path(v) => {
                self.signature("o");
                self.string(v);
            }
            Value::Flag(v) => {
                self.signature("b");
                self.u32(v as u32);
            }
            Value::Number(v) => {
                self.signature("i");
                self.u32(v as u32);
            }
            Value::Unsigned(v) => {
                self.signature("u");
                self.u32(v);
            }
            Value::Nothing(signature) => {
                self.signature(signature);
                self.array(if signature.starts_with("a(") { 8 } else { 4 }, |_| {}); // structures align to 8, strings to 4
            }
            Value::ToolTip(title, text) => {
                self.signature("(sa(iiay)ss)");
                self.pad(8);
                self.string("");
                self.array(8, |_| {});
                self.string(title);
                self.string(text);
            }
        }
    }
}

enum Value<'a> {
    Text(&'a str),
    Path(&'a str),
    Flag(bool),
    Number(i32),
    Unsigned(u32),
    Nothing(&'a str),           // an empty array of the given type
    ToolTip(&'a str, &'a str), // no icon, a title and a line of text
}

/// Reads values in D-Bus's wire format. None means the message ended early or is malformed.
struct Reader<'a> {
    data: &'a [u8],
    at: usize,
}

impl<'a> Reader<'a> {
    fn pad(&mut self, to: usize) {
        self.at = self.at.next_multiple_of(to);
    }
    fn bytes(&mut self, n: usize) -> Option<&'a [u8]> {
        let bytes = self.data.get(self.at..self.at.checked_add(n)?)?;
        self.at += n;
        Some(bytes)
    }
    fn u32(&mut self) -> Option<u32> {
        self.pad(4);
        Some(u32::from_le_bytes(self.bytes(4)?.try_into().ok()?))
    }
    fn string(&mut self) -> Option<&'a str> {
        let length = self.u32()? as usize;
        let text = std::str::from_utf8(self.bytes(length)?).ok()?;
        self.bytes(1)?;
        Some(text)
    }
    fn signature(&mut self) -> Option<&'a str> {
        let length = self.bytes(1)?[0] as usize;
        let text = std::str::from_utf8(self.bytes(length)?).ok()?;
        self.bytes(1)?;
        Some(text)
    }
    /// Skips one value of the first type in `signature` and returns the types after it.
    fn skip<'s>(&mut self, signature: &'s str) -> Option<&'s str> {
        let (kind, rest) = (*signature.as_bytes().first()?, &signature[1..]);
        match kind {
            b'y' => drop(self.bytes(1)?),
            b'n' | b'q' => {
                self.pad(2);
                self.bytes(2)?;
            }
            b'b' | b'i' | b'u' | b'h' => drop(self.u32()?),
            b'x' | b't' | b'd' => {
                self.pad(8);
                self.bytes(8)?;
            }
            b's' | b'o' => drop(self.string()?),
            b'g' => drop(self.signature()?),
            b'v' => {
                let inner = self.signature()?;
                self.skip(inner)?;
            }
            b'a' => {
                // The length is in bytes, so the elements need no parsing, only their alignment.
                let length = self.u32()? as usize;
                self.pad(match *rest.as_bytes().first()? {
                    b'y' | b'g' | b'v' => 1,
                    b'n' | b'q' => 2,
                    b'x' | b't' | b'd' | b'(' | b'{' => 8,
                    _ => 4,
                });
                self.bytes(length)?;
                return rest.get(one_type(rest)?..);
            }
            b'(' | b'{' => {
                self.pad(8);
                let mut inside = rest;
                while !inside.starts_with([')', '}']) {
                    inside = self.skip(inside)?;
                }
                return inside.get(1..);
            }
            _ => return None,
        }
        Some(rest)
    }
    /// Skips a value of every type in `signature`.
    fn skip_all(&mut self, mut signature: &str) -> Option<()> {
        while !signature.is_empty() {
            signature = self.skip(signature)?;
        }
        Some(())
    }
}

/// The length of the first complete type in a signature.
fn one_type(signature: &str) -> Option<usize> {
    match *signature.as_bytes().first()? {
        b'a' => Some(1 + one_type(&signature[1..])?),
        open @ (b'(' | b'{') => {
            let mut depth = 0;
            for (i, c) in signature.bytes().enumerate() {
                depth += (c == open) as i32 - (c == open + 1 || c == open + 2) as i32; // ')' follows '(' and '}' is two after '{'
                if depth == 0 {
                    return Some(i + 1);
                }
            }
            None
        }
        _ => Some(1),
    }
}

/// What a message is about. Empty strings and zero stand for "not there".
#[derive(Default)]
struct Head<'a> {
    kind: u8, // 1 call, 2 reply, 3 error, 4 signal
    no_reply: bool,
    serial: u32,
    path: &'a str,
    interface: &'a str,
    member: &'a str,
    error: &'a str,
    reply_to: u32,
    other: &'a str, // who it goes to, or who it came from
}

/// One line of the menu.
struct Item {
    id: i32,
    label: String,
    kind: Kind,
    children: Vec<Item>,
}

#[derive(PartialEq)]
enum Kind {
    Plain,
    Label, // greyed out
    Separator,
    Check(bool),
    Radio(bool),
}

/// The whole menu as it is now, under an invisible root. It mirrors the macOS menu.
fn menu(s: &State) -> Item {
    let mut next = 20000; // ids of the lines that only sit there
    let mut line = |label: &str, kind: Kind, children: Vec<Item>| {
        next += 1;
        Item { id: next, label: label.into(), kind, children }
    };
    let act = |tag: i32, label: &str, kind: Kind| Item { id: tag, label: label.into(), kind, children: Vec::new() };
    let volume = |tag: i32, chosen: usize| [20, 40, 60, 80, 100].map(|pct| act(tag * 1000 + pct, &format!("{pct}%"), Kind::Radio(chosen == pct as usize))).into();

    let mut keys = Vec::new();
    let mut i = 0;
    for (title, count) in GROUPS {
        if i > 0 {
            keys.push(line("", Kind::Separator, Vec::new()));
        }
        keys.push(line(title, Kind::Label, Vec::new()));
        for (name, _) in &PACKS[i..i + count] {
            keys.push(act(T_KEY * 1000 + i as i32, name, Kind::Radio(s.key == i)));
            i += 1;
        }
    }
    let mut mice = vec![act(T_MOUSE * 1000, "Off", Kind::Radio(s.mouse == 0))];
    mice.extend(MICE.iter().enumerate().map(|(i, m)| act(T_MOUSE * 1000 + i as i32 + 1, m.0, Kind::Radio(s.mouse == i + 1))));
    let spatial = WIDTH.iter().enumerate().map(|(i, w)| act(T_SPATIAL * 1000 + i as i32, w.0, Kind::Radio(s.spatial == i))).collect();

    let mut lines = vec![line(&format!("{} · {} keys today", PACKS[s.key].0, s.stats.n), Kind::Label, Vec::new())];
    if s.deaf {
        lines.extend(ears(s).lines().map(|l| line(l, Kind::Label, Vec::new())));
    }
    lines.extend([
        line("", Kind::Separator, Vec::new()),
        act(T_ON * 1000, "Enable Keycaps", Kind::Check(s.on)),
        line("Keyboard Volume", Kind::Plain, volume(T_VOLUME, s.vol)),
        line("Mouse Volume", Kind::Plain, volume(T_MVOLUME, s.mvol)),
        line("Switch Keys", Kind::Plain, keys),
        line("Mouse Clicks", Kind::Plain, mice),
        line("Spatial Audio", Kind::Plain, spatial),
        act(T_KEYUP * 1000, "Key-up sounds", Kind::Check(s.keyup)),
        act(T_SCROLL * 1000, "Scroll ticks", Kind::Check(s.scroll)),
        line("", Kind::Separator, Vec::new()),
        act(T_QUIT * 1000, "Quit Keycaps", Kind::Plain),
    ]);
    Item { id: 0, label: String::new(), kind: Kind::Plain, children: lines }
}

fn find(item: &Item, id: i32) -> Option<&Item> {
    if item.id == id { Some(item) } else { item.children.iter().find_map(|c| find(c, id)) }
}

/// An item's properties, as the dictionary the menu protocol wants.
fn properties(b: &mut Body, item: &Item) {
    b.array(8, |b| {
        if item.kind == Kind::Separator {
            return b.entry("type", Value::Text("separator"));
        }
        if item.id != 0 {
            b.entry("label", Value::Text(&item.label.replace('_', "__"))); // a single _ would mark a shortcut letter
        }
        match item.kind {
            Kind::Label => b.entry("enabled", Value::Flag(false)),
            Kind::Check(on) | Kind::Radio(on) => {
                b.entry("toggle-type", Value::Text(if matches!(item.kind, Kind::Check(_)) { "checkmark" } else { "radio" }));
                b.entry("toggle-state", Value::Number(on as i32));
            }
            _ => {}
        }
        if !item.children.is_empty() {
            b.entry("children-display", Value::Text("submenu"));
        }
    });
}

/// An item with its children, `depth` levels down (negative: all of them): (ia{sv}av).
fn layout(b: &mut Body, item: &Item, depth: i32) {
    b.pad(8);
    b.u32(item.id as u32);
    properties(b, item);
    b.array(1, |b| {
        for child in item.children.iter().filter(|_| depth != 0) {
            b.signature("(ia{sv}av)");
            layout(b, child, depth - 1);
        }
    });
}

/// The command behind a menu item, as `keycaps <command>` would take it.
fn command_of(id: i32) -> Option<String> {
    let (tag, i) = (id / 1000, (id % 1000) as usize);
    Some(match tag {
        T_ON => "toggle".into(),
        T_VOLUME => format!("volume {i}"),
        T_MVOLUME => format!("mouse-volume {i}"),
        T_KEY => format!("switch {}", PACKS.get(i)?.1),
        T_MOUSE => format!("mouse {}", i.checked_sub(1).map_or(Some("off"), |m| MICE.get(m).map(|m| m.1))?),
        T_SPATIAL => format!("spatial {}", WIDTH.get(i)?.0),
        T_KEYUP => "keyup".into(),
        T_SCROLL => "scroll".into(),
        T_QUIT => "quit".into(),
        _ => return None,
    })
}

pub struct Tray {
    bus: UnixStream,
    input: Vec<u8>,
    serial: u32,
    revision: u32, // of the menu: raised whenever it may look different
}

impl Tray {
    /// Connects to the session bus and offers the icon to whoever shows the tray. None if
    /// there is no bus, as on a text console.
    pub fn connect() -> Option<Tray> {
        // shortcut: the first address only and no %-escapes, which is what every desktop sets; parse the rest if one ever does not.
        let address = std::env::var("DBUS_SESSION_BUS_ADDRESS").unwrap_or_else(|_| format!("unix:path={}/bus", std::env::var("XDG_RUNTIME_DIR").unwrap_or_default()));
        let place = |key: &str| address.split([';', ',']).find_map(|part| part.strip_prefix("unix:").unwrap_or(part).strip_prefix(key));
        let mut bus = match (place("path="), place("abstract=")) {
            (Some(path), _) => UnixStream::connect(path).ok()?,
            (_, Some(name)) => UnixStream::connect_addr(&SocketAddr::from_abstract_name(name).ok()?).ok()?,
            _ => return None,
        };
        // A bus that stops listening must not hang the app.
        bus.set_write_timeout(Some(Duration::from_secs(1))).ok()?;
        bus.set_read_timeout(Some(Duration::from_secs(1))).ok()?;
        // Who we are is the socket's own business: the bus only needs the user id in hex digits.
        let uid: String = unsafe { super::getuid() }.to_string().bytes().map(|b| format!("{b:02x}")).collect();
        bus.write_all(format!("\0AUTH EXTERNAL {uid}\r\n").as_bytes()).ok()?;
        let mut answer = [0u8; 64];
        let n = bus.read(&mut answer).ok()?;
        if !answer[..n].starts_with(b"OK ") {
            return None;
        }
        bus.write_all(b"BEGIN\r\n").ok()?;
        let mut tray = Tray { bus, input: Vec::new(), serial: 0, revision: 1 };
        let daemon = Head { kind: 1, path: "/org/freedesktop/DBus", interface: "org.freedesktop.DBus", other: "org.freedesktop.DBus", ..Head::default() };
        tray.send(Head { member: "Hello", ..daemon }, "", Body::default()).ok()?;
        // Trays come and go (a panel restarting, a desktop that starts after us): be told.
        let mut rule = Body::default();
        rule.string(&format!("type='signal',sender='org.freedesktop.DBus',member='NameOwnerChanged',arg0='{WATCHER}'"));
        tray.send(Head { member: "AddMatch", no_reply: true, ..daemon }, "s", rule).ok()?;
        tray.register().ok()?;
        Some(tray)
    }

    pub fn fd(&self) -> RawFd {
        self.bus.as_raw_fd()
    }

    /// Tells the tray's host that there is an icon at this connection. It asks for the rest.
    fn register(&mut self) -> Result<()> {
        let mut body = Body::default();
        body.string(ITEM_PATH);
        self.send(Head { kind: 1, no_reply: true, path: "/StatusNotifierWatcher", interface: WATCHER, member: "RegisterStatusNotifierItem", other: WATCHER, ..Head::default() }, "s", body)
    }

    /// Says that the menu and the tooltip may have changed, so that they are asked for again.
    pub fn changed(&mut self) -> Result<()> {
        self.revision += 1;
        let mut body = Body::default();
        body.u32(self.revision);
        body.u32(0); // from the root down
        self.send(Head { kind: 4, path: MENU_PATH, interface: MENU, member: "LayoutUpdated", ..Head::default() }, "ui", body)?;
        self.send(Head { kind: 4, path: ITEM_PATH, interface: ITEM, member: "NewToolTip", ..Head::default() }, "", Body::default())
    }

    fn send(&mut self, head: Head, signature: &str, body: Body) -> Result<()> {
        self.serial += 1;
        let mut m = Body(vec![b'l', head.kind, head.no_reply as u8, 1]);
        m.u32(body.0.len() as u32);
        m.u32(self.serial);
        m.array(8, |m| {
            let texts = [(1, "o", head.path), (2, "s", head.interface), (3, "s", head.member), (4, "s", head.error), (6, "s", head.other)];
            for (code, kind, text) in texts.into_iter().filter(|t| !t.2.is_empty()) {
                m.pad(8);
                m.0.push(code);
                m.signature(kind);
                m.string(text);
            }
            if head.reply_to != 0 {
                m.pad(8);
                m.0.push(5);
                m.variant(Value::Unsigned(head.reply_to));
            }
            if !signature.is_empty() {
                m.pad(8);
                m.0.push(8);
                m.signature("g");
                m.signature(signature);
            }
        });
        m.pad(8);
        m.0.extend(body.0);
        self.bus.write_all(&m.0)
    }

    /// Reads what has arrived and answers it. False once the bus is gone.
    pub fn serve(&mut self, s: &mut State) -> bool {
        let mut chunk = [0u8; 4096];
        match self.bus.read(&mut chunk) {
            Ok(n) if n > 0 => self.input.extend_from_slice(&chunk[..n]),
            _ => return false,
        }
        // A message: 16 fixed bytes, the header fields, padding to a multiple of 8, the body.
        while self.input.len() >= 16 {
            let word = |at: usize| u32::from_le_bytes(self.input[at..at + 4].try_into().unwrap()) as usize;
            let (fields, body) = (16 + word(12), (16 + word(12)).next_multiple_of(8));
            let end = body + word(4);
            if self.input[0] != b'l' || end > 1 << 27 {
                return false; // not the byte order of this machine, or nonsense
            }
            if self.input.len() < end {
                break;
            }
            let message: Vec<u8> = self.input.drain(..end).collect();
            let mut head = Head { kind: message[1], no_reply: message[2] & 1 != 0, serial: word_of(&message, 8), ..Head::default() };
            let mut signature = "";
            let mut r = Reader { data: &message[..fields], at: 16 };
            while r.at < fields {
                r.pad(8);
                let (Some(&[code]), Some(kind)) = (r.bytes(1), r.signature()) else { break };
                match (code, kind) {
                    (1, "o") => head.path = r.string().unwrap_or(""),
                    (2, "s") => head.interface = r.string().unwrap_or(""),
                    (3, "s") => head.member = r.string().unwrap_or(""),
                    (7, "s") => head.other = r.string().unwrap_or(""),
                    (8, "g") => signature = r.signature().unwrap_or(""),
                    _ => {
                        if r.skip(kind).is_none() {
                            break;
                        }
                    }
                }
            }
            let mut arguments = Reader { data: &message[body..], at: 0 };
            let sent = match head.kind {
                1 => {
                    let answer = self.call(s, &head, signature, &mut arguments);
                    let to = Head { reply_to: head.serial, other: head.other, ..Head::default() };
                    match answer {
                        _ if head.no_reply => Ok(()),
                        Some((signature, body)) => self.send(Head { kind: 2, ..to }, signature, body),
                        None => {
                            let mut body = Body::default();
                            body.string("Keycaps does not know that one");
                            self.send(Head { kind: 3, error: "org.freedesktop.DBus.Error.UnknownMethod", ..to }, "s", body)
                        }
                    }
                }
                // A tray has appeared (its name has a new owner): offer the icon again.
                4 if head.member == "NameOwnerChanged" && arguments.skip_all("ss").is_some() && arguments.string().is_some_and(|owner| !owner.is_empty()) => self.register(),
                _ => Ok(()),
            };
            if sent.is_err() {
                return false;
            }
        }
        true
    }

    /// Answers one call: the signature and body of the reply, or None for "no such thing".
    fn call(&mut self, s: &mut State, head: &Head, signature: &str, r: &mut Reader) -> Option<(&'static str, Body)> {
        let mut b = Body::default();
        let root = menu(s);
        let ids = |r: &mut Reader| -> Option<Vec<i32>> {
            let count = r.u32()? as usize / 4;
            (0..count).map(|_| r.u32().map(|v| v as i32)).collect()
        };
        let reply = match (head.interface, head.member, signature) {
            (PROPERTIES, "GetAll", "s") => {
                let interface = r.string()?;
                b.array(8, |b| property_names(interface).iter().for_each(|name| b.entry(name, property(s, interface, name))));
                "a{sv}"
            }
            (PROPERTIES, "Get", "ss") => {
                let (interface, name) = (r.string()?, r.string()?);
                b.variant(property(s, interface, property_names(interface).iter().find(|n| **n == name)?));
                "v"
            }
            ("org.freedesktop.DBus.Introspectable", "Introspect", _) => {
                b.string(&introspection(head.path));
                "s"
            }
            ("org.freedesktop.DBus.Peer", "Ping", _) => "",
            // A click on the icon itself. The middle button switches the sound on and off.
            (ITEM, "SecondaryActivate", _) => {
                self.act(s, "toggle");
                ""
            }
            (ITEM, "Activate" | "ContextMenu" | "Scroll", _) => "",
            (MENU, "GetLayout", "iias") => {
                let (parent, depth) = (r.u32()? as i32, r.u32()? as i32);
                b.u32(self.revision);
                layout(&mut b, find(&root, parent)?, depth);
                "u(ia{sv}av)"
            }
            (MENU, "GetGroupProperties", "aias") => {
                let wanted = ids(r)?;
                b.array(8, |b| {
                    fn each<'a>(item: &'a Item, all: &mut Vec<&'a Item>) {
                        all.push(item);
                        item.children.iter().for_each(|c| each(c, all));
                    }
                    let mut all = Vec::new();
                    each(&root, &mut all);
                    for item in all.into_iter().filter(|i| wanted.is_empty() || wanted.contains(&i.id)) {
                        b.pad(8);
                        b.u32(item.id as u32);
                        properties(b, item);
                    }
                });
                "a(ia{sv})"
            }
            (MENU, "Event", "isvu") => {
                let (id, what) = (r.u32()? as i32, r.string()?);
                self.event(s, id, what);
                ""
            }
            (MENU, "EventGroup", "a(isvu)") => {
                let end = r.u32()? as usize + r.at.next_multiple_of(8);
                while r.at < end {
                    r.pad(8);
                    let (id, what) = (r.u32()? as i32, r.string()?);
                    r.skip_all("vu")?;
                    self.event(s, id, what);
                }
                b.array(4, |_| {});
                "ai"
            }
            // The count on the first line moves with every key, so the menu is always worth asking for again.
            (MENU, "AboutToShow", "i") => {
                b.u32((r.u32()? == 0) as u32);
                "b"
            }
            (MENU, "AboutToShowGroup", "ai") => {
                b.array(4, |b| b.u32(0));
                b.array(4, |_| {});
                "aiai"
            }
            _ => return None,
        };
        Some((reply, b))
    }

    /// Something happened to a menu item: a click carries out its command, pointing at a
    /// switch or a mouse plays it, and closing the menu ends that preview.
    fn event(&mut self, s: &mut State, id: i32, what: &str) {
        match (what, id / 1000, (id % 1000) as usize) {
            ("clicked", ..) => {
                if let Some(line) = command_of(id) {
                    self.act(s, &line);
                }
            }
            ("hovered", T_KEY, i) if i < PACKS.len() => preview(s, Some(i), None),
            ("hovered", T_MOUSE, i) if (1..=MICE.len()).contains(&i) => preview(s, None, Some(i - 1)),
            ("closed", ..) => preview(s, None, None),
            _ => {}
        }
    }

    fn act(&mut self, s: &mut State, line: &str) {
        if let Err(problem) = command(s, line) {
            eprintln!("keycaps: {problem}");
        }
        let _ = self.changed(); // a failure shows at the next read
    }
}

fn word_of(message: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(message[at..at + 4].try_into().unwrap())
}

fn property_names(interface: &str) -> &'static [&'static str] {
    match interface {
        ITEM => &["Category", "Id", "Title", "Status", "WindowId", "IconName", "IconThemePath", "OverlayIconName", "AttentionIconName", "AttentionMovieName", "IconPixmap", "OverlayIconPixmap", "AttentionIconPixmap", "ToolTip", "ItemIsMenu", "Menu"],
        MENU => &["Version", "TextDirection", "Status", "IconThemePath"],
        _ => &[],
    }
}

/// A property of the icon or of the menu.
fn property<'a>(s: &'a State, interface: &str, name: &str) -> Value<'a> {
    match name {
        "Status" if interface == MENU => Value::Text("normal"),
        "IconThemePath" if interface == MENU => Value::Nothing("as"),
        "Category" => Value::Text("ApplicationStatus"),
        "Id" => Value::Text("keycaps"),
        "Title" => Value::Text("Keycaps"),
        "Status" => Value::Text("Active"),
        "WindowId" => Value::Number(0),
        // A keyboard from the desktop's own icon theme, which draws it in the panel's colours.
        "IconName" => Value::Text("input-keyboard-symbolic"),
        "IconPixmap" | "OverlayIconPixmap" | "AttentionIconPixmap" => Value::Nothing("a(iiay)"),
        "ToolTip" => Value::ToolTip("Keycaps", if s.on { PACKS[s.key].0 } else { "Off" }),
        "ItemIsMenu" => Value::Flag(true),
        "Menu" => Value::Path(MENU_PATH),
        "Version" => Value::Unsigned(3),
        "TextDirection" => Value::Text("ltr"),
        _ => Value::Text(""),
    }
}

/// What the object at `path` offers, for tools that ask. Tray hosts know it already.
fn introspection(path: &str) -> String {
    let interface = |name: &str, body: &str| format!("<interface name=\"{name}\">{body}</interface>");
    let args = |types: &str, direction: &str| types.split(' ').filter(|t| !t.is_empty()).map(|t| format!("<arg type=\"{t}\" direction=\"{direction}\"/>")).collect::<String>();
    let method = |name: &str, takes: &str, gives: &str| format!("<method name=\"{name}\">{}{}</method>", args(takes, "in"), args(gives, "out"));
    let readable = |names: &[(&str, &str)]| names.iter().map(|(n, t)| format!("<property name=\"{n}\" type=\"{t}\" access=\"read\"/>")).collect::<String>();
    let own = match path {
        ITEM_PATH => interface(
            ITEM,
            &(readable(&[("Category", "s"), ("Id", "s"), ("Title", "s"), ("Status", "s"), ("WindowId", "i"), ("IconName", "s"), ("IconThemePath", "s"), ("OverlayIconName", "s"), ("AttentionIconName", "s"), ("AttentionMovieName", "s"), ("IconPixmap", "a(iiay)"), ("OverlayIconPixmap", "a(iiay)"), ("AttentionIconPixmap", "a(iiay)"), ("ToolTip", "(sa(iiay)ss)"), ("ItemIsMenu", "b"), ("Menu", "o")])
                + &["Activate", "SecondaryActivate", "ContextMenu"].map(|m| method(m, "i i", "")).concat()
                + &method("Scroll", "i s", "")
                + "<signal name=\"NewToolTip\"/>"),
        ),
        MENU_PATH => interface(
            MENU,
            &(readable(&[("Version", "u"), ("TextDirection", "s"), ("Status", "s"), ("IconThemePath", "as")])
                + &method("GetLayout", "i i as", "u (ia{sv}av)")
                + &method("GetGroupProperties", "ai as", "a(ia{sv})")
                + &method("Event", "i s v u", "")
                + &method("EventGroup", "a(isvu)", "ai")
                + &method("AboutToShow", "i", "b")
                + &method("AboutToShowGroup", "ai", "ai ai")
                + "<signal name=\"LayoutUpdated\"><arg type=\"u\"/><arg type=\"i\"/></signal>"),
        ),
        _ => "<node name=\"StatusNotifierItem\"/><node name=\"MenuBar\"/>".into(),
    };
    let common = interface(PROPERTIES, &(method("Get", "s s", "v") + &method("GetAll", "s", "a{sv}"))) + &interface("org.freedesktop.DBus.Introspectable", &method("Introspect", "", "s"));
    format!("<!DOCTYPE node PUBLIC \"-//freedesktop//DTD D-BUS Object Introspection 1.0//EN\" \"http://www.freedesktop.org/standards/dbus/1.0/introspect.dtd\"><node>{own}{common}</node>")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values_are_written_aligned_and_read_back() {
        // A string after a byte is padded to 4, a dictionary entry to 8, and an array
        // carries the length of its elements without the padding before them.
        let mut b = Body(vec![7]);
        b.string("ab");
        b.array(8, |b| b.entry("on", Value::Flag(true)));
        assert_eq!(b.0, [7, 0, 0, 0, 2, 0, 0, 0, b'a', b'b', 0, 0, 16, 0, 0, 0, 2, 0, 0, 0, b'o', b'n', 0, 1, b'b', 0, 0, 0, 1, 0, 0, 0]);
        // Skipping by signature lands exactly behind each value, however it is nested.
        let mut r = Reader { data: &b.0, at: 0 };
        assert_eq!(r.skip("ysa{sv}"), Some("sa{sv}"));
        assert_eq!((r.string(), r.skip("a{sv}"), r.at), (Some("ab"), Some(""), b.0.len()));
        assert_eq!((one_type("a(ia{sv}av)u"), one_type("{sv}s"), one_type("(i"), one_type("")), (Some(11), Some(4), None, None));
        assert!(Reader { data: &b.0[..20], at: 0 }.skip("ysa{sv}").and_then(|rest| Reader { data: &b.0[..20], at: 1 }.skip(rest)).is_some());
        assert!(Reader { data: &b.0[..20], at: 12 }.skip("a{sv}").is_none(), "an array longer than the message");
        let mut names = Body::default();
        ["name", "", ":1.7"].iter().for_each(|text| names.string(text));
        let mut r = Reader { data: &names.0, at: 0 };
        assert_eq!((r.skip_all("ss"), r.string()), (Some(()), Some(":1.7")));
    }

    #[test]
    fn every_menu_item_has_its_own_id_and_the_clickable_ones_a_command() {
        let s = super::super::tests::state();
        let root = menu(&s);
        fn each<'a>(item: &'a Item, all: &mut Vec<&'a Item>) {
            all.push(item);
            item.children.iter().for_each(|c| each(c, all));
        }
        let mut all = Vec::new();
        each(&root, &mut all);
        let mut ids: Vec<i32> = all.iter().map(|i| i.id).collect();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), all.len());
        // 1 info line, 3 separators and 4 group titles at their places, 9 submenu or action lines...
        let clickable: Vec<&&Item> = all.iter().filter(|i| i.id != 0 && i.children.is_empty() && !matches!(i.kind, Kind::Label | Kind::Separator)).collect();
        assert_eq!(clickable.len(), 1 + 5 + 5 + PACKS.len() + 1 + MICE.len() + WIDTH.len() + 2 + 1);
        for item in clickable {
            assert!(command_of(item.id).is_some(), "{} does nothing", item.label);
        }
        assert_eq!((command_of(T_KEY * 1000 + 7).as_deref(), command_of(T_MOUSE * 1000).as_deref(), command_of(T_MOUSE * 1000 + 99)), (Some("switch holypanda"), Some("mouse off"), None));
        // The whole menu as one reply, and one submenu a level deep.
        let (mut whole, mut shallow) = (Body::default(), Body::default());
        layout(&mut whole, &root, -1);
        layout(&mut shallow, &root, 1);
        let mut r = Reader { data: &whole.0, at: 0 };
        assert_eq!((r.skip("(ia{sv}av)"), r.at), (Some(""), whole.0.len()));
        assert!(shallow.0.len() < whole.0.len() / 3);
    }
}
