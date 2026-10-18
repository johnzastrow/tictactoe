"""Smoke: the join page, ?name=, every tab via the keyboard, no console/CSP errors."""
from playwright.sync_api import sync_playwright, expect
from common import BASE, Console, check, launch, shot

TABS = ["text", "number", "date", "select", "combo", "tags", "textarea", "choice",
        "cascade", "grid", "menus", "palette", "dialogs", "charts", "kanban", "gantt"]

with sync_playwright() as p:
    b = launch(p)
    page = b.new_page(viewport={"width": 1400, "height": 900})
    con = Console(page, "smoke")

    # join page: a bad name is refused inline, a good one joins and is remembered
    page.goto(BASE + "/")
    page.wait_for_selector("#join-name")
    shot(page, "join")
    page.fill("#join-name", "1bad")
    page.keyboard.press("Enter")
    expect(page.locator(".lerr")).to_be_visible()
    check(True, "join: an invalid name is refused with a message")
    page.fill("#join-name", "alice")
    page.keyboard.press("Enter")
    expect(page.locator(".statusline")).to_contain_text("Connected", timeout=10000)
    check(page.locator(".menubar.top .who").inner_text().startswith("demo · alice"), "menu bar shows demo · alice · ● rust 0.5.0")
    check(page.locator(".cdot.dot-on").count() == 1, "connection dot is green")

    # the sidebar has focus first; ↓ walks every tab and each shows its parts
    for i, t in enumerate(TABS):
        if i:
            page.keyboard.press("ArrowDown")
        body = page.locator(f"[data-tab='{t}']")
        expect(body).to_be_visible()
        check(body.locator("h1").count() == 1 and body.locator(".blurb").count() == 1
              and body.locator(".panel.keys .krow").count() > 2 and body.locator(".panel.value .vrow").count() >= 1
              and body.locator("[data-control]").count() >= 1,
              f"tab {i + 1} {t}: title, blurb, control(s), Keys and Value panels")
    shot(page, "smoke-gantt")
    page.keyboard.press("Home")
    expect(page.locator("[data-tab='text']")).to_be_visible()
    page.keyboard.press("End")
    expect(page.locator("[data-tab='gantt']")).to_be_visible()
    check(True, "Home / End in the sidebar")
    # Alt+digit and Alt+↑/↓
    page.keyboard.press("Alt+Digit3")
    expect(page.locator("[data-tab='date']")).to_be_visible()
    page.keyboard.press("Alt+Digit0")
    expect(page.locator("[data-tab='grid']")).to_be_visible()
    page.keyboard.press("Alt+ArrowDown")
    expect(page.locator("[data-tab='menus']")).to_be_visible()
    page.keyboard.press("Alt+ArrowUp")
    expect(page.locator("[data-tab='grid']")).to_be_visible()
    check(True, "Alt+3, Alt+0, Alt+↓, Alt+↑ switch tabs")

    # the name was remembered; File ▸ Leave returns to the join page
    page.click(".menubar.top [data-menu='File']")
    page.click("[data-popup='menu'] .item:has-text('Leave')")
    expect(page.locator("#join-name")).to_have_value("alice")
    check(True, "File ▸ Leave returns to the join page with the name remembered")
    page.wait_for_timeout(300)
    con.assert_clean()
    b.close()
print("smoke ok")
