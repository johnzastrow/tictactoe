"""Shared helpers for the pt web e2e scripts (Playwright, system Chrome)."""
import os
import sys

BASE = os.environ.get("PT_URL", "http://127.0.0.1:8792")
PASSWORD = os.environ.get("PT_PASSWORD", "web-pass-123")
SHOTS = os.environ.get("PT_SHOTS", "/tmp/pt-shots")
os.makedirs(SHOTS, exist_ok=True)


class Console:
    """Collects console errors and CSP violations from a page."""

    def __init__(self, page, name):
        self.name = name
        self.errors = []
        page.on("console", lambda m: m.type in ("error", "warning") and self._add(f"{m.type}: {m.text}"))
        page.on("pageerror", lambda e: self._add(f"pageerror: {e}"))

    def _add(self, msg):
        self.errors.append(msg)
        if os.environ.get("PT_VERBOSE"):
            print("   [console]", msg[:2000])

    def assert_clean(self):
        bad = [e for e in self.errors if "favicon" not in e]
        assert not bad, f"{self.name} console errors: {bad}"


def login(page, user, password=PASSWORD):
    page.goto(BASE + "/")
    page.wait_for_selector("#login-user", timeout=15000)
    page.fill("#login-user", user)
    page.fill("#login-pass", password)
    page.keyboard.press("Enter")
    page.wait_for_selector("#g-projects .row", timeout=15000)
    # snapshot arrives over the socket
    page.wait_for_function("document.querySelectorAll('#g-projects .row').length > 10", timeout=15000)


def shot(page, name):
    path = os.path.join(SHOTS, name + ".png")
    page.screenshot(path=path)
    return path


def check(cond, msg):
    if not cond:
        print("FAIL:", msg)
        sys.exit(1)
    print("ok  ", msg)
