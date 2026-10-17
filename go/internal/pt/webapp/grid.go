//go:build js && wasm

package webapp

// The editable, virtualised data grid. Only the rows in view exist in the
// DOM (a small pool of row elements); every cell write is cached, so a
// re-render after scrolling or a remote change touches only cells whose
// text or class actually changed. Events are delegated to the grid root.

import (
	"encoding/json"
	"fmt"
	"math"
	"strconv"
	"strings"
	"syscall/js"

	"git.wharf.fluidgrid.org/jcz/tictactoe/go/internal/pt/core"
)

type GridOpts struct {
	ID           string
	Entity       core.Entity
	Activity     bool // rows are activity ids (read-only)
	Cols         func() []core.Column
	Frozen       int
	DefaultSort  []core.SortSpec
	DefaultGroup string
	ReadOnly     func() bool
	OnCursor     func()
	OnActivate   func(id int64) bool // Enter on a read-only row
	OnNew        func()
	ProjectHint  func() *int64 // project for new tasks / deps choices
}

type cellEl struct {
	el cachedEl
	v  cachedEl
}

type rowEl struct {
	el     cachedEl
	gutter cachedEl
	cells  []*cellEl
}

type Grid struct {
	app  *App
	opts GridOpts

	all       []core.Column
	vis       []int // indexes into all, display order
	hidden    map[string]bool
	order     []string
	widths    map[string]float64
	sort      []core.SortSpec
	group     string
	collapsed map[string]bool

	ids  []int64
	rows []core.GridRow

	curRow, curCol int
	curID          int64
	curGroup       string
	marks          map[int64]bool
	anchor         int

	root, inner, head, top, body, bot, foot, empty js.Value
	headCells                                       []*cachedEl
	footCells                                       []*cachedEl
	footGutter                                      cachedEl
	pool                                            []*rowEl
	first                                           int
	scope                                           Scope
	mounted                                         bool

	colsDirty bool
	colsVer   int
	rowsDirty bool
	scrollReq bool
	layoutSig string

	ed      *Editor
	edBase  any
	edField string
	edID    int64
	edRow   int
	edCol   int

	resizing *resizeState
}

type resizeState struct {
	key    string
	startX float64
	startW float64
	move   js.Func
	up     js.Func
}

type gridLayout struct {
	Order  []string           `json:"order"`
	Hidden []string           `json:"hidden"`
	Widths map[string]float64 `json:"widths"`
	Sort   []core.SortSpec    `json:"sort"`
	Group  *string            `json:"group"`
}

func NewGrid(a *App, o GridOpts) *Grid {
	g := &Grid{app: a, opts: o, hidden: map[string]bool{}, widths: map[string]float64{}, collapsed: map[string]bool{},
		marks: map[int64]bool{}, colsDirty: true, rowsDirty: true, sort: o.DefaultSort, group: o.DefaultGroup, anchor: -1}
	var l gridLayout
	if s := storageGet("pt.grid." + o.ID); s != "" && json.Unmarshal([]byte(s), &l) == nil {
		g.order = l.Order
		for _, h := range l.Hidden {
			g.hidden[h] = true
		}
		if l.Widths != nil {
			g.widths = l.Widths
		}
		if l.Sort != nil {
			g.sort = l.Sort
		}
		if l.Group != nil {
			g.group = *l.Group
		}
	}
	return g
}

func (g *Grid) saveLayout() {
	l := gridLayout{Order: g.order, Widths: g.widths, Sort: g.sort, Group: &g.group}
	for k, h := range g.hidden {
		if h {
			l.Hidden = append(l.Hidden, k)
		}
	}
	b, _ := json.Marshal(l)
	storageSet("pt.grid."+g.opts.ID, string(b))
}

func (g *Grid) rowH() float64 {
	if g.app.density == "compact" {
		return 21
	}
	return 25
}

func colPx(c *core.Column, widths map[string]float64) float64 {
	if w, ok := widths[c.Key]; ok && w >= 24 {
		return w
	}
	w := float64(c.Width)*7.6 + 16
	switch c.Key {
	case "health", "priority", "owner_id", "assignee_id", "program_id", "username", "user":
		w += 14 // room for the leading dot/swatch
	}
	return math.Round(w)
}

// ---- data in -------------------------------------------------------------------

func (g *Grid) SetIDs(ids []int64) {
	g.ids = ids
	g.rowsDirty = true
}

func (g *Grid) ColumnsChanged() { g.colsDirty = true; g.rowsDirty = true }

func (g *Grid) Invalidate() { g.rowsDirty = true }

func (g *Grid) visCols() []*core.Column {
	out := make([]*core.Column, len(g.vis))
	for i, j := range g.vis {
		out[i] = &g.all[j]
	}
	return out
}

func (g *Grid) visColsFlat() []core.Column {
	out := make([]core.Column, len(g.vis))
	for i, j := range g.vis {
		out[i] = g.all[j]
	}
	return out
}

func (g *Grid) computeCols() {
	g.all = g.opts.Cols()
	// display order: saved order first, then any new columns in default order
	idx := map[string]int{}
	for i, c := range g.all {
		idx[c.Key] = i
	}
	var vis []int
	seen := map[string]bool{}
	for _, k := range g.order {
		if i, ok := idx[k]; ok && !seen[k] {
			seen[k] = true
			vis = append(vis, i)
		}
	}
	for i, c := range g.all {
		if !seen[c.Key] {
			vis = append(vis, i)
		}
	}
	g.vis = g.vis[:0]
	for _, i := range vis {
		c := g.all[i]
		h, explicit := g.hidden[c.Key]
		if (explicit && h) || (!explicit && c.Hidden) {
			continue
		}
		g.vis = append(g.vis, i)
	}
	if g.curCol >= len(g.vis) {
		g.curCol = max(0, len(g.vis)-1)
	}
}

func (g *Grid) frozenN() int { return min(g.opts.Frozen, len(g.vis)) }

func (g *Grid) rebuildRows() {
	cols := g.allForRows()
	if g.opts.Activity {
		// newest first; activity has no core grid entity, so sort here
		ids := append([]int64(nil), g.ids...)
		g.rows = make([]core.GridRow, len(ids))
		for i, id := range ids {
			g.rows[i] = core.GridRow{ID: id}
		}
	} else {
		g.rows = core.BuildRows(g.app.r, cols, g.ids, g.sort, g.group, g.collapsed)
	}
	// keep the cursor on the same row id (or group)
	found := false
	for i, r := range g.rows {
		if (!r.IsGroup && g.curID != 0 && r.ID == g.curID) || (r.IsGroup && g.curID == 0 && g.curGroup != "" && r.Key == g.curGroup) {
			g.curRow = i
			found = true
			break
		}
	}
	if !found {
		g.curRow = min(g.curRow, max(0, len(g.rows)-1))
		g.syncCurID()
	}
	for id := range g.marks {
		if !g.app.r.Exists(g.opts.Entity, id) && !g.opts.Activity {
			delete(g.marks, id)
		}
	}
	g.rowsDirty = false
}

// allForRows: all columns (sorting/grouping may use hidden ones).
func (g *Grid) allForRows() []core.Column { return g.all }

func (g *Grid) syncCurID() {
	g.curID, g.curGroup = 0, ""
	if g.curRow >= 0 && g.curRow < len(g.rows) {
		r := g.rows[g.curRow]
		if r.IsGroup {
			g.curGroup = r.Key
		} else {
			g.curID = r.ID
		}
	}
}

// CurrentID: the data row under the cursor (0 on a group row).
func (g *Grid) CurrentID() int64 { return g.curID }

func (g *Grid) CurrentCol() *core.Column {
	if g.curCol < 0 || g.curCol >= len(g.vis) {
		return nil
	}
	return &g.all[g.vis[g.curCol]]
}

func (g *Grid) Count() int {
	n := 0
	for _, r := range g.rows {
		if !r.IsGroup {
			n++
		}
	}
	if g.group != "" {
		// collapsed groups hide rows but they still match the filter
		return len(g.ids)
	}
	return n
}

// ---- mounting ---------------------------------------------------------------------

func (g *Grid) Mount(parent js.Value) {
	g.scope.Release()
	g.root = E("div", "grid")
	Attr(g.root, "tabindex", "0")
	Data(g.root, "grid", g.opts.ID)
	g.inner = E("div", "gin")
	g.head = E("div", "ghead")
	g.top = E("div", "gspace")
	g.body = E("div", "gbody")
	g.bot = E("div", "gspace")
	g.foot = E("div", "gfoot")
	g.empty = ET("div", "gempty hidden", "No rows match the filter.")
	Add(g.inner, g.head, g.top, g.body, g.bot, g.foot)
	Add(g.root, g.inner, g.empty)
	Add(parent, g.root)
	g.pool = nil
	g.layoutSig = ""
	g.colsDirty = true
	g.rowsDirty = true
	g.mounted = true
	g.scope.On(g.root, "scroll", func(js.Value) { g.app.Invalidate() })
	g.scope.On(g.root, "mousedown", g.onMouseDown)
	g.scope.On(g.root, "dblclick", g.onDblClick)
	g.scope.On(g.root, "contextmenu", g.onContext)
	g.scope.On(g.root, "focus", func(js.Value) { g.app.Invalidate() })
}

func (g *Grid) Unmount() {
	g.closeEditor()
	g.scope.Release()
	g.mounted = false
	g.pool = nil
}

func (g *Grid) Focus() {
	if g.mounted {
		g.root.Call("focus", map[string]any{"preventScroll": true})
	}
}

func (g *Grid) HasFocus() bool {
	if !g.mounted {
		return false
	}
	ae := doc.Get("activeElement")
	return ae.Truthy() && g.root.Call("contains", ae).Bool()
}

// ---- rendering ----------------------------------------------------------------------

func (g *Grid) Render() {
	if !g.mounted {
		return
	}
	if g.colsVer != g.app.colsVersion {
		g.colsVer = g.app.colsVersion
		g.colsDirty, g.rowsDirty = true, true
	}
	if g.colsDirty {
		g.computeCols()
		g.colsDirty = false
	}
	if g.rowsDirty {
		g.rebuildRows()
	}
	rowH := g.rowH()
	cols := g.visCols()
	// layout signature: rebuild header/footer/pool when columns change
	sig := fmt.Sprint(rowH, g.frozenN())
	for _, c := range cols {
		sig += "|" + c.Key + ":" + strconv.FormatFloat(colPx(c, g.widths), 'f', 0, 64) + c.Title
	}
	if sig != g.layoutSig {
		g.layoutSig = sig
		g.buildHeadFoot(cols)
		Clear(g.body)
		g.pool = nil
	}
	n := len(g.rows)
	viewH := g.root.Get("clientHeight").Float()
	if viewH <= 0 {
		viewH = 600
	}
	if g.scrollReq {
		g.scrollReq = false
		g.scrollToCursor(viewH, cols)
	}
	scrollTop := g.root.Get("scrollTop").Float()
	first := int(scrollTop / rowH)
	if first > n {
		first = n
	}
	count := int(viewH/rowH) + 3
	if first+count > n {
		count = n - first
	}
	if count < 0 {
		count = 0
	}
	g.first = first
	Style(g.top, "height", Px(float64(first)*rowH))
	Style(g.bot, "height", Px(float64(n-first-count)*rowH))
	for len(g.pool) < count {
		g.pool = append(g.pool, g.newRow(len(g.pool), cols))
	}
	for i, pr := range g.pool {
		if i < count {
			pr.el.SetStyle("display", "")
			g.renderRow(pr, first+i, cols)
		} else {
			pr.el.SetStyle("display", "none")
		}
	}
	if n == 0 {
		g.empty.Set("className", "gempty")
	} else {
		g.empty.Set("className", "gempty hidden")
	}
	g.renderHeadState(cols)
	g.renderFoot(cols)
	if g.ed != nil {
		g.positionEditor(cols)
	}
}

func (g *Grid) frozenLeft(cols []*core.Column, j int) float64 {
	x := 30.0
	for i := 0; i < j; i++ {
		x += colPx(cols[i], g.widths)
	}
	return x
}

func (g *Grid) totalWidth(cols []*core.Column) float64 {
	return g.frozenLeft(cols, len(cols))
}

func (g *Grid) buildHeadFoot(cols []*core.Column) {
	Clear(g.head)
	Clear(g.foot)
	Style(g.inner, "width", Px(g.totalWidth(cols)))
	gh := E("div", "gutter")
	Attr(gh, "title", "Space marks a row")
	Add(g.head, gh)
	fg := E("div", "gutter")
	g.footGutter = cachedEl{el: fg}
	Add(g.foot, fg)
	g.headCells = nil
	g.footCells = nil
	fz := g.frozenN()
	for j, c := range cols {
		cls := "hcell"
		fcls := "cell"
		if c.Align == AlignRightCore {
			cls += " r"
			fcls += " r"
		}
		h := E("div", cls)
		Data(h, "h", strconv.Itoa(j))
		Style(h, "width", Px(colPx(c, g.widths)))
		f := E("div", fcls)
		Style(f, "width", Px(colPx(c, g.widths)))
		if j < fz {
			left := Px(g.frozenLeft(cols, j))
			extra := " frz"
			if j == fz-1 {
				extra += " frz-last"
			}
			h.Set("className", cls+extra)
			f.Set("className", fcls+extra)
			Style(h, "left", left)
			Style(f, "left", left)
		}
		title := ET("span", "ht", c.Title)
		Attr(h, "title", c.Title+" — click to sort, Shift+click adds a sort key, drag the edge to resize")
		arrow := E("span", "ha")
		rs := E("div", "rs")
		Data(rs, "rs", c.Key)
		Add(h, title, arrow, rs)
		Add(g.head, h)
		Add(g.foot, f)
		g.headCells = append(g.headCells, &cachedEl{el: arrow})
		g.footCells = append(g.footCells, &cachedEl{el: f})
	}
}

func (g *Grid) renderHeadState(cols []*core.Column) {
	for j, c := range cols {
		txt := ""
		for i, s := range g.sort {
			if s.Key == c.Key {
				if s.Desc {
					txt = "▼"
				} else {
					txt = "▲"
				}
				if len(g.sort) > 1 {
					txt += strconv.Itoa(i + 1)
				}
			}
		}
		if g.group == c.Key {
			txt += " ⊞"
		}
		g.headCells[j].SetText(txt)
	}
}

func (g *Grid) renderFoot(cols []*core.Column) {
	if g.opts.Activity {
		g.footGutter.SetText("")
		for _, f := range g.footCells {
			f.SetText("")
		}
		return
	}
	totals := core.Totals(g.app.r, g.visColsFlat(), g.ids)
	g.footGutter.SetText("Σ")
	// the row count goes in the first wide column without a total
	label := -1
	for j, c := range cols {
		if totals[j] == nil && colPx(c, g.widths) >= 70 {
			label = j
			break
		}
	}
	for j := range cols {
		t := ""
		if j == label {
			t = fmt.Sprintf("%d rows", len(g.ids))
		}
		if totals[j] != nil {
			t = core.FmtHours(*totals[j])
		}
		g.footCells[j].SetText(t)
	}
}

func (g *Grid) newRow(slot int, cols []*core.Column) *rowEl {
	el := E("div", "row")
	Data(el, "s", strconv.Itoa(slot))
	pr := &rowEl{el: cachedEl{el: el}}
	gut := E("div", "gutter")
	pr.gutter = cachedEl{el: gut}
	Add(el, gut)
	fz := g.frozenN()
	for j, c := range cols {
		ce := E("div", "cell")
		Data(ce, "c", strconv.Itoa(j))
		Style(ce, "width", Px(colPx(c, g.widths)))
		if j < fz {
			Style(ce, "left", Px(g.frozenLeft(cols, j)))
		}
		v := E("span", "v")
		Add(ce, v)
		Add(el, ce)
		pr.cells = append(pr.cells, &cellEl{el: cachedEl{el: ce}, v: cachedEl{el: v}})
	}
	Add(g.body, el)
	return pr
}

// AlignRightCore mirrors core.AlignRight (kept local for brevity).
const AlignRightCore = core.AlignRight

func (g *Grid) cell(col *core.Column, id int64) core.Cell {
	if g.opts.Activity {
		c := core.ActivityCell(g.app.r, col.Key, id)
		if col.Key == "ts" {
			if a := g.app.r.Activity[id]; a != nil {
				c.Text = core.ActivityWhen(a, nowSecs())
			}
		}
		return c
	}
	return core.CellFor(g.app.r, col, id)
}

func (g *Grid) renderRow(pr *rowEl, idx int, cols []*core.Column) {
	row := g.rows[idx]
	a := g.app
	focused := g.HasFocus() || g.ed != nil
	cls := "row"
	if idx%2 == 1 {
		cls += " odd"
	}
	if idx == g.curRow {
		cls += " currow"
		if focused {
			cls += " focus"
		}
	}
	fz := g.frozenN()
	if row.IsGroup {
		pr.el.SetClass(cls + " grp")
		pr.gutter.SetText("")
		pr.gutter.SetStyle("background", "")
		for j, c := range cols {
			ce := pr.cells[j]
			ccls := "cell"
			if c.Align == core.AlignRight {
				ccls += " r"
			}
			if j < fz {
				ccls += " frz"
				if j == fz-1 {
					ccls += " frz-last"
				}
			}
			if idx == g.curRow && focused && j == 0 {
				ccls += " cur"
			}
			txt := ""
			vcls := "v"
			if j == 0 {
				tog := "▾ "
				if row.Collapsed {
					tog = "▸ "
				}
				txt = tog + row.Label + "  (" + strconv.Itoa(row.Count) + ")"
				vcls = "v glabel"
				ccls += " gfirst"
			}
			if s := g.groupSum(row, c); s != "" {
				txt = s
				vcls = "v gsum"
			}
			ce.el.SetClass(ccls)
			ce.v.SetClass(vcls)
			ce.v.SetText(txt)
			ce.el.SetTitle("")
			ce.el.SetStyle("--pc", "")
			ce.v.SetStyle("--p", "")
			ce.v.SetStyle("--c", "")
		}
		return
	}
	id := row.ID
	if g.marks[id] {
		cls += " marked"
	}
	key := core.RowKey{Entity: g.opts.Entity, ID: id}
	if _, ok := a.flash[key]; ok && !g.opts.Activity {
		cls += " flash"
	}
	pr.el.SetClass(cls)
	// gutter: mark, or a presence badge
	gtxt := ""
	gbg := ""
	gtitle := ""
	if g.marks[id] {
		gtxt = "●"
	}
	var peers []core.Peer
	if !g.opts.Activity {
		peers = a.peersOn(g.opts.Entity, id)
	}
	if len(peers) > 0 {
		u := a.r.Users[peers[0].UserID]
		if u != nil {
			gtxt = u.Initials()
			gbg = userColor(u.ID)
			gtitle = u.DisplayName + " is here"
		}
	}
	pr.gutter.SetText(gtxt)
	pr.gutter.SetStyle("background", gbg)
	pr.gutter.SetTitle(gtitle)
	if gbg != "" {
		pr.gutter.SetClass("gutter peer")
	} else {
		pr.gutter.SetClass("gutter")
	}
	for j, c := range cols {
		ce := pr.cells[j]
		cell := g.cell(c, id)
		ccls := "cell"
		if c.Align == core.AlignRight {
			ccls += " r"
		}
		if j < fz {
			ccls += " frz"
			if j == fz-1 {
				ccls += " frz-last"
			}
		}
		if idx == g.curRow && j == g.curCol {
			if focused {
				ccls += " cur"
			} else {
				ccls += " curdim"
			}
		}
		title := ""
		pc := ""
		if c.Field != "" {
			ck := cellKey{g.opts.Entity, id, c.Field}
			if a.pendCells[ck] > 0 {
				ccls += " pending"
			}
			if cf, ok := a.conflicts[ck]; ok {
				ccls += " conflict"
				who := "someone"
				if cf.by != nil {
					who = a.r.UserName(cf.by)
				}
				title = fmt.Sprintf("Conflict: %s set this to %s. Enter edits from their value, Esc dismisses.", who,
					core.DisplayValue(a.r, g.opts.Entity, c.Field, cf.current))
			}
			for _, p := range peers {
				if p.Focus != nil && p.Focus.Field != nil && *p.Focus.Field == c.Field {
					ccls += " peer"
					pc = userColor(p.UserID)
					if u := a.r.Users[p.UserID]; u != nil {
						title = u.DisplayName + " is on this cell"
					}
				}
			}
		}
		if g.ed != nil && idx == g.edRow && j == g.edCol {
			ccls += " editing"
		}
		vcls, text, p, cvar := styleOf(cell)
		if strings.Contains(ccls, "conflict") {
			text = "⚠ " + text
		}
		ce.el.SetClass(ccls)
		ce.el.SetTitle(title)
		ce.el.SetStyle("--pc", pc)
		ce.v.SetStyle("--p", p)
		ce.v.SetStyle("--c", cvar)
		if cell.Style.Kind == core.StTags && text != "" {
			// one chip per tag; rebuild only when the text changed
			if ce.v.text != text || ce.v.class != vcls {
				ce.v.SetClass(vcls)
				ce.v.text = text
				Clear(ce.v.el)
				for _, t := range strings.Fields(text) {
					Add(ce.v.el, ET("span", "tag", t))
				}
			}
			continue
		}
		ce.v.SetClass(vcls)
		ce.v.SetText(text)
	}
}

// groupSum: the subtotal text for column c of a group row.
func (g *Grid) groupSum(row core.GridRow, c *core.Column) string {
	for i := range g.all {
		if g.all[i].Key == c.Key && i < len(row.Sums) && row.Sums[i] != nil && c.Sum {
			return core.FmtHours(*row.Sums[i])
		}
	}
	return ""
}

// styleOf maps a core style hint to (span class, text, --p, --c).
func styleOf(c core.Cell) (string, string, string, string) {
	t := c.Text
	switch c.Style.Kind {
	case core.StMuted:
		return "v muted", t, "", ""
	case core.StStrong:
		return "v strong", t, "", ""
	case core.StProjectStatus:
		return "v chip ps-" + c.Style.Value, t, "", ""
	case core.StTaskStatus:
		return "v chip ts-" + c.Style.Value, t, "", ""
	case core.StPriority:
		return "v pri pri-" + c.Style.Value, t, "", ""
	case core.StHealth:
		return "v health h-" + c.Style.Value, t, "", ""
	case core.StRole:
		return "v chip role-" + c.Style.Value, t, "", ""
	case core.StOverdue:
		return "v c-err", t, "", ""
	case core.StDueSoon:
		return "v c-warn", t, "", ""
	case core.StBar:
		p := math.Max(0, math.Min(100, c.Style.Pct))
		return "v barv l" + strconv.Itoa(c.Style.Level), t, strconv.FormatFloat(p, 'f', 1, 64) + "%", ""
	case core.StBad:
		return "v c-err", t, "", ""
	case core.StGood:
		return "v c-ok", t, "", ""
	case core.StTags:
		return "v tags", t, "", ""
	case core.StColor:
		return "v swatch", t, "", c.Style.Value
	case core.StUser:
		if t == "" {
			return "v", t, "", ""
		}
		return "v user", t, "", userColor(c.Style.UserID)
	case core.StStruck:
		return "v struck", t, "", ""
	}
	return "v", t, "", ""
}

// ---- cursor & scrolling ----------------------------------------------------------------

func (g *Grid) scrollToCursor(viewH float64, cols []*core.Column) {
	rowH := g.rowH()
	top := g.root.Get("scrollTop").Float()
	y := float64(g.curRow) * rowH
	// header occupies one row at the top, footer one at the bottom
	if y < top {
		g.root.Set("scrollTop", y)
	} else if y+rowH > top+viewH-2*rowH {
		g.root.Set("scrollTop", y+3*rowH-viewH)
	}
	if g.curCol < len(cols) && g.curCol >= g.frozenN() {
		left := g.root.Get("scrollLeft").Float()
		viewW := g.root.Get("clientWidth").Float()
		x := g.frozenLeft(cols, g.curCol)
		w := colPx(cols[g.curCol], g.widths)
		fzw := g.frozenLeft(cols, g.frozenN())
		if x-left < fzw {
			g.root.Set("scrollLeft", x-fzw)
		} else if x+w-left > viewW {
			g.root.Set("scrollLeft", x+w-viewW+4)
		}
	}
}

func (g *Grid) moveTo(row, col int) {
	if len(g.rows) == 0 {
		g.curRow, g.curCol = 0, max(0, min(col, len(g.vis)-1))
		g.syncCurID()
		return
	}
	g.curRow = max(0, min(row, len(g.rows)-1))
	g.curCol = max(0, min(col, len(g.vis)-1))
	g.syncCurID()
	g.scrollReq = true
	if g.opts.OnCursor != nil {
		g.opts.OnCursor()
	}
	g.app.sendFocus(false)
}

func (g *Grid) pageRows() int {
	h := g.root.Get("clientHeight").Float()
	return max(1, int(h/g.rowH())-3)
}

// ---- keyboard -------------------------------------------------------------------------------

func (g *Grid) Key(k Key) bool {
	if g.ed != nil {
		return g.ed.Key(k)
	}
	if !g.HasFocus() {
		return false
	}
	r, c := g.curRow, g.curCol
	onGroup := r < len(g.rows) && g.rows[r].IsGroup
	switch {
	case k.Is("ArrowUp"):
		g.moveTo(r-1, c)
	case k.Is("ArrowDown"):
		g.moveTo(r+1, c)
	case k.Is("ArrowLeft") && onGroup:
		g.setCollapsed(g.rows[r].Key, true)
	case k.Is("ArrowRight") && onGroup:
		g.setCollapsed(g.rows[r].Key, false)
	case k.Is("ArrowLeft"):
		g.moveTo(r, c-1)
	case k.Is("ArrowRight"):
		g.moveTo(r, c+1)
	case k.Is("Tab"):
		if c+1 < len(g.vis) {
			g.moveTo(r, c+1)
		} else {
			g.moveTo(r+1, 0)
		}
	case k.Is("S-Tab"):
		if c > 0 {
			g.moveTo(r, c-1)
		} else if r > 0 {
			g.moveTo(r-1, len(g.vis)-1)
		}
	case k.Is("Home"):
		g.moveTo(r, 0)
	case k.Is("End"):
		g.moveTo(r, len(g.vis)-1)
	case k.Is("C-Home"):
		g.moveTo(0, c)
	case k.Is("C-End"):
		g.moveTo(len(g.rows)-1, c)
	case k.Is("PageUp"):
		g.moveTo(r-g.pageRows(), c)
	case k.Is("PageDown"):
		g.moveTo(r+g.pageRows(), c)
	case k.Is("S-ArrowUp", "S-ArrowDown"):
		d := 1
		if k.Is("S-ArrowUp") {
			d = -1
		}
		if g.anchor < 0 {
			g.anchor = r
		}
		g.moveTo(r+d, c)
		g.markRange(g.anchor, g.curRow)
	case k.Is(" ") && !onGroup:
		if g.curID != 0 {
			if g.marks[g.curID] {
				delete(g.marks, g.curID)
			} else {
				g.marks[g.curID] = true
			}
			g.anchor = g.curRow
		}
	case k.Is("Enter", " ") && onGroup:
		g.setCollapsed(g.rows[r].Key, !g.rows[r].Collapsed)
	case k.Is("Enter", "F2"):
		if g.conflictAtCursor() {
			g.BeginEdit("", false)
		} else if !g.BeginEdit("", false) && g.opts.OnActivate != nil && g.curID != 0 {
			g.opts.OnActivate(g.curID)
		}
	case k.Is("Escape"):
		if g.conflictAtCursor() {
			col := g.CurrentCol()
			delete(g.app.conflicts, cellKey{g.opts.Entity, g.curID, col.Field})
		} else if len(g.marks) > 0 {
			g.marks = map[int64]bool{}
		} else {
			return false
		}
	case k.Is("Delete", "Backspace") && !k.Ctrl:
		if k.Name == "Backspace" {
			g.BeginEdit("", true)
		} else {
			g.ClearCell()
		}
	case k.Printable():
		if !g.BeginEdit(k.Name, true) {
			return false
		}
	default:
		return false
	}
	if !k.Is("S-ArrowUp", "S-ArrowDown") {
		g.anchor = -1
	}
	return true
}

func (g *Grid) conflictAtCursor() bool {
	col := g.CurrentCol()
	if col == nil || col.Field == "" || g.curID == 0 {
		return false
	}
	_, ok := g.app.conflicts[cellKey{g.opts.Entity, g.curID, col.Field}]
	return ok
}

func (g *Grid) markRange(a, b int) {
	if a > b {
		a, b = b, a
	}
	g.marks = map[int64]bool{}
	for i := a; i <= b && i < len(g.rows); i++ {
		if !g.rows[i].IsGroup {
			g.marks[g.rows[i].ID] = true
		}
	}
}

func (g *Grid) MarkAll() {
	if len(g.marks) > 0 {
		g.marks = map[int64]bool{}
		return
	}
	for _, r := range g.rows {
		if !r.IsGroup {
			g.marks[r.ID] = true
		}
	}
}

func (g *Grid) setCollapsed(key string, v bool) {
	if v {
		g.collapsed[key] = true
	} else {
		delete(g.collapsed, key)
	}
	g.rowsDirty = true
}

// markedIDs in display order (or the current row when none are marked).
func (g *Grid) markedIDs() []int64 {
	var out []int64
	for _, r := range g.rows {
		if !r.IsGroup && g.marks[r.ID] {
			out = append(out, r.ID)
		}
	}
	if len(out) == 0 && g.curID != 0 {
		out = []int64{g.curID}
	}
	return out
}

// ---- sorting / grouping -----------------------------------------------------------------------

func (g *Grid) CycleSort(add bool) {
	col := g.CurrentCol()
	if col == nil {
		return
	}
	g.cycleSortKey(col.Key, add)
}

func (g *Grid) cycleSortKey(key string, add bool) {
	idx := -1
	for i, s := range g.sort {
		if s.Key == key {
			idx = i
		}
	}
	switch {
	case idx < 0 && add:
		g.sort = append(g.sort, core.SortSpec{Key: key})
	case idx < 0:
		g.sort = []core.SortSpec{{Key: key}}
	case !g.sort[idx].Desc:
		if add {
			g.sort[idx].Desc = true
		} else {
			g.sort = []core.SortSpec{{Key: key, Desc: true}}
		}
	default:
		g.sort = append(g.sort[:idx:idx], g.sort[idx+1:]...)
	}
	g.rowsDirty = true
	g.saveLayout()
	g.app.setStatus("info", "Sort: "+g.SortLabel())
}

func (g *Grid) SortLabel() string {
	if len(g.sort) == 0 {
		return "none"
	}
	var parts []string
	for _, s := range g.sort {
		t := s.Key
		for _, c := range g.all {
			if c.Key == s.Key {
				t = c.Title
			}
		}
		if s.Desc {
			t += " ▼"
		} else {
			t += " ▲"
		}
		parts = append(parts, t)
	}
	return strings.Join(parts, ", ")
}

func (g *Grid) SetSort(s []core.SortSpec) {
	g.sort = s
	g.rowsDirty = true
	g.saveLayout()
}

func (g *Grid) ToggleGroup() {
	col := g.CurrentCol()
	if col == nil {
		return
	}
	if g.group == col.Key {
		g.SetGroup("")
	} else {
		g.SetGroup(col.Key)
	}
}

func (g *Grid) SetGroup(key string) {
	g.group = key
	g.collapsed = map[string]bool{}
	g.rowsDirty = true
	g.saveLayout()
	if key == "" {
		g.app.setStatus("info", "Grouping cleared")
	} else {
		g.app.setStatus("info", "Grouped by "+g.colTitle(key))
	}
}

func (g *Grid) colTitle(key string) string {
	for _, c := range g.all {
		if c.Key == key {
			return c.Title
		}
	}
	return key
}

func (g *Grid) Indicators() string {
	var parts []string
	if len(g.sort) > 0 {
		parts = append(parts, "⇅ "+g.SortLabel())
	}
	if g.group != "" {
		parts = append(parts, "⊞ "+g.colTitle(g.group))
	}
	if n := len(g.marks); n > 0 {
		parts = append(parts, fmt.Sprintf("● %d marked", n))
	}
	return strings.Join(parts, "   ")
}

// ---- editing ------------------------------------------------------------------------------------

func (g *Grid) canEdit(col *core.Column) bool {
	if col == nil || !col.Editable() || g.opts.Activity || g.curID == 0 {
		return false
	}
	if !g.app.r.CanEdit() {
		return false
	}
	if g.opts.ReadOnly != nil && g.opts.ReadOnly() {
		return false
	}
	if fm := core.Field(g.opts.Entity, col.Field); fm != nil && fm.Perm == core.PermAdmin && !g.app.r.IsAdmin() {
		return false
	}
	return true
}

// BeginEdit opens the editor for the current cell. seed is the first typed
// character (replace mode) when byTyping.
func (g *Grid) BeginEdit(seed string, byTyping bool) bool {
	col := g.CurrentCol()
	if !g.canEdit(col) {
		if col != nil && col.Editable() && g.curID != 0 && !g.app.r.CanEdit() {
			g.app.setStatus("warn", "Read only: viewers can't edit")
		}
		return false
	}
	id := g.curID
	ck := cellKey{g.opts.Entity, id, col.Field}
	base := g.app.r.FieldValue(g.opts.Entity, id, col.Field)
	if cf, ok := g.app.conflicts[ck]; ok {
		// edit again starting from their value
		base = cf.current
		delete(g.app.conflicts, ck)
	}
	if col.Editor == core.EdBool {
		b, _ := base.(bool)
		g.app.SetField(g.opts.Entity, id, col.Field, base, !b)
		return true
	}
	g.edBase, g.edField, g.edID, g.edRow, g.edCol = base, col.Field, id, g.curRow, g.curCol
	text := core.EditText(g.app.r, g.opts.Entity, col.Field, id)
	if ck2, ok := g.app.conflicts[ck]; ok {
		text = core.DisplayValue(g.app.r, g.opts.Entity, col.Field, ck2.current)
	}
	var hint *int64
	if g.opts.ProjectHint != nil {
		hint = g.opts.ProjectHint()
	}
	ed := NewEditor(g.app, EditorOpts{
		Kind: col.Editor, Entity: g.opts.Entity, Field: col.Field, ID: id, ProjectHint: hint,
		Initial: base, InitialText: text, Seed: seed, Replace: byTyping, Title: col.Title,
		OnCommit: func(v any, move string) { g.commitEdit(v, move) },
		OnCancel: func() { g.closeEditor(); g.Focus() },
	})
	g.ed = ed
	Add(g.inner, ed.root)
	g.positionEditor(g.visCols())
	ed.Focus()
	return true
}

func (g *Grid) positionEditor(cols []*core.Column) {
	if g.ed == nil || g.edCol >= len(cols) {
		return
	}
	rowH := g.rowH()
	y := float64(g.edRow+1) * rowH // +1: header row
	x := g.frozenLeft(cols, g.edCol)
	if g.edCol < g.frozenN() {
		x += g.root.Get("scrollLeft").Float()
	}
	w := colPx(cols[g.edCol], g.widths)
	g.ed.Place(x, y, w, rowH)
}

func (g *Grid) commitEdit(v any, move string) {
	e, id, field, base := g.opts.Entity, g.edID, g.edField, g.edBase
	g.closeEditor()
	g.app.SetField(e, id, field, base, v)
	switch move {
	case "down":
		g.moveTo(g.curRow+1, g.curCol)
	case "right":
		if g.curCol+1 < len(g.vis) {
			g.moveTo(g.curRow, g.curCol+1)
		} else {
			g.moveTo(g.curRow+1, 0)
		}
	case "left":
		g.moveTo(g.curRow, g.curCol-1)
	}
	g.Focus()
	g.app.Invalidate()
}

func (g *Grid) closeEditor() {
	if g.ed != nil {
		g.ed.Destroy()
		g.ed = nil
	}
	g.app.Invalidate()
}

func (g *Grid) Editing() bool { return g.ed != nil }

func (g *Grid) ClearCell() {
	col := g.CurrentCol()
	if !g.canEdit(col) {
		return
	}
	v, err := core.ParseInput(g.app.r, g.opts.Entity, col.Field, g.curID, "")
	if err != nil {
		g.app.setStatus("err", err.Error())
		return
	}
	g.app.SetField(g.opts.Entity, g.curID, col.Field, g.app.r.FieldValue(g.opts.Entity, g.curID, col.Field), v)
}

// ---- clipboard / bulk ----------------------------------------------------------------------------

func (g *Grid) cellText(col *core.Column, id int64) string {
	if col.Field != "" && !g.opts.Activity {
		return core.EditText(g.app.r, g.opts.Entity, col.Field, id)
	}
	return g.cell(col, id).Text
}

func (g *Grid) Copy() {
	cols := g.visCols()
	var b strings.Builder
	if len(g.marks) > 0 {
		for _, id := range g.markedIDs() {
			for j, c := range cols {
				if j > 0 {
					b.WriteByte('\t')
				}
				b.WriteString(strings.ReplaceAll(g.cellText(c, id), "\t", " "))
			}
			b.WriteByte('\n')
		}
		g.app.setStatus("ok", fmt.Sprintf("✓ Copied %d rows", len(g.marks)))
	} else if col := g.CurrentCol(); col != nil && g.curID != 0 {
		b.WriteString(g.cellText(col, g.curID))
		g.app.setStatus("ok", "✓ Copied cell")
	}
	g.app.copyText(b.String())
}

func (g *Grid) Paste(text string) {
	text = strings.TrimRight(strings.ReplaceAll(text, "\r\n", "\n"), "\n")
	if text == "" || g.opts.Activity {
		return
	}
	if !g.app.r.CanEdit() {
		g.app.setStatus("warn", "Read only")
		return
	}
	cols := g.visCols()
	var ops []core.Op
	var errs []string
	ri := g.curRow
	for _, line := range strings.Split(text, "\n") {
		for ri < len(g.rows) && g.rows[ri].IsGroup {
			ri++
		}
		if ri >= len(g.rows) {
			break
		}
		id := g.rows[ri].ID
		for j, val := range strings.Split(line, "\t") {
			cj := g.curCol + j
			if cj >= len(cols) {
				break
			}
			c := cols[cj]
			if !c.Editable() {
				continue
			}
			if fm := core.Field(g.opts.Entity, c.Field); fm != nil && fm.Perm == core.PermAdmin && !g.app.r.IsAdmin() {
				continue
			}
			v, err := core.ParseInput(g.app.r, g.opts.Entity, c.Field, id, val)
			if err != nil {
				errs = append(errs, c.Title+": "+err.Error())
				continue
			}
			base := g.app.r.FieldValue(g.opts.Entity, id, c.Field)
			if !core.ValuesEqual(base, v) {
				ops = append(ops, core.SetOp(g.opts.Entity, id, c.Field, base, v))
			}
		}
		ri++
	}
	if len(errs) > 0 {
		g.app.toast("warn", fmt.Sprintf("%d value(s) skipped", len(errs)), errs[0])
	}
	if len(ops) > 0 {
		g.app.Mutate(ops, "paste", nil)
		g.app.setStatus("ok", fmt.Sprintf("Pasting %d cell(s)…", len(ops)))
	}
}

func (g *Grid) FillDown() {
	col := g.CurrentCol()
	if !g.canEdit(col) {
		return
	}
	ids := g.markedIDs()
	var src int64
	var targets []int64
	if len(g.marks) >= 2 {
		src, targets = ids[0], ids[1:]
	} else {
		// copy from the data row above
		for i := g.curRow - 1; i >= 0; i-- {
			if !g.rows[i].IsGroup {
				src = g.rows[i].ID
				break
			}
		}
		targets = []int64{g.curID}
	}
	if src == 0 {
		return
	}
	v := g.app.r.FieldValue(g.opts.Entity, src, col.Field)
	var ops []core.Op
	for _, id := range targets {
		base := g.app.r.FieldValue(g.opts.Entity, id, col.Field)
		if !core.ValuesEqual(base, v) {
			ops = append(ops, core.SetOp(g.opts.Entity, id, col.Field, base, v))
		}
	}
	g.app.Mutate(ops, "fill down", nil)
	g.app.setStatus("ok", fmt.Sprintf("Filled %d cell(s) down", len(ops)))
}

func (g *Grid) DeleteRows() {
	if g.opts.Activity || g.opts.Entity == core.EntUser {
		g.app.setStatus("warn", "These rows can't be deleted (deactivate users instead)")
		return
	}
	ids := g.markedIDs()
	if len(ids) == 0 {
		return
	}
	var labels []string
	for i, id := range ids {
		if i == 5 {
			labels = append(labels, fmt.Sprintf("… and %d more", len(ids)-5))
			break
		}
		labels = append(labels, g.app.r.RowLabel(g.opts.Entity, id))
	}
	e := g.opts.Entity
	msg := fmt.Sprintf("Delete %d %s(s)?\n%s", len(ids), e.Label(), strings.Join(labels, "\n"))
	if e == core.EntProject {
		msg += "\nTheir tasks are deleted too. You can undo with ^Z."
	}
	g.app.OpenDialog(newConfirm(g.app, "Delete", msg, "Delete", func() {
		var ops []core.Op
		for _, id := range ids {
			ops = append(ops, core.DeleteOp(e, id))
		}
		g.marks = map[int64]bool{}
		g.app.Mutate(ops, "delete", nil)
	}))
}

// ---- mouse ------------------------------------------------------------------------------------------

func (g *Grid) cellAt(t js.Value) (row, col int, ok bool) {
	ce := Closest(t, ".cell")
	re := Closest(t, ".row")
	if !re.Truthy() || Closest(t, ".gfoot").Truthy() {
		return 0, 0, false
	}
	slot, _ := strconv.Atoi(DataOf(re, "s"))
	row = g.first + slot
	col = g.curCol
	if ce.Truthy() {
		col, _ = strconv.Atoi(DataOf(ce, "c"))
	}
	return row, col, row < len(g.rows)
}

func (g *Grid) onMouseDown(ev js.Value) {
	t := ev.Get("target")
	if ev.Get("button").Int() != 0 {
		return
	}
	if g.ed != nil && g.ed.root.Call("contains", t).Bool() {
		return
	}
	if rs := Closest(t, "[data-rs]"); rs.Truthy() {
		ev.Call("preventDefault")
		g.startResize(DataOf(rs, "rs"), ev.Get("clientX").Float())
		return
	}
	if h := Closest(t, "[data-h]"); h.Truthy() {
		j, _ := strconv.Atoi(DataOf(h, "h"))
		if j < len(g.vis) {
			g.cycleSortKey(g.all[g.vis[j]].Key, ev.Get("shiftKey").Bool())
		}
		g.app.Invalidate()
		return
	}
	row, col, ok := g.cellAt(t)
	if !ok {
		return
	}
	if g.ed != nil {
		g.ed.CommitIfValid()
	}
	if Closest(t, ".gutter").Truthy() && !g.rows[row].IsGroup {
		id := g.rows[row].ID
		if ev.Get("shiftKey").Bool() && g.anchor >= 0 {
			g.markRange(g.anchor, row)
		} else if g.marks[id] {
			delete(g.marks, id)
		} else {
			g.marks[id] = true
			g.anchor = row
		}
	} else if g.rows[row].IsGroup {
		g.curRow = row
		g.syncCurID()
		g.setCollapsed(g.rows[row].Key, !g.rows[row].Collapsed)
	}
	g.moveTo(row, col)
	g.scrollReq = false
	g.Focus()
	ev.Call("preventDefault")
	g.app.Invalidate()
}

func (g *Grid) onDblClick(ev js.Value) {
	if _, _, ok := g.cellAt(ev.Get("target")); ok && g.ed == nil {
		if !g.BeginEdit("", false) && g.opts.OnActivate != nil && g.curID != 0 {
			g.opts.OnActivate(g.curID)
		}
		g.app.Invalidate()
	}
}

func (g *Grid) onContext(ev js.Value) {
	row, col, ok := g.cellAt(ev.Get("target"))
	if !ok {
		return
	}
	ev.Call("preventDefault")
	g.moveTo(row, col)
	g.scrollReq = false
	g.Focus()
	g.OpenContext(ev.Get("clientX").Float(), ev.Get("clientY").Float())
	g.app.Invalidate()
}

func (g *Grid) startResize(key string, x float64) {
	var w float64
	for _, c := range g.all {
		if c.Key == key {
			w = colPx(&c, g.widths)
		}
	}
	st := &resizeState{key: key, startX: x, startW: w}
	st.move = js.FuncOf(func(_ js.Value, args []js.Value) any {
		nx := args[0].Get("clientX").Float()
		g.widths[key] = math.Max(36, math.Round(st.startW+nx-st.startX))
		g.app.Invalidate()
		return nil
	})
	st.up = js.FuncOf(func(js.Value, []js.Value) any {
		doc.Call("removeEventListener", "mousemove", st.move)
		doc.Call("removeEventListener", "mouseup", st.up)
		st.move.Release()
		st.up.Release()
		g.resizing = nil
		g.saveLayout()
		return nil
	})
	g.resizing = st
	doc.Call("addEventListener", "mousemove", st.move)
	doc.Call("addEventListener", "mouseup", st.up)
}

// ---- context menu ------------------------------------------------------------------------------------

// queryTerm: a filter term matching the current cell's value.
func (g *Grid) queryTerm(col *core.Column, id int64) string {
	r := g.app.r
	if g.opts.Activity || col == nil {
		return ""
	}
	v := r.FieldValue(g.opts.Entity, id, col.Field)
	quote := func(s string) string {
		if strings.ContainsAny(s, " \t") {
			return `"` + s + `"`
		}
		return s
	}
	switch col.Key {
	case "status", "priority", "health":
		if s, ok := v.(string); ok {
			return col.Key + ":" + s
		}
	case "owner_id", "assignee_id":
		f := "owner"
		if col.Key == "assignee_id" {
			f = "assignee"
		}
		if n, ok := core.AsInt(v); ok {
			return f + ":" + r.Username(&n)
		}
		return f + ":none"
	case "program_id":
		if n, ok := core.AsInt(v); ok {
			if p := r.Programs[n]; p != nil {
				return "program:" + strings.ToLower(p.Prefix)
			}
		}
		return "program:none"
	case "project":
		if t := r.Tasks[id]; t != nil {
			return "project:" + strings.ToLower(r.ProjectCode(t.ProjectID))
		}
	case "tags":
		if l, ok := core.AsList(v); ok && len(l) > 0 {
			return "#" + fmt.Sprint(l[0])
		}
	case "start_date", "due_date":
		f := strings.TrimSuffix(col.Key, "_date")
		if s, ok := v.(string); ok {
			return f + ":" + s
		}
		return f + ":none"
	case "code", "name", "title", "phase":
		f := col.Key
		if f == "name" && g.opts.Entity == core.EntTask {
			f = "title"
		}
		if s, ok := v.(string); ok && s != "" {
			return f + ":" + quote(strings.ToLower(s))
		}
	}
	if cfid, ok := core.CfID(col.Key); ok {
		if cf := r.CustomFields[cfid]; cf != nil && v != nil {
			return cf.QueryName() + ":" + quote(strings.ToLower(fmt.Sprint(v)))
		}
	}
	return ""
}

func (g *Grid) OpenContext(x, y float64) {
	col := g.CurrentCol()
	id := g.curID
	if x < 0 {
		// keyboard: below the current cell
		cols := g.visCols()
		rect := g.root.Call("getBoundingClientRect")
		x = rect.Get("left").Float() + g.frozenLeft(cols, g.curCol) - g.root.Get("scrollLeft").Float() + 10
		y = rect.Get("top").Float() + float64(g.curRow-g.first+2)*g.rowH() + 2
	}
	term := g.queryTerm(col, id)
	items := []ctxItem{
		{"Edit cell", "Enter", func() { g.BeginEdit("", false) }, g.canEdit(col)},
		{"Record form…", "^E", func() { g.app.editRecord() }, !g.opts.Activity && id != 0},
		{"-", "", nil, true},
		{"Copy", "^C", g.Copy, true},
		{"Paste", "^V", func() { g.Paste(g.app.clipboard) }, g.app.clipboard != ""},
		{"Fill down", "^D", g.FillDown, g.canEdit(col)},
		{"Clear cell", "Del", g.ClearCell, g.canEdit(col)},
		{"-", "", nil, true},
		{"Filter by this value", "", func() { g.app.addQueryTerm(term) }, term != ""},
		{"Exclude this value", "", func() { g.app.addQueryTerm("-" + term) }, term != ""},
		{"Sort ▲", "", func() { g.SetSort([]core.SortSpec{{Key: col.Key}}) }, col != nil},
		{"Sort ▼", "", func() { g.SetSort([]core.SortSpec{{Key: col.Key, Desc: true}}) }, col != nil},
		{"Group by this column", "^R", g.ToggleGroup, col != nil && !g.opts.Activity},
		{"Hide column", "", func() { g.hidden[col.Key] = true; g.ColumnsChanged(); g.saveLayout() }, col != nil},
		{"-", "", nil, true},
		{"Delete row(s)", "F8", g.DeleteRows, !g.opts.Activity && g.app.r.CanEdit()},
	}
	g.app.ctx = &ContextMenu{x: x, y: y, items: items, sel: 0}
}

func (a *App) addQueryTerm(term string) {
	q := strings.TrimSpace(a.curView().Query())
	if q != "" {
		q += " "
	}
	a.curView().SetQuery(q + term)
	a.syncQueryInput()
	a.Invalidate()
}
