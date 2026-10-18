"""Rust vs Go, web vs terminal: one 2x2 still and one 2x2 clip per item.

    python compose4.py

    ┌──────── RUST ────────┬───────── GO ─────────┐
    │ web                  │ web                  │
    ├──────────────────────┼──────────────────────┤
    │ terminal             │ terminal             │
    └──────────────────────┴──────────────────────┘

Stills come from {rust,go}/raw/{web,tui}-NN-name.png, clips from
{rust,go}/raw/{web,tui}-NN-name.mp4. The four clips are lined up on the
moment the control opens: the web clips start LEAD seconds before it (see
cut_web.py); for the terminal clips the open time is worked out from the
item's steps. Also writes contact sheets of the 16 tabs, one per client.
Writes compare/NN-name.png, compare/NN-name.gif and compare/tabs-*.png.
"""
import os
import subprocess

from PIL import Image, ImageDraw, ImageFont

from gen_tapes import LEAD_IN
from steps import ITEMS, TABS, TITLES, TYPE_MS

OUT = "compare"
LEAD = 0.5          # seconds before the control opens, as in cut_web.py
BG, FG, DIM = (17, 21, 31), (230, 233, 240), (140, 148, 165)
CELL_BG = (11, 14, 22)
RUST, GO = (247, 112, 51), (0, 173, 216)
COLS = [("rust", "RUST", RUST, "Leptos → WASM  ·  ratatui"),
        ("go", "GO", GO, "Go → WASM (syscall/js)  ·  Bubble Tea")]
ROWS = [("web", "Web"), ("tui", "Terminal")]
os.makedirs(OUT, exist_ok=True)


def font(size):
    for f in ("/System/Library/Fonts/SFNS.ttf", "/System/Library/Fonts/Helvetica.ttc",
              "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf"):
        if os.path.exists(f):
            return ImageFont.truetype(f, size)
    return ImageFont.load_default()


def fit(w, h, cw, ch):
    """Size of a w x h image scaled to fit inside cw x ch."""
    k = min(cw / w, ch / h)
    return max(2, round(w * k) // 2 * 2), max(2, round(h * k) // 2 * 2)


def tui_open_time(steps):
    """Seconds from the tape's Show to the "open" step (mirrors gen_tapes.body)."""
    t = LEAD_IN / 1000
    for s in steps:
        if s == "open":
            return t
        if s == "shot":
            t += 3.0
        elif s == "reset":
            t += 1.6 + TYPE_MS / 1000 * 3
        elif s[0] == "k":
            t += s[2] * TYPE_MS / 1000 + 0.15
        elif s[0] == "t":
            t += len(s[1]) * TYPE_MS / 1000
        elif s[0] == "s":
            t += s[1] / 1000
    raise SystemExit("no open step")


CW, CH, PAD, HEAD, CAP, GAP = 760, 452, 20, 118, 34, 16


def layout():
    rects, y = {}, HEAD
    for kind, _ in ROWS:
        for i, (lang, *_) in enumerate(COLS):
            rects[(lang, kind)] = (PAD + i * (CW + GAP), y + CAP, CW, CH)
        y += CAP + CH + GAP
    return (PAD * 2 + 2 * CW + GAP, y + PAD - GAP), rects


def canvas(title, size, rects):
    """Background with title, language headers, row captions and empty cells."""
    im = Image.new("RGB", size, BG)
    d = ImageDraw.Draw(im)
    d.text((20, 12), title, fill=FG, font=font(30))
    for lang, tag, color, sub in COLS:
        x = rects[(lang, "web")][0]
        d.rectangle([x, 58, x + CW, 62], fill=color)
        d.text((x, 70), tag, fill=color, font=font(30))
        d.text((x + d.textlength(tag + "   ", font=font(30)), 77), sub, fill=DIM, font=font(20))
    for (lang, kind), (x, y, w, h) in rects.items():
        tag = dict((c[0], c[1]) for c in COLS)[lang]
        d.text((x, y - 28), f"{tag} · {dict(ROWS)[kind]}", fill=DIM, font=font(20))
        d.rectangle([x, y, x + w - 1, y + h - 1], fill=CELL_BG)
    return im


size, rects = layout()
for name, _, _, steps in ITEMS:
    # ---- still ----
    out = canvas(TITLES[name], size, rects)
    for (lang, kind), (x, y, cw, ch) in rects.items():
        im = Image.open(f"{lang}/raw/{kind}-{name}.png").convert("RGB")
        w, h = fit(*im.size, cw, ch)
        out.paste(im.resize((w, h), Image.LANCZOS), (x + (cw - w) // 2, y + (ch - h) // 2))
    out.save(f"{OUT}/{name}.png", optimize=True)

    # ---- clip: same layout, the four recordings overlaid on the canvas ----
    bg = f"{OUT}/.bg-{name}.png"
    canvas(TITLES[name], size, rects).save(bg)
    inputs, filters, last, durations = [], [], "0:v", []
    for n, (lang, kind) in enumerate(((lang, kind) for kind, _ in ROWS for lang, *_ in COLS), 1):
        src = f"{lang}/raw/{kind}-{name}.mp4"
        dur = float(subprocess.run(["ffprobe", "-v", "error", "-show_entries", "format=duration",
                                    "-of", "csv=p=0", src], capture_output=True, text=True).stdout)
        pre = ""
        if kind == "tui":
            start = max(0.0, tui_open_time(steps) - LEAD)
            pre = f"trim=start={start:.2f},setpts=PTS-STARTPTS,"
            dur -= start
        durations.append(dur)
        x, y, cw, ch = rects[(lang, kind)]
        src_w, src_h = (1640, 960) if kind == "tui" else (1280, 760)
        w, h = fit(src_w, src_h, cw, ch)
        inputs += ["-i", src]
        filters.append(f"[{n}:v]{pre}fps=10,scale={w}:{h}:flags=lanczos,"
                       f"tpad=stop_mode=clone:stop_duration=30[v{n}]")
        # shortest=1: every clip is padded, so each overlay ends with the background
        filters.append(f"[{last}][v{n}]overlay={x + (cw - w) // 2}:{y + (ch - h) // 2}:shortest=1[o{n}]")
        last = f"o{n}"
    total = max(durations) + 0.5
    gw = min(1200, size[0])
    filters.append(f"[{last}]fps=10,scale={gw}:-2:flags=lanczos,split[a][b];"
                   "[a]palettegen=max_colors=128:stats_mode=diff[p];"
                   "[b][p]paletteuse=dither=none:diff_mode=rectangle")
    # the background is a still looped for exactly `total` seconds: an endless
    # input would keep palettegen buffering frames until memory runs out
    subprocess.run(["ffmpeg", "-y", "-loglevel", "error", "-loop", "1", "-framerate", "10",
                    "-t", f"{total:.2f}", "-i", bg, *inputs, "-filter_complex", ";".join(filters),
                    f"{OUT}/{name}.gif"], check=True)
    os.remove(bg)
    print("ok", name, f"{total:.1f}s")

# ---- contact sheets: the 16 tabs of each client in a 4x4 grid ----
SHEETS = {("rust", "web"): "webtour", ("rust", "tui"): "tour", ("go", "web"): "webtour", ("go", "tui"): "tour"}
TW, TH, TPAD, TCAP = 480, 285, 14, 30
for (lang, kind), prefix in SHEETS.items():
    tag = dict((c[0], c[1]) for c in COLS)[lang]
    color = dict((c[0], c[2]) for c in COLS)[lang]
    sheet = Image.new("RGB", (TPAD + 4 * (TW + TPAD), 70 + 4 * (TH + TCAP + TPAD)), BG)
    d = ImageDraw.Draw(sheet)
    d.text((TPAD, 16), tag, fill=color, font=font(32))
    d.text((TPAD + d.textlength(tag + "  ", font=font(32)), 20),
           f"· {dict(ROWS)[kind]}: all 16 tabs", fill=FG, font=font(26))
    for i, (tab_id, title) in enumerate(TABS):
        path = f"{lang}/raw/{prefix}-{i + 1:02d}-{tab_id}.png"
        if not os.path.exists(path):
            continue
        im = Image.open(path).convert("RGB")
        w, h = fit(*im.size, TW, TH)
        x, y = TPAD + (i % 4) * (TW + TPAD), 70 + (i // 4) * (TH + TCAP + TPAD)
        d.text((x, y), f"{i + 1} {title}", fill=DIM, font=font(20))
        sheet.paste(im.resize((w, h), Image.LANCZOS), (x, y + TCAP))
    sheet.save(f"{OUT}/tabs-{lang}-{kind}.png", optimize=True)
    print("ok", f"tabs-{lang}-{kind}")
