"""Replay steps.py in a web client: a 2x still per item, or one recorded session.

    DEMO_URL=http://127.0.0.1:18797 python capture_web.py rust|go OUT_DIR [--video]

Without --video it writes OUT_DIR/web-NN-name.png (the whole 1280x760 window
at 2x). With --video it records the session instead (a screenshot taken while
recording leaves a grey frame in the video, hence two passes) and writes
OUT_DIR/web-session.webm plus OUT_DIR/segments.json (when each item opened and
ended) for cut_web.py. Both web clients use the same DOM hooks
(data-tab-link, data-tab) and the same keys, so one script drives both.
"""
import json
import os
import shutil
import sys
import time

from playwright.sync_api import sync_playwright

from steps import ITEMS, TABS, TYPE_MS

BASE = os.environ.get("DEMO_URL", "http://127.0.0.1:18797")
IMPL, OUT = sys.argv[1], sys.argv[2]
VIDEO = "--video" in sys.argv[3:]
W, H = 1280, 760
SETTLE = 1500
os.makedirs(OUT, exist_ok=True)

# VHS key names → Playwright key names
KEYS = {"Up": "ArrowUp", "Down": "ArrowDown", "Left": "ArrowLeft", "Right": "ArrowRight",
        "Enter": "Enter", "Tab": "Tab", "Escape": "Escape", "Space": "Space",
        "Backspace": "Backspace", "PageUp": "PageUp", "PageDown": "PageDown"}

SEGS = {}
T0 = 0.0
errors = []


def now():
    return round(time.time() - T0, 2)


def key(page, name):
    if name.startswith("Ctrl+"):
        page.keyboard.press("Control+" + name[5:].lower())
    else:
        page.keyboard.press(KEYS[name])


def run(page, name, steps):
    for s in steps:
        if s == "open":
            page.wait_for_timeout(300)
            SEGS[name] = {"open": now()}
        elif s == "shot":
            page.wait_for_timeout(SETTLE)
            if not VIDEO:
                page.screenshot(path=os.path.join(OUT, f"web-{name}.png"))
            page.wait_for_timeout(SETTLE)
            SEGS[name]["end"] = now()
        elif s == "reset":
            key(page, "Escape")
            page.wait_for_timeout(300)
            key(page, "Ctrl+R")
            page.wait_for_timeout(500)
            page.keyboard.type("y")
            page.wait_for_timeout(800)
        elif s[0] == "k":
            for _ in range(s[2]):
                key(page, s[1])
                page.wait_for_timeout(TYPE_MS)
            page.wait_for_timeout(150)
        elif s[0] == "t":
            page.keyboard.type(s[1], delay=TYPE_MS)
        elif s[0] == "s":
            page.wait_for_timeout(s[1])
    page.wait_for_timeout(500)
    SEGS[name]["box"] = [0, 0, W, H]


with sync_playwright() as p:
    b = p.chromium.launch(channel="chrome")
    vdir = os.path.join(OUT, "video")
    rec = {"record_video_dir": vdir, "record_video_size": {"width": W, "height": H}} if VIDEO else {}
    ctx = b.new_context(viewport={"width": W, "height": H}, device_scale_factor=2,
                        color_scheme="dark", **rec)
    page = ctx.new_page()
    page.on("console", lambda m: m.type == "error" and errors.append(m.text))
    page.on("pageerror", lambda e: errors.append(str(e)))
    T0 = time.time()
    page.goto(BASE + "/?name=alice")
    page.wait_for_selector('[data-tab-link="text"]', timeout=30000)
    page.wait_for_timeout(1500)

    only = os.environ.get("ONLY")  # e.g. ONLY=07-textarea for one item
    for name, tab, _, steps in ITEMS:
        if only and name != only:
            continue
        page.click(f'[data-tab-link="{TABS[tab - 1][0]}"]')
        page.wait_for_selector(f'[data-tab="{TABS[tab - 1][0]}"]')
        page.wait_for_timeout(800)
        run(page, name, steps)
        print("ok", name)

    if not VIDEO and not only:
        # the tour: every tab as it first appears
        for i, (tab_id, _) in enumerate(TABS, 1):
            page.click(f'[data-tab-link="{tab_id}"]')
            page.wait_for_timeout(700)
            page.screenshot(path=os.path.join(OUT, f"webtour-{i:02d}-{tab_id}.png"))

    page.wait_for_timeout(500)
    video = page.video.path() if VIDEO else None
    ctx.close()
    b.close()

if VIDEO:
    shutil.move(video, os.path.join(OUT, "web-session.webm"))
    shutil.rmtree(os.path.join(OUT, "video"), ignore_errors=True)
    with open(os.path.join(OUT, "segments.json"), "w") as f:
        json.dump(SEGS, f, indent=1)
if errors:
    print("console errors:", *errors, sep="\n  ")
