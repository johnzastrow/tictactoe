"""Web | terminal side by side for one language, one image per item.

    python compose.py rust|go

Reads <lang>/raw/web-NN-name.png (the whole browser window at 2x) and
<lang>/raw/tui-NN-name.png (the whole 1640x960 terminal frame), scales both to
one height and writes <lang>/NN-name.png. Whole screens rather than crops:
in demo each tab holds one control, so the full screen is the point.
"""
import os
import sys

from PIL import Image, ImageDraw, ImageFont

from steps import ITEMS

LANG = sys.argv[1]
H = 640          # height of each half, px
GAP, TOP, MARGIN = 24, 56, 24
BG, FG = (17, 21, 31), (230, 233, 240)
LABELS = {
    "rust": ("RUST · Web (Leptos → WASM)", "RUST · Terminal (ratatui)"),
    "go": ("GO · Web (Go → WASM, syscall/js)", "GO · Terminal (Bubble Tea)"),
}
LANG_COLOR = {"RUST": (247, 112, 51), "GO": (0, 173, 216)}


def font(size):
    for f in ("/System/Library/Fonts/SFNS.ttf", "/System/Library/Fonts/Helvetica.ttc",
              "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf"):
        if os.path.exists(f):
            return ImageFont.truetype(f, size)
    return ImageFont.load_default()


label_font, bold_font = font(26), font(28)


def label(d, xy, text):
    """Draw "LANG · rest" with the language name in its colour."""
    lang, _, rest = text.partition(" · ")
    d.text(xy, lang, fill=LANG_COLOR[lang], font=bold_font)
    x = xy[0] + d.textlength(lang + "  ", font=bold_font)
    d.text((x, xy[1]), rest, fill=FG, font=label_font)


def fit(im, h):
    return im.resize((round(im.width * h / im.height), h), Image.LANCZOS)


for name, *_ in ITEMS:
    web_path, tui_path = f"{LANG}/raw/web-{name}.png", f"{LANG}/raw/tui-{name}.png"
    if not (os.path.exists(web_path) and os.path.exists(tui_path)):
        print("skip (missing a still)", name)
        continue
    web = fit(Image.open(web_path).convert("RGB"), H)
    tui = fit(Image.open(tui_path).convert("RGB"), H)
    out = Image.new("RGB", (MARGIN * 2 + web.width + GAP + tui.width, TOP + H + MARGIN), BG)
    d = ImageDraw.Draw(out)
    left, right = LABELS[LANG]
    label(d, (MARGIN, 14), left)
    label(d, (MARGIN + web.width + GAP, 14), right)
    out.paste(web, (MARGIN, TOP))
    out.paste(tui, (MARGIN + web.width + GAP, TOP))
    out.save(f"{LANG}/{name}.png", optimize=True)
    print("ok", name, f"{out.width}x{out.height}")
