"""Two users in two browser contexts: live updates, presence, conflicts."""
from playwright.sync_api import sync_playwright, expect
from common import Console, login, shot, check

with sync_playwright() as p:
    b = p.chromium.launch(channel="chrome")
    ca = b.new_context(viewport={"width": 1500, "height": 900})
    cb = b.new_context(viewport={"width": 1500, "height": 900}, color_scheme="dark")
    a, bb = ca.new_page(), cb.new_page()
    con_a, con_b = Console(a, "alice"), Console(bb, "bob")
    login(a, "alice")
    login(bb, "bob")

    # presence: both users online
    expect(a.locator(".statusline .avs .av")).to_have_count(2, timeout=5000)
    check(True, "presence: alice sees 2 users online")

    # both put the cursor on the first project's Name cell
    for pg in (a, bb):
        pg.focus("#g-projects")
        pg.keyboard.press("Control+Home")
        pg.keyboard.press("Home")
        pg.keyboard.press("ArrowRight")
    pid = a.locator("#g-projects .row.currow").get_attribute("data-id")
    cell = f'#g-projects .row[data-id="{pid}"] .cell[data-col="name"]'
    # bob sees alice's presence marker on that row (gutter initials) and cell underline
    expect(bb.locator(f'#g-projects .row[data-id="{pid}"] .pav')).to_be_visible(timeout=5000)
    check(True, "presence: bob sees alice's marker on the row she is on")
    shot(bb, "collab-presence")

    # live update: alice renames, bob's grid updates and flashes, bob gets a toast
    a.keyboard.press("F2"); a.keyboard.press("ControlOrMeta+a"); a.keyboard.type("Live from Alice"); a.keyboard.press("Enter")
    expect(bb.locator(cell)).to_have_text("Live from Alice", timeout=5000)
    check(True, "live update: bob sees alice's edit")
    expect(bb.locator(".toast")).to_contain_text("Alice Nguyen", timeout=3000)
    check(True, "bob gets a toast naming alice")
    a.keyboard.press("ArrowUp")

    # conflict: bob starts editing, alice changes the same cell, bob commits
    bb.keyboard.press("F2")
    bb.wait_for_selector("#g-projects input.ed")
    a.keyboard.press("F2"); a.keyboard.press("ControlOrMeta+a"); a.keyboard.type("Alice wins"); a.keyboard.press("Enter")
    expect(a.locator(cell)).to_have_text("Alice wins", timeout=5000)
    bb.keyboard.press("ControlOrMeta+a"); bb.keyboard.type("Bob loses"); bb.keyboard.press("Enter")
    expect(bb.locator(cell + ".conflict")).to_be_visible(timeout=5000)
    check(bb.locator(cell).inner_text().endswith("Alice wins"), "conflict: bob's cell shows alice's value, marked ⚠")
    expect(bb.locator(".toast.warn")).to_contain_text("your edit wasn't applied", timeout=3000)
    check(True, "conflict toast explains what happened")
    check("conflict" in bb.locator(".statusline").inner_text(), "status line counts the conflict")
    shot(bb, "collab-conflict")
    # Enter on the conflict cell edits from their value; Esc dismisses
    bb.keyboard.press("ArrowUp")
    bb.keyboard.press("Escape")
    expect(bb.locator(cell + ".conflict")).to_have_count(0)
    check(True, "Esc dismisses the conflict marker")

    # undo is per user: alice's ^Z restores her previous value everywhere
    a.focus("#g-projects")
    a.keyboard.press("Control+z")
    expect(bb.locator(cell)).to_have_text("Live from Alice", timeout=5000)
    check(True, "alice's undo propagates to bob")

    # activity view shows the changes live for bob
    bb.keyboard.press("Alt+Digit6")
    expect(bb.locator("#g-activity .row[data-id]").first).to_contain_text("Alice", timeout=5000)
    check(True, "activity feed lists alice's change")

    con_a.assert_clean()
    con_b.assert_clean()
    b.close()
print("collab ok")
