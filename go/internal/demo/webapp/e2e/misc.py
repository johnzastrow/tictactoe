"""Headers (strict CSP), arrows/Tab/Enter/Esc reach every tab, panel
toggles, theme persistence across a reload, a narrow window, and (with
DEMO_E2E_RESTART set) reconnecting after the server restarts."""
import os
import subprocess
from common import *

RESTART = os.environ.get("DEMO_E2E_RESTART")  # a command that restarts the server keeping the DB

with sync_playwright() as p:
    b = launch(p)
    errors = []
    ctx, page = new_page(b, errors)

    # security headers on the page
    r = page.request.get(URL + "/")
    csp = r.headers.get("content-security-policy", "")
    check("script-src 'self' 'wasm-unsafe-eval'" in csp and "unsafe-inline" not in csp.split("script-src")[1].split(";")[0],
          "CSP: scripts from self + wasm only, no inline scripts")
    check(r.headers.get("x-frame-options") == "DENY" and r.headers.get("x-content-type-options") == "nosniff", "security headers")

    join(page, "carol")
    reset_all(page)

    # only arrows, Tab, Enter and Esc: every tab, into its body and back
    key(page, "Escape")
    key(page, "Home")
    for i, t in enumerate(TABS):
        if i:
            key(page, "ArrowDown")
        check(page.query_selector(f'.tabbody[data-tab="{t}"]:not(.hidden)') is not None, f"↓ reaches {t}")
        key(page, "Enter")
        ctl = focused_control(page)
        check(ctl is not None, f"Enter focuses a control on {t} ({ctl})")
        key(page, "Escape")
        key(page, "Escape")
        check(page.evaluate("() => document.activeElement.classList.contains('sidebar')"), f"Esc returns to the sidebar from {t}")

    # panel toggles from the View menu
    key(page, "Alt+v")
    clk(page, '.menubar [data-item="toggle_keys"]')
    check(not page.is_visible('.tabbody:not(.hidden) .keyspanel'), "View ▸ Toggle Keys panel hides it")
    key(page, "Alt+v")
    clk(page, '.menubar [data-item="toggle_keys"]')
    check(page.is_visible('.tabbody:not(.hidden) .keyspanel'), "and shows it again")

    # theme persists across a reload (localStorage)
    key(page, "Control+k")
    typ(page, "solarized")
    key(page, "Enter")
    page.reload()
    page.wait_for_selector(".menubar .dot-on", timeout=20000)
    check(page.get_attribute("html", "data-theme") == "solarized", "theme remembered across a reload")
    page.screenshot(path=f"{SHOTS}/solarized-text.png")
    key(page, "Control+k")
    typ(page, "theme gruv")
    key(page, "Enter")
    clk(page, '[data-tab-link="kanban"]')
    page.screenshot(path=f"{SHOTS}/gruvbox-kanban.png")
    key(page, "Control+k")
    typ(page, "midnight")
    key(page, "Enter")

    # About shows both implementations
    key(page, "Alt+h")
    clk(page, '.menubar [data-item="about"]')
    about = page.inner_text('[data-popup="dialog"]')
    check("go 0.5.0" in about and "web-go" in about and "Protocol" in about, "About lists server, client and protocol")
    key(page, "Escape")

    # narrow window: still usable
    page.set_viewport_size({"width": 760, "height": 820})
    clk(page, '[data-tab-link="date"]')
    page.screenshot(path=f"{SHOTS}/narrow-date.png")
    sw = page.evaluate("() => document.documentElement.scrollWidth <= window.innerWidth + 1")
    check(sw, "no horizontal page scroll at 760px")
    page.set_viewport_size({"width": 1440, "height": 900})

    # reconnect after a server restart: amber dot, then green; values reloaded
    if RESTART:
        subprocess.run(RESTART, shell=True, check=True)
        check(wait_text(page, ".menubar .conn", "reconnecting", 5) or page.query_selector(".menubar .dot-on") is not None,
              "amber while reconnecting")
        page.wait_for_selector(".menubar .dot-on", timeout=20000)
        check(True, "reconnected after the restart")
        goto_tab(page, "select")
        key(page, "Enter")
        typ(page, "blo")
        key(page, "Enter")
        check(wait_value(page, "select.status", '"blocked"'), "edits work after reconnecting")
    else:
        print("  (skip) reconnect: set DEMO_E2E_RESTART to a command that restarts the server")

    v = csp_violations(page)
    check(not v, "no CSP violations reported: " + "; ".join(v[:3]))
    real = [e for e in real_errors(errors) if "WebSocket" not in e and "ERR_CONNECTION_REFUSED" not in e]
    check(not real, "no console errors: " + "; ".join(real[:4]))
    b.close()
done()
