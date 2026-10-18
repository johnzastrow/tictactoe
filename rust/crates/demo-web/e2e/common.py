"""Shared helpers for the demo web e2e scripts (Playwright, system Chrome).

DOM hooks (shared with the Go web client): tab body `[data-tab=<id>]`,
controls `[data-control=<key or name>]`, popups `[data-popup=<kind>]`,
sidebar items `[data-tab-link=<id>]`, Value panel rows `.vrow[data-key=<key>]`.
"""
import json
import os
import sys

from playwright.sync_api import expect

BASE = os.environ.get("DEMO_URL", "http://127.0.0.1:8893")
SHOTS = os.environ.get("DEMO_SHOTS", "/tmp/demo-shots")
os.makedirs(SHOTS, exist_ok=True)


class Console:
    """Collects console errors/warnings, page errors and CSP violations."""

    def __init__(self, page, name):
        self.name = name
        self.errors = []
        page.on("console", lambda m: m.type in ("error", "warning") and self._add(f"{m.type}: {m.text}"))
        page.on("pageerror", lambda e: self._add(f"pageerror: {e}"))

    def _add(self, msg):
        self.errors.append(msg)
        if os.environ.get("DEMO_VERBOSE"):
            print("   [console]", msg[:2000])

    def assert_clean(self):
        bad = [e for e in self.errors if "favicon" not in e]
        check(not bad, f"{self.name}: no console errors or CSP violations {bad if bad else ''}")


def join(page, name):
    """Join directly with ?name= and wait for the welcome."""
    page.goto(f"{BASE}/?name={name}")
    page.wait_for_selector("[data-tab]", timeout=15000)
    expect(page.locator(".statusline")).to_contain_text("Connected", timeout=10000)


def goto_tab(page, tab_id):
    page.click(f"[data-tab-link='{tab_id}']")
    page.wait_for_selector(f"[data-tab='{tab_id}']")


def value(page, key):
    """The synced value of `key` as the Value panel shows it (parsed JSON)."""
    return json.loads(page.locator(f".vrow[data-key='{key}'] .vv").inner_text())


def expect_value(page, key, want, timeout=5000):
    page.wait_for_function(
        "([k, w]) => { const e = document.querySelector(`.vrow[data-key='${k}'] .vv`);"
        " try { return e && JSON.stringify(JSON.parse(e.innerText)) === JSON.stringify(w); } catch { return false; } }",
        arg=[key, want],
        timeout=timeout,
    )


def focus(page, control):
    page.focus(f"[data-control='{control}']")


def shot(page, name):
    path = os.path.join(SHOTS, name + ".png")
    page.screenshot(path=path)
    return path


def check(cond, msg):
    if not cond:
        print("FAIL:", msg)
        sys.exit(1)
    print("ok  ", msg)


def launch(p):
    return p.chromium.launch(channel="chrome", headless=os.environ.get("DEMO_HEADED") is None)
