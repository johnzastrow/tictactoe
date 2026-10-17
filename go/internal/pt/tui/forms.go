package tui

// Concrete dialogs: login, cascading New project / New task, the record
// form, small admin forms, menus, the context menu and the command palette.

import (
	"context"
	"fmt"
	"sort"
	"strings"
	"time"

	tea "charm.land/bubbletea/v2"

	"git.wharf.fluidgrid.org/jcz/tictactoe/go/internal/pt/client"
	"git.wharf.fluidgrid.org/jcz/tictactoe/go/internal/pt/core"
)

// ---- login ----

func newLoginForm(a *App) *form {
	f := newForm("Log in to pt", 60)
	f.isLogin = true
	f.buttons = []string{"Log in", "Quit"}
	srv := f.add(NewInput(IText, "Server"))
	srv.SetText(a.opts.Server)
	user := f.add(NewInput(IText, "Username"))
	user.SetText(a.opts.User)
	user.Required = true
	pw := f.add(NewInput(IPassword, "Password"))
	pw.Required = true
	rem := f.add(NewInput(IBool, "Remember me"))
	rem.value, rem.hasValue = true, true
	rem.Hint = "keeps a session token in your config folder (mode 0600)"
	if a.opts.User != "" {
		f.focus = 2
	} else {
		f.focus = 1
	}
	f.onSave = func(a *App, f *form) tea.Cmd {
		server := strings.TrimSpace(srv.Text())
		if server != a.opts.Server || a.cl == nil {
			cl, err := client.New(server, a.opts.CACert)
			if err != nil {
				srv.Err = err.Error()
				return nil
			}
			a.cl = cl
			a.opts.Server = server
		}
		f.busy = true
		a.loginErr = ""
		cl, u, p := a.cl, strings.TrimSpace(user.Text()), pw.Text()
		return func() tea.Msg {
			ctx, cancel := context.WithTimeout(context.Background(), 15*time.Second)
			defer cancel()
			res, err := cl.Login(ctx, u, p, clientID)
			return loginMsg{res, err}
		}
	}
	return f
}

func (f *form) remember() bool {
	if in := f.field("Remember me"); in != nil {
		b, _ := in.value.(bool)
		return b
	}
	return false
}

// ---- new project (cascading: program → code, template → due/budget) ----

func (a *App) newProjectDialog() {
	r := a.rep
	f := newForm("New project", 72)
	prog := f.add(a.fieldInput(core.EntProject, "program_id", "Program", core.EdCombo, 0, nil))
	code := f.add(a.fieldInput(core.EntProject, "code", "Code", core.EdText, 0, nil))
	code.Required = true
	name := f.add(a.fieldInput(core.EntProject, "name", "Name", core.EdText, 0, nil))
	name.Required = true
	tmpl := f.add(a.fieldInput(core.EntProject, "template_id", "Template", core.EdSelect, 0, nil))
	owner := f.add(a.fieldInput(core.EntProject, "owner_id", "Owner", core.EdCombo, 0, nil))
	start := f.add(a.fieldInput(core.EntProject, "start_date", "Start", core.EdDate, 0, nil))
	due := f.add(a.fieldInput(core.EntProject, "due_date", "Due", core.EdDate, 0, nil))
	budget := f.add(a.fieldInput(core.EntProject, "budget_hours", "Budget h", core.EdNumber, 0, nil))

	// sensible defaults from the current context
	var progID any
	if g := a.grids["projects"]; g != nil {
		if p := r.Projects[g.CurID()]; p != nil && p.ProgramID != nil {
			progID = *p.ProgramID
		}
	}
	prog.SetValue(progID, prog.allChoices())
	me := r.MeID()
	owner.SetValue(me, owner.allChoices())
	start.SetText(r.Today.ISO())
	start.parseDate()
	if len(r.Templates) > 0 {
		ids := core.SortedIDs(r.Templates)
		tmpl.SetValue(ids[0], tmpl.allChoices())
	}
	codeEdited, dueEdited, budgetEdited := false, false, false
	refresh := func() {
		if !codeEdited {
			pv, _ := prog.Value()
			code.SetText(r.NextCode(asIDPtr(pv)))
		}
		tv, _ := tmpl.Value()
		tid := asIDPtr(tv)
		n, hours, span := 0, 0.0, int64(0)
		if tid != nil {
			n, hours, span = r.TemplateStats(*tid)
		}
		tmpl.Hint = fmt.Sprintf("%d tasks, %s h, %d days", n, core.FmtHours(hours), span)
		if tid == nil {
			tmpl.Hint = "blank project: no tasks"
		}
		if sd, err := core.ParseNatural(start.Text(), r.Today); err == nil && sd != nil && !dueEdited && span > 0 {
			due.SetText(sd.AddDays(int(span)).ISO())
			due.parseDate()
		}
		if !budgetEdited && hours > 0 {
			budget.SetText(core.FmtHours(hours))
		}
	}
	refresh()
	f.onChange = func(a *App, f *form, i int) {
		switch f.fields[i] {
		case code:
			codeEdited = true
		case due:
			dueEdited = true
		case budget:
			budgetEdited = true
		}
		refresh()
	}
	f.focus = 2
	f.onSave = func(a *App, f *form) tea.Cmd {
		vals := map[string]any{}
		for _, in := range f.fields {
			v, err := a.inputValue(in)
			if err != nil {
				in.Err = err.Error()
				f.setFocus(a, f.idx(in))
				return nil
			}
			if v != nil && v != "" {
				vals[in.Field] = v
			}
		}
		vals["status"] = "planning"
		return a.mutateForm(f, []core.Op{core.CreateOp(core.EntProject, "p", vals)}, nil, "Project created",
			func(a *App, res core.ResultMsg) tea.Cmd {
				if id, ok := res.Created["p"]; ok {
					a.reveal(vProjects, "projects", id)
				}
				return nil
			})
	}
	a.push(f)
}

func asIDPtr(v any) *int64 {
	if f, ok := core.AsFloat(v); ok {
		id := int64(f)
		return &id
	}
	return nil
}

// ---- new task (cascading: program → project → phase / assignee) ----

func (a *App) newTaskDialog(projectID int64, status core.TaskStatus) {
	r := a.rep
	f := newForm("New task", 72)
	prog := f.add(a.fieldInput(core.EntProject, "program_id", "Program", core.EdCombo, 0, nil))
	prog.AllowCreate = false
	prog.Hint = "narrows the project list"
	proj := f.add(a.fieldInput(core.EntTask, "project_id", "Project", core.EdCombo, 0, nil))
	proj.Required = true
	phase := f.add(a.fieldInput(core.EntTask, "phase", "Phase", core.EdSuggest, 0, nil))
	title := f.add(a.fieldInput(core.EntTask, "title", "Title", core.EdText, 0, nil))
	title.Required = true
	assignee := f.add(a.fieldInput(core.EntTask, "assignee_id", "Assignee", core.EdCombo, 0, nil))
	st := f.add(a.fieldInput(core.EntTask, "status", "Status", core.EdSelect, 0, nil))
	start := f.add(a.fieldInput(core.EntTask, "start_date", "Start", core.EdDate, 0, nil))
	f.add(a.fieldInput(core.EntTask, "due_date", "Due", core.EdDate, 0, nil))
	f.add(a.fieldInput(core.EntTask, "est_hours", "Est h", core.EdNumber, 0, nil))

	// project list follows the program
	proj.ChoicesFn = func() []core.Choice {
		all := core.Choices(r, core.EntTask, "project_id", 0, nil)
		pv, _ := prog.Value()
		pid := asIDPtr(pv)
		if pid == nil {
			return all
		}
		var out []core.Choice
		for _, c := range all {
			if p := r.Projects[int64(must(core.AsFloat(c.Value)))]; p != nil && p.ProgramID != nil && *p.ProgramID == *pid {
				out = append(out, c)
			}
		}
		return out
	}
	curProject := func() *int64 {
		v, _ := proj.Value()
		return asIDPtr(v)
	}
	phase.ChoicesFn = func() []core.Choice {
		pid := curProject()
		if pid == nil {
			return nil
		}
		return core.Choices(r, core.EntTask, "phase", 0, pid)
	}
	if p := r.Projects[projectID]; p != nil {
		if p.ProgramID != nil {
			prog.SetValue(*p.ProgramID, prog.allChoices())
		}
		proj.Reload()
		proj.SetValue(projectID, proj.allChoices())
		if p.OwnerID != nil {
			assignee.SetValue(*p.OwnerID, assignee.allChoices())
		}
		f.focus = f.idx(title) // project known: start at the title (Shift+Tab for phase)
	} else {
		f.focus = 0
	}
	if status == "" {
		status = core.TaskTodo
	}
	st.SetValue(string(status), st.allChoices())
	start.SetText(r.Today.ISO())
	start.parseDate()
	f.onChange = func(a *App, f *form, i int) {
		switch f.fields[i] {
		case prog:
			proj.Reload()
			if pid := curProject(); pid != nil {
				if p := r.Projects[*pid]; p != nil {
					pv, _ := prog.Value()
					if pp := asIDPtr(pv); pp != nil && (p.ProgramID == nil || *p.ProgramID != *pp) {
						proj.SetValue(nil, nil)
						proj.SetText("")
					}
				}
			}
			phase.Reload()
		case proj:
			phase.Reload()
			if pid := curProject(); pid != nil {
				if p := r.Projects[*pid]; p != nil && p.OwnerID != nil {
					assignee.SetValue(*p.OwnerID, assignee.allChoices())
				}
				ph := r.PhasesForProject(*pid)
				if len(ph) > 0 {
					phase.Hint = "phases: " + strings.Join(ph, ", ")
				}
			}
		}
	}
	f.onChange(a, f, 1)
	f.onSave = func(a *App, f *form) tea.Cmd {
		vals := map[string]any{}
		for _, in := range f.fields[1:] {
			v, err := a.inputValue(in)
			if err != nil {
				in.Err = err.Error()
				f.setFocus(a, f.idx(in))
				return nil
			}
			if v != nil && v != "" {
				vals[in.Field] = v
			}
		}
		return a.mutateForm(f, []core.Op{core.CreateOp(core.EntTask, "t", vals)}, nil, "Task created",
			func(a *App, res core.ResultMsg) tea.Cmd {
				if id, ok := res.Created["t"]; ok && a.view == vTasks {
					a.reveal(vTasks, "tasks", id)
				}
				return nil
			})
	}
	a.push(f)
}

func must(f float64, _ bool) float64 { return f }

// ---- record form (^E) ----

func (a *App) columnsFor(ent core.Entity) []core.Column {
	switch ent {
	case core.EntProject:
		return core.ProjectColumns(a.rep)
	case core.EntTask:
		return core.TaskColumns(a.rep, false)
	case core.EntTemplate:
		return core.TemplateColumns()
	case core.EntTemplateTask:
		return core.TemplateTaskColumns()
	case core.EntCustomField:
		return core.CustomFieldColumns()
	case core.EntUser:
		return core.UserColumns()
	}
	return core.ProgramColumns()
}

func (a *App) recordForm(ent core.Entity, id int64) {
	r := a.rep
	if !r.Exists(ent, id) {
		return
	}
	f := newForm("Edit "+ent.Label()+" "+r.RowLabel(ent, id), min(96, a.w-4))
	f.ent, f.id = ent, id
	f.bases = map[string]any{}
	f.labelW = 12
	var hint *int64
	if t := r.Tasks[id]; ent == core.EntTask && t != nil {
		pid := t.ProjectID
		hint = &pid
	}
	for _, c := range a.columnsFor(ent) {
		if !c.Editable() {
			continue
		}
		in := a.fieldInput(ent, c.Field, c.Title, c.Editor, id, hint)
		if c.Editor == core.EdMultiline {
			in.Height = 4
			if a.h < 34 {
				in.Height = 3
			}
		}
		v := r.FieldValue(ent, id, c.Field)
		f.bases[c.Field] = v
		a.setInputValue(in, v)
		if !r.CanEdit() {
			in.Hint = "read only"
		}
		f.add(in)
	}
	if !r.CanEdit() {
		f.buttons = []string{"Close"}
	}
	// recent activity on this record
	var acts []*core.Activity
	for _, aid := range core.SortedIDs(r.Activity) {
		ac := r.Activity[aid]
		if ac.Entity == string(ent) && ac.EntityID == id {
			acts = append(acts, ac)
		}
	}
	if len(acts) > 4 {
		acts = acts[len(acts)-4:]
	}
	if len(acts) > 0 {
		f.extraH = len(acts) + 1
		f.extra = func(a *App, f *form, b *Buf, rr Rect) {
			t := a.th()
			b.Text(rr.X, rr.Y, "Recent changes", Sty{FG: t.Accent, BG: t.Panel, A: Bold}, rr.W)
			for i := len(acts) - 1; i >= 0; i-- {
				ac := acts[i]
				line := fmt.Sprintf("%s  %-8s %s", core.RelativeTime(ac.TS, a.now.Unix()), a.rep.Username(ac.UserID), ac.Label)
				st := Sty{FG: t.Muted, BG: t.Panel}
				if ac.State == core.StateUndone {
					st = st.With(Strike)
				}
				b.Text(rr.X+1, rr.Y+len(acts)-i, line, st, rr.W-1)
			}
		}
	}
	// keep the form inside the screen: drop text areas to 2 lines if needed
	for f.layoutH() > a.h-2 && f.extraH > 0 {
		f.extraH, f.extra = 0, nil
	}
	f.onSave = func(a *App, f *form) tea.Cmd {
		if !a.rep.CanEdit() {
			a.closeOverlay(f)
			return nil
		}
		var ops []core.Op
		var cells []cellKey
		force := f.theirs == nil && f.msg != "" // second save after a conflict = save mine
		_ = force
		for _, in := range f.fields {
			v, err := a.inputValue(in)
			if err != nil {
				in.Err = err.Error()
				f.setFocus(a, f.idx(in))
				return nil
			}
			base := f.bases[in.Field]
			if core.ValuesEqual(base, v) {
				continue
			}
			ops = append(ops, core.SetOp(ent, id, in.Field, base, v))
			cells = append(cells, cellKey{ent, id, in.Field})
		}
		if len(ops) == 0 {
			a.closeOverlay(f)
			return nil
		}
		for _, in := range f.fields {
			in.Err = ""
		}
		f.theirs = nil
		return a.mutateForm(f, ops, cells, fmt.Sprintf("Saved %d field(s)", len(ops)), func(a *App, res core.ResultMsg) tea.Cmd {
			// successful fields now have new bases
			for _, op := range ops {
				conflicted := false
				for _, c := range res.Conflicts {
					if c.Field == op.Field {
						conflicted = true
					}
				}
				if !conflicted {
					f.bases[op.Field] = op.Value
				}
			}
			return nil
		})
	}
	a.push(f)
}

// ---- small admin forms ----

func (a *App) simpleCreate(title string, ent core.Entity, fields []struct {
	name, label string
	ed          core.Editor
	req         bool
}, preset map[string]any, after func(a *App, id int64)) {
	f := newForm(title, 64)
	for _, fd := range fields {
		in := a.fieldInput(ent, fd.name, fd.label, fd.ed, 0, nil)
		in.Required = fd.req
		if fd.name == "password" {
			in.Kind = IPassword
		}
		if v, ok := preset[fd.name]; ok {
			a.setInputValue(in, v)
		}
		f.add(in)
	}
	f.onSave = func(a *App, f *form) tea.Cmd {
		vals := map[string]any{}
		for k, v := range preset {
			vals[k] = v
		}
		for _, in := range f.fields {
			v, err := a.inputValue(in)
			if err != nil {
				in.Err = err.Error()
				return nil
			}
			if v != nil && v != "" {
				vals[in.Field] = v
			}
		}
		return a.mutateForm(f, []core.Op{core.CreateOp(ent, "n", vals)}, nil, "Created "+ent.Label(), func(a *App, res core.ResultMsg) tea.Cmd {
			if id, ok := res.Created["n"]; ok && after != nil {
				after(a, id)
			}
			return nil
		})
	}
	a.push(f)
}

type fdef = struct {
	name, label string
	ed          core.Editor
	req         bool
}

func (a *App) newUserDialog() {
	f := newForm("New user", 64)
	un := f.add(a.fieldInput(core.EntUser, "username", "Username", core.EdText, 0, nil))
	un.Required = true
	dn := f.add(a.fieldInput(core.EntUser, "display_name", "Name", core.EdText, 0, nil))
	dn.Required = true
	role := f.add(a.fieldInput(core.EntUser, "role", "Role", core.EdSelect, 0, nil))
	role.SetValue("member", role.allChoices())
	pw := f.add(NewInput(IPassword, "Password"))
	pw.Required = true
	pw.Hint = "8–256 characters"
	pw2 := f.add(NewInput(IPassword, "Confirm"))
	pw2.Required = true
	f.onSave = func(a *App, f *form) tea.Cmd {
		if pw.Text() != pw2.Text() {
			pw2.Err = "passwords don't match"
			return nil
		}
		rv, _ := role.Value()
		vals := map[string]any{"username": strings.TrimSpace(un.Text()), "display_name": strings.TrimSpace(dn.Text()), "role": rv, "password": pw.Text()}
		return a.mutateForm(f, []core.Op{core.CreateOp(core.EntUser, "u", vals)}, nil, "User created", nil)
	}
	a.push(f)
}

func (a *App) setPasswordDialog(uid int64) {
	r := a.rep
	u := r.Users[uid]
	if u == nil {
		return
	}
	f := newForm("Set password — "+u.Username, 64)
	var old *Input
	if !r.IsAdmin() {
		old = f.add(NewInput(IPassword, "Current"))
		old.Required = true
	}
	pw := f.add(NewInput(IPassword, "New password"))
	pw.Required = true
	pw2 := f.add(NewInput(IPassword, "Confirm"))
	pw2.Required = true
	f.onSave = func(a *App, f *form) tea.Cmd {
		if pw.Text() != pw2.Text() {
			pw2.Err = "passwords don't match"
			return nil
		}
		a.req++
		msg := core.SetPasswordMsg{Req: a.req, UserID: uid, Password: pw.Text()}
		if old != nil {
			o := old.Text()
			msg.OldPassword = &o
		}
		a.pending[a.req] = &pendingReq{label: "Password changed", form: f}
		f.busy = true
		return a.send(msg)
	}
	a.push(f)
}

func (a *App) promptDialog(title, label, initial string, onOK func(a *App, s string) tea.Cmd) {
	f := newForm(title, 60)
	in := f.add(NewInput(IText, label))
	in.SetText(initial)
	in.Required = true
	f.buttons = []string{"OK", "Cancel"}
	f.onSave = func(a *App, f *form) tea.Cmd {
		a.closeOverlay(f)
		return onOK(a, strings.TrimSpace(in.Text()))
	}
	a.push(f)
}

// sortDialog: up to three sort keys for the active grid.
func (a *App) sortDialog(g *Grid) {
	f := newForm("Sort", 60)
	var keys, dirs []*Input
	colChoices := []core.Choice{{Value: nil, Label: "(none)"}}
	for _, c := range g.vis() {
		colChoices = append(colChoices, core.Choice{Value: c.Key, Label: c.Title})
	}
	dirChoices := []core.Choice{{Value: false, Label: "▲ ascending"}, {Value: true, Label: "▼ descending"}}
	for i := 0; i < 3; i++ {
		k := NewInput(ISelect, fmt.Sprintf("Then by %d", i+1))
		if i == 0 {
			k.Label = "Sort by"
		}
		k.ChoicesFn = func() []core.Choice { return colChoices }
		d := NewInput(ISelect, "  direction")
		d.ChoicesFn = func() []core.Choice { return dirChoices }
		var kv any
		dv := false
		if i < len(g.Sort) {
			kv, dv = g.Sort[i].Key, g.Sort[i].Desc
		}
		k.SetValue(kv, colChoices)
		d.SetValue(dv, dirChoices)
		f.add(k)
		f.add(d)
		keys, dirs = append(keys, k), append(dirs, d)
	}
	f.buttons = []string{"Apply", "Cancel"}
	f.onSave = func(a *App, f *form) tea.Cmd {
		var spec []core.SortSpec
		for i := range keys {
			kv, _ := keys[i].Value()
			dv, _ := dirs[i].Value()
			if s, ok := kv.(string); ok && s != "" {
				desc, _ := dv.(bool)
				spec = append(spec, core.SortSpec{Key: s, Desc: desc})
			}
		}
		g.Sort = spec
		a.rebuild()
		a.closeOverlay(f)
		return nil
	}
	a.push(f)
}

// ---- menus ----

type menuDef struct {
	title string
	accel rune
	items func(a *App) []menuItem
}

func act(a Action, accel rune) menuItem {
	return menuItem{label: a.Label(), accel: accel, run: func(app *App) tea.Cmd { return app.do(a) }}
}

func (a *App) actItem(ac Action, accel rune) menuItem {
	it := act(ac, accel)
	it.key = keysFor(ac, a.keyCtx())
	return it
}

// menuBar is filled in init: its items call actions that open menus.
var menuBar []menuDef

func init() {
	menuBar = []menuDef{
		{"File", 'f', func(a *App) []menuItem {
			return []menuItem{a.actItem(ANewProject, 'p'), a.actItem(ANewTask, 't'), {sep: true}, a.actItem(ASetPassword, 'w'), a.actItem(ALogout, 'l'), a.actItem(AQuit, 'q')}
		}},
		{"Edit", 'e', func(a *App) []menuItem {
			return []menuItem{a.actItem(AUndo, 'u'), a.actItem(ARedo, 'r'), {sep: true}, a.actItem(ACopy, 'c'), a.actItem(APaste, 'p'),
				a.actItem(AFillDown, 'f'), a.actItem(AClear, 'l'), {sep: true}, a.actItem(AEditRecord, 'e'), a.actItem(ADelete, 'd')}
		}},
		{"View", 'v', func(a *App) []menuItem {
			var out []menuItem
			for i := range viewNames {
				it := a.actItem(AView1+Action(i), rune('1'+i))
				it.label = fmt.Sprintf("%d %s", i+1, viewNames[i])
				it.key = fmt.Sprintf("Alt+%d", i+1)
				out = append(out, it)
			}
			out = append(out, menuItem{sep: true}, a.actItem(AToggleDetail, 'd'), a.actItem(AColumns, 'c'))
			for i, t := range themes {
				ti := i
				out = append(out, menuItem{label: "Theme: " + t.Name, run: func(a *App) tea.Cmd { a.theme = ti; return nil }})
			}
			return out
		}},
		{"Data", 'd', func(a *App) []menuItem {
			out := []menuItem{a.actItem(AFilter, 'f'), a.actItem(AClearFilter, 'c'), {sep: true}, a.actItem(ASortColumn, 's'),
				a.actItem(ASortDialog, 'o'), a.actItem(AGroupColumn, 'g'), a.actItem(AClearGroup, 'r'), {sep: true}, a.actItem(ASaveFilter, 'v')}
			for _, sf := range a.saved {
				s := sf
				out = append(out, menuItem{label: "★ " + s.Name, detail: viewNames[s.View], run: func(a *App) tea.Cmd { a.applySaved(s); return nil }})
			}
			return out
		}},
		{"Help", 'h', func(a *App) []menuItem {
			return []menuItem{a.actItem(AHelp, 'k'), a.actItem(AAbout, 'a')}
		}},
	}
}

// menuTitleX returns the x position of each menu bar title.
func menuTitleX() []int {
	xs := make([]int, len(menuBar))
	x := 1
	for i, m := range menuBar {
		xs[i] = x
		x += StrW(m.title) + 2
	}
	return xs
}

func (a *App) openMenu(i int) {
	items := menuBar[i].items(a)
	l := &listOverlay{items: items, x: menuTitleX()[i] - 1, y: 1, menuBar: i}
	l.move(0)
	if len(items) > 0 && (items[0].sep || items[0].off) {
		l.move(1)
	}
	a.push(l)
}

// contextMenu for the cell under the cursor (right-click / ⇧F10).
func (a *App) contextMenu(x, y int) {
	g := a.activeGrid()
	var items []menuItem
	if g != nil {
		col, _ := g.CurCol()
		cell := core.CellFor(a.rep, &col, g.CurID())
		items = []menuItem{
			a.actItem(AEdit, 'e'), a.actItem(AEditRecord, 'r'), {sep: true},
			a.actItem(ACopy, 'c'), a.actItem(APaste, 'p'), a.actItem(AFillDown, 'f'), a.actItem(AClear, 'l'), {sep: true},
		}
		if qb := a.queries[a.view]; cell.Text != "" && qb.hasTarget {
			term := filterTerm(col, cell.Text)
			if term != "" {
				items = append(items,
					menuItem{label: "Filter by this value", accel: 'b', detail: term, run: func(a *App) tea.Cmd { a.addFilterTerm(term); return nil }},
					menuItem{label: "Exclude this value", accel: 'x', detail: "-" + term, run: func(a *App) tea.Cmd { a.addFilterTerm("-" + term); return nil }})
			}
		}
		key := col.Key
		items = append(items,
			menuItem{label: "Sort ▲", accel: 'a', run: func(a *App) tea.Cmd { g.Sort = []core.SortSpec{{Key: key}}; a.rebuild(); return nil }},
			menuItem{label: "Sort ▼", accel: 'd', run: func(a *App) tea.Cmd { g.Sort = []core.SortSpec{{Key: key, Desc: true}}; a.rebuild(); return nil }},
			a.actItem(AGroupColumn, 'g'),
			menuItem{label: "Hide column", accel: 'h', run: func(a *App) tea.Cmd {
				for i := range g.Cols {
					if g.Cols[i].Key == key {
						g.Cols[i].Hidden = true
					}
				}
				a.rebuild()
				return nil
			}},
			menuItem{sep: true}, a.actItem(ADelete, 't'))
		if g.Name == "users" {
			items = append(items, a.actItem(ASetPassword, 'w'))
		}
	} else {
		items = []menuItem{a.actItem(AEditRecord, 'e'), a.actItem(ANew, 'n'), a.actItem(ADelete, 'd')}
	}
	a.push(&listOverlay{title: "", items: items, x: x, y: y, menuBar: -1})
}

// filterTerm builds a query term for "filter by this value".
func filterTerm(col core.Column, text string) string {
	q := func(s string) string {
		s = strings.ToLower(s)
		if strings.ContainsAny(s, " ,") {
			return "\"" + s + "\""
		}
		return s
	}
	switch col.Key {
	case "status", "priority", "health":
		return col.Key + ":" + strings.ReplaceAll(strings.ToLower(text), " ", "_")
	case "owner_id":
		return "owner:" + q(strings.Fields(text)[0])
	case "assignee_id":
		return "assignee:" + q(strings.Fields(text)[0])
	case "program_id":
		return "program:" + q(strings.Fields(text)[0])
	case "project", "code":
		return "project:" + strings.ToLower(text)
	case "tags":
		return "#" + strings.Fields(text)[0]
	case "phase":
		return "phase:" + q(text)
	}
	return ""
}

func (a *App) addFilterTerm(term string) {
	qb := a.queries[a.view]
	t := qb.in.Text()
	if t != "" && !strings.HasSuffix(t, " ") {
		t += " "
	}
	qb.in.SetText(t + term)
	a.rebuild()
}

// ---- command palette ----

func (a *App) palette() {
	var items []menuItem
	seen := map[Action]bool{}
	for _, b := range bindings {
		if seen[b.Action] {
			continue
		}
		seen[b.Action] = true
		items = append(items, a.actItem(b.Action, 0))
	}
	for _, ac := range []Action{ANewProject, ANewTask, AClearFilter, ASaveFilter, ASortDialog, AClearGroup, ASetPassword, ALogout, AAbout} {
		if !seen[ac] {
			items = append(items, a.actItem(ac, 0))
		}
	}
	for i, t := range themes {
		ti := i
		items = append(items, menuItem{label: "Theme: " + t.Name, run: func(a *App) tea.Cmd { a.theme = ti; return nil }})
	}
	for _, sf := range a.saved {
		s := sf
		items = append(items, menuItem{label: "Saved filter: " + s.Name, detail: viewNames[s.View], run: func(a *App) tea.Cmd { a.applySaved(s); return nil }})
	}
	r := a.rep
	pids := core.SortedIDs(r.Projects)
	sort.Slice(pids, func(i, j int) bool { return r.Projects[pids[i]].Code < r.Projects[pids[j]].Code })
	for _, id := range pids {
		p := r.Projects[id]
		pid := id
		items = append(items, menuItem{label: "Go to project " + p.Code + "  " + p.Name, detail: r.ProgramName(p.ProgramID),
			run: func(a *App) tea.Cmd { a.reveal(vProjects, "projects", pid); return nil }})
	}
	for _, id := range core.SortedIDs(r.Tasks) {
		t := r.Tasks[id]
		tid := id
		items = append(items, menuItem{label: "Go to task " + r.TaskRef(t) + "  " + t.Title, detail: t.Status.Label(),
			run: func(a *App) tea.Cmd { a.reveal(vTasks, "tasks", tid); return nil }})
	}
	l := &listOverlay{title: "Commands", all: items, x: -1, width: min(90, a.w-6), filter: NewInput(IText, "search"), menuBar: -1, maxRows: max(5, min(16, a.h-12))}
	l.filter.Placeholder = "type a command, project code or task…"
	l.refilter()
	a.push(l)
}

// reveal switches to a view and puts the cursor on a row (clearing filters
// and collapsed groups that would hide it).
func (a *App) reveal(view int, grid string, id int64) {
	if !a.rep.Exists(a.grids[grid].Ent, id) {
		a.wantReveal = &revealReq{view, grid, id}
		return
	}
	a.setView(view)
	a.pane = 0
	g := a.grids[grid]
	a.rebuild()
	if !g.hasID(id) {
		a.queries[view].in.SetText("")
		g.Collapsed = map[string]bool{}
		a.rebuild()
	}
	for i, row := range g.Rows {
		if !row.IsGroup && row.ID == id {
			g.Row = i
			break
		}
	}
	a.rebuildDetail()
}

func (g *Grid) hasID(id int64) bool {
	for _, row := range g.Rows {
		if !row.IsGroup && row.ID == id {
			return true
		}
	}
	return false
}
