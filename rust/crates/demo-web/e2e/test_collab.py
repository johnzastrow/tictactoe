"""Two browsers: live changes, presence dots, flashes, conflicts, toasts."""
from playwright.sync_api import sync_playwright, expect
from common import Console, check, expect_value, focus, goto_tab, join, launch, shot, value

with sync_playwright() as p:
    b = launch(p)
    ca = b.new_context(viewport={"width": 1400, "height": 900})
    cb = b.new_context(viewport={"width": 1400, "height": 900})
    a, bb = ca.new_page(), cb.new_page()
    con_a, con_b = Console(a, "alice"), Console(bb, "bob")
    join(a, "alice")
    join(bb, "bob")

    expect(a.locator(".statusline .av")).to_have_count(2, timeout=5000)
    check(a.locator(".statusline .online").inner_text() == "online: 2", "presence: both online, initials shown")

    # focus messages: bob sees alice's dot on the tab she is on
    goto_tab(a, "select")
    expect(bb.locator("[data-tab-link='select'] .pdot")).to_have_count(1, timeout=5000)
    check(True, "presence: bob sees alice's dot on Dropdown")

    # a change from alice arrives live at bob, flashes, and the status says who
    goto_tab(bb, "select")
    focus(a, "select.status")
    a.keyboard.press("Enter"); a.keyboard.type("done"); a.keyboard.press("Enter")
    expect_value(bb, "select.status", "done")
    expect(bb.locator(".statusline")).to_contain_text("alice changed select.status")
    check(bb.locator(".vrow[data-key='select.status'] .vmeta").inner_text().startswith("v2 · by alice"), "live: bob's Value panel shows v2 · by alice")
    expect(bb.locator("[data-control='select.status']")).to_contain_text("Done")
    shot(bb, "collab-live")

    # editing while someone else changes: keep the draft, warn; Esc takes theirs
    goto_tab(a, "text")
    goto_tab(bb, "text")
    focus(bb, "text.name")
    bb.keyboard.press("ControlOrMeta+a"); bb.keyboard.type("Bob's name")
    focus(a, "text.name")
    a.keyboard.press("ControlOrMeta+a"); a.keyboard.type("Alice's name"); a.keyboard.press("Enter")
    expect(bb.locator(".fwarn")).to_contain_text("changed by alice", timeout=5000)
    check(bb.locator("[data-control='text.name']").input_value() == "Bob's name", "conflict: bob keeps his text, sees ⚠ changed by alice")
    # committing the stale draft is a conflict: bob gets alice's value
    bb.keyboard.press("Enter")
    expect(bb.locator(".statusline")).to_contain_text("alice changed text.name first, showing theirs", timeout=5000)
    expect(bb.locator("[data-control='text.name']")).to_have_value("Alice's name")
    check(value(bb, "text.name") == "Alice's name", "conflict: the server kept alice's value")
    shot(bb, "collab-conflict")

    # broadcast toast reaches everyone
    goto_tab(a, "dialogs")
    a.click("[data-control='dialogs.broadcast']")
    a.keyboard.type("Standup in 5"); a.keyboard.press("Enter")
    expect(bb.locator(".toast")).to_contain_text("alice: Standup in 5")
    check(True, "broadcast toast arrives at bob")

    # reset by bob shows up at alice
    goto_tab(bb, "select")
    bb.keyboard.press("ControlOrMeta+r"); bb.keyboard.press("y")
    goto_tab(a, "select")
    expect_value(a, "select.status", "in_progress")
    check(True, "bob's ^R reset reaches alice")

    # bob leaves: alice sees one online
    bb.click(".menubar.top [data-menu='File']")
    bb.click("[data-popup='menu'] .item:has-text('Leave')")
    expect(a.locator(".statusline .online")).to_have_text("online: 1", timeout=5000)
    check(True, "presence: leaving updates everyone")

    con_a.assert_clean()
    con_b.assert_clean()
    b.close()
print("collab ok")
