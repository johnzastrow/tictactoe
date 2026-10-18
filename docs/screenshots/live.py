"""Browser half of the live demo: plays O in the web page while the TUI
(driven by tapes/live.tape) plays X, recording a video the whole time.

    python live.py http://127.0.0.1:8786 OUT_DIR

Writes OUT_DIR/web.webm, OUT_DIR/web.png (final position, 2x) and
OUT_DIR/web-offset.txt (seconds from video start to X's first move, used to
line the two recordings up).
"""
import os
import shutil
import sys
import time
import urllib.request

from playwright.sync_api import sync_playwright


def wait_marks(page, n, timeout=60):
    """Wait until X has n marks (a locator poll: the page's CSP blocks eval)."""
    end = time.time() + timeout
    while page.locator("#board .cell--x").count() < n:
        if time.time() > end:
            raise SystemExit(f"timed out waiting for {n} X marks")
        page.wait_for_timeout(50)


URL, OUT = sys.argv[1], sys.argv[2]
O_MOVES = [0, 8]  # top-left, bottom-right: never in X's middle column

# wait for the TUI's built-in web server to come up
for _ in range(300):
    try:
        urllib.request.urlopen(URL, timeout=1)
        break
    except OSError:
        time.sleep(0.1)

with sync_playwright() as p:
    b = p.chromium.launch(channel="chrome")
    size = {"width": 640, "height": 860}
    ctx = b.new_context(viewport=size, device_scale_factor=2, color_scheme="dark",
                        record_video_dir=OUT, record_video_size=size)
    page = ctx.new_page()
    t0 = time.time()
    page.goto(URL)
    page.wait_for_selector("#conn.conn--up")
    first_x = None
    for n, cell in enumerate(O_MOVES, 1):
        wait_marks(page, n)
        if first_x is None:
            first_x = time.time() - t0
        page.wait_for_timeout(900)  # let the viewer see X arrive
        page.locator("#board .cell").nth(cell).click()
    wait_marks(page, 3)
    page.wait_for_timeout(1500)
    page.screenshot(path=os.path.join(OUT, "web.png"))
    page.wait_for_timeout(1000)
    video = page.video.path()
    ctx.close()
    b.close()
shutil.move(video, os.path.join(OUT, "web.webm"))
with open(os.path.join(OUT, "web-offset.txt"), "w") as f:
    f.write(f"{first_x:.2f}\n")
print("web ok, first X at", round(first_x, 2), "s")
