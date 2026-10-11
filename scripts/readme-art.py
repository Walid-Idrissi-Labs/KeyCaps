#!/usr/bin/env python3
"""Redraws the README artwork in assets/readme. Run it from the project root:

    python3 scripts/readme-art.py

Every colour is in COLOURS, as a light and a dark value; the menu rows are in main_rows
and sub_rows and should match build_menu in src/main.rs.
"""
import math
import os
from xml.sax.saxutils import escape

out = "assets/readme"
os.makedirs(out, exist_ok=True)

SANS = '-apple-system,BlinkMacSystemFont,"SF Pro Display","SF Pro Text","Segoe UI","Helvetica Neue",Arial,sans-serif'
SERIF = 'ui-serif,"Iowan Old Style","Palatino Linotype",Palatino,Georgia,"Times New Roman",serif'

# Light and dark values of every colour the drawings use.
COLOURS = {
    "ground-a": ("#f6f6f4", "#202022"),
    "ground-b": ("#e9e9e6", "#161618"),
    "edge": ("#1c1c1e14", "#ffffff14"),
    "ink": ("#1c1c1e", "#f5f5f7"),
    "soft": ("#5e5e62", "#a1a1a6"),
    "faint": ("#8e8e93", "#77777c"),
    "accent": ("#d4621a", "#ff8f45"),
    "pill": ("#ffffff", "#2a2a2d"),
    "case": ("#d2d2ce", "#0f0f11"),
    "side": ("#8f8f8c", "#09090a"),
    "top-a": ("#fdfdfc", "#48484c"),
    "top-b": ("#dcdcd8", "#2f2f32"),
    "legend": ("#5a5a5e", "#c4c4c9"),
    "hot-side": ("#9a4410", "#b3561a"),
    "hot-a": ("#f08a3c", "#ffa35c"),
    "hot-b": ("#d4621a", "#f07f2e"),
    "hot-legend": ("#ffffff", "#1c1c1e"),
    "menu": ("#f4f4f2", "#2b2b2e"),
    "menu-edge": ("#1c1c1e26", "#ffffff2e"),
    "bar": ("#dededa", "#0f0f11"),
    "select": ("#6e6e73", "#8e8e93"),
    "on-select": ("#ffffff", "#111113"),
    "rule": ("#1c1c1e1f", "#ffffff24"),
    "left": ("#1c1c1e", "#f5f5f7"),
    "right": ("#d4621a", "#ff8f45"),
}


def style(extra=""):
    """CSS custom properties for both schemes, and the shared text rules."""
    light = "".join(f"--{k}:{v[0]};" for k, v in COLOURS.items())
    dark = "".join(f"--{k}:{v[1]};" for k, v in COLOURS.items())
    return (
        f"<style>svg{{{light}}}@media (prefers-color-scheme:dark){{svg{{{dark}}}}}"
        f"text{{font-family:{SANS}}}.serif{{font-family:{SERIF};font-style:italic}}"
        ".ink{fill:var(--ink)}.soft{fill:var(--soft)}.faint{fill:var(--faint)}.accent{fill:var(--accent)}"
        f"{extra}</style>"
    )


def gradient(name, a, b):
    return (
        f'<linearGradient id="{name}" x1="0" y1="0" x2="0" y2="1">'
        f'<stop offset="0" style="stop-color:var(--{a})"/><stop offset="1" style="stop-color:var(--{b})"/></linearGradient>'
    )


def text(x, y, s, size, cls="ink", weight=400, anchor="middle", extra=""):
    return f'<text x="{x:g}" y="{y:g}" font-size="{size:g}" font-weight="{weight}" text-anchor="{anchor}" class="{cls}"{extra}>{escape(s)}</text>'


def svg(name, w, h, title, body, extra_style="", defs=""):
    doc = (
        f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {w:g} {h:g}" width="{w:g}" height="{h:g}" role="img" aria-label="{escape(title)}">'
        f"<title>{escape(title)}</title>{style(extra_style)}<defs>{defs}</defs>{body}</svg>\n"
    )
    with open(f"{out}/{name}", "w", encoding="utf-8") as f:
        f.write(doc)
    print(f"{name}: {len(doc)} bytes")


KEY_STYLE = ".side{fill:var(--side)}.top{fill:url(#top)}.legend{fill:var(--legend)}.hot .side{fill:var(--hot-side)}.hot .top{fill:url(#hot)}.hot .legend{fill:var(--hot-legend)}"
KEY_DEFS = gradient("top", "top-a", "top-b") + gradient("hot", "hot-a", "hot-b")


def key(x, y, w, h, legend="", hot=False, size=None):
    """A keycap: a darker body with a lighter, slightly raised top face."""
    size = size or (h * 0.4 if len(legend) <= 1 else h * 0.25)
    lip = h * 0.14
    hot_class = ' class="hot"' if hot else ""
    label = text(x + w / 2, y + (h - lip) / 2 + size * 0.36, legend, size, "legend", 600) if legend else ""
    return (
        f'<g{hot_class}><rect x="{x:g}" y="{y:g}" width="{w:g}" height="{h:g}" rx="{h * 0.17:g}" class="side"/>'
        f'<rect x="{x + h * 0.06:g}" y="{y + h * 0.03:g}" width="{w - h * 0.12:g}" height="{h - lip:g}" rx="{h * 0.13:g}" class="top"/>{label}</g>'
    )


# ---------------------------------------------------------------- hero
ROWS = [
    [("`", 1), ("1", 1), ("2", 1), ("3", 1), ("4", 1), ("5", 1), ("6", 1), ("7", 1), ("8", 1), ("9", 1), ("0", 1), ("-", 1), ("=", 1), ("delete", 2)],
    [("tab", 1.5), ("Q", 1), ("W", 1), ("E", 1), ("R", 1), ("T", 1), ("Y", 1), ("U", 1), ("I", 1), ("O", 1), ("P", 1), ("[", 1), ("]", 1), ("\\", 1.5)],
    [("caps", 1.75), ("A", 1), ("S", 1), ("D", 1), ("F", 1), ("G", 1), ("H", 1), ("J", 1), ("K", 1), ("L", 1), (";", 1), ("'", 1), ("return", 2.25)],
    [("shift", 2.25), ("Z", 1), ("X", 1), ("C", 1), ("V", 1), ("B", 1), ("N", 1), ("M", 1), (",", 1), (".", 1), ("/", 1), ("shift", 2.75)],
    [("fn", 1), ("⌃", 1), ("⌥", 1), ("⌘", 1.25), ("", 5.5), ("⌘", 1.25), ("⌥", 1), ("←", 1), ("↓", 1), ("→", 1)],
]
U, GAP = 52, 6
board_w = 15 * U - GAP
bx, by = (1200 - board_w) / 2, 372
keys = ""
for r, row in enumerate(ROWS):
    x = bx
    for legend, units in row:
        keys += key(x, by + r * U, units * U - GAP, U - GAP, legend, hot=legend == "K")
        x += units * U
board_h = 5 * U - GAP
hero = (
    '<rect width="1200" height="680" rx="32" fill="url(#ground)"/>'
    '<rect x="0.5" y="0.5" width="1199" height="679" rx="31.5" fill="none" style="stroke:var(--edge)"/>'
    '<rect x="402" y="44" width="396" height="38" rx="19" style="fill:var(--pill);stroke:var(--edge)"/>'
    '<circle cx="426" cy="63" r="5" class="accent"/>'
    + text(442, 68.5, "Real switch recordings · free and open source", 15, "soft", 600, "start", ' textLength="336" lengthAdjust="spacingAndGlyphs"')
    + text(600, 178, "Quiet keys.", 86, "ink", 700, extra=' letter-spacing="-2.5"')
    + f'<text x="600" y="270" font-size="92" text-anchor="middle" class="serif accent" letter-spacing="-1">Mechanical sound.</text>'
    + text(600, 326, "Thirteen recorded switches and seven mice, for macOS and Linux.", 23, "soft", 400)
    + f'<rect x="{bx - 18:g}" y="{by - 18:g}" width="{board_w + 36:g}" height="{board_h + 36:g}" rx="20" style="fill:var(--case)"/>'
    + keys
)
svg("hero.svg", 1200, 680, "Quiet keys. Mechanical sound. Thirteen recorded switches and seven mice, for macOS and Linux.", hero, KEY_STYLE, gradient("ground", "ground-a", "ground-b") + KEY_DEFS)

# ---------------------------------------------------------------- buttons
ARROW = '<path d="M0 -9V5M-6 0L0 6L6 0M-8 10H8" fill="none" style="stroke:var(--on)" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round"/>'
for name, label in [("download.svg", "Download for macOS"), ("linux.svg", "Download for Linux")]:
    svg(
        name, 264, 56, label,
        '<rect x="1" y="1" width="262" height="54" rx="27" style="fill:var(--ink);stroke:var(--edge)"/>'
        f'<g transform="translate(42 27)">{ARROW}</g>'
        + text(66, 34, label, 18, "on", 650, "start", ' textLength="166" lengthAdjust="spacingAndGlyphs"'),
        "svg{--on:#ffffff}@media (prefers-color-scheme:dark){svg{--on:#1c1c1e}}.on{fill:var(--on)}",
    )
svg(
    "source.svg", 216, 56, "Build from source",
    '<rect x="1" y="1" width="214" height="54" rx="27" style="fill:var(--pill);stroke:var(--menu-edge)"/>'
    + text(108, 34, "Build from source", 18, "ink", 600, "middle", ' textLength="148" lengthAdjust="spacingAndGlyphs"'),
)

# ---------------------------------------------------------------- menu
ROW, PAD, FS = 24, 6, 13.5


def menu(x, y, w, rows):
    """A macOS-style menu. Rows: None = separator, else (title, kind, right) with kind in
    '', 'check', 'head' (group heading), 'info' (greyed line), 'select' (highlighted)."""
    h = PAD * 2 + sum(11 if r is None else ROW for r in rows)
    s = f'<rect x="{x:g}" y="{y:g}" width="{w:g}" height="{h:g}" rx="9" style="fill:var(--menu);stroke:var(--menu-edge)"/>'
    cy, anchors = y + PAD, {}
    for r in rows:
        if r is None:
            s += f'<rect x="{x + 12:g}" y="{cy + 5:g}" width="{w - 24:g}" height="1" style="fill:var(--rule)"/>'
            cy += 11
            continue
        title, kind, right = r
        anchors[title] = cy
        cls, weight = "ink", 400
        if kind == "select":
            s += f'<rect x="{x + 5:g}" y="{cy + 1:g}" width="{w - 10:g}" height="{ROW - 2:g}" rx="5" style="fill:var(--select)"/>'
            cls = "on-select"
        elif kind == "head":
            cls, weight = "faint", 600
        elif kind == "info":
            cls = "faint"
        base = cy + ROW / 2 + FS * 0.35
        if kind == "check":
            s += text(x + 14, base, "✓", FS, cls, 600)
        s += text(x + (12 if kind == "head" else 26), base, title, FS if kind != "head" else FS - 1.5, cls, weight, "start")
        if right:
            s += text(x + w - 14, base, right, FS, "on-select" if kind == "select" else "faint", 400, "end")
        cy += ROW
    return s, anchors, h


main_rows = [
    ("NovelKeys Cream · 1284 keys today", "info", ""),
    None,
    ("Enable Keycaps", "check", "⇧⌘K"),
    ("Keyboard Volume", "", "›"),
    ("Mouse Volume", "", "›"),
    ("Switch Keys", "select", "›"),
    ("Mouse Clicks", "", "›"),
    ("Spatial Audio", "", "›"),
    ("Key-up sounds", "check", ""),
    ("Scroll ticks", "", ""),
    None,
    ("Quit Keycaps", "", "⌘Q"),
]
sub_rows = [
    ("Linear", "head", ""), ("Alpaca", "", ""), ("Gateron Black Ink", "", ""), ("Gateron Red Ink", "", ""),
    ("NovelKeys Cream", "check", ""), ("Cherry MX Black", "", ""), ("Turquoise Tealios", "", ""), None,
    ("Tactile", "head", ""), ("Cherry MX Brown", "", ""), ("Holy Panda", "select", "playing"), ("Topre", "", ""), None,
    ("Clicky", "head", ""), ("Alps SKCM Blue", "", ""), ("Cherry MX Blue", "", ""), ("Kailh Box Navy", "", ""), ("Buckling Spring", "", ""), None,
    ("Synthetic", "head", ""), ("Synthetic Linear", "", ""), ("Synthetic Tactile", "", ""), ("Synthetic Clicky", "", ""),
]
MX, MY, MW, SW = 24, 44, 268, 214
main_svg, anchors, main_h = menu(MX, MY, MW, main_rows)
sub_y = anchors["Switch Keys"] - PAD
sub_svg, _, sub_h = menu(MX + MW - 4, sub_y, SW, sub_rows)
total_w, total_h = MX + MW - 4 + SW + 24, sub_y + sub_h + 24
# The menu bar glyph is the app's own template image (assets/icons/industrial/menubar-titanium-k).
GLYPH = (
    '<g fill="none" style="stroke:var(--on-select)" stroke-width="1.25" stroke-linecap="round" stroke-linejoin="round">'
    '<path d="M5 2.25H13Q15 2.25 15.4 4.4L16.5 13Q16.8 15.75 14 15.75H4Q1.2 15.75 1.5 13L2.6 4.4Q3 2.25 5 2.25Z"/>'
    '<path d="M2.35 12.7Q9 14.2 15.65 12.7"/><path d="M6.9 5V10M11.1 5L6.9 7.6M8.6 6.55L11.1 10"/></g>'
)
menu_body = (
    f'<rect width="{total_w:g}" height="{total_h:g}" rx="18" fill="url(#ground)"/>'
    f'<rect x="0.5" y="0.5" width="{total_w - 1:g}" height="{total_h - 1:g}" rx="17.5" fill="none" style="stroke:var(--edge)"/>'
    f'<path d="M0 18Q0 0 18 0H{total_w - 18:g}Q{total_w:g} 0 {total_w:g} 18V32H0Z" style="fill:var(--bar)"/>'
    f'<rect x="{MX + 2:g}" y="4" width="34" height="24" rx="6" style="fill:var(--select)"/>'
    f'<g transform="translate({MX + 10:g} 7)">{GLYPH}</g>'
    + main_svg + sub_svg
)
svg("menu.svg", total_w, total_h, "The Keycaps menu, with the Switch Keys list open and Holy Panda being previewed", menu_body,
    ".on-select{fill:var(--on-select)}", gradient("ground", "ground-a", "ground-b"))

# ---------------------------------------------------------------- spatial
# The home row at the app's "Natural" width (0.9), with its equal-power pan law:
# left = sqrt(1 - p), right = sqrt(p), p running 0..1 across the speakers.
letters = ["A", "S", "D", "F", "G", "H", "J", "K", "L", ";", "'"]
W, H, K = 760, 290, 50
x0 = (W - (len(letters) * (K + 8) - 8)) / 2
body = (
    f'<rect width="{W}" height="{H}" rx="18" fill="url(#ground)"/>'
    f'<rect x="0.5" y="0.5" width="{W - 1}" height="{H - 1}" rx="17.5" fill="none" style="stroke:var(--edge)"/>'
)
BASE, BAR_MAX, KEY_Y = 172, 124, 188
for i, letter in enumerate(letters):
    column = i + 1  # caps lock is column 0 of the 13 keys in this row
    pan = (column * 200 // 12 - 100) / 100 * 0.9
    p = (pan + 1) / 2
    left, right = math.sqrt(1 - p), math.sqrt(p)
    x = x0 + i * (K + 8)
    body += key(x, KEY_Y, K, K, letter)
    body += f'<rect x="{x + 7:g}" y="{BASE - left * BAR_MAX:g}" width="15" height="{left * BAR_MAX:g}" rx="4" style="fill:var(--left)"/>'
    body += f'<rect x="{x + K - 22:g}" y="{BASE - right * BAR_MAX:g}" width="15" height="{right * BAR_MAX:g}" rx="4" style="fill:var(--right)"/>'
legend_y = H - 24
body += (
    f'<rect x="{x0:g}" y="{legend_y - 11:g}" width="12" height="12" rx="3" style="fill:var(--left)"/>'
    + text(x0 + 19, legend_y, "left speaker", 14, "soft", 500, "start")
    + f'<rect x="{x0 + 122:g}" y="{legend_y - 11:g}" width="12" height="12" rx="3" style="fill:var(--right)"/>'
    + text(x0 + 141, legend_y, "right speaker", 14, "soft", 500, "start")
    + text(W - x0, legend_y, "Spatial Audio: Natural", 14, "faint", 500, "end")
)
svg("spatial.svg", W, H, "How loud each key of the home row is in the left and the right speaker", body, KEY_STYLE,
    gradient("ground", "ground-a", "ground-b") + KEY_DEFS)

# ---------------------------------------------------------------- stats
STATS = [
    ("13", "recorded switches"), ("7", "mice, button by button"), ("0%", "CPU while you rest"), ("0", "key presses stored or sent"),
    ("2 ms", "from sample start to the hit"), ("±3%", "pitch drift, stroke to stroke"), ("1.4 MB", "to download on Linux, 3.4 on macOS"), ("0", "dependencies"),
]
CW, CH, CG = 282, 126, 24
cards = ""
for i, (figure, label) in enumerate(STATS):
    x, y = (i % 4) * (CW + CG), (i // 4) * (CH + CG)
    cards += (
        f'<rect x="{x + 0.5:g}" y="{y + 0.5:g}" width="{CW - 1:g}" height="{CH - 1:g}" rx="18" fill="url(#ground)" style="stroke:var(--edge)"/>'
        + text(x + 26, y + 64, figure, 44, "ink", 700, "start", ' letter-spacing="-1"')
        + text(x + 26, y + 96, label, 17, "soft", 400, "start")
    )
svg("stats.svg", 4 * CW + 3 * CG, 2 * CH + CG, "; ".join(f"{figure} {label}" for figure, label in STATS), cards, "", gradient("ground", "ground-a", "ground-b"))
