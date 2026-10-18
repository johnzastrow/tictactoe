"""Capture each web form control as a zoomed still and a video clip.

    PT_URL=http://127.0.0.1:8799 PT_PASSWORD=demo-pass-123 \
        python capture_web.py rust|go OUT_DIR [--video]

Renders at 2x in a small window. Each control is opened with the keyboard
(the same keys as the TUI tapes, typed at the same pace). Without --video it
writes stills clipped to the control plus its grid cell, OUT_DIR/web-NN-name.png.
With --video it records the session instead (a screenshot taken while
recording leaves a grey frame in the video, hence two passes) and writes
OUT_DIR/web-session.webm (at 1x: Playwright records CSS pixels) and
OUT_DIR/segments.json
(when each control opened and closed, and its clip box) for cut_web.py.
Nothing is saved: every editor is cancelled.
"""
import json
import os
import shutil
import sys
import time

from playwright.sync_api import sync_playwright

BASE = os.environ.get("PT_URL", "http://127.0.0.1:8799")
PASSWORD = os.environ.get("PT_PASSWORD", "demo-pass-123")
IMPL, OUT = sys.argv[1], sys.argv[2]
VIDEO = "--video" in sys.argv[3:]
os.makedirs(OUT, exist_ok=True)

# Selectors differ between the two hand-built web clients.
SEL = {
    "rust": {
        "projects": "#g-projects", "tasks": "#g-tasks", "row": ".row[data-id]",
        "cur": ".cell.cur", "choices": ".choices", "datepop": ".datepop",
        "chips": ".chipsed, .popup:has(.chipsed)", "qinput": "#query", "qsugg": ".qsugg",
        "dialog": ".dialog", "palette": ".palette", "ctx": ".ctxmenu",
        "cols": ".colsdlg", "field": '.dialog .fld:has(.flabel:text-is("{}")) input',
    },
    "go": {
        "projects": ".pane.main .grid", "tasks": ".pane.main .grid", "row": ".gbody .row",
        "cur": ".cell.cur", "choices": ".popup:not(.hidden)", "datepop": ".popup.datepop",
        "chips": ".edroot:not(.embedded), .popup:not(.hidden)", "qinput": "#query", "qsugg": ".qsugg",
        "dialog": ".dialog", "palette": ".dialog:has(#palette-input)", "ctx": ".ctxmenu",
        "cols": ".dialog:has(.collist)", "field": '.dialog .frow:has(.flabel:text-is("{}")) input',
    },
}[IMPL]
# Rust grid cells carry data-col; the Go grid is addressed by header title.
COLS = {"owner_id": "Owner", "status": "Status", "due_date": "Due", "tags": "Tags", "deps": "Deps"}


def bbox(page, sels):
    """Union of the bounding boxes of every visible element matching sels."""
    boxes = []
    for s in sels:
        for el in page.locator(s).all():
            b = el.bounding_box()
            if b and b["width"] > 0 and b["height"] > 0:
                boxes.append(b)
    if not boxes:
        raise SystemExit(f"nothing visible for {sels}")
    x0 = min(b["x"] for b in boxes)
    y0 = min(b["y"] for b in boxes)
    x1 = max(b["x"] + b["width"] for b in boxes)
    y1 = max(b["y"] + b["height"] for b in boxes)
    return x0, y0, x1, y1


SEGS = {}
T0 = 0.0
TYPE_DELAY = 120  # ms per key, as Set TypingSpeed in tapes/_setup.tape


def now():
    return round(time.time() - T0, 2)


def begin(page, name):
    """Mark the moment just before the control opens (the clip starts here)."""
    page.wait_for_timeout(300)
    SEGS[name] = {"open": now()}


def typed(page, text):
    page.keyboard.type(text, delay=TYPE_DELAY)


def snap(page, name, sels, pad=28):
    page.wait_for_timeout(1500)
    vw, vh = page.viewport_size["width"], page.viewport_size["height"]
    x0, y0, x1, y1 = bbox(page, sels)
    x0, y0 = max(0, x0 - pad), max(0, y0 - pad)
    x1, y1 = min(vw, x1 + pad), min(vh, y1 + pad)
    if not VIDEO:
        page.screenshot(path=os.path.join(OUT, "web-" + name + ".png"),
                        clip={"x": x0, "y": y0, "width": x1 - x0, "height": y1 - y0})
    SEGS[name]["box"] = [x0, y0, x1, y1]
    page.wait_for_timeout(1500)
    SEGS[name]["end"] = now()
    print("ok", name)


def focus(page, grid):
    if IMPL == "rust":
        page.focus(grid)
    else:
        page.click(f"{grid} .gbody .row .cell >> nth=1")


def cancel(page, grid):
    for _ in range(3):
        page.keyboard.press("Escape")
        page.wait_for_timeout(80)
    if IMPL == "rust":
        page.focus(grid)


def goto_col(page, grid, col):
    page.keyboard.press("Home")
    if IMPL == "go":
        titles = page.eval_on_selector_all(f"{grid} .ghead .hcell .ht", "els => els.map(e => e.textContent)")
        for _ in range(titles.index(COLS[col])):
            page.keyboard.press("ArrowRight")
        page.wait_for_timeout(100)
        return
    for _ in range(40):
        if page.locator(f"{grid} {SEL['cur']}").get_attribute("data-col") == col:
            return
        page.keyboard.press("ArrowRight")
    raise SystemExit(f"column {col} not reached")


def cur(grid):
    return f"{grid} {SEL['cur']}"


with sync_playwright() as p:
    b = p.chromium.launch(channel="chrome")
    vdir = os.path.join(OUT, "video")
    rec = {"record_video_dir": vdir, "record_video_size": {"width": 1280, "height": 760}} if VIDEO else {}
    ctx = b.new_context(viewport={"width": 1280, "height": 760}, device_scale_factor=2,
                        color_scheme="dark", **rec)
    page = ctx.new_page()
    T0 = time.time()
    page.goto(BASE + "/")
    page.wait_for_selector("#login-user", timeout=20000)
    page.fill("#login-user", "alice")
    page.fill("#login-pass", PASSWORD)
    page.keyboard.press("Enter")
    P, T = SEL["projects"], SEL["tasks"]
    page.wait_for_selector(f"{P} {SEL['row']}", timeout=20000)
    page.wait_for_timeout(800)
    focus(page, P)
    page.keyboard.press("ArrowDown")

    # 01 fuzzy combobox: owner
    goto_col(page, P, "owner_id")
    begin(page, "01-combobox")
    page.keyboard.press("Enter")
    typed(page, "fr")
    snap(page, "01-combobox", [cur(P), SEL["choices"]])
    cancel(page, P)

    # 02 dropdown: status
    goto_col(page, P, "status")
    begin(page, "02-dropdown")
    page.keyboard.press("Enter")
    snap(page, "02-dropdown", [cur(P), SEL["choices"]])
    cancel(page, P)

    # 03 natural-language date + calendar
    goto_col(page, P, "due_date")
    begin(page, "03-date")
    page.keyboard.press("F2")
    page.keyboard.press("ControlOrMeta+a")
    typed(page, "fri")
    snap(page, "03-date", [cur(P), SEL["datepop"]])
    cancel(page, P)

    # 04 tag chips
    goto_col(page, P, "tags")
    begin(page, "04-tags")
    page.keyboard.press("F2")
    typed(page, "pi")
    snap(page, "04-tags", [cur(P), SEL["chips"], SEL["choices"]])
    cancel(page, P)

    # 05 query bar autocomplete
    begin(page, "05-query")
    page.keyboard.press("Control+f")
    typed(page, "status:active health:")
    snap(page, "05-query", [SEL["qinput"], SEL["qsugg"]])
    page.keyboard.press("ControlOrMeta+a"); page.keyboard.press("Backspace")
    cancel(page, P)

    # 06 command palette
    begin(page, "06-palette")
    page.keyboard.press("Control+k")
    page.wait_for_timeout(400)
    typed(page, "nwr")
    snap(page, "06-palette", [SEL["palette"]])
    cancel(page, P)

    # 07 context menu
    goto_col(page, P, "status")
    begin(page, "07-context-menu")
    page.keyboard.press("Shift+F10")
    snap(page, "07-context-menu", [cur(P), SEL["ctx"]])
    cancel(page, P)

    # 08 column chooser
    begin(page, "08-columns")
    page.keyboard.press("F4")
    snap(page, "08-columns", [SEL["cols"]])
    cancel(page, P)

    # 09 record form
    begin(page, "09-record")
    page.keyboard.press("Control+e")
    snap(page, "09-record", [SEL["dialog"]], pad=12)
    cancel(page, P)

    # 10 cascading New project: program → code, template → dates and budget
    begin(page, "10-new-project")
    page.keyboard.press("Insert")
    page.wait_for_selector(SEL["dialog"])
    page.locator(SEL["field"].format("Program")).click()
    typed(page, "munic"); page.keyboard.press("Enter")
    page.locator(SEL["field"].format("Name")).fill("Storm drain inventory")
    page.locator(SEL["field"].format("Template")).click()
    typed(page, "data mig")
    snap(page, "10-new-project", [SEL["dialog"], SEL["choices"]], pad=12)
    cancel(page, P)

    # 11 cascading New task: program → project → template phases
    page.keyboard.press("Alt+Digit2")
    page.wait_for_selector(f"{T} {SEL['row']}")
    focus(page, T)
    begin(page, "11-new-task")
    page.keyboard.press("Insert")
    page.wait_for_selector(SEL["dialog"])
    page.locator(SEL["field"].format("Phase")).click()
    snap(page, "11-new-task", [SEL["dialog"], SEL["choices"]], pad=12)
    cancel(page, T)

    # 12 dependency chips (tasks of the same project)
    page.keyboard.press("Control+Home"); page.keyboard.press("ArrowDown")
    goto_col(page, T, "deps")
    begin(page, "12-deps")
    page.keyboard.press("F2")
    typed(page, "sprint")
    snap(page, "12-deps", [cur(T), SEL["chips"], SEL["choices"]])
    cancel(page, T)

    page.wait_for_timeout(500)
    video = page.video.path() if VIDEO else None
    ctx.close()
    b.close()
if VIDEO:
    shutil.move(video, os.path.join(OUT, "web-session.webm"))
    shutil.rmtree(os.path.join(OUT, "video"), ignore_errors=True)
    with open(os.path.join(OUT, "segments.json"), "w") as f:
        json.dump(SEGS, f, indent=1)
