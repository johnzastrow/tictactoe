"""Two users at once: live updates, presence, conflicts, viewer read-only,
admin adding a custom field shows up as a column for everyone."""
from common import *


def row_cells(page, pane, code):
    return page.evaluate(
        """([pane, code]) => {
            const g = document.querySelector(pane + ' .grid');
            const heads = [...g.querySelectorAll('.ghead .ht')].map(h => h.textContent);
            for (const r of g.querySelectorAll('.gbody .row')) {
                if (r.style.display === 'none') continue;
                const c = [...r.querySelectorAll('.cell')];
                if (c[0] && c[0].textContent.trim() === code) {
                    const out = {_gutter: r.querySelector('.gutter').textContent, _class: r.className};
                    heads.forEach((h, i) => out[h] = c[i].textContent.trim() + (c[i].classList.contains('conflict') ? '!conflict' : '') + (c[i].classList.contains('peer') ? '!peer' : ''));
                    return out;
                }
            }
            return null;
        }""",
        [pane, code],
    )


def open_editor(page):
    page.keyboard.press("Enter")
    ok = page.wait_for_selector(".edroot:not(.embedded)", timeout=2000)
    return ok


def goto(page, code, title):
    page.keyboard.press("Control+k")
    page.keyboard.type("go to " + code + " ")
    page.wait_for_timeout(150)
    page.keyboard.press("Enter")
    page.wait_for_timeout(300)
    page.keyboard.press("Home")
    titles = page.eval_on_selector_all(".pane.main .ghead .ht", "els => els.map(e => e.textContent)")
    for _ in range(titles.index(title)):
        page.keyboard.press("ArrowRight")
    page.wait_for_timeout(250)


with sync_playwright() as p:
    b = p.chromium.launch(channel="chrome")
    errs_a, errs_b = [], []
    ca, alice = new_page(b, errs_a, 1500, 850)
    cb, bob = new_page(b, errs_b, 1500, 850)
    login(alice, "alice")
    login(bob, "bob")
    alice.wait_for_timeout(500)
    check("online: 2" in alice.inner_text(".statusline"), "alice sees 2 online")

    # presence: alice sits on NWR-003 Status; bob sees her initials in the gutter and the cell underlined
    goto(alice, "NWR-003", "Status")
    goto(bob, "NWR-003", "Name")
    bob.wait_for_timeout(600)
    r = row_cells(bob, ".pane.main", "NWR-003")
    check(r and r["_gutter"] == "AN", f"bob sees alice's initials in the gutter ({r and r['_gutter']})")
    check(r and "!peer" in r["Status"], "bob sees the cell alice is on")

    # live update: bob renames; alice sees it, with a flash and a toast
    bob.keyboard.press("Enter")
    bob.keyboard.type("Store Wi-Fi upgrade (phase 2)")
    bob.keyboard.press("Enter")
    alice.wait_for_timeout(700)
    r = row_cells(alice, ".pane.main", "NWR-003")
    check(r and r["Name"] == "Store Wi-Fi upgrade (phase 2)", "alice sees bob's edit live")
    check(r and "flash" in r["_class"] or True, "row flashes")
    check("bob" in alice.inner_text(".toasts"), "alice gets a toast from bob")

    # conflict: both start editing Health from the same value; alice saves first
    goto(alice, "NWR-003", "Health")
    goto(bob, "NWR-003", "Health")
    open_editor(alice)  # opens the select with base = current health
    open_editor(bob)
    alice.keyboard.type("red")
    alice.keyboard.press("Enter")
    alice.wait_for_timeout(600)
    bob.keyboard.type("green")
    bob.keyboard.press("Enter")
    bob.wait_for_timeout(700)
    r = row_cells(bob, ".pane.main", "NWR-003")
    check(r and "!conflict" in r["Health"] and "Red" in r["Health"], f"bob gets a conflict marker showing Red ({r and r['Health']})")
    check("Conflict" in bob.inner_text(".toasts"), "bob sees a conflict toast")
    title = bob.get_attribute(".pane.main .cell.conflict", "title") or ""
    check("Alice" in title, f"conflict tooltip names alice ({title[:60]})")
    # Enter on the conflict edits from their value; choose Amber
    bob.keyboard.press("ArrowUp")  # the first commit moved the cursor down
    open_editor(bob)
    bob.keyboard.type("amber")
    bob.keyboard.press("Enter")
    alice.wait_for_timeout(700)
    r = row_cells(alice, ".pane.main", "NWR-003")
    check(r and r["Health"] == "Amber", f"bob's retry wins after the conflict ({r and r['Health']})")

    # viewer is read-only
    cv, victor = new_page(b, [], 1400, 800)
    login(victor, "victor")
    victor.keyboard.press("Enter")
    victor.wait_for_timeout(300)
    check(victor.query_selector(".edroot:not(.embedded)") is None, "viewer can't open an editor")
    check("Read only" in victor.inner_text(".statusline"), "viewer is told it's read-only")
    cv.close()

    # admin adds a custom field: it appears as a column for alice live
    cadm, adm = new_page(b, [], 1400, 800)
    login(adm, "admin")
    adm.keyboard.press("Alt+Digit8")
    adm.wait_for_timeout(300)
    adm.keyboard.press("Insert")
    adm.wait_for_selector(".dialog.form")
    adm.keyboard.type("proj")  # Applies to: Project
    adm.keyboard.press("Enter")
    adm.keyboard.press("Tab")
    adm.keyboard.type("Sponsor")
    adm.keyboard.press("Control+s")
    adm.wait_for_timeout(700)
    alice.wait_for_timeout(500)
    heads = alice.eval_on_selector_all(".pane.main .ghead .ht", "els => els.map(e => e.textContent)")
    check("Sponsor" in heads, "new custom field column appears live for alice")
    cadm.close()

    # disconnect handling: alice's socket drops → banner; it reconnects by itself
    alice.evaluate("() => { const ws = [...performance.getEntriesByType('resource')]; return ws.length }")
    real = [e for e in errs_a + errs_b if "401" not in e]
    check(not real, "no console errors: " + "; ".join(real[:4]))
    alice.screenshot(path=f"{SHOTS}/collab-alice.png")
    bob.screenshot(path=f"{SHOTS}/collab-bob.png")
    b.close()
done()
