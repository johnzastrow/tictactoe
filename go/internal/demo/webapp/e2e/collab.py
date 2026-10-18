"""Two people at once: presence, a change from one page arriving live on
the other (with flash and status line), a conflict on a text field, a
broadcast toast and a kanban move."""
from common import *

with sync_playwright() as p:
    b = launch(p)
    errs_a, errs_b = [], []
    ca, alice = new_page(b, errs_a, 1400, 860)
    cb, bob = new_page(b, errs_b, 1400, 860)
    join(alice, "alice")
    reset_all(alice)
    join(bob, "bob")
    check(wait_text(alice, ".statusline", "online: 2"), "alice sees 2 online")
    check(wait_text(bob, ".statusline", "online: 2"), "bob sees 2 online")

    # presence: bob goes to Dropdown; alice sees his dot next to that tab
    goto_tab(alice, "select")
    goto_tab(bob, "select")
    wait(alice, 400)
    check(alice.query_selector('[data-tab-link="select"] .pdot') is not None, "alice sees bob's dot on the Dropdown tab")
    title = alice.get_attribute('.statusline .av:not(.me)', "title") or ""
    check("bob" in title and "Dropdown" in title, f"presence initials say where bob is ({title})")

    # live change: bob picks Review; alice's control and Value panel follow, flashing
    key(bob, "Enter")
    typ(bob, "rev")
    key(bob, "Enter")
    check(wait_value(alice, "select.status", '"review"'), "alice sees bob's change live")
    check("bob changed select.status" in status(alice), "alice's status line names bob")
    check(alice.query_selector('[data-tab="select"] .vrow.flash') is not None, "the Value panel flashes")
    check("Review" in alice.inner_text('[data-control="status"]'), "alice's dropdown shows the new value")
    check("by bob" in alice.inner_text('[data-tab="select"] .vrow[data-key="select.status"] .vdesc'), "Value panel: by bob")
    alice.screenshot(path=f"{SHOTS}/collab-alice-live.png")

    # conflict: alice edits Name; bob saves Name first; alice is warned, Enter → conflict → theirs
    goto_tab(alice, "text")
    goto_tab(bob, "text")
    key(alice, "ControlOrMeta+a")
    typ(alice, "Alice's name")
    key(bob, "ControlOrMeta+a")
    typ(bob, "Bob's name")
    key(bob, "Enter")
    check(wait_text(alice, '[data-tab="text"] .ctlarea', "changed by bob"), "alice's field shows ⚠ changed by bob")
    check(alice.input_value('[data-control="name"]') == "Alice's name", "alice keeps her text meanwhile")
    key(alice, "Enter")
    check(wait_text(alice, ".statusline", "bob changed text.name first, showing theirs"), "conflict reported in the status line")
    check(alice.input_value('[data-control="name"]') == "Bob's name", "the control shows their value")
    check(wait_value(bob, "text.name", "Bob's name"), "bob's value stands")
    alice.screenshot(path=f"{SHOTS}/collab-alice-conflict.png")
    # Esc takes theirs
    key(alice, "ControlOrMeta+a")
    typ(alice, "Draft")
    key(bob, "ControlOrMeta+a")
    typ(bob, "Bob again")
    key(bob, "Enter")
    wait_text(alice, '[data-tab="text"] .ctlarea', "changed by bob")
    key(alice, "Escape")
    check(alice.input_value('[data-control="name"]') == "Bob again", "Esc takes theirs")

    # broadcast toast from bob reaches alice
    goto_tab(bob, "dialogs")
    key(bob, "End")
    for _ in range(4):
        key(bob, "ArrowRight")
    key(bob, "Enter")
    typ(bob, "Lunch is here")
    key(bob, "Enter")
    check(wait_text(alice, ".toasts", "Lunch is here"), "alice gets bob's broadcast toast")
    check("bob" in alice.inner_text(".toasts"), "toast names the sender")

    # kanban: bob moves a card; alice sees it in the new column
    goto_tab(alice, "kanban")
    goto_tab(bob, "kanban")
    key(bob, "Shift+ArrowRight")
    wait(alice, 500)
    doing = alice.inner_text('[data-tab="kanban"] [data-col="doing"]')
    check("User test" in doing, "alice sees bob's card move")
    alice.screenshot(path=f"{SHOTS}/collab-alice-kanban.png")

    for name, pg, errs in (("alice", alice, errs_a), ("bob", bob, errs_b)):
        v = csp_violations(pg)
        real = real_errors(errs)
        check(not v and not real, f"{name}: no console errors or CSP violations: " + "; ".join((v + real)[:4]))
    b.close()
done()
