"""Dialogs (cascading New project / New task, record form), board drag & drop,
timeline moves, dashboard drill-down, palette, context menu, columns,
read-only viewer, logout."""
from playwright.sync_api import sync_playwright, expect
from common import Console, login, shot, check


def field(page, label):
    return page.locator(f'.dialog .fld:has(.flabel:text-is("{label}")) input').first


with sync_playwright() as p:
    b = p.chromium.launch(channel="chrome")
    page = b.new_page(viewport={"width": 1600, "height": 950})
    con = Console(page, "views")
    login(page, "alice")
    page.focus("#g-projects")

    # ---- New project (cascading) ----
    page.keyboard.press("Insert")
    page.wait_for_selector(".dialog.newproj")
    prog = field(page, "Program")
    prog.click()
    page.keyboard.type("munic")
    page.keyboard.press("Enter")
    expect(field(page, "Code")).to_have_value("GIS-008")
    check(True, "program → code cascades to the next free code (GIS-008)")
    field(page, "Name").fill("E2E migration project")
    field(page, "Template").click()
    page.keyboard.type("data mig")
    page.keyboard.press("Enter")
    expect(page.locator(".dialog .fhint", has_text="9 tasks")).to_be_visible()
    check(True, "template shows its stats (9 tasks)")
    due = field(page, "Due").input_value()
    check(due != "", f"due defaults from template span ({due})")
    shot(page, "dlg-newproject")
    page.keyboard.press("ControlOrMeta+s")
    expect(page.locator(".toast", has_text="Created GIS-008 with 9 tasks")).to_be_visible(timeout=5000)
    check(True, "^S creates the project with its template tasks")
    page.wait_for_timeout(400)
    expect(page.locator("#g-projects .row.currow .cell[data-col=code]")).to_have_text("GIS-008", timeout=3000)
    check(True, "cursor jumps to the new project")
    check(page.locator("#g-projtasks .row[data-id]").count() == 9, "detail grid shows its 9 tasks")
    new_pid = page.locator("#g-projects .row.currow").get_attribute("data-id")
    row = f'#g-projects .row[data-id="{new_pid}"]' 

    # ---- undo / redo the create ----
    page.focus("#g-projects")
    page.keyboard.press("Control+z")
    expect(page.locator(row)).to_have_count(0, timeout=5000)
    check(True, "^Z undoes the create")
    page.keyboard.press("Control+y")
    expect(page.locator(row)).to_have_count(1, timeout=5000)
    expect(page.locator(row + ' .cell[data-col="code"]')).to_have_text("GIS-008")
    check(True, "^Y redoes it")

    # ---- New task with the project preset; phases cascade from its template ----
    page.keyboard.press("Alt+Digit2")
    page.wait_for_timeout(300)
    page.keyboard.press("Insert")
    page.wait_for_selector(".dialog.newtask")
    before = field(page, "Project").input_value()
    field(page, "Program").click()
    page.keyboard.type("munic")
    page.keyboard.press("Enter")
    page.wait_for_timeout(100)
    check(field(page, "Project").input_value() == "", f"changing program clears a project from another program ({before!r})")
    field(page, "Project").click()
    page.wait_for_selector(".dialog .choices .choice")
    opts = page.locator(".dialog .choices .choice .lbl").all_inner_texts()
    check(all(o.startswith("GIS-") for o in opts), f"project list is filtered by program ({len(opts)} GIS projects)")
    page.keyboard.type("008")
    page.keyboard.press("Enter")
    field(page, "Phase").click()
    page.wait_for_selector(".dialog .choices .choice")
    phases = page.locator(".dialog .choices .choice .lbl").all_inner_texts()
    check(phases[:4] == ["Assess", "Map", "Migrate", "Validate"], f"phase list comes from the project's template {phases}")
    page.keyboard.press("ArrowDown"); page.keyboard.press("Enter")
    field(page, "Title").fill("E2E task")
    ass = field(page, "Assignee").input_value()
    check(ass == "Alice Nguyen", f"assignee defaults to the project owner ({ass})")
    field(page, "Due").fill("fri")
    expect(page.locator('.dialog .fld:has(.flabel:text-is("Due")) .dprev')).to_contain_text("Fri")
    shot(page, "dlg-newtask")
    page.keyboard.press("ControlOrMeta+s")
    expect(page.locator(".toast", has_text="Created GIS-008 #10")).to_be_visible(timeout=5000)
    check(True, "new task created as GIS-008 #10")

    # ---- record form: edit notes, save; card shows it ----
    page.focus("#g-tasks")
    page.keyboard.press("Control+e")
    page.wait_for_selector(".dialog.record")
    page.locator('.dialog .fld:has(.flabel:text-is("Notes")) textarea').fill("Notes from the record form")
    page.keyboard.press("ControlOrMeta+s")
    expect(page.locator(".dialog")).to_have_count(0, timeout=5000)
    expect(page.locator(".card .notes")).to_have_text("Notes from the record form", timeout=5000)
    check(True, "record form saves; the task card shows the notes")

    # ---- context menu → filter by value ----
    page.click('#g-tasks .row.currow .cell[data-col="status"]', button="right")
    page.wait_for_selector(".ctxmenu")
    shot(page, "ctxmenu")
    page.click(".ctxmenu .item:has-text('Filter by status:')")
    q = page.input_value("#query")
    check(q.startswith("status:"), f"context menu adds a filter term ({q!r})")
    page.keyboard.press("Control+f"); page.keyboard.press("ControlOrMeta+a"); page.keyboard.press("Backspace"); page.keyboard.press("Escape")

    # ---- columns dialog: hide a column with Space ----
    page.focus("#g-tasks")
    n0 = page.locator("#g-tasks .ghead .hcell").count()
    page.keyboard.press("F4")
    page.wait_for_selector(".colsdlg .collist")
    page.keyboard.press("ArrowDown"); page.keyboard.press("ArrowDown"); page.keyboard.press("ArrowDown")
    page.keyboard.press(" ")
    page.keyboard.press("Enter")
    n1 = page.locator("#g-tasks .ghead .hcell").count()
    check(n1 == n0 - 1, f"F4 + Space hides a column ({n0} → {n1})")
    page.keyboard.press("F4"); page.keyboard.press("r"); page.keyboard.press("Enter")

    # ---- palette ----
    page.keyboard.press("Control+k")
    page.wait_for_selector(".palette input:focus")
    page.keyboard.type("timeline")
    page.keyboard.press("Enter")
    expect(page.locator(".tab.active")).to_contain_text("Timeline")
    check(True, "^K palette runs 'Go to Timeline'")

    # ---- timeline: keyboard move and mouse drag ----
    page.focus("#timeline")
    page.keyboard.press("Shift+ArrowRight")
    expect(page.locator(".smsg")).to_contain_text("Moved +7 days", timeout=5000)
    check(True, "Shift+→ moves the selected bar one week")
    page.keyboard.press("Control+z")
    page.wait_for_timeout(300)
    page.keyboard.press("ArrowRight")  # expand first project
    page.wait_for_timeout(200)
    check(page.locator(".tl-row").count() > 50, "→ expands a project into its tasks")
    bar = page.locator(".tl-svg .bar-g rect.pb").nth(3)
    box = bar.bounding_box()
    page.mouse.move(box["x"] + 20, box["y"] + 5)
    page.mouse.down()
    page.mouse.move(box["x"] + 60, box["y"] + 5, steps=5)
    page.mouse.up()
    expect(page.locator(".smsg")).to_contain_text("Moved", timeout=5000)
    check(True, "dragging a Gantt bar moves it")
    shot(page, "timeline-expanded")

    # ---- board: drag and drop + keyboard move ----
    page.keyboard.press("Alt+Digit3")
    page.wait_for_selector('.bcol[data-status="todo"] .card2')
    card = page.locator('.bcol[data-status="todo"] .card2').first
    cid = card.get_attribute("data-id")
    card.drag_to(page.locator('.bcol[data-status="review"]').first, target_position={"x": 60, "y": 30})
    expect(page.locator(f'.bcol[data-status="review"] .card2[data-id="{cid}"]')).to_have_count(1, timeout=5000)
    check(True, "drag & drop moves a card to Review")
    page.focus("#board")
    page.locator(f'.card2[data-id="{cid}"]').click()
    page.keyboard.press("Shift+ArrowRight")
    expect(page.locator(f'.bcol[data-status="done"] .card2[data-id="{cid}"]')).to_have_count(1, timeout=5000)
    check(True, "Shift+→ moves the card to Done")
    page.keyboard.press("Control+r")
    page.wait_for_selector(".lane-h")
    check(page.locator(".lane-h").count() >= 5, "^R shows swimlanes by assignee")
    shot(page, "board-lanes")
    page.keyboard.press("Control+r")

    # ---- dashboard drill-down ----
    page.keyboard.press("Alt+Digit5")
    page.click(".kpi:has-text('Overdue tasks')")
    expect(page.locator(".tab.active")).to_contain_text("Tasks")
    check(page.input_value("#query") == "is:overdue", "KPI tile drills into Tasks with is:overdue")

    # ---- saved filter: save, clear, re-apply from the Data menu ----
    page.keyboard.press("Control+f"); page.keyboard.press("ControlOrMeta+a"); page.keyboard.type("@me is:open"); page.keyboard.press("Enter")
    page.keyboard.press("Alt+KeyD"); page.click(".drop .item:has-text('Save filter')")
    page.fill(".savedlg input", "My open work"); page.keyboard.press("ControlOrMeta+s")
    page.keyboard.press("Control+f"); page.keyboard.press("ControlOrMeta+a"); page.keyboard.press("Backspace"); page.keyboard.press("Escape")
    page.keyboard.press("Alt+KeyD"); page.click(".drop .item:has-text('My open work')")
    check(page.input_value("#query").strip() == "@me is:open", "saved filter re-applies from the Data menu")

    # ---- help search ----
    page.keyboard.press("F1")
    page.fill(".help input", "fill down")
    rows = page.locator(".help .hrowk").count()
    check(rows >= 1 and rows < 5, f"help search filters the keymap ({rows} rows)")
    page.keyboard.press("Escape")

    # ---- logout via File menu ----
    page.keyboard.press("Alt+KeyF")
    page.wait_for_selector(".drop .item")
    page.click(".drop .item:has-text('Log out')")
    page.wait_for_selector("#login-user", timeout=5000)
    check(True, "File ▸ Log out returns to the login page")

    # ---- read-only viewer ----
    login(page, "victor")
    page.focus("#g-projects")
    page.keyboard.press("F2")
    check(page.locator("#g-projects input.ed").count() == 0, "viewer can't open an editor")
    expect(page.locator(".smsg")).to_contain_text("read-only")
    check(page.locator(".lock").count() >= 1, "grids show a read-only badge for viewers")

    con.assert_clean()
    b.close()
print("views ok")
