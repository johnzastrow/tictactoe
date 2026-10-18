//go:build js && wasm

package webapp

// Tab 10 — Grid: an editable table of grid.rows with per-column editors
// (text, number, status dropdown, natural date), a computed Total column,
// a totals row and view-only sorting. Every edit commits the whole list.

import (
	"encoding/json"
	"fmt"
	"math"
	"sort"
	"strconv"
	"strings"
	"syscall/js"

	"git.wharf.fluidgrid.org/jcz/tictactoe/go/internal/demo/core"
)

var gridCols = []struct {
	title string
	edit  bool
	right bool
}{{"#", false, true}, {"Item", true, false}, {"Qty", true, true}, {"Price", true, true},
	{"Total", false, true}, {"Status", true, false}, {"Due", true, false}}

const (
	gcID = iota
	gcItem
	gcQty
	gcPrice
	gcTotal
	gcStatus
	gcDue
)

type gridTab struct {
	tabBase
	grid    js.Value
	row     int // cursor, in display order
	col     int
	sortCol int // -1 = stored order
	sortDir int // 1 ▲, -1 ▼
	editing bool
	editID  int64 // row being edited
	editCol int
	ed      js.Value // the text editor input
	edMsg   js.Value
	edErr   string
	status  *Dropdown
	sig     string
	// lastClick is the time (ms) of the last press on a cell
	lastClick float64
}

func newGridTab(b tabBase) Tab {
	t := &gridTab{tabBase: b, sortCol: -1, col: gcItem}
	t.build = t.mount
	return t
}

func (t *gridTab) mount(area js.Value) {
	t.grid = E("div", "dgrid")
	Attr(t.grid, "role", "grid")
	t.addCtl(t.grid, "grid", "grid.rows")
	Add(area, t.grid)
	t.status = newDropdown(t.a, &t.scope, core.Statuses, func(id string) { t.commitCell(id) })
	t.status.onClose = func() {
		if t.editing && t.editCol == gcStatus {
			t.editing = false
			t.a.Invalidate()
		}
	}
	t.scope.On(t.grid, "mousedown", func(ev js.Value) {
		tgt := ev.Get("target")
		if t.editing && Closest(tgt, "input").Truthy() {
			return
		}
		if h := Closest(tgt, "[data-h]"); h.Truthy() {
			ev.Call("preventDefault")
			c, _ := strconv.Atoi(DataOf(h, "h"))
			t.Focus(0)
			t.col = c
			t.cycleSort()
			return
		}
		if cell := Closest(tgt, "[data-r]"); cell.Truthy() {
			ev.Call("preventDefault")
			if t.editing {
				t.cancelEdit()
			}
			r, _ := strconv.Atoi(DataOf(cell, "r"))
			c, _ := strconv.Atoi(DataOf(cell, "c"))
			// a second press on the same cell within 450 ms is a double-click
			// (detected here: the first press re-renders the cell, so the
			// browser's own dblclick never fires on it)
			now := nowMs()
			double := r == t.row && c == t.col && now-t.lastClick < 450
			t.row, t.col, t.lastClick = r, c, now
			t.Focus(0)
			if double {
				t.lastClick = 0
				t.startEdit("")
			}
			t.a.Invalidate()
		}
	})
}

// order is the display order (indices into rows): stored, or sorted.
func (t *gridTab) order(rows []core.GridRow) []int {
	idx := make([]int, len(rows))
	for i := range idx {
		idx[i] = i
	}
	if t.sortCol < 0 {
		return idx
	}
	key := func(r core.GridRow) (float64, string) {
		switch t.sortCol {
		case gcID:
			return float64(r.ID), ""
		case gcItem:
			return 0, strings.ToLower(r.Item)
		case gcQty:
			return float64(r.Qty), ""
		case gcPrice:
			return r.Price, ""
		case gcTotal:
			return float64(r.Qty) * r.Price, ""
		case gcStatus:
			for i, s := range core.Statuses {
				if s.ID == r.Status {
					return float64(i), ""
				}
			}
		case gcDue:
			if r.Due == nil {
				return 0, "~" // nulls last
			}
			return 0, *r.Due
		}
		return 0, ""
	}
	sort.SliceStable(idx, func(i, j int) bool {
		ni, si := key(rows[idx[i]])
		nj, sj := key(rows[idx[j]])
		less := ni < nj || ni == nj && si < sj
		if t.sortDir < 0 {
			less = ni > nj || ni == nj && si > sj
		}
		return less
	})
	return idx
}

func money(f float64) string { return strconv.FormatFloat(f, 'f', 2, 64) }

func cellText(r core.GridRow, c int) string {
	switch c {
	case gcID:
		return strconv.FormatInt(r.ID, 10)
	case gcItem:
		return r.Item
	case gcQty:
		return strconv.FormatInt(r.Qty, 10)
	case gcPrice:
		return money(r.Price)
	case gcTotal:
		return money(core.Round2(float64(r.Qty) * r.Price))
	case gcStatus:
		return r.Status
	case gcDue:
		if r.Due != nil {
			return *r.Due
		}
	}
	return ""
}

func (t *gridTab) cycleSort() {
	switch {
	case t.sortCol != t.col:
		t.sortCol, t.sortDir = t.col, 1
	case t.sortDir == 1:
		t.sortDir = -1
	default:
		t.sortCol = -1
	}
	label := "off (stored order)"
	if t.sortCol >= 0 {
		label = gridCols[t.sortCol].title + map[int]string{1: " ▲", -1: " ▼"}[t.sortDir]
	}
	t.a.info("Sort: " + label + " — a view only, the stored order is unchanged")
	t.a.Invalidate()
}

func (t *gridTab) curRow() (core.GridRow, bool) {
	rows := t.a.v.GridRows()
	ord := t.order(rows)
	if t.row < 0 || t.row >= len(ord) {
		return core.GridRow{}, false
	}
	return rows[ord[t.row]], true
}

// replaceRow commits rows with one row changed (by id); nil change deletes it.
func (t *gridTab) commitRows(rows []core.GridRow) error {
	if rows == nil {
		rows = []core.GridRow{}
	}
	return t.a.Commit("grid.rows", rows)
}

func (t *gridTab) withRow(id int64, f func(r *core.GridRow)) []core.GridRow {
	rows := append([]core.GridRow(nil), t.a.v.GridRows()...)
	for i := range rows {
		if rows[i].ID == id {
			f(&rows[i])
		}
	}
	return rows
}

func (t *gridTab) startEdit(initial string) {
	r, ok := t.curRow()
	if !ok || !gridCols[t.col].edit {
		if ok {
			t.a.info(gridCols[t.col].title + " is read-only")
		}
		return
	}
	t.editing, t.editID, t.editCol, t.edErr = true, r.ID, t.col, ""
	if t.col == gcStatus {
		t.status.value = r.Status
		t.status.Open()
		if initial != "" {
			t.status.Key(Key{Name: initial})
		}
		t.sig = ""
		t.a.Invalidate()
		return
	}
	t.ed = E("input", "ced")
	Attr(t.ed, "spellcheck", "false")
	Attr(t.ed, "autocomplete", "off")
	Data(t.ed, "control", "grid-editor")
	t.edMsg = E("div", "edprev hidden")
	val := cellText(r, t.col)
	if t.col == gcPrice {
		val = fmtNum(r.Price)
	}
	if initial != "" {
		val = initial
	}
	t.ed.Set("value", val)
	t.scope.On(t.ed, "input", func(js.Value) { t.edErr = ""; t.a.Invalidate() })
	t.sig = ""
	t.a.Invalidate()
	After(0, func() {
		if t.ed.Truthy() {
			t.ed.Call("focus")
			if initial == "" {
				t.ed.Call("select")
			}
		}
	})
}

func (t *gridTab) cancelEdit() {
	t.editing, t.edErr = false, ""
	if t.status.open {
		t.status.Close()
	}
	t.ed = js.Undefined()
	t.sig = ""
	t.grid.Call("focus")
	t.a.Invalidate()
}

// commitCell parses the editor (or takes the status id) and commits; on a
// local error the editor stays open with the message.
func (t *gridTab) commitCell(status string) bool {
	var perr error
	rows := t.withRow(t.editID, func(r *core.GridRow) {
		switch t.editCol {
		case gcStatus:
			r.Status = status
			return
		}
		s := strings.TrimSpace(t.ed.Get("value").String())
		switch t.editCol {
		case gcItem:
			r.Item = s
		case gcQty:
			n, err := strconv.ParseInt(s, 10, 64)
			if err != nil {
				perr = fmt.Errorf("qty must be a whole number")
			}
			r.Qty = n
		case gcPrice:
			f, err := strconv.ParseFloat(s, 64)
			if err != nil || math.IsNaN(f) || math.IsInf(f, 0) {
				perr = fmt.Errorf("price must be a number")
			}
			r.Price = f
		case gcDue:
			d, err := core.ParseNatural(s, today())
			if err != nil {
				perr = err
			} else if d == nil {
				r.Due = nil
			} else {
				r.Due = ptr(d.ISO())
			}
		}
	})
	if perr == nil {
		perr = t.commitRows(rows)
	}
	if perr != nil {
		t.edErr = perr.Error()
		t.a.setStatus("err", "✗ "+perr.Error())
		t.a.Invalidate()
		return false
	}
	t.editing, t.ed = false, js.Undefined()
	t.sig = ""
	t.grid.Call("focus")
	t.a.Invalidate()
	return true
}

func (t *gridTab) Key(k Key) bool {
	rows := t.a.v.GridRows()
	n := len(rows)
	if t.editing {
		if t.editCol == gcStatus {
			return t.status.Key(k)
		}
		switch {
		case k.Is("Enter"):
			if t.commitCell("") {
				t.row = min(t.row+1, n-1)
			}
		case k.Is("Tab"):
			if t.commitCell("") {
				t.col = min(t.col+1, len(gridCols)-1)
			}
		case k.Is("Escape"):
			t.cancelEdit()
		default:
			return false
		}
		return true
	}
	last := len(gridCols) - 1
	switch {
	case k.Is("ArrowUp"):
		t.row = max(0, t.row-1)
	case k.Is("ArrowDown"):
		t.row = min(n-1, t.row+1)
	case k.Is("ArrowLeft"):
		t.col = max(0, t.col-1)
	case k.Is("ArrowRight"):
		t.col = min(last, t.col+1)
	case k.Is("Tab"):
		if t.col < last {
			t.col++
		} else if t.row < n-1 {
			t.row, t.col = t.row+1, 0
		}
	case k.Is("S-Tab"):
		if t.col > 0 {
			t.col--
		} else if t.row > 0 {
			t.row, t.col = t.row-1, last
		} else {
			return false // back to the sidebar
		}
	case k.Is("Home"):
		t.col = 0
	case k.Is("End"):
		t.col = last
	case k.Is("C-Home"):
		t.row = 0
	case k.Is("C-End"):
		t.row = n - 1
	case k.Is("Enter", "F2"):
		t.startEdit("")
	case k.Is("Delete"):
		if r, ok := t.curRow(); ok {
			_ = t.commitRows(t.withRow(r.ID, func(r *core.GridRow) { r.Due = nil }))
		}
	case k.Is("Insert"):
		t.addRow()
	case k.Is("F8", "C-Delete"):
		t.deleteRow()
	case k.Is("C-o"):
		t.cycleSort()
	case k.Printable() && k.Name != " ":
		t.startEdit(k.Name)
	default:
		return false
	}
	t.a.Invalidate()
	return true
}

func (t *gridTab) addRow() {
	rows := append([]core.GridRow(nil), t.a.v.GridRows()...)
	var ids []int64
	for _, r := range rows {
		ids = append(ids, r.ID)
	}
	nr := core.GridRow{ID: core.NextID(ids), Item: "New item", Qty: 1, Price: 0, Status: "todo"}
	if t.commitRows(append(rows, nr)) != nil {
		return
	}
	// the cursor goes to the new row (wherever the sort puts it) and edits Item
	rows = t.a.v.GridRows()
	for i, oi := range t.order(rows) {
		if rows[oi].ID == nr.ID {
			t.row = i
		}
	}
	t.col = gcItem
	t.startEdit("")
}

func (t *gridTab) deleteRow() {
	r, ok := t.curRow()
	if !ok {
		return
	}
	id := r.ID
	t.a.OpenDialog(newConfirm(t.a, "Delete row", fmt.Sprintf("Delete row %d — %s?", r.ID, r.Item), "Delete", func() {
		var out []core.GridRow
		for _, x := range t.a.v.GridRows() {
			if x.ID != id {
				out = append(out, x)
			}
		}
		_ = t.commitRows(out)
	}))
}

func (t *gridTab) Synced(key string, ev syncEv) {
	if ev.conflict && t.editing {
		t.cancelEdit()
	}
}

func (t *gridTab) Render() {
	rows := t.a.v.GridRows()
	if t.row >= len(rows) {
		t.row = max(0, len(rows)-1)
	}
	if t.editing && t.editCol != gcStatus && t.ed.Truthy() && t.ed.Get("isConnected").Bool() {
		// editing in place: only the preview / error under the editor changes
		t.renderEdMsg()
		return
	}
	raw, _ := json.Marshal(rows)
	sig := fmt.Sprint(string(raw), t.row, t.col, t.sortCol, t.sortDir, t.editing, t.editCol, t.isCur(0))
	if sig != t.sig {
		t.sig = sig
		t.rebuild(rows)
	}
	if t.editing && t.editCol == gcStatus {
		t.status.anchor = t.grid.Call("querySelector", ".gcell.cur")
		t.status.Render()
	}
}

func (t *gridTab) renderEdMsg() {
	s := t.ed.Get("value").String()
	msg, cls := t.edErr, "edprev err"
	if msg == "" && t.editCol == gcDue {
		msg, _ = core.DatePreview(s, today())
		cls = "edprev"
		if _, err := core.ParseNatural(s, today()); err != nil {
			cls = "edprev err"
		}
		if strings.TrimSpace(s) == "" {
			msg = "empty = no due date"
		}
	}
	if msg == "" {
		cls = "edprev hidden"
	}
	t.edMsg.Set("className", cls)
	t.edMsg.Set("textContent", msg)
	if t.edErr != "" {
		t.ed.Set("className", "ced bad")
	} else {
		t.ed.Set("className", "ced")
	}
}

func (t *gridTab) rebuild(rows []core.GridRow) {
	Clear(t.grid)
	tbl := E("div", "gtable")
	head := E("div", "grow ghead2")
	for c, col := range gridCols {
		cls := "gcell hcell2 c" + strconv.Itoa(c)
		if col.right {
			cls += " r"
		}
		h := E("div", cls)
		Data(h, "h", strconv.Itoa(c))
		arrow := "⇅"
		if t.sortCol == c {
			arrow = map[int]string{1: "▲", -1: "▼"}[t.sortDir]
			h.Set("className", cls+" sorted")
		}
		Add(h, ET("span", "", col.title), ET("span", "ha", arrow))
		Add(head, h)
	}
	Add(tbl, head)
	var sumQty int64
	var sumTotal float64
	focused := t.isCur(0)
	for di, ri := range t.order(rows) {
		r := rows[ri]
		sumQty += r.Qty
		sumTotal += core.Round2(float64(r.Qty) * r.Price)
		rcls := "grow"
		if di%2 == 1 {
			rcls += " odd"
		}
		if di == t.row {
			rcls += " currow"
		}
		re := E("div", rcls)
		for c, col := range gridCols {
			cls := "gcell c" + strconv.Itoa(c)
			if col.right {
				cls += " r"
			}
			if !col.edit {
				cls += " ro"
			}
			if di == t.row && c == t.col {
				cls += " cur"
				if !focused {
					cls += " dim"
				}
			}
			ce := E("div", cls)
			Data(ce, "r", strconv.Itoa(di))
			Data(ce, "c", strconv.Itoa(c))
			switch {
			case t.editing && r.ID == t.editID && c == t.editCol && c != gcStatus:
				ce.Set("className", cls+" editing")
				Add(ce, t.ed, t.edMsg)
				t.renderEdMsg()
			case c == gcStatus:
				if s, ok := core.FindChoice(core.Statuses, r.Status); ok {
					Add(ce, chip(s))
				}
			case c == gcDue && r.Due != nil:
				if d, ok := core.ParseISO(*r.Due); ok {
					Add(ce, Txt(d.Short()+" "+strconv.Itoa(d.Year())))
					Attr(ce, "title", d.Long())
				}
			case c == gcDue:
				Add(ce, ET("span", "muted", "—"))
			default:
				Add(ce, Txt(core.SafeText(cellText(r, c))))
			}
			Add(re, ce)
		}
		Add(tbl, re)
	}
	foot := E("div", "grow gfoot2")
	for c, col := range gridCols {
		cls := "gcell c" + strconv.Itoa(c)
		if col.right {
			cls += " r"
		}
		txt := ""
		switch c {
		case gcItem:
			txt = plural(len(rows), "row")
		case gcQty:
			txt = strconv.FormatInt(sumQty, 10)
		case gcTotal:
			txt = money(sumTotal)
		case gcID:
			txt = "Σ"
		}
		Add(foot, ET("div", cls, txt))
	}
	Add(tbl, foot)
	Add(t.grid, tbl)
	if cur := t.grid.Call("querySelector", ".gcell.cur"); cur.Truthy() {
		cur.Call("scrollIntoView", map[string]any{"block": "nearest", "inline": "nearest"})
	}
}

func (t *gridTab) Hints() []hint {
	if t.editing {
		if t.editCol == gcStatus {
			return t.status.Hints()
		}
		return []hint{{"Enter", "Save ↓", "Enter"}, {"Tab", "Save →", "Tab"}, {"Esc", "Cancel", "Escape"}}
	}
	return []hint{{"arrows", "Move", ""}, {"Enter", "Edit", "Enter"}, {"Ins", "Add row", "Insert"}, {"F8", "Delete row", "F8"},
		{"Del", "Clear due", "Delete"}, {"^O", "Sort", "C-o"}}
}
