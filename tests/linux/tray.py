"""A stand-in for a desktop tray: the StatusNotifierWatcher, plus the same menu client
library (libdbusmenu-glib) that Ubuntu's indicators, Xfce and Waybar use to read menus.
Usage: tray.py /path/to/keycaps   (which must already be running, on the same session bus)"""
import subprocess, sys
import dbus, dbus.service, dbus.mainloop.glib
import gi
gi.require_version("Dbusmenu", "0.4")
from gi.repository import Dbusmenu, GLib

keycaps = sys.argv[1]
dbus.mainloop.glib.DBusGMainLoop(set_as_default=True)
bus = dbus.SessionBus()
loop = GLib.MainLoop()
failures = []
def check(what, ok):
    print(("  ok    " if ok else "  FAIL  ") + what)
    if not ok: failures.append(what)
def status():
    return subprocess.run([keycaps, "status"], capture_output=True, text=True).stdout

class Watcher(dbus.service.Object):
    @dbus.service.method("org.kde.StatusNotifierWatcher", in_signature="s", sender_keyword="sender")
    def RegisterStatusNotifierItem(self, service, sender=None):
        print(f"registered: {sender} {service}")
        GLib.idle_add(inspect, str(sender), str(service))

name = dbus.service.BusName("org.kde.StatusNotifierWatcher", bus)
watcher = Watcher(bus, "/StatusNotifierWatcher")
state = {}

def tree(item, depth=0, out=None):
    out = [] if out is None else out
    kind = item.property_get("type") or ""
    toggle = item.property_get("toggle-type") or ""
    mark = {("checkmark", 1): "[x] ", ("checkmark", 0): "[ ] ", ("radio", 1): "(o) ", ("radio", 0): "( ) "}.get((toggle, item.property_get_int("toggle-state")), "")
    label = "--------" if kind == "separator" else mark + (item.property_get("label") or "")
    enabled = item.property_get_bool("enabled") if item.property_exist("enabled") else True
    out.append((depth, label + ("" if enabled else "   (greyed)"), item))
    for c in item.get_children(): tree(c, depth + 1, out)
    return out

def by_label(label):
    for _, text, item in tree(state["client"].get_root()):
        if text.replace("[x] ", "").replace("[ ] ", "").replace("(o) ", "").replace("( ) ", "") == label: return item

def inspect(sender, path):
    props = dbus.Interface(bus.get_object(sender, path), "org.freedesktop.DBus.Properties").GetAll("org.kde.StatusNotifierItem")
    print("icon properties:", {k: (str(v) if not isinstance(v, (dbus.Array, dbus.Struct)) else [str(x) for x in v]) for k, v in props.items() if k in ("Id", "Title", "Status", "Category", "IconName", "Menu", "ItemIsMenu", "ToolTip")})
    check("the icon names a menu and an icon", str(props["Menu"]) == "/MenuBar" and str(props["IconName"]) != "" and bool(props["ItemIsMenu"]))
    one = dbus.Interface(bus.get_object(sender, path), "org.freedesktop.DBus.Properties").Get("org.kde.StatusNotifierItem", "Title")
    check("a single property can be read", str(one) == "Keycaps")
    xml = dbus.Interface(bus.get_object(sender, "/MenuBar"), "org.freedesktop.DBus.Introspectable").Introspect()
    check("introspection parses and lists the menu interface", "com.canonical.dbusmenu" in xml and "GetLayout" in xml)
    state["client"] = Dbusmenu.Client.new(sender, str(props["Menu"]))
    state["client"].connect("layout-updated", lambda c: GLib.idle_add(step))
    GLib.timeout_add(4000, lambda: (failures.append("timed out"), loop.quit()))
    return False

steps = iter(range(100))
def step():
    n = next(steps)
    root = state["client"].get_root()
    if root is None: return False
    if n == 0:
        lines = tree(root)
        print("the menu as the client library sees it:")
        for depth, text, _ in lines[1:]: print("   " + "    " * (depth - 1) + text)
        check("the menu has its 12 top lines", len(root.get_children()) == 12)
        check("NovelKeys Cream is the chosen switch", by_label("NovelKeys Cream").property_get_int("toggle-state") == 1)
        by_label("Holy Panda").handle_event("clicked", GLib.Variant("i", 0), 0)
    elif n == 1:
        check("clicking Holy Panda moved the mark", by_label("Holy Panda").property_get_int("toggle-state") == 1 and by_label("NovelKeys Cream").property_get_int("toggle-state") == 0)
        check("and the app agrees", "switch holypanda" in status())
        by_label("Enable Keycaps").handle_event("clicked", GLib.Variant("i", 0), 0)
    elif n == 2:
        check("clicking Enable Keycaps switched it off", by_label("Enable Keycaps").property_get_int("toggle-state") == 0 and "Keycaps is off" in status())
        by_label("80%").handle_event("clicked", GLib.Variant("i", 0), 0)       # the first 80% is the keyboard's
    elif n == 3:
        check("keyboard volume is 80", "\nvolume 80\n" in status())
        by_label("Wide").handle_event("clicked", GLib.Variant("i", 0), 0)
    elif n == 4:
        check("spatial is wide", "spatial wide" in status())
        by_label("Topre").handle_event("hovered", GLib.Variant("i", 0), 0)     # a preview changes nothing
        by_label("Topre").handle_event("closed", GLib.Variant("i", 0), 0)
        subprocess.run([keycaps, "mouse", "razer-mamba-elite"])                 # a command from outside shows in the menu
    elif n == 5:
        check("a command from the shell reached the menu", by_label("Razer Mamba Elite").property_get_int("toggle-state") == 1 and "switch holypanda" in status())
        by_label("Quit Keycaps").handle_event("clicked", GLib.Variant("i", 0), 0)
        GLib.timeout_add(500, lambda: (check("Quit Keycaps ended the app", subprocess.run([keycaps, "status"], capture_output=True).returncode != 0), loop.quit()))
    return False

GLib.timeout_add(3000, lambda: (None if state else (failures.append("no icon registered"), loop.quit())) and False)
loop.run()
print("FAILED: " + "; ".join(failures) if failures else "tray protocol: all checks passed")
sys.exit(1 if failures else 0)
