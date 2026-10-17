package tui

import (
	"fmt"
	"strings"
	"testing"

	tea "charm.land/bubbletea/v2"

	"git.wharf.fluidgrid.org/jcz/tictactoe/go/internal/pt/core"
)

func TestTextEditCommitsAndMovesDown(t *testing.T) {
	a := testApp(t, 160, 45, "alice")
	g := a.grids["projects"]
	a.gotoCol(t, "name")
	id := g.CurID()
	old := a.rep.Projects[id].Name
	a.press("enter")
	if a.editor == nil {
		t.Fatal("editor did not open")
	}
	a.press("ctrl+u")
	a.typeText("Renamed project")
	a.press("enter")
	if a.editor != nil {
		t.Fatal("editor still open")
	}
	ops := a.lastOps(t)
	if len(ops) != 1 || ops[0].Field != "name" || ops[0].Value != "Renamed project" || ops[0].Base != old || ops[0].ID != id {
		t.Fatalf("bad op: %+v", ops)
	}
	if g.Row != 1 {
		t.Errorf("cursor should move down, row=%d", g.Row)
	}
	if !a.pendCells[cellKey{core.EntProject, id, "name"}] {
		t.Error("cell should be pending")
	}
	// the pending cell renders italic/dim until the result arrives
	a.onServerMsg(core.ResultMsg{Req: a.req, OK: true})
	if len(a.pendCells) != 0 {
		t.Error("pending not cleared")
	}
}

func TestTypingStartsEditAndSelectParses(t *testing.T) {
	a := testApp(t, 160, 45, "alice")
	a.gotoCol(t, "status")
	a.typeText("on")
	if a.editor == nil || a.editor.in.Kind != ISelect {
		t.Fatal("typing should open the select editor")
	}
	if !strings.Contains(a.screen(), "On hold") {
		t.Error("popup should list On hold")
	}
	a.press("enter")
	if op := a.lastOps(t)[0]; op.Field != "status" || op.Value != "on_hold" {
		t.Fatalf("bad op %+v", op)
	}
}

func TestComboFuzzyAssignee(t *testing.T) {
	a := testApp(t, 160, 45, "alice")
	a.setView(vTasks)
	a.gotoCol(t, "assignee_id")
	a.typeText("hei")
	a.press("enter")
	op := a.lastOps(t)[0]
	if f, _ := core.AsFloat(op.Value); int64(f) != a.userID("heidi") {
		t.Fatalf("want heidi, got %+v", op)
	}
}

func TestDateNaturalAndCalendar(t *testing.T) {
	a := testApp(t, 160, 45, "alice")
	a.gotoCol(t, "due_date")
	a.press("enter", "ctrl+u")
	a.typeText("fri")
	s := a.screen()
	if !strings.Contains(s, "fri → Fri") || !strings.Contains(s, "Mo Tu We Th Fr Sa Su") {
		t.Fatalf("date preview / calendar missing:\n%s", s)
	}
	a.press("down") // +1 week in the calendar
	a.press("enter")
	want, _ := core.ParseNatural("fri", a.rep.Today)
	if op := a.lastOps(t)[0]; op.Value != want.AddDays(7).ISO() {
		t.Fatalf("want %s got %+v", want.AddDays(7).ISO(), op)
	}
}

func TestNumberAndBadInput(t *testing.T) {
	a := testApp(t, 160, 45, "alice")
	a.gotoCol(t, "budget_hours")
	a.press("enter", "ctrl+u")
	a.typeText("12x")
	if a.editor.in.Err == "" {
		t.Error("letters should be rejected in number input")
	}
	a.typeText("3.5")
	a.press("up") // +1
	a.press("tab")
	op := a.lastOps(t)[0]
	if f, _ := core.AsFloat(op.Value); f != 124.5 {
		t.Fatalf("want 124.5, got %+v", op)
	}
}

func TestChipsTagsAndDeps(t *testing.T) {
	a := testApp(t, 160, 45, "alice")
	a.setView(vTasks)
	a.gotoCol(t, "tags")
	a.press("enter")
	a.typeText("zz-new")
	a.press("enter") // add chip
	a.press("enter") // commit
	op := a.lastOps(t)[0]
	list, _ := core.AsList(op.Value)
	found := false
	for _, v := range list {
		if v == "zz-new" {
			found = true
		}
	}
	if op.Field != "tags" || !found {
		t.Fatalf("bad tags op %+v", op)
	}
}

func TestConflictMarkerAndToast(t *testing.T) {
	a := testApp(t, 160, 45, "alice")
	a.gotoCol(t, "name")
	g := a.grids["projects"]
	id := g.CurID()
	a.press("enter")
	a.typeText("x")
	a.press("enter")
	bob := a.userID("bob")
	a.onServerMsg(core.ResultMsg{Req: a.req, OK: true, Conflicts: []core.Conflict{{Index: 0, Entity: core.EntProject, ID: id, Field: "name", Current: "Bob's name", By: &bob}}})
	if _, ok := a.conflicts[cellKey{core.EntProject, id, "name"}]; !ok {
		t.Fatal("conflict not recorded")
	}
	s := a.screen()
	if !strings.Contains(s, "⚠") || !strings.Contains(s, "bob changed Name") {
		t.Fatalf("conflict not shown:\n%s", s)
	}
}

func TestFilterBarLiveAndErrors(t *testing.T) {
	a := testApp(t, 160, 45, "alice")
	a.press("ctrl+f")
	if !a.filterOn {
		t.Fatal("^F should focus the filter")
	}
	a.typeText("status:active")
	n := 0
	for _, p := range a.rep.Projects {
		if p.Status == core.ProjectActive {
			n++
		}
	}
	if a.grids["projects"].DataRows != n {
		t.Fatalf("filter: %d rows, want %d", a.grids["projects"].DataRows, n)
	}
	a.typeText(" frob:1")
	if s := a.screen(); !strings.Contains(s, "unknown field") {
		t.Errorf("error not shown:\n%s", s)
	}
	// autocomplete: "pri" → priority:
	a.press("ctrl+u")
	a.typeText("pri")
	if qb := a.queries[vProjects]; len(qb.sugg) == 0 || qb.sugg[0] != "priority:" {
		t.Fatalf("suggestions %v", qb.sugg)
	}
	a.press("tab")
	if got := a.queries[vProjects].in.Text(); got != "priority:" {
		t.Fatalf("accept suggestion: %q", got)
	}
	a.press("esc")
	if a.filterOn {
		t.Error("esc should leave the filter")
	}
}

func TestNewProjectCascade(t *testing.T) {
	a := testApp(t, 160, 45, "alice")
	a.press("insert")
	f, ok := a.top().(*form)
	if !ok {
		t.Fatal("no form")
	}
	code := f.field("code").Text()
	if !strings.Contains(code, "-") {
		t.Fatalf("code not suggested: %q", code)
	}
	if !strings.Contains(f.field("template_id").Hint, "tasks") {
		t.Errorf("template stats hint missing: %q", f.field("template_id").Hint)
	}
	a.typeText("Brand new thing")
	a.press("ctrl+s")
	ops := a.lastOps(t)
	if ops[0].Kind != "create" || ops[0].Values["name"] != "Brand new thing" || ops[0].Values["template_id"] == nil {
		t.Fatalf("bad create %+v", ops[0])
	}
	// success closes the form and reveals the project
	newID := int64(9999)
	p := *a.rep.Projects[a.grids["projects"].CurID()]
	p.ID, p.Code, p.Name = newID, code, "Brand new thing"
	// the server replies before it broadcasts the new row
	a.onServerMsg(core.ResultMsg{Req: a.req, OK: true, Created: map[string]int64{"p": newID}})
	a.onServerMsg(core.Changes{By: ptr(a.rep.MeID()), Upserts: core.Tables{Projects: []core.Project{p}}})
	if a.top() != nil {
		t.Error("form should close on success")
	}
	if a.grids["projects"].CurID() != newID {
		t.Error("cursor should be on the new project")
	}
}

func ptr[T any](v T) *T { return &v }

func TestNewTaskCascadingProject(t *testing.T) {
	a := testApp(t, 160, 45, "alice")
	a.setView(vTasks)
	pid := a.rep.Tasks[a.grids["tasks"].CurID()].ProjectID
	a.press("insert")
	f := a.top().(*form)
	v, _ := f.field("project_id").Value()
	if fv, _ := core.AsFloat(v); int64(fv) != pid {
		t.Fatalf("project not preset: %v", v)
	}
	// phases offered come from the project
	ph := f.field("phase")
	if len(ph.allChoices()) == 0 {
		t.Fatal("no phases offered")
	}
	f.setFocus(a, f.idx(f.field("title")))
	a.typeText("Write tests")
	a.press("ctrl+s")
	op := a.lastOps(t)[0]
	if op.Kind != "create" || op.Values["title"] != "Write tests" {
		t.Fatalf("bad op %+v", op)
	}
	if fv, _ := core.AsFloat(op.Values["project_id"]); int64(fv) != pid {
		t.Fatalf("project_id %v", op.Values["project_id"])
	}
}

func TestRecordFormSavesOnlyChanges(t *testing.T) {
	a := testApp(t, 160, 45, "alice")
	a.press("ctrl+e")
	f, ok := a.top().(*form)
	if !ok {
		t.Fatal("record form did not open")
	}
	name := f.field("name")
	f.setFocus(a, f.idx(name))
	a.typeText(" (edited)")
	a.press("ctrl+s")
	ops := a.lastOps(t)
	if len(ops) != 1 || ops[0].Field != "name" || !strings.HasSuffix(ops[0].Value.(string), "(edited)") {
		t.Fatalf("ops %+v", ops)
	}
	// a conflict keeps the form open and marks the field
	bob := a.userID("bob")
	a.onServerMsg(core.ResultMsg{Req: a.req, OK: true, Conflicts: []core.Conflict{{Entity: core.EntProject, ID: f.id, Field: "name", Current: "theirs", By: &bob}}})
	if a.top() != f || !strings.Contains(name.Err, "bob changed") {
		t.Fatalf("conflict not shown on form: %q", name.Err)
	}
}

func TestUndoRedoSend(t *testing.T) {
	a := testApp(t, 160, 45, "alice")
	a.press("ctrl+z")
	a.press("ctrl+y")
	out := *a.outbox
	if _, ok := out[len(out)-2].(core.UndoMsg); !ok {
		t.Fatalf("undo not sent: %#v", out)
	}
	if _, ok := out[len(out)-1].(core.RedoMsg); !ok {
		t.Fatalf("redo not sent: %#v", out)
	}
	a.onServerMsg(core.ResultMsg{Req: a.req, OK: false, Error: &core.ErrorBody{Code: "nothing_to_redo", Message: "nothing to redo"}})
	if !strings.Contains(a.status, "Nothing to redo") {
		t.Errorf("status %q", a.status)
	}
}

func TestCopyPasteFillDown(t *testing.T) {
	a := testApp(t, 160, 45, "alice")
	a.setView(vTasks)
	a.gotoCol(t, "phase")
	g := a.grids["tasks"]
	a.press("space", "space", "space") // mark three rows
	if len(g.Marked) != 3 {
		t.Fatalf("marked %d", len(g.Marked))
	}
	a.press("ctrl+c")
	if strings.Count(a.clip, "\n") != 3 || !strings.Contains(a.clip, "\t") {
		t.Fatalf("clip %q", a.clip)
	}
	// fill down: first marked row's phase into the others
	a.press("ctrl+d")
	ops := a.lastOps(t)
	for _, op := range ops {
		if op.Field != "phase" {
			t.Fatalf("fill op %+v", op)
		}
	}
	// paste a single value onto marked rows
	a.onPaste("Review")
	ops = a.lastOps(t)
	if len(ops) == 0 || ops[0].Value != "Review" {
		t.Fatalf("paste ops %+v", ops)
	}
	// multi-cell paste: two columns starting at status
	g.Marked = map[int64]bool{}
	a.gotoCol(t, "status")
	a.onPaste("done\tlow\nreview\thigh\n")
	ops = a.lastOps(t)
	if len(ops) < 2 {
		t.Fatalf("expected several ops, got %+v", ops)
	}
}

func TestViewerIsReadOnly(t *testing.T) {
	a := testApp(t, 160, 45, "victor")
	a.gotoCol(t, "name")
	a.press("enter")
	if a.editor != nil {
		t.Fatal("viewer must not edit")
	}
	if len(*a.outbox) != 0 {
		t.Fatal("nothing should be sent")
	}
}

func TestDeleteConfirm(t *testing.T) {
	a := testApp(t, 160, 45, "alice")
	a.setView(vTasks)
	a.press("f8")
	c, ok := a.top().(*confirmDlg)
	if !ok {
		t.Fatal("no confirm")
	}
	if !strings.Contains(a.screen(), "Delete task") {
		t.Error("confirm text")
	}
	a.press("enter") // default is Cancel
	if len(*a.outbox) != 0 {
		t.Fatal("cancel must not delete")
	}
	_ = c
	a.press("f8", "y")
	if op := a.lastOps(t)[0]; op.Kind != "delete" {
		t.Fatalf("op %+v", op)
	}
}

func TestBoardMoveCard(t *testing.T) {
	a := testApp(t, 160, 45, "alice")
	a.setView(vBoard)
	id := a.board.curID(a)
	a.press("shift+right")
	op := a.lastOps(t)[0]
	if op.ID != id || op.Field != "status" || op.Value != "in_progress" {
		t.Fatalf("op %+v", op)
	}
	a.press("ctrl+r")
	if !a.board.lanes || !strings.Contains(a.screen(), "▾ ") {
		t.Error("swimlanes")
	}
}

func TestTimelineShiftAndZoom(t *testing.T) {
	a := testApp(t, 160, 45, "alice")
	a.setView(vTimeline)
	a.press("right") // expand first project
	a.press("down")  // first task
	row, _ := a.tl.cur(a)
	if row.ent != core.EntTask {
		t.Fatalf("expected a task row, got %+v", row)
	}
	a.press("shift+right")
	ops := a.lastOps(t)
	if len(ops) != 2 || ops[0].Field != "start_date" || ops[1].Field != "due_date" {
		t.Fatalf("ops %+v", ops)
	}
	z := a.tl.zoom
	a.press("+")
	if a.tl.zoom != z-1 {
		t.Error("zoom in")
	}
	a.press("]")
	if op := a.lastOps(t); len(op) != 1 || op[0].Field != "due_date" {
		t.Fatalf("extend due %+v", op)
	}
}

func TestDashboardDrill(t *testing.T) {
	a := testApp(t, 160, 45, "alice")
	a.setView(vDashboard)
	a.press("right") // Overdue tasks
	a.press("enter")
	if a.view != vTasks || a.queries[vTasks].in.Text() != "is:overdue" {
		t.Fatalf("drill: view %d query %q", a.view, a.queries[vTasks].in.Text())
	}
}

func TestOverlaysRender(t *testing.T) {
	a := testApp(t, 160, 45, "alice")
	a.press("f1")
	if !strings.Contains(a.screen(), "keyboard reference") {
		t.Error("help")
	}
	a.typeText("fill")
	if !strings.Contains(a.screen(), "Fill down") {
		t.Error("help search")
	}
	a.press("esc", "ctrl+k")
	a.typeText("go to project nwr-004")
	s := a.screen()
	if !strings.Contains(s, "NWR-004") {
		t.Errorf("palette:\n%s", s)
	}
	a.press("enter")
	if a.rep.Projects[a.grids["projects"].CurID()].Code != "NWR-004" {
		t.Error("palette should jump to NWR-004")
	}
	a.press("f10")
	if !strings.Contains(a.screen(), "New project") {
		t.Error("file menu")
	}
	a.press("right")
	if !strings.Contains(a.screen(), "Fill down") {
		t.Error("edit menu")
	}
	a.press("esc", "f4")
	if !strings.Contains(a.screen(), "Columns") {
		t.Error("columns dialog")
	}
	a.press("esc", "shift+f10")
}

func TestMouseClickAndDoubleClick(t *testing.T) {
	a := testApp(t, 160, 45, "alice")
	m := Model{a}
	_, hits := a.frame()
	var target Hit
	for _, h := range hits {
		if h.Kind == HitCell && h.S == "projects" && h.A == 3 && h.B == 1 {
			target = h
		}
	}
	if target.Kind != HitCell {
		t.Fatal("no cell hit")
	}
	click := tea.MouseClickMsg{X: target.R.X + 1, Y: target.R.Y, Button: tea.MouseLeft}
	m.Update(click)
	g := a.grids["projects"]
	if g.Row != 3 || g.Col != 1 {
		t.Fatalf("click moved to %d,%d", g.Row, g.Col)
	}
	m.Update(tea.MouseReleaseMsg{X: click.X, Y: click.Y, Button: tea.MouseLeft})
	m.Update(click)
	if a.editor == nil {
		t.Fatal("double-click should edit")
	}
	// tabs are clickable
	a.editor = nil
	_, hits = a.frame()
	for _, h := range hits {
		if h.Kind == HitTab && h.A == vBoard {
			m.Update(tea.MouseClickMsg{X: h.R.X + 1, Y: h.R.Y, Button: tea.MouseLeft})
		}
	}
	if a.view != vBoard {
		t.Error("tab click")
	}
}

func TestBoardDragAndDrop(t *testing.T) {
	a := testApp(t, 160, 45, "alice")
	a.setView(vBoard)
	m := Model{a}
	_, hits := a.frame()
	var card, col Hit
	for _, h := range hits {
		if h.Kind == HitCard && h.A == 0 && h.B == 0 {
			card = h
		}
		if h.Kind == HitBoardCol && h.A == 3 {
			col = h
		}
	}
	id := a.board.curID(a)
	m.Update(tea.MouseClickMsg{X: card.R.X + 2, Y: card.R.Y, Button: tea.MouseLeft})
	m.Update(tea.MouseMotionMsg{X: col.R.X + 3, Y: col.R.Y + 5, Button: tea.MouseLeft})
	m.Update(tea.MouseReleaseMsg{X: col.R.X + 3, Y: col.R.Y + 5, Button: tea.MouseLeft})
	op := a.lastOps(t)[0]
	if op.ID != id || op.Value != "review" {
		t.Fatalf("drop op %+v", op)
	}
}

func TestRemoteChangeFlashesAndToasts(t *testing.T) {
	a := testApp(t, 160, 45, "alice")
	g := a.grids["projects"]
	p := *a.rep.Projects[g.CurID()]
	p.Name = "Changed by bob"
	bob := a.userID("bob")
	grp := int64(1)
	a.onServerMsg(core.Changes{By: &bob, Group: &grp, Upserts: core.Tables{Projects: []core.Project{p}},
		Activity: []core.Activity{{ID: 99999, TS: "2026-10-17T10:00:00Z", UserID: &bob, Group: 1, Action: core.ActionSet, Entity: "project", EntityID: p.ID, Field: "name", Label: p.Code + " name: x → Changed by bob", State: core.StateDone}}})
	if _, ok := a.flash[core.RowKey{Entity: core.EntProject, ID: p.ID}]; !ok {
		t.Error("row should flash")
	}
	if s := a.screen(); !strings.Contains(s, "bob: "+p.Code) || !strings.Contains(s, "Changed by bob") {
		t.Errorf("toast/row:\n%s", s)
	}
}

func Test256ColourFallback(t *testing.T) {
	a := testApp(t, 100, 30, "alice")
	a.truecolor = false
	b, _ := a.frame()
	out := b.Render(false)
	if strings.Contains(out, ";2;") || !strings.Contains(out, "38;5;") {
		t.Fatal("256-colour output expected")
	}
	if to256(hex("#ffffff")) != 231 && to256(hex("#ffffff")) != 255 {
		t.Errorf("white maps to %d", to256(hex("#ffffff")))
	}
	if to256(hex("#ff0000")) != 196 {
		t.Errorf("red maps to %d", to256(hex("#ff0000")))
	}
}

func TestPresenceMarkers(t *testing.T) {
	a := testApp(t, 160, 45, "alice")
	g := a.grids["projects"]
	id := g.CurID()
	bob := a.userID("bob")
	f := "name"
	a.rep.Apply(core.PresenceMsg{Peers: []core.Peer{
		{Conn: 1, UserID: a.rep.MeID(), Client: "tui-go"},
		{Conn: 2, UserID: bob, Client: "web-rust", Focus: &core.Focus{Entity: core.EntProject, ID: id, Field: &f}},
	}})
	s := a.screen()
	if !strings.Contains(s, " BS ") || !strings.Contains(s, "online: 2") {
		t.Errorf("presence:\n%s", s)
	}
	// focus messages are sent after the debounce
	a.focusSent = ""
	a.focusAt = a.now.Add(-time1s())
	if cmd := a.maybeSendFocus(); cmd == nil && len(*a.outbox) == 0 {
		t.Error("focus not sent")
	}
	last := (*a.outbox)[len(*a.outbox)-1]
	if fm, ok := last.(core.FocusMsg); !ok || fm.ID == nil || *fm.ID != id {
		t.Fatalf("focus %#v", last)
	}
}

func TestListFieldTypingReplacesValue(t *testing.T) {
	a := testApp(t, 160, 45, "alice")
	a.press("insert")
	f := a.top().(*form)
	f.setFocus(a, 0) // Program (pre-filled)
	a.typeText("muni")
	a.press("enter")
	v, _ := f.field("program_id").Value()
	if a.rep.ProgramName(asIDPtr(v)) != "Municipal GIS" {
		t.Fatalf("program %v", v)
	}
	if code := f.field("code").Text(); !strings.HasPrefix(code, "GIS-") {
		t.Fatalf("code should follow the program: %q", code)
	}
	f.setFocus(a, f.idx(f.field("template_id")))
	a.typeText("zzz")
	a.press("enter")
	tv, _ := f.field("template_id").Value()
	if tv == nil || f.field("template_id").Err == "" {
		t.Fatalf("non-matching entry should keep the value and warn: %v %q", tv, f.field("template_id").Err)
	}
}

func TestSuggestCompletesPrefix(t *testing.T) {
	a := testApp(t, 160, 45, "alice")
	a.setView(vTasks)
	a.gotoCol(t, "phase")
	a.press("enter", "ctrl+u")
	a.typeText("te")
	a.press("enter")
	if op := a.lastOps(t)[0]; op.Value != "Test" {
		t.Fatalf("want Test, got %+v", op)
	}
	a.gotoCol(t, "phase")
	a.press("enter", "ctrl+u")
	a.typeText("Brand new phase")
	a.press("enter")
	if op := a.lastOps(t)[0]; op.Value != "Brand new phase" {
		t.Fatalf("free text: %+v", op)
	}
}

func TestTallFormScrollsOnSmallScreens(t *testing.T) {
	a := testApp(t, 80, 24, "alice")
	f := newForm("Tall", 60)
	for i := 0; i < 30; i++ {
		f.add(NewInput(IText, fmt.Sprintf("Field %02d", i)))
	}
	a.push(f)
	s := a.screen()
	if !strings.Contains(s, "▼ more") || strings.Contains(s, "Field 29") {
		t.Fatalf("expected a scroll hint:\n%s", s)
	}
	for i := 0; i < 29; i++ {
		a.press("tab")
	}
	s = a.screen()
	if !strings.Contains(s, "Field 29") || !strings.Contains(s, "▲ more") || !strings.Contains(s, "[ Save ]") {
		t.Fatalf("last field should be visible after tabbing:\n%s", s)
	}
}
