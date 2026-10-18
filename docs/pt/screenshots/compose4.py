"""Rust vs Go, web vs terminal: one 2x2 still and one 2x2 clip per control.

    python compose4.py

    ┌──────── RUST ────────┬───────── GO ─────────┐
    │ web                  │ web                  │
    ├──────────────────────┼──────────────────────┤
    │ terminal             │ terminal             │
    └──────────────────────┴──────────────────────┘

Stills come from {rust,go}/raw/{web,tui}-NN-name.png, clips from
{rust,go}/raw/{web,tui}-NN-name.mp4. The four clips are lined up on the
moment the control opens: the web clips start LEAD seconds before it (see
cut_web.py); for the TUI clips the open time is read from the "# open" marker
in the tape. Writes compare/NN-name.png and compare/NN-name.gif.
"""
import os
import re
import subprocess

from PIL import Image, ImageDraw, ImageFont

from boxes import BOXES, WEB_MAXH

OUT = "compare"
LEAD = 0.5          # seconds before the control opens, as in cut_web.py
TYPING = 0.12       # tapes/_setup.tape: Set TypingSpeed 120ms
BG, FG, DIM = (17, 21, 31), (230, 233, 240), (140, 148, 165)
CELL_BG = (11, 14, 22)
RUST, GO = (247, 112, 51), (0, 173, 216)
COLS = [("rust", "RUST", RUST, "Leptos → WASM  ·  ratatui"),
        ("go", "GO", GO, "Go → WASM (syscall/js)  ·  Bubble Tea")]
ROWS = [("web", "Web"), ("tui", "Terminal")]
TITLES = {
    "01-combobox": "Fuzzy combobox (Owner)", "02-dropdown": "Dropdown select (Status)",
    "03-date": "Date editor with calendar", "04-tags": "Tag chips",
    "05-query": "Query bar autocomplete", "06-palette": "Command palette",
    "07-context-menu": "Context menu", "08-columns": "Column chooser",
    "09-record": "Record form", "10-new-project": "New project (cascading)",
    "11-new-task": "New task (cascading)", "12-deps": "Dependency chips",
}
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


def tape_open_time(name):
    """Seconds from the tape's Show (end of _setup.tape) to its # open marker."""
    t = 0.0
    for ln in open(f"tapes/{name}.tape"):
        ln = ln.strip()
        if ln.startswith("# open"):
            return t
        if not ln or ln.startswith(("#", "Output", "Source", "Screenshot")):
            continue
        if m := re.match(r"Sleep (\d+)(ms|s)$", ln):
            t += int(m[1]) / (1000 if m[2] == "ms" else 1)
        elif m := re.match(r'Type "(.*)"$', ln):
            t += len(m[1]) * TYPING
        else:  # a key, maybe with a repeat count: "Right 3"
            m = re.match(r"\S+(?: (\d+))?$", ln)
            t += int(m[1] or 1) * TYPING
    raise SystemExit(f"no # open marker in {name}.tape")


def layout(cells):
    """cells[(lang, kind)] = (w, h). Returns canvas size and each cell's rect."""
    CW, MAXH, PAD, HEAD, CAP, GAP = 760, 560, 20, 118, 34, 16
    rects, y = {}, HEAD
    for kind, _ in ROWS:
        hs = [fit(*cells[(lang, kind)], CW, MAXH)[1] for lang, *_ in COLS]
        rh = max(hs)
        for i, (lang, *_) in enumerate(COLS):
            rects[(lang, kind)] = (PAD + i * (CW + GAP), y + CAP, CW, rh)
        y += CAP + rh + GAP
    return (PAD * 2 + 2 * CW + GAP, y + PAD - GAP), rects


def canvas(name, size, rects):
    """Background with title, language headers, row captions and empty cells."""
    im = Image.new("RGB", size, BG)
    d = ImageDraw.Draw(im)
    d.text((20, 12), TITLES[name], fill=FG, font=font(30))
    for lang, tag, color, sub in COLS:
        x = rects[(lang, "web")][0]
        d.rectangle([x, 58, x + rects[(lang, "web")][2], 62], fill=color)
        d.text((x, 70), tag, fill=color, font=font(30))
        d.text((x + d.textlength(tag + "   ", font=font(30)), 77), sub, fill=DIM, font=font(20))
    for (lang, kind), (x, y, w, h) in rects.items():
        tag = dict((c[0], c[1]) for c in COLS)[lang]
        d.text((x, y - 28), f"{tag} · {dict(ROWS)[kind]}", fill=DIM, font=font(20))
        d.rectangle([x, y, x + w - 1, y + h - 1], fill=CELL_BG)
    return im


def still(lang, kind, name):
    im = Image.open(f"{lang}/raw/{kind}-{name}.png").convert("RGB")
    if kind == "web" and name in WEB_MAXH and im.height > WEB_MAXH[name]:
        im = im.crop((0, 0, im.width, WEB_MAXH[name]))
    if kind == "tui":
        im = im.crop(BOXES[lang][name])
    return im


for name in TITLES:
    # ---- still ----
    ims = {(lang, kind): still(lang, kind, name) for lang, *_ in COLS for kind, _ in ROWS}
    size, rects = layout({k: v.size for k, v in ims.items()})
    out = canvas(name, size, rects)
    for k, im in ims.items():
        x, y, cw, ch = rects[k]
        w, h = fit(*im.size, cw, ch)
        out.paste(im.resize((w, h), Image.LANCZOS), (x + (cw - w) // 2, y + (ch - h) // 2))
    out.save(f"{OUT}/{name}.png", optimize=True)

    # ---- clip: same layout, the four recordings overlaid on the canvas ----
    bg = f"{OUT}/.bg-{name}.png"
    canvas(name, size, rects).save(bg)
    inputs, filters, last = [], [], "0:v"
    durations = []
    for n, (lang, kind) in enumerate(((lang, kind) for kind, _ in ROWS for lang, *_ in COLS), 1):
        src = f"{lang}/raw/{kind}-{name}.mp4"
        dur = float(subprocess.run(["ffprobe", "-v", "error", "-show_entries", "format=duration",
                                    "-of", "csv=p=0", src], capture_output=True, text=True).stdout)
        if kind == "tui":
            start = max(0.0, tape_open_time(name) - LEAD)
            l, t, r, b = BOXES[lang][name]
            src_w, src_h = r - l, b - t
            pre = f"trim=start={start:.2f},setpts=PTS-STARTPTS,crop={src_w}:{src_h}:{l}:{t},"
            dur -= start
        else:
            src_w, src_h = ims[(lang, kind)].size  # the still has the clip's crop
            pre = ""
            if name in WEB_MAXH:
                pre = f"crop=iw:min(ih\\,{WEB_MAXH[name]}):0:0,"
        durations.append(dur)
        x, y, cw, ch = rects[(lang, kind)]
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
                   "[a]palettegen=max_colors=96:stats_mode=diff[p];"
                   "[b][p]paletteuse=dither=none:diff_mode=rectangle")
    # the background is a still looped for exactly `total` seconds: an endless
    # input would keep palettegen buffering frames until memory runs out
    subprocess.run(["ffmpeg", "-y", "-loglevel", "error", "-loop", "1", "-framerate", "10",
                    "-t", f"{total:.2f}", "-i", bg, *inputs, "-filter_complex", ";".join(filters),
                    f"{OUT}/{name}.gif"], check=True)
    os.remove(bg)
    print("ok", name, f"{size[0]}x{size[1]}", f"{total:.1f}s")
