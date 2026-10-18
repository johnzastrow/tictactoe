package tui

// List controls: the popup list shared by dropdowns, the combobox and the
// tag suggestions, and tabs 4 Dropdown, 5 Combobox, 6 Tags and 9 Cascade.

import (
	"fmt"
	"strings"

	tea "charm.land/bubbletea/v2"

	"git.wharf.fluidgrid.org/jcz/tictactoe/go/internal/demo/core"
)

const popupRows = 8

type pickItem struct {
	id, label, detail string
	color             string // #rrggbb dot before the label ("" = none)
	matched           []int  // rune positions in label to highlight
	create            bool   // the "✚ Create" entry
}

// pickList is an open popup list: items (already filtered), selection and
// scroll.
type pickList struct {
	items    []pickItem
	sel, top int
	limit    int  // rows shown (0 = popupRows)
	rect     Rect // last drawn
}

func (p *pickList) rows() int {
	if p.limit > 0 {
		return p.limit
	}
	return popupRows
}

func (p *pickList) move(d int) {
	if len(p.items) == 0 {
		return
	}
	p.sel = max(0, min(len(p.items)-1, p.sel+d))
	p.scroll()
}

func (p *pickList) scroll() {
	if p.sel < p.top {
		p.top = p.sel
	}
	if p.sel >= p.top+p.rows() {
		p.top = p.sel - p.rows() + 1
	}
	p.top = max(0, min(p.top, len(p.items)-1))
}

func (p *pickList) current() (pickItem, bool) {
	if p.sel >= 0 && p.sel < len(p.items) {
		return p.items[p.sel], true
	}
	return pickItem{}, false
}

// listKey handles navigation keys common to every popup list.
func (p *pickList) listKey(key string) bool {
	switch key {
	case "up":
		p.move(-1)
	case "down":
		p.move(1)
	case "pgup":
		p.move(-p.rows())
	case "pgdown":
		p.move(p.rows())
	default:
		return false
	}
	return true
}

// draw puts the popup under the field at fr (above it when there is no
// room below) and returns one hit per row (S "item", A index, B ctl).
func (p *pickList) draw(a *App, b *Buf, fr Rect, w, ctl int, empty string) []Hit {
	t := a.th()
	p.scroll()
	rows := max(1, min(p.rows(), len(p.items)))
	r := Rect{fr.X, fr.Y + 1, max(w, fr.W), rows + 2}
	if r.Bottom() > b.Clip.Bottom() && fr.Y-rows-2 >= b.Clip.Y {
		r.Y = fr.Y - rows - 2
	}
	r.X = min(r.X, b.Clip.Right()-r.W)
	p.rect = r
	fill := Sty{FG: t.FG, BG: t.Panel}
	b.Box(r, Sty{FG: t.Accent, BG: t.Panel}, fill, "", fill)
	b.Shadow(r, t.Shadow)
	hits := []Hit{{R: r, Kind: HitCtl, S: "popup", B: ctl}}
	if len(p.items) == 0 {
		b.Text(r.X+2, r.Y+1, empty, fill.Fg(t.Muted), r.W-4)
		return hits
	}
	for i := 0; i < rows && p.top+i < len(p.items); i++ {
		idx := p.top + i
		it := p.items[idx]
		row := Rect{r.X + 1, r.Y + 1 + i, r.W - 2, 1}
		st := fill
		if idx == p.sel {
			st = t.sel()
		}
		b.Fill(row, ' ', st)
		x := row.X + 1
		if it.create {
			b.Text(x, row.Y, it.label, st.Fg(choose(idx == p.sel, t.CellFG, t.OK)).With(Bold), row.W-2)
			hits = append(hits, Hit{R: row, Kind: HitCtl, S: "item", A: idx, B: ctl})
			continue
		}
		if it.color != "" {
			b.Set(x, row.Y, '●', st.Fg(hex(it.color)))
			x += 2
		}
		mset := map[int]bool{}
		for _, m := range it.matched {
			mset[m] = true
		}
		dw := StrW(it.detail)
		for ci, ch := range []rune(it.label) {
			s := st
			if mset[ci] {
				s = s.With(Bold | Underline).Fg(choose(idx == p.sel, t.CellFG, t.Accent))
			}
			if x >= row.Right()-1 {
				break
			}
			x += b.Set(x, row.Y, ch, s)
		}
		if it.detail != "" && x+2+dw < row.Right() {
			b.Text(row.Right()-dw-1, row.Y, it.detail, st.Fg(choose(idx == p.sel, t.CellFG, t.Muted)), dw)
		}
		hits = append(hits, Hit{R: row, Kind: HitCtl, S: "item", A: idx, B: ctl})
	}
	if len(p.items) > rows {
		pos := r.Y + 1 + p.top*rows/len(p.items)
		b.Set(r.Right()-1, pos, '┃', fill.Fg(t.Accent))
	}
	return hits
}

// ================= dropdown =================

// dropdown is a closed field showing the value as a coloured chip; it opens
// a filterable list (ui-spec tab 4; also tab 9's three levels).
type dropdown struct {
	label    string
	choices  func(a *App) []core.Choice
	current  func(a *App) string // id ("" = none)
	pick     func(a *App, id string)
	disabled func(a *App) string // non-empty: why it is disabled
	clear    func(a *App)        // Del (nil: not clearable)
	flashKey string

	open   bool
	filter lineInput
	list   pickList
}

func (d *dropdown) refilter(a *App) {
	ch := d.choices(a)
	labels := make([]string, len(ch))
	for i, c := range ch {
		labels[i] = c.Label
	}
	d.list.items = nil
	for _, rk := range core.FuzzyRank(d.filter.Text(), labels) {
		c := ch[rk.Index]
		d.list.items = append(d.list.items, pickItem{id: c.ID, label: c.Label, color: c.Color, matched: rk.Match.Positions})
	}
	d.list.sel, d.list.top = 0, 0
}

func (d *dropdown) openList(a *App) {
	if d.disabled != nil {
		if why := d.disabled(a); why != "" {
			a.info(why)
			return
		}
	}
	d.open = true
	d.filter.SetText("")
	d.refilter(a)
	cur := d.current(a)
	for i, it := range d.list.items {
		if it.id == cur {
			d.list.sel = i
		}
	}
	d.list.scroll()
}

// key handles a key for a focused dropdown.
func (d *dropdown) key(a *App, k tea.KeyPressMsg) bool {
	key := k.Keystroke()
	if !d.open {
		switch key {
		case "enter", "space", "down":
			d.openList(a)
			return true
		case "delete":
			if d.clear != nil {
				d.clear(a)
				return true
			}
		}
		// typing opens the list filtered by that letter
		if k.Text != "" && k.Mod&(tea.ModCtrl|tea.ModAlt) == 0 && key != "space" {
			d.openList(a)
			if d.open {
				d.filter.Insert(k.Text)
				d.refilter(a)
			}
			return true
		}
		return false
	}
	switch key {
	case "esc":
		d.open = false
		return true
	case "enter":
		if it, ok := d.list.current(); ok {
			d.open = false
			d.pick(a, it.id)
		}
		return true
	case "tab", "shift+tab":
		d.open = false
		return false
	}
	if d.list.listKey(key) {
		return true
	}
	if handled, changed := d.filter.Key(k); handled {
		if changed {
			d.refilter(a)
		}
		return true
	}
	return true // the open list is modal
}

// draw renders "Label  [● Value ▾]" and, when open, the list.
func (d *dropdown) draw(a *App, b *Buf, x, y, labelW, w int, focused bool, ctl int) (Rect, []Hit) {
	t := a.th()
	off := d.disabled != nil && d.disabled(a) != ""
	ls := t.base().Fg(choose(focused, t.Accent, t.Muted))
	if focused {
		ls = ls.With(Bold)
	}
	b.Fit(x, y, d.label, ls, labelW, 0)
	fr := Rect{x + labelW, y, w, 1}
	st := fieldStyle(t, focused)
	if _, ok := a.flash[d.flashKey]; ok && d.flashKey != "" {
		st = Sty{FG: t.BG, BG: t.Warn}
	}
	b.Fill(fr, ' ', st)
	cur := d.current(a)
	var c core.Choice
	found := false
	for _, ch := range d.choices(a) {
		if ch.ID == cur {
			c, found = ch, true
		}
	}
	switch {
	case off:
		b.Text(fr.X+1, y, d.disabled(a), st.Fg(t.Muted).With(Italic), fr.W-3)
	case d.open:
		d.filter.placeholder = "type to filter"
		d.filter.Draw(b, Rect{fr.X + 1, y, fr.W - 3, 1}, t, true, st)
	case !found:
		b.Text(fr.X+1, y, "— none —", st.Fg(t.Muted), fr.W-3)
	default:
		xx := fr.X + 1
		if c.Color != "" {
			b.Set(xx, y, '●', st.Fg(hex(c.Color)))
			xx += 2
			b.Text(xx, y, c.Label, st.Fg(hex(c.Color)).With(Bold), fr.Right()-xx-2)
		} else {
			b.Text(xx, y, core.SafeText(c.Label), st, fr.Right()-xx-2)
		}
	}
	b.Set(fr.Right()-2, y, choose(d.open, '▴', '▾'), st.Fg(t.Muted))
	hits := []Hit{{R: Rect{x, y, labelW + w, 1}, Kind: HitCtl, S: "drop", A: ctl}}
	return fr, hits
}

// drawPopup draws the open list (after the other controls, so it covers them).
func (d *dropdown) drawPopup(a *App, b *Buf, fr Rect, ctl int) []Hit {
	if !d.open {
		return nil
	}
	return d.list.draw(a, b, fr, fr.W, ctl, "no matches")
}

// mouse handles clicks on a dropdown's field or its list.
func (d *dropdown) mouse(a *App, h Hit) {
	switch h.S {
	case "drop":
		if d.open {
			d.open = false
		} else {
			d.openList(a)
		}
	case "item":
		d.list.sel = h.A
		if it, ok := d.list.current(); ok {
			d.open = false
			d.pick(a, it.id)
		}
	}
}

// ================= 4 Dropdown =================

type selectTab struct {
	ctlBase
	drops [2]*dropdown
}

func newSelectTab() *selectTab {
	t := &selectTab{}
	for i, kc := range []struct {
		key, label string
		list       []core.Choice
	}{{"select.status", "Status", core.Statuses}, {"select.priority", "Priority", core.Priorities}} {
		key, list := kc.key, kc.list
		t.drops[i] = &dropdown{
			label: kc.label, flashKey: key,
			choices: func(*App) []core.Choice { return list },
			current: func(a *App) string { return a.vals.String(key) },
			pick: func(a *App, id string) {
				if err := a.commit(key, id); err != nil && err != errOffline {
					a.setStatus("✗ "+err.Error(), kErr)
				}
			},
		}
	}
	return t
}

func (t *selectTab) explain() string {
	return "Closed dropdowns that show the value as a coloured chip and open a list you can filter by typing."
}

func (t *selectTab) controls() int { return 2 }

func (t *selectTab) keyHelp() [][2]string {
	return [][2]string{
		{"Enter Space ↓", "open"}, {"↑↓", "move"}, {"type", "filter / jump"}, {"Enter", "pick & save"},
		{"Esc", "close"}, {"Tab", "next dropdown"}, {"click", "open / pick"},
	}
}

func (t *selectTab) hints(a *App) []hint {
	if t.drops[t.cur].open {
		return []hint{{"↑↓", "Move", ""}, {"type", "Filter", ""}, h("enter", "Pick"), {"Esc", "Close", "esc"}}
	}
	return []hint{h("enter", "Open"), {"↑", "Prev", "up"}, {"↓", "Open", "down"}, h("tab", "Next")}
}

func (t *selectTab) key(a *App, k tea.KeyPressMsg) bool {
	d := t.drops[t.cur]
	if !d.open && k.Keystroke() == "up" && t.cur > 0 {
		t.cur--
		return true
	}
	return d.key(a, k)
}

func (t *selectTab) leave(*App) {
	for _, d := range t.drops {
		d.open = false
	}
}

func (t *selectTab) draw(a *App, b *Buf, r Rect, focused bool) []Hit {
	var hits []Hit
	var frs [2]Rect
	gap := choose(r.H >= 8, 3, 2)
	for i, d := range t.drops {
		fr, hs := d.draw(a, b, r.X, r.Y+i*gap, 10, 26, focused && t.cur == i, i)
		frs[i] = fr
		hits = append(hits, hs...)
		th := a.th()
		e := a.vals.Get(core.Tabs[3].Keys[i])
		b.Text(fr.Right()+2, fr.Y, "id: "+core.SafeText(a.vals.String(core.Tabs[3].Keys[i]))+" · v"+fmt.Sprint(e.Version), th.base().Fg(th.Muted), r.Right()-fr.Right()-2)
	}
	for i, d := range t.drops {
		hits = append(hits, d.drawPopup(a, b, frs[i], i)...)
	}
	return hits
}

func (t *selectTab) mouse(a *App, m hitMsg) {
	if dd, ok := isWheel(m); ok && t.drops[t.cur].open {
		t.drops[t.cur].list.move(dd)
		return
	}
	if !isClick(m) || m.hit.S == "popup" {
		return
	}
	ctl := choose(m.hit.S == "drop", m.hit.A, m.hit.B)
	for i, d := range t.drops {
		if i != ctl {
			d.open = false
		}
	}
	t.cur = ctl
	t.drops[ctl].mouse(a, m.hit)
}

// ================= 5 Combobox =================

type combo struct {
	ckey, label string
	editing     bool // typing a query
	in          lineInput
	open        bool
	list        pickList
}

func (c *combo) refilter() {
	c.list.items = nil
	for _, rk := range core.RankPeople(c.in.Text()) {
		p := core.People[rk.Index]
		n := len([]rune(p.Name))
		var inName []int
		for _, pos := range rk.Match.Positions {
			if pos < n {
				inName = append(inName, pos)
			}
		}
		c.list.items = append(c.list.items, pickItem{id: p.Username, label: p.Name, detail: p.Username + " · " + p.Team, matched: inName})
	}
	c.list.sel, c.list.top = 0, 0
}

func (c *combo) close() {
	c.open, c.editing = false, false
	c.in.SetText("")
}

func (c *combo) commit(a *App, id any) {
	c.close()
	if err := a.commit(c.ckey, id); err != nil && err != errOffline {
		a.setStatus("✗ "+err.Error(), kErr)
	}
}

func (c *combo) key(a *App, k tea.KeyPressMsg) bool {
	key := k.Keystroke()
	if c.open {
		switch key {
		case "esc":
			c.close()
			return true
		case "enter":
			if it, ok := c.list.current(); ok {
				c.commit(a, it.id)
			}
			return true
		case "tab", "shift+tab":
			c.close()
			return false
		}
		if c.list.listKey(key) {
			return true
		}
	}
	switch key {
	case "down":
		c.open, c.editing = true, true
		c.refilter()
		return true
	case "delete":
		if !c.editing || c.in.Text() == "" {
			c.commit(a, nil)
			return true
		}
	case "enter":
		c.open, c.editing = true, true
		c.refilter()
		return true
	}
	handled, changed := c.in.Key(k)
	if changed {
		c.editing, c.open = true, true
		c.refilter()
	}
	return handled
}

func (c *combo) shown(a *App) string {
	if u := a.vals.OptString(c.ckey); u != nil {
		if p, ok := core.PersonByUsername(*u); ok {
			return p.Name
		}
		return core.SafeText(*u)
	}
	return ""
}

type comboTab struct {
	ctlBase
	combos [2]*combo
}

func newComboTab() *comboTab {
	return &comboTab{combos: [2]*combo{{ckey: "combo.owner", label: "Owner"}, {ckey: "combo.reviewer", label: "Reviewer"}}}
}

func (t *comboTab) explain() string {
	return "Type to search people (fuzzy: name, username, team); matched letters are highlighted. Del clears to none."
}

func (t *comboTab) controls() int { return 2 }

func (t *comboTab) keyHelp() [][2]string {
	return [][2]string{
		{"type", "search"}, {"↓ Enter", "open list"}, {"↑↓", "move"}, {"Enter", "pick & save"},
		{"Esc", "close"}, {"Del", "clear (none)"}, {"Tab", "next"}, {"click", "open / pick"},
	}
}

func (t *comboTab) hints(a *App) []hint {
	if t.combos[t.cur].open {
		return []hint{{"type", "Search", ""}, {"↑↓", "Move", ""}, h("enter", "Pick"), {"Esc", "Close", "esc"}}
	}
	return []hint{{"type", "Search", ""}, {"↓", "Open", "down"}, {"Del", "Clear", "delete"}, h("tab", "Next")}
}

func (t *comboTab) key(a *App, k tea.KeyPressMsg) bool {
	c := t.combos[t.cur]
	if !c.open && k.Keystroke() == "up" && t.cur > 0 {
		t.cur--
		return true
	}
	return c.key(a, k)
}

func (t *comboTab) leave(*App) {
	for _, c := range t.combos {
		c.close()
	}
}

func (t *comboTab) paste(a *App, s string) {
	c := t.combos[t.cur]
	if c.in.Insert(s) {
		c.editing, c.open = true, true
		c.refilter()
	}
}

func (t *comboTab) draw(a *App, b *Buf, r Rect, focused bool) []Hit {
	th := a.th()
	var hits []Hit
	var frs [2]Rect
	gap := choose(r.H >= 8, 3, 2)
	for i, c := range t.combos {
		y := r.Y + i*gap
		fcs := focused && t.cur == i
		ls := th.base().Fg(choose(fcs, th.Accent, th.Muted))
		if fcs {
			ls = ls.With(Bold)
		}
		b.Fit(r.X, y, c.label, ls, 10, 0)
		fr := Rect{r.X + 10, y, 30, 1}
		frs[i] = fr
		st := fieldStyle(th, fcs)
		if _, ok := a.flash[c.ckey]; ok {
			st = Sty{FG: th.BG, BG: th.Warn}
		}
		if c.editing {
			c.in.Draw(b, Rect{fr.X + 1, y, fr.W - 3, 1}, th, fcs, st)
			b.Set(fr.X, y, ' ', st)
			b.Text(fr.Right()-2, y, " ", st, 2)
		} else {
			b.Fill(fr, ' ', st)
			if s := c.shown(a); s != "" {
				b.Text(fr.X+1, y, s, st, fr.W-3)
				if u := a.vals.OptString(c.ckey); u != nil {
					if p, ok := core.PersonByUsername(*u); ok {
						b.Text(fr.Right()+2, y, p.Username+" · "+p.Team, th.base().Fg(th.Muted), r.Right()-fr.Right()-2)
					}
				}
			} else {
				b.Text(fr.X+1, y, "— none —", st.Fg(th.Muted), fr.W-3)
			}
			if fcs {
				b.Set(fr.X+1, y, []rune(choose(c.shown(a) == "", "—", c.shown(a)))[0], Sty{FG: st.BG, BG: th.FG})
			}
		}
		b.Set(fr.Right()-2, y, '▾', st.Fg(th.Muted))
		hits = append(hits, Hit{R: Rect{r.X, y, 40, 1}, Kind: HitCtl, S: "combo", A: i})
	}
	for i, c := range t.combos {
		if c.open {
			hits = append(hits, c.list.draw(a, b, frs[i], 44, i, "no one matches")...)
		}
	}
	return hits
}

func (t *comboTab) mouse(a *App, m hitMsg) {
	c := t.combos[t.cur]
	if dd, ok := isWheel(m); ok && c.open {
		c.list.move(dd)
		return
	}
	if !isClick(m) {
		return
	}
	switch m.hit.S {
	case "combo":
		for i, o := range t.combos {
			if i != m.hit.A {
				o.close()
			}
		}
		t.cur = m.hit.A
		c = t.combos[t.cur]
		if c.open {
			c.close()
		} else {
			c.open, c.editing = true, true
			c.refilter()
		}
	case "item":
		c.list.sel = m.hit.A
		if it, ok := c.list.current(); ok {
			c.commit(a, it.id)
		}
	}
}

// ================= 6 Tags =================

type tagsTab struct {
	ctlBase
	in      lineInput
	list    pickList
	chipSel int // −1: none
	err     string
}

func newTagsTab() *tagsTab {
	t := &tagsTab{chipSel: -1}
	t.in.max = 24
	t.in.placeholder = "add a tag…"
	return t
}

func (t *tagsTab) explain() string {
	return "Tags as removable chips; type to filter the suggestions or create a new tag. Every add or remove saves."
}

func (t *tagsTab) controls() int { return 1 }

func (t *tagsTab) keyHelp() [][2]string {
	return [][2]string{
		{"type", "filter / new tag"}, {"↑↓", "choose"}, {"Enter", "add"}, {"⌫ (empty)", "remove last"},
		{"←→ (empty)", "select chip"}, {"Del", "remove chip"}, {"click ×", "remove"}, {"click", "add suggestion"},
	}
}

func (t *tagsTab) hints(a *App) []hint {
	return []hint{{"type", "Filter", ""}, {"↑↓", "Choose", ""}, h("enter", "Add"), {"⌫", "Remove last", "backspace"}, {"←→", "Chips", ""}}
}

// refilter rebuilds the suggestions for the typed text.
func (t *tagsTab) refilter(a *App) {
	have := a.vals.Strings("tags.tags")
	q := strings.ToLower(strings.TrimSpace(t.in.Text()))
	t.list.items = nil
	exact := false
	for _, s := range core.SuggestTags(q, have) {
		t.list.items = append(t.list.items, pickItem{id: s.Tag, label: s.Tag, detail: fmt.Sprintf("%d uses", s.Uses), matched: substrPos(s.Tag, q)})
		exact = exact || s.Tag == q
	}
	t.err = ""
	if q != "" && !exact && !hasStr(have, q) {
		if core.ValidTag(q) {
			t.list.items = append(t.list.items, pickItem{id: q, label: "✚ Create \"" + q + "\"", create: true})
		} else {
			t.err = "a–z, 0–9, _ or - (starting with a letter or digit), up to 24"
		}
	}
	if hasStr(have, q) {
		t.err = "already added"
	}
	t.list.sel = max(0, min(t.list.sel, len(t.list.items)-1))
	t.list.scroll()
}

func substrPos(s, q string) []int {
	i := strings.Index(s, q)
	if q == "" || i < 0 {
		return nil
	}
	start := len([]rune(s[:i]))
	out := make([]int, len([]rune(q)))
	for j := range out {
		out[j] = start + j
	}
	return out
}

func (t *tagsTab) save(a *App, tags []string) {
	if err := a.commit("tags.tags", tags); err != nil && err != errOffline {
		t.err = err.Error()
	}
}

func (t *tagsTab) add(a *App, it pickItem) {
	have := a.vals.Strings("tags.tags")
	if len(have) >= 12 {
		t.err = "at most 12 tags"
		return
	}
	t.save(a, append(append([]string{}, have...), it.id))
	t.in.SetText("")
	t.list.sel = 0
	t.refilter(a)
}

func (t *tagsTab) remove(a *App, i int) {
	have := a.vals.Strings("tags.tags")
	if i < 0 || i >= len(have) {
		return
	}
	t.save(a, append(append([]string{}, have[:i]...), have[i+1:]...))
	t.chipSel = -1
	t.refilter(a)
}

func (t *tagsTab) key(a *App, k tea.KeyPressMsg) bool {
	t.refilter(a)
	have := a.vals.Strings("tags.tags")
	empty := t.in.Text() == ""
	switch k.Keystroke() {
	case "enter":
		if it, ok := t.list.current(); ok {
			t.add(a, it)
		} else if t.err != "" {
			a.setStatus("✗ "+t.err, kErr)
		}
		return true
	case "up", "down", "pgup", "pgdown":
		t.list.listKey(k.Keystroke())
		return true
	case "backspace":
		if empty {
			if t.chipSel >= 0 {
				t.remove(a, t.chipSel)
			} else if len(have) > 0 {
				t.remove(a, len(have)-1)
			}
			return true
		}
	case "left":
		if empty && len(have) > 0 {
			if t.chipSel < 0 {
				t.chipSel = len(have) - 1
			} else {
				t.chipSel = max(0, t.chipSel-1)
			}
			return true
		}
	case "right":
		if empty && t.chipSel >= 0 {
			t.chipSel++
			if t.chipSel >= len(have) {
				t.chipSel = -1
			}
			return true
		}
	case "delete":
		if empty && t.chipSel >= 0 {
			t.remove(a, t.chipSel)
			return true
		}
	case "esc":
		if t.chipSel >= 0 || !empty {
			t.chipSel = -1
			t.in.SetText("")
			t.refilter(a)
			return true
		}
		return false
	}
	handled, changed := t.in.Key(k)
	if changed {
		t.chipSel = -1
		t.list.sel = 0
		t.refilter(a)
	}
	return handled
}

func (t *tagsTab) paste(a *App, s string) {
	t.in.Insert(strings.ToLower(s))
	t.refilter(a)
}

func (t *tagsTab) draw(a *App, b *Buf, r Rect, focused bool) []Hit {
	th := a.th()
	t.refilter(a)
	var hits []Hit
	b.Text(r.X, r.Y, "Tags", th.base().Fg(choose(focused, th.Accent, th.Muted)).With(choose(focused, Bold, Attr(0))), 6)
	box := Rect{r.X + 6, r.Y, min(r.W-6, 70), 1}
	st := fieldStyle(th, focused)
	if _, ok := a.flash["tags.tags"]; ok {
		st = Sty{FG: th.FG, BG: mix(th.Panel2, th.Warn, 0.35)}
	}
	b.Fill(box, ' ', st)
	x := box.X + 1
	for i, tag := range a.vals.Strings("tags.tags") {
		tag = core.SafeText(tag)
		cs := st.Fg(th.Accent2)
		if i == t.chipSel {
			cs = th.sel().With(Bold)
		}
		w := StrW(tag) + 4
		if x+w > box.Right()-8 {
			b.Text(x, r.Y, "…", st.Fg(th.Muted), 1)
			x += 2
			break
		}
		b.Text(x, r.Y, "["+tag+" ", cs, -1)
		b.Text(x+w-2, r.Y, "×]", cs.Fg(choose(i == t.chipSel, th.CellFG, th.Err)), 2)
		hits = append(hits, Hit{R: Rect{x + w - 2, r.Y, 1, 1}, Kind: HitCtl, S: "chipx", A: i},
			Hit{R: Rect{x, r.Y, w - 2, 1}, Kind: HitCtl, S: "chip", A: i})
		x += w + 1
	}
	fr := Rect{x, r.Y, max(4, box.Right()-x-1), 1}
	t.in.Draw(b, fr, th, focused && t.chipSel < 0, st)
	hits = append([]Hit{{R: box, Kind: HitCtl, S: "box"}}, hits...)
	n := len(a.vals.Strings("tags.tags"))
	info := fmt.Sprintf("%d / 12 tags", n)
	if t.err != "" {
		b.Text(box.X, r.Y+1, "✗ "+t.err, th.base().Fg(th.Err), r.W-6)
	} else {
		b.Text(box.X, r.Y+1, info+" · lower case, a–z 0–9 _ -", th.base().Fg(th.Muted), r.W-6)
	}
	// suggestions: always open while focused (or click a suggestion)
	// the list stays inside the control area
	t.list.limit = max(2, min(popupRows, r.Bottom()-r.Y-4))
	if focused || len(t.in.text) > 0 {
		hits = append(hits, t.list.draw(a, b, Rect{fr.X - 1, r.Y + 1, 1, 1}, 34, 0, "no suggestions — type a new tag")...)
	}
	return hits
}

func (t *tagsTab) mouse(a *App, m hitMsg) {
	if dd, ok := isWheel(m); ok {
		t.list.move(dd)
		return
	}
	if !isClick(m) {
		return
	}
	t.refilter(a)
	switch m.hit.S {
	case "chipx":
		t.remove(a, m.hit.A)
	case "chip":
		t.chipSel = m.hit.A
	case "item":
		t.list.sel = m.hit.A
		if it, ok := t.list.current(); ok {
			t.add(a, it)
		}
	}
}

// ================= 9 Cascade =================

type cascadeTab struct {
	ctlBase
	drops [3]*dropdown
}

func strp(s string) *string { return &s }

func deref(p *string) string {
	if p == nil {
		return ""
	}
	return *p
}

func newCascadeTab() *cascadeTab {
	t := &cascadeTab{}
	save := func(a *App, c core.Cascade) {
		if err := a.commit("cascade.pick", c); err != nil && err != errOffline {
			a.setStatus("✗ "+err.Error(), kErr)
		}
	}
	names := func(list []string) []core.Choice {
		out := make([]core.Choice, len(list))
		for i, s := range list {
			out[i] = core.Choice{ID: s, Label: s}
		}
		return out
	}
	t.drops[0] = &dropdown{label: "Program", flashKey: "cascade.pick",
		choices: func(*App) []core.Choice {
			var l []string
			for _, p := range core.Programs {
				l = append(l, p.Name)
			}
			return names(l)
		},
		current: func(a *App) string { return deref(a.vals.Cascade().Program) },
		pick: func(a *App, id string) {
			c := a.vals.Cascade()
			if deref(c.Program) == id {
				return
			}
			save(a, core.Cascade{Program: strp(id)}) // a new program clears the rest
		},
		clear: func(a *App) { save(a, core.Cascade{}) },
	}
	t.drops[1] = &dropdown{label: "Project", flashKey: "cascade.pick",
		choices: func(a *App) []core.Choice {
			var l []string
			for _, j := range core.ProjectsOf(deref(a.vals.Cascade().Program)) {
				l = append(l, j.Name)
			}
			return names(l)
		},
		current: func(a *App) string { return deref(a.vals.Cascade().Project) },
		disabled: func(a *App) string {
			if a.vals.Cascade().Program == nil {
				return "pick a program first"
			}
			return ""
		},
		pick: func(a *App, id string) {
			c := a.vals.Cascade()
			if deref(c.Project) == id {
				return
			}
			save(a, core.Cascade{Program: c.Program, Project: strp(id)})
		},
		clear: func(a *App) { c := a.vals.Cascade(); save(a, core.Cascade{Program: c.Program}) },
	}
	t.drops[2] = &dropdown{label: "Phase", flashKey: "cascade.pick",
		choices: func(a *App) []core.Choice {
			c := a.vals.Cascade()
			return names(core.PhasesOf(deref(c.Program), deref(c.Project)))
		},
		current: func(a *App) string { return deref(a.vals.Cascade().Phase) },
		disabled: func(a *App) string {
			if a.vals.Cascade().Project == nil {
				return "pick a project first"
			}
			return ""
		},
		pick: func(a *App, id string) {
			c := a.vals.Cascade()
			c.Phase = strp(id)
			save(a, c)
		},
		clear: func(a *App) { c := a.vals.Cascade(); c.Phase = nil; save(a, c) },
	}
	return t
}

func (t *cascadeTab) explain() string {
	return "Three dependent dropdowns: each level lists only the choices of the level above; changing one clears those below."
}

func (t *cascadeTab) controls() int { return 3 }

func (t *cascadeTab) keyHelp() [][2]string {
	return [][2]string{
		{"Enter Space ↓", "open"}, {"type", "filter"}, {"Enter", "pick & save"}, {"Esc", "close"},
		{"Del", "clear level"}, {"Tab", "next level"}, {"click", "open / pick"},
	}
}

func (t *cascadeTab) hints(a *App) []hint {
	if t.drops[t.cur].open {
		return []hint{{"↑↓", "Move", ""}, {"type", "Filter", ""}, h("enter", "Pick"), {"Esc", "Close", "esc"}}
	}
	return []hint{h("enter", "Open"), {"Del", "Clear", "delete"}, h("tab", "Next level")}
}

func (t *cascadeTab) key(a *App, k tea.KeyPressMsg) bool {
	d := t.drops[t.cur]
	if !d.open && k.Keystroke() == "up" && t.cur > 0 {
		t.cur--
		return true
	}
	return d.key(a, k)
}

func (t *cascadeTab) leave(*App) {
	for _, d := range t.drops {
		d.open = false
	}
}

func (t *cascadeTab) draw(a *App, b *Buf, r Rect, focused bool) []Hit {
	th := a.th()
	var hits []Hit
	var frs [3]Rect
	gap := choose(r.H >= 9, 2, 1)
	for i, d := range t.drops {
		fr, hs := d.draw(a, b, r.X, r.Y+i*gap, 10, 28, focused && t.cur == i, i)
		frs[i] = fr
		hits = append(hits, hs...)
		n := len(d.choices(a))
		word := []string{"program", "project", "phase"}[i]
		if d.disabled == nil || d.disabled(a) == "" {
			b.Text(fr.Right()+2, fr.Y, fmt.Sprintf("%d %ss", n, word), th.base().Fg(th.Muted), r.Right()-fr.Right()-2)
		}
	}
	// breadcrumb
	y := r.Y + 3*gap
	c := a.vals.Cascade()
	parts := []string{}
	for _, p := range []*string{c.Program, c.Project, c.Phase} {
		if p != nil {
			parts = append(parts, core.SafeText(*p))
		}
	}
	crumb := strings.Join(parts, " ▸ ")
	if crumb == "" {
		crumb = "— nothing picked —"
	}
	b.Text(r.X, y, "Path", th.base().Fg(th.Muted), 10)
	b.Text(r.X+10, y, crumb, th.base().Fg(th.Accent2).With(Bold), r.W-10)
	for i, d := range t.drops {
		hits = append(hits, d.drawPopup(a, b, frs[i], i)...)
	}
	return hits
}

func (t *cascadeTab) mouse(a *App, m hitMsg) {
	if dd, ok := isWheel(m); ok && t.drops[t.cur].open {
		t.drops[t.cur].list.move(dd)
		return
	}
	if !isClick(m) || m.hit.S == "popup" {
		return
	}
	if m.hit.S == "drop" {
		for i, d := range t.drops {
			if i != m.hit.A {
				d.open = false
			}
		}
		t.cur = m.hit.A
	}
	t.drops[t.cur].mouse(a, m.hit)
}

func hasStr(list []string, s string) bool {
	for _, x := range list {
		if x == s {
			return true
		}
	}
	return false
}
