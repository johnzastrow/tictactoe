package tui

// Overlays: list overlays (menu bar dropdowns with submenus, context menus,
// the command palette), confirm, message, one-line prompt, the profile form
// and the searchable help screen. They stack; the top one gets keys and
// mouse first. All draw a drop shadow like pt's popups.

import (
	"fmt"
	"strings"

	tea "charm.land/bubbletea/v2"

	"git.wharf.fluidgrid.org/jcz/tictactoe/go/internal/demo/core"
)

type overlay interface {
	draw(a *App, b *Buf) []Hit
	key(a *App, k tea.KeyPressMsg)
	mouse(a *App, m hitMsg)
	paste(a *App, s string)
	hints(a *App) []hint
}

func (a *App) push(o overlay) { a.overlays = append(a.overlays, o) }

func (a *App) closeOverlay(o overlay) {
	for i := len(a.overlays) - 1; i >= 0; i-- {
		if a.overlays[i] == o {
			a.overlays = append(a.overlays[:i], a.overlays[i+1:]...)
			return
		}
	}
}

func (a *App) top() overlay {
	if len(a.overlays) == 0 {
		return nil
	}
	return a.overlays[len(a.overlays)-1]
}

func centered(w, h, sw, sh int) Rect {
	w, h = min(w, sw-2), min(h, sh-2)
	return Rect{(sw - w) / 2, max(0, (sh-h)/2), w, h}
}

func backdrop(b *Buf) Hit { return Hit{R: Rect{0, 0, b.W, b.H}, Kind: HitBackdrop} }

// ================= list overlay =================

type menuItem struct {
	label   string
	key     string // shown right-aligned
	accel   rune   // underlined letter (lower case)
	run     func(a *App)
	sep     bool
	off     bool // disabled: shown dim, skipped
	checked *bool
	sub     []menuItem
}

type listOverlay struct {
	title   string
	items   []menuItem
	sel     int
	top     int
	x, y    int // anchor; x < 0 = centred near the top
	width   int
	filter  *lineInput // the palette's search box
	all     []menuItem
	matches [][]int
	menuBar int // ≥ 0: dropdown of the app menu bar entry n
	maxRows int
	parent  *listOverlay
	// onLR switches to the adjacent menu of a bar (app or demo) on ←/→.
	onLR func(a *App, d int)
	// path names the menus above this one ("Format ▸ Text").
	path string
	// onRun runs before an item (the palette records palette.last).
	onRun func(a *App, it menuItem)
	rect  Rect // last drawn
}

func newList(title string, items []menuItem, x, y int) *listOverlay {
	l := &listOverlay{title: title, items: items, all: items, x: x, y: y, menuBar: -1}
	l.sel = -1
	l.move(1)
	return l
}

func (l *listOverlay) rows() int {
	n := len(l.items)
	if l.maxRows > 0 {
		n = min(n, l.maxRows)
	}
	return max(1, n)
}

func (l *listOverlay) size() (int, int) {
	w := l.width
	if w == 0 {
		w = 20
		for _, it := range l.items {
			w = max(w, StrW(it.label)+StrW(it.key)+8)
		}
	}
	h := l.rows() + 2
	if l.filter != nil {
		h += 2
	}
	return w, h
}

func (l *listOverlay) place(b *Buf) Rect {
	w, h := l.size()
	if l.x < 0 {
		r := centered(w, h, b.W, b.H)
		r.Y = max(1, b.H/6)
		return r
	}
	x, y := l.x, l.y
	if x+w > b.W {
		x = max(0, b.W-w)
	}
	if y+h > b.H {
		y = max(0, b.H-h)
	}
	return Rect{x, y, w, h}
}

func (l *listOverlay) draw(a *App, b *Buf) []Hit {
	t := a.th()
	r := l.place(b)
	l.rect = r
	var hits []Hit
	if l.parent == nil {
		hits = append(hits, backdrop(b))
	}
	fill := Sty{FG: t.FG, BG: t.Panel}
	b.Box(r, Sty{FG: t.Border, BG: t.Panel}, fill, l.title, Sty{FG: t.Accent, BG: t.Panel, A: Bold})
	b.Shadow(r, t.Shadow)
	hits = append(hits, Hit{R: r, Kind: HitDialog})
	y := r.Y + 1
	if l.filter != nil {
		b.Text(r.X+2, y, "›", fill.Fg(t.Accent).With(Bold), 1)
		l.filter.Draw(b, Rect{r.X + 4, y, r.W - 6, 1}, t, true, fieldStyle(t, true))
		y += 2
		b.Text(r.X+1, y-1, strings.Repeat("─", r.W-2), fill.Fg(t.Border), r.W-2)
	}
	if len(l.items) == 0 {
		b.Text(r.X+2, y, "no matches", fill.Fg(t.Muted), r.W-4)
	}
	for i := 0; i < l.rows() && l.top+i < len(l.items); i++ {
		idx := l.top + i
		hits = append(hits, drawMenuRow(b, t, Rect{r.X + 1, y + i, r.W - 2, 1}, l.items[idx], idx == l.sel, l.match(idx), idx))
	}
	if len(l.items) > l.rows() {
		pos := y + l.top*l.rows()/len(l.items)
		b.Set(r.Right()-1, min(pos, r.Bottom()-2), '┃', Sty{FG: t.Accent, BG: t.Panel})
	}
	return hits
}

func (l *listOverlay) match(i int) []int {
	if l.matches != nil && i < len(l.matches) {
		return l.matches[i]
	}
	return nil
}

// drawMenuRow draws one list row (shared by overlays and the inline
// palette): check mark, label with accelerator / matched letters, key or ▸.
func drawMenuRow(b *Buf, t *Theme, row Rect, it menuItem, selected bool, matched []int, idx int) Hit {
	fill := Sty{FG: t.FG, BG: t.Panel}
	if it.sep {
		b.Text(row.X, row.Y, strings.Repeat("─", row.W), fill.Fg(t.Border), row.W)
		return Hit{}
	}
	st := fill
	if selected {
		st = t.sel()
	}
	if it.off {
		st = st.Fg(t.Muted)
	}
	b.Fill(row, ' ', st)
	if it.checked != nil && *it.checked {
		b.Set(row.X, row.Y, '✓', st.Fg(choose(selected, t.CellFG, t.OK)))
	}
	x := row.X + 2
	mset := map[int]bool{}
	for _, v := range matched {
		mset[v] = true
	}
	right := it.key
	if it.sub != nil {
		right = "▸"
	}
	accelDone := false
	for ci, ch := range []rune(it.label) {
		s := st
		if mset[ci] {
			s = s.With(Bold | Underline)
			if !selected {
				s = s.Fg(t.Accent)
			}
		}
		if !accelDone && it.accel != 0 && !it.off && strings.EqualFold(string(ch), string(it.accel)) {
			s = s.With(Underline)
			if !selected {
				s = s.Fg(t.Accent)
			}
			accelDone = true
		}
		if x >= row.Right()-StrW(right)-2 {
			break
		}
		x += b.Set(x, row.Y, ch, s)
	}
	if right != "" {
		kw := StrW(right)
		b.Text(row.Right()-kw-1, row.Y, right, st.Fg(choose(selected, t.CellFG, t.Muted)), kw)
	}
	return Hit{R: row, Kind: HitListItem, A: idx}
}

func (l *listOverlay) refilter() {
	if l.filter == nil {
		return
	}
	q := l.filter.Text()
	labels := make([]string, len(l.all))
	for i, it := range l.all {
		labels[i] = it.label
	}
	l.items, l.matches = nil, nil
	for _, rk := range core.FuzzyRank(q, labels) {
		l.items = append(l.items, l.all[rk.Index])
		l.matches = append(l.matches, rk.Match.Positions)
	}
	l.sel, l.top = 0, 0
}

func (l *listOverlay) move(d int) {
	n := len(l.items)
	if n == 0 {
		return
	}
	for i := 0; i < n; i++ {
		l.sel = (l.sel + d + n) % n
		if !l.items[l.sel].sep && !l.items[l.sel].off {
			break
		}
	}
	if l.sel < l.top {
		l.top = l.sel
	}
	if l.sel >= l.top+l.rows() {
		l.top = l.sel - l.rows() + 1
	}
}

// closeChain closes this list and every parent menu.
func (l *listOverlay) closeChain(a *App) {
	for p := l; p != nil; p = p.parent {
		a.closeOverlay(p)
	}
}

func (l *listOverlay) openSub(a *App, i int) {
	it := l.items[i]
	sub := newList("", it.sub, l.rect.Right()-1, l.rect.Y+1+i-l.top)
	sub.parent, sub.onLR = l, l.onLR
	sub.path = strings.TrimPrefix(l.path+" ▸ "+it.label, " ▸ ")
	a.push(sub)
}

func (l *listOverlay) run(a *App, i int) {
	if i < 0 || i >= len(l.items) || l.items[i].sep || l.items[i].off {
		return
	}
	it := l.items[i]
	if it.sub != nil {
		l.sel = i
		l.openSub(a, i)
		return
	}
	l.closeChain(a)
	if it.checked != nil {
		*it.checked = !*it.checked
	}
	if l.onRun != nil {
		l.onRun(a, it)
	}
	if it.run != nil {
		it.run(a)
	}
}

func (l *listOverlay) key(a *App, k tea.KeyPressMsg) {
	key := k.Keystroke()
	switch key {
	case "esc":
		a.closeOverlay(l)
		return
	case "up":
		l.move(-1)
		return
	case "down":
		l.move(1)
		return
	case "pgup", "pgdown":
		for i := 0; i < l.rows(); i++ {
			l.move(choose(key == "pgup", -1, 1))
		}
		return
	case "enter":
		l.run(a, l.sel)
		return
	case "right":
		if l.sel >= 0 && l.sel < len(l.items) && l.items[l.sel].sub != nil {
			l.openSub(a, l.sel)
			return
		}
		if l.onLR != nil {
			l.closeChain(a)
			l.onLR(a, 1)
			return
		}
	case "left":
		if l.parent != nil {
			a.closeOverlay(l)
			return
		}
		if l.onLR != nil {
			l.closeChain(a)
			l.onLR(a, -1)
			return
		}
	case "f10":
		if l.menuBar >= 0 {
			l.closeChain(a)
			return
		}
	}
	if l.filter != nil {
		if handled, changed := l.filter.Key(k); handled {
			if changed {
				l.refilter()
			}
		}
		return
	}
	if key == "space" {
		l.run(a, l.sel)
		return
	}
	// accelerator letters
	if k.Text != "" && k.Mod&(tea.ModCtrl|tea.ModAlt) == 0 {
		r := []rune(strings.ToLower(k.Text))[0]
		for i, it := range l.items {
			if it.accel == r {
				l.run(a, i)
				return
			}
		}
	}
}

func (l *listOverlay) mouse(a *App, m hitMsg) {
	if d, ok := isWheel(m); ok {
		l.move(d)
		return
	}
	inMe := m.ok && m.hit.Kind == HitListItem && l.rect.Contains(m.m.X, m.m.Y)
	if _, motion := m.ev.(tea.MouseMotionMsg); motion {
		if inMe {
			l.sel = m.hit.A
		} else if l.menuBar >= 0 && l.parent == nil {
			// hovering another menu title switches menus
			for _, h := range m.hits {
				if h.Kind == HitMenuTitle && h.R.Contains(m.m.X, m.m.Y) && h.A != l.menuBar {
					a.closeOverlay(l)
					a.openMenu(h.A)
				}
			}
		}
		return
	}
	if !isClick(m) {
		return
	}
	if inMe {
		l.run(a, m.hit.A)
		return
	}
	if l.parent != nil && l.parent.rect.Contains(m.m.X, m.m.Y) {
		a.closeOverlay(l)
		l.parent.mouse(a, m)
		return
	}
	for _, h := range m.hits {
		if h.Kind == HitMenuTitle && h.R.Contains(m.m.X, m.m.Y) {
			l.closeChain(a)
			if h.A != l.menuBar {
				a.openMenu(h.A)
			}
			return
		}
	}
	l.closeChain(a)
}

func (l *listOverlay) paste(a *App, s string) {
	if l.filter != nil && l.filter.Insert(s) {
		l.refilter()
	}
}

func (l *listOverlay) hints(a *App) []hint {
	out := []hint{{"↑↓", "Move", ""}, h("enter", "Run"), h("esc", "Close")}
	if l.filter != nil {
		return append([]hint{{"type", "Filter", ""}}, out...)
	}
	if l.onLR != nil || l.parent != nil {
		out = append(out, hint{"←→", "Menus", ""})
	}
	return out
}

// ================= confirm =================

type confirmDlg struct {
	title, text string
	yes         string
	onYes       func(a *App)
	onNo        func(a *App)
	sel         int // 0 cancel (the default), 1 yes
}

func (a *App) confirm(title, text, yes string, onYes func(a *App)) *confirmDlg {
	c := &confirmDlg{title: title, text: text, yes: yes, onYes: onYes}
	a.push(c)
	return c
}

func (c *confirmDlg) draw(a *App, b *Buf) []Hit {
	t := a.th()
	lines := wrap(c.text, 44)
	r := centered(50, len(lines)+6, b.W, b.H)
	fill := Sty{FG: t.FG, BG: t.Panel}
	hits := []Hit{backdrop(b)}
	b.Box(r, Sty{FG: t.Warn, BG: t.Panel}, fill, c.title, Sty{FG: t.Warn, BG: t.Panel, A: Bold})
	b.Shadow(r, t.Shadow)
	for i, l := range lines {
		b.Text(r.X+3, r.Y+2+i, l, fill, r.W-6)
	}
	y := r.Bottom() - 2
	labels := []string{"[ Cancel ]", "[ " + c.yes + " ]"}
	x := r.Right() - 3
	for i := 1; i >= 0; i-- {
		w := StrW(labels[i])
		x -= w
		st := Sty{FG: t.FG, BG: t.Panel2}
		if c.sel == i {
			st = Sty{FG: t.CellFG, BG: choose(i == 1, t.Err, t.CellBG), A: Bold}
		}
		b.Text(x, y, labels[i], st, w)
		hits = append(hits, Hit{R: Rect{x, y, w, 1}, Kind: HitButton, A: i})
		x -= 2
	}
	b.Text(r.X+3, y, "y / n", fill.Fg(t.Muted), 6)
	return hits
}

func (c *confirmDlg) answer(a *App, yes bool) {
	a.closeOverlay(c)
	if yes && c.onYes != nil {
		c.onYes(a)
	} else if !yes && c.onNo != nil {
		c.onNo(a)
	}
}

func (c *confirmDlg) key(a *App, k tea.KeyPressMsg) {
	switch keyName(k) {
	case "esc", "n", "N":
		c.answer(a, false)
	case "y", "Y":
		c.answer(a, true)
	case "left", "right", "tab", "shift+tab":
		c.sel = 1 - c.sel
	case "enter", "space":
		c.answer(a, c.sel == 1)
	}
}

func (c *confirmDlg) mouse(a *App, m hitMsg) {
	if isClick(m) && m.ok && m.hit.Kind == HitButton {
		c.answer(a, m.hit.A == 1)
	}
}

func (c *confirmDlg) paste(*App, string) {}

func (c *confirmDlg) hints(*App) []hint {
	return []hint{h("y", c.yes), h("n", "Cancel"), {"←→", "Choose", ""}, h("enter", "OK"), h("esc", "Cancel")}
}

// ================= message =================

type msgDlg struct {
	title string
	lines []string
}

func (a *App) message(title string, lines ...string) { a.push(&msgDlg{title, lines}) }

func (d *msgDlg) draw(a *App, b *Buf) []Hit {
	t := a.th()
	var body []string
	for _, l := range d.lines {
		body = append(body, wrap(l, 50)...)
	}
	r := centered(56, len(body)+6, b.W, b.H)
	fill := Sty{FG: t.FG, BG: t.Panel}
	b.Box(r, Sty{FG: t.Info, BG: t.Panel}, fill, d.title, Sty{FG: t.Info, BG: t.Panel, A: Bold})
	b.Shadow(r, t.Shadow)
	for i, l := range body {
		b.Text(r.X+3, r.Y+2+i, l, fill, r.W-6)
	}
	br := Rect{r.X + r.W/2 - 3, r.Bottom() - 2, 6, 1}
	b.Text(br.X, br.Y, "[ OK ]", t.sel().With(Bold), 6)
	return []Hit{backdrop(b), {R: br, Kind: HitButton}}
}

func (d *msgDlg) key(a *App, k tea.KeyPressMsg) {
	switch k.Keystroke() {
	case "esc", "enter", "space":
		a.closeOverlay(d)
	}
}

func (d *msgDlg) mouse(a *App, m hitMsg) {
	if isClick(m) && m.ok && m.hit.Kind == HitButton {
		a.closeOverlay(d)
	}
}

func (d *msgDlg) paste(*App, string) {}
func (d *msgDlg) hints(*App) []hint  { return []hint{h("enter", "OK"), h("esc", "Close")} }

// ================= one-line prompt =================

type promptDlg struct {
	title, label, help string
	in                 lineInput
	err                string
	// onOK returns an error to show (the dialog stays open) or nil.
	onOK func(a *App, text string) error
}

func (a *App) prompt(title, label, value string, onOK func(a *App, text string) error) *promptDlg {
	p := &promptDlg{title: title, label: label, onOK: onOK, help: "Enter OK · Esc cancel"}
	p.in.SetText(value)
	a.push(p)
	return p
}

func (p *promptDlg) draw(a *App, b *Buf) []Hit {
	t := a.th()
	r := centered(58, 7, b.W, b.H)
	fill := Sty{FG: t.FG, BG: t.Panel}
	b.Box(r, Sty{FG: t.Accent, BG: t.Panel}, fill, p.title, Sty{FG: t.Accent, BG: t.Panel, A: Bold})
	b.Shadow(r, t.Shadow)
	lw := StrW(p.label) + 1
	b.Text(r.X+2, r.Y+2, p.label, fill.Fg(t.Muted), lw)
	fr := Rect{r.X + 3 + lw, r.Y + 2, r.W - 5 - lw, 1}
	p.in.Draw(b, fr, t, true, fieldStyle(t, true))
	if p.err != "" {
		b.Text(fr.X, r.Y+3, "✗ "+p.err, fill.Fg(t.Err), fr.W)
	}
	b.Text(r.X+2, r.Bottom()-2, p.help, fill.Fg(t.Muted), r.W-4)
	ok := Rect{r.Right() - 9, r.Bottom() - 2, 6, 1}
	b.Text(ok.X, ok.Y, "[ OK ]", t.sel().With(Bold), 6)
	return []Hit{backdrop(b), {R: ok, Kind: HitButton}}
}

func (p *promptDlg) submit(a *App) {
	if err := p.onOK(a, p.in.Text()); err != nil {
		p.err = err.Error()
		return
	}
	a.closeOverlay(p)
}

func (p *promptDlg) key(a *App, k tea.KeyPressMsg) {
	switch k.Keystroke() {
	case "esc":
		a.closeOverlay(p)
	case "enter", "ctrl+s":
		p.submit(a)
	default:
		if _, changed := p.in.Key(k); changed {
			p.err = ""
		}
	}
}

func (p *promptDlg) mouse(a *App, m hitMsg) {
	if isClick(m) && m.ok && m.hit.Kind == HitButton {
		p.submit(a)
	}
}

func (p *promptDlg) paste(_ *App, s string) { p.in.Insert(s) }
func (p *promptDlg) hints(*App) []hint {
	return []hint{h("enter", "OK"), h("esc", "Cancel"), h("ctrl+u", "Clear")}
}

// ================= form (dialog.profile) =================

type formDlg struct {
	title  string
	labels []string
	ins    []*lineInput
	errs   []string
	focus  int // fields, then Save (n), Cancel (n+1)
	msg    string
	onSave func(a *App, f *formDlg) bool // true closes the form
	rect   Rect
}

func (f *formDlg) n() int { return len(f.ins) }

func (f *formDlg) draw(a *App, b *Buf) []Hit {
	t := a.th()
	h := 4 + 2*f.n() + 2
	r := centered(60, h, b.W, b.H)
	f.rect = r
	fill := Sty{FG: t.FG, BG: t.Panel}
	hits := []Hit{backdrop(b)}
	b.Box(r, Sty{FG: t.Accent, BG: t.Panel}, fill, f.title, Sty{FG: t.Accent, BG: t.Panel, A: Bold})
	b.Shadow(r, t.Shadow)
	y := r.Y + 2
	for i, in := range f.ins {
		lst := fill.Fg(choose(f.focus == i, t.Accent, t.Muted))
		if f.focus == i {
			lst = lst.With(Bold)
		}
		b.Fit(r.X+2, y, f.labels[i], lst, 8, 0)
		fr := Rect{r.X + 11, y, r.W - 14, 1}
		st := fieldStyle(t, f.focus == i)
		in.Draw(b, fr, t, f.focus == i, st)
		if f.errs[i] != "" {
			b.Restyle(fr, func(s Sty) Sty { return s.ULine(t.Err) })
			b.Text(fr.X, y+1, "✗ "+f.errs[i], fill.Fg(t.Err), fr.W)
		}
		hits = append(hits, Hit{R: Rect{r.X + 2, y, r.W - 4, 1}, Kind: HitField, A: i})
		y += 2
	}
	if f.msg != "" {
		b.Text(r.X+2, y, f.msg, fill.Fg(t.Err), r.W-4)
	}
	by := r.Bottom() - 2
	x := r.Right() - 3
	for i, lbl := range []string{"[ Cancel ]", "[ Save ]"} {
		w := StrW(lbl)
		x -= w
		idx := f.n() + 1 - i
		st := Sty{FG: t.FG, BG: t.Panel2}
		if f.focus == idx {
			st = t.sel().With(Bold)
		} else if idx == f.n() {
			st = st.Fg(t.Accent).With(Bold)
		}
		b.Text(x, by, lbl, st, w)
		hits = append(hits, Hit{R: Rect{x, by, w, 1}, Kind: HitButton, A: idx - f.n()})
		x -= 2
	}
	b.Text(r.X+2, by, "^S save · Tab next · Esc cancel", fill.Fg(t.Muted), max(0, x-r.X-3))
	return hits
}

func (f *formDlg) save(a *App) {
	if f.onSave(a, f) {
		a.closeOverlay(f)
	}
}

func (f *formDlg) key(a *App, k tea.KeyPressMsg) {
	key := k.Keystroke()
	total := f.n() + 2
	switch key {
	case "esc":
		a.closeOverlay(f)
		return
	case "ctrl+s":
		f.save(a)
		return
	case "tab", "down":
		f.focus = (f.focus + 1) % total
		return
	case "shift+tab", "up":
		f.focus = (f.focus + total - 1) % total
		return
	}
	if f.focus >= f.n() {
		switch key {
		case "left", "right":
			f.focus = choose(f.focus == f.n(), f.n()+1, f.n())
		case "enter", "space":
			if f.focus == f.n() {
				f.save(a)
			} else {
				a.closeOverlay(f)
			}
		}
		return
	}
	if key == "enter" {
		f.focus++
		return
	}
	if _, changed := f.ins[f.focus].Key(k); changed {
		f.errs[f.focus] = ""
		f.msg = ""
	}
}

func (f *formDlg) mouse(a *App, m hitMsg) {
	if !isClick(m) || !m.ok {
		return
	}
	switch m.hit.Kind {
	case HitField:
		f.focus = m.hit.A
	case HitButton:
		if m.hit.A == 0 {
			f.save(a)
		} else {
			a.closeOverlay(f)
		}
	}
}

func (f *formDlg) paste(_ *App, s string) {
	if f.focus < f.n() {
		f.ins[f.focus].Insert(s)
	}
}

func (f *formDlg) hints(*App) []hint {
	return []hint{h("tab", "Next field"), h("ctrl+s", "Save"), h("enter", "Next / press"), h("esc", "Cancel")}
}

// ================= help (F1) =================

type helpDlg struct {
	filter lineInput
	scroll int
}

func (a *App) openHelp() { a.push(&helpDlg{}) }

// helpLines: "#" starts a section, "" key = a paragraph line.
func (a *App) helpLines() [][2]string {
	var out [][2]string
	add := func(k, v string) { out = append(out, [2]string{k, v}) }
	cur := a.curTabInfo()
	add("#", fmt.Sprintf("This tab — %d %s", cur.Num, cur.Title))
	for _, l := range wrap(a.curTab().explain(), 90) {
		add("", l)
	}
	for _, k := range a.curTab().keyHelp() {
		add(k[0], k[1])
	}
	add("#", "Getting around")
	for _, kv := range [][2]string{
		{"↑ ↓ (sidebar)", "previous / next tab — switches immediately"},
		{"Home / End (sidebar)", "first / last tab"},
		{"Enter → Tab (sidebar)", "move into the tab body (first control)"},
		{"Tab / ⇧Tab", "next / previous control; ⇧Tab from the first returns to the sidebar"},
		{"Esc", "close a popup; with none open, back to the sidebar"},
		{"F6", "sidebar ⇄ tab body"},
		{"mouse", "click tabs, fields, items, buttons; drag sliders, cards and bars"},
	} {
		add(kv[0], kv[1])
	}
	add("#", "Global keys")
	for _, kv := range [][2]string{
		{"F1 / ^G", "this help (type to search)"},
		{"F10 · Alt+F / Alt+V / Alt+H", "menu bar"},
		{"^K / ^P", "command palette"},
		{"^PgDn / ^PgUp", "next / previous tab"},
		{"Alt+1…9, Alt+0", "go to tab 1–9, 10"},
		{"^R", "reset this tab to its defaults (asks first)"},
		{"^T", "next theme"},
		{"^L", "redraw"},
		{"⇧F10 · Menu · right-click", "context menu (Menus tab)"},
		{"^Q", "quit"},
	} {
		add(kv[0], kv[1])
	}
	add("#", "All tabs")
	for i, tb := range core.Tabs {
		for _, l := range wrap(fmt.Sprintf("%d %s — %s", tb.Num, tb.Title, a.tabs[i].explain()), 90) {
			add("", l)
		}
		for _, k := range a.tabs[i].keyHelp() {
			add(k[0], k[1])
		}
	}
	add("#", "How demo works")
	for _, l := range wrap("One server holds the current value of every control. Every client — TUIs in Go and Rust, "+
		"browsers — is connected live: a change made anywhere is sent as a `set` and broadcast to everyone, "+
		"and the Value panel shows who changed it and when. Each key has its own version; an edit carries "+
		"the version it started from, so if someone else changed that key first the server reports a "+
		"conflict and this client shows their value (⚠ in the status line).", 90) {
		add("", l)
	}
	return out
}

func (d *helpDlg) visible(a *App) [][2]string {
	q := strings.ToLower(strings.TrimSpace(d.filter.Text()))
	var lines [][2]string
	for _, l := range a.helpLines() {
		if q == "" || l[0] == "#" || strings.Contains(strings.ToLower(l[0]+" "+l[1]), q) {
			lines = append(lines, l)
		}
	}
	var kept [][2]string
	for i, l := range lines {
		if l[0] == "#" && (i+1 >= len(lines) || lines[i+1][0] == "#") {
			continue // drop sections with nothing left
		}
		kept = append(kept, l)
	}
	return kept
}

func (d *helpDlg) draw(a *App, b *Buf) []Hit {
	t := a.th()
	r := Rect{1, 1, b.W - 2, b.H - 2}
	fill := Sty{FG: t.FG, BG: t.Panel}
	b.Box(r, Sty{FG: t.Accent, BG: t.Panel}, fill, "Help — demo keyboard reference", Sty{FG: t.Accent, BG: t.Panel, A: Bold})
	b.Shadow(r, t.Shadow)
	b.Text(r.X+2, r.Y+1, "Search:", fill.Fg(t.Muted), 8)
	d.filter.Draw(b, Rect{r.X + 10, r.Y + 1, min(40, r.W-14), 1}, t, true, fieldStyle(t, true))
	lines := d.visible(a)
	body := Rect{r.X + 2, r.Y + 3, r.W - 4, r.H - 5}
	d.scroll = max(0, min(d.scroll, len(lines)-body.H))
	kw := min(30, body.W/3)
	for i := 0; i < body.H && d.scroll+i < len(lines); i++ {
		l := lines[d.scroll+i]
		y := body.Y + i
		switch l[0] {
		case "#":
			b.Text(body.X, y, l[1], fill.Fg(t.Accent).With(Bold|Underline), body.W)
		case "":
			b.Text(body.X+2, y, l[1], fill, body.W-2)
		default:
			b.Fit(body.X+2, y, l[0], fill.Fg(t.Warn).With(Bold), kw, 0)
			b.Text(body.X+kw+3, y, l[1], fill, body.W-kw-3)
		}
	}
	if len(lines) > body.H {
		pos := body.Y + d.scroll*body.H/len(lines)
		b.Set(r.Right()-1, pos, '┃', fill.Fg(t.Accent))
	}
	b.Text(r.X+2, r.Bottom()-2, "type to search · ↑↓ PgUp PgDn scroll · Esc close", fill.Fg(t.Muted), r.W-4)
	return []Hit{backdrop(b)}
}

func (d *helpDlg) key(a *App, k tea.KeyPressMsg) {
	switch k.Keystroke() {
	case "esc", "f1", "ctrl+g":
		a.closeOverlay(d)
	case "up":
		d.scroll--
	case "down":
		d.scroll++
	case "pgup":
		d.scroll -= 10
	case "pgdown":
		d.scroll += 10
	case "home", "ctrl+home":
		d.scroll = 0
	default:
		if _, changed := d.filter.Key(k); changed {
			d.scroll = 0
		}
	}
	d.scroll = max(0, d.scroll)
}

func (d *helpDlg) mouse(a *App, m hitMsg) {
	if dd, ok := isWheel(m); ok {
		d.scroll = max(0, d.scroll+dd*3)
	}
}

func (d *helpDlg) paste(_ *App, s string) { d.filter.Insert(s) }
func (d *helpDlg) hints(*App) []hint {
	return []hint{{"type", "Search", ""}, {"↑↓", "Scroll", ""}, {"PgUp/PgDn", "Page", ""}, h("esc", "Close")}
}
