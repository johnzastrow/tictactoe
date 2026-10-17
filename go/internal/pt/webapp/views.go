//go:build js && wasm

package webapp

import (
	"fmt"
	"sort"
	"strings"
	"syscall/js"

	"git.wharf.fluidgrid.org/jcz/tictactoe/go/internal/pt/core"
)

type View interface {
	ID() string
	Target() (core.Target, bool)
	Mount(parent js.Value)
	Unmount()
	Render()
	DataChanged(snapshot bool)
	PresenceChanged()
	Key(k Key) bool
	Hints() []hint
	Focus() (core.Entity, int64, string)
	FocusDOM()
	ActiveGrid() *Grid
	Query() string
	SetQuery(q string)
	QueryErrors() []core.QueryError
	Count() (int, int)
	Indicators() string
	NextPane(d int)
	ToggleDetail()
	New()
}

func makeViews(a *App) []View {
	return []View{
		newProjectsView(a), newTasksView(a), newBoardView(a), newTimelineView(a), newDashboardView(a),
		newActivityView(a), newTemplatesView(a), newFieldsView(a), newUsersView(a),
	}
}

// ---- shared pieces -------------------------------------------------------------------

type queryState struct {
	q        string
	compiled *core.Query
	target   core.Target
	dirty    bool
}

func (s *queryState) Query() string { return s.q }

func (s *queryState) setQ(q string) {
	if q != s.q {
		s.q = q
		s.compiled = nil
		s.dirty = true
	}
}

func (s *queryState) compile(r *core.Replica) *core.Query {
	if s.compiled == nil {
		s.compiled = core.CompileQuery(s.q, s.target, r)
	}
	return s.compiled
}

func (s *queryState) QueryErrors() []core.QueryError {
	if s.compiled == nil {
		return nil
	}
	return s.compiled.Errors
}

func gridHints(g *Grid) []hint {
	if g == nil {
		return nil
	}
	if g.Editing() {
		switch g.ed.o.Kind {
		case core.EdChips:
			return []hint{{"Enter", "Add", ""}, {"⌫", "Remove last", ""}, {"Tab", "Save", ""}, {"Esc", "Cancel", ""}}
		case core.EdDate:
			return []hint{{"Enter", "Save", ""}, {"↑↓", "±week", ""}, {"PgUp/Dn", "±month", ""}, {"^←→", "±day", ""}, {"Esc", "Cancel", ""}}
		case core.EdMultiline:
			return []hint{{"^Enter", "Save", ""}, {"Esc", "Cancel", ""}}
		case core.EdSelect, core.EdCombo:
			return []hint{{"type", "Filter", ""}, {"↑↓", "Choose", ""}, {"Enter", "Pick", ""}, {"Tab", "Pick & next", ""}, {"Esc", "Cancel", ""}}
		}
		return []hint{{"Enter", "Save ↓", ""}, {"Tab", "Save →", ""}, {"Esc", "Cancel", ""}}
	}
	return []hint{{"F2", "Edit", "key:F2"}, {"Ins", "New", "new"}, {"^E", "Form", "edit_record"}, {"F8", "Delete", "delete"},
		{"Space", "Mark", ""}, {"^C/^V", "Copy/Paste", ""}, {"^D", "Fill↓", "fill_down"}, {"^O", "Sort", "sort_col"},
		{"^R", "Group", "group_col"}, {"F4", "Columns", "columns"}, {"F6", "Pane", "next_pane"}}
}

func pane(title string, cls string) (js.Value, js.Value, js.Value) {
	p := E("div", "pane "+cls)
	t := E("div", "ptitle")
	tt := ET("span", "", title)
	cnt := E("span", "pcount")
	Add(t, tt, cnt)
	Add(p, t)
	return p, tt, cnt
}

// ---- 1 Projects ------------------------------------------------------------------------

type projectsView struct {
	app         *App
	qs          queryState
	main, tasks *Grid
	focus       int // 0 main, 1 detail
	detail      bool
	root        js.Value
	detailPane  js.Value
	dTitle      js.Value
	mCount      js.Value
	dCount      js.Value
	lastProject int64
}

func newProjectsView(a *App) *projectsView {
	v := &projectsView{app: a, detail: storageGet("pt.detail.projects") != "off"}
	v.qs.target = core.TargetProjects
	v.main = NewGrid(a, GridOpts{ID: "projects", Entity: core.EntProject, Frozen: 2,
		Cols:        func() []core.Column { return core.ProjectColumns(a.r) },
		DefaultSort: []core.SortSpec{{Key: "code"}},
		OnCursor:    func() { v.syncDetail() },
		OnActivate:  func(id int64) bool { a.editRecord(); return true },
	})
	v.tasks = NewGrid(a, GridOpts{ID: "ptasks", Entity: core.EntTask, Frozen: 2,
		Cols:        func() []core.Column { return core.TaskColumns(a.r, false) },
		DefaultSort: []core.SortSpec{{Key: "seq"}},
		ProjectHint: func() *int64 { return v.currentProject() },
	})
	return v
}

func (v *projectsView) ID() string                        { return "projects" }
func (v *projectsView) Target() (core.Target, bool)        { return core.TargetProjects, true }
func (v *projectsView) Query() string                      { return v.qs.q }
func (v *projectsView) QueryErrors() []core.QueryError     { return v.qs.QueryErrors() }
func (v *projectsView) SetQuery(q string)                  { v.qs.setQ(q) }
func (v *projectsView) DataChanged(bool)                   { v.qs.dirty = true; v.main.Invalidate(); v.tasks.Invalidate() }
func (v *projectsView) PresenceChanged()                   {}
func (v *projectsView) Count() (int, int)                  { return len(v.main.ids), len(v.app.r.Projects) }
func (v *projectsView) Indicators() string                 { return v.ActiveGrid().Indicators() }
func (v *projectsView) currentProject() *int64             { id := v.main.CurrentID(); if id == 0 { return nil }; return &id }

func (v *projectsView) ActiveGrid() *Grid {
	if v.focus == 1 && v.detail {
		return v.tasks
	}
	return v.main
}

func (v *projectsView) Mount(parent js.Value) {
	v.root = E("div", "split")
	mp, _, mc := pane("Projects", "main")
	v.mCount = mc
	v.main.Mount(mp)
	dp, dt, dc := pane("Tasks", "detail")
	v.detailPane, v.dTitle, v.dCount = dp, dt, dc
	v.tasks.Mount(dp)
	Add(v.root, mp, dp)
	Add(parent, v.root)
	v.qs.dirty = true
	v.lastProject = -1
	// clicking a pane makes it the active one
	permanentScope(v.root, "focusin", func(ev js.Value) {
		if v.detailPane.Call("contains", ev.Get("target")).Bool() {
			v.focus = 1
		} else {
			v.focus = 0
		}
		v.app.Invalidate()
	})
}

func (v *projectsView) Unmount() { v.main.Unmount(); v.tasks.Unmount() }

func (v *projectsView) refilter() {
	r := v.app.r
	q := v.qs.compile(r)
	var ids []int64
	for _, id := range core.SortedIDs(r.Projects) {
		if q.MatchesProject(r.Projects[id], r) {
			ids = append(ids, id)
		}
	}
	v.main.SetIDs(ids)
	v.qs.dirty = false
	v.lastProject = -1
}

func (v *projectsView) syncDetail() {
	pid := v.main.CurrentID()
	if pid == v.lastProject && !v.tasks.rowsDirty {
		return
	}
	v.lastProject = pid
	v.tasks.SetIDs(append([]int64(nil), v.app.r.ProjectTasks(pid)...))
}

func (v *projectsView) Render() {
	if v.qs.dirty {
		v.refilter()
	}
	v.main.Render()
	if v.tasks.rowsDirty {
		v.lastProject = -1
	}
	v.syncDetail()
	if v.detail {
		v.detailPane.Get("classList").Call("remove", "hidden")
		v.tasks.Render()
		title := "Tasks"
		if p := v.app.r.Projects[v.main.CurrentID()]; p != nil {
			title = "Tasks of " + p.Code + " · " + p.Name
		}
		v.dTitle.Set("textContent", title)
		v.dCount.Set("textContent", fmt.Sprintf("%d tasks", len(v.tasks.ids)))
	} else {
		v.detailPane.Get("classList").Call("add", "hidden")
	}
	v.mCount.Set("textContent", fmt.Sprintf("%d projects", len(v.main.ids)))
}

func (v *projectsView) Key(k Key) bool { return v.ActiveGrid().Key(k) }

func (v *projectsView) Hints() []hint { return gridHints(v.ActiveGrid()) }

func (v *projectsView) Focus() (core.Entity, int64, string) {
	g := v.ActiveGrid()
	f := ""
	if c := g.CurrentCol(); c != nil {
		f = c.Field
	}
	return g.opts.Entity, g.CurrentID(), f
}

func (v *projectsView) FocusDOM() { v.ActiveGrid().Focus() }

func (v *projectsView) NextPane(int) {
	if !v.detail {
		return
	}
	v.focus = 1 - v.focus
	v.ActiveGrid().Focus()
}

func (v *projectsView) ToggleDetail() {
	v.detail = !v.detail
	if v.detail {
		storageSet("pt.detail.projects", "on")
	} else {
		storageSet("pt.detail.projects", "off")
		v.focus = 0
	}
	v.app.Invalidate()
}

func (v *projectsView) New() {
	if v.focus == 1 && v.detail {
		v.app.OpenDialog(newTaskDialog(v.app, v.currentProject(), ""))
		return
	}
	v.app.OpenDialog(newProjectDialog(v.app))
}

// permanentScope attaches a listener that lives as long as the element.
var viewScope Scope

func permanentScope(e js.Value, ev string, fn func(js.Value)) { viewScope.On(e, ev, fn) }

// ---- 2 Tasks -----------------------------------------------------------------------------

type tasksView struct {
	app    *App
	qs     queryState
	grid   *Grid
	detail bool
	root   js.Value
	card   js.Value
	cCount js.Value
	cardID int64
	cardV  uint64
}

func newTasksView(a *App) *tasksView {
	v := &tasksView{app: a, detail: storageGet("pt.detail.tasks") != "off"}
	v.qs.target = core.TargetTasks
	v.grid = NewGrid(a, GridOpts{ID: "tasks", Entity: core.EntTask, Frozen: 3,
		Cols:         func() []core.Column { return core.TaskColumns(a.r, true) },
		DefaultSort:  []core.SortSpec{{Key: "project"}, {Key: "seq"}},
		DefaultGroup: "project",
		OnActivate:   func(int64) bool { a.editRecord(); return true },
	})
	return v
}

func (v *tasksView) ID() string                    { return "tasks" }
func (v *tasksView) Target() (core.Target, bool)    { return core.TargetTasks, true }
func (v *tasksView) Query() string                  { return v.qs.q }
func (v *tasksView) QueryErrors() []core.QueryError { return v.qs.QueryErrors() }
func (v *tasksView) SetQuery(q string)              { v.qs.setQ(q) }
func (v *tasksView) DataChanged(bool)               { v.qs.dirty = true; v.grid.Invalidate(); v.cardV = 0 }
func (v *tasksView) PresenceChanged()               {}
func (v *tasksView) Count() (int, int)              { return len(v.grid.ids), len(v.app.r.Tasks) }
func (v *tasksView) Indicators() string             { return v.grid.Indicators() }
func (v *tasksView) ActiveGrid() *Grid              { return v.grid }
func (v *tasksView) Key(k Key) bool                 { return v.grid.Key(k) }
func (v *tasksView) Hints() []hint                  { return gridHints(v.grid) }
func (v *tasksView) FocusDOM()                      { v.grid.Focus() }
func (v *tasksView) NextPane(int)                   { v.grid.Focus() }
func (v *tasksView) Unmount()                       { v.grid.Unmount() }

func (v *tasksView) Focus() (core.Entity, int64, string) {
	f := ""
	if c := v.grid.CurrentCol(); c != nil {
		f = c.Field
	}
	return core.EntTask, v.grid.CurrentID(), f
}

func (v *tasksView) Mount(parent js.Value) {
	v.root = E("div", "split row")
	mp, _, cnt := pane("Tasks", "main")
	v.cCount = cnt
	v.grid.Mount(mp)
	v.card = E("div", "pane card-pane")
	Add(v.root, mp, v.card)
	Add(parent, v.root)
	v.qs.dirty = true
	v.cardV = 0
}

func (v *tasksView) Render() {
	if v.qs.dirty {
		r := v.app.r
		q := v.qs.compile(r)
		var ids []int64
		for _, id := range core.SortedIDs(r.Tasks) {
			if q.MatchesTask(r.Tasks[id], r) {
				ids = append(ids, id)
			}
		}
		v.grid.SetIDs(ids)
		v.qs.dirty = false
	}
	v.grid.Render()
	v.cCount.Set("textContent", fmt.Sprintf("%d tasks", len(v.grid.ids)))
	if !v.detail {
		v.card.Get("classList").Call("add", "hidden")
		return
	}
	v.card.Get("classList").Call("remove", "hidden")
	id := v.grid.CurrentID()
	if id == v.cardID && v.cardV == v.app.r.Version {
		return
	}
	v.cardID, v.cardV = id, v.app.r.Version
	renderTaskCard(v.app, v.card, id)
}

func (v *tasksView) ToggleDetail() {
	v.detail = !v.detail
	if v.detail {
		storageSet("pt.detail.tasks", "on")
	} else {
		storageSet("pt.detail.tasks", "off")
	}
	v.cardV = 0
}

func (v *tasksView) New() {
	var pid *int64
	if t := v.app.r.Tasks[v.grid.CurrentID()]; t != nil {
		p := t.ProjectID
		pid = &p
	}
	v.app.OpenDialog(newTaskDialog(v.app, pid, ""))
}

// renderTaskCard: the read-only record card beside the Tasks grid.
func renderTaskCard(a *App, el js.Value, id int64) {
	Clear(el)
	r := a.r
	t := r.Tasks[id]
	if t == nil {
		Add(el, ET("div", "card empty muted", "Select a task to see its details."))
		return
	}
	c := E("div", "card")
	h := E("div", "card-h")
	Add(h, ET("span", "code", r.TaskRef(t)), ET("span", "chip ts-"+string(t.Status), t.Status.Label()))
	if r.TaskOverdue(t) {
		Add(h, ET("span", "chip ts-blocked", "overdue"))
	}
	Add(c, h, ET("h3", "", t.Title))
	if p := r.Projects[t.ProjectID]; p != nil {
		Add(c, ET("div", "muted", p.Name+" · "+r.ProgramName(p.ProgramID)))
	}
	kv := E("dl", "kv")
	row := func(k, v string) {
		if v == "" {
			v = "—"
		}
		Add(kv, ET("dt", "", k), ET("dd", "", v))
	}
	row("Phase", t.Phase)
	row("Priority", t.Priority.Label())
	row("Assignee", r.UserName(t.AssigneeID))
	row("Start", deref(t.StartDate))
	row("Due", deref(t.DueDate))
	est := "—"
	if t.EstHours != nil {
		est = core.FmtHours(*t.EstHours) + " h"
	}
	row("Estimate", est)
	row("Actual", core.FmtHours(t.ActualHours)+" h")
	if len(t.Tags) > 0 {
		row("Tags", strings.Join(t.Tags, " "))
	}
	for _, cf := range r.CustomFieldsFor(core.CfTask) {
		if v, ok := t.CF[fmt.Sprint(cf.ID)]; ok && v != nil {
			row(cf.Name, core.DisplayValue(r, core.EntTask, cf.Key(), v))
		}
	}
	Add(c, kv)
	if t.EstHours != nil && *t.EstHours > 0 {
		pct := t.ActualHours * 100 / *t.EstHours
		bar := E("div", "bar wide")
		fill := E("span", "fill")
		lvl := ""
		if pct > 100 {
			lvl = " l2"
		} else if pct > 90 {
			lvl = " l1"
		}
		fill.Set("className", "fill"+lvl)
		Style(fill, "width", fmt.Sprintf("%.0f%%", min(pct, 100)))
		Add(bar, fill)
		Add(c, ET("div", "muted", fmt.Sprintf("Hours used: %.0f%% of estimate", pct)), bar)
	}
	if len(t.Deps) > 0 {
		Add(c, ET("h4", "", "Depends on"))
		for _, d := range t.Deps {
			if dt := r.Tasks[d]; dt != nil {
				line := E("div", "depline")
				Add(line, ET("span", "chip ts-"+string(dt.Status), dt.Status.Label()), Txt(fmt.Sprintf(" #%d %s", dt.Seq, dt.Title)))
				Add(c, line)
			}
		}
	}
	var blocking []string
	for _, oid := range r.ProjectTasks(t.ProjectID) {
		if ot := r.Tasks[oid]; ot != nil {
			for _, d := range ot.Deps {
				if d == t.ID {
					blocking = append(blocking, fmt.Sprintf("#%d %s", ot.Seq, ot.Title))
				}
			}
		}
	}
	if len(blocking) > 0 {
		Add(c, ET("h4", "", "Blocks"))
		for _, b := range blocking {
			Add(c, ET("div", "depline", b))
		}
	}
	if strings.TrimSpace(t.Notes) != "" {
		Add(c, ET("h4", "", "Notes"), ET("div", "notes", t.Notes))
	}
	acts := recentActivity(r, "task", id, 6)
	if len(acts) > 0 {
		Add(c, ET("h4", "", "Recent changes"))
		for _, act := range acts {
			line := E("div", "actline")
			Add(line, ET("span", "muted", core.RelativeTime(act.TS, nowSecs())+" · "+r.Username(act.UserID)+" "), Txt(act.Label))
			Add(c, line)
		}
	}
	Add(c, ET("div", "card-foot muted", "^E edits every field · ^B hides this card"))
	Add(el, c)
}

func deref(s *string) string {
	if s == nil {
		return ""
	}
	return *s
}

func recentActivity(r *core.Replica, entity string, id int64, n int) []*core.Activity {
	var out []*core.Activity
	ids := core.SortedIDs(r.Activity)
	for i := len(ids) - 1; i >= 0 && len(out) < n; i-- {
		a := r.Activity[ids[i]]
		if a.Entity == entity && a.EntityID == id {
			out = append(out, a)
		}
	}
	return out
}

// ---- 6 Activity ---------------------------------------------------------------------------

type activityView struct {
	app  *App
	qs   queryState
	grid *Grid
	cnt  js.Value
}

func newActivityView(a *App) *activityView {
	v := &activityView{app: a}
	v.qs.target = core.TargetActivity
	v.grid = NewGrid(a, GridOpts{ID: "activity", Entity: core.EntUser, Activity: true,
		Cols: core.ActivityColumns,
		OnActivate: func(id int64) bool {
			if act := a.r.Activity[id]; act != nil {
				if e, ok := core.ParseEntity(act.Entity); ok {
					a.jumpTo(e, act.EntityID)
				}
			}
			return true
		},
	})
	return v
}

func (v *activityView) ID() string                    { return "activity" }
func (v *activityView) Target() (core.Target, bool)    { return core.TargetActivity, true }
func (v *activityView) Query() string                  { return v.qs.q }
func (v *activityView) QueryErrors() []core.QueryError { return v.qs.QueryErrors() }
func (v *activityView) SetQuery(q string)              { v.qs.setQ(q) }
func (v *activityView) DataChanged(bool)               { v.qs.dirty = true; v.grid.Invalidate() }
func (v *activityView) PresenceChanged()               {}
func (v *activityView) Count() (int, int)              { return len(v.grid.ids), len(v.app.r.Activity) }
func (v *activityView) Indicators() string             { return "newest first" }
func (v *activityView) ActiveGrid() *Grid              { return v.grid }
func (v *activityView) Key(k Key) bool                 { return v.grid.Key(k) }
func (v *activityView) FocusDOM()                      { v.grid.Focus() }
func (v *activityView) NextPane(int)                   {}
func (v *activityView) ToggleDetail()                  {}
func (v *activityView) New()                           {}
func (v *activityView) Unmount()                       { v.grid.Unmount() }
func (v *activityView) Focus() (core.Entity, int64, string) { return "", 0, "" }

func (v *activityView) Hints() []hint {
	return []hint{{"Enter", "Go to record", ""}, {"^Z", "Undo yours", "undo"}, {"^Y", "Redo", "redo"}, {"@name", "Filter by person", ""}}
}

func (v *activityView) Mount(parent js.Value) {
	p, _, cnt := pane("Activity — live", "full")
	v.cnt = cnt
	v.grid.Mount(p)
	Add(parent, p)
	v.qs.dirty = true
}

func (v *activityView) Render() {
	if v.qs.dirty {
		r := v.app.r
		q := v.qs.compile(r)
		all := core.SortedIDs(r.Activity)
		var ids []int64
		for i := len(all) - 1; i >= 0; i-- {
			if q.MatchesActivity(r.Activity[all[i]], r) {
				ids = append(ids, all[i])
			}
		}
		v.grid.SetIDs(ids)
		v.qs.dirty = false
	}
	v.grid.Render()
	v.cnt.Set("textContent", fmt.Sprintf("%d changes", len(v.grid.ids)))
}

// ---- 7 Templates / 8 Fields / 9 Users: simple master(-detail) grids ---------------------------

type simpleView struct {
	app    *App
	id     string
	title  string
	grids  []*Grid
	titles []string
	ids    []func() []int64
	focus  int
	extra  func(parent js.Value) // optional extra content (template preview)
	render func()
	newFn  []func()
	root   js.Value
	cnts   []js.Value
	panes  []js.Value
	lock   js.Value
}

func (v *simpleView) ID() string                    { return v.id }
func (v *simpleView) Target() (core.Target, bool)    { return 0, false }
func (v *simpleView) Query() string                  { return "" }
func (v *simpleView) QueryErrors() []core.QueryError { return nil }
func (v *simpleView) SetQuery(string)                {}
func (v *simpleView) PresenceChanged()               {}
func (v *simpleView) ActiveGrid() *Grid              { return v.grids[v.focus] }
func (v *simpleView) Key(k Key) bool                 { return v.ActiveGrid().Key(k) }
func (v *simpleView) FocusDOM()                      { v.ActiveGrid().Focus() }
func (v *simpleView) ToggleDetail()                  {}
func (v *simpleView) Indicators() string             { return v.ActiveGrid().Indicators() }
func (v *simpleView) Count() (int, int)              { return len(v.grids[0].ids), len(v.grids[0].ids) }

func (v *simpleView) DataChanged(bool) {
	for _, g := range v.grids {
		g.Invalidate()
		g.ColumnsChanged()
	}
}

func (v *simpleView) Hints() []hint {
	h := gridHints(v.ActiveGrid())
	if !v.app.r.IsAdmin() && !v.ActiveGrid().Editing() {
		h = append([]hint{{"🔒", "Admins edit here", ""}}, h...)
	}
	return h
}

func (v *simpleView) Focus() (core.Entity, int64, string) {
	g := v.ActiveGrid()
	f := ""
	if c := g.CurrentCol(); c != nil {
		f = c.Field
	}
	return g.opts.Entity, g.CurrentID(), f
}

func (v *simpleView) NextPane(d int) {
	v.focus = (v.focus + d + len(v.grids)) % len(v.grids)
	v.ActiveGrid().Focus()
}

func (v *simpleView) New() {
	if !v.app.r.IsAdmin() {
		v.app.toast("warn", "Admins only", "Ask an admin to make this change.")
		return
	}
	if v.focus < len(v.newFn) && v.newFn[v.focus] != nil {
		v.newFn[v.focus]()
	}
}

func (v *simpleView) Mount(parent js.Value) {
	cls := "split"
	if v.id == "templates" {
		cls = "split row"
	}
	v.root = E("div", cls)
	v.cnts = nil
	v.panes = nil
	for i, g := range v.grids {
		c := "main"
		if i > 0 {
			c = "detail"
		}
		if v.id == "templates" && i == 0 {
			c = "narrow"
		}
		p, tt, cnt := pane(v.titles[i], c)
		if !v.app.r.IsAdmin() {
			Add(tt, ET("span", "lock", "  🔒 read-only (admins edit)"))
		}
		v.cnts = append(v.cnts, cnt)
		v.panes = append(v.panes, p)
		g.Mount(p)
		if v.extra != nil && i == len(v.grids)-1 {
			v.extra(p)
		}
		Add(v.root, p)
	}
	Add(parent, v.root)
	permanentScope(v.root, "focusin", func(ev js.Value) {
		for i, p := range v.panes {
			if p.Call("contains", ev.Get("target")).Bool() {
				v.focus = i
			}
		}
		v.app.Invalidate()
	})
}

func (v *simpleView) Unmount() {
	for _, g := range v.grids {
		g.Unmount()
	}
}

func (v *simpleView) Render() {
	for i, g := range v.grids {
		if g.rowsDirty {
			g.SetIDs(v.ids[i]())
		}
		g.Render()
		v.cnts[i].Set("textContent", fmt.Sprintf("%d", len(g.ids)))
	}
	if v.render != nil {
		v.render()
	}
}

func adminOnly(a *App) func() bool { return func() bool { return !a.r.IsAdmin() } }

func newTemplatesView(a *App) *simpleView {
	v := &simpleView{app: a, id: "templates", titles: []string{"Templates", "Template tasks"}}
	tg := NewGrid(a, GridOpts{ID: "templates", Entity: core.EntTemplate, Cols: core.TemplateColumns,
		DefaultSort: []core.SortSpec{{Key: "name"}}, ReadOnly: adminOnly(a)})
	var preview js.Value
	var previewSig string
	tt := NewGrid(a, GridOpts{ID: "ttasks", Entity: core.EntTemplateTask, Cols: core.TemplateTaskColumns,
		DefaultSort: []core.SortSpec{{Key: "seq"}}, ReadOnly: adminOnly(a)})
	lastT := int64(-1)
	tg.opts.OnCursor = func() { tt.Invalidate() }
	v.grids = []*Grid{tg, tt}
	v.ids = []func() []int64{
		func() []int64 { return core.SortedIDs(a.r.Templates) },
		func() []int64 {
			var ids []int64
			for _, t := range a.r.TemplateTasksOf(tg.CurrentID()) {
				ids = append(ids, t.ID)
			}
			return ids
		},
	}
	v.extra = func(p js.Value) {
		preview = E("div", "minigantt")
		Add(p, preview)
		previewSig = ""
	}
	v.render = func() {
		tid := tg.CurrentID()
		if tid != lastT {
			lastT = tid
			tt.Invalidate()
		}
		sig := fmt.Sprint(tid, a.r.Version, a.theme)
		if sig == previewSig || !preview.Truthy() {
			return
		}
		previewSig = sig
		renderMiniGantt(a, preview, tid)
		if t := a.r.Templates[tid]; t != nil {
			n, h, span := a.r.TemplateStats(tid)
			v.cnts[1].Set("textContent", fmt.Sprintf("%s · %d tasks · %s h · %d days", t.Name, n, core.FmtHours(h), span))
		}
	}
	v.newFn = []func(){
		func() {
			a.OpenDialog(newPromptDialog(a, "New template", "Name", "", func(name string) {
				a.Mutate([]core.Op{core.CreateOp(core.EntTemplate, "t", map[string]any{"name": name})}, "", nil)
			}))
		},
		func() {
			tid := tg.CurrentID()
			if tid == 0 {
				return
			}
			a.OpenDialog(newPromptDialog(a, "New template task", "Title", "", func(title string) {
				a.Mutate([]core.Op{core.CreateOp(core.EntTemplateTask, "t", map[string]any{"template_id": tid, "title": title})}, "", nil)
			}))
		},
	}
	return v
}

// renderMiniGantt draws a template's tasks as bars by offset/duration.
func renderMiniGantt(a *App, el js.Value, tid int64) {
	Clear(el)
	tts := a.r.TemplateTasksOf(tid)
	if len(tts) == 0 {
		return
	}
	span := int64(1)
	for _, t := range tts {
		span = max(span, t.OffsetDays+t.DurationDays)
	}
	const rowH, labelW, w = 16.0, 170.0, 560.0
	h := float64(len(tts))*rowH + 22
	svg := S("svg", "width", fmt.Sprint(labelW+w+10), "height", fmt.Sprint(h), "role", "img", "aria-label", "template timeline")
	scale := w / float64(span)
	for d := int64(0); d <= span; d += 7 {
		x := labelW + float64(d)*scale
		Add(svg, S("line", "x1", fmt.Sprint(x), "x2", fmt.Sprint(x), "y1", "14", "y2", fmt.Sprint(h), "class", "gl"))
		t := S("text", "x", fmt.Sprint(x+2), "y", "10", "class", "gt")
		t.Set("textContent", fmt.Sprintf("d%d", d))
		Add(svg, t)
	}
	phases := map[string]int{}
	for i, t := range tts {
		if _, ok := phases[t.Phase]; !ok {
			phases[t.Phase] = len(phases)
		}
		y := 18 + float64(i)*rowH
		lbl := S("text", "x", "4", "y", fmt.Sprint(y+10), "class", "gl-t")
		lbl.Set("textContent", truncate(t.Title, 26))
		Add(svg, lbl)
		bar := S("rect", "x", fmt.Sprint(labelW+float64(t.OffsetDays)*scale), "y", fmt.Sprint(y+2),
			"width", fmt.Sprint(max(3, float64(t.DurationDays)*scale)), "height", fmt.Sprint(rowH-5), "rx", "2",
			"class", fmt.Sprintf("ph%d", phases[t.Phase]%8))
		title := S("title")
		title.Set("textContent", fmt.Sprintf("%s · %s · day %d–%d", t.Title, t.Phase, t.OffsetDays, t.OffsetDays+t.DurationDays))
		Add(bar, title)
		Add(svg, bar)
	}
	Add(el, svg)
}

func truncate(s string, n int) string {
	r := []rune(s)
	if len(r) <= n {
		return s
	}
	return string(r[:n-1]) + "…"
}

func newFieldsView(a *App) *simpleView {
	v := &simpleView{app: a, id: "fields", titles: []string{"Custom fields"}}
	g := NewGrid(a, GridOpts{ID: "fields", Entity: core.EntCustomField, Cols: core.CustomFieldColumns,
		DefaultSort: []core.SortSpec{{Key: "entity"}, {Key: "seq"}}, ReadOnly: adminOnly(a)})
	v.grids = []*Grid{g}
	v.ids = []func() []int64{func() []int64 { return core.SortedIDs(a.r.CustomFields) }}
	v.newFn = []func(){func() { a.OpenDialog(newFieldDialog(a)) }}
	return v
}

func newUsersView(a *App) *simpleView {
	v := &simpleView{app: a, id: "users", titles: []string{"Users", "Programs"}}
	ug := NewGrid(a, GridOpts{ID: "users", Entity: core.EntUser, Cols: core.UserColumns,
		DefaultSort: []core.SortSpec{{Key: "username"}}, ReadOnly: adminOnly(a)})
	pg := NewGrid(a, GridOpts{ID: "programs", Entity: core.EntProgram, Cols: core.ProgramColumns,
		DefaultSort: []core.SortSpec{{Key: "name"}}, ReadOnly: adminOnly(a)})
	v.grids = []*Grid{ug, pg}
	v.ids = []func() []int64{
		func() []int64 {
			ids := core.SortedIDs(a.r.Users)
			sort.Slice(ids, func(i, j int) bool { return a.r.Users[ids[i]].Username < a.r.Users[ids[j]].Username })
			return ids
		},
		func() []int64 { return core.SortedIDs(a.r.Programs) },
	}
	v.newFn = []func(){
		func() { a.OpenDialog(newUserDialog(a)) },
		func() { a.OpenDialog(newProgramDialog(a)) },
	}
	return v
}

// ---- navigation helpers ------------------------------------------------------------------------

// jumpTo shows a record in its home view.
func (a *App) jumpTo(e core.Entity, id int64) {
	switch e {
	case core.EntProject:
		v := a.views[0].(*projectsView)
		v.SetQuery("")
		a.SwitchView(0)
		a.ensureMounted()
		v.refilter()
		v.focus = 0
		v.main.SelectID(id)
	case core.EntTask:
		v := a.views[1].(*tasksView)
		v.SetQuery("")
		a.SwitchView(1)
		a.ensureMounted()
		v.Render()
		v.grid.SelectID(id)
	default:
		return
	}
	a.syncQueryInput()
	a.Invalidate()
}

// SelectID moves the cursor to a data row, expanding its group if needed.
func (g *Grid) SelectID(id int64) {
	if g.colsDirty {
		g.computeCols()
		g.colsDirty = false
	}
	if g.rowsDirty {
		g.rebuildRows()
	}
	if g.group != "" {
		if col := findColumn(g.all, g.group); col != nil {
			key := g.cell(col, id).Sort.GroupKey()
			if g.collapsed[key] {
				delete(g.collapsed, key)
				g.rebuildRows()
			}
		}
	}
	for i, r := range g.rows {
		if !r.IsGroup && r.ID == id {
			g.moveTo(i, g.curCol)
			break
		}
	}
	After(40, func() { g.Focus(); g.app.Invalidate() })
}

func findColumn(cols []core.Column, key string) *core.Column {
	for i := range cols {
		if cols[i].Key == key {
			return &cols[i]
		}
	}
	return nil
}

// contextProject: the project the user is looking at, for New task.
func (a *App) contextProject() *int64 {
	e, id, _ := a.curView().Focus()
	switch e {
	case core.EntProject:
		if id != 0 {
			return &id
		}
	case core.EntTask:
		if t := a.r.Tasks[id]; t != nil {
			p := t.ProjectID
			return &p
		}
	}
	return nil
}

func (a *App) editRecord() {
	g := a.curView().ActiveGrid()
	var e core.Entity
	var id int64
	if g != nil {
		e, id = g.opts.Entity, g.CurrentID()
		if g.opts.Activity {
			return
		}
	} else {
		e, id, _ = a.curView().Focus()
	}
	if id == 0 || !a.r.Exists(e, id) {
		return
	}
	a.OpenDialog(newRecordForm(a, e, id))
}
