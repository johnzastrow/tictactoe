package tui

// Tabs 1 Text and 2 Number, and syncField: a one-line input bound to a
// key. Untouched, it shows the synced value; the first edit makes it a
// draft based on the version shown then. Enter commits the draft with that
// base (so a change by someone else meanwhile is reported as a conflict),
// Esc reverts to the synced value.

import (
	"fmt"
	"strconv"
	"strings"
	"unicode"

	tea "charm.land/bubbletea/v2"

	"git.wharf.fluidgrid.org/jcz/tictactoe/go/internal/demo/core"
)

type syncField struct {
	key   string
	label string
	in    lineInput
	dirty bool
	base  int64
	err   string
	// theirs is set when the synced value changed under a draft.
	theirs string
	sent   string // the text last committed (restored if the server refuses it)
	// show turns the synced value into text; parse turns text into a value.
	show  func(a *App) string
	parse func(a *App, s string) (any, error)
}

func newSyncField(key, label string) *syncField {
	f := &syncField{key: key, label: label}
	f.show = func(a *App) string { return core.SafeText(a.vals.String(key)) }
	f.parse = func(_ *App, s string) (any, error) { return s, nil }
	return f
}

// sync shows the synced value unless there is a draft.
func (f *syncField) sync(a *App) {
	if f.dirty {
		return
	}
	if s := f.show(a); s != f.in.Text() {
		f.in.SetText(s)
	}
}

// validate checks the draft with the server's rules (cross-key included).
func (f *syncField) validate(a *App) {
	f.err = ""
	v, err := f.parse(a, f.in.Text())
	if err == nil {
		_, err = a.vals.PrepareSet(f.key, v)
	}
	if err != nil {
		f.err = err.Error()
	}
}

// touch marks the field as a draft (keeping the base of the first edit).
func (f *syncField) touch(a *App) {
	if !f.dirty {
		f.dirty = true
		f.base = a.vals.Version(f.key)
	}
	f.validate(a)
}

// edit applies an editing key; it reports whether the key was used.
func (f *syncField) edit(a *App, k tea.KeyPressMsg) bool {
	f.sync(a)
	handled, changed := f.in.Key(k)
	if changed {
		f.touch(a)
	}
	return handled
}

func (f *syncField) insert(a *App, s string) {
	f.sync(a)
	if f.in.Insert(s) {
		f.touch(a)
	}
}

// commit sends the draft; invalid drafts stay with their message.
func (f *syncField) commit(a *App) {
	if !f.dirty {
		a.info(f.key + " unchanged")
		return
	}
	f.validate(a)
	if f.err != "" {
		a.setStatus("✗ "+f.key+": "+f.err, kErr)
		return
	}
	v, _ := f.parse(a, f.in.Text())
	f.sent = f.in.Text()
	if err := a.commitBase(f.key, v, f.base); err != nil {
		if err != errOffline {
			f.err = err.Error()
		}
		return
	}
	f.dirty, f.theirs = false, ""
}

// revert drops the draft (Esc: "take theirs" when it changed meanwhile).
func (f *syncField) revert(a *App) bool {
	if !f.dirty {
		return false
	}
	f.dirty, f.err, f.theirs = false, "", ""
	f.sync(a)
	return true
}

func (f *syncField) changed(a *App, kind changeKind, by string) {
	switch kind {
	case chRemote:
		if f.dirty {
			f.theirs = "⚠ changed by " + by
		}
	case chError:
		f.dirty, f.base = true, a.vals.Version(f.key)
		f.in.SetText(f.sent)
		f.err = by
	case chReset, chConflict:
		f.dirty, f.err, f.theirs = false, "", ""
	}
}

// drawField draws "Label  [input]  message" at y and returns the input rect.
func (f *syncField) drawField(a *App, b *Buf, x, y, labelW, inputW int, focused bool, r Rect) Rect {
	t := a.th()
	f.sync(a)
	ls := t.base().Fg(choose(focused, t.Accent, t.Muted))
	if focused {
		ls = ls.With(Bold)
	}
	b.Fit(x, y, f.label, ls, labelW, 0)
	fr := Rect{x + labelW, y, inputW, 1}
	st := fieldStyle(t, focused)
	if _, ok := a.flash[f.key]; ok && !f.dirty {
		st = Sty{FG: t.BG, BG: t.Warn}
	}
	f.in.Draw(b, fr, t, focused, st)
	if f.err != "" {
		b.Restyle(fr, func(s Sty) Sty { return s.ULine(t.Err).Fg(t.Err) })
	}
	return fr
}

// message is the text shown beside / under a field.
func (f *syncField) message() (string, bool) {
	switch {
	case f.err != "":
		return "✗ " + f.err, true
	case f.theirs != "":
		return f.theirs + " · Esc takes theirs", false
	case f.dirty:
		return "● edited · Enter saves · Esc reverts", false
	}
	return "", false
}

// drawMessage puts the field's message right of it when it fits, else on
// the next row.
func (f *syncField) drawMessage(a *App, b *Buf, fr Rect, r Rect) {
	t := a.th()
	msg, bad := f.message()
	if msg == "" {
		return
	}
	st := t.base().Fg(choose(bad, t.Err, choose(f.theirs != "", t.Warn, t.Muted)))
	if room := r.Right() - fr.Right() - 2; room >= min(StrW(msg), 30) {
		b.Text(fr.Right()+2, fr.Y, msg, st, room)
	} else {
		b.Text(fr.X, fr.Y+1, msg, st, r.Right()-fr.X)
	}
}

// ================= 1 Text =================

type textTab struct {
	ctlBase
	fields []*syncField
	pw     lineInput
	showPW bool
}

func newTextTab() *textTab {
	t := &textTab{fields: []*syncField{
		newSyncField("text.name", "Name"),
		newSyncField("text.email", "Email"),
		newSyncField("text.code", "Code"),
	}}
	t.fields[2].in.upper = true
	t.fields[2].in.max = 12
	t.fields[0].in.max = 100
	t.pw.mask = true
	t.pw.placeholder = "local only, never sent"
	return t
}

func (t *textTab) explain() string {
	return "Single-line text fields with live validation; Enter saves, Esc reverts. The password field is masked and never leaves this client."
}

func (t *textTab) controls() int { return 4 }

func (t *textTab) keyHelp() [][2]string {
	return [][2]string{
		{"Tab ↑↓", "next field"}, {"Enter", "save"}, {"Esc", "revert / take theirs"},
		{"←→ Home End", "move"}, {"^←/^→", "word"}, {"⌫ Del", "delete"}, {"^U", "clear"},
		{"paste", "insert"}, {"^O", "show password"},
	}
}

func (t *textTab) hints(a *App) []hint {
	if t.cur == 3 {
		return []hint{h("ctrl+o", choose(t.showPW, "Hide", "Show")), h("ctrl+u", "Clear"), h("tab", "Next")}
	}
	return []hint{h("enter", "Save"), {"Esc", "Revert", "esc"}, h("ctrl+u", "Clear"), h("tab", "Next field")}
}

func (t *textTab) changed(a *App, key string, kind changeKind, by string) {
	for _, f := range t.fields {
		if f.key == key {
			f.changed(a, kind, by)
		}
	}
}

func (t *textTab) key(a *App, k tea.KeyPressMsg) bool {
	key := k.Keystroke()
	switch key {
	case "up":
		if t.cur > 0 {
			t.cur--
			return true
		}
		return false
	case "down":
		t.cur = min(t.cur+1, 3)
		return true
	case "ctrl+o":
		t.showPW = !t.showPW
		t.pw.mask = !t.showPW
		return true
	}
	if t.cur == 3 {
		handled, _ := t.pw.Key(k)
		return handled
	}
	f := t.fields[t.cur]
	switch key {
	case "enter":
		f.commit(a)
		return true
	case "esc":
		return f.revert(a)
	}
	return f.edit(a, k)
}

func (t *textTab) paste(a *App, s string) {
	if t.cur == 3 {
		t.pw.Insert(s)
		return
	}
	t.fields[t.cur].insert(a, s)
}

func (t *textTab) draw(a *App, b *Buf, r Rect, focused bool) []Hit {
	th := a.th()
	var hits []Hit
	gap := choose(r.H >= 8, 2, 1)
	iw := min(40, r.W-12)
	for i, f := range t.fields {
		y := r.Y + i*gap
		fr := f.drawField(a, b, r.X, y, 10, iw, focused && t.cur == i, r)
		f.drawMessage(a, b, fr, r)
		hits = append(hits, Hit{R: Rect{r.X, y, fr.Right() - r.X, 1}, Kind: HitCtl, S: "field", A: i})
	}
	y := r.Y + 3*gap
	fcs := focused && t.cur == 3
	b.Fit(r.X, y, "Password", th.base().Fg(choose(fcs, th.Accent, th.Muted)), 10, 0)
	pr := Rect{r.X + 10, y, min(24, iw), 1}
	t.pw.Draw(b, pr, th, fcs, fieldStyle(th, fcs))
	hits = append(hits, Hit{R: Rect{r.X, y, pr.Right() - r.X, 1}, Kind: HitCtl, S: "field", A: 3})
	br := button(b, th, pr.Right()+2, y, choose(t.showPW, "◎ hide", "◉ show"), false, th.CellBG)
	hits = append(hits, Hit{R: br, Kind: HitCtl, S: "showpw", A: 3})
	b.Text(br.Right()+2, y, "^O · masked, local only", th.base().Fg(th.Muted), r.Right()-br.Right()-2)
	return hits
}

func (t *textTab) mouse(a *App, m hitMsg) {
	if !isClick(m) {
		return
	}
	t.cur = m.hit.A
	if m.hit.S == "showpw" {
		t.showPW = !t.showPW
		t.pw.mask = !t.showPW
	}
}

// ================= 2 Number =================

type numberTab struct {
	ctlBase
	hours, qty *syncField
}

func fmtNum(f float64) string { return strconv.FormatFloat(f, 'f', -1, 64) }

func newNumberTab() *numberTab {
	t := &numberTab{hours: newSyncField("number.hours", "Hours"), qty: newSyncField("number.qty", "Quantity")}
	t.hours.show = func(a *App) string { return fmtNum(a.vals.Float("number.hours")) }
	t.qty.show = func(a *App) string { return strconv.FormatInt(a.vals.Int("number.qty"), 10) }
	parse := func(_ *App, s string) (any, error) {
		f, err := strconv.ParseFloat(strings.TrimSpace(s), 64)
		if err != nil {
			return nil, fmt.Errorf("must be a number")
		}
		return f, nil
	}
	t.hours.parse, t.qty.parse = parse, parse
	t.hours.in.accept = func(r rune) bool { return unicode.IsDigit(r) || r == '.' || r == '-' }
	t.qty.in.accept = func(r rune) bool { return unicode.IsDigit(r) }
	t.hours.in.max, t.qty.in.max = 10, 4
	return t
}

func (t *numberTab) explain() string {
	return "Spinners for a decimal and a whole number (Enter saves), and a slider that saves on every change."
}

func (t *numberTab) controls() int { return 3 }

func (t *numberTab) keyHelp() [][2]string {
	return [][2]string{
		{"↑↓ + -", "±1"}, {"⇧↑↓", "±0.25 (hours)"}, {"digits . -", "type"}, {"Enter", "save"}, {"Esc", "revert"},
		{"Tab", "next"}, {"slider ←→", "±1"}, {"PgUp/PgDn", "±10"}, {"Home/End", "0 / 100"}, {"click/drag", "set"},
	}
}

func (t *numberTab) hints(a *App) []hint {
	if t.cur == 2 {
		return []hint{{"←→", "±1", ""}, {"PgUp/Dn", "±10", ""}, h("home", "0"), h("end", "100"), h("tab", "Next")}
	}
	return []hint{{"↑↓", "±1", ""}, {"⇧↑↓", "±0.25", ""}, h("enter", "Save"), {"Esc", "Revert", "esc"}, h("tab", "Next")}
}

func (t *numberTab) changed(a *App, key string, kind changeKind, by string) {
	switch key {
	case "number.hours":
		t.hours.changed(a, kind, by)
	case "number.qty":
		t.qty.changed(a, kind, by)
	}
}

func (t *numberTab) field() *syncField { return choose(t.cur == 0, t.hours, t.qty) }

// step adds d to a spinner's draft (clamped to its range).
func (t *numberTab) step(a *App, f *syncField, d float64) {
	f.sync(a)
	v, err := strconv.ParseFloat(strings.TrimSpace(f.in.Text()), 64)
	if err != nil {
		v, _ = strconv.ParseFloat(f.show(a), 64)
	}
	hi := choose(f == t.hours, 1000.0, 99.0)
	v = max(0, min(hi, core.Round2(v+d)))
	f.in.SetText(fmtNum(v))
	f.touch(a)
}

func (t *numberTab) setPercent(a *App, v int64) {
	v = max(0, min(100, v))
	if v != a.vals.Int("number.percent") {
		_ = a.commit("number.percent", v)
	}
}

func (t *numberTab) key(a *App, k tea.KeyPressMsg) bool {
	key := keyName(k)
	if t.cur == 2 {
		p := a.vals.Int("number.percent")
		switch key {
		case "left", "-":
			t.setPercent(a, p-1)
		case "right", "+", "=":
			t.setPercent(a, p+1)
		case "pgup", "shift+right":
			t.setPercent(a, p+10)
		case "pgdown", "shift+left":
			t.setPercent(a, p-10)
		case "home":
			t.setPercent(a, 0)
		case "end":
			t.setPercent(a, 100)
		case "up":
			t.cur = 1
		case "down":
		default:
			return false
		}
		return true
	}
	f := t.field()
	switch key {
	case "up", "+", "=":
		t.step(a, f, 1)
	case "down", "-":
		t.step(a, f, -1)
	case "shift+up":
		t.step(a, f, choose(f == t.hours, 0.25, 1.0))
	case "shift+down":
		t.step(a, f, choose(f == t.hours, -0.25, -1.0))
	case "enter":
		f.commit(a)
	case "esc":
		return f.revert(a)
	default:
		return f.edit(a, k)
	}
	return true
}

func (t *numberTab) paste(a *App, s string) {
	if t.cur < 2 {
		t.field().insert(a, s)
	}
}

func (t *numberTab) draw(a *App, b *Buf, r Rect, focused bool) []Hit {
	th := a.th()
	var hits []Hit
	gap := choose(r.H >= 8, 2, 1)
	for i, f := range []*syncField{t.hours, t.qty} {
		y := r.Y + i*gap
		fcs := focused && t.cur == i
		b.Fit(r.X, y, f.label, th.base().Fg(choose(fcs, th.Accent, th.Muted)).With(choose(fcs, Bold, Attr(0))), 10, 0)
		minus := button(b, th, r.X+10, y, "−", false, th.CellBG)
		fr := Rect{minus.Right() + 1, y, 12, 1}
		f.drawField(a, b, fr.X, y, 0, fr.W, fcs, r)
		plus := button(b, th, fr.Right()+1, y, "+", false, th.CellBG)
		hits = append(hits,
			Hit{R: Rect{r.X, y, 10, 1}, Kind: HitCtl, S: "field", A: i},
			Hit{R: fr, Kind: HitCtl, S: "field", A: i},
			Hit{R: minus, Kind: HitCtl, S: "minus", A: i},
			Hit{R: plus, Kind: HitCtl, S: "plus", A: i})
		f.drawMessage(a, b, plus, r)
		if f.err == "" && !f.dirty {
			rng := choose(i == 0, "0–1000, 2 decimals", "whole number 0–99")
			b.Text(plus.Right()+2, y, rng, th.base().Fg(th.Muted), r.Right()-plus.Right()-2)
		}
	}
	// slider
	y := r.Y + 2*gap
	fcs := focused && t.cur == 2
	b.Fit(r.X, y, "Percent", th.base().Fg(choose(fcs, th.Accent, th.Muted)).With(choose(fcs, Bold, Attr(0))), 10, 0)
	tr := Rect{r.X + 11, y, max(10, min(51, r.W-20)), 1}
	p := int(a.vals.Int("number.percent"))
	thumb := tr.X + p*(tr.W-1)/100
	for x := tr.X; x < tr.Right(); x++ {
		c := choose(x <= thumb, th.Accent, th.Border)
		b.Set(x, y, choose(x <= thumb, '━', '─'), th.base().Fg(c))
	}
	b.Set(tr.X-1, y, '├', th.base().Fg(th.Border))
	b.Set(tr.Right(), y, '┤', th.base().Fg(th.Border))
	ts := th.base().Fg(choose(fcs, th.Warn, th.FG)).With(Bold)
	if _, ok := a.flash["number.percent"]; ok {
		ts = ts.Fg(th.Warn)
	}
	b.Set(thumb, y, '●', ts)
	b.Text(tr.Right()+2, y, fmt.Sprintf("%3d%%", p), th.base().With(Bold), 5)
	hits = append(hits, Hit{R: Rect{r.X, y, 10, 1}, Kind: HitCtl, S: "field", A: 2},
		Hit{R: Rect{tr.X - 1, y, tr.W + 2, 1}, Kind: HitCtl, S: "track", A: tr.X, B: tr.W})
	if r.H > 2*gap+2 {
		b.Text(r.X+11, y+1, "0", th.base().Fg(th.Muted), 1)
		b.Text(tr.Right()-3, y+1, "100", th.base().Fg(th.Muted), 3)
	}
	return hits
}

// percentAt maps a column on the slider track to 0–100.
func percentAt(x, tx, tw int) int64 {
	return int64(max(0, min(100, ((x-tx)*100+(tw-1)/2)/max(1, tw-1))))
}

func (t *numberTab) mouse(a *App, m hitMsg) {
	h := m.hit
	if d, ok := isWheel(m); ok && h.S == "track" {
		t.setPercent(a, a.vals.Int("number.percent")-int64(d))
		return
	}
	if !isClick(m) {
		return
	}
	t.cur = h.A
	switch h.S {
	case "minus", "plus":
		f := choose(h.A == 0, t.hours, t.qty)
		t.step(a, f, choose(h.S == "plus", 1.0, -1.0))
		f.commit(a) // the mouse path saves at once
	case "track":
		t.cur = 2
		t.setPercent(a, percentAt(m.m.X, h.A, h.B))
		tx, tw := h.A, h.B
		a.drag = &dragState{x0: m.m.X, y0: m.m.Y,
			move: func(a *App, x, _ int) { t.setPercent(a, percentAt(x, tx, tw)) }}
	}
}
