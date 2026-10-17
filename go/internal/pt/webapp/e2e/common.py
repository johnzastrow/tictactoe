"""Shared helpers for the Go web client's Playwright end-to-end scripts.

Run against a Go server seeded with `pt seed --password $PT_E2E_PASSWORD`:
    PT_E2E_URL=http://127.0.0.1:8796 PT_E2E_PASSWORD=... python e2e/<script>.py
Screenshots go to $PT_E2E_SHOTS (default: ./shots).
"""
import os
import sys
import time

from playwright.sync_api import sync_playwright, expect  # noqa: F401

URL = os.environ.get("PT_E2E_URL", "http://127.0.0.1:8796")
PASSWORD = os.environ.get("PT_E2E_PASSWORD", "goweb-pass-1")
SHOTS = os.environ.get("PT_E2E_SHOTS", "shots")
os.makedirs(SHOTS, exist_ok=True)

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


def new_page(browser, errors, width=1600, height=900):
    ctx = browser.new_context(viewport={"width": width, "height": height})
    page = ctx.new_page()
    page.on("console", lambda m: errors.append(f"console.{m.type}: {m.text}") if m.type in ("error", "warning") or "panic" in m.text or "goroutine" in m.text or ".go:" in m.text else None)
    page.on("pageerror", lambda e: errors.append(f"pageerror: {e}"))
    return ctx, page


def login(page, user, password=PASSWORD):
    page.goto(URL + "/")
    page.wait_for_selector("#login-user", timeout=20000)
    page.fill("#login-user", user)
    page.fill("#login-pass", password)
    page.keyboard.press("Enter")
    page.wait_for_selector(".grid .row", timeout=20000)
    wait_idle(page)


def wait_idle(page, ms=250):
    page.wait_for_timeout(ms)


def key(page, k, times=1):
    for _ in range(times):
        page.keyboard.press(k)
    wait_idle(page, 120)


def cur_text(page):
    el = page.query_selector(".grid:focus-within .cell.cur .v, .grid .cell.cur .v")
    return el.inner_text() if el else None


def status(page):
    el = page.query_selector(".statusline .smsg")
    return el.inner_text() if el else ""


def wait_text(page, selector, text, timeout=5.0):
    """Poll until selector's text contains text (the CSP forbids wait_for_function)."""
    end = time.time() + timeout
    while time.time() < end:
        el = page.query_selector(selector)
        if el and text in el.inner_text():
            return True
        page.wait_for_timeout(50)
    return False
