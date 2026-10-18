"""Smoke: join page, ?name= join, every tab renders with its hooks, theme
switch, no console errors or CSP violations, screenshots of every tab."""
import time
from common import *

with sync_playwright() as p:
    b = launch(p)
    errors = []
    ctx, page = new_page(b, errors)

    # join page: the name is remembered in localStorage
    page.goto(URL + "/")
    page.wait_for_selector("#join-name", timeout=20000)
    page.fill("#join-name", "9bad")
    page.click("#join-btn")
    wait(page, 150)
    check("letter" in page.inner_text(".lerr"), "invalid name rejected inline")
    page.fill("#join-name", "Smokey")
    t0 = time.time()
    page.click("#join-btn")
    page.wait_for_selector(".menubar .dot-on", timeout=20000)
    print(f"time to connected after join: {time.time() - t0:.2f}s")
    check("go 0.5.0" in page.inner_text(".menubar .conn"), "server impl and version shown")
    check("Smokey" in page.inner_text(".menubar .conn"), "name shown in the menu bar")
    check(wait_text(page, ".statusline", "online: 1"), "presence: online 1")

    # File ▸ Leave returns to the join page with the name remembered
    key(page, "Alt+f")
    check(page.query_selector('[data-popup="menu"]') is not None, "Alt+F opens the File menu")
    page.click('.menubar [data-item="leave"]')
    page.wait_for_selector("#join-name")
    check(page.input_value("#join-name") == "Smokey", "name remembered after Leave")

    join(page, "alice")
    for i, t in enumerate(TABS):
        page.click(f'[data-tab-link="{t}"]')
        wait(page, 250)
        shown = page.query_selector(f'.tabbody[data-tab="{t}"]:not(.hidden)')
        check(shown is not None, f"tab {i+1} {t} shown")
        n = page.eval_on_selector_all(f'[data-tab="{t}"] [data-control]', "els => els.length")
        check(n > 0, f"tab {t} has data-control hooks ({n})")
        check(page.query_selector(f'[data-tab="{t}"] .vrow') is not None, f"tab {t} Value panel filled")
        check(page.query_selector(f'[data-tab-link="{t}"].cur') is not None, f"sidebar marks {t}")
        page.screenshot(path=f"{SHOTS}/{i+1:02d}-{t}.png")

    # theme via the View menu
    key(page, "Alt+v")
    page.click('.menubar [data-item="theme:daylight"]')
    wait(page, 150)
    check(page.get_attribute("html", "data-theme") == "daylight", "theme switched from the View menu")
    page.screenshot(path=f"{SHOTS}/daylight-gantt.png")
    key(page, "Alt+v")
    page.click('.menubar [data-item="theme:midnight"]')

    v = csp_violations(page)
    check(not v, "no CSP violations reported: " + "; ".join(v[:3]))
    real = real_errors(errors)
    check(not real, "no console errors or CSP violations: " + "; ".join(real[:5]))
    b.close()
done()
