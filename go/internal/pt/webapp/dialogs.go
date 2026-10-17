//go:build js && wasm

package webapp

import (
	"fmt"
	"sort"
	"strconv"
	"strings"
	"syscall/js"

	"git.wharf.fluidgrid.org/jcz/tictactoe/go/internal/pt/core"
)

type Dialog interface {
	Build() js.Value
	Key(k Key) bool
	Hints() []hint
	Destroy()
	AfterOpen()
}

var dialogBuilt Dialog
var dialogScope Scope

func (a *App) OpenDialog(d Dialog) {
	a.closeMenus()
	if a.dialog != nil {
		a.CloseDialog()
	}
	a.dialog = d
	a.Invalidate()
}

func (a *App) CloseDialog() {
	if a.dialog != nil {
		a.dialog.Destroy()
	}
	a.dialog = nil
	a.Invalidate()
	After(10, func() {
		if a.dialog == nil && len(a.views) > 0 {
			a.curView().FocusDOM()
		}
	})
}

func (a *App) renderDialog() {
	if a.dialog == dialogBuilt {
		return
	}
	dialogScope.Release()
	Clear(a.dialogLayer)
	dialogBuilt = a.dialog
	if a.dialog == nil {
		a.dialogLayer.Set("className", "dialog-layer")
		return
	}
	a.dialogLayer.Set("className", "dialog-layer open")
	back := E("div", "backdrop")
	dialogScope.On(back, "mousedown", func(ev js.Value) {
		if ev.Get("target").Equal(back) {
			a.CloseDialog()
		}
	})
	Add(back, a.dialog.Build())
	Add(a.dialogLayer, back)
	a.dialog.AfterOpen()
}

func dialogBox(title, cls string) (js.Value, js.Value) {
	box := E("div", "dialog "+cls)
	Attr(box, "role", "dialog")
	h := E("div", "dhead")
	Add(h, ET("span", "", title), ET("span", "dclose muted", "Esc"))
	body := E("div", "dbody")
	Add(box, h, body)
	return box, body
}

// ---- forms -------------------------------------------------------------------------------

type formField struct {
	key   string
	label string
	ed    *Editor
	row   js.Value
	msg   js.Value
	base  any
	help  string
}

type formDialog struct {
	app     *App
	title   string
	cls     string
	fields  []*formField
	info    js.Value
	foot    js.Value
	errEl   js.Value
	saveLbl string
	onSave  func(f *formDialog)
	extra   func(body js.Value)
	scope   Scope
	saving  bool
	built   bool
	focusAt int
}

func (f *formDialog) add(key, label string, o EditorOpts) *formField {
	o.Embedded = true
	if o.Field == "" {
		o.Field = key
	}
	ff := &formField{key: key, label: label, base: o.Initial}
	ff.ed = NewEditor(f.app, o)
	f.fields = append(f.fields, ff)
	return ff
}

func (f *formDialog) field(key string) *formField {
	for _, ff := range f.fields {
		if ff.key == key {
			return ff
		}
	}
	return nil
}

func (f *formDialog) val(key string) any {
	if ff := f.field(key); ff != nil {
		v, _ := ff.ed.Value()
		return v
	}
	return nil
}

func (f *formDialog) Build() js.Value {
	box, body := dialogBox(f.title, "form "+f.cls)
	grid := E("div", "fgrid")
	for i, ff := range f.fields {
		row := E("label", "frow")
		lab := ET("span", "flabel", ff.label)
		ff.msg = E("span", "fmsg")
		in := E("div", "fin")
		Add(in, ff.ed.root, ff.msg)
		Add(row, lab, in)
		if ff.ed.o.Kind == core.EdMultiline || ff.ed.o.Kind == core.EdChips {
			row.Set("className", "frow wide")
		}
		ff.row = row
		Data(ff.row, "f", strconv.Itoa(i))
		Add(grid, row)
	}
	Add(body, grid)
	f.info = E("div", "finfo")
	Add(body, f.info)
	if f.extra != nil {
		f.extra(body)
	}
	f.errEl = E("div", "ferr")
	f.foot = E("div", "dfoot")
	save := ET("button", "btn primary", f.saveLbl)
	Attr(save, "type", "button")
	cancel := ET("button", "btn", "Cancel")
	Attr(cancel, "type", "button")
	Add(f.foot, f.errEl, E("span", "spacer"), ET("span", "muted", "^S saves · Esc cancels  "), cancel, save)
	Add(box, f.foot)
	f.scope.On(save, "click", func(js.Value) { f.save() })
	f.scope.On(cancel, "click", func(js.Value) { f.app.CloseDialog() })
	f.built = true
	return box
}

func (f *formDialog) AfterOpen() {
	if f.focusAt < len(f.fields) {
		f.fields[f.focusAt].ed.Focus()
	}
}

func (f *formDialog) save() {
	if f.saving {
		return
	}
	// a field with a parse error blocks saving
	for _, ff := range f.fields {
		if _, err := ff.ed.Value(); err != nil {
			f.setMsg(ff.key, err.Error())
			ff.ed.Focus()
			return
		}
	}
	f.onSave(f)
}

func (f *formDialog) setMsg(key, msg string) {
	if ff := f.field(key); ff != nil && ff.msg.Truthy() {
		ff.msg.Set("textContent", msg)
		if msg == "" {
			ff.row.Get("classList").Call("remove", "bad")
		} else {
			ff.row.Get("classList").Call("add", "bad")
		}
	}
}

func (f *formDialog) setError(msg string) {
	if f.errEl.Truthy() {
		f.errEl.Set("textContent", msg)
	}
}

func (f *formDialog) Key(k Key) bool {
	// the focused field's editor handles its own keys (lists, dates…)
	for _, ff := range f.fields {
		if ff.ed.focused {
			if ff.ed.Key(k) {
				return true
			}
		}
	}
	if k.Is("C-s") || (k.Is("C-Enter")) {
		f.save()
		return true
	}
	return false
}

func (f *formDialog) Hints() []hint {
	return []hint{{"Tab", "Next field", ""}, {"↑↓", "Choose", ""}, {"Enter", "Pick", ""}, {"^S", "Save", ""}, {"Esc", "Cancel", ""}}
}

func (f *formDialog) Destroy() {
	f.scope.Release()
	for _, ff := range f.fields {
		ff.ed.Destroy()
	}
}

// submitCreate sends one create op and reports field errors from the server.
func (f *formDialog) submitCreate(e core.Entity, values map[string]any, done func(id int64)) {
	f.saving = true
	f.setError("Saving…")
	f.app.Mutate([]core.Op{core.CreateOp(e, "new", values)}, "create", func(m core.ResultMsg) {
		f.saving = false
		if m.Error != nil {
			f.setError("✗ " + m.Error.Message)
			return
		}
		id := m.Created["new"]
		f.app.CloseDialog()
		f.app.toast("ok", "Created", f.app.r.RowLabel(e, id))
		if done != nil {
			After(50, func() { done(id) })
		}
	})
}

// ---- New project (cascading) ---------------------------------------------------------------

func newProjectDialog(a *App) Dialog {
	f := &formDialog{app: a, title: "New project", saveLbl: "Create project"}
	codeEdited := false
	var info func()
	prog := f.add("program_id", "Program", EditorOpts{Kind: core.EdCombo, Entity: core.EntProject, Placeholder: "type to search programs"})
	code := f.add("code", "Code", EditorOpts{Kind: core.EdText, Entity: core.EntProject, InitialText: a.r.NextCode(nil)})
	name := f.add("name", "Name", EditorOpts{Kind: core.EdText, Entity: core.EntProject, Placeholder: "what is this project?"})
	tpl := f.add("template_id", "Template", EditorOpts{Kind: core.EdSelect, Entity: core.EntProject,
		Choices: func() []core.Choice { return core.Choices(a.r, core.EntProject, "template_id", 0, nil) }})
	owner := f.add("owner_id", "Owner", EditorOpts{Kind: core.EdCombo, Entity: core.EntProject, Initial: a.r.MeID(),
		InitialText: a.r.Me.DisplayName})
	start := f.add("start_date", "Start", EditorOpts{Kind: core.EdDate, Entity: core.EntProject, Initial: today().ISO(), InitialText: today().ISO()})
	due := f.add("due_date", "Due", EditorOpts{Kind: core.EdDate, Entity: core.EntProject})
	budget := f.add("budget_hours", "Budget h", EditorOpts{Kind: core.EdNumber, Entity: core.EntProject})
	_ = name
	owner.ed.in.Set("value", a.r.Me.DisplayName)
	dueEdited, budgetEdited := false, false
	recalc := func() {
		tid, _ := core.AsInt(tpl.ed.value)
		n, h, span := a.r.TemplateStats(tid)
		if tid == 0 {
			f.info.Set("textContent", "Blank project: no tasks are created.")
		} else {
			f.info.Set("textContent", fmt.Sprintf("Template creates %d tasks · %s h estimated · %d days", n, core.FmtHours(h), span))
		}
		if !dueEdited && tid != 0 {
			if s, ok := f.val("start_date").(string); ok {
				if d, ok := core.ParseISO(s); ok {
					due.ed.in.Set("value", d.AddDays(int(span)).ISO())
				}
			}
		}
		if !budgetEdited && tid != 0 {
			budget.ed.in.Set("value", core.FmtHours(h))
		}
	}
	info = recalc
	prog.ed.o.OnChange = func(v any) {
		if !codeEdited {
			if id, ok := core.AsInt(v); ok {
				code.ed.in.Set("value", a.r.NextCode(&id))
			} else {
				code.ed.in.Set("value", a.r.NextCode(nil))
			}
		}
	}
	code.ed.o.OnChange = func(any) { codeEdited = true }
	tpl.ed.o.OnChange = func(any) { info() }
	start.ed.o.OnChange = func(any) { info() }
	due.ed.o.OnChange = func(any) { dueEdited = true }
	budget.ed.o.OnChange = func(any) { budgetEdited = true }
	// default template: the first real one
	for _, c := range tpl.ed.choices {
		if c.Value != nil {
			tpl.ed.value = c.Value
			tpl.ed.in.Set("value", c.Label)
			break
		}
	}
	f.extra = func(js.Value) { After(0, info) }
	f.onSave = func(f *formDialog) {
		vals := map[string]any{}
		for _, k := range []string{"code", "name", "program_id", "template_id", "owner_id", "start_date", "due_date", "budget_hours"} {
			if v := f.val(k); v != nil && v != "" {
				vals[k] = v
			}
		}
		f.submitCreate(core.EntProject, vals, func(id int64) { a.jumpTo(core.EntProject, id) })
	}
	return f
}

// ---- New task (cascading Program ▸ Project ▸ Phase) --------------------------------------------

func newTaskDialog(a *App, projectID *int64, status string) Dialog {
	f := &formDialog{app: a, title: "New task", saveLbl: "Create task", focusAt: 3}
	var progID *int64
	if projectID != nil {
		if p := a.r.Projects[*projectID]; p != nil {
			progID = p.ProgramID
		}
	} else {
		f.focusAt = 0
	}
	progChoices := func() []core.Choice {
		cs := core.Choices(a.r, core.EntProject, "program_id", 0, nil)
		cs[0].Label = "(all programs)"
		return cs
	}
	var prog, proj, phase, assignee *formField
	projChoices := func() []core.Choice {
		pid, hasProg := core.AsInt(prog.ed.value)
		var out []core.Choice
		for _, c := range core.Choices(a.r, core.EntTask, "project_id", 0, nil) {
			id, _ := core.AsInt(c.Value)
			p := a.r.Projects[id]
			if hasProg && (p.ProgramID == nil || *p.ProgramID != pid) {
				continue
			}
			out = append(out, c)
		}
		return out
	}
	var pv any
	var ptxt string
	if projectID != nil {
		pv = *projectID
		ptxt = a.r.ProjectCode(*projectID)
	}
	var progV any
	if progID != nil {
		progV = *progID
	}
	prog = f.add("program", "Program", EditorOpts{Kind: core.EdCombo, Entity: core.EntProject, Field: "program_id", Initial: progV, Choices: progChoices})
	proj = f.add("project_id", "Project", EditorOpts{Kind: core.EdCombo, Entity: core.EntTask, Initial: pv, InitialText: ptxt})
	proj.ed.o.Choices = projChoices
	proj.ed.choices = projChoices()
	proj.ed.in.Set("value", proj.ed.labelOf(pv))
	phaseChoices := func() []core.Choice {
		id, _ := core.AsInt(proj.ed.value)
		return core.Choices(a.r, core.EntTask, "phase", 0, &id)
	}
	phase = f.add("phase", "Phase", EditorOpts{Kind: core.EdSuggest, Entity: core.EntTask, Choices: phaseChoices, Placeholder: "pick or type a phase"})
	f.add("title", "Title", EditorOpts{Kind: core.EdText, Entity: core.EntTask, Placeholder: "what needs doing?"})
	var owner any
	if projectID != nil {
		if p := a.r.Projects[*projectID]; p != nil && p.OwnerID != nil {
			owner = *p.OwnerID
		}
	}
	assignee = f.add("assignee_id", "Assignee", EditorOpts{Kind: core.EdCombo, Entity: core.EntTask, Initial: owner})
	st := status
	if st == "" {
		st = "todo"
	}
	f.add("status", "Status", EditorOpts{Kind: core.EdSelect, Entity: core.EntTask, Initial: st})
	f.add("start_date", "Start", EditorOpts{Kind: core.EdDate, Entity: core.EntTask, Initial: today().ISO(), InitialText: today().ISO()})
	f.add("due_date", "Due", EditorOpts{Kind: core.EdDate, Entity: core.EntTask})
	f.add("est_hours", "Est h", EditorOpts{Kind: core.EdNumber, Entity: core.EntTask})
	prog.ed.o.OnChange = func(v any) {
		proj.ed.choices = projChoices()
		// drop a project outside the chosen program
		keep := false
		for _, c := range proj.ed.choices {
			if core.ValuesEqual(c.Value, proj.ed.value) {
				keep = true
			}
		}
		if !keep {
			proj.ed.value = nil
			proj.ed.in.Set("value", "")
		}
		f.info.Set("textContent", fmt.Sprintf("%d projects in this program", len(proj.ed.choices)))
	}
	proj.ed.o.OnChange = func(v any) {
		phase.ed.choices = phaseChoices()
		phase.ed.refilter()
		id, _ := core.AsInt(v)
		if p := a.r.Projects[id]; p != nil {
			if p.OwnerID != nil && assignee.ed.value == nil {
				assignee.ed.value = *p.OwnerID
				assignee.ed.in.Set("value", assignee.ed.labelOf(*p.OwnerID))
			}
			if p.ProgramID != nil && prog.ed.value == nil {
				prog.ed.value = *p.ProgramID
				prog.ed.in.Set("value", prog.ed.labelOf(*p.ProgramID))
			}
			f.info.Set("textContent", fmt.Sprintf("%s · %d tasks · phases: %s", p.Name, len(a.r.ProjectTasks(id)),
				strings.Join(a.r.PhasesForProject(id), ", ")))
		}
	}
	f.onSave = func(f *formDialog) {
		if proj.ed.value == nil {
			f.setMsg("project_id", "choose a project")
			proj.ed.Focus()
			return
		}
		vals := map[string]any{}
		for _, k := range []string{"project_id", "phase", "title", "assignee_id", "status", "start_date", "due_date", "est_hours"} {
			if v := f.val(k); v != nil && v != "" {
				vals[k] = v
			}
		}
		f.submitCreate(core.EntTask, vals, func(id int64) {
			if a.view != 0 && a.view != 2 {
				a.jumpTo(core.EntTask, id)
			}
		})
	}
	return f
}

// ---- record form (^E) ------------------------------------------------------------------------

func newRecordForm(a *App, e core.Entity, id int64) Dialog {
	title := "Edit " + e.Label() + " " + a.r.RowLabel(e, id)
	f := &formDialog{app: a, title: title, saveLbl: "Save", cls: "wide"}
	editable := a.r.CanEdit()
	for _, m := range core.Fields(e) {
		if m.CreateOnly || m.Kind.Tag == core.KPassword {
			continue
		}
		col := core.Column{Entity: e, Field: m.Name}
		kind := editorForField(e, m.Name)
		if m.Perm == core.PermAdmin && !a.r.IsAdmin() {
			editable = false
		}
		_ = col
		f.add(m.Name, m.Label, EditorOpts{Kind: kind, Entity: e, ID: id, Initial: a.r.FieldValue(e, id, m.Name),
			InitialText: core.EditText(a.r, e, m.Name, id)})
	}
	if e == core.EntProject || e == core.EntTask {
		ce := core.CfTask
		if e == core.EntProject {
			ce = core.CfProject
		}
		for _, cf := range a.r.CustomFieldsFor(ce) {
			k := cf.Key()
			kind := map[core.CfKind]core.Editor{core.CfText: core.EdText, core.CfNumber: core.EdNumber, core.CfDate: core.EdDate, core.CfEnum: core.EdSelect}[cf.Kind]
			f.add(k, cf.Name, EditorOpts{Kind: kind, Entity: e, ID: id, Initial: a.r.FieldValue(e, id, k), InitialText: core.EditText(a.r, e, k, id)})
		}
	}
	if !editable {
		f.saveLbl = "Close"
	}
	force := map[string]bool{}
	f.extra = func(body js.Value) {
		if e == core.EntUser && (a.r.IsAdmin() || id == a.r.MeID()) {
			b := ET("button", "btn", "Set password…")
			Attr(b, "type", "button")
			f.scope.On(b, "click", func(js.Value) { a.OpenDialog(newPasswordDialog(a, id)) })
			Add(body, b)
		}
		acts := recentActivity(a.r, string(e), id, 8)
		if len(acts) > 0 {
			Add(body, ET("h4", "fsec", "Recent changes"))
			for _, act := range acts {
				Add(body, ET("div", "actline", core.RelativeTime(act.TS, nowSecs())+" · "+a.r.Username(act.UserID)+" · "+act.Label))
			}
		}
		if !editable {
			Add(body, ET("div", "lock", "🔒 Read only for your role."))
		}
	}
	f.onSave = func(f *formDialog) {
		if !editable {
			a.CloseDialog()
			return
		}
		var ops []core.Op
		for _, ff := range f.fields {
			v, _ := ff.ed.Value()
			if core.ValuesEqual(v, ff.base) && !force[ff.key] {
				continue
			}
			op := core.SetOp(e, id, ff.key, ff.base, v)
			op.Force = force[ff.key]
			ops = append(ops, op)
		}
		if len(ops) == 0 {
			a.CloseDialog()
			return
		}
		f.saving = true
		f.setError("Saving…")
		a.Mutate(ops, "record", func(m core.ResultMsg) {
			f.saving = false
			if m.Error != nil {
				f.setError("✗ " + m.Error.Message)
				return
			}
			if len(m.Conflicts) > 0 {
				f.setError(fmt.Sprintf("⚠ %d field(s) were changed by someone else. Save again to overwrite with yours, or Esc.", len(m.Conflicts)))
				for _, c := range m.Conflicts {
					who := "someone"
					if c.By != nil {
						who = a.r.UserName(c.By)
					}
					f.setMsg(c.Field, fmt.Sprintf("%s set it to %s", who, core.DisplayValue(a.r, e, c.Field, c.Current)))
					force[c.Field] = true
					delete(a.conflicts, cellKey{e, id, c.Field})
				}
				// other fields were saved: their new base is our value
				for _, ff := range f.fields {
					if !force[ff.key] {
						ff.base, _ = ff.ed.Value()
					}
				}
				return
			}
			a.CloseDialog()
		})
	}
	return f
}

func editorForField(e core.Entity, field string) core.Editor {
	for _, c := range append(core.ProjectColumns(app.r), append(core.TaskColumns(app.r, false), append(core.UserColumns(),
		append(core.ProgramColumns(), append(core.TemplateColumns(), append(core.TemplateTaskColumns(), core.CustomFieldColumns()...)...)...)...)...)...) {
		if c.Entity == e && c.Field == field && c.Editor != core.EdNone {
			return c.Editor
		}
	}
	m := core.Field(e, field)
	if m == nil {
		return core.EdText
	}
	switch m.Kind.Tag {
	case core.KEnum:
		return core.EdSelect
	case core.KRef:
		return core.EdCombo
	case core.KDate:
		return core.EdDate
	case core.KHours:
		return core.EdNumber
	case core.KInt:
		return core.EdInt
	case core.KTags, core.KDeps, core.KOptions:
		return core.EdChips
	case core.KBool:
		return core.EdBool
	}
	if m.Kind.Tag == core.KText && m.Kind.Max > 2000 {
		return core.EdMultiline
	}
	return core.EdText
}

// ---- admin dialogs ----------------------------------------------------------------------------

func newFieldDialog(a *App) Dialog {
	f := &formDialog{app: a, title: "New custom field", saveLbl: "Add field"}
	f.add("entity", "Applies to", EditorOpts{Kind: core.EdSelect, Entity: core.EntCustomField, Initial: "task"})
	f.add("name", "Name", EditorOpts{Kind: core.EdText, Entity: core.EntCustomField})
	f.add("kind", "Kind", EditorOpts{Kind: core.EdSelect, Entity: core.EntCustomField, Initial: "text"})
	f.add("options", "Options (enum)", EditorOpts{Kind: core.EdChips, Entity: core.EntCustomField, Placeholder: "type an option, Enter"})
	f.onSave = func(f *formDialog) {
		vals := map[string]any{"entity": f.val("entity"), "name": f.val("name"), "kind": f.val("kind")}
		if l, ok := f.val("options").([]any); ok && len(l) > 0 {
			vals["options"] = l
		}
		f.submitCreate(core.EntCustomField, vals, nil)
	}
	return f
}

func newUserDialog(a *App) Dialog {
	f := &formDialog{app: a, title: "New user", saveLbl: "Add user"}
	f.add("username", "Username", EditorOpts{Kind: core.EdText, Entity: core.EntUser})
	f.add("display_name", "Name", EditorOpts{Kind: core.EdText, Entity: core.EntUser})
	f.add("role", "Role", EditorOpts{Kind: core.EdSelect, Entity: core.EntUser, Initial: "member"})
	pw := f.add("password", "Password", EditorOpts{Kind: core.EdText, Entity: core.EntUser})
	pw2 := f.add("password2", "Again", EditorOpts{Kind: core.EdText, Entity: core.EntUser, Field: "password"})
	Attr(pw.ed.in, "type", "password")
	Attr(pw2.ed.in, "type", "password")
	f.onSave = func(f *formDialog) {
		if f.val("password") != f.val("password2") {
			f.setMsg("password2", "passwords don't match")
			return
		}
		f.submitCreate(core.EntUser, map[string]any{"username": f.val("username"), "display_name": f.val("display_name"),
			"role": f.val("role"), "password": f.val("password")}, nil)
	}
	return f
}

func newProgramDialog(a *App) Dialog {
	f := &formDialog{app: a, title: "New program", saveLbl: "Add program"}
	f.add("name", "Name", EditorOpts{Kind: core.EdText, Entity: core.EntProgram})
	f.add("client", "Client", EditorOpts{Kind: core.EdText, Entity: core.EntProgram})
	f.add("prefix", "Code prefix", EditorOpts{Kind: core.EdText, Entity: core.EntProgram, Placeholder: "e.g. NWR"})
	f.add("color", "Colour", EditorOpts{Kind: core.EdText, Entity: core.EntProgram, InitialText: "#5aa9ff"})
	f.onSave = func(f *formDialog) {
		vals := map[string]any{}
		for _, k := range []string{"name", "client", "prefix", "color"} {
			if v := f.val(k); v != nil && v != "" {
				vals[k] = v
			}
		}
		f.submitCreate(core.EntProgram, vals, nil)
	}
	return f
}

func newPasswordDialog(a *App, uid int64) Dialog {
	f := &formDialog{app: a, title: "Set password for " + a.r.RowLabel(core.EntUser, uid), saveLbl: "Set password"}
	if uid == a.r.MeID() && !a.r.IsAdmin() {
		old := f.add("old", "Current", EditorOpts{Kind: core.EdText, Entity: core.EntUser, Field: "display_name"})
		Attr(old.ed.in, "type", "password")
	}
	p := f.add("pw", "New password", EditorOpts{Kind: core.EdText, Entity: core.EntUser, Field: "display_name"})
	Attr(p.ed.in, "type", "password")
	f.onSave = func(f *formDialog) {
		pw := p.ed.text()
		if len([]rune(pw)) < 8 {
			f.setMsg("pw", "at least 8 characters")
			return
		}
		a.reqN++
		req := a.reqN
		msg := core.SetPasswordMsg{Req: req, UserID: uid, Password: pw}
		if o := f.field("old"); o != nil {
			s := o.ed.text()
			msg.OldPassword = &s
		}
		a.pending[req] = &pendingReq{label: "password", onDone: func(m core.ResultMsg) {
			if m.Error != nil {
				f.setError("✗ " + m.Error.Message)
				return
			}
			a.CloseDialog()
			a.toast("ok", "Password changed", "Their other sessions were signed out.")
		}}
		a.send(msg)
	}
	return f
}

// ---- prompt / confirm / about -----------------------------------------------------------------

func newPromptDialog(a *App, title, label, initial string, ok func(string)) Dialog {
	f := &formDialog{app: a, title: title, saveLbl: "OK"}
	in := f.add("v", label, EditorOpts{Kind: core.EdText, Entity: core.EntProject, Field: "name", InitialText: initial})
	f.onSave = func(f *formDialog) {
		v := strings.TrimSpace(in.ed.text())
		if v == "" {
			f.setMsg("v", "required")
			return
		}
		a.CloseDialog()
		ok(v)
	}
	return f
}

type confirmDialog struct {
	app     *App
	title   string
	msg     string
	okLabel string
	ok      func()
	scope   Scope
	sel     int // 0 cancel, 1 ok
	btns    [2]js.Value
}

func newConfirm(a *App, title, msg, okLabel string, ok func()) Dialog {
	return &confirmDialog{app: a, title: title, msg: msg, okLabel: okLabel, ok: ok}
}

func (d *confirmDialog) Build() js.Value {
	box, body := dialogBox(d.title, "confirm")
	for i, line := range strings.Split(d.msg, "\n") {
		cls := ""
		if i == 0 {
			cls = "strong"
		}
		Add(body, ET("div", cls, line))
	}
	foot := E("div", "dfoot")
	cancel := ET("button", "btn", "Cancel (n)")
	ok := ET("button", "btn danger", d.okLabel+" (y)")
	d.btns = [2]js.Value{cancel, ok}
	Add(foot, E("span", "spacer"), cancel, ok)
	Add(box, foot)
	d.scope.On(cancel, "click", func(js.Value) { d.app.CloseDialog() })
	d.scope.On(ok, "click", func(js.Value) { d.app.CloseDialog(); d.ok() })
	return box
}

func (d *confirmDialog) AfterOpen() { d.btns[0].Call("focus") }

func (d *confirmDialog) Key(k Key) bool {
	switch {
	case k.Is("y"):
		d.app.CloseDialog()
		d.ok()
	case k.Is("n"):
		d.app.CloseDialog()
	case k.Is("ArrowLeft", "ArrowRight", "Tab", "S-Tab"):
		d.sel = 1 - d.sel
		d.btns[d.sel].Call("focus")
	case k.Is("Enter"):
		if d.sel == 1 {
			d.app.CloseDialog()
			d.ok()
		} else {
			d.app.CloseDialog()
		}
	default:
		return false
	}
	return true
}

func (d *confirmDialog) Hints() []hint {
	return []hint{{"y", d.okLabel, ""}, {"n", "Cancel", ""}, {"←→", "Choose", ""}, {"Enter", "Confirm choice", ""}}
}
func (d *confirmDialog) Destroy() { d.scope.Release() }

type infoDialog struct {
	app   *App
	title string
	build func(body js.Value)
}

func (d *infoDialog) Build() js.Value {
	box, body := dialogBox(d.title, "info")
	d.build(body)
	return box
}
func (d *infoDialog) AfterOpen()    {}
func (d *infoDialog) Key(Key) bool  { return false }
func (d *infoDialog) Hints() []hint { return []hint{{"Esc", "Close", ""}} }
func (d *infoDialog) Destroy()      {}

func newAboutDialog(a *App) Dialog {
	return &infoDialog{app: a, title: "About pt", build: func(body js.Value) {
		srv := "?"
		if a.r.Server != nil {
			srv = a.r.Server.Impl + " " + a.r.Server.Version
		}
		kv := E("dl", "kv")
		for _, p := range [][2]string{
			{"Client", "web-go — Go " + goVersion() + " compiled to WebAssembly, hand-written DOM kit"},
			{"Server", srv},
			{"Protocol", fmt.Sprint(core.ProtocolVersion)},
			{"Signed in", a.r.Me.DisplayName + " (" + string(a.r.Me.Role) + ")"},
			{"Data", fmt.Sprintf("%d projects · %d tasks · %d users", len(a.r.Projects), len(a.r.Tasks), len(a.r.Users))},
		} {
			Add(kv, ET("dt", "", p[0]), ET("dd", "", p[1]))
		}
		Add(body, kv, ET("p", "muted", "One SQLite server, many live clients: TUIs and browsers in Rust and Go share one protocol."))
	}}
}

// ---- help (F1) --------------------------------------------------------------------------------

type helpDialog struct {
	app   *App
	in    js.Value
	list  js.Value
	scope Scope
}

func newHelp(a *App) Dialog { return &helpDialog{app: a} }

var staticHelp = [][3]string{
	{"Grid", "↑ ↓ ← →", "Move"}, {"Grid", "Tab / ⇧Tab", "Next / previous cell"}, {"Grid", "Home / End", "First / last column"},
	{"Grid", "^Home / ^End", "First / last row"}, {"Grid", "PgUp / PgDn", "Page"}, {"Grid", "Enter, F2, double-click", "Edit cell"},
	{"Grid", "type a letter", "Start editing with it"}, {"Grid", "Del", "Clear cell"}, {"Grid", "Space", "Mark row"},
	{"Grid", "⇧↑ / ⇧↓", "Extend marks"}, {"Grid", "^V or paste", "Paste TSV from the current cell"},
	{"Grid", "← / → on a group", "Collapse / expand"}, {"Grid", "Click header / ⇧click", "Sort / add sort key"},
	{"Grid", "Drag header edge", "Resize column"}, {"Grid", "Right-click, ⇧F10", "Context menu"},
	{"Editors", "Enter / Tab / Esc", "Save ↓ / save → / cancel"}, {"Editors", "↑ ↓ in lists", "Choose"},
	{"Editors", "Dates", "fri, tom, +3d, -2w, eom, 11/3, nov 3 — ↑↓ ±week, PgUp/PgDn ±month"},
	{"Editors", "Numbers", "↑ ↓ ±1, ⇧↑ ⇧↓ ±0.25"}, {"Editors", "Chips", "Enter adds, Backspace removes, Tab saves"},
	{"Board", "⇧← ⇧→ or < >", "Move card to the next status"}, {"Board", "Drag a card", "Move it"},
	{"Timeline", "+ / -", "Zoom"}, {"Timeline", "⇧← ⇧→", "Move bar"}, {"Timeline", "[ / ]", "Shorten / extend"},
	{"Timeline", "t", "Today"}, {"Filter", "status:active @me due<+7d", "Terms are ANDed"},
	{"Filter", "#tag  -term  \"two words\"", "Tag, negate, phrase"}, {"Filter", "is:overdue has:deps", "Predicates"},
}

func (d *helpDialog) Build() js.Value {
	box, body := dialogBox("Keyboard help", "help")
	intro := ET("p", "muted", "pt keeps every action on the keyboard. The hint bar at the bottom always shows the keys for "+
		"what you're doing; ^K finds any command by name. Changes save immediately and everyone sees them live; "+
		"if someone else changed a cell first, it turns red — press Enter on it to edit from their value.")
	d.in = E("input", "ed search")
	Attr(d.in, "placeholder", "search keys and actions…")
	d.list = E("div", "helplist")
	Add(body, intro, d.in, d.list)
	d.scope.On(d.in, "input", func(js.Value) { d.fill() })
	d.fill()
	return box
}

func (d *helpDialog) fill() {
	q := strings.ToLower(d.in.Get("value").String())
	Clear(d.list)
	rows := [][3]string{}
	for _, act := range actions {
		keys := []string{}
		for _, k := range act.Keys {
			keys = append(keys, Label(k))
		}
		rows = append(rows, [3]string{act.Group, strings.Join(keys, ", "), act.Label})
	}
	rows = append(rows, staticHelp...)
	sort.SliceStable(rows, func(i, j int) bool { return rows[i][0] < rows[j][0] })
	group := ""
	for _, r := range rows {
		if q != "" && !strings.Contains(strings.ToLower(r[0]+" "+r[1]+" "+r[2]), q) {
			continue
		}
		if r[0] != group {
			group = r[0]
			Add(d.list, ET("h4", "", group))
		}
		line := E("div", "hl-row")
		Add(line, ET("kbd", "", r[1]), ET("span", "", r[2]))
		Add(d.list, line)
	}
}

func (d *helpDialog) AfterOpen()     { d.in.Call("focus") }
func (d *helpDialog) Key(k Key) bool { return false }
func (d *helpDialog) Hints() []hint  { return []hint{{"type", "Search", ""}, {"Esc", "Close", ""}} }
func (d *helpDialog) Destroy()       { d.scope.Release() }

// ---- command palette (^K) -----------------------------------------------------------------------

type palItem struct {
	label, detail, key string
	run                func()
	kind               int // 0 command, 1 project, 2 task: ties rank in this order
}

type paletteDialog struct {
	app      *App
	items    []palItem
	labels   []core.Choice
	filtered []core.FilteredChoice
	sel      int
	in, list js.Value
	scope    Scope
}

func newPalette(a *App) Dialog {
	d := &paletteDialog{app: a}
	for _, act := range actions {
		act := act
		if act.When != nil && !act.When(a) {
			continue
		}
		d.items = append(d.items, palItem{act.Label, act.Group, keyLabel(act), func() { act.Run(a) }, 0})
	}
	for _, t := range themes {
		t := t
		d.items = append(d.items, palItem{"Theme: " + themeNames[t], "View", "", func() { a.setTheme(t) }, 0})
	}
	for _, f := range a.savedQ {
		f := f
		d.items = append(d.items, palItem{"★ " + f.Name, "Saved filter · " + viewNames[f.View], "", func() { a.applySaved(f) }, 0})
	}
	for _, id := range core.SortedIDs(a.r.Projects) {
		p := a.r.Projects[id]
		pid := id
		d.items = append(d.items, palItem{"Go to " + p.Code + " " + p.Name, "Project · " + a.r.ProgramName(p.ProgramID), "",
			func() { a.jumpTo(core.EntProject, pid) }, 1})
	}
	for _, id := range core.SortedIDs(a.r.Tasks) {
		t := a.r.Tasks[id]
		tid := id
		d.items = append(d.items, palItem{"Go to " + a.r.TaskRef(t) + " " + t.Title, "Task · " + t.Status.Label(), "",
			func() { a.jumpTo(core.EntTask, tid) }, 2})
	}
	for _, it := range d.items {
		d.labels = append(d.labels, core.Choice{Label: it.label, Detail: it.detail})
	}
	return d
}

func (d *paletteDialog) Build() js.Value {
	box, body := dialogBox("Commands", "palette")
	d.in = E("input", "ed search")
	Attr(d.in, "placeholder", "type a command, view, project code or task…")
	Attr(d.in, "id", "palette-input")
	d.list = E("div", "choices pal")
	Add(body, d.in, d.list)
	d.scope.On(d.in, "input", func(js.Value) { d.sel = 0; d.fill() })
	d.scope.On(d.list, "mousedown", func(ev js.Value) {
		ev.Call("preventDefault")
		if c := Closest(ev.Get("target"), "[data-i]"); c.Truthy() {
			i, _ := strconv.Atoi(DataOf(c, "i"))
			d.runAt(i)
		}
	})
	d.fill()
	return box
}

func (d *paletteDialog) fill() {
	q := d.in.Get("value").String()
	if strings.TrimSpace(q) == "" {
		// commands first when empty
		d.filtered = nil
		for i, it := range d.items {
			if !strings.HasPrefix(it.label, "Go to ") || strings.HasPrefix(it.detail, "Views") {
				d.filtered = append(d.filtered, core.FilteredChoice{Index: i})
			}
		}
	} else {
		d.filtered = wordFilter(d.items, q)
		if len(d.filtered) == 0 {
			d.filtered = core.FilterChoices(d.labels, q)
		}
	}
	Clear(d.list)
	for i, f := range d.filtered {
		if i >= 80 {
			break
		}
		it := d.items[f.Index]
		cls := "choice"
		if i == d.sel {
			cls += " sel"
		}
		el := E("div", cls)
		Data(el, "i", strconv.Itoa(i))
		lbl := E("span", "lbl")
		highlight(lbl, it.label, f.Matched)
		Add(el, lbl, ET("span", "det", it.detail))
		if it.key != "" {
			Add(el, ET("kbd", "pk", it.key))
		}
		Add(d.list, el)
	}
	if len(d.filtered) == 0 {
		Add(d.list, ET("div", "choice empty", "nothing matches"))
	}
	if s := d.list.Call("querySelector", ".choice.sel"); s.Truthy() {
		s.Call("scrollIntoView", map[string]any{"block": "nearest"})
	}
}

// wordFilter: every query word must appear in the label or detail; earlier
// and tighter matches rank first. Returns matched rune indices for highlight.
func wordFilter(items []palItem, q string) []core.FilteredChoice {
	words := strings.Fields(strings.ToLower(q))
	type scored struct {
		fc    core.FilteredChoice
		score int
	}
	var out []scored
	for i, it := range items {
		label := strings.ToLower(it.label)
		hay := label + " " + strings.ToLower(it.detail)
		score := 0
		var matched []int
		ok := true
		for _, w := range words {
			j := strings.Index(hay, w)
			if j < 0 {
				ok = false
				break
			}
			score += j
			if j < len(label) {
				start := len([]rune(label[:j]))
				for k := 0; k < len([]rune(w)); k++ {
					matched = append(matched, start+k)
				}
			} else {
				score += 200 // only in the detail text
			}
		}
		if ok {
			out = append(out, scored{core.FilteredChoice{Index: i, Matched: matched}, score*1000 + it.kind*200 + len(label)})
		}
	}
	sort.SliceStable(out, func(a, b int) bool { return out[a].score < out[b].score })
	res := make([]core.FilteredChoice, len(out))
	for i, s := range out {
		res[i] = s.fc
	}
	return res
}

func (d *paletteDialog) runAt(i int) {
	if i < 0 || i >= len(d.filtered) {
		return
	}
	it := d.items[d.filtered[i].Index]
	d.app.CloseDialog()
	it.run()
	d.app.Invalidate()
}

func (d *paletteDialog) Key(k Key) bool {
	n := min(len(d.filtered), 80)
	switch {
	case k.Is("ArrowDown") && n > 0:
		d.sel = (d.sel + 1) % n
	case k.Is("ArrowUp") && n > 0:
		d.sel = (d.sel - 1 + n) % n
	case k.Is("Enter"):
		d.runAt(d.sel)
		return true
	default:
		return false
	}
	d.fill()
	return true
}

func (d *paletteDialog) AfterOpen() { d.in.Call("focus") }
func (d *paletteDialog) Hints() []hint {
	return []hint{{"type", "Search", ""}, {"↑↓", "Choose", ""}, {"Enter", "Run", ""}, {"Esc", "Close", ""}}
}
func (d *paletteDialog) Destroy() { d.scope.Release() }

// ---- columns (F4) & sort ---------------------------------------------------------------------------

type columnsDialog struct {
	g     *Grid
	sel   int
	list  js.Value
	scope Scope
}

func newColumnsDialog(g *Grid) Dialog { return &columnsDialog{g: g} }

func (d *columnsDialog) keys() []string {
	g := d.g
	// all columns in display order (hidden ones where they'd be)
	var out []string
	seen := map[string]bool{}
	for _, k := range g.order {
		if findColumn(g.all, k) != nil && !seen[k] {
			out = append(out, k)
			seen[k] = true
		}
	}
	for _, c := range g.all {
		if !seen[c.Key] {
			out = append(out, c.Key)
		}
	}
	return out
}

func (d *columnsDialog) isHidden(key string) bool {
	if h, ok := d.g.hidden[key]; ok {
		return h
	}
	c := findColumn(d.g.all, key)
	return c != nil && c.Hidden
}

func (d *columnsDialog) Build() js.Value {
	box, body := dialogBox("Columns — "+d.g.opts.ID, "columns")
	d.list = E("div", "collist")
	Add(body, d.list, ET("div", "muted", "Space shows/hides · ⇧↑⇧↓ or [ ] moves · + − width · r resets"))
	d.scope.On(d.list, "mousedown", func(ev js.Value) {
		if c := Closest(ev.Get("target"), "[data-i]"); c.Truthy() {
			ev.Call("preventDefault")
			d.sel, _ = strconv.Atoi(DataOf(c, "i"))
			if Closest(ev.Get("target"), ".cbx").Truthy() {
				d.toggle()
			}
			d.fill()
		}
	})
	d.fill()
	return box
}

func (d *columnsDialog) fill() {
	Clear(d.list)
	for i, k := range d.keys() {
		c := findColumn(d.g.all, k)
		cls := "choice"
		if i == d.sel {
			cls += " sel"
		}
		el := E("div", cls)
		Data(el, "i", strconv.Itoa(i))
		mark := "☑"
		if d.isHidden(k) {
			mark = "☐"
		}
		Add(el, ET("span", "cbx", mark), ET("span", "lbl", c.Title), ET("span", "det", fmt.Sprintf("%.0f px", colPx(c, d.g.widths))))
		Add(d.list, el)
	}
}

func (d *columnsDialog) toggle() {
	keys := d.keys()
	if d.sel < len(keys) {
		k := keys[d.sel]
		d.g.hidden[k] = !d.isHidden(k)
	}
	d.apply()
}

func (d *columnsDialog) apply() {
	d.g.ColumnsChanged()
	d.g.saveLayout()
	d.g.app.Invalidate()
}

func (d *columnsDialog) Key(k Key) bool {
	keys := d.keys()
	switch {
	case k.Is("ArrowDown"):
		d.sel = min(d.sel+1, len(keys)-1)
	case k.Is("ArrowUp"):
		d.sel = max(d.sel-1, 0)
	case k.Is(" ", "Enter"):
		d.toggle()
	case k.Is("S-ArrowUp", "["), k.Is("S-ArrowDown", "]"):
		dir := 1
		if k.Is("S-ArrowUp", "[") {
			dir = -1
		}
		j := d.sel + dir
		if j >= 0 && j < len(keys) {
			keys[d.sel], keys[j] = keys[j], keys[d.sel]
			d.g.order = keys
			d.sel = j
			d.apply()
		}
	case k.Is("+", "="), k.Is("-"):
		c := findColumn(d.g.all, keys[d.sel])
		w := colPx(c, d.g.widths)
		if k.Is("-") {
			w -= 10
		} else {
			w += 10
		}
		d.g.widths[c.Key] = max(36, w)
		d.apply()
	case k.Is("r"):
		d.g.order, d.g.hidden, d.g.widths = nil, map[string]bool{}, map[string]float64{}
		d.apply()
	default:
		return false
	}
	d.fill()
	return true
}

func (d *columnsDialog) AfterOpen() {}
func (d *columnsDialog) Hints() []hint {
	return []hint{{"Space", "Show/hide", ""}, {"[ ]", "Move", ""}, {"+ −", "Width", ""}, {"r", "Reset", ""}, {"Esc", "Done", ""}}
}
func (d *columnsDialog) Destroy() { d.scope.Release() }

type sortDialog struct {
	g     *Grid
	rows  [3][2]js.Value
	scope Scope
}

func newSortDialog(g *Grid) Dialog { return &sortDialog{g: g} }

func (d *sortDialog) Build() js.Value {
	box, body := dialogBox("Sort", "sort")
	for i := 0; i < 3; i++ {
		row := E("div", "frow")
		sel := E("select", "ed")
		Add(sel, ET("option", "", "(none)"))
		for _, c := range d.g.all {
			o := ET("option", "", c.Title)
			o.Set("value", c.Key)
			Add(sel, o)
		}
		dir := E("select", "ed")
		Add(dir, ET("option", "", "Ascending ▲"), ET("option", "", "Descending ▼"))
		if i < len(d.g.sort) {
			sel.Set("value", d.g.sort[i].Key)
			if d.g.sort[i].Desc {
				dir.Set("selectedIndex", 1)
			}
		}
		lbl := "Then by"
		if i == 0 {
			lbl = "Sort by"
		}
		Add(row, ET("span", "flabel", lbl), sel, dir)
		Add(body, row)
		d.rows[i] = [2]js.Value{sel, dir}
	}
	foot := E("div", "dfoot")
	ok := ET("button", "btn primary", "Apply")
	Add(foot, E("span", "spacer"), ok)
	Add(box, foot)
	d.scope.On(ok, "click", func(js.Value) { d.apply() })
	return box
}

func (d *sortDialog) apply() {
	var spec []core.SortSpec
	for _, r := range d.rows {
		k := r[0].Get("value").String()
		if k != "" && k != "(none)" {
			spec = append(spec, core.SortSpec{Key: k, Desc: r[1].Get("selectedIndex").Int() == 1})
		}
	}
	d.g.SetSort(spec)
	d.g.app.CloseDialog()
}

func (d *sortDialog) AfterOpen() { d.rows[0][0].Call("focus") }
func (d *sortDialog) Key(k Key) bool {
	if k.Is("C-s", "C-Enter") {
		d.apply()
		return true
	}
	return false
}
func (d *sortDialog) Hints() []hint {
	return []hint{{"Tab", "Next", ""}, {"↑↓", "Change", ""}, {"^S", "Apply", ""}, {"Esc", "Cancel", ""}}
}
func (d *sortDialog) Destroy() { d.scope.Release() }

// ---- context menu ---------------------------------------------------------------------------------

type ctxItem struct {
	label, key string
	run        func()
	enabled    bool
}

type ContextMenu struct {
	x, y  float64
	items []ctxItem
	sel   int
	built bool
}

var ctxScope Scope
var ctxEl js.Value
var ctxShown *ContextMenu

func (c *ContextMenu) move(d int) {
	for range c.items {
		c.sel = (c.sel + d + len(c.items)) % len(c.items)
		if it := c.items[c.sel]; it.label != "-" && it.enabled {
			return
		}
	}
}

func (c *ContextMenu) Key(k Key) bool {
	switch {
	case k.Is("Escape"):
		app.ctx = nil
	case k.Is("ArrowDown"):
		c.move(1)
	case k.Is("ArrowUp"):
		c.move(-1)
	case k.Is("Enter", " "):
		it := c.items[c.sel]
		app.ctx = nil
		if it.enabled && it.run != nil {
			it.run()
		}
	default:
		return true
	}
	return true
}

func (a *App) renderContext() {
	if a.ctx == nil {
		if ctxEl.Truthy() {
			ctxEl.Call("remove")
			ctxEl = js.Value{}
			ctxScope.Release()
		}
		ctxShown = nil
		return
	}
	if ctxShown == a.ctx && ctxEl.Truthy() {
		// update selection only
		kids := ctxEl.Get("children")
		for i := 0; i < kids.Length(); i++ {
			el := kids.Index(i)
			idx, _ := strconv.Atoi(DataOf(el, "i"))
			if DataOf(el, "i") == "" {
				continue
			}
			cls := "item"
			if !a.ctx.items[idx].enabled {
				cls += " disabled"
			}
			if idx == a.ctx.sel {
				cls += " sel"
			}
			el.Set("className", cls)
		}
		return
	}
	if ctxEl.Truthy() {
		ctxEl.Call("remove")
		ctxScope.Release()
	}
	c := a.ctx
	if !c.items[c.sel].enabled {
		c.move(1)
	}
	ctxShown = c
	el := E("div", "drop ctxmenu")
	for i, it := range c.items {
		if it.label == "-" {
			Add(el, E("div", "sep"))
			continue
		}
		cls := "item"
		if !it.enabled {
			cls += " disabled"
		}
		if i == c.sel {
			cls += " sel"
		}
		row := E("div", cls)
		Data(row, "i", strconv.Itoa(i))
		Add(row, ET("span", "chk", ""), ET("span", "il", it.label), ET("span", "ik", it.key))
		Add(el, row)
	}
	Style(el, "position", "fixed")
	Style(el, "left", Px(c.x))
	Style(el, "top", Px(c.y))
	Add(a.popLayer, el)
	// keep it on screen
	h := el.Get("offsetHeight").Float()
	if c.y+h > global.Get("innerHeight").Float()-8 {
		Style(el, "top", Px(max(8, c.y-h)))
	}
	ctxEl = el
	ctxScope.On(el, "mousedown", func(ev js.Value) {
		ev.Call("preventDefault")
		if r := Closest(ev.Get("target"), "[data-i]"); r.Truthy() {
			i, _ := strconv.Atoi(DataOf(r, "i"))
			it := c.items[i]
			a.ctx = nil
			if it.enabled && it.run != nil {
				it.run()
			}
			a.Invalidate()
		}
	})
}
