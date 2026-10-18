"""Build the side-by-side gallery images: web control on the left, TUI on the right.

    python compose.py rust|go

Reads <lang>/raw/web-NN-name.png (already cropped by capture_web.py) and
<lang>/raw/tui-NN-name.png (full 1640x960 terminal frames from the tapes), crops
the TUI frame to the box below, scales both halves to one height (or stacks wide ones) and writes
<lang>/NN-name.png.
"""
import glob
import os
import sys

from PIL import Image, ImageDraw, ImageFont

LANG = sys.argv[1]
H = 600          # height of each half, px
GAP, TOP, MARGIN = 24, 56, 24
BG, FG, DIM = (17, 21, 31), (230, 233, 240), (140, 148, 165)
LABELS = {
    "rust": ("Web · Leptos → WASM", "Terminal · ratatui"),
    "go": ("Web · Go → WASM (syscall/js)", "Terminal · Bubble Tea"),
}

# TUI crop boxes (left, top, right, bottom) in terminal-frame pixels. The
# terminal is 1640x960 at 26 pt; cells are about 15.6 x 31 px.
BOXES = {
    "rust": {
        "01-combobox": (640, 100, 1540, 320),
        "02-dropdown": (1060, 100, 1625, 390),
        "03-date": (1080, 90, 1632, 520),
        "04-tags": (900, 105, 1632, 320),
        "05-query": (20, 70, 870, 470),
        "06-palette": (350, 70, 1290, 610),
        "07-context-menu": (1100, 90, 1625, 650),
        "08-columns": (450, 100, 1190, 820),
        "09-record": (130, 35, 1500, 855),
        "10-new-project": (140, 215, 1495, 685),
        "11-new-task": (140, 205, 1505, 712),
        "12-deps": (700, 105, 1632, 460),
    },
    "go": {
        "01-combobox": (640, 100, 1540, 320),
        "02-dropdown": (1060, 100, 1625, 390),
        "03-date": (1100, 90, 1632, 560),
        "04-tags": (900, 105, 1632, 320),
        "05-query": (20, 70, 870, 470),
        "06-palette": (80, 70, 1545, 560),
        "07-context-menu": (1160, 90, 1625, 740),
        "08-columns": (430, 80, 1200, 830),
        "09-record": (40, 40, 1610, 810),
        "10-new-project": (230, 225, 1420, 680),
        "11-new-task": (220, 190, 1420, 670),
        "12-deps": (700, 105, 1632, 460),
    },
}

# Tall web dialogs are cut to their top part (2x pixels).
WEB_MAXH = {"06-palette": 900}


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
        d.text((MARGIN, 14), left, fill=FG, font=label_font)
        out.paste(web, (MARGIN, TOP))
        d.text((MARGIN, TOP + web.height + 14), right, fill=FG, font=label_font)
        out.paste(tui, (MARGIN, TOP * 2 + web.height))
    else:
        h = max(420, min(H, web.height, tui.height))
        web, tui = fit(web, h), fit(tui, h)
        out = Image.new("RGB", (MARGIN * 2 + web.width + GAP + tui.width, TOP + h + MARGIN), BG)
        d = ImageDraw.Draw(out)
        d.text((MARGIN, 14), left, fill=FG, font=label_font)
        d.text((MARGIN + web.width + GAP, 14), right, fill=FG, font=label_font)
        out.paste(web, (MARGIN, TOP))
        out.paste(tui, (MARGIN + web.width + GAP, TOP))
    out.save(f"{LANG}/{name}.png", optimize=True)
    print("ok", name, f"{out.width}x{out.height}")
