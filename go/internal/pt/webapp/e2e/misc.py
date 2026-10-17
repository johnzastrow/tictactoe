"""Context menu, saved filters, Alt menus, cascading New task, admin edits,
templates mini-Gantt, and reconnect after the server restarts."""
import os
import subprocess
import time
from common import *

SERVER_CMD = os.environ.get("PT_E2E_RESTART")  # optional: a command that restarts the server keeping the DB

with sync_playwright() as p:
    b = p.chromium.launch(channel="chrome")
    errors = []
    ctx, page = new_page(b, errors, 1600, 900)
    login(page, "admin")

    # context menu → filter by this value
    page.click(".pane.main .gbody .row >> nth=2 >> .cell >> nth=2", button="right")  # Program cell
    page.wait_for_selector(".ctxmenu")
    page.click(".ctxmenu .item:has-text('Filter by this value')")
    page.wait_for_timeout(300)
    q = page.input_value("#query")
    check(q.startswith("program:"), f"context menu adds a filter ({q})")
    # saved filter via the Data menu (Alt+D)
    page.click(".pane.main .gbody .row .cell >> nth=0")
    key(page, "Alt+d")
    check("Save filter" in page.inner_text(".menu.open"), "Alt+D opens the Data menu")
    page.click(".menu.open .item:has-text('Save filter')")
    page.wait_for_selector(".dialog.form")
    page.keyboard.type("My program")
    key(page, "Enter")
    key(page, "Control+s")
    page.wait_for_timeout(300)
    page.fill("#query", "")
    key(page, "Enter")
    key(page, "Control+k")
    page.keyboard.type("my program")
    page.wait_for_timeout(200)
    key(page, "Enter")
    page.wait_for_timeout(300)
    check(page.input_value("#query") == q, "saved filter reapplied from the palette")
    page.fill("#query", "")
    key(page, "Enter")

    # cascading New task from the Tasks view: Program → Project → Phase
    key(page, "Alt+Digit2")
    page.wait_for_timeout(300)
    key(page, "Insert")
    page.wait_for_selector(".dialog.form")
    page.click(".dialog .frow input >> nth=0")  # Program
    page.keyboard.type("fabrikam")
    page.wait_for_timeout(150)
    key(page, "Enter")
    key(page, "Tab")  # Project: its list opens on focus
    page.wait_for_timeout(200)
    labels = page.eval_on_selector_all(".popup:not(.hidden) .choice .lbl", "els => els.map(e => e.textContent)")
    check(labels and all(l.startswith("FAB-") for l in labels), f"projects cascade from the program ({labels[:3]})")
    page.keyboard.type("outage")
    page.wait_for_timeout(150)
    key(page, "Enter")
    key(page, "Tab")
    page.wait_for_timeout(150)
    phases = page.eval_on_selector_all(".popup:not(.hidden) .choice .lbl", "els => els.map(e => e.textContent)")
    check("Collect" in phases or "Process" in phases, f"phases cascade from the project's template ({phases[:4]})")
    page.keyboard.type("Process")
    key(page, "Tab")
    page.keyboard.type("Cascade test task")
    key(page, "Control+s")
    page.wait_for_timeout(800)
    check(page.query_selector(".dialog") is None, "task created")
    check("Cascade test task" in page.inner_text(".pane.main .row.currow"), "cursor on the new task")

    # templates (admin can edit) + mini Gantt
    key(page, "Alt+Digit7")
    page.wait_for_timeout(400)
    check(page.query_selector(".minigantt svg rect") is not None, "template mini-Gantt drawn")
    page.click(".pane.detail .gbody .row .cell >> nth=1")
    key(page, "Enter")
    page.keyboard.type("Kickoff (renamed)")
    key(page, "Enter")
    page.wait_for_timeout(500)
    check("Kickoff (renamed)" in page.inner_text(".pane.detail .gbody") or "renamed" in page.inner_text(".pane.detail"), "admin edits a template task")

    # users: record form → set password dialog exists
    key(page, "Alt+Digit9")
    page.wait_for_timeout(300)
    page.click(".pane.main .gbody .row >> nth=1 >> .cell >> nth=1")
    key(page, "Control+e")
    page.wait_for_selector(".dialog.form")
    check(page.query_selector(".dialog button:has-text('Set password')") is not None, "admin can set a user's password")
    key(page, "Escape")

    # reconnect
    if SERVER_CMD:
        subprocess.run(SERVER_CMD, shell=True, check=True)
        page.wait_for_timeout(300)
        t0 = time.time()
        ok = False
        while time.time() - t0 < 10:
            if page.query_selector(".menubar .dot-on") and not page.query_selector(".offline:not(.hidden)"):
                ok = True
                break
            page.wait_for_timeout(200)
        check(ok, f"reconnects after a server restart ({time.time() - t0:.1f}s)")

    real = [e for e in errors if "401" not in e and "ERR_CONNECTION_REFUSED" not in e and "WebSocket" not in e]
    check(not real, "no console errors: " + "; ".join(real[:4]))
    b.close()
done()
