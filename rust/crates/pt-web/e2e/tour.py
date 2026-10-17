"""Screenshot every view in two themes; fail on console errors."""
import sys
from playwright.sync_api import sync_playwright
from common import Console, login, shot, check

VIEWS = ["Projects", "Tasks", "Board", "Timeline", "Dashboard", "Activity", "Templates", "Fields", "Users"]
only = sys.argv[1:]

with sync_playwright() as p:
    b = p.chromium.launch(channel="chrome")
    for scheme, theme in (("dark", "midnight"), ("light", "daylight")):
        if only and theme not in only:
            continue
        ctx = b.new_context(viewport={"width": 1680, "height": 1000}, color_scheme=scheme)
        page = ctx.new_page()
        con = Console(page, theme)
        login(page, "admin")
        for i, v in enumerate(VIEWS, 1):
            page.keyboard.press(f"Alt+Digit{i}")
            page.wait_for_timeout(500)
            shot(page, f"{theme}-{i}-{v.lower()}")
        # dialogs
        page.keyboard.press("Alt+Digit1"); page.wait_for_timeout(300)
        page.keyboard.press("Control+k"); page.wait_for_selector(".palette input:focus"); page.keyboard.type("go nwr"); page.wait_for_timeout(200)
        shot(page, f"{theme}-palette")
        page.keyboard.press("Escape")
        page.keyboard.press("F1"); page.wait_for_timeout(200); shot(page, f"{theme}-help"); page.keyboard.press("Escape")
        page.keyboard.press("Control+e"); page.wait_for_timeout(300); shot(page, f"{theme}-record"); page.keyboard.press("Escape")
        page.keyboard.press("F10"); page.wait_for_timeout(200); page.keyboard.press("ArrowRight"); page.keyboard.press("ArrowRight"); page.wait_for_timeout(100)
        shot(page, f"{theme}-menu"); page.keyboard.press("Escape")
        print(theme, "errors:", con.errors)
        con.assert_clean()
        ctx.close()
    b.close()
print("tour ok")
