"""Smoke: login, the projects grid renders, no console/CSP errors."""
from playwright.sync_api import sync_playwright
from common import Console, login, shot, check

with sync_playwright() as p:
    b = p.chromium.launch(channel="chrome")
    page = b.new_page(viewport={"width": 1600, "height": 950})
    con = Console(page, "smoke")
    login(page, "alice")
    check(page.locator("#g-projects .row").count() > 10, "projects rows render")
    shot(page, "smoke-projects")
    page.wait_for_timeout(500)
    print("\n".join(con.errors) or "no console errors")
    con.assert_clean()
    b.close()
