package tui

// Key and mouse routing, and every action: navigation, editing, clipboard,
// fill-down, delete, undo/redo, views, panes.

import (
	"context"
	"fmt"
	"strings"
	"time"

	tea "charm.land/bubbletea/v2"

	"git.wharf.fluidgrid.org/jcz/tictactoe/go/internal/pt/client"
	"git.wharf.fluidgrid.org/jcz/tictactoe/go/internal/pt/core"
)

func (a *App) onKey(k tea.KeyPressMsg) tea.Cmd {
	key := k.Keystroke()
	if key == "ctrl+c" && a.ph != phMain {
		a.quitting = true
		return tea.Quit
	}
	switch a.ph {
	case phLogin:
		if key == "ctrl+q" {
			a.quitting = true
			return tea.Quit
		}
		return a.login.key(a, k)
	case phConnecting:
		if key == "ctrl+q" || key == "esc" {
			a.quitting = true
			return tea.Quit
		}
		return nil
	}
	if key == "ctrl+q" && len(a.overlays) == 0 {
		a.quitting = true
		return tea.Quit
	}
	if o := a.top(); o != nil {
		return o.key(a, k)
	}
	if a.editor != nil {
		return a.editorKey(k)
	}
	if a.filterOn {
		return a.filterKey(k)
	}
	// Alt+letter opens a menu
	if strings.HasPrefix(key, "alt+") && len(key) == 5 {
		for i, m := range menuBar {
			if rune(key[4]) == m.accel {
				a.openMenu(i)
				return nil
			}
		}
	}
	if ac := lookup(a.keyCtx(), keyName(k)); ac != ANone {
		return a.do(ac)
	}
	switch a.view {
	case vBoard:
		return a.board.key(a, k)
	case vTimeline:
		return a.tl.key(a, k)
	case vDashboard:
		return a.dash.key(a, k)
	}
	return a.gridKey(k)
}

func (a *App) setView(v int) {
	if v == a.view {
		return
	}
	a.view = v
	a.pane = 0
	a.filterOn = false
	a.editor = nil
	a.rebuild()
}

// do runs an action (keys, menus, palette and hint clicks all land here).
func (a *App) do(ac Action) tea.Cmd {
	g := a.activeGrid()
	switch ac {
	case AHelp:
		a.push(&helpDlg{filter: NewInput(IText, "search")})
	case AMenu:
		a.openMenu(0)
	case APalette:
		a.palette()
	case AFilter:
		if a.showQuery() {
			a.filterOn = true
			a.updateSuggestions()
		}
	case AClearFilter:
		a.queries[a.view].in.SetText("")
		a.rebuild()
	case ASaveFilter:
		a.promptDialog("Save filter", "Name", "", func(a *App, name string) tea.Cmd {
			sf := savedFilter{Name: name, View: a.view, Query: a.queries[a.view].in.Text()}
			if g := a.activeGrid(); g != nil {
				sf.Sort, sf.Group = g.Sort, g.GroupBy
			}
			a.saved = append(a.saved, sf)
			if err := storeSaved(a.saved); err != nil {
				a.toast("Couldn't save: "+err.Error(), 3)
			} else {
				a.toast("Saved filter ★ "+name, 1)
			}
			return nil
		})
	case ANextView:
		a.setView((a.view + 1) % len(viewNames))
	case APrevView:
		a.setView((a.view + len(viewNames) - 1) % len(viewNames))
	case AView1, AView2, AView3, AView4, AView5, AView6, AView7, AView8, AView9:
		a.setView(int(ac - AView1))
	case ANextPane, APrevPane:
		if a.hasPanes() {
			a.pane = 1 - a.pane
		} else if a.showQuery() {
			a.filterOn = !a.filterOn
		}
	case AUndo, ARedo:
		if !a.online() {
			a.toast("Offline", 3)
			return nil
		}
		a.req++
		label := "Undone"
		var msg any = core.UndoMsg{Req: a.req}
		if ac == ARedo {
			label, msg = "Redone", core.RedoMsg{Req: a.req}
		}
		a.pending[a.req] = &pendingReq{label: label}
		return a.send(msg)
	case ANew:
		return a.newRow()
	case ANewProject:
		if a.rep.CanEdit() {
			a.newProjectDialog()
		}
	case ANewTask:
		if a.rep.CanEdit() {
			a.newTaskDialog(a.contextProject(), "")
		}
	case AEditRecord:
		switch a.view {
		case vBoard:
			if id := a.board.curID(a); id != 0 {
				a.recordForm(core.EntTask, id)
			}
		case vTimeline:
			if row, ok := a.tl.cur(a); ok {
				a.recordForm(row.ent, row.id)
			}
		default:
			if g != nil && g.CurID() != 0 && g.Name != "activity" {
				a.recordForm(g.Ent, g.CurID())
			} else if g != nil && g.Name == "activity" {
				return a.jumpToActivity()
			}
		}
	case ADelete:
		return a.deleteRows()
	case AToggleDetail:
		a.detail[a.view] = !a.detail[a.view]
		if !a.detail[a.view] {
			a.pane = 0
		}
		a.rebuild()
	case AColumns:
		if g != nil {
			a.push(&columnsDlg{g: g})
		}
	case ASortColumn:
		if g != nil {
			if c, ok := g.CurCol(); ok {
				g.CycleSort(c.Key, false)
				a.rebuild()
			}
		}
	case ASortDialog:
		if g != nil {
			a.sortDialog(g)
		}
	case AGroupColumn:
		if g != nil {
			if c, ok := g.CurCol(); ok {
				if g.GroupBy == c.Key {
					g.GroupBy = ""
				} else {
					g.GroupBy = c.Key
					g.Collapsed = map[string]bool{}
				}
				a.rebuild()
			}
		}
	case AClearGroup:
		if g != nil {
			g.GroupBy = ""
			a.rebuild()
		}
	case ATheme:
		a.theme = (a.theme + 1) % len(themes)
		a.setStatus("Theme: "+themes[a.theme].Name, 0)
	case ARedraw:
		return tea.ClearScreen
	case AQuit:
		a.quitting = true
		return tea.Quit
	case ALogout:
		return a.logout()
	case AContextMenu:
		x, y := 4, 6
		if g != nil {
			if r, ok := g.CellRect(); ok {
				x, y = r.X, r.Y+1
			}
		}
		a.contextMenu(x, y)
	case ACopy:
		return a.copy()
	case APaste:
		if a.clip != "" {
			return a.onPaste(a.clip)
		}
		return tea.ReadClipboard
	case AFillDown:
		return a.fillDown()
	case AMarkAll:
		if g != nil {
			if len(g.Marked) > 0 {
				g.Marked = map[int64]bool{}
			} else {
				for _, row := range g.Rows {
					if !row.IsGroup {
						g.Marked[row.ID] = true
					}
				}
			}
		}
	case AMark:
		if g != nil {
			if g.ToggleGroup(nil) {
				a.rebuild()
				return nil
			}
			if id := g.CurID(); id != 0 {
				if g.Marked[id] {
					delete(g.Marked, id)
				} else {
					g.Marked[id] = true
				}
				g.anchor = g.Row
				g.Move(1, 0)
				a.rebuildDetail()
			}
		}
	case AClear:
		return a.clearCell()
	case AEdit:
		return a.startEdit("", false)
	case ASetPassword:
		if g != nil && g.Name == "users" {
			a.setPasswordDialog(g.CurID())
		} else if a.rep.Me != nil {
			a.setPasswordDialog(a.rep.Me.ID)
		}
	case AAbout:
		a.about()
	default:
		switch a.view {
		case vBoard:
			return a.board.action(a, ac)
		case vTimeline:
			return a.tl.action(a, ac)
		case vDashboard:
			return a.dash.action(a, ac)
		}
	}
	return nil
}

func (a *App) about() {
	r := a.rep
	f := newForm("About pt", 64)
	f.buttons = []string{"Close"}
	srv := "—"
	if r.Server != nil {
		srv = r.Server.Impl + " " + r.Server.Version
	}
	lines := []string{
		"pt — projects & tasks",
		"Client: Go TUI (Bubble Tea v2) " + core.Version,
		"Server: " + srv + " at " + a.opts.Server,
		fmt.Sprintf("Protocol: v%d · %d projects · %d tasks · %d users", core.ProtocolVersion, len(r.Projects), len(r.Tasks), len(r.Users)),
		"Colours: " + choose(a.truecolor, "24-bit", "xterm-256"),
	}
	f.extraH = len(lines)
	f.extra = func(a *App, f *form, b *Buf, rr Rect) {
		t := a.th()
		for i, l := range lines {
			st := Sty{FG: t.FG, BG: t.Panel}
			if i == 0 {
				st = st.Fg(t.Accent).With(Bold)
			}
			b.Text(rr.X, rr.Y+i, l, st, rr.W)
		}
	}
	a.push(f)
}

func (a *App) logout() tea.Cmd {
	cl := a.cl
	_ = client.ForgetToken(a.opts.Server)
	a.cancel()
	a.ctx, a.cancel = context.WithCancel(context.Background())
	a.sess = nil
	a.rep = core.NewReplica(todayLocal())
	a.grids = map[string]*Grid{}
	a.setupViews()
	a.overlays = nil
	a.editor = nil
	a.ph = phLogin
	a.loginErr = ""
	a.login = newLoginForm(a)
	return func() tea.Msg {
		ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
		defer cancel()
		_ = cl.Logout(ctx)
		return logoutMsg{}
	}
}

// contextProject is the project the cursor is "in" (for New task).
func (a *App) contextProject() int64 {
	r := a.rep
	switch a.view {
	case vProjects:
		return a.grids["projects"].CurID()
	case vTasks:
		if t := r.Tasks[a.grids["tasks"].CurID()]; t != nil {
			return t.ProjectID
		}
	case vBoard:
		if a.board.project != nil {
			return *a.board.project
		}
		if t := r.Tasks[a.board.curID(a)]; t != nil {
			return t.ProjectID
		}
	case vTimeline:
		if row, ok := a.tl.cur(a); ok {
			if row.ent == core.EntProject {
				return row.id
			}
			if t := r.Tasks[row.id]; t != nil {
				return t.ProjectID
			}
		}
	}
	return 0
}

func (a *App) newRow() tea.Cmd {
	if !a.rep.CanEdit() {
		a.toast("You have read-only access (viewer)", 2)
		return nil
	}
	admin := a.rep.IsAdmin()
	needAdmin := func() bool {
		if !admin {
			a.toast("Only admins can add these", 2)
		}
		return admin
	}
	switch a.view {
	case vBoard:
		a.newTaskDialog(a.contextProject(), a.board.curStatus())
		return nil
	case vTimeline:
		a.newTaskDialog(a.contextProject(), "")
		return nil
	}
	g := a.activeGrid()
	if g == nil {
		return nil
	}
	switch g.Name {
	case "projects":
		a.newProjectDialog()
	case "projtasks":
		a.newTaskDialog(a.grids["projects"].CurID(), "")
	case "tasks":
		a.newTaskDialog(a.contextProject(), "")
	case "templates":
		if needAdmin() {
			a.simpleCreate("New template", core.EntTemplate, []fdef{{"name", "Name", core.EdText, true}, {"description", "Description", core.EdText, false}}, nil,
				func(a *App, id int64) { a.reveal(vTemplates, "templates", id) })
		}
	case "tmpltasks":
		if needAdmin() {
			tid := a.grids["templates"].CurID()
			if tid == 0 {
				return nil
			}
			a.simpleCreate("New template task", core.EntTemplateTask, []fdef{
				{"title", "Title", core.EdText, true}, {"phase", "Phase", core.EdSuggest, false}, {"est_hours", "Est h", core.EdNumber, false},
				{"offset_days", "Offset d", core.EdInt, false}, {"duration_days", "Duration d", core.EdInt, false},
			}, map[string]any{"template_id": tid}, nil)
		}
	case "fields":
		if needAdmin() {
			a.simpleCreate("New custom field", core.EntCustomField, []fdef{
				{"entity", "Applies to", core.EdSelect, true}, {"name", "Name", core.EdText, true}, {"kind", "Kind", core.EdSelect, true},
				{"options", "Options", core.EdChips, false},
			}, nil, nil)
		}
	case "users":
		if needAdmin() {
			a.newUserDialog()
		}
	case "programs":
		if needAdmin() {
			a.simpleCreate("New program", core.EntProgram, []fdef{
				{"name", "Name", core.EdText, true}, {"client", "Client", core.EdText, false}, {"prefix", "Code prefix", core.EdText, false},
				{"color", "Colour", core.EdText, false},
			}, map[string]any{"color": "#5aa9ff"}, nil)
		}
	}
	return nil
}

func (a *App) deleteRows() tea.Cmd {
	var ent core.Entity
	var ids []int64
	switch a.view {
	case vBoard:
		ent, ids = core.EntTask, []int64{a.board.curID(a)}
	case vTimeline:
		if row, ok := a.tl.cur(a); ok {
			ent, ids = row.ent, []int64{row.id}
		}
	default:
		g := a.activeGrid()
		if g == nil || g.ReadOnly {
			return nil
		}
		ent, ids = g.Ent, g.MarkedIDs()
	}
	if len(ids) == 0 || ids[0] == 0 {
		return nil
	}
	if ent == core.EntUser {
		a.toast("Users can't be deleted — set Active to no instead", 2)
		return nil
	}
	if !a.rep.CanEdit() {
		a.toast("You have read-only access (viewer)", 2)
		return nil
	}
	what := fmt.Sprintf("%d %ss", len(ids), ent.Label())
	if len(ids) == 1 {
		what = ent.Label() + " " + a.rep.RowLabel(ent, ids[0])
	}
	text := "Delete " + what + "?"
	if ent == core.EntProject {
		n := 0
		for _, id := range ids {
			n += len(a.rep.ProjectTasks(id))
		}
		text += fmt.Sprintf(" This also deletes %d tasks.", n)
	}
	if ent == core.EntProject || ent == core.EntTask {
		text += " You can undo with ^Z."
	} else {
		text += " This can't be undone."
	}
	a.push(&confirmDlg{title: "Delete", text: text, yes: "Delete", onYes: func(a *App) tea.Cmd {
		var ops []core.Op
		for _, id := range ids {
			ops = append(ops, core.DeleteOp(ent, id))
		}
		if g := a.activeGrid(); g != nil {
			g.Marked = map[int64]bool{}
		}
		return a.mutate(ops, nil, "Deleted "+what, nil)
	}})
	return nil
}

func (a *App) jumpToActivity() tea.Cmd {
	ac := a.rep.Activity[a.grids["activity"].CurID()]
	if ac == nil {
		return nil
	}
	switch ac.Entity {
	case "project":
		if a.rep.Projects[ac.EntityID] != nil {
			a.reveal(vProjects, "projects", ac.EntityID)
		}
	case "task":
		if a.rep.Tasks[ac.EntityID] != nil {
			a.reveal(vTasks, "tasks", ac.EntityID)
		}
	}
	return nil
}

// ---- grid keys ----

func (a *App) gridKey(k tea.KeyPressMsg) tea.Cmd {
	g := a.activeGrid()
	if g == nil {
		return nil
	}
	key := k.Keystroke()
	page := max(1, bodyRows(g.rect)-1)
	moved := true
	switch key {
	case "up":
		g.Move(-1, 0)
	case "down":
		g.Move(1, 0)
	case "left":
		if g.Row < len(g.Rows) && g.Rows[g.Row].IsGroup {
			c := true
			g.ToggleGroup(&c)
			a.rebuild()
		} else {
			g.Move(0, -1)
		}
	case "right":
		if g.Row < len(g.Rows) && g.Rows[g.Row].IsGroup {
			c := false
			g.ToggleGroup(&c)
			a.rebuild()
		} else {
			g.Move(0, 1)
		}
	case "tab":
		g.Next(false)
	case "shift+tab":
		g.Next(true)
	case "home":
		g.Col = 0
	case "end":
		g.Col = len(g.vis()) - 1
	case "ctrl+home":
		g.Row = 0
	case "ctrl+end":
		g.Row = len(g.Rows) - 1
	case "pgup":
		g.Move(-page, 0)
	case "pgdown":
		g.Move(page, 0)
	case "shift+up", "shift+down":
		if id := g.CurID(); id != 0 {
			g.Marked[id] = true
		}
		if key == "shift+up" {
			g.Move(-1, 0)
		} else {
			g.Move(1, 0)
		}
		if id := g.CurID(); id != 0 {
			g.Marked[id] = true
		}
	case "enter":
		if g.ToggleGroup(nil) {
			a.rebuild()
			return nil
		}
		if g.Name == "activity" {
			return a.jumpToActivity()
		}
		return a.startEdit("", false)
	case "backspace":
		return a.startEdit("", true)
	case "esc":
		g.Marked = map[int64]bool{}
		a.setStatus("", 0)
	default:
		moved = false
		if k.Text != "" && k.Mod&(tea.ModCtrl|tea.ModAlt) == 0 {
			return a.startEdit(k.Text, true)
		}
	}
	if moved {
		a.rebuildDetail()
	}
	return nil
}

// ---- cell editing ----

func (a *App) canEditGrid(g *Grid) (core.Column, int64, bool) {
	col, ok := g.CurCol()
	id := g.CurID()
	if !ok || id == 0 {
		return col, 0, false
	}
	if g.ReadOnly || !col.Editable() {
		a.setStatus("ℹ "+col.Title+" is calculated, not editable", 0)
		return col, id, false
	}
	if !a.rep.CanEdit() {
		a.toast("You have read-only access (viewer)", 2)
		return col, id, false
	}
	if m := core.Field(g.Ent, col.Field); m != nil && m.Perm == core.PermAdmin && !a.rep.IsAdmin() {
		a.toast("Only admins can change "+col.Title, 2)
		return col, id, false
	}
	if !a.online() {
		a.toast("Offline: edits are disabled", 3)
		return col, id, false
	}
	return col, id, true
}

// startEdit opens the editor for the current cell. seed replaces the text
// (typing starts editing); replace with an empty seed clears it (Backspace).
func (a *App) startEdit(seed string, replace bool) tea.Cmd {
	g := a.activeGrid()
	if g == nil {
		return nil
	}
	col, id, ok := a.canEditGrid(g)
	if !ok {
		return nil
	}
	key := cellKey{g.Ent, id, col.Field}
	// on a conflict cell, Enter edits again starting from their value
	delete(a.conflicts, key)
	var hint *int64
	if t := a.rep.Tasks[id]; g.Ent == core.EntTask && t != nil {
		pid := t.ProjectID
		hint = &pid
	}
	in := a.fieldInput(g.Ent, col.Field, col.Title, col.Editor, id, hint)
	base := a.rep.FieldValue(g.Ent, id, col.Field)
	switch in.Kind {
	case IBool:
		b, _ := base.(bool)
		return a.mutate([]core.Op{core.SetOp(g.Ent, id, col.Field, base, !b)}, []cellKey{key}, col.Title+" changed", nil)
	case IArea:
		a.openNotes(g.Ent, id, col.Field)
		return nil
	}
	if in.Kind.isList() {
		a.setInputValue(in, base)
		if in.Kind == ISuggest {
			in.SetText(core.EditText(a.rep, g.Ent, col.Field, id))
		}
		if replace {
			in.SetText(seed)
		}
	} else {
		in.SetText(core.EditText(a.rep, g.Ent, col.Field, id))
		if replace {
			in.SetText(seed)
		}
	}
	in.SetToday(a.rep.Today)
	in.refilter()
	a.editor = &cellEdit{in: in, g: g, key: key, base: base}
	return nil
}

func (a *App) editorKey(k tea.KeyPressMsg) tea.Cmd {
	e := a.editor
	res := e.in.Key(k)
	switch res {
	case RCancel:
		a.editor = nil
	case RCommit, RCommitNext, RCommitPrev:
		return a.commitEdit(res)
	case RIgnored:
		if k.Keystroke() == "ctrl+s" {
			return a.commitEdit(RCommit)
		}
	}
	return nil
}

func (a *App) commitEdit(res InputResult) tea.Cmd {
	e := a.editor
	in := e.in
	g := e.g
	if name, ok := in.CreateRequested(); ok {
		a.editor = nil
		return a.createProgramThenSet(e, name)
	}
	if in.Kind.isList() && in.Kind != IChips && in.Kind != ISuggest {
		if c, ok := in.selected(); ok && (in.navigated || len(in.text) > 0 && !in.matchesValue()) {
			in.SetValue(c.Value, in.allChoices())
			in.navigated = false
		}
	}
	v, err := a.inputValue(in)
	if err != nil {
		in.Err = err.Error()
		return nil
	}
	a.editor = nil
	var cmd tea.Cmd
	if !core.ValuesEqual(v, e.base) {
		cmd = a.mutate([]core.Op{core.SetOp(e.key.E, e.key.ID, e.key.F, e.base, v)}, []cellKey{e.key}, in.Label+" saved", nil)
	}
	switch res {
	case RCommit:
		g.Move(1, 0)
	case RCommitNext:
		g.Next(false)
	case RCommitPrev:
		g.Next(true)
	}
	a.rebuildDetail()
	return cmd
}

func (a *App) createProgramThenSet(e *cellEdit, name string) tea.Cmd {
	prefix := ""
	for _, w := range strings.Fields(strings.ToUpper(name)) {
		if len(prefix) < 3 && w[0] >= 'A' && w[0] <= 'Z' {
			prefix += w[:1]
		}
	}
	return a.mutate([]core.Op{core.CreateOp(core.EntProgram, "p", map[string]any{"name": name, "prefix": prefix})}, nil, "Created program "+name,
		func(a *App, res core.ResultMsg) tea.Cmd {
			id, ok := res.Created["p"]
			if !ok {
				return nil
			}
			return a.mutate([]core.Op{core.SetOp(e.key.E, e.key.ID, e.key.F, e.base, id)}, []cellKey{e.key}, "Program set", nil)
		})
}

func (a *App) clearCell() tea.Cmd {
	g := a.activeGrid()
	if g == nil {
		return nil
	}
	col, id, ok := a.canEditGrid(g)
	if !ok {
		return nil
	}
	v, err := core.ParseInput(a.rep, g.Ent, col.Field, id, "")
	if err != nil {
		a.toast(err.Error(), 3)
		return nil
	}
	base := a.rep.FieldValue(g.Ent, id, col.Field)
	if core.ValuesEqual(base, v) {
		return nil
	}
	return a.mutate([]core.Op{core.SetOp(g.Ent, id, col.Field, base, v)}, []cellKey{{g.Ent, id, col.Field}}, col.Title+" cleared", nil)
}

// ---- clipboard & bulk edits ----

// copyText is what a cell copies as: editable fields round-trip via EditText.
func (a *App) copyText(g *Grid, c core.Column, id int64) string {
	if c.Field != "" {
		return strings.ReplaceAll(core.EditText(a.rep, g.Ent, c.Field, id), "\n", " ")
	}
	cell := core.CellFor(a.rep, &c, id)
	return cell.Text
}

func (a *App) copy() tea.Cmd {
	g := a.activeGrid()
	if g == nil {
		return nil
	}
	var sb strings.Builder
	n := 0
	if len(g.Marked) > 0 {
		vis := g.vis()
		for _, id := range g.MarkedIDs() {
			cells := make([]string, len(vis))
			for i, c := range vis {
				cells[i] = a.copyText(g, c, id)
			}
			sb.WriteString(strings.Join(cells, "\t") + "\n")
			n++
		}
	} else if c, ok := g.CurCol(); ok && g.CurID() != 0 {
		sb.WriteString(a.copyText(g, c, g.CurID()))
		n = 1
	}
	if n == 0 {
		return nil
	}
	a.clip = sb.String()
	if n == 1 && len(g.Marked) == 0 {
		a.toast("Copied cell", 1)
	} else {
		a.toast(fmt.Sprintf("Copied %d rows as TSV", n), 1)
	}
	return tea.SetClipboard(a.clip)
}

func (a *App) onPaste(s string) tea.Cmd {
	if a.ph == phLogin {
		a.login.paste(a, s)
		return nil
	}
	if o := a.top(); o != nil {
		o.paste(a, s)
		return nil
	}
	if a.editor != nil {
		a.editor.in.Insert(s)
		return nil
	}
	if a.filterOn {
		a.queries[a.view].in.Insert(s)
		a.rebuild()
		return nil
	}
	return a.pasteGrid(s)
}

// pasteGrid pastes TSV at the cursor, filling right and down (one undo step).
// A single value pasted onto marked rows fills them all.
func (a *App) pasteGrid(s string) tea.Cmd {
	g := a.activeGrid()
	if g == nil || g.ReadOnly {
		return nil
	}
	if !a.rep.CanEdit() {
		a.toast("You have read-only access (viewer)", 2)
		return nil
	}
	s = strings.TrimRight(strings.ReplaceAll(s, "\r\n", "\n"), "\n")
	lines := strings.Split(s, "\n")
	grid := make([][]string, len(lines))
	for i, l := range lines {
		grid[i] = strings.Split(l, "\t")
	}
	vis := g.vis()
	// target rows: data rows from the cursor down
	var rows []int64
	if len(grid) == 1 && len(grid[0]) == 1 && len(g.Marked) > 1 {
		rows = g.MarkedIDs()
	} else {
		for i := g.Row; i < len(g.Rows) && len(rows) < len(grid); i++ {
			if !g.Rows[i].IsGroup {
				rows = append(rows, g.Rows[i].ID)
			}
		}
	}
	var ops []core.Op
	var cells []cellKey
	var errs []string
	for ri, id := range rows {
		src := grid[min(ri, len(grid)-1)]
		for ci, text := range src {
			vi := g.Col + ci
			if vi >= len(vis) {
				break
			}
			c := vis[vi]
			if !c.Editable() {
				continue
			}
			v, err := core.ParseInput(a.rep, g.Ent, c.Field, id, text)
			if err != nil {
				errs = append(errs, c.Title+": "+err.Error())
				continue
			}
			base := a.rep.FieldValue(g.Ent, id, c.Field)
			if core.ValuesEqual(base, v) {
				continue
			}
			ops = append(ops, core.SetOp(g.Ent, id, c.Field, base, v))
			cells = append(cells, cellKey{g.Ent, id, c.Field})
		}
	}
	if len(errs) > 0 {
		a.toast(fmt.Sprintf("%d value(s) skipped: %s", len(errs), errs[0]), 2)
	}
	if len(ops) == 0 {
		if len(errs) == 0 {
			a.setStatus("Nothing to paste (values unchanged)", 0)
		}
		return nil
	}
	return a.mutate(ops, cells, fmt.Sprintf("Pasted %d cell(s)", len(ops)), nil)
}

// fillDown copies the first marked row's value into the other marked rows,
// or (nothing marked) the value from the row above.
func (a *App) fillDown() tea.Cmd {
	g := a.activeGrid()
	if g == nil {
		return nil
	}
	col, id, ok := a.canEditGrid(g)
	if !ok {
		return nil
	}
	var src int64
	var targets []int64
	marked := g.MarkedIDs()
	if len(g.Marked) >= 2 {
		src, targets = marked[0], marked[1:]
	} else {
		for i := g.Row - 1; i >= 0; i-- {
			if !g.Rows[i].IsGroup {
				src = g.Rows[i].ID
				break
			}
		}
		targets = []int64{id}
	}
	if src == 0 {
		return nil
	}
	v := a.rep.FieldValue(g.Ent, src, col.Field)
	if col.Field == "deps" {
		a.toast("Dependencies can't be filled down", 2)
		return nil
	}
	var ops []core.Op
	var cells []cellKey
	for _, t := range targets {
		base := a.rep.FieldValue(g.Ent, t, col.Field)
		if !core.ValuesEqual(base, v) {
			ops = append(ops, core.SetOp(g.Ent, t, col.Field, base, v))
			cells = append(cells, cellKey{g.Ent, t, col.Field})
		}
	}
	if len(ops) == 0 {
		return nil
	}
	return a.mutate(ops, cells, fmt.Sprintf("Filled %d cell(s)", len(ops)), nil)
}

// ---- filter bar ----

func (a *App) updateSuggestions() {
	qb := a.queries[a.view]
	qb.sugg = a.querySuggestions(qb)
	qb.sel = 0
}

func (a *App) filterKey(k tea.KeyPressMsg) tea.Cmd {
	qb := a.queries[a.view]
	key := k.Keystroke()
	switch key {
	case "esc", "enter", "f6", "ctrl+f", "f3":
		if key == "enter" && len(qb.sugg) > 0 && qb.sel > 0 {
			qb.acceptSuggestion(qb.sugg[qb.sel])
			a.updateSuggestions()
			a.rebuild()
			return nil
		}
		a.filterOn = false
		qb.sugg = nil
		return nil
	case "tab":
		if len(qb.sugg) > 0 {
			qb.acceptSuggestion(qb.sugg[qb.sel])
			a.updateSuggestions()
			a.rebuild()
		}
		return nil
	case "up":
		if qb.sel > 0 {
			qb.sel--
		}
		return nil
	case "down":
		if qb.sel < len(qb.sugg)-1 {
			qb.sel++
		}
		return nil
	case "f1":
		return a.do(AHelp)
	}
	if qb.in.Key(k) == RNone {
		a.updateSuggestions()
		a.rebuild()
	}
	return nil
}

func (a *App) applySaved(s savedFilter) {
	a.setView(s.View)
	a.queries[s.View].in.SetText(s.Query)
	if g := a.activeGrid(); g != nil && s.Sort != nil {
		g.Sort = s.Sort
		g.GroupBy = s.Group
	}
	a.rebuild()
	a.setStatus("★ "+s.Name, 1)
}

// ---- mouse ----

func (a *App) onMouse(m hitMsg) tea.Cmd {
	if a.ph == phLogin {
		return a.login.mouse(a, m)
	}
	if a.ph != phMain {
		return nil
	}
	if o := a.top(); o != nil {
		return o.mouse(a, m)
	}
	// drags in progress
	if a.drag != nil {
		switch ev := m.ev.(type) {
		case tea.MouseMotionMsg:
			a.drag.x, a.drag.y = ev.X, ev.Y
			if abs(ev.X-a.drag.x0)+abs(ev.Y-a.drag.y0) > 0 {
				a.drag.moved = true
			}
			if a.drag.kind == HitColBorder {
				if g := a.activeGrid(); g != nil {
					g.ResizeVisible(a.drag.a, a.drag.b+ev.X-a.drag.x0)
				}
			}
			return nil
		case tea.MouseReleaseMsg:
			d := a.drag
			a.drag = nil
			d.x, d.y = ev.X, ev.Y
			switch d.kind {
			case HitCard:
				return a.board.drop(a, d, m.hits)
			case HitTLBar, HitTLBarEnd:
				return a.tl.drop(a, d)
			}
			return nil
		}
	}
	if a.editor != nil {
		if m.ok && m.hit.S == "editor" {
			if isClick(m) && a.editor.in.Click(m.hit) {
				return a.commitEdit(RNone)
			}
			if d, ok := isWheel(m); ok {
				a.editor.in.Wheel(d)
			}
			return nil
		}
		if _, ok := m.ev.(tea.MouseClickMsg); ok {
			// clicking elsewhere saves the edit, then handles the click
			cmd := a.commitEdit(RNone)
			return tea.Batch(cmd, a.onMouse(m))
		}
		return nil
	}
	if a.filterOn && m.ok && m.hit.S == "query" && isClick(m) {
		qb := a.queries[a.view]
		qb.acceptSuggestion(qb.sugg[m.hit.A])
		a.updateSuggestions()
		a.rebuild()
		return nil
	}
	click, isClk := m.ev.(tea.MouseClickMsg)
	if d, ok := isWheel(m); ok {
		return a.wheel(m, d)
	}
	if !m.ok {
		return nil
	}
	h := m.hit
	if isClk && click.Button == tea.MouseLeft {
		dbl := a.now.Sub(a.lastClick) < 400*time.Millisecond && a.lastClickH.Kind == h.Kind && a.lastClickH.A == h.A && a.lastClickH.B == h.B
		a.lastClick, a.lastClickH = time.Now(), h
		a.now = a.lastClick
		if a.filterOn && h.Kind != HitFilter {
			a.filterOn = false
		}
		switch h.Kind {
		case HitMenuTitle:
			a.openMenu(h.A)
		case HitTab:
			a.setView(h.A)
		case HitHint:
			return a.do(Action(h.A))
		case HitFilter:
			a.filterOn = true
			a.updateSuggestions()
		case HitToast:
			if h.A < len(a.toasts) {
				a.toasts = append(a.toasts[:h.A], a.toasts[h.A+1:]...)
			}
		case HitPane:
			a.pane = h.A
		case HitHeader:
			a.pane = h.Pane
			g := a.grids[h.S]
			v := g.vis()
			if h.A < len(v) {
				g.CycleSort(v[h.A].Key, click.Mod&tea.ModShift != 0)
				a.rebuild()
			}
		case HitColBorder:
			a.pane = h.Pane
			g := a.grids[h.S]
			v := g.vis()
			if h.A < len(v) {
				a.drag = &dragState{kind: HitColBorder, x0: click.X, y0: click.Y, a: h.A, b: v[h.A].Width}
			}
		case HitCell, HitGutter, HitGroup:
			a.pane = h.Pane
			g := a.grids[h.S]
			g.Row = h.A
			if h.Kind == HitCell {
				g.Col = h.B
			}
			a.rebuildDetail()
			switch h.Kind {
			case HitGutter:
				if id := g.CurID(); id != 0 {
					if g.Marked[id] {
						delete(g.Marked, id)
					} else {
						g.Marked[id] = true
					}
				}
			case HitGroup:
				g.ToggleGroup(nil)
				a.rebuild()
			case HitCell:
				if click.Mod&tea.ModShift != 0 {
					if id := g.CurID(); id != 0 {
						g.Marked[id] = true
					}
				}
				if dbl {
					if g.Name == "activity" {
						return a.jumpToActivity()
					}
					return a.startEdit("", false)
				}
			}
		default:
			switch a.view {
			case vBoard:
				return a.board.click(a, h, click, dbl)
			case vTimeline:
				return a.tl.click(a, h, click, dbl)
			case vDashboard:
				return a.dash.click(a, h, dbl)
			}
		}
		return nil
	}
	if isClk && click.Button == tea.MouseRight {
		switch h.Kind {
		case HitCell:
			a.pane = h.Pane
			g := a.grids[h.S]
			g.Row, g.Col = h.A, h.B
			a.rebuildDetail()
			a.contextMenu(click.X, click.Y+1)
		case HitCard, HitTLRow, HitTLBar:
			if h.Kind == HitCard {
				a.board.click(a, h, click, false)
			} else {
				a.tl.click(a, h, click, false)
			}
			a.contextMenu(click.X, click.Y+1)
		}
	}
	return nil
}

func (a *App) wheel(m hitMsg, d int) tea.Cmd {
	switch a.view {
	case vBoard:
		a.board.wheel(a, m, d)
		return nil
	case vTimeline:
		a.tl.wheel(a, m, d)
		return nil
	}
	if m.ok && (m.hit.S != "") {
		if g := a.grids[m.hit.S]; g != nil {
			a.pane = m.hit.Pane
			g.Move(3*d, 0)
			a.rebuildDetail()
		}
	}
	return nil
}

func abs(x int) int {
	if x < 0 {
		return -x
	}
	return x
}
