"""Build the side-by-side gallery images: web control on the left, TUI on the right.

    python compose.py rust|go

Reads <lang>/raw/web-NN-name.png (already cropped by capture_web.py) and
<lang>/raw/tui-NN-name.png (full 1640x960 terminal frames from the tapes), crops
the TUI frame to its box in boxes.py, scales both halves to one height (or
stacks wide ones) and writes <lang>/NN-name.png.
"""
import glob
import os
import sys

from PIL import Image, ImageDraw, ImageFont

from boxes import BOXES, WEB_MAXH

LANG = sys.argv[1]
H = 600          # height of each half, px
GAP, TOP, MARGIN = 24, 56, 24
BG, FG, DIM = (17, 21, 31), (230, 233, 240), (140, 148, 165)
LABELS = {
    "rust": ("RUST · Web (Leptos → WASM)", "RUST · Terminal (ratatui)"),
    "go": ("GO · Web (Go → WASM, syscall/js)", "GO · Terminal (Bubble Tea)"),
}
LANG_COLOR = {"RUST": (247, 112, 51), "GO": (0, 173, 216)}


def label(d, xy, text):
    """Draw "LANG · rest" with the language name in its colour."""
    lang, _, rest = text.partition(" · ")
    d.text(xy, lang, fill=LANG_COLOR[lang], font=bold_font)
    x = xy[0] + d.textlength(lang + "  ", font=bold_font)
    d.text((x, xy[1]), rest, fill=FG, font=label_font)


def font(size):
    for f in ("/System/Library/Fonts/SFNS.ttf", "/System/Library/Fonts/Helvetica.ttc",
              "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf"):
        if os.path.exists(f):
            return ImageFont.truetype(f, size)
    return ImageFont.load_default()


def fit(im, h):
    return im.resize((round(im.width * h / im.height), h), Image.LANCZOS)


def fitw(im, w):
    return im.resize((w, round(im.height * w / im.width)), Image.LANCZOS)


label_font = font(26)
bold_font = font(28)
for web_path in sorted(glob.glob(f"{LANG}/raw/web-*.png")):
    name = os.path.basename(web_path)[4:-4]
    tui_path = f"{LANG}/raw/tui-{name}.png"
    if not os.path.exists(tui_path):
        print("skip (no TUI frame)", name)
        continue
    web = Image.open(web_path).convert("RGB")
    tui = Image.open(tui_path).convert("RGB")
    if name in WEB_MAXH and web.height > WEB_MAXH[name]:
        web = web.crop((0, 0, web.width, WEB_MAXH[name]))
    box = BOXES[LANG].get(name)
    if box:
        tui = tui.crop(box)
    left, right = LABELS[LANG]
    if web.width / web.height + tui.width / tui.height > 3.2:
        # wide controls: stack, web on top, both the same width
        W = min(1600, max(web.width, tui.width))
        web, tui = fitw(web, W), fitw(tui, W)
        out = Image.new("RGB", (MARGIN * 2 + W, TOP * 2 + web.height + tui.height + MARGIN), BG)
        d = ImageDraw.Draw(out)
        label(d, (MARGIN, 14), left)
        out.paste(web, (MARGIN, TOP))
        label(d, (MARGIN, TOP + web.height + 14), right)
        out.paste(tui, (MARGIN, TOP * 2 + web.height))
    else:
        h = max(420, min(H, web.height, tui.height))
        web, tui = fit(web, h), fit(tui, h)
        out = Image.new("RGB", (MARGIN * 2 + web.width + GAP + tui.width, TOP + h + MARGIN), BG)
        d = ImageDraw.Draw(out)
        label(d, (MARGIN, 14), left)
        label(d, (MARGIN + web.width + GAP, 14), right)
        out.paste(web, (MARGIN, TOP))
        out.paste(tui, (MARGIN + web.width + GAP, TOP))
    out.save(f"{LANG}/{name}.png", optimize=True)
    print("ok", name, f"{out.width}x{out.height}")
