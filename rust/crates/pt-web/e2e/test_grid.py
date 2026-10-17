"""Grid: keyboard navigation and every editor type, query bar, sort, group."""
import re
from playwright.sync_api import sync_playwright, expect
from common import Console, login, shot, check

CELL = '#g-projects .row[data-id="{id}"] .cell[data-col="{col}"]'


def cur(page, grid="g-projects"):
    return page.locator(f"#{grid} .cell.cur")


def goto_col(page, col, grid="g-projects"):
    """Move the cursor to a column by pressing Home then → until it is current."""
    page.keyboard.press("Home")
    for _ in range(40):
        if cur(page, grid).get_attribute("data-col") == col:
            return
        page.keyboard.press("ArrowRight")
    raise AssertionError(f"column {col} not reached")


def cur_row_id(page, grid="g-projects"):
    return page.locator(f"#{grid} .row.currow").get_attribute("data-id")


with sync_playwright() as p:
    b = p.chromium.launch(channel="chrome")
    page = b.new_page(viewport={"width": 1600, "height": 950})
    con = Console(page, "grid")
    login(page, "alice")
    page.focus("#g-projects")

    # navigation
    first = cur_row_id(page)
    page.keyboard.press("ArrowDown")
    second = cur_row_id(page)
    check(first != second, "↓ moves to the next row")
    page.keyboard.press("Control+End")
    last = cur_row_id(page)
    check(last not in (first, second), "^End jumps to the last row")
    page.keyboard.press("Control+Home")
    check(cur_row_id(page) == first, "^Home jumps back")
    page.keyboard.press("ArrowDown")
    pid = cur_row_id(page)

    # text editor: F2, replace, Enter
    goto_col(page, "name")
    page.keyboard.press("F2")
    page.wait_for_selector("#g-projects input.ed")
    page.keyboard.press("ControlOrMeta+a")
    page.keyboard.type("Renamed by e2e")
    page.keyboard.press("Enter")
    expect(page.locator(CELL.format(id=pid, col="name"))).to_have_text("Renamed by e2e", timeout=5000)
    check(True, "text editor saves (F2, type, Enter)")
    check(cur_row_id(page) != pid, "Enter moves down after saving")
    page.keyboard.press("ArrowUp")

    # type-to-edit replaces content
    page.keyboard.type("Z")
    page.wait_for_selector("#g-projects input.ed")
    page.keyboard.press("Escape")
    expect(page.locator(CELL.format(id=pid, col="name"))).to_have_text("Renamed by e2e")
    check(True, "Esc cancels an edit")

    # select editor (status): type a letter to filter, Enter
    goto_col(page, "status")
    page.keyboard.type("on h")
    page.wait_for_selector(".choices .choice.sel")
    shot(page, "edit-select")
    page.keyboard.press("Enter")
    expect(page.locator(CELL.format(id=pid, col="status"))).to_have_text("On hold", timeout=5000)
    check(True, "select editor with type-to-filter")
    page.keyboard.press("ArrowUp")

    # combobox (owner) with fuzzy filter
    goto_col(page, "owner_id")
    page.keyboard.press("Enter")
    page.keyboard.type("bob")
    page.wait_for_selector(".choices .choice.sel .hl")
    shot(page, "edit-combo")
    page.keyboard.press("Tab")
    expect(page.locator(CELL.format(id=pid, col="owner_id"))).to_contain_text("Bob Stone", timeout=5000)
    check(cur(page).get_attribute("data-col") == "status", "Tab saves and moves right")

    # date editor with natural language + calendar
    goto_col(page, "due_date")
    page.keyboard.press("F2")
    page.keyboard.press("ControlOrMeta+a")
    page.keyboard.type("eom")
    page.wait_for_selector(".datepop .preview")
    prev = page.locator(".datepop .preview").inner_text()
    check("→" in prev, f"date preview shows '{prev}'")
    page.keyboard.press("PageDown")  # +1 month
    shot(page, "edit-date")
    page.keyboard.press("Enter")
    page.wait_for_timeout(400)
    due = page.locator(CELL.format(id=pid, col="due_date")).inner_text()
    check(re.match(r"\d{4}-\d{2}-\d{2}", due) is not None, f"date editor saved {due}")
    page.keyboard.press("ArrowUp")

    # number editor with arrow increments
    goto_col(page, "budget_hours")
    before = float(page.locator(CELL.format(id=pid, col="budget_hours")).inner_text() or 0)
    page.keyboard.press("F2")
    page.keyboard.press("ArrowUp"); page.keyboard.press("ArrowUp")
    page.keyboard.press("Enter")
    page.wait_for_timeout(400)
    after = float(page.locator(CELL.format(id=pid, col="budget_hours")).inner_text())
    check(after == before + 2, f"number editor ↑↑ adds 2 ({before} → {after})")

    # chips editor (tags): add an existing and a new tag
    page.keyboard.press("ArrowUp")
    check(cur_row_id(page) == pid, "cursor back on the edited row")
    goto_col(page, "tags")
    page.keyboard.press("F2")
    page.wait_for_selector(".chipsed input")
    page.keyboard.type("pilo"); page.keyboard.press("Enter")
    page.keyboard.type("e2e-tag"); page.keyboard.press("Enter")
    shot(page, "edit-chips")
    page.keyboard.press("Enter")
    expect(page.locator(CELL.format(id=pid, col="tags"))).to_contain_text("e2e-tag", timeout=5000)
    expect(page.locator(CELL.format(id=pid, col="tags"))).to_contain_text("pilot")
    check(True, "chips editor adds existing + new tags")
    page.keyboard.press("ArrowUp")

    # invalid input shows an error and keeps the editor open
    goto_col(page, "start_date")
    page.keyboard.press("F2"); page.keyboard.press("ControlOrMeta+a"); page.keyboard.type("not a date"); page.keyboard.press("Enter")
    check(page.locator("#g-projects input.ed.bad").count() == 1, "invalid date is rejected in place")
    page.keyboard.press("Escape")

    # query bar: filter, live validation, autocomplete
    total = page.locator(".qcount").inner_text()
    page.keyboard.press("Control+f")
    page.keyboard.type("status:active")
    page.wait_for_timeout(300)
    filtered = page.locator(".qcount").inner_text()
    check(filtered != total, f"filter narrows rows ({total} → {filtered})")
    page.keyboard.type(" health:zzz")
    page.wait_for_selector(".qerr")
    check("unknown health" in page.locator(".qmsg").inner_text(), "bad term is underlined with a message")
    page.keyboard.press("ControlOrMeta+a"); page.keyboard.press("Backspace")
    page.keyboard.type("prio")
    page.wait_for_selector(".qsugg .choice")
    shot(page, "query-autocomplete")
    page.keyboard.press("Tab")
    page.keyboard.type("cri")
    page.wait_for_selector(".qsugg .choice")
    page.keyboard.press("Tab")
    val = page.input_value("#query")
    check(val.startswith("priority:critical"), f"autocomplete completes field and value ({val!r})")
    page.keyboard.press("Enter")  # back to the grid
    page.wait_for_timeout(200)
    rows = page.locator("#g-projects .row[data-id]").count()
    crit = page.locator('#g-projects .cell[data-col="priority"]:has-text("Critical")').count()
    check(rows == crit and rows > 0, f"all {rows} rows are critical")
    page.keyboard.press("Control+f"); page.keyboard.press("ControlOrMeta+a"); page.keyboard.press("Backspace"); page.keyboard.press("Escape")

    # sort by header click and ^O; group by ^R
    page.click('#g-projects .hcell:has-text("Due")')
    page.wait_for_timeout(200)
    dues = page.locator('#g-projects .cell[data-col="due_date"]').all_inner_texts()
    dues = [d for d in dues if d]
    check(dues[:10] == sorted(dues[:10]), "header click sorts ascending")
    page.focus("#g-projects")
    goto_col(page, "status")
    page.keyboard.press("Control+r")
    page.wait_for_selector("#g-projects .row.grp")
    groups = page.locator("#g-projects .row.grp").count()
    shot(page, "grouped")
    check(groups >= 2, f"^R groups by status ({groups} group headers rendered)")
    page.keyboard.press("Control+Home")
    page.keyboard.press("Enter")  # collapse the first group
    page.wait_for_timeout(200)
    check(page.locator("#g-projects .row.grp").first.locator(".gtog").inner_text() == "▸", "Enter collapses a group")
    page.keyboard.press("Control+r")

    # mark rows + fill down (^D) on the detail task grid
    page.keyboard.press("F6")
    page.wait_for_timeout(200)
    check(page.evaluate("document.activeElement.id") == "g-projtasks", "F6 moves to the detail grid")
    goto_col(page, "priority", "g-projtasks")
    page.keyboard.press("Control+Home")
    page.keyboard.press("Space"); page.keyboard.press("Shift+ArrowDown"); page.keyboard.press("Shift+ArrowDown")
    marked = page.locator("#g-projtasks .row.marked").count()
    check(marked == 3, f"Space + Shift+↓ marks 3 rows ({marked})")
    page.keyboard.press("Control+d")
    page.wait_for_timeout(500)
    pr = page.locator('#g-projtasks .row.marked .cell[data-col="priority"]').all_inner_texts()
    check(len(set(pr)) == 1, f"^D fills down the first value {pr}")
    # copy + paste
    page.keyboard.press("Escape")
    page.keyboard.press("Control+Home")
    goto_col(page, "phase", "g-projtasks")
    page.evaluate("""() => {
        const dt = new DataTransfer(); dt.setData('text/plain', 'Pasted A\\nPasted B');
        document.getElementById('g-projtasks').dispatchEvent(new ClipboardEvent('paste', {clipboardData: dt, bubbles: true}));
    }""")
    page.wait_for_timeout(500)
    ph = page.locator('#g-projtasks .cell[data-col="phase"]').all_inner_texts()[:2]
    check(ph == ["Pasted A", "Pasted B"], f"TSV paste fills down {ph}")

    # undo the paste (one step)
    page.keyboard.press("Control+z")
    page.wait_for_timeout(500)
    ph2 = page.locator('#g-projtasks .cell[data-col="phase"]').all_inner_texts()[:2]
    check(ph2 != ["Pasted A", "Pasted B"], f"^Z undoes the whole paste {ph2}")
    page.keyboard.press("Control+y")
    page.wait_for_timeout(500)
    ph3 = page.locator('#g-projtasks .cell[data-col="phase"]').all_inner_texts()[:2]
    check(ph3 == ["Pasted A", "Pasted B"], "^Y redoes it")

    con.assert_clean()
    b.close()
print("grid ok")
