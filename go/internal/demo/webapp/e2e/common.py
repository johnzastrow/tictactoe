"""Shared helpers for the Go demo web client's Playwright end-to-end scripts.

Run against a running Go demo server (a fresh DB, or `demo reset` first):
    DEMO_E2E_URL=http://127.0.0.1:8797 uv run --with playwright python e2e/<script>.py
Screenshots go to $DEMO_E2E_SHOTS (default: ./shots). Chrome is used via
channel="chrome" (set DEMO_E2E_CHANNEL="" for Playwright's bundled Chromium).
"""
import os
import sys
import time

from playwright.sync_api import sync_playwright  # noqa: F401

URL = os.environ.get("DEMO_E2E_URL", "http://127.0.0.1:8797")
SHOTS = os.environ.get("DEMO_E2E_SHOTS", "shots")
CHANNEL = os.environ.get("DEMO_E2E_CHANNEL", "chrome")
os.makedirs(SHOTS, exist_ok=True)

TABS = ["text", "number", "date", "select", "combo", "tags", "textarea", "choice",
        "cascade", "grid", "menus", "palette", "dialogs", "charts", "kanban", "gantt"]

FAILED = []


def check(cond, msg):
    if cond:
        print("  ok  ", msg)
    else:
        print("  FAIL", msg)
        FAILED.append(msg)


def done():
    if FAILED:
        print(f"\n{len(FAILED)} check(s) failed:")
        for f in FAILED:
            print("  -", f)
        sys.exit(1)
    print("\nall checks passed")


def launch(p):
    if CHANNEL:
        return p.chromium.launch(channel=CHANNEL)
    return p.chromium.launch()


def new_page(browser, errors, width=1440, height=900):
    """A page that records console errors/warnings, page errors and CSP violations."""
    ctx = browser.new_context(viewport={"width": width, "height": height})
    # record CSP violations explicitly (init scripts run via CDP, outside the page's CSP)
    ctx.add_init_script("window.__csp = []; document.addEventListener('securitypolicyviolation',"
                        " e => window.__csp.push(e.violatedDirective + ' ' + e.blockedURI));")
    page = ctx.new_page()
    page.on("console", lambda m: errors.append(f"console.{m.type}: {m.text}")
            if m.type in ("error", "warning") or "panic" in m.text or ".go:" in m.text else None)
    page.on("pageerror", lambda e: errors.append(f"pageerror: {e}"))
    return ctx, page


def join(page, name, theme=None):
    """Join directly with ?name= and wait for the welcome."""
    page.goto(f"{URL}/?name={name}")
    page.wait_for_selector(".menubar .dot-on", timeout=20000)
    page.wait_for_selector('[data-tab="text"]', state="attached")
    wait(page, 300)


def wait(page, ms=200):
    page.wait_for_timeout(ms)


def typ(page, text):
    """Type text and let the next frame render (controls update on requestAnimationFrame)."""
    page.keyboard.type(text)
    wait(page, 120)


def clk(page, selector, **kw):
    """Click and let the next frame render."""
    page.click(selector, **kw)
    wait(page, 120)


def key(page, k, times=1):
    for _ in range(times):
        page.keyboard.press(k)
    wait(page, 90)


def goto_tab(page, tab_id):
    """Click the sidebar entry, then put the focus into the tab body."""
    page.click(f'[data-tab-link="{tab_id}"]')
    wait(page, 150)
    key(page, "Enter")
    wait(page, 100)


def body(page, tab_id):
    return f'[data-tab="{tab_id}"]'


def status(page):
    el = page.query_selector(".statusline .smsg")
    return el.inner_text() if el else ""


def value(page, k):
    """The Value panel's `key = value` text for a key on the shown tab."""
    el = page.query_selector(f'.tabbody:not(.hidden) .vrow[data-key="{k}"] .vv')
    return el.get_attribute("title") if el else None  # the full value (the text may be cut)


def wait_value(page, k, text, timeout=5.0):
    """Poll until the Value panel shows text for key k (the CSP forbids wait_for_function)."""
    end = time.time() + timeout
    while time.time() < end:
        v = value(page, k)
        if v is not None and text in v:
            return True
        page.wait_for_timeout(50)
    return False


def wait_value_not(page, k, text, timeout=5.0):
    """Poll until the Value panel for key k no longer contains text."""
    end = time.time() + timeout
    while time.time() < end:
        v = value(page, k)
        if v is not None and text not in v:
            return True
        page.wait_for_timeout(50)
    return False


def wait_text(page, selector, text, timeout=5.0):
    end = time.time() + timeout
    while time.time() < end:
        el = page.query_selector(selector)
        if el and text in el.inner_text():
            return True
        page.wait_for_timeout(50)
    return False


def real_errors(errors):
    return [e for e in errors if "favicon" not in e]


def reset_all(page):
    """File ▸ Reset all tabs… and confirm (every key back to its default)."""
    key(page, "Alt+f")
    page.click('.menubar [data-item="reset_all"]')
    page.wait_for_selector('[data-popup="dialog"]')
    key(page, "y")
    wait(page, 400)


def focused_control(page):
    return page.evaluate("() => { const e = document.activeElement.closest('[data-control]'); return e ? e.dataset.control : null }")


def csp_violations(page):
    return page.evaluate("() => window.__csp || []")
