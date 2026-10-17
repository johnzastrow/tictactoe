"""Keyboard-driven editing: every editor type, filter, sort/group, undo/redo,
cascading New Project, palette, record form, fill-down, paste."""
import re
import datetime
from common import *


def cell_text(page, row_code, col_title):
    """Text of the cell in the row whose first column is row_code."""
    return page.evaluate(
        """([code, title]) => {
            const g = document.querySelector('.pane.main .grid');
            const heads = [...g.querySelectorAll('.ghead .hcell')].map(h => h.querySelector('.ht').textContent);
            const j = heads.indexOf(title);
            for (const r of g.querySelectorAll('.gbody .row')) {
                if (r.style.display === 'none') continue;
                const cells = r.querySelectorAll('.cell');
                if (cells[0] && cells[0].textContent.trim() === code) return cells[j].textContent.trim();
            }
            return null;
        }""",
        [row_code, col_title],
    )


def goto_col(page, title):
    """Move the cursor to the column with this title (Home, then right)."""
    key(page, "Home")
    titles = page.eval_on_selector_all(".grid:focus-within .ghead .hcell .ht", "els => els.map(e => e.textContent)")
    for _ in range(titles.index(title)):
        page.keyboard.press("ArrowRight")
    wait_idle(page, 100)


def editor(page):
    return page.query_selector(".edroot:not(.embedded) input, .edroot:not(.embedded) textarea")


with sync_playwright() as p:
    b = p.chromium.launch(channel="chrome")
    errors = []
    ctx, page = new_page(b, errors)
    login(page, "alice")
    key(page, "Alt+Digit1")
    page.click(".pane.main .gbody .row .cell >> nth=1")  # first row, Name
    key(page, "Control+Home")
    first_code = page.inner_text(".pane.main .gbody .row .cell >> nth=0").strip()
    check(first_code == "CTH-001", f"first row {first_code}")

    # --- text editor ---
    goto_col(page, "Name")
    key(page, "Enter")
    check(editor(page) is not None, "Enter opens the text editor")
    page.keyboard.type("Patient portal redesign v2")
    key(page, "Enter")
    page.wait_for_timeout(500)
    check(cell_text(page, "CTH-001", "Name") == "Patient portal redesign v2", "text edit saved")
    check("Saved" in status(page), "status says Saved: " + status(page))
    # Enter moved the cursor down
    check(page.inner_text(".pane.main .row.currow .cell >> nth=0").strip() == "CTH-002", "Enter moves down")
    key(page, "ArrowUp")

    # --- select editor (type to filter) ---
    goto_col(page, "Status")
    page.keyboard.type("on")
    page.wait_for_selector(".popup .choice.sel")
    sel = page.inner_text(".popup .choice.sel .lbl")
    check(sel == "On hold", f"typing filters the dropdown ({sel})")
    key(page, "Enter")
    page.wait_for_timeout(400)
    check(cell_text(page, "CTH-002", "Status") == "On hold" or cell_text(page, "CTH-001", "Status") == "On hold", "select saved")
    key(page, "ArrowUp")
    cur = page.inner_text(".pane.main .row.currow .cell >> nth=0").strip()
    check(cur == "CTH-001", f"back on CTH-001 ({cur})")

    # --- combobox (fuzzy) ---
    goto_col(page, "Owner")
    page.keyboard.type("bob")
    page.wait_for_selector(".popup .choice.sel")
    hl = page.inner_text(".popup .choice.sel .lbl")
    check(hl == "Bob Stone", f"fuzzy combobox ranks Bob first ({hl})")
    check(page.query_selector(".popup .choice.sel .hl") is not None, "matched chars highlighted")
    key(page, "Tab")  # pick & move right
    page.wait_for_timeout(400)
    check(cell_text(page, "CTH-001", "Owner") == "Bob Stone", "combobox saved")

    # --- date editor with natural language ---
    goto_col(page, "Due")
    key(page, "Enter")  # opening selects the current text
    page.keyboard.type("+30d")
    page.wait_for_selector(".popup.datepop .preview")
    prev = page.inner_text(".popup.datepop .preview")
    check("→" in prev, f"date preview ({prev})")
    key(page, "ArrowDown")  # +1 week in the calendar
    key(page, "Enter")
    page.wait_for_timeout(400)
    want = (datetime.date.today() + datetime.timedelta(days=37)).isoformat()
    got = cell_text(page, "CTH-002", "Due")
    got1 = cell_text(page, "CTH-001", "Due")
    check(want in (got, got1), f"date saved {want} ({got1})")
    key(page, "ArrowUp")

    # --- number editor ---
    goto_col(page, "Budget h")
    page.keyboard.type("4")
    page.keyboard.type("20")
    key(page, "ArrowUp")  # +1 → 421
    key(page, "Enter")
    page.wait_for_timeout(400)
    check(cell_text(page, "CTH-001", "Budget h") == "421", f"number ↑ and save ({cell_text(page, 'CTH-001', 'Budget h')})")
    key(page, "ArrowUp")

    # --- chips editor ---
    goto_col(page, "Tags")
    key(page, "Enter")
    page.keyboard.type("e2e-tag")
    key(page, "Enter")  # adds a chip
    check(page.query_selector(".edroot .chipx") is not None, "chip added")
    key(page, "Tab")  # saves
    page.wait_for_timeout(400)
    tags = cell_text(page, "CTH-001", "Tags") or ""
    check("e2e-tag" in tags, f"chips saved ({tags})")

    # --- undo / redo ---
    key(page, "Control+z")
    page.wait_for_timeout(500)
    tags = cell_text(page, "CTH-001", "Tags") or ""
    check("e2e-tag" not in tags, f"undo removed the tag ({tags})")
    key(page, "Control+y")
    page.wait_for_timeout(500)
    tags = cell_text(page, "CTH-001", "Tags") or ""
    check("e2e-tag" in tags, f"redo re-added the tag ({tags})")

    # --- escape cancels ---
    goto_col(page, "Name")
    page.keyboard.type("zzz")
    key(page, "Escape")
    check(cell_text(page, "CTH-001", "Name") == "Patient portal redesign v2", "Esc cancels")

    # --- filter bar ---
    key(page, "Control+f")
    page.keyboard.type("status:active")
    page.wait_for_timeout(300)
    cnt = page.inner_text(".qcount")
    check(re.search(r"\[ (\d+) / 50 \]", cnt) and int(re.search(r"\[ (\d+)", cnt).group(1)) < 50, f"filter narrows ({cnt})")
    page.keyboard.type(" frob:1")
    page.wait_for_timeout(300)
    check("unknown field" in page.inner_text(".qmsg"), "bad term reported")
    check(page.query_selector(".qback .qerr") is not None, "bad term underlined")
    page.fill("#query", "")
    page.keyboard.type("@m")
    wait_text(page, ".qsugg", "@me")
    check("@me" in page.inner_text(".qsugg"), "autocomplete offers @me")
    key(page, "Tab")
    check(page.input_value("#query").startswith("@me"), "Tab accepts suggestion")
    page.fill("#query", "")
    page.keyboard.type(" ")
    page.keyboard.press("Backspace")
    key(page, "Enter")  # back to grid

    # --- sort & group ---
    goto_col(page, "Program")
    key(page, "Control+r")
    page.wait_for_timeout(300)
    check(page.query_selector(".pane.main .row.grp") is not None, "^R groups by column")
    check("Program" in page.inner_text(".qind"), "group indicator")
    key(page, "Control+Home")
    key(page, "ArrowLeft")  # collapse first group
    page.wait_for_timeout(200)
    check("▸" in page.inner_text(".pane.main .row.grp >> nth=0"), "← collapses a group")
    key(page, "Control+r")
    goto_col(page, "Budget h")
    key(page, "Control+o")
    key(page, "Control+o")
    page.wait_for_timeout(300)
    check("▼" in page.inner_text(".pane.main .ghead"), "^O sorts descending")
    key(page, "Control+o")  # off
    page.click(".pane.main .ghead .hcell >> nth=0")  # sort by code again
    page.wait_for_timeout(300)

    # --- palette: go to a project ---
    key(page, "Control+k")
    page.keyboard.type("go to fab-002")
    page.wait_for_timeout(200)
    key(page, "Enter")
    page.wait_for_timeout(500)
    check(page.inner_text(".pane.main .row.currow .cell >> nth=0").strip() == "FAB-002", "palette jumps to FAB-002")

    # --- record form ---
    key(page, "Control+e")
    page.wait_for_selector(".dialog.form")
    check("FAB-002" in page.inner_text(".dialog .dhead"), "record form opens")
    check(len(page.query_selector_all(".dialog .frow")) >= 15, "all fields + custom fields")
    key(page, "Escape")
    page.wait_for_timeout(200)
    check(page.query_selector(".dialog") is None, "Esc closes the form")

    # --- cascading New project ---
    key(page, "Insert")
    page.wait_for_selector(".dialog.form")
    page.wait_for_timeout(200)
    page.keyboard.type("northw")
    page.wait_for_timeout(300)
    page.wait_for_selector(".popup:not(.hidden) .choice.sel")
    key(page, "Enter")
    code = page.eval_on_selector_all(".dialog .frow input", "els => els[1].value")
    check(code.startswith("NWR-0"), f"code pre-filled from program ({code})")
    key(page, "Tab")
    key(page, "Tab")
    page.keyboard.type("E2E pilot project")
    key(page, "Tab")
    page.keyboard.type("data mig")
    page.wait_for_selector(".popup:not(.hidden) .choice.sel")
    key(page, "Enter")
    page.wait_for_timeout(200)
    info = page.inner_text(".dialog .finfo")
    check("Template creates 9 tasks" in info, f"template info ({info})")
    key(page, "Control+s")
    page.wait_for_timeout(900)
    check(page.query_selector(".dialog") is None, "dialog closed after create")
    cur = page.inner_text(".pane.main .row.currow .cell >> nth=0").strip()
    check(cur == code, f"cursor on the new project ({cur})")
    n = page.inner_text(".pane.detail .pcount")
    check(n.startswith("9 "), f"template tasks created ({n})")

    # --- fill down in the task detail grid ---
    key(page, "F6")
    page.wait_for_timeout(200)
    goto_col(page, "Phase")
    page.keyboard.press("Space")
    key(page, "Shift+ArrowDown", 2)
    check("3 marked" in page.inner_text(".qind"), "Shift+↓ marks rows")
    key(page, "Control+d")
    page.wait_for_timeout(500)
    phases = page.eval_on_selector_all(".pane.detail .gbody .row", "rs => rs.slice(0,3).map(r => r.querySelectorAll('.cell')[2].textContent)")
    check(len(set(phases)) == 1, f"fill down copied the first value ({phases})")

    # --- copy / paste TSV ---
    key(page, "Escape")
    key(page, "Control+Home")
    goto_col(page, "Est h")
    page.evaluate("""() => {
        const dt = new DataTransfer(); dt.setData('text/plain', '7\\n8\\n9');
        document.dispatchEvent(new ClipboardEvent('paste', {clipboardData: dt, bubbles: true}));
    }""")
    page.wait_for_timeout(600)
    est = page.eval_on_selector_all(".pane.detail .gbody .row", """rs => {
        const heads = [...document.querySelectorAll('.pane.detail .ghead .ht')].map(h => h.textContent);
        const j = heads.indexOf('Est h');
        return rs.slice(0,3).map(r => r.querySelectorAll('.cell')[j].textContent);
    }""")
    check(est == ["7", "8", "9"], f"paste TSV fills down ({est})")

    # --- delete with confirm, then undo ---
    key(page, "F6")
    key(page, "F8")
    page.wait_for_selector(".dialog.confirm")
    key(page, "y")
    page.wait_for_timeout(600)
    check(cell_text(page, code, "Code") is None, "project deleted")
    key(page, "Control+z")
    page.wait_for_timeout(600)
    check(cell_text(page, code, "Code") == code, "undo restores the project")
    # clean up: delete it again for repeatable runs
    key(page, "F8")
    page.wait_for_selector(".dialog.confirm")
    key(page, "y")
    page.wait_for_timeout(500)

    real = [e for e in errors if "401" not in e]
    check(not real, "no console errors: " + "; ".join(real[:5]))
    page.screenshot(path=f"{SHOTS}/interact-end.png")
    b.close()
done()
