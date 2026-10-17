"""Smoke: login, every view renders, no console/CSP errors, screenshots."""
import time
from common import *

with sync_playwright() as p:
    b = p.chromium.launch(channel="chrome")
    errors = []
    ctx, page = new_page(b, errors)
    t0 = time.time()
    login(page, "alice")
    print(f"time to interactive after login: {time.time() - t0:.2f}s")
    check(page.query_selector(".menubar .conn .dot-on") is not None, "connected dot")
    check("go" in page.inner_text(".menubar .conn"), "server impl shown")
    for i, name in enumerate(["projects", "tasks", "board", "timeline", "dashboard", "activity", "templates", "fields", "users"]):
        page.keyboard.press(f"Alt+Digit{i+1}")
        page.wait_for_timeout(500)
        page.screenshot(path=f"{SHOTS}/{i+1}-{name}.png")
        check(name.capitalize() in page.inner_text(".tab.active"), f"view {name} active")
    # the only expected error is the 401 from the pre-login /api/me probe
    real = [e for e in errors if "401" not in e]
    check(not real, "no console errors or CSP violations: " + "; ".join(real[:5]))
    b.close()
done()
