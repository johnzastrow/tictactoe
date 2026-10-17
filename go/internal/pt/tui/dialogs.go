package tui

// Overlays (ui-spec §5): forms (login, new project / task with cascading
// selects, record form, small admin forms), list overlays (menus, context
// menu, command palette), confirm, columns, help. They stack; the top one
// gets keys and mouse first.

import (
	"fmt"
	"strings"

	tea "charm.land/bubbletea/v2"

	"git.wharf.fluidgrid.org/jcz/tictactoe/go/internal/pt/core"
)

type overlay interface {
	draw(a *App, b *Buf) []Hit
	key(a *App, k tea.KeyPressMsg) tea.Cmd
	mouse(a *App, m hitMsg) tea.Cmd
	paste(a *App, s string)
}

func (a *App) push(o overlay) {
	if f, ok := o.(*form); ok {
		f.setFocus(a, f.focus) // prime the first field (list filter, date preview)
	}
	a.overlays = append(a.overlays, o)
}

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

func isClick(m hitMsg) bool {
	c, ok := m.ev.(tea.MouseClickMsg)
	return ok && c.Button == tea.MouseLeft
}

func isWheel(m hitMsg) (int, bool) {
	w, ok := m.ev.(tea.MouseWheelMsg)
	if !ok {
		return 0, false
	}
	switch w.Button {
	case tea.MouseWheelUp:
		return -1, true
	case tea.MouseWheelDown:
		return 1, true
	}
	return 0, false
}

func centered(w, h, sw, sh int) Rect {
	w, h = min(w, sw-2), min(h, sh-2)
	return Rect{(sw - w) / 2, max(0, (sh-h)/2), w, h}
}

// ================= form =================

type form struct {
	title    string
	fields   []*Input
	labels   []string
	focus    int
	buttons  []string
	width    int
	labelW   int
	msg      string
	busy     bool
	onSave   func(a *App, f *form) tea.Cmd
	onChange func(a *App, f *form, i int)
	extra    func(a *App, f *form, b *Buf, r Rect)
	extraH   int
	touched  map[int]bool
	// record form
	ent     core.Entity
	id      int64
	bases   map[string]any
	theirs  map[string]any
	isLogin bool
	rect    Rect
	scroll  int // first field shown when the form is taller than the screen
}

func newForm(title string, width int) *form {
	return &form{title: title, width: width, labelW: 14, buttons: []string{"Save", "Cancel"}, touched: map[int]bool{}}
}

func (f *form) add(in *Input) *Input {
	f.fields = append(f.fields, in)
	return in
}

func (f *form) field(name string) *Input {
	for _, in := range f.fields {
		if in.Field == name || in.Label == name {
			return in
		}
	}
	return nil
}

func (f *form) idx(in *Input) int {
	for i, x := range f.fields {
		if x == in {
			return i
		}
	}
	return -1
}

func (f *form) layoutH() int {
	h := 3 // border + top padding + bottom border
	for _, in := range f.fields {
		h += fieldH(in)
	}
	h += f.extraH
	h += 3 // message, buttons, padding
	return h
}

func fieldH(in *Input) int {
	h := 1
	if in.Kind == IArea {
		h = max(3, in.Height)
	}
	if in.Err != "" || in.Hint != "" {
		h++
	}
	return h
}

func (f *form) draw(a *App, b *Buf) []Hit {
	t := a.th()
	var hits []Hit
	r := centered(f.width, min(f.layoutH(), b.H-2), b.W, b.H)
	f.rect = r
	bodyEnd := r.Bottom() - 3 - f.extraH // rows left for fields
	hits = append(hits, Hit{R: Rect{0, 0, b.W, b.H}, Kind: HitBackdrop})
	fill := Sty{FG: t.FG, BG: t.Panel}
	b.Box(r, Sty{FG: t.Accent, BG: t.Panel}, fill, f.title, Sty{FG: t.Accent, BG: t.Panel, A: Bold})
	b.Shadow(r, t.Shadow)
	hits = append(hits, Hit{R: r, Kind: HitDialog})
	y := r.Y + 2
	x := r.X + 2
	iw := r.W - 4 - f.labelW - 1
	var popupFor *Input
	var popupAt Rect
	if f.scroll > 0 {
		b.Text(r.Right()-12, r.Y+1, "▲ more", fill.Fg(t.Accent), 8)
	}
	for i, in := range f.fields {
		if i < f.scroll {
			continue
		}
		if y+fieldH(in) > bodyEnd {
			b.Text(r.Right()-12, bodyEnd, "▼ more", fill.Fg(t.Accent), 8)
			break
		}
		lst := fill.Fg(t.Muted)
		if i == f.focus {
			lst = fill.Fg(t.Accent).With(Bold)
		}
		label := in.Label
		if in.Required {
			label += "*"
		}
		b.Fit(x, y, label, lst, f.labelW, 0)
		h := 1
		if in.Kind == IArea {
			h = max(3, in.Height)
		}
		fr := Rect{x + f.labelW + 1, y, iw, h}
		if in.Kind == IChips || in.Kind.isList() && in.Kind != IChips {
			// lists are as wide as the field
		}
		in.Draw(b, fr, t, i == f.focus && !f.busy)
		hits = append(hits, Hit{R: Rect{x, y, r.W - 4, h}, Kind: HitField, A: i})
		y += h
		if in.Err != "" {
			b.Fit(fr.X, y, "⚠ "+in.Err, fill.Fg(t.Err), iw, 0)
			y++
		} else if in.Hint != "" {
			b.Fit(fr.X, y, in.Hint, fill.Fg(t.Muted).With(Italic), iw, 0)
			y++
		}
		if i == f.focus && (in.Kind.isList() || in.Kind == IDate) {
			popupFor, popupAt = in, fr
		}
	}
	if f.extra != nil && f.extraH > 0 {
		f.extra(a, f, b, Rect{x, y, r.W - 4, f.extraH})
		y += f.extraH
	}
	y++
	if f.msg != "" {
		b.Fit(x, y-1, "✗ "+f.msg, fill.Fg(t.Err).With(Bold), r.W-4, 0)
	}
	// buttons, right-aligned
	bx := r.Right() - 2
	for i := len(f.buttons) - 1; i >= 0; i-- {
		lbl := "[ " + f.buttons[i] + " ]"
		if i == 0 && f.busy {
			lbl = "[ … ]"
		}
		w := StrW(lbl)
		bx -= w
		st := Sty{FG: t.FG, BG: t.Panel2}
		if f.focus == len(f.fields)+i {
			st = Sty{FG: t.CellFG, BG: t.CellBG, A: Bold}
		} else if i == 0 {
			st = st.Fg(t.Accent).With(Bold)
		}
		b.Text(bx, y, lbl, st, w)
		hits = append(hits, Hit{R: Rect{bx, y, w, 1}, Kind: HitButton, A: i})
		bx -= 2
	}
	help := "^S save · Esc cancel · Tab next"
	if f.isLogin {
		help = "Enter log in · Tab next · Esc quit"
	}
	b.Text(x, y, help, fill.Fg(t.Muted), max(0, bx-x-1))
	if popupFor != nil && !f.busy {
		pw, ph := popupFor.PopupSize()
		pw = max(pw, min(popupAt.W, 60))
		pr := Rect{popupAt.X, popupAt.Y + 1, pw, ph}
		if pr.Bottom() > b.H {
			pr.Y = popupAt.Y - ph
		}
		for _, h := range popupFor.DrawPopup(b, pr, t) {
			h.Pane = 9 // popup of the focused form field
			hits = append(hits, h)
		}
	}
	return hits
}

func (f *form) setFocus(a *App, i int) {
	n := len(f.fields) + len(f.buttons)
	f.focus = (i + n) % n
	f.scrollTo(a)
	if f.focus < len(f.fields) {
		in := f.fields[f.focus]
		in.SetToday(a.rep.Today)
		in.refilter()
	}
}

// scrollTo keeps the focused field inside the visible part of the form.
func (f *form) scrollTo(a *App) {
	h := min(f.layoutH(), max(minH, a.h)-2)
	room := h - 6 - f.extraH // borders, padding, message, buttons
	if f.focus >= len(f.fields) || f.layoutH() <= max(minH, a.h)-2 {
		if f.layoutH() <= max(minH, a.h)-2 {
			f.scroll = 0
		}
		return
	}
	if f.focus < f.scroll {
		f.scroll = f.focus
	}
	for f.scroll < f.focus {
		used := 0
		for j := f.scroll; j <= f.focus; j++ {
			used += fieldH(f.fields[j])
		}
		if used <= room {
			break
		}
		f.scroll++
	}
}

func (f *form) key(a *App, k tea.KeyPressMsg) tea.Cmd {
	if f.busy {
		return nil
	}
	key := k.Keystroke()
	switch key {
	case "ctrl+s":
		return f.save(a)
	case "esc":
		a.closeOverlay(f)
		if f.isLogin {
			a.quitting = true
			return tea.Quit
		}
		return nil
	}
	if f.focus >= len(f.fields) {
		bi := f.focus - len(f.fields)
		switch key {
		case "tab", "right", "down":
			f.setFocus(a, f.focus+1)
		case "shift+tab", "left", "up":
			f.setFocus(a, f.focus-1)
		case "enter", "space":
			if bi == 0 {
				return f.save(a)
			}
			a.closeOverlay(f)
			if f.isLogin {
				a.quitting = true
				return tea.Quit
			}
		}
		return nil
	}
	in := f.fields[f.focus]
	// conflict resolution: Enter on a conflicted field keeps their value
	if key == "enter" && f.theirs != nil {
		if v, ok := f.theirs[in.Field]; ok {
			a.setInputValue(in, v)
			delete(f.theirs, in.Field)
			in.Err = ""
			return nil
		}
	}
	if (key == "up" || key == "down") && !in.Kind.isList() && in.Kind != IDate && in.Kind != INumber && in.Kind != IInt && in.Kind != IArea {
		if key == "up" {
			f.setFocus(a, f.focus-1)
		} else {
			f.setFocus(a, f.focus+1)
		}
		return nil
	}
	res := in.Key(k)
	switch res {
	case RNone:
		f.touched[f.focus] = true
		in.Err = ""
		f.msg = ""
		if f.onChange != nil {
			f.onChange(a, f, f.focus)
		}
	case RCommit, RCommitNext:
		if name, ok := in.CreateRequested(); ok && res == RCommit {
			return a.createProgramFromForm(f, in, name)
		}
		if in.Kind.isList() && in.Kind != IChips {
			if c, ok := in.selected(); ok && (in.navigated || len(in.text) > 0 && !in.matchesValue()) {
				in.SetValue(c.Value, in.allChoices())
				in.navigated = false
			} else if in.Kind != ISuggest && len(in.text) > 0 && !in.matchesValue() {
				in.Revert()
				in.Err = "no match — kept the previous value"
				return nil
			}
			f.touched[f.focus] = true
			if f.onChange != nil {
				f.onChange(a, f, f.focus)
			}
		}
		if f.isLogin && res == RCommit {
			return f.save(a)
		}
		f.setFocus(a, f.focus+1)
	case RCommitPrev:
		f.setFocus(a, f.focus-1)
	case RCancel:
		a.closeOverlay(f)
	}
	return nil
}

func (f *form) mouse(a *App, m hitMsg) tea.Cmd {
	if d, ok := isWheel(m); ok {
		if f.focus < len(f.fields) {
			f.fields[f.focus].Wheel(d)
		}
		return nil
	}
	if !isClick(m) || !m.ok {
		return nil
	}
	switch m.hit.Kind {
	case HitField:
		f.setFocus(a, m.hit.A)
		in := f.fields[m.hit.A]
		if in.Kind == IBool {
			b, _ := in.value.(bool)
			in.value, in.hasValue = !b, true
		}
	case HitButton:
		f.focus = len(f.fields) + m.hit.A
		if m.hit.A == 0 {
			return f.save(a)
		}
		a.closeOverlay(f)
		if f.isLogin {
			a.quitting = true
			return tea.Quit
		}
	case HitPopupItem, HitCalDay, HitCalPrev, HitCalNext:
		if f.focus < len(f.fields) {
			in := f.fields[f.focus]
			if in.Click(m.hit) {
				if name, ok := in.CreateRequested(); ok {
					return a.createProgramFromForm(f, in, name)
				}
				if c, ok := in.selected(); ok && in.Kind.isList() && in.Kind != IChips && in.Kind != ISuggest {
					in.SetValue(c.Value, in.allChoices())
				}
				f.touched[f.focus] = true
				if f.onChange != nil {
					f.onChange(a, f, f.focus)
				}
				f.setFocus(a, f.focus+1)
			}
		}
	case HitBackdrop:
		// clicks outside the dialog are ignored (forms are modal)
	}
	return nil
}

func (f *form) paste(a *App, s string) {
	if f.focus < len(f.fields) {
		f.fields[f.focus].Insert(s)
		if f.onChange != nil {
			f.onChange(a, f, f.focus)
		}
	}
}

func (f *form) save(a *App) tea.Cmd {
	if f.onSave == nil {
		a.closeOverlay(f)
		return nil
	}
	// required fields
	for i, in := range f.fields {
		if !in.Required {
			continue
		}
		empty := strings.TrimSpace(in.Text()) == ""
		if in.Kind == ISelect || in.Kind == ICombo {
			v, ok := in.Value()
			empty = !ok || v == nil
		}
		if empty {
			in.Err = "required"
			f.setFocus(a, i)
			return nil
		}
	}
	return f.onSave(a, f)
}

// conflict marks a field whose base was stale (record form).
func (f *form) conflict(a *App, c core.Conflict) {
	in := f.field(c.Field)
	if in == nil {
		return
	}
	who := "Someone"
	if c.By != nil {
		who = a.rep.Username(c.By)
	}
	in.Err = fmt.Sprintf("%s changed this to %s — Enter: take theirs · ^S: save mine", who, core.DisplayValue(a.rep, c.Entity, c.Field, c.Current))
	if f.theirs == nil {
		f.theirs = map[string]any{}
	}
	f.theirs[c.Field] = c.Current
	if f.bases != nil {
		f.bases[c.Field] = c.Current
	}
	f.msg = "Some fields changed while you were editing; review the marked fields."
}

// setInputValue loads a field value into an input.
func (a *App) setInputValue(in *Input, v any) {
	if in.Kind.isList() || in.Kind == IBool {
		in.SetValue(v, in.allChoices())
		if in.Kind == ISuggest {
			s, _ := v.(string)
			in.SetText(s)
		}
		return
	}
	if v == nil {
		in.SetText("")
		return
	}
	in.SetText(core.DisplayValue(a.rep, in.Ent, in.Field, v))
	if f, ok := core.AsFloat(v); ok && in.Kind != IText {
		in.SetText(core.FmtHours(f))
	}
	if s, ok := v.(string); ok {
		in.SetText(s)
	}
}

// inputValue converts an input to a field value.
func (a *App) inputValue(in *Input) (any, error) {
	switch in.Kind {
	case ISelect, ICombo:
		v, ok := in.Value()
		if !ok {
			return nil, fmt.Errorf("choose a value from the list")
		}
		return v, nil
	case IChips, IBool:
		v, _ := in.Value()
		return v, nil
	case IArea:
		return in.Text(), nil
	}
	if in.Field == "" || in.Kind == IPassword {
		return in.Text(), nil
	}
	return core.ParseInput(a.rep, in.Ent, in.Field, in.RowID, in.Text())
}

// fieldInput builds an input for an entity field (dialogs and record form).
func (a *App) fieldInput(ent core.Entity, field, label string, ed core.Editor, rowID int64, hint *int64) *Input {
	in := NewInput(kindFor(ed), label)
	in.Ent, in.Field, in.RowID, in.ProjectHint = ent, field, rowID, hint
	r := a.rep
	in.ChoicesFn = func() []core.Choice { return core.Choices(r, ent, field, rowID, in.ProjectHint) }
	in.SetToday(r.Today)
	if field == "program_id" && r.IsAdmin() {
		in.AllowCreate = true
	}
	return in
}

// createProgramFromForm: "✚ Create 'X'" in a program combobox (admins).
func (a *App) createProgramFromForm(f *form, in *Input, name string) tea.Cmd {
	prefix := ""
	for _, w := range strings.Fields(strings.ToUpper(name)) {
		if len(prefix) < 3 && w[0] >= 'A' && w[0] <= 'Z' {
			prefix += w[:1]
		}
	}
	return a.mutate([]core.Op{core.CreateOp(core.EntProgram, "p", map[string]any{"name": name, "prefix": prefix})}, nil,
		"Created program "+name, func(a *App, res core.ResultMsg) tea.Cmd {
			if id, ok := res.Created["p"]; ok {
				in.Reload()
				in.SetValue(id, in.allChoices())
				if f.onChange != nil {
					f.onChange(a, f, f.idx(in))
				}
			}
			return nil
		})
}

// ================= list overlay (menus, context menu, palette) =================

type menuItem struct {
	label  string
	key    string
	accel  rune
	run    func(a *App) tea.Cmd
	sep    bool
	off    bool
	detail string
}

type listOverlay struct {
	title   string
	items   []menuItem
	sel     int
	top     int
	x, y    int // anchor (menus/context); -1 = centred
	width   int
	filter  *Input
	all     []menuItem
	matches [][]int
	menuBar int // ≥0: dropdown of menu bar entry n
	maxRows int
}

func (l *listOverlay) rows() int {
	n := len(l.items)
	if l.maxRows > 0 {
		n = min(n, l.maxRows)
	}
	return max(1, n)
}

func (l *listOverlay) rect(b *Buf) Rect {
	w := l.width
	if w == 0 {
		w = 20
		for _, it := range l.items {
			w = max(w, StrW(it.label)+StrW(it.key)+6)
		}
	}
	h := l.rows() + 2
	if l.filter != nil {
		h += 2
	}
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
	r := l.rect(b)
	var hits []Hit
	hits = append(hits, Hit{R: Rect{0, 0, b.W, b.H}, Kind: HitBackdrop})
	fill := Sty{FG: t.FG, BG: t.Panel}
	b.Box(r, Sty{FG: t.Border, BG: t.Panel}, fill, l.title, Sty{FG: t.Accent, BG: t.Panel, A: Bold})
	b.Shadow(r, t.Shadow)
	y := r.Y + 1
	if l.filter != nil {
		l.filter.Draw(b, Rect{r.X + 2, y, r.W - 4, 1}, t, true)
		y += 2
		b.Text(r.X+2, y-1, strings.Repeat("─", r.W-4), fill.Fg(t.Border), r.W-4)
	}
	if len(l.items) == 0 {
		b.Text(r.X+2, y, "no matches", fill.Fg(t.Muted), r.W-4)
	}
	for i := 0; i < l.rows() && l.top+i < len(l.items); i++ {
		idx := l.top + i
		it := l.items[idx]
		row := Rect{r.X + 1, y + i, r.W - 2, 1}
		if it.sep {
			b.Text(row.X, row.Y, strings.Repeat("─", row.W), fill.Fg(t.Border), row.W)
			continue
		}
		st := fill
		if idx == l.sel {
			st = Sty{FG: t.CellFG, BG: t.CellBG}
		}
		if it.off {
			st = st.Fg(t.Muted)
		}
		b.Fill(row, ' ', st)
		x := row.X + 1
		var m []int
		if l.matches != nil && idx < len(l.matches) {
			m = l.matches[idx]
		}
		mset := map[int]bool{}
		for _, v := range m {
			mset[v] = true
		}
		accelDone := false
		for ci, ch := range []rune(it.label) {
			s := st
			if mset[ci] {
				s = s.With(Bold | Underline)
			}
			if !accelDone && it.accel != 0 && (ch == it.accel || ch == it.accel-32 || ch == it.accel+32) {
				s = s.With(Underline)
				if idx != l.sel {
					s = s.Fg(t.Accent)
				}
				accelDone = true
			}
			if x >= row.Right()-StrW(it.key)-2 {
				break
			}
			x += b.Set(x, row.Y, ch, s)
		}
		right := it.key
		if it.detail != "" {
			right = it.detail
		}
		if right != "" {
			kw := StrW(right)
			ks := st.Fg(t.Muted)
			if idx == l.sel {
				ks = st
			}
			b.Text(row.Right()-kw-1, row.Y, right, ks, kw)
		}
		hits = append(hits, Hit{R: row, Kind: HitListItem, A: idx})
	}
	if len(l.items) > l.rows() {
		pos := y + l.top*l.rows()/len(l.items)
		b.Set(r.Right()-1, min(pos, r.Bottom()-2), '┃', Sty{FG: t.Accent, BG: t.Panel})
	}
	return hits
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
	if strings.TrimSpace(q) == "" {
		l.items = l.all
	} else {
		for _, rk := range core.Rank(q, labels) {
			l.items = append(l.items, l.all[rk.Index])
			l.matches = append(l.matches, rk.Match.Indices)
		}
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

func (l *listOverlay) run(a *App, i int) tea.Cmd {
	if i < 0 || i >= len(l.items) || l.items[i].sep || l.items[i].off {
		return nil
	}
	it := l.items[i]
	a.closeOverlay(l)
	if it.run != nil {
		return it.run(a)
	}
	return nil
}

func (l *listOverlay) key(a *App, k tea.KeyPressMsg) tea.Cmd {
	key := k.Keystroke()
	switch key {
	case "esc":
		a.closeOverlay(l)
		return nil
	case "up":
		l.move(-1)
		return nil
	case "down":
		l.move(1)
		return nil
	case "pgup":
		for i := 0; i < l.rows(); i++ {
			l.move(-1)
		}
		return nil
	case "pgdown":
		for i := 0; i < l.rows(); i++ {
			l.move(1)
		}
		return nil
	case "enter":
		return l.run(a, l.sel)
	case "left", "right":
		if l.menuBar >= 0 {
			d := 1
			if key == "left" {
				d = -1
			}
			a.closeOverlay(l)
			a.openMenu((l.menuBar + d + len(menuBar)) % len(menuBar))
			return nil
		}
	case "f10":
		if l.menuBar >= 0 {
			a.closeOverlay(l)
			return nil
		}
	}
	if l.filter != nil {
		if l.filter.Key(k) == RNone {
			l.refilter()
		}
		return nil
	}
	// accelerator letters
	if k.Text != "" {
		r := []rune(strings.ToLower(k.Text))[0]
		for i, it := range l.items {
			if it.accel != 0 && (it.accel == r || it.accel+32 == r) {
				return l.run(a, i)
			}
		}
	}
	return nil
}

func (l *listOverlay) mouse(a *App, m hitMsg) tea.Cmd {
	if d, ok := isWheel(m); ok {
		l.move(d)
		return nil
	}
	if _, motion := m.ev.(tea.MouseMotionMsg); motion && m.ok && m.hit.Kind == HitListItem {
		l.sel = m.hit.A
		return nil
	}
	if motion, ok := m.ev.(tea.MouseMotionMsg); ok && l.menuBar >= 0 {
		// hovering another menu title switches menus
		if h, ok := hitOfKind(m.hits, motion.X, motion.Y, HitMenuTitle); ok && h.A != l.menuBar {
			a.closeOverlay(l)
			a.openMenu(h.A)
		}
		return nil
	}
	if !isClick(m) {
		return nil
	}
	if m.ok && m.hit.Kind == HitListItem {
		return l.run(a, m.hit.A)
	}
	if mc, ok := m.ev.(tea.MouseClickMsg); ok {
		if h, ok := hitOfKind(m.hits, mc.X, mc.Y, HitMenuTitle); ok {
			a.closeOverlay(l)
			if h.A != l.menuBar {
				a.openMenu(h.A)
			}
			return nil
		}
	}
	a.closeOverlay(l)
	return nil
}

func (l *listOverlay) paste(a *App, s string) {
	if l.filter != nil {
		l.filter.Insert(s)
		l.refilter()
	}
}

// ================= confirm =================

type confirmDlg struct {
	title, text string
	yes         string
	onYes       func(a *App) tea.Cmd
	sel         int // 0 cancel (default), 1 yes
}

func (c *confirmDlg) draw(a *App, b *Buf) []Hit {
	t := a.th()
	lines := wrap(c.text, 52)
	r := centered(58, len(lines)+6, b.W, b.H)
	fill := Sty{FG: t.FG, BG: t.Panel}
	hits := []Hit{{R: Rect{0, 0, b.W, b.H}, Kind: HitBackdrop}}
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
			st = Sty{FG: t.CellFG, BG: t.CellBG, A: Bold}
			if i == 1 {
				st.BG = t.Err
			}
		}
		b.Text(x, y, labels[i], st, w)
		hits = append(hits, Hit{R: Rect{x, y, w, 1}, Kind: HitButton, A: i})
		x -= 2
	}
	b.Text(r.X+3, y, "y / n", fill.Fg(t.Muted), 6)
	return hits
}

func (c *confirmDlg) key(a *App, k tea.KeyPressMsg) tea.Cmd {
	switch k.Keystroke() {
	case "esc", "n":
		a.closeOverlay(c)
	case "y":
		a.closeOverlay(c)
		return c.onYes(a)
	case "left", "right", "tab", "shift+tab":
		c.sel = 1 - c.sel
	case "enter", "space":
		a.closeOverlay(c)
		if c.sel == 1 {
			return c.onYes(a)
		}
	}
	return nil
}

func (c *confirmDlg) mouse(a *App, m hitMsg) tea.Cmd {
	if isClick(m) && m.ok && m.hit.Kind == HitButton {
		a.closeOverlay(c)
		if m.hit.A == 1 {
			return c.onYes(a)
		}
	}
	return nil
}

func (c *confirmDlg) paste(*App, string) {}

func wrap(s string, w int) []string {
	var out []string
	for _, para := range strings.Split(s, "\n") {
		line := ""
		for _, word := range strings.Fields(para) {
			if line != "" && StrW(line)+1+StrW(word) > w {
				out = append(out, line)
				line = word
			} else if line == "" {
				line = word
			} else {
				line += " " + word
			}
		}
		out = append(out, line)
	}
	return out
}

// ================= columns dialog (F4) =================

type columnsDlg struct {
	g   *Grid
	sel int
	top int
}

func (c *columnsDlg) draw(a *App, b *Buf) []Hit {
	t := a.th()
	n := len(c.g.Cols)
	rows := min(n, b.H-8)
	r := centered(48, rows+5, b.W, b.H)
	fill := Sty{FG: t.FG, BG: t.Panel}
	hits := []Hit{{R: Rect{0, 0, b.W, b.H}, Kind: HitBackdrop}}
	b.Box(r, Sty{FG: t.Accent, BG: t.Panel}, fill, "Columns", Sty{FG: t.Accent, BG: t.Panel, A: Bold})
	b.Shadow(r, t.Shadow)
	if c.sel < c.top {
		c.top = c.sel
	}
	if c.sel >= c.top+rows {
		c.top = c.sel - rows + 1
	}
	for i := 0; i < rows && c.top+i < n; i++ {
		idx := c.top + i
		col := c.g.Cols[idx]
		st := fill
		if idx == c.sel {
			st = Sty{FG: t.CellFG, BG: t.CellBG}
		}
		row := Rect{r.X + 1, r.Y + 1 + i, r.W - 2, 1}
		b.Fill(row, ' ', st)
		box := "[✓]"
		if col.Hidden {
			box = "[ ]"
		}
		b.Text(row.X+1, row.Y, box+" "+col.Title, st, row.W-10)
		b.Fit(row.Right()-8, row.Y, fmt.Sprintf("%3d ch", col.Width), st.Fg(t.Muted), 7, 1)
		hits = append(hits, Hit{R: row, Kind: HitListItem, A: idx})
	}
	b.Text(r.X+2, r.Bottom()-2, "Space show/hide · [ ] move · + - width · r reset · Esc", fill.Fg(t.Muted), r.W-4)
	return hits
}

func (c *columnsDlg) key(a *App, k tea.KeyPressMsg) tea.Cmd {
	cols := c.g.Cols
	switch k.Keystroke() {
	case "esc", "enter", "f4":
		a.closeOverlay(c)
	case "up":
		c.sel = max(0, c.sel-1)
	case "down":
		c.sel = min(len(cols)-1, c.sel+1)
	case "space":
		visible := 0
		for _, x := range cols {
			if !x.Hidden {
				visible++
			}
		}
		if !cols[c.sel].Hidden && visible <= 1 {
			return nil
		}
		cols[c.sel].Hidden = !cols[c.sel].Hidden
		a.rebuild()
	case "[", "shift+up":
		if c.sel > 0 {
			cols[c.sel], cols[c.sel-1] = cols[c.sel-1], cols[c.sel]
			c.sel--
			a.rebuild()
		}
	case "]", "shift+down":
		if c.sel < len(cols)-1 {
			cols[c.sel], cols[c.sel+1] = cols[c.sel+1], cols[c.sel]
			c.sel++
			a.rebuild()
		}
	case "+", "=":
		cols[c.sel].Width = min(80, cols[c.sel].Width+1)
	case "-":
		cols[c.sel].Width = max(3, cols[c.sel].Width-1)
	case "r":
		fresh := tuneCols(defaultColumns(a, c.g))
		c.g.Cols = fresh
		a.rebuild()
	}
	return nil
}

func defaultColumns(a *App, g *Grid) []core.Column {
	switch g.Name {
	case "projects":
		return core.ProjectColumns(a.rep)
	case "projtasks":
		return core.TaskColumns(a.rep, false)
	case "tasks":
		return core.TaskColumns(a.rep, true)
	case "activity":
		return core.ActivityColumns()
	case "templates":
		return core.TemplateColumns()
	case "tmpltasks":
		return core.TemplateTaskColumns()
	case "fields":
		return core.CustomFieldColumns()
	case "users":
		return core.UserColumns()
	}
	return core.ProgramColumns()
}

func (c *columnsDlg) mouse(a *App, m hitMsg) tea.Cmd {
	if d, ok := isWheel(m); ok {
		c.sel = max(0, min(len(c.g.Cols)-1, c.sel+d))
		return nil
	}
	if isClick(m) && m.ok {
		switch m.hit.Kind {
		case HitListItem:
			if c.sel == m.hit.A {
				c.g.Cols[c.sel].Hidden = !c.g.Cols[c.sel].Hidden
				a.rebuild()
			}
			c.sel = m.hit.A
		case HitBackdrop:
			a.closeOverlay(c)
		}
	}
	return nil
}

func (c *columnsDlg) paste(*App, string) {}

// ================= help (F1) =================

type helpDlg struct {
	filter *Input
	scroll int
}

func (h *helpDlg) lines() [][2]string {
	var out [][2]string
	add := func(k, v string) { out = append(out, [2]string{k, v}) }
	add("#", "How pt works")
	add("", "Every change you make is sent to the server and appears live in every other TUI and browser.")
	add("", "If someone changed a value since you looked at it, your edit is not applied and the cell turns red (⚠).")
	add("", "Filters run on your machine: type in the filter bar (^F) to narrow any grid instantly.")
	add("#", "Filter language")
	add("word  \"two words\"", "free text")
	add("status:active,on_hold", "field equals one of")
	add("due<+14d  est>=8", "compare dates and numbers")
	add("@me  @alice  #tag", "people and tags")
	add("is:overdue  is:open  has:deps", "predicates")
	add("-status:done", "negate a term")
	ctxNames := map[Ctx]string{CtxGlobal: "Everywhere", CtxGrid: "Grids", CtxBoard: "Board", CtxTimeline: "Timeline", CtxDashboard: "Dashboard"}
	for _, ctx := range []Ctx{CtxGlobal, CtxGrid, CtxBoard, CtxTimeline, CtxDashboard} {
		add("#", ctxNames[ctx])
		for _, b := range bindings {
			if b.Ctx != ctx {
				continue
			}
			var ks []string
			for _, k := range b.Keys {
				ks = append(ks, keyLabel(k))
			}
			add(strings.Join(ks, " / "), b.Action.Label())
		}
	}
	add("#", "Grid navigation & editing")
	for _, kv := range [][2]string{
		{"↑ ↓ ← →  Tab ⇧Tab", "move between cells"},
		{"Home End ^Home ^End PgUp PgDn", "jump"},
		{"Enter / F2 / double-click", "edit the cell"},
		{"type a character", "start editing with it"},
		{"Enter · Tab · Esc (in editor)", "save & down · save & right · cancel"},
		{"Space · ⇧↑ ⇧↓ · ^A", "mark rows"},
		{"^C · ^V · ^D", "copy TSV · paste TSV · fill down"},
		{"Del", "clear the cell"},
		{"Enter / ← → on a group row", "collapse / expand"},
		{"click header · ⇧click", "sort · add sort key"},
		{"drag header edge", "resize column"},
		{"right-click / ⇧F10", "context menu"},
	} {
		add(kv[0], kv[1])
	}
	add("#", "Editors")
	for _, kv := range [][2]string{
		{"dates", "today, tom, fri, +3d, -2w, eom, 11/3, nov 3 · ↑↓ week · PgUp/PgDn month · ^←/^→ day"},
		{"lists", "type to filter (fuzzy) · ↑↓ · Enter"},
		{"tags / deps", "type, Enter adds · Backspace removes · Tab or Enter on empty saves"},
		{"numbers", "↑↓ ±1 · ⇧↑↓ ±0.25"},
		{"dialogs", "Tab moves · ^S saves · Esc cancels"},
	} {
		add(kv[0], kv[1])
	}
	return out
}

func (h *helpDlg) draw(a *App, b *Buf) []Hit {
	t := a.th()
	r := Rect{2, 1, b.W - 4, b.H - 2}
	fill := Sty{FG: t.FG, BG: t.Panel}
	b.Box(r, Sty{FG: t.Accent, BG: t.Panel}, fill, "Help — pt keyboard reference (F1)", Sty{FG: t.Accent, BG: t.Panel, A: Bold})
	b.Shadow(r, t.Shadow)
	b.Text(r.X+2, r.Y+1, "Search:", fill.Fg(t.Muted), 8)
	h.filter.Draw(b, Rect{r.X + 10, r.Y + 1, min(40, r.W-14), 1}, t, true)
	q := strings.ToLower(h.filter.Text())
	var lines [][2]string
	for _, l := range h.lines() {
		if q == "" || l[0] == "#" || strings.Contains(strings.ToLower(l[0]+" "+l[1]), q) {
			lines = append(lines, l)
		}
	}
	// drop empty sections
	var kept [][2]string
	for i, l := range lines {
		if l[0] == "#" && (i+1 >= len(lines) || lines[i+1][0] == "#") {
			continue
		}
		kept = append(kept, l)
	}
	body := Rect{r.X + 2, r.Y + 3, r.W - 4, r.H - 5}
	h.scroll = max(0, min(h.scroll, len(kept)-body.H))
	for i := 0; i < body.H && h.scroll+i < len(kept); i++ {
		l := kept[h.scroll+i]
		y := body.Y + i
		if l[0] == "#" {
			b.Text(body.X, y, l[1], fill.Fg(t.Accent).With(Bold|Underline), body.W)
			continue
		}
		if l[0] == "" {
			b.Text(body.X+2, y, l[1], fill, body.W-2)
			continue
		}
		b.Fit(body.X+2, y, l[0], fill.Fg(t.Warn).With(Bold), 32, 0)
		b.Text(body.X+35, y, l[1], fill, body.W-35)
	}
	b.Text(r.X+2, r.Bottom()-2, "type to search · ↑↓ PgUp PgDn scroll · Esc close", fill.Fg(t.Muted), r.W-4)
	return []Hit{{R: Rect{0, 0, b.W, b.H}, Kind: HitBackdrop}}
}

func (h *helpDlg) key(a *App, k tea.KeyPressMsg) tea.Cmd {
	switch k.Keystroke() {
	case "esc", "f1", "ctrl+g":
		a.closeOverlay(h)
	case "up":
		h.scroll--
	case "down":
		h.scroll++
	case "pgup":
		h.scroll -= 10
	case "pgdown":
		h.scroll += 10
	default:
		if h.filter.Key(k) == RNone {
			h.scroll = 0
		}
	}
	h.scroll = max(0, h.scroll)
	return nil
}

func (h *helpDlg) mouse(a *App, m hitMsg) tea.Cmd {
	if d, ok := isWheel(m); ok {
		h.scroll = max(0, h.scroll+d*3)
	}
	return nil
}

func (h *helpDlg) paste(a *App, s string) { h.filter.Insert(s) }

// ================= notes editor =================

func (a *App) openNotes(ent core.Entity, id int64, field string) {
	f := newForm("Notes — "+a.rep.RowLabel(ent, id), min(90, a.w-4))
	in := a.fieldInput(ent, field, "Notes", core.EdMultiline, id, nil)
	in.Height = max(6, min(18, a.h-14))
	base := a.rep.FieldValue(ent, id, field)
	s, _ := base.(string)
	in.SetText(s)
	f.labelW = 6
	f.add(in)
	f.onSave = func(a *App, f *form) tea.Cmd {
		v := in.Text()
		if v == s {
			a.closeOverlay(f)
			return nil
		}
		f.bases = map[string]any{field: base}
		return a.mutateForm(f, []core.Op{core.SetOp(ent, id, field, base, v)}, []cellKey{{ent, id, field}}, "Notes saved", nil)
	}
	f.buttons = []string{"Save", "Cancel"}
	a.push(f)
}
