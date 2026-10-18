package tui

// Tab 10 Grid: an editable grid of grid.rows with a computed Total column,
// a totals row, zebra rows and a view-only sort. Each cell opens the right
// editor: text (Item), number (Qty, Price), status list, natural date (Due).
// Every committed edit sends the whole list.

import (
	"fmt"
	"sort"
	"strconv"
	"strings"
	"unicode"

	tea "charm.land/bubbletea/v2"

	"git.wharf.fluidgrid.org/jcz/tictactoe/go/internal/demo/core"
)

const (
	gcID = iota
	gcItem
	gcQty
	gcPrice
	gcTotal
	gcStatus
	gcDue
	gcCount
)

var gridTitles = []string{"#", "Item", "Qty", "Price", "Total", "Status", "Due"}

func gridEditable(c int) bool { return c != gcID && c != gcTotal }

type gridEdit struct {
	col  int
	id   int64
	in   lineInput
	list pickList // status
	err  string
}

type gridTab struct {
	ctlBase
	row, col int
	top      int
	sortCol  int // −1: stored order
	sortDesc bool
	ed       *gridEdit
	cells    Rect // last drawn body (for scrolling)
}

func newGridTab() *gridTab { return &gridTab{col: gcItem, sortCol: -1} }

func (t *gridTab) explain() string {
	return "An editable grid: Enter or typing edits a cell with the right editor, Ins adds a row, F8 deletes one; sorting is only a view."
}

func (t *gridTab) controls() int { return 1 }

func (t *gridTab) keyHelp() [][2]string {
	return [][2]string{
		{"arrows Tab", "move"}, {"Home/End", "first/last col"}, {"Enter F2 type", "edit"}, {"Enter", "save ↓"},
		{"Esc", "cancel"}, {"Del", "clear Due"}, {"Ins", "add row"}, {"F8 ^Del", "delete row"}, {"^O", "sort column"},
		{"click", "select"}, {"dbl-click", "edit"}, {"click header", "sort"},
	}
}

func (t *gridTab) hints(a *App) []hint {
	if t.ed != nil {
		hs := []hint{h("enter", "Save ↓"), h("tab", "Save →"), {"Esc", "Cancel", "esc"}}
		switch t.ed.col {
		case gcStatus:
			hs = append(hs, hint{"↑↓", "Choose", ""}, hint{"type", "Filter", ""})
		case gcDue:
			hs = append(hs, hint{"e.g.", "fri +3d eom", ""})
		case gcQty, gcPrice:
			hs = append(hs, hint{"↑↓", "±1", ""})
		}
		return hs
	}
	return []hint{h("enter", "Edit"), {"Ins", "Add row", "insert"}, {"F8", "Delete", "f8"}, {"^O", "Sort", "ctrl+o"}, {"Del", "Clear due", "delete"}}
}

// order is the displayed order of row indices (the sort is a view).
func (t *gridTab) order(rows []core.GridRow) []int {
	idx := make([]int, len(rows))
	for i := range idx {
		idx[i] = i
	}
	if t.sortCol < 0 {
		return idx
	}
	less := func(x, y core.GridRow) bool {
		switch t.sortCol {
		case gcID:
			return x.ID < y.ID
		case gcItem:
			return strings.ToLower(x.Item) < strings.ToLower(y.Item)
		case gcQty:
			return x.Qty < y.Qty
		case gcPrice:
			return x.Price < y.Price
		case gcTotal:
			return float64(x.Qty)*x.Price < float64(y.Qty)*y.Price
		case gcStatus:
			return statusIndex(x.Status) < statusIndex(y.Status)
		case gcDue:
			// no date sorts last
			return x.Due != nil && (y.Due == nil || *x.Due < *y.Due)
		}
		return false
	}
	sort.SliceStable(idx, func(i, j int) bool {
		x, y := rows[idx[i]], rows[idx[j]]
		if t.sortDesc {
			return less(y, x)
		}
		return less(x, y)
	})
	return idx
}

func statusIndex(id string) int {
	for i, s := range core.Statuses {
		if s.ID == id {
			return i
		}
	}
	return len(core.Statuses)
}

// cur returns the row under the cursor (ok false for an empty grid).
func (t *gridTab) cur(a *App) (core.GridRow, int, bool) {
	rows := a.vals.GridRows()
	if len(rows) == 0 {
		return core.GridRow{}, -1, false
	}
	t.row = max(0, min(t.row, len(rows)-1))
	i := t.order(rows)[t.row]
	return rows[i], i, true
}

func (t *gridTab) save(a *App, rows []core.GridRow) error {
	if rows == nil {
		rows = []core.GridRow{}
	}
	err := a.commit("grid.rows", rows)
	if err != nil && err != errOffline {
		a.setStatus("✗ "+err.Error(), kErr)
	}
	return err
}

func fmtMoney(f float64) string { return strconv.FormatFloat(f, 'f', 2, 64) }

// cellText is what a cell shows (and what an editor starts from).
func cellText(r core.GridRow, c int, short bool) string {
	switch c {
	case gcID:
		return strconv.FormatInt(r.ID, 10)
	case gcItem:
		return core.SafeText(r.Item)
	case gcQty:
		return strconv.FormatInt(r.Qty, 10)
	case gcPrice:
		return fmtMoney(r.Price)
	case gcTotal:
		return fmtMoney(core.Round2(float64(r.Qty) * r.Price))
	case gcStatus:
		if s, ok := core.FindChoice(core.Statuses, r.Status); ok {
			return s.Label
		}
		return core.SafeText(r.Status)
	case gcDue:
		if r.Due == nil {
			return ""
		}
		if d, ok := core.ParseISO(*r.Due); ok && short {
			return d.Short()
		}
		return core.SafeText(*r.Due)
	}
	return ""
}

func (t *gridTab) startEdit(a *App, seed string) {
	r, _, ok := t.cur(a)
	if !ok || !gridEditable(t.col) {
		if ok {
			a.info(gridTitles[t.col] + " is read-only")
		}
		return
	}
	e := &gridEdit{col: t.col, id: r.ID}
	switch t.col {
	case gcQty:
		e.in.accept = unicode.IsDigit
	case gcPrice:
		e.in.accept = func(r rune) bool { return unicode.IsDigit(r) || r == '.' }
	case gcItem:
		e.in.max = 40
	}
	if t.col == gcDue && r.Due != nil {
		e.in.SetText(*r.Due)
	} else if t.col != gcStatus {
		e.in.SetText(cellText(r, t.col, false))
	}
	if seed != "" {
		e.in.SetText("")
		e.in.Insert(seed)
	}
	t.ed = e
	if t.col == gcStatus {
		t.refilterStatus()
		for i, it := range e.list.items {
			if it.id == r.Status && seed == "" {
				e.list.sel = i
			}
		}
	}
}

func (t *gridTab) refilterStatus() {
	e := t.ed
	labels := make([]string, len(core.Statuses))
	for i, s := range core.Statuses {
		labels[i] = s.Label
	}
	e.list.items = nil
	for _, rk := range core.FuzzyRank(e.in.Text(), labels) {
		s := core.Statuses[rk.Index]
		e.list.items = append(e.list.items, pickItem{id: s.ID, label: s.Label, color: s.Color, matched: rk.Match.Positions})
	}
	e.list.sel, e.list.top = 0, 0
}

// commitEdit saves the editor; ok false keeps it open with an error.
func (t *gridTab) commitEdit(a *App) bool {
	e := t.ed
	rows := a.vals.GridRows()
	i := -1
	for j, r := range rows {
		if r.ID == e.id {
			i = j
		}
	}
	if i < 0 {
		t.ed = nil // the row was deleted meanwhile
		return true
	}
	r := &rows[i]
	txt := strings.TrimSpace(e.in.Text())
	switch e.col {
	case gcItem:
		r.Item = txt
	case gcQty:
		n, err := strconv.ParseInt(txt, 10, 64)
		if err != nil {
			e.err = "a whole number 0–9999"
			return false
		}
		r.Qty = n
	case gcPrice:
		f, err := strconv.ParseFloat(txt, 64)
		if err != nil {
			e.err = "a number like 12.50"
			return false
		}
		r.Price = f
	case gcStatus:
		it, ok := e.list.current()
		if !ok {
			e.err = "pick a status"
			return false
		}
		r.Status = it.id
	case gcDue:
		d, err := core.ParseNatural(txt, a.today)
		if err != nil {
			e.err = err.Error()
			return false
		}
		r.Due = nil
		if d != nil {
			s := d.ISO()
			r.Due = &s
		}
	}
	if err := t.save(a, rows); err != nil {
		if err != errOffline {
			e.err = err.Error()
			if k := strings.Index(e.err, ": "); k >= 0 && strings.HasPrefix(e.err, "row ") {
				e.err = e.err[k+2:]
			}
		}
		return err == errOffline
	}
	t.ed = nil
	return true
}

func (t *gridTab) editKey(a *App, k tea.KeyPressMsg) bool {
	e := t.ed
	key := k.Keystroke()
	switch key {
	case "esc":
		t.ed = nil
		return true
	case "enter":
		if t.commitEdit(a) {
			t.move(a, 1, 0)
		}
		return true
	case "tab":
		if t.commitEdit(a) {
			t.col = min(gcCount-1, t.col+1)
		}
		return true
	case "shift+tab":
		if t.commitEdit(a) {
			t.col = max(0, t.col-1)
		}
		return true
	}
	if e.col == gcStatus && e.list.listKey(key) {
		return true
	}
	if (e.col == gcQty || e.col == gcPrice) && (key == "up" || key == "down") {
		f, _ := strconv.ParseFloat(strings.TrimSpace(e.in.Text()), 64)
		f = max(0, f+choose(key == "up", 1.0, -1.0))
		e.in.SetText(choose(e.col == gcQty, strconv.FormatInt(int64(f), 10), fmtMoney(f)))
		return true
	}
	if _, changed := e.in.Key(k); changed {
		e.err = ""
		if e.col == gcStatus {
			t.refilterStatus()
		}
	}
	return true // the editor is modal
}

func (t *gridTab) move(a *App, dr, dc int) {
	n := len(a.vals.GridRows())
	t.row = max(0, min(n-1, t.row+dr))
	t.col = max(0, min(gcCount-1, t.col+dc))
}

func (t *gridTab) addRow(a *App) {
	rows := a.vals.GridRows()
	if len(rows) >= 50 {
		a.setStatus("✗ at most 50 rows", kErr)
		return
	}
	ids := make([]int64, len(rows))
	for i, r := range rows {
		ids[i] = r.ID
	}
	id := core.NextID(ids)
	rows = append(rows, core.GridRow{ID: id, Item: "New item", Qty: 1, Price: 0, Status: "todo"})
	if t.save(a, rows) != nil {
		return
	}
	// put the cursor on the new row and edit its Item
	for i, j := range t.order(a.vals.GridRows()) {
		if a.vals.GridRows()[j].ID == id {
			t.row = i
		}
	}
	t.col = gcItem
	t.startEdit(a, "")
	t.ed.in.SetText("New item")
}

func (t *gridTab) deleteRow(a *App) {
	r, _, ok := t.cur(a)
	if !ok {
		return
	}
	id := r.ID
	a.confirm("Delete row", fmt.Sprintf("Delete row #%d %q?", r.ID, core.SafeText(r.Item)), "Delete", func(a *App) {
		var out []core.GridRow
		for _, x := range a.vals.GridRows() {
			if x.ID != id {
				out = append(out, x)
			}
		}
		t.save(a, out)
		t.move(a, 0, 0)
	})
}

func (t *gridTab) cycleSort(c int) {
	switch {
	case t.sortCol != c:
		t.sortCol, t.sortDesc = c, false
	case !t.sortDesc:
		t.sortDesc = true
	default:
		t.sortCol = -1
	}
}

func (t *gridTab) key(a *App, k tea.KeyPressMsg) bool {
	if t.ed != nil {
		return t.editKey(a, k)
	}
	n := len(a.vals.GridRows())
	switch key := k.Keystroke(); key {
	case "up":
		t.move(a, -1, 0)
	case "down":
		t.move(a, 1, 0)
	case "left":
		t.move(a, 0, -1)
	case "right":
		t.move(a, 0, 1)
	case "home":
		t.col = 0
	case "end":
		t.col = gcCount - 1
	case "ctrl+home":
		t.row = 0
	case "ctrl+end":
		t.row = n - 1
	case "pgup":
		t.move(a, -max(1, t.cells.H-1), 0)
	case "pgdown":
		t.move(a, max(1, t.cells.H-1), 0)
	case "tab":
		if t.col == gcCount-1 && t.row >= n-1 {
			return false
		}
		if t.col++; t.col == gcCount {
			t.col, t.row = 0, t.row+1
		}
	case "shift+tab":
		if t.col == 0 && t.row == 0 {
			return false
		}
		if t.col--; t.col < 0 {
			t.col, t.row = gcCount-1, t.row-1
		}
	case "enter", "f2":
		t.startEdit(a, "")
	case "delete":
		if t.col != gcDue {
			a.info("Del clears the Due date; F8 deletes the row")
			return true
		}
		if r, i, ok := t.cur(a); ok && r.Due != nil {
			rows := a.vals.GridRows()
			rows[i].Due = nil
			t.save(a, rows)
		}
	case "insert", "ctrl+n":
		t.addRow(a)
	case "f8", "ctrl+delete":
		t.deleteRow(a)
	case "ctrl+o":
		t.cycleSort(t.col)
	default:
		if k.Text != "" && k.Mod&(tea.ModCtrl|tea.ModAlt) == 0 && key != "space" {
			t.startEdit(a, k.Text)
			return true
		}
		return false
	}
	return true
}

func (t *gridTab) paste(a *App, s string) {
	if t.ed != nil {
		t.ed.in.Insert(s)
	}
}

func (t *gridTab) leave(*App) { t.ed = nil }

// widths lays the columns out for the available width.
func gridWidths(w int) ([gcCount]int, bool) {
	if w >= 72 {
		ws := [gcCount]int{3, 0, 5, 10, 11, 13, 11}
		ws[gcItem] = max(12, min(30, w-59))
		return ws, false
	}
	ws := [gcCount]int{3, 0, 4, 9, 10, 9, 8}
	ws[gcItem] = max(6, w-49)
	return ws, true
}

func (t *gridTab) draw(a *App, b *Buf, r Rect, focused bool) []Hit {
	th := a.th()
	rows := a.vals.GridRows()
	order := t.order(rows)
	ws, short := gridWidths(r.W)
	xs := [gcCount]int{}
	x := r.X
	for c := 0; c < gcCount; c++ {
		xs[c] = x
		x += ws[c] + 1
	}
	tw := x - r.X - 1
	var hits []Hit
	// header
	hs := Sty{FG: th.FG, BG: th.Panel2, A: Bold}
	b.Fill(Rect{r.X, r.Y, tw, 1}, ' ', hs)
	for c := 0; c < gcCount; c++ {
		title := gridTitles[c]
		if t.sortCol == c {
			title += choose(t.sortDesc, " ▼", " ▲")
		}
		align := choose(c == gcQty || c == gcPrice || c == gcTotal || c == gcID, 1, 0)
		b.Fit(xs[c], r.Y, title, hs.Fg(choose(t.sortCol == c, th.Accent, th.FG)), ws[c], align)
		hits = append(hits, Hit{R: Rect{xs[c], r.Y, ws[c], 1}, Kind: HitCtl, S: "header", A: c})
	}
	// body rows (scrolled), then the totals row
	bodyH := max(1, r.H-3)
	t.cells = Rect{r.X, r.Y + 1, tw, bodyH}
	t.row = max(0, min(t.row, len(rows)-1))
	if t.row < t.top {
		t.top = t.row
	}
	if t.row >= t.top+bodyH {
		t.top = t.row - bodyH + 1
	}
	t.top = max(0, min(t.top, max(0, len(rows)-bodyH)))
	_, flashing := a.flash["grid.rows"]
	var sumQty int64
	var sumTotal float64
	for _, row := range rows {
		sumQty += row.Qty
		sumTotal += core.Round2(float64(row.Qty) * row.Price)
	}
	var edRect Rect
	for i := 0; i < bodyH && t.top+i < len(order); i++ {
		di := t.top + i
		row := rows[order[di]]
		y := r.Y + 1 + i
		bg := choose(di%2 == 1, th.Zebra, th.BG)
		if di == t.row {
			bg = th.CurBG
		}
		if flashing {
			bg = mix(bg, th.Warn, 0.2)
		}
		b.Fill(Rect{r.X, y, tw, 1}, ' ', Sty{FG: th.FG, BG: bg})
		for c := 0; c < gcCount; c++ {
			st := Sty{FG: th.FG, BG: bg}
			text := cellText(row, c, short)
			align := choose(c == gcQty || c == gcPrice || c == gcTotal || c == gcID, 1, 0)
			switch c {
			case gcID, gcTotal:
				st = st.Fg(th.Muted)
			case gcStatus:
				if s, ok := core.FindChoice(core.Statuses, row.Status); ok {
					st = st.Fg(hex(s.Color))
					text = "● " + text
				}
			case gcDue:
				if row.Due != nil && *row.Due < a.today.ISO() {
					st = st.Fg(th.Err)
				}
				if text == "" {
					text, st = "—", st.Fg(th.Muted)
				}
			}
			if di == t.row && c == t.col {
				st = Sty{FG: th.CellFG, BG: choose(focused, th.CellBG, th.SelBG), A: Bold}
				if c == gcStatus || c == gcID || c == gcTotal {
					st.FG = th.CellFG
				}
				b.Fill(Rect{xs[c], y, ws[c], 1}, ' ', st)
			}
			b.Fit(xs[c], y, text, st, ws[c], align)
			hits = append(hits, Hit{R: Rect{xs[c], y, ws[c] + 1, 1}, Kind: HitCtl, S: "cell", A: di, B: c})
			if t.ed != nil && di == t.row && c == t.ed.col {
				edRect = Rect{xs[c], y, ws[c], 1}
			}
		}
	}
	// totals
	ty := r.Y + 1 + min(bodyH, max(1, len(rows)))
	ts := Sty{FG: th.FG, BG: th.Panel2, A: Bold}
	b.Fill(Rect{r.X, ty, tw, 1}, ' ', ts)
	b.Fit(xs[gcItem], ty, "Total", ts, ws[gcItem], 0)
	b.Fit(xs[gcQty], ty, strconv.FormatInt(sumQty, 10), ts, ws[gcQty], 1)
	b.Fit(xs[gcTotal], ty, fmtMoney(sumTotal), ts.Fg(th.Accent), ws[gcTotal], 1)
	info := fmt.Sprintf("row %d of %d", t.row+1, len(rows))
	if len(rows) == 0 {
		info = "no rows — Ins adds one"
	}
	if t.sortCol >= 0 {
		info += " · sorted by " + gridTitles[t.sortCol] + choose(t.sortDesc, " ▼", " ▲") + " (view only)"
	}
	if len(rows) > bodyH {
		info += fmt.Sprintf(" · showing %d–%d", t.top+1, min(len(rows), t.top+bodyH))
	}
	b.Text(r.X, ty+1, info, th.base().Fg(th.Muted), r.W)
	if t.ed != nil && edRect.W > 0 {
		hits = append(hits, t.drawEditor(a, b, edRect, r)...)
	}
	return hits
}

func (t *gridTab) drawEditor(a *App, b *Buf, cr Rect, r Rect) []Hit {
	th := a.th()
	e := t.ed
	fr := Rect{cr.X, cr.Y, max(cr.W, choose(e.col == gcDue || e.col == gcItem, 18, cr.W)), 1}
	if fr.Right() > r.Right() { // the last columns open their editor leftwards
		fr.X = max(r.X, r.Right()-fr.W)
	}
	st := Sty{FG: th.FG, BG: mix(th.Panel2, th.Accent, 0.3)}
	e.in.placeholder = choose(e.col == gcStatus, "type to filter", "")
	e.in.Draw(b, fr, th, true, st)
	b.Restyle(fr, func(s Sty) Sty { return s.ULine(th.Accent) })
	var hits []Hit
	switch {
	case e.col == gcStatus:
		hits = e.list.draw(a, b, fr, 20, 0, "no matches")
	case e.col == gcDue:
		p, ok := core.DatePreview(e.in.Text(), a.today)
		if strings.TrimSpace(e.in.Text()) == "" {
			p = "empty = no due date"
		}
		msg := " " + p + " "
		st := Sty{FG: th.CellFG, BG: choose(ok, th.OK, th.Err)}
		b.Text(min(fr.X, r.Right()-StrW(msg)), fr.Y+1, msg, st, r.Right()-fr.X+StrW(msg))
	}
	if e.err != "" {
		msg := " ✗ " + e.err + " "
		y := choose(fr.Y > r.Y+1, fr.Y-1, fr.Y+1)
		b.Text(fr.X, y, msg, Sty{FG: th.CellFG, BG: th.Err}, r.Right()-fr.X)
	}
	return hits
}

func (t *gridTab) mouse(a *App, m hitMsg) {
	h := m.hit
	if d, ok := isWheel(m); ok {
		if t.ed != nil && t.ed.col == gcStatus {
			t.ed.list.move(d)
		} else if t.ed == nil {
			t.move(a, d, 0)
		}
		return
	}
	if !isClick(m) {
		return
	}
	switch h.S {
	case "item":
		if t.ed != nil {
			t.ed.list.sel = h.A
			t.commitEdit(a)
		}
	case "header":
		if t.ed == nil {
			t.cycleSort(h.A)
		}
	case "cell":
		if t.ed != nil {
			if h.A == t.row && h.B == t.ed.col {
				return
			}
			if !t.commitEdit(a) { // clicking elsewhere saves the edit first
				return
			}
		}
		dbl := a.doubleClick(h)
		t.row, t.col = h.A, h.B
		if dbl {
			t.startEdit(a, "")
		}
	}
}
