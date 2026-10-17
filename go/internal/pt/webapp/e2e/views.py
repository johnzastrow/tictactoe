"""Board, Timeline, Dashboard, Activity, menus, help, themes, scroll performance."""
import datetime
from common import *


def card_status(page, card_sel):
    return page.evaluate("""(sel) => {
        const c = document.querySelector(sel);
        if (!c) return null;
        const col = c.closest('.bcol');
        const i = [...col.parentElement.querySelectorAll('.bcol')].indexOf(col);
        return i;
    }""", card_sel)


with sync_playwright() as p:
    b = p.chromium.launch(channel="chrome")
    errors = []
    ctx, page = new_page(b, errors, 1600, 900)
    login(page, "carol")

    # ---- board ----
    key(page, "Alt+Digit3")
    page.wait_for_selector(".bcard")
    page.click(".bcard >> nth=0")
    sel_id = page.get_attribute(".bcard.sel", "data-card")
    check(card_status(page, ".bcard.sel") == 0, "first card is in To do")
    key(page, "Shift+ArrowRight")
    page.wait_for_timeout(600)
    sel = f'.bcard[data-card="{sel_id}"]'
    check(card_status(page, sel) == 1, "⇧→ moves the card to In progress")
    key(page, ">")
    page.wait_for_timeout(600)
    check(card_status(page, sel) == 2, "> moves it to Blocked")
    # drag it to Review
    page.drag_and_drop(sel, '.bcol[data-drop="3"]')
    page.wait_for_timeout(700)
    check(card_status(page, sel) == 3, "drag & drop moves it to Review")
    # cascading scope: program then project
    key(page, "F6")
    page.keyboard.type("municipal")
    page.wait_for_timeout(200)
    key(page, "Enter")
    key(page, "Tab")
    page.keyboard.type("gis-003")
    page.wait_for_timeout(200)
    key(page, "Enter")
    page.wait_for_timeout(400)
    codes = set(page.eval_on_selector_all(".bcard .code", "els => els.map(e => e.textContent)"))
    check(codes == {"GIS-003"}, f"scope Program ▸ Project filters cards ({codes})")
    # swimlanes
    page.click(".board")
    key(page, "Control+r")
    page.wait_for_timeout(400)
    check(len(page.query_selector_all(".blane")) > 1, "^R shows swimlanes by assignee")
    page.screenshot(path=f"{SHOTS}/board-swim.png")
    key(page, "Control+r")

    # ---- timeline ----
    key(page, "Alt+Digit4")
    page.wait_for_selector(".tlrow")
    page.click(".tlrow >> nth=0")
    key(page, "ArrowRight")  # expand
    page.wait_for_timeout(400)
    n = len(page.query_selector_all(".tlrow"))
    check(n > 50, f"→ expands a project into tasks ({n} rows)")
    first = page.inner_text(".tlrow >> nth=0")
    code = first.split()[1]
    before = page.evaluate("""(code) => window.__x = 0""", code)
    key(page, "ArrowDown")  # first task
    task_label = page.inner_text(".tlrow.cur")
    # read the task's dates from the record form
    def task_dates():
        key(page, "Control+e")
        page.wait_for_selector(".dialog.form")
        vals = page.eval_on_selector_all(".dialog .frow", """rows => {
            const out = {};
            rows.forEach(r => { out[r.querySelector('.flabel').textContent] = (r.querySelector('input') || {}).value; });
            return out;
        }""")
        key(page, "Escape")
        page.wait_for_timeout(150)
        return vals.get("Start"), vals.get("Due")
    s0, d0 = task_dates()
    key(page, "Shift+ArrowRight")  # zoom is weeks: +7 days
    page.wait_for_timeout(700)
    s1, d1 = task_dates()
    shift = lambda a, n: (datetime.date.fromisoformat(a) + datetime.timedelta(days=n)).isoformat()
    check(s1 == shift(s0, 7) and d1 == shift(d0, 7), f"⇧→ moves the bar a week ({s0}→{s1})")
    key(page, "]")
    page.wait_for_timeout(700)
    s2, d2 = task_dates()
    check(s2 == s1 and d2 == shift(d1, 7), f"] extends the due date ({d1}→{d2})")
    # drag the bar back by ~1 week with the mouse
    bar = page.query_selector(".tlbar[data-row='1'] rect")
    box = bar.bounding_box()
    left = page.query_selector(".tlleft").bounding_box()
    if box["x"] < left["x"] + left["width"] + 60:
        # bring the bar out from under the sticky label column
        page.evaluate("(dx) => { document.querySelector('.tl').scrollLeft -= dx }", left["x"] + left["width"] + 120 - box["x"])
        page.wait_for_timeout(200)
        box = page.query_selector(".tlbar[data-row='1'] rect").bounding_box()
    page.mouse.move(box["x"] + 10, box["y"] + box["height"] / 2)
    page.mouse.down()
    page.mouse.move(box["x"] + 10 - 7 * 7, box["y"] + box["height"] / 2, steps=6)
    page.mouse.up()
    page.wait_for_timeout(800)
    s3, d3 = task_dates()
    check(s3 == s0, f"dragging the bar left moves it back ({s2}→{s3})")
    key(page, "+")
    page.wait_for_timeout(300)
    check("days" in page.inner_text(".qind"), "+ zooms to days")
    page.screenshot(path=f"{SHOTS}/timeline-expanded.png")

    # ---- dashboard drill-down ----
    key(page, "Alt+Digit5")
    page.wait_for_selector(".kpi")
    key(page, "ArrowRight")  # Overdue tasks tile
    key(page, "Enter")
    page.wait_for_timeout(500)
    check(page.inner_text(".tab.active").startswith("2"), "Enter drills into Tasks")
    check(page.input_value("#query") == "is:overdue", "with the is:overdue filter")
    # ---- activity: jump to record ----
    key(page, "Alt+Digit6")
    page.wait_for_selector(".grid .row")
    page.keyboard.press("Control+Home")
    page.wait_for_timeout(200)
    top = page.inner_text(".grid .row.currow .cell >> nth=3")
    check("carol" in page.inner_text(".grid .row.currow").lower() or "Carol" in page.inner_text(".grid .row.currow"), f"latest activity is carol's ({top})")
    key(page, "Enter")
    page.wait_for_timeout(500)
    check(page.inner_text(".tab.active").startswith("2"), "Enter on activity jumps to the task")

    # ---- menus & help ----
    key(page, "F10")
    check(page.query_selector(".menu.open .drop") is not None, "F10 opens the menu bar")
    key(page, "ArrowRight")
    key(page, "ArrowRight")
    check("View" in page.inner_text(".menu.open"), "→ moves to the View menu")
    key(page, "Escape")
    key(page, "F1")
    page.wait_for_selector(".dialog.help")
    page.keyboard.type("fill")
    check("Fill down" in page.inner_text(".helplist"), "help search finds fill down")
    key(page, "Escape")

    # ---- themes (light + dark screenshots) ----
    for t in ["daylight", "solarized", "gruvbox", "midnight"]:
        page.evaluate(f"() => {{ document.documentElement.setAttribute('data-theme', '{t}'); }}")
        for i, name in [(1, "projects"), (3, "board"), (4, "timeline"), (5, "dashboard")]:
            key(page, f"Alt+Digit{i}")
            page.wait_for_timeout(350)
            if t in ("daylight", "midnight"):
                page.screenshot(path=f"{SHOTS}/theme-{t}-{name}.png")
    key(page, "Control+t")
    check(page.evaluate("document.documentElement.getAttribute('data-theme')") != "midnight", "^T cycles theme")
    key(page, "Control+t")
    key(page, "Control+t")
    key(page, "Control+t")

    # ---- scroll performance on the 971-task grid ----
    key(page, "Alt+Digit2")
    page.wait_for_timeout(300)
    page.fill("#query", "")
    key(page, "Enter")
    page.click(".grid .row.grp >> nth=0")
    # ungroup to get a flat 971-row grid
    page.keyboard.press("Home")
    key(page, "Control+r")
    page.wait_for_timeout(400)
    t = page.evaluate("""async () => {
        const g = document.querySelector('.grid');
        const frames = [];
        let last = performance.now();
        for (let i = 0; i < 60; i++) {
            g.scrollTop += 400;
            await new Promise(r => requestAnimationFrame(r));
            const now = performance.now(); frames.push(now - last); last = now;
        }
        frames.sort((a, b) => a - b);
        return {median: frames[30], p95: frames[57], rows: document.querySelectorAll('.gbody .row').length};
    }""")
    print(f"  scroll: median frame {t['median']:.1f} ms, p95 {t['p95']:.1f} ms, pooled rows {t['rows']}")
    check(t["p95"] < 50, "scrolling stays under 50 ms/frame (p95)")
    t = page.evaluate("""async () => {
        const t0 = performance.now();
        document.querySelector('#query').focus();
        return performance.now() - t0;
    }""")
    page.keyboard.type("status:todo @carol")
    page.wait_for_timeout(300)
    check("/ 971" in page.inner_text(".qcount"), "filter over 971 tasks")
    key(page, "Control+r")

    real = [e for e in errors if "401" not in e]
    check(not real, "no console errors: " + "; ".join(real[:4]))
    b.close()
done()
