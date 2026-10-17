//go:build js && wasm

package webapp

import (
	"fmt"
	"sort"
	"strconv"
	"syscall/js"

	"git.wharf.fluidgrid.org/jcz/tictactoe/go/internal/pt/core"
)

const boardCap = 120 // cards rendered per column (per lane)

type boardLane struct {
	key   string
	label string
	user  *int64
	cells [5][]int64
}

type boardView struct {
	app      *App
	qs       queryState
	lanes    []boardLane
	swim     bool
	lane     int
	col      int
	idx      int
	selID    int64
	root     js.Value
	scopeBar js.Value
	boardEl  js.Value
	prog     *Editor
	proj     *Editor
	sig      string
	scope    Scope
	shown    int
	total    int
}

var statusOrder = []core.TaskStatus{core.TaskTodo, core.TaskInProgress, core.TaskBlocked, core.TaskReview, core.TaskDone}

func newBoardView(a *App) *boardView {
	v := &boardView{app: a, swim: storageGet("pt.board.swim") == "on"}
	v.qs.target = core.TargetTasks
	return v
}

func (v *boardView) ID() string                    { return "board" }
func (v *boardView) Target() (core.Target, bool)    { return core.TargetTasks, true }
func (v *boardView) Query() string                  { return v.qs.q }
func (v *boardView) QueryErrors() []core.QueryError { return v.qs.QueryErrors() }
func (v *boardView) SetQuery(q string)              { v.qs.setQ(q); v.sig = "" }
func (v *boardView) DataChanged(bool)               { v.qs.compiled = nil; v.sig = "" }
func (v *boardView) PresenceChanged()               { v.sig = "" }
func (v *boardView) ActiveGrid() *Grid              { return nil }
func (v *boardView) Count() (int, int)              { return v.shown, len(v.app.r.Tasks) }
func (v *boardView) NextPane(int)                   { v.prog.Focus() }
func (v *boardView) ToggleDetail()                  {}
func (v *boardView) FocusDOM()                      { v.boardEl.Call("focus", map[string]any{"preventScroll": true}) }

func (v *boardView) Indicators() string {
	if v.swim {
		return "≡ swimlanes by assignee"
	}
	return ""
}

func (v *boardView) Focus() (core.Entity, int64, string) {
	if v.selID != 0 {
		return core.EntTask, v.selID, "status"
	}
	return "", 0, ""
}

func (v *boardView) Hints() []hint {
	return []hint{{"←→↑↓", "Move", ""}, {"⇧←→ / < >", "Change status", ""}, {"Enter", "Open", "edit_record"},
		{"Ins", "New task here", "new"}, {"^R", "Swimlanes", ""}, {"F6", "Scope", "next_pane"}, {"drag", "Move card", ""}}
}

func (v *boardView) Mount(parent js.Value) {
	v.scope.Release()
	v.root = E("div", "boardwrap")
	v.scopeBar = E("div", "scopebar")
	a := v.app
	progChoices := func() []core.Choice {
		cs := core.Choices(a.r, core.EntProject, "program_id", 0, nil)
		cs[0].Label = "All programs"
		return cs
	}
	projChoices := func() []core.Choice {
		out := []core.Choice{{Value: nil, Label: "All projects", Style: core.Style{Kind: core.StMuted}}}
		pid, hasProg := core.AsInt(v.prog.value)
		for _, c := range core.Choices(a.r, core.EntTask, "project_id", 0, nil) {
			id, _ := core.AsInt(c.Value)
			if p := a.r.Projects[id]; hasProg && (p.ProgramID == nil || *p.ProgramID != pid) {
				continue
			}
			out = append(out, c)
		}
		return out
	}
	v.prog = NewEditor(a, EditorOpts{Kind: core.EdCombo, Entity: core.EntProject, Field: "program_id", Embedded: true,
		Choices: progChoices, OnChange: func(any) {
			v.proj.choices = projChoices()
			v.proj.value = nil
			v.proj.in.Set("value", "All projects")
			v.sig = ""
			a.Invalidate()
		}})
	v.prog.in.Set("value", "All programs")
	v.proj = NewEditor(a, EditorOpts{Kind: core.EdCombo, Entity: core.EntTask, Field: "project_id", Embedded: true,
		Choices: projChoices, OnChange: func(any) { v.sig = ""; a.Invalidate() }})
	v.proj.in.Set("value", "All projects")
	swim := ET("button", "btn sm", "")
	Attr(swim, "type", "button")
	v.scope.On(swim, "click", func(js.Value) { v.toggleSwim() })
	Add(v.scopeBar, ET("span", "qlabel", "Scope ▸"), v.prog.root, ET("span", "muted", "▸"), v.proj.root, E("span", "spacer"), swim)
	v.boardEl = E("div", "board")
	Attr(v.boardEl, "tabindex", "0")
	Add(v.root, v.scopeBar, v.boardEl)
	Add(parent, v.root)
	v.sig = ""
	v.scope.On(v.boardEl, "mousedown", func(ev js.Value) {
		if c := Closest(ev.Get("target"), "[data-card]"); c.Truthy() {
			id, _ := strconv.ParseInt(DataOf(c, "card"), 10, 64)
			v.selectID(id)
			a.Invalidate()
		}
	})
	v.scope.On(v.boardEl, "dblclick", func(ev js.Value) {
		if c := Closest(ev.Get("target"), "[data-card]"); c.Truthy() {
			a.editRecordOf(core.EntTask, v.selID)
		}
	})
	v.scope.On(v.boardEl, "dragstart", func(ev js.Value) {
		if c := Closest(ev.Get("target"), "[data-card]"); c.Truthy() {
			ev.Get("dataTransfer").Call("setData", "text/plain", DataOf(c, "card"))
			ev.Get("dataTransfer").Set("effectAllowed", "move")
			c.Get("classList").Call("add", "dragging")
		}
	})
	v.scope.On(v.boardEl, "dragover", func(ev js.Value) {
		if cell := Closest(ev.Get("target"), "[data-drop]"); cell.Truthy() {
			ev.Call("preventDefault")
			ev.Get("dataTransfer").Set("dropEffect", "move")
			cell.Get("classList").Call("add", "over")
		}
	})
	v.scope.On(v.boardEl, "dragleave", func(ev js.Value) {
		if cell := Closest(ev.Get("target"), "[data-drop]"); cell.Truthy() {
			cell.Get("classList").Call("remove", "over")
		}
	})
	v.scope.On(v.boardEl, "drop", func(ev js.Value) {
		cell := Closest(ev.Get("target"), "[data-drop]")
		if !cell.Truthy() {
			return
		}
		ev.Call("preventDefault")
		id, _ := strconv.ParseInt(ev.Get("dataTransfer").Call("getData", "text/plain").String(), 10, 64)
		col, _ := strconv.Atoi(DataOf(cell, "drop"))
		lane, _ := strconv.Atoi(DataOf(cell, "lane"))
		v.moveCard(id, col, lane)
		v.sig = ""
		a.Invalidate()
	})
	v.scope.On(swim, "mouseover", func(js.Value) {})
	v.swimBtn(swim)
}

func (v *boardView) swimBtn(b js.Value) {
	if v.swim {
		b.Set("textContent", "≡ Swimlanes: assignee (^R)")
	} else {
		b.Set("textContent", "≡ Swimlanes: off (^R)")
	}
}

func (v *boardView) toggleSwim() {
	v.swim = !v.swim
	if v.swim {
		storageSet("pt.board.swim", "on")
	} else {
		storageSet("pt.board.swim", "off")
	}
	if b := v.scopeBar.Call("querySelector", "button"); b.Truthy() {
		v.swimBtn(b)
	}
	v.sig = ""
	v.app.Invalidate()
}

func (v *boardView) Unmount() {
	v.scope.Release()
	if v.prog != nil {
		v.prog.Destroy()
		v.proj.Destroy()
	}
}

func (v *boardView) build() {
	a := v.app
	r := a.r
	q := v.qs.compile(r)
	progID, hasProg := core.AsInt(v.prog.value)
	projID, hasProj := core.AsInt(v.proj.value)
	lanes := map[string]*boardLane{}
	var order []string
	v.shown = 0
	for _, id := range core.SortedIDs(r.Tasks) {
		t := r.Tasks[id]
		if hasProj && t.ProjectID != projID {
			continue
		}
		if hasProg {
			p := r.Projects[t.ProjectID]
			if p == nil || p.ProgramID == nil || *p.ProgramID != progID {
				continue
			}
		}
		if !q.MatchesTask(t, r) {
			continue
		}
		key, label := "all", "All"
		var user *int64
		if v.swim {
			key, label = "~", "Unassigned"
			if t.AssigneeID != nil {
				key = r.Username(t.AssigneeID)
				label = r.UserName(t.AssigneeID)
				u := *t.AssigneeID
				user = &u
			}
		}
		l := lanes[key]
		if l == nil {
			l = &boardLane{key: key, label: label, user: user}
			lanes[key] = l
			order = append(order, key)
		}
		ci := 0
		for i, s := range statusOrder {
			if s == t.Status {
				ci = i
			}
		}
		l.cells[ci] = append(l.cells[ci], id)
		v.shown++
	}
	sort.Strings(order)
	v.lanes = v.lanes[:0]
	for _, k := range order {
		l := lanes[k]
		for c := range l.cells {
			ids := l.cells[c]
			// most urgent first: priority, then due date
			sort.SliceStable(ids, func(i, j int) bool {
				ti, tj := r.Tasks[ids[i]], r.Tasks[ids[j]]
				if ti.Priority.Index() != tj.Priority.Index() {
					return ti.Priority.Index() > tj.Priority.Index()
				}
				return deref(ti.DueDate) < deref(tj.DueDate) && ti.DueDate != nil || tj.DueDate == nil && ti.DueDate != nil
			})
		}
		v.lanes = append(v.lanes, *l)
	}
	if len(v.lanes) == 0 {
		v.lanes = []boardLane{{key: "all", label: "All"}}
	}
	// keep the selection on the same card
	if v.selID != 0 {
		v.locate(v.selID)
	}
	v.clampSel()
}

func (v *boardView) locate(id int64) bool {
	for li, l := range v.lanes {
		for ci, ids := range l.cells {
			for i, x := range ids {
				if x == id {
					v.lane, v.col, v.idx = li, ci, i
					return true
				}
			}
		}
	}
	return false
}

func (v *boardView) clampSel() {
	v.lane = max(0, min(v.lane, len(v.lanes)-1))
	v.col = max(0, min(v.col, 4))
	ids := v.lanes[v.lane].cells[v.col]
	v.idx = max(0, min(v.idx, min(len(ids), boardCap)-1))
	if len(ids) == 0 {
		v.selID = 0
	} else {
		v.selID = ids[v.idx]
	}
}

func (v *boardView) selectID(id int64) {
	if v.locate(id) {
		v.selID = id
		v.app.sendFocus(false)
	}
}

func (v *boardView) Render() {
	a := v.app
	sig := fmt.Sprint(a.r.Version, v.qs.q, v.prog.value, v.proj.value, v.swim, v.selID, a.density, len(a.pendCells))
	if sig == v.sig {
		return
	}
	v.build()
	v.sig = fmt.Sprint(a.r.Version, v.qs.q, v.prog.value, v.proj.value, v.swim, v.selID, a.density, len(a.pendCells))
	r := a.r
	Clear(v.boardEl)
	cls := "board"
	if v.swim {
		cls += " swim"
	}
	v.boardEl.Set("className", cls)
	// column headers with counts and hours
	head := E("div", "bhead")
	if v.swim {
		Add(head, E("div", "lanelabel"))
	}
	for ci, s := range statusOrder {
		n, est := 0, 0.0
		for _, l := range v.lanes {
			n += len(l.cells[ci])
			for _, id := range l.cells[ci] {
				if t := r.Tasks[id]; t.EstHours != nil {
					est += *t.EstHours
				}
			}
		}
		h := E("div", "bcolh")
		Add(h, ET("span", "chip ts-"+string(s), s.Label()), ET("span", "bcount", fmt.Sprintf("%d · %s h", n, core.FmtHours(est))))
		Add(head, h)
	}
	Add(v.boardEl, head)
	for li, l := range v.lanes {
		row := E("div", "blane")
		if v.swim {
			lab := E("div", "lanelabel")
			if l.user != nil {
				if u := r.Users[*l.user]; u != nil {
					av := ET("span", "av", u.Initials())
					Style(av, "background", userColor(u.ID))
					Add(lab, av)
				}
			}
			cnt := 0
			for _, c := range l.cells {
				cnt += len(c)
			}
			Add(lab, ET("span", "", l.label), ET("span", "bcount", strconv.Itoa(cnt)))
			Add(row, lab)
		}
		for ci := range statusOrder {
			cell := E("div", "bcol")
			Data(cell, "drop", strconv.Itoa(ci))
			Data(cell, "lane", strconv.Itoa(li))
			ids := l.cells[ci]
			for i, id := range ids {
				if i >= boardCap {
					Add(cell, ET("div", "bmore muted", fmt.Sprintf("+ %d more — narrow the scope or filter", len(ids)-boardCap)))
					break
				}
				Add(cell, v.card(id, li == v.lane && ci == v.col && i == v.idx))
			}
			if len(ids) == 0 {
				Add(cell, ET("div", "bempty muted", "drop here"))
			}
			Add(row, cell)
		}
		Add(v.boardEl, row)
	}
	if s := v.boardEl.Call("querySelector", ".card.sel"); s.Truthy() {
		s.Call("scrollIntoView", map[string]any{"block": "nearest", "inline": "nearest"})
	}
}

func (v *boardView) card(id int64, sel bool) js.Value {
	a := v.app
	r := a.r
	t := r.Tasks[id]
	cls := "card bcard"
	if sel {
		cls += " sel"
	}
	if a.pendCells[cellKey{core.EntTask, id, "status"}] > 0 {
		cls += " pending"
	}
	if _, ok := a.flash[core.RowKey{Entity: core.EntTask, ID: id}]; ok {
		cls += " flash"
	}
	c := E("div", cls)
	Data(c, "card", strconv.FormatInt(id, 10))
	Attr(c, "draggable", "true")
	top := E("div", "ctop")
	Add(top, ET("span", "code", r.ProjectCode(t.ProjectID)), ET("span", "muted", fmt.Sprintf("#%d", t.Seq)))
	if t.Priority == core.PriorityCritical || t.Priority == core.PriorityHigh {
		Add(top, ET("span", "pri pri-"+string(t.Priority), ""))
	}
	Add(top, E("span", "spacer"))
	if peers := a.peersOn(core.EntTask, id); len(peers) > 0 {
		if u := r.Users[peers[0].UserID]; u != nil {
			pv := ET("span", "av peer", u.Initials())
			Style(pv, "background", userColor(u.ID))
			Attr(pv, "title", u.DisplayName+" is looking at this")
			Add(top, pv)
		}
	}
	if t.AssigneeID != nil {
		if u := r.Users[*t.AssigneeID]; u != nil {
			av := ET("span", "av", u.Initials())
			Style(av, "background", userColor(u.ID))
			Attr(av, "title", u.DisplayName)
			Add(top, av)
		}
	}
	Add(c, top, ET("div", "ctitle", t.Title))
	foot := E("div", "cfoot")
	if t.DueDate != nil {
		dcls := "muted"
		if r.TaskOverdue(t) {
			dcls = "c-err"
		} else if d, ok := core.ParseISO(*t.DueDate); ok && int(d)-int(r.Today) <= 7 && t.Status != core.TaskDone {
			dcls = "c-warn"
		}
		if d, ok := core.ParseISO(*t.DueDate); ok {
			Add(foot, ET("span", dcls, "⏱ "+d.Short(r.Today.Year())))
		}
	}
	if t.EstHours != nil && *t.EstHours > 0 {
		pct := t.ActualHours * 100 / *t.EstHours
		bar := E("span", "bar")
		fill := E("span", "fill")
		if pct > 100 {
			fill.Set("className", "fill l2")
		}
		Style(fill, "width", fmt.Sprintf("%.0f%%", min(pct, 100)))
		Add(bar, fill)
		Add(foot, E("span", "spacer"), bar, ET("span", "muted", fmt.Sprintf("%s/%s h", core.FmtHours(t.ActualHours), core.FmtHours(*t.EstHours))))
	}
	Add(c, foot)
	return c
}

// moveCard changes status (and, across swimlanes, the assignee) in one mutate.
func (v *boardView) moveCard(id int64, col, lane int) {
	a := v.app
	t := a.r.Tasks[id]
	if t == nil || col < 0 || col >= len(statusOrder) {
		return
	}
	var ops []core.Op
	if ns := statusOrder[col]; ns != t.Status {
		ops = append(ops, core.SetOp(core.EntTask, id, "status", string(t.Status), string(ns)))
	}
	if v.swim && lane >= 0 && lane < len(v.lanes) {
		l := v.lanes[lane]
		var cur, nu any
		if t.AssigneeID != nil {
			cur = *t.AssigneeID
		}
		if l.user != nil {
			nu = *l.user
		}
		if !core.ValuesEqual(cur, nu) {
			ops = append(ops, core.SetOp(core.EntTask, id, "assignee_id", cur, nu))
		}
	}
	if len(ops) == 0 {
		return
	}
	v.selID = id
	a.Mutate(ops, "move card", nil)
	a.setStatus("info", fmt.Sprintf("Moving %s to %s", a.r.TaskRef(t), statusOrder[col].Label()))
}

func (v *boardView) Key(k Key) bool {
	if v.prog.focused || v.proj.focused {
		ed := v.prog
		if v.proj.focused {
			ed = v.proj
		}
		if ed.Key(k) {
			return true
		}
		if k.Is("Escape", "ArrowDown") && !k.InInput || k.Is("Escape") {
			v.FocusDOM()
			return true
		}
		return false
	}
	if !v.boardEl.Call("contains", doc.Get("activeElement")).Bool() && !v.boardEl.Equal(doc.Get("activeElement")) {
		return false
	}
	cells := func() []int64 { return v.lanes[v.lane].cells[v.col] }
	switch {
	case k.Is("S-ArrowLeft", "<"):
		if v.selID != 0 && v.col > 0 {
			v.moveCard(v.selID, v.col-1, v.lane)
		}
	case k.Is("S-ArrowRight", ">"):
		if v.selID != 0 && v.col < 4 {
			v.moveCard(v.selID, v.col+1, v.lane)
		}
	case k.Is("ArrowLeft"):
		v.col = max(0, v.col-1)
		v.idx = min(v.idx, max(0, len(cells())-1))
	case k.Is("ArrowRight"):
		v.col = min(4, v.col+1)
		v.idx = min(v.idx, max(0, len(cells())-1))
	case k.Is("ArrowDown"):
		if v.idx+1 < min(len(cells()), boardCap) {
			v.idx++
		} else if v.lane+1 < len(v.lanes) {
			v.lane++
			v.idx = 0
		}
	case k.Is("ArrowUp"):
		if v.idx > 0 {
			v.idx--
		} else if v.lane > 0 {
			v.lane--
			v.idx = max(0, len(cells())-1)
		}
	case k.Is("Home"):
		v.idx = 0
	case k.Is("End"):
		v.idx = max(0, len(cells())-1)
	case k.Is("Enter"):
		if v.selID != 0 {
			v.app.editRecordOf(core.EntTask, v.selID)
		}
	case k.Is("C-r"):
		v.toggleSwim()
	default:
		return false
	}
	v.clampSel()
	v.app.sendFocus(false)
	v.sig = ""
	return true
}

func (v *boardView) New() {
	var pid *int64
	if id, ok := core.AsInt(v.proj.value); ok {
		pid = &id
	} else if t := v.app.r.Tasks[v.selID]; t != nil {
		p := t.ProjectID
		pid = &p
	}
	v.app.OpenDialog(newTaskDialog(v.app, pid, string(statusOrder[v.col])))
}

func (a *App) editRecordOf(e core.Entity, id int64) {
	if id != 0 && a.r.Exists(e, id) {
		a.OpenDialog(newRecordForm(a, e, id))
	}
}
