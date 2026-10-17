//go:build js && wasm

package webapp

import (
	"fmt"
	"math"
	"strconv"
	"syscall/js"

	"git.wharf.fluidgrid.org/jcz/tictactoe/go/internal/pt/core"
)

type tlRow struct {
	e     core.Entity
	id    int64
	depth int
	start *core.Date
	due   *core.Date
}

type timelineView struct {
	app      *App
	qs       queryState
	rows     []tlRow
	expanded map[int64]bool
	cur      int
	zoom     int // 0 day, 1 week, 2 month
	root     js.Value
	left     js.Value
	right    js.Value
	scroller js.Value
	sig      string
	scope    Scope
	from     core.Date
	days     int
	drag     *tlDrag
	scrollTo bool
	shown    int
}

type tlDrag struct {
	row     int
	resize  bool
	startX  float64
	dx      float64
	bar     js.Value
	move    js.Func
	up      js.Func
	origW   float64
	pxDay   float64
	moved   bool
}

var zoomPx = []float64{26, 7, 2.2}
var zoomName = []string{"days", "weeks", "months"}
var zoomStep = []int{1, 7, 30}

func newTimelineView(a *App) *timelineView {
	v := &timelineView{app: a, expanded: map[int64]bool{}, zoom: 1, scrollTo: true}
	if z, err := strconv.Atoi(storageGet("pt.tl.zoom")); err == nil && z >= 0 && z < 3 {
		v.zoom = z
	}
	v.qs.target = core.TargetProjects
	return v
}

func (v *timelineView) ID() string                    { return "timeline" }
func (v *timelineView) Target() (core.Target, bool)    { return core.TargetProjects, true }
func (v *timelineView) Query() string                  { return v.qs.q }
func (v *timelineView) QueryErrors() []core.QueryError { return v.qs.QueryErrors() }
func (v *timelineView) SetQuery(q string)              { v.qs.setQ(q); v.sig = "" }
func (v *timelineView) DataChanged(bool)               { v.qs.compiled = nil; v.sig = "" }
func (v *timelineView) PresenceChanged()               {}
func (v *timelineView) ActiveGrid() *Grid              { return nil }
func (v *timelineView) Count() (int, int)              { return v.shown, len(v.app.r.Projects) }
func (v *timelineView) Indicators() string             { return "zoom: " + zoomName[v.zoom] }
func (v *timelineView) NextPane(int)                   {}
func (v *timelineView) ToggleDetail()                  {}
func (v *timelineView) FocusDOM()                      { v.scroller.Call("focus", map[string]any{"preventScroll": true}) }

func (v *timelineView) Focus() (core.Entity, int64, string) {
	if v.cur < len(v.rows) {
		return v.rows[v.cur].e, v.rows[v.cur].id, "due_date"
	}
	return "", 0, ""
}

func (v *timelineView) Hints() []hint {
	return []hint{{"↑↓", "Row", ""}, {"→/Enter", "Expand", ""}, {"←", "Collapse", ""}, {"⇧←→", "Move bar", ""},
		{"[ ]", "Shorter/longer", ""}, {"+ −", "Zoom", ""}, {"t", "Today", ""}, {"^E", "Form", "edit_record"}, {"drag", "Move / resize", ""}}
}

func (v *timelineView) Mount(parent js.Value) {
	v.scope.Release()
	v.root = E("div", "pane full tlpane")
	t := E("div", "ptitle")
	Add(t, ET("span", "", "Timeline"), ET("span", "pcount", ""))
	v.scroller = E("div", "tl")
	Attr(v.scroller, "tabindex", "0")
	Add(v.root, t, v.scroller)
	Add(parent, v.root)
	v.sig = ""
	v.scrollTo = true
	v.scope.On(v.scroller, "mousedown", v.onMouseDown)
	v.scope.On(v.scroller, "dblclick", func(ev js.Value) {
		if r := Closest(ev.Get("target"), "[data-row]"); r.Truthy() {
			i, _ := strconv.Atoi(DataOf(r, "row"))
			if i < len(v.rows) {
				v.app.editRecordOf(v.rows[i].e, v.rows[i].id)
			}
		}
	})
}

func (v *timelineView) Unmount() { v.scope.Release() }

func parseDP(s *string) *core.Date {
	if s == nil {
		return nil
	}
	if d, ok := core.ParseISO(*s); ok {
		return &d
	}
	return nil
}

func (v *timelineView) build() {
	r := v.app.r
	q := v.qs.compile(r)
	v.rows = v.rows[:0]
	v.shown = 0
	ids := core.SortedIDs(r.Projects)
	// order by start date, then code
	sortByStart(r, ids)
	for _, id := range ids {
		p := r.Projects[id]
		if !q.MatchesProject(p, r) {
			continue
		}
		v.shown++
		v.rows = append(v.rows, tlRow{e: core.EntProject, id: id, start: parseDP(p.StartDate), due: parseDP(p.DueDate)})
		if v.expanded[id] {
			for _, tid := range r.ProjectTasks(id) {
				t := r.Tasks[tid]
				v.rows = append(v.rows, tlRow{e: core.EntTask, id: tid, depth: 1, start: parseDP(t.StartDate), due: parseDP(t.DueDate)})
			}
		}
	}
	// date range: around everything visible, at least 90 days
	from, to := r.Today.AddDays(-30), r.Today.AddDays(60)
	for _, row := range v.rows {
		if row.start != nil && *row.start < from {
			from = *row.start
		}
		if row.due != nil && *row.due > to {
			to = *row.due
		}
	}
	v.from = from.AddDays(-7).WeekStart()
	v.days = int(to-v.from) + 21
	v.cur = max(0, min(v.cur, len(v.rows)-1))
}

func sortByStart(r *core.Replica, ids []int64) {
	key := func(id int64) string {
		p := r.Projects[id]
		return deref(p.StartDate) + p.Code
	}
	for i := 1; i < len(ids); i++ {
		for j := i; j > 0 && key(ids[j]) < key(ids[j-1]); j-- {
			ids[j], ids[j-1] = ids[j-1], ids[j]
		}
	}
}

const tlRowH = 24.0
const tlLabelW = 300.0
const tlHeadH = 38.0

func (v *timelineView) x(d core.Date) float64 {
	return float64(int(d)-int(v.from)) * zoomPx[v.zoom]
}

func (v *timelineView) Render() {
	a := v.app
	sig := fmt.Sprint(a.r.Version, v.qs.q, v.zoom, v.cur, len(v.expanded), a.theme, a.density, len(a.pendCells))
	for id := range v.expanded {
		sig += "," + strconv.FormatInt(id, 10)
	}
	if sig == v.sig || v.drag != nil {
		return
	}
	v.sig = sig
	v.build()
	r := a.r
	px := zoomPx[v.zoom]
	w := float64(v.days) * px
	h := tlHeadH + float64(len(v.rows))*tlRowH + 8
	_ = h
	keepLeft := v.scroller.Get("scrollLeft").Float()
	keepTop := v.scroller.Get("scrollTop").Float()
	Clear(v.scroller)
	wrap := E("div", "tlwrap")
	Style(wrap, "width", Px(tlLabelW+w))
	// left: sticky labels
	left := E("div", "tlleft")
	Style(left, "height", Px(h))
	lh := E("div", "tllhead")
	Add(lh, ET("span", "", fmt.Sprintf("%d projects", v.shown)), ET("span", "muted", " · "+zoomName[v.zoom]))
	Add(left, lh)
	for i, row := range v.rows {
		cls := "tlrow"
		if i == v.cur {
			cls += " cur"
		}
		el := E("div", cls)
		Data(el, "row", strconv.Itoa(i))
		Style(el, "padding-left", Px(8+float64(row.depth)*18))
		if row.e == core.EntProject {
			p := r.Projects[row.id]
			tog := "▸ "
			if v.expanded[row.id] {
				tog = "▾ "
			}
			Add(el, ET("span", "tog", tog), ET("span", "code", p.Code), ET("span", "tll", " "+p.Name))
			Add(el, ET("span", "health h-"+string(p.Health), ""))
		} else {
			t := r.Tasks[row.id]
			Add(el, ET("span", "muted", fmt.Sprintf("#%d ", t.Seq)), ET("span", "tll", t.Title))
		}
		Add(left, el)
	}
	// right: a sticky header SVG over the body SVG
	bodyH := float64(len(v.rows))*tlRowH + 8
	head := S("svg", "class", "tlsvg", "width", f1(w), "height", f1(tlHeadH))
	svg := S("svg", "class", "tlsvg", "width", f1(w), "height", f1(bodyH))
	defs := S("defs")
	marker := S("marker", "id", "arr", "viewBox", "0 0 8 8", "refX", "7", "refY", "4", "markerWidth", "7", "markerHeight", "7", "orient", "auto-start-reverse")
	Add(marker, S("path", "d", "M0,0 L8,4 L0,8 z", "class", "deparrow"))
	Add(defs, marker)
	Add(svg, defs)
	// weekend shading & grid
	if v.zoom < 2 {
		for d := 0; d < v.days; d++ {
			if v.from.AddDays(d).Weekday() >= 5 {
				Add(svg, S("rect", "x", f1(float64(d)*px), "y", "0", "width", f1(px), "height", f1(bodyH), "class", "tlwe"))
			}
		}
	}
	for d := 0; d < v.days; d++ {
		day := v.from.AddDays(d)
		x := float64(d) * px
		if day.Day() == 1 || d == 0 {
			Add(head, S("line", "x1", f1(x), "x2", f1(x), "y1", "0", "y2", f1(tlHeadH), "class", "tlmonth"))
			Add(svg, S("line", "x1", f1(x), "x2", f1(x), "y1", "0", "y2", f1(bodyH), "class", "tlmonth"))
			t := S("text", "x", f1(x+4), "y", "14", "class", "tlmt")
			t.Set("textContent", fmt.Sprintf("%s %d", core.MonthNames[day.Month()-1], day.Year()))
			Add(head, t)
		}
		switch {
		case v.zoom == 0:
			t := S("text", "x", f1(x+px/2-4), "y", "32", "class", "tlday")
			t.Set("textContent", strconv.Itoa(day.Day()))
			Add(head, t)
		case v.zoom == 1 && day.Weekday() == 0:
			Add(svg, S("line", "x1", f1(x), "x2", f1(x), "y1", "0", "y2", f1(bodyH), "class", "tlweek"))
			t := S("text", "x", f1(x+3), "y", "32", "class", "tlday")
			t.Set("textContent", strconv.Itoa(day.Day()))
			Add(head, t)
		}
	}
	// rows
	rowY := map[int64]float64{}
	barEnd := map[int64]float64{}
	barStart := map[int64]float64{}
	labelX := v.scroller.Get("scrollLeft").Float() + 4 // keep bar labels clear of the sticky column
	for i, row := range v.rows {
		y := float64(i) * tlRowH
		if i == v.cur {
			Add(svg, S("rect", "x", "0", "y", f1(y), "width", f1(w), "height", f1(tlRowH), "class", "tlcur"))
		}
		if row.start == nil && row.due == nil {
			continue
		}
		s, e := row.start, row.due
		if s == nil {
			s = e
		}
		if e == nil {
			e = s
		}
		x0 := v.x(*s)
		x1 := v.x(e.AddDays(1))
		bw := math.Max(x1-x0, 3)
		g := S("g", "class", "tlbar", "data-row", strconv.Itoa(i))
		cls, prog, tip := "", 0.0, ""
		if row.e == core.EntProject {
			p := r.Projects[row.id]
			m := r.Metrics(row.id)
			cls = "pbar h-" + string(p.Health)
			prog = m.PctDone()
			tip = fmt.Sprintf("%s %s · %s → %s · %.0f%% done", p.Code, p.Name, deref(p.StartDate), deref(p.DueDate), prog)
		} else {
			t := r.Tasks[row.id]
			cls = "tbar ts-" + string(t.Status)
			if t.EstHours != nil && *t.EstHours > 0 {
				prog = math.Min(100, t.ActualHours*100 / *t.EstHours)
			}
			if t.Status == core.TaskDone {
				prog = 100
			}
			tip = fmt.Sprintf("#%d %s · %s → %s · %s", t.Seq, t.Title, deref(t.StartDate), deref(t.DueDate), t.Status.Label())
			rowY[row.id] = y + tlRowH/2
			barStart[row.id] = x0
			barEnd[row.id] = x0 + bw
			if *s == *e && (t.EstHours == nil || *t.EstHours == 0) {
				// milestone diamond
				cx, cy := x0+px/2, y+tlRowH/2
				Add(g, S("path", "d", fmt.Sprintf("M%.1f,%.1f l7,7 l-7,7 l-7,-7 z", cx, cy-7), "class", "milestone"))
				ti := S("title")
				ti.Set("textContent", tip+" (milestone)")
				Add(g, ti)
				Add(svg, g)
				continue
			}
		}
		if a.pendCells[cellKey{row.e, row.id, "due_date"}] > 0 || a.pendCells[cellKey{row.e, row.id, "start_date"}] > 0 {
			cls += " pending"
		}
		Add(g, S("rect", "x", f1(x0), "y", f1(y+5), "width", f1(bw), "height", f1(tlRowH-10), "rx", "3", "class", cls))
		if prog > 0 {
			Add(g, S("rect", "x", f1(x0), "y", f1(y+tlRowH-8), "width", f1(bw*prog/100), "height", "3", "rx", "1.5", "class", "tlprog"))
		}
		Add(g, S("rect", "x", f1(x0+bw-5), "y", f1(y+5), "width", "6", "height", f1(tlRowH-10), "class", "tlgrip", "data-grip", "1"))
		ti := S("title")
		ti.Set("textContent", tip)
		Add(g, ti)
		if row.e == core.EntProject && bw > 60 {
			lx := math.Max(x0+6, labelX)
			if lx < x0+bw-40 {
				lbl := S("text", "x", f1(lx), "y", f1(y+16), "class", "tlbl")
				lbl.Set("textContent", r.Projects[row.id].Code)
				Add(g, lbl)
			}
		}
		Add(svg, g)
	}
	// dependency arrows between visible task bars
	for _, row := range v.rows {
		if row.e != core.EntTask {
			continue
		}
		t := r.Tasks[row.id]
		for _, d := range t.Deps {
			ye, ok1 := rowY[d]
			ys, ok2 := rowY[row.id]
			if !ok1 || !ok2 {
				continue
			}
			x0 := barEnd[d]
			x1 := barStart[row.id]
			mid := x0 + 8
			path := fmt.Sprintf("M%.1f,%.1f H%.1f V%.1f H%.1f", x0, ye, mid, ys, x1-1)
			cls := "deppath"
			if st, _ := r.TaskStatusOf(d); st != core.TaskDone {
				cls += " open"
			}
			Add(svg, S("path", "d", path, "class", cls, "marker-end", "url(#arr)"))
		}
	}
	// today line
	tx := v.x(r.Today) + px/2
	Add(svg, S("line", "x1", f1(tx), "x2", f1(tx), "y1", "0", "y2", f1(bodyH), "class", "tltoday"))
	Add(head, S("line", "x1", f1(tx), "x2", f1(tx), "y1", "18", "y2", f1(tlHeadH), "class", "tltoday"))
	tt := S("text", "x", f1(tx+3), "y", "26", "class", "tltodayt")
	tt.Set("textContent", "today")
	Add(head, tt)
	headWrap := E("div", "tlhead")
	Add(headWrap, head)
	right := E("div", "tlright")
	Add(right, headWrap, svg)
	Add(wrap, left, right)
	Add(v.scroller, wrap)
	v.left, v.right = left, right
	if v.scrollTo {
		v.scrollTo = false
		v.scroller.Set("scrollLeft", math.Max(0, tx-200))
	} else {
		v.scroller.Set("scrollLeft", keepLeft)
		v.scroller.Set("scrollTop", keepTop)
	}
	// keep the current row visible
	y := tlHeadH + float64(v.cur)*tlRowH
	top := v.scroller.Get("scrollTop").Float()
	vh := v.scroller.Get("clientHeight").Float()
	if y < top+tlHeadH {
		v.scroller.Set("scrollTop", y-tlHeadH)
	} else if y+tlRowH > top+vh {
		v.scroller.Set("scrollTop", y+tlRowH-vh+4)
	}
	v.root.Call("querySelector", ".pcount").Set("textContent", fmt.Sprintf("%d rows · drag bars to move, edges to resize", len(v.rows)))
}

func f1(x float64) string { return strconv.FormatFloat(x, 'f', 1, 64) }

func (v *timelineView) Key(k Key) bool {
	if !v.scroller.Equal(doc.Get("activeElement")) {
		return false
	}
	row := func() *tlRow {
		if v.cur < len(v.rows) {
			return &v.rows[v.cur]
		}
		return nil
	}
	switch {
	case k.Is("ArrowUp"):
		v.cur = max(0, v.cur-1)
	case k.Is("ArrowDown"):
		v.cur = min(len(v.rows)-1, v.cur+1)
	case k.Is("PageUp"):
		v.cur = max(0, v.cur-15)
	case k.Is("PageDown"):
		v.cur = min(len(v.rows)-1, v.cur+15)
	case k.Is("Home"):
		v.cur = 0
	case k.Is("End"):
		v.cur = len(v.rows) - 1
	case k.Is("ArrowRight", "Enter"):
		if r := row(); r != nil && r.e == core.EntProject {
			if k.Is("Enter") && v.expanded[r.id] {
				delete(v.expanded, r.id)
			} else {
				v.expanded[r.id] = true
			}
		} else if r != nil && k.Is("Enter") {
			v.app.editRecordOf(r.e, r.id)
		} else {
			v.scroller.Set("scrollLeft", v.scroller.Get("scrollLeft").Float()+120)
		}
	case k.Is("ArrowLeft"):
		if r := row(); r != nil && r.e == core.EntProject && v.expanded[r.id] {
			delete(v.expanded, r.id)
		} else if r != nil && r.e == core.EntTask {
			// jump to the parent project
			for i := v.cur; i >= 0; i-- {
				if v.rows[i].e == core.EntProject {
					v.cur = i
					break
				}
			}
		} else {
			v.scroller.Set("scrollLeft", v.scroller.Get("scrollLeft").Float()-120)
		}
	case k.Is("S-ArrowLeft", "S-ArrowRight"):
		d := zoomStep[v.zoom]
		if k.Is("S-ArrowLeft") {
			d = -d
		}
		v.shift(v.cur, d, d)
	case k.Is("["):
		v.shift(v.cur, 0, -zoomStep[v.zoom])
	case k.Is("]"):
		v.shift(v.cur, 0, zoomStep[v.zoom])
	case k.Is("+", "="):
		v.setZoom(v.zoom - 1)
	case k.Is("-"):
		v.setZoom(v.zoom + 1)
	case k.Is("t"):
		v.scrollTo = true
	case k.Is("Escape"):
		return false
	default:
		return false
	}
	v.sig = ""
	v.app.sendFocus(false)
	return true
}

func (v *timelineView) setZoom(z int) {
	if z < 0 || z > 2 {
		return
	}
	v.zoom = z
	storageSet("pt.tl.zoom", strconv.Itoa(z))
	v.scrollTo = true
}

// shift moves start by ds and due by dd days (one mutate, both fields).
func (v *timelineView) shift(i, ds, dd int) {
	if i < 0 || i >= len(v.rows) || !v.app.r.CanEdit() {
		return
	}
	row := v.rows[i]
	if row.start == nil && row.due == nil {
		v.app.setStatus("warn", "This row has no dates to move")
		return
	}
	var ops []core.Op
	if row.start != nil && ds != 0 {
		ops = append(ops, core.SetOp(row.e, row.id, "start_date", row.start.ISO(), row.start.AddDays(ds).ISO()))
	}
	if row.due != nil && dd != 0 {
		nd := row.due.AddDays(dd)
		if row.start != nil && nd < row.start.AddDays(ds) {
			nd = row.start.AddDays(ds)
		}
		if nd != *row.due {
			ops = append(ops, core.SetOp(row.e, row.id, "due_date", row.due.ISO(), nd.ISO()))
		}
	}
	// the due date moves first when shifting right, so due ≥ start holds mid-batch too
	if ds > 0 && len(ops) == 2 {
		ops[0], ops[1] = ops[1], ops[0]
	}
	v.app.Mutate(ops, "move bar", nil)
}

func (v *timelineView) onMouseDown(ev js.Value) {
	t := ev.Get("target")
	r := Closest(t, "[data-row]")
	if !r.Truthy() {
		return
	}
	i, _ := strconv.Atoi(DataOf(r, "row"))
	if i >= len(v.rows) {
		return
	}
	if Closest(t, ".tlleft").Truthy() && Closest(t, ".tog").Truthy() {
		row := v.rows[i]
		if row.e == core.EntProject {
			if v.expanded[row.id] {
				delete(v.expanded, row.id)
			} else {
				v.expanded[row.id] = true
			}
		}
	}
	v.cur = i
	v.sig = ""
	v.FocusDOM()
	ev.Call("preventDefault")
	if Closest(t, ".tlbar").Truthy() && v.app.r.CanEdit() {
		// start dragging this bar
		bar := Closest(t, ".tlbar")
		d := &tlDrag{row: i, resize: DataOf(t, "grip") == "1", startX: ev.Get("clientX").Float(), bar: bar, pxDay: zoomPx[v.zoom]}
		d.move = js.FuncOf(func(_ js.Value, args []js.Value) any {
			d.dx = args[0].Get("clientX").Float() - d.startX
			if math.Abs(d.dx) > 2 {
				d.moved = true
			}
			if d.resize {
				// stretch only the main rect via transform on the grip
				d.bar.Call("setAttribute", "transform", "")
				rects := d.bar.Call("querySelectorAll", "rect")
				if rects.Length() > 0 {
					main := rects.Index(0)
					if d.origW == 0 {
						d.origW, _ = strconv.ParseFloat(main.Call("getAttribute", "width").String(), 64)
					}
					main.Call("setAttribute", "width", f1(math.Max(3, d.origW+d.dx)))
				}
			} else {
				d.bar.Call("setAttribute", "transform", "translate("+f1(d.dx)+",0)")
			}
			return nil
		})
		d.up = js.FuncOf(func(js.Value, []js.Value) any {
			doc.Call("removeEventListener", "mousemove", d.move)
			doc.Call("removeEventListener", "mouseup", d.up)
			d.move.Release()
			d.up.Release()
			v.drag = nil
			days := int(math.Round(d.dx / d.pxDay))
			if d.moved && days != 0 {
				if d.resize {
					v.shift(d.row, 0, days)
				} else {
					v.shift(d.row, days, days)
				}
			}
			v.sig = ""
			v.app.Invalidate()
			return nil
		})
		v.drag = d
		doc.Call("addEventListener", "mousemove", d.move)
		doc.Call("addEventListener", "mouseup", d.up)
	}
	v.app.Invalidate()
}

func (v *timelineView) New() { v.app.OpenDialog(newProjectDialog(v.app)) }
