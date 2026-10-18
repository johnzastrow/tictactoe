package tui

// Tabs 7 Text area and 8 Choices.

import (
	"fmt"

	tea "charm.land/bubbletea/v2"

	"git.wharf.fluidgrid.org/jcz/tictactoe/go/internal/demo/core"
)

// ================= 7 Text area =================

type areaTab struct {
	ctlBase
	area   *areaInput
	dirty  bool
	base   int64
	theirs string
	shown  string // the synced text the editor was loaded from
	err    string
	box    Rect // last drawn editor rect
}

func newAreaTab() *areaTab { return &areaTab{area: newArea("")} }

func (t *areaTab) explain() string {
	return "A multi-line editor with word wrap: Enter is a newline, ^S saves, Esc reverts. Leaving with Tab keeps your draft."
}

func (t *areaTab) controls() int { return 1 }

func (t *areaTab) keyHelp() [][2]string {
	return [][2]string{
		{"^S", "save"}, {"Esc", "revert (asks)"}, {"Enter", "newline"}, {"arrows", "move"}, {"Home/End", "line"},
		{"PgUp/PgDn", "page"}, {"^Home/^End", "start / end"}, {"Tab", "leave, keep draft"},
	}
}

func (t *areaTab) hints(a *App) []hint {
	return []hint{h("ctrl+s", "Save"), {"Esc", "Revert", "esc"}, {"Enter", "Newline", ""}, h("tab", "Leave")}
}

func (t *areaTab) note(a *App) string {
	switch {
	case t.theirs != "":
		return t.theirs
	case t.dirty:
		return "● unsaved"
	}
	return ""
}

func (t *areaTab) sync(a *App) {
	if t.dirty {
		return
	}
	if s := core.SafeText(a.vals.String("textarea.notes")); s != t.shown {
		t.shown = s
		t.area.SetText(s)
	}
}

func (t *areaTab) changed(a *App, key string, kind changeKind, by string) {
	switch kind {
	case chRemote:
		if t.dirty {
			t.theirs = "⚠ changed by " + by + " · Esc takes theirs"
		}
	case chConflict, chReset:
		t.dirty, t.theirs, t.err = false, "", ""
	case chError:
		t.err = by
	}
}

func (t *areaTab) save(a *App) {
	if !t.dirty {
		a.info("textarea.notes unchanged")
		return
	}
	if err := a.commitBase("textarea.notes", t.area.Text(), t.base); err != nil {
		if err != errOffline {
			t.err = err.Error()
			a.setStatus("✗ "+err.Error(), kErr)
		}
		return
	}
	t.dirty, t.theirs, t.err = false, "", ""
	t.shown = t.area.Text()
}

func (t *areaTab) revert(a *App) {
	t.dirty, t.theirs, t.err = false, "", ""
	t.shown = "\x00" // force a reload
	t.sync(a)
}

func (t *areaTab) touch(a *App) {
	if !t.dirty {
		t.dirty = true
		t.base = a.vals.Version("textarea.notes")
	}
	t.err = ""
	if t.area.Runes() > 5000 {
		t.err = "at most 5000 characters"
	}
}

func (t *areaTab) key(a *App, k tea.KeyPressMsg) bool {
	t.sync(a)
	switch k.Keystroke() {
	case "ctrl+s":
		t.save(a)
		return true
	case "esc":
		if !t.dirty {
			return false
		}
		a.confirm("Discard changes", "Discard your unsaved changes to the notes?", "Discard", func(a *App) { t.revert(a) })
		return true
	case "tab", "shift+tab":
		return false
	}
	handled, changed := t.area.Key(k)
	if changed {
		t.touch(a)
	}
	return handled
}

func (t *areaTab) paste(a *App, s string) {
	t.sync(a)
	t.area.Insert(s)
	t.touch(a)
}

func (t *areaTab) draw(a *App, b *Buf, r Rect, focused bool) []Hit {
	th := a.th()
	t.sync(a)
	box := Rect{r.X, r.Y, min(r.W, 90), max(3, r.H-1)}
	border := Sty{FG: choose(focused, th.Accent, th.Border), BG: th.BG}
	if _, ok := a.flash["textarea.notes"]; ok {
		border = border.Fg(th.Warn)
	}
	b.Box(box, border, fieldStyle(th, false), "Notes", border.With(Bold))
	in := Rect{box.X + 1, box.Y + 1, box.W - 2, box.H - 2}
	t.box = in
	t.area.Draw(b, in, th, focused, fieldStyle(th, false))
	ln, col := t.area.Pos()
	foot := fmt.Sprintf("Ln %d, Col %d · %d words · %d/5000", ln, col, t.area.Words(), t.area.Runes())
	fs := th.base().Fg(th.Muted)
	if t.err != "" {
		foot, fs = "✗ "+t.err+" · "+foot, th.base().Fg(th.Err)
	} else if t.dirty {
		foot = "● unsaved · " + foot
		fs = th.base().Fg(th.Warn)
	}
	b.Text(box.X+1, box.Bottom(), foot, fs, box.W-2)
	return []Hit{{R: in, Kind: HitCtl, S: "area"}}
}

func (t *areaTab) mouse(a *App, m hitMsg) {
	if d, ok := isWheel(m); ok {
		t.area.moveRows(3 * d)
		return
	}
	if isClick(m) {
		t.sync(a)
		t.area.ClickAt(m.m.X-t.box.X, m.m.Y-t.box.Y)
	}
}

// ================= 8 Choices =================

type choiceTab struct {
	ctlBase
	chCur, sizeCur int
}

func newChoiceTab() *choiceTab { return &choiceTab{sizeCur: -1} }

func (t *choiceTab) explain() string {
	return "A checkbox, a checkbox group, a radio group and a toggle switch. Every change saves at once."
}

func (t *choiceTab) controls() int { return 4 }

func (t *choiceTab) keyHelp() [][2]string {
	return [][2]string{
		{"Space Enter", "toggle / select"}, {"←→ ↑↓", "move in a group"}, {"Tab", "next group"}, {"click", "toggle"},
	}
}

func (t *choiceTab) hints(a *App) []hint {
	switch t.cur {
	case 1, 2:
		return []hint{{"←→", "Move", ""}, h("space", "Toggle"), h("tab", "Next group")}
	}
	return []hint{h("space", "Toggle"), {"↑↓", "Prev / next", ""}, h("tab", "Next")}
}

func (t *choiceTab) focusCtl(a *App, i int) {
	t.cur = i
	if i == 2 {
		t.sizeCur = sizeIndex(a.vals.String("choice.size"))
	}
}

func sizeIndex(id string) int {
	for i, s := range core.Sizes {
		if s.ID == id {
			return i
		}
	}
	return 0
}

func (t *choiceTab) save(a *App, key string, v any) {
	if err := a.commit(key, v); err != nil && err != errOffline {
		a.setStatus("✗ "+err.Error(), kErr)
	}
}

// toggle acts on the focused control (and option).
func (t *choiceTab) toggle(a *App) {
	switch t.cur {
	case 0:
		t.save(a, "choice.notify", !a.vals.Bool("choice.notify"))
	case 1:
		if !a.vals.Bool("choice.notify") {
			a.info("turn on Notify first")
			return
		}
		id := core.Channels[t.chCur].ID
		var out []string
		found := false
		for _, c := range a.vals.Strings("choice.channels") {
			if c == id {
				found = true
			} else {
				out = append(out, c)
			}
		}
		if !found {
			out = append(out, id)
		}
		if out == nil {
			out = []string{}
		}
		t.save(a, "choice.channels", out)
	case 2:
		t.save(a, "choice.size", core.Sizes[max(0, t.sizeCur)].ID)
	case 3:
		t.save(a, "choice.dark", !a.vals.Bool("choice.dark"))
	}
}

func (t *choiceTab) key(a *App, k tea.KeyPressMsg) bool {
	key := k.Keystroke()
	switch key {
	case "space", "enter":
		t.toggle(a)
		return true
	case "left", "right", "up", "down":
	default:
		return false
	}
	d := choose(key == "left" || key == "up", -1, 1)
	switch t.cur {
	case 1:
		t.chCur = max(0, min(len(core.Channels)-1, t.chCur+d))
	case 2:
		if t.sizeCur < 0 {
			t.sizeCur = sizeIndex(a.vals.String("choice.size"))
		}
		t.sizeCur = max(0, min(len(core.Sizes)-1, t.sizeCur+d))
	default:
		if key == "left" || key == "right" {
			if t.cur == 3 {
				t.save(a, "choice.dark", key == "right")
			}
			return true
		}
		n := t.cur + d
		if n < 0 || n > 3 {
			return false
		}
		t.focusCtl(a, n)
	}
	return true
}

func (t *choiceTab) draw(a *App, b *Buf, r Rect, focused bool) []Hit {
	th := a.th()
	var hits []Hit
	gap := choose(r.H >= 8, 2, 1)
	label := func(i int, s string) {
		fcs := focused && t.cur == i
		b.Fit(r.X, r.Y+i*gap, s, th.base().Fg(choose(fcs, th.Accent, th.Muted)).With(choose(fcs, Bold, Attr(0))), 12, 0)
	}
	flash := func(key string) bool { _, ok := a.flash[key]; return ok }
	opt := func(x, y int, text string, on, cur, dim bool, ctl, sub int, key string) int {
		st := th.base()
		if dim {
			st = st.Fg(th.Muted).With(Dim)
		}
		if on && !dim {
			st = st.Fg(th.OK)
		}
		if flash(key) {
			st = st.Bg(mix(th.BG, th.Warn, 0.35))
		}
		if cur {
			st = th.sel().With(Bold)
		}
		w := b.Text(x, y, text, st, r.Right()-x)
		hits = append(hits, Hit{R: Rect{x, y, w, 1}, Kind: HitCtl, S: "opt", A: ctl, B: sub})
		return w
	}
	notify := a.vals.Bool("choice.notify")
	label(0, "Notify")
	opt(r.X+12, r.Y, choose(notify, "[✓] Notify me", "[ ] Notify me"), notify, focused && t.cur == 0, false, 0, 0, "choice.notify")
	label(1, "Channels")
	have := map[string]bool{}
	for _, c := range a.vals.Strings("choice.channels") {
		have[c] = true
	}
	x := r.X + 12
	for i, c := range core.Channels {
		box := choose(have[c.ID], "[✓] ", "[ ] ")
		x += opt(x, r.Y+gap, box+c.Label, have[c.ID], focused && t.cur == 1 && t.chCur == i, !notify, 1, i, "choice.channels") + 2
	}
	if !notify {
		b.Text(x, r.Y+gap, "(Notify is off)", th.base().Fg(th.Muted).With(Italic), r.Right()-x)
	}
	label(2, "Size")
	size := a.vals.String("choice.size")
	x = r.X + 12
	for i, s := range core.Sizes {
		dot := choose(s.ID == size, "(●) ", "( ) ")
		cur := focused && t.cur == 2 && (t.sizeCur == i || t.sizeCur < 0 && s.ID == size)
		x += opt(x, r.Y+2*gap, dot+s.Label, s.ID == size, cur, false, 2, i, "choice.size") + 2
	}
	label(3, "Dark mode")
	dark := a.vals.Bool("choice.dark")
	opt(r.X+12, r.Y+3*gap, choose(dark, "[ ━● ] On", "[ ●━ ] Off"), dark, focused && t.cur == 3, false, 3, 0, "choice.dark")
	return hits
}

func (t *choiceTab) mouse(a *App, m hitMsg) {
	if !isClick(m) {
		return
	}
	t.cur = m.hit.A
	switch t.cur {
	case 1:
		t.chCur = m.hit.B
	case 2:
		t.sizeCur = m.hit.B
	}
	t.toggle(a)
}
