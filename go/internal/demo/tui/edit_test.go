package tui

import (
	"strings"
	"testing"

	tea "charm.land/bubbletea/v2"

	"git.wharf.fluidgrid.org/jcz/tictactoe/go/internal/demo/core"
)

// setOf asserts the last set's key and returns its decoded value.
func setOf[T any](t *testing.T, a *App, key string) T {
	t.Helper()
	m := a.lastSet(t)
	if m.Key != key {
		t.Fatalf("last set is %s=%s, want %s", m.Key, m.Value, key)
	}
	return decode[T](t, m.Value)
}

func TestSidebarNavigation(t *testing.T) {
	a := testApp(t, 115, 33)
	if a.inBody || a.tab != 0 {
		t.Fatal("starts in the sidebar on tab 1")
	}
	a.press("down", "down", "down")
	if a.tab != 3 {
		t.Fatalf("tab %d", a.tab)
	}
	a.press("end")
	if a.tab != 15 {
		t.Fatal("End = last tab")
	}
	a.press("home", "enter")
	if !a.inBody {
		t.Fatal("Enter moves into the body")
	}
	a.press("tab", "tab", "tab", "tab")
	if a.inBody {
		t.Fatal("Tab past the last control returns to the sidebar")
	}
	a.press("tab", "shift+tab")
	if a.inBody {
		t.Fatal("Shift+Tab from the first control returns to the sidebar")
	}
	a.press("right", "esc")
	if a.inBody {
		t.Fatal("Esc returns to the sidebar")
	}
	a.press("ctrl+pgdown", "ctrl+pgdown")
	if a.tab != 2 {
		t.Fatalf("^PgDn: tab %d", a.tab)
	}
	a.press("alt+0")
	if a.tab != 9 {
		t.Fatalf("Alt+0: tab %d", a.tab)
	}
	// focus goes to the server once the tab has been stable for a moment
	a.now = a.now.Add(focusDelay * 2)
	a.maybeSendFocus()
	found := false
	for _, m := range *a.outbox {
		if f, ok := m.(core.FocusMsg); ok && f.Tab == "grid" {
			found = true
		}
	}
	if !found {
		t.Fatalf("no focus message: %#v", *a.outbox)
	}
}

func TestTextCommitValidateRevert(t *testing.T) {
	a := testApp(t, 115, 33)
	a.goTab(1)
	a.press("ctrl+u")
	a.typeText("Grace Hopper")
	a.press("enter")
	m := a.lastSet(t)
	if m.Key != "text.name" || string(m.Value) != `"Grace Hopper"` || m.Base != 1 {
		t.Fatalf("set %+v", m)
	}
	a.ack(t)
	if a.vals.String("text.name") != "Grace Hopper" || a.vals.Version("text.name") != 2 {
		t.Fatal("ack should store the value")
	}
	// Code: typed lower case shows upper case; invalid isn't sent
	a.press("down", "down", "ctrl+u")
	a.typeText("ab-1")
	n := a.setCount()
	a.press("enter")
	if a.setCount() != n {
		t.Fatal("invalid code was sent")
	}
	if s := a.screen(); !strings.Contains(s, "AB-1") || !strings.Contains(s, "must look like ABC-123") {
		t.Fatalf("no upper case / error:\n%s", s)
	}
	a.press("esc")
	if !strings.Contains(a.screen(), "ENG-042") {
		t.Fatal("Esc should revert")
	}
	a.typeText("23")
	a.press("ctrl+u")
	a.typeText("xyz-123")
	a.press("enter")
	if v := setOf[string](t, a, "text.code"); v != "XYZ-123" {
		t.Fatalf("code %q", v)
	}
	// the password never leaves
	a.press("down")
	a.typeText("secret")
	if s := a.screen(); strings.Contains(s, "secret") || !strings.Contains(s, "••••••") {
		t.Fatal("password must be masked")
	}
	a.press("ctrl+o")
	if !strings.Contains(a.screen(), "secret") {
		t.Fatal("^O shows it")
	}
	for _, m := range *a.outbox {
		if s, ok := m.(core.SetMsg); ok && strings.Contains(string(s.Value), "secret") {
			t.Fatal("password was sent")
		}
	}
}

func TestTextConflict(t *testing.T) {
	a := testApp(t, 115, 33)
	a.goTab(1)
	a.typeText("!") // a draft based on v1
	a.onServerMsg(core.ChangedMsg{Key: "text.name", Value: []byte(`"Bob's"`), Version: 2, By: "bob", Conn: 2, At: "2026-10-18T10:00:00Z"})
	if s := a.screen(); !strings.Contains(s, "changed by bob") || !strings.Contains(s, "Ada Lovelace!") {
		t.Fatalf("draft kept with a warning:\n%s", s)
	}
	a.press("enter")
	m := a.lastSet(t)
	if m.Base != 1 {
		t.Fatalf("base %d, want the version editing began from", m.Base)
	}
	cur := core.Entry{Value: []byte(`"Bob's"`), Version: 2, By: "bob", At: "2026-10-18T10:00:00Z"}
	a.onServerMsg(core.AckMsg{Req: m.Req, Conflict: true, Current: &cur})
	if !strings.Contains(a.status, "bob changed text.name first, showing theirs") {
		t.Fatalf("status %q", a.status)
	}
	if a.vals.String("text.name") != "Bob's" || !strings.Contains(a.screen(), "Bob's") {
		t.Fatal("should show theirs")
	}
	if _, ok := a.flash["text.name"]; !ok {
		t.Fatal("the value should flash")
	}
}

func TestNumberSpinnersAndSlider(t *testing.T) {
	a := testApp(t, 115, 33)
	a.goTab(2)
	a.press("up", "shift+up", "enter")
	if v := setOf[float64](t, a, "number.hours"); v != 13.75 {
		t.Fatalf("hours %v", v)
	}
	a.press("tab", "down", "down", "enter")
	if v := setOf[int](t, a, "number.qty"); v != 1 {
		t.Fatalf("qty %v", v)
	}
	a.press("tab", "right")
	if v := setOf[int](t, a, "number.percent"); v != 41 {
		t.Fatalf("percent %v", v)
	}
	// a second change while the first is in flight is queued, not sent
	n := a.setCount()
	a.press("pgup")
	if a.setCount() != n || a.vals.Int("number.percent") != 51 {
		t.Fatal("expected an optimistic, queued change")
	}
	m := a.lastSet(t)
	v := int64(2)
	a.onServerMsg(core.AckMsg{Req: m.Req, OK: true, Version: &v})
	m2 := a.lastSet(t)
	if string(m2.Value) != "51" || m2.Base != 2 {
		t.Fatalf("queued set %+v", m2)
	}
	a.press("home")
	if a.vals.Int("number.percent") != 0 {
		t.Fatal("Home = 0")
	}
}

func TestDateNaturalAndCrossRule(t *testing.T) {
	a := testApp(t, 115, 33)
	a.goTab(3)
	a.press("ctrl+u")
	a.typeText("fri")
	if !strings.Contains(a.screen(), "fri → Fri 23 Oct 2026") {
		t.Fatalf("preview:\n%s", a.screen())
	}
	a.press("enter")
	if v := setOf[string](t, a, "date.start"); v != "2026-10-23" {
		t.Fatalf("start %q", v)
	}
	a.press("tab", "ctrl+u")
	a.typeText("2026-10-01")
	n := a.setCount()
	a.press("enter")
	if a.setCount() != n || !strings.Contains(a.screen(), "due must be on or after start") {
		t.Fatalf("due before start must not be sent:\n%s", a.screen())
	}
	a.press("esc", "down", "enter") // calendar: +1 week from 20 Nov
	if v := setOf[string](t, a, "date.due"); v != "2026-11-27" {
		t.Fatalf("due %q", v)
	}
	a.ack(t)
	a.press("end", "delete")
	if m := a.lastSet(t); m.Key != "date.due" || string(m.Value) != "null" {
		t.Fatalf("Del should clear: %+v", m)
	}
	a.ack(t)
	a.press("shift+tab", "ctrl+t", "enter")
	if v := setOf[string](t, a, "date.start"); v != "2026-10-18" {
		t.Fatalf("^T today: %q", v)
	}
}

func TestDropdown(t *testing.T) {
	a := testApp(t, 115, 33)
	a.goTab(4)
	a.press("enter", "down", "enter")
	if v := setOf[string](t, a, "select.status"); v != "blocked" {
		t.Fatalf("status %q", v)
	}
	a.press("tab", "c", "enter") // typing filters: c → Critical
	if v := setOf[string](t, a, "select.priority"); v != "critical" {
		t.Fatalf("priority %q", v)
	}
	a.press("enter", "esc")
	if a.tabs[3].(*selectTab).drops[1].open {
		t.Fatal("Esc closes the list")
	}
}

func TestCombobox(t *testing.T) {
	a := testApp(t, 115, 33)
	a.goTab(5)
	a.typeText("gl")
	if s := a.screen(); !strings.Contains(s, "Grace Liu") || !strings.Contains(s, "grace · Engineering") {
		t.Fatalf("list:\n%s", s)
	}
	a.press("enter")
	if v := setOf[string](t, a, "combo.owner"); v != "grace" {
		t.Fatalf("owner %q", v)
	}
	a.ack(t)
	a.press("delete")
	if m := a.lastSet(t); string(m.Value) != "null" {
		t.Fatalf("Del clears: %s", m.Value)
	}
	a.ack(t)
	a.press("tab", "down", "down", "enter")
	if v := setOf[string](t, a, "combo.reviewer"); v != "bob" {
		t.Fatalf("reviewer %q", v)
	}
}

func TestTags(t *testing.T) {
	a := testApp(t, 115, 33)
	a.goTab(6)
	a.typeText("bu")
	a.press("enter")
	if v := setOf[[]string](t, a, "tags.tags"); strings.Join(v, ",") != "bug,frontend,q4" {
		t.Fatalf("tags %v", v)
	}
	a.ack(t)
	a.typeText("New-One")
	if !strings.Contains(a.screen(), `✚ Create "new-one"`) {
		t.Fatalf("create entry:\n%s", a.screen())
	}
	a.press("enter")
	if v := setOf[[]string](t, a, "tags.tags"); strings.Join(v, ",") != "bug,frontend,new-one,q4" {
		t.Fatalf("tags %v", v)
	}
	a.ack(t)
	a.press("backspace")
	if v := setOf[[]string](t, a, "tags.tags"); len(v) != 3 {
		t.Fatalf("backspace removes the last chip: %v", v)
	}
	a.ack(t)
	a.press("left", "left", "delete")
	if v := setOf[[]string](t, a, "tags.tags"); strings.Join(v, ",") != "bug,new-one" {
		t.Fatalf("chip delete: %v", v)
	}
	a.typeText("Bad tag")
	if !strings.Contains(a.screen(), "✗ a–z") {
		t.Fatal("invalid tag should be marked")
	}
}

func TestTextArea(t *testing.T) {
	a := testApp(t, 115, 33)
	a.goTab(7)
	a.press("ctrl+end", "enter")
	a.typeText("More.")
	if !strings.Contains(a.screen(), "● unsaved") || !strings.Contains(a.screen(), "Ln 4, Col 6") {
		t.Fatalf("footer:\n%s", a.screen())
	}
	a.press("ctrl+s")
	if v := setOf[string](t, a, "textarea.notes"); v != core.DefaultNotes+"\nMore." {
		t.Fatalf("notes %q", v)
	}
	a.typeText("x")
	a.press("esc")
	if _, ok := a.top().(*confirmDlg); !ok {
		t.Fatal("Esc on a draft asks first")
	}
	a.press("y")
	if a.tabs[6].(*areaTab).dirty {
		t.Fatal("reverted")
	}
}

func TestChoices(t *testing.T) {
	a := testApp(t, 115, 33)
	a.goTab(8)
	a.press("tab", "right", "space")
	if v := setOf[[]string](t, a, "choice.channels"); strings.Join(v, ",") != "email,sms,push" {
		t.Fatalf("channels %v", v)
	}
	a.press("tab", "right", "space")
	if v := setOf[string](t, a, "choice.size"); v != "l" {
		t.Fatalf("size %q", v)
	}
	a.press("tab", "space")
	if v := setOf[bool](t, a, "choice.dark"); !v {
		t.Fatal("dark")
	}
	a.press("shift+tab", "shift+tab", "shift+tab", "space")
	if v := setOf[bool](t, a, "choice.notify"); v {
		t.Fatal("notify off")
	}
	n := a.setCount()
	a.press("tab", "space")
	if a.setCount() != n || !strings.Contains(a.status, "turn on Notify first") {
		t.Fatalf("channels disabled: %q", a.status)
	}
}

func TestCascade(t *testing.T) {
	a := testApp(t, 115, 33)
	a.goTab(9)
	a.press("enter", "down", "enter")
	c := setOf[core.Cascade](t, a, "cascade.pick")
	if deref(c.Program) != "Contoso Health" || c.Project != nil || c.Phase != nil {
		t.Fatalf("program pick clears the rest: %+v", c)
	}
	a.ack(t)
	a.press("tab", "enter", "enter")
	c = setOf[core.Cascade](t, a, "cascade.pick")
	if deref(c.Project) != "Patient portal" {
		t.Fatalf("project %+v", c)
	}
	a.ack(t)
	a.press("tab")
	a.typeText("tes")
	a.press("enter")
	c = setOf[core.Cascade](t, a, "cascade.pick")
	if deref(c.Phase) != "Test" {
		t.Fatalf("phase %+v", c)
	}
	if !strings.Contains(a.screen(), "Contoso Health ▸ Patient portal ▸ Test") {
		t.Fatal("breadcrumb")
	}
}

func TestGrid(t *testing.T) {
	a := testApp(t, 115, 33)
	a.goTab(10)
	a.press("enter", "ctrl+u")
	a.typeText("Desk")
	a.press("enter")
	rows := setOf[[]core.GridRow](t, a, "grid.rows")
	if rows[0].Item != "Desk" {
		t.Fatalf("item %+v", rows[0])
	}
	a.ack(t)
	// typing starts an edit; status uses a list; due takes natural dates
	a.press("right", "9", "enter")
	if rows := setOf[[]core.GridRow](t, a, "grid.rows"); rows[1].Qty != 9 {
		t.Fatalf("qty %+v", rows[1])
	}
	a.ack(t)
	a.press("right", "right", "right", "enter", "down", "enter")
	if rows := setOf[[]core.GridRow](t, a, "grid.rows"); rows[2].Status != "blocked" {
		t.Fatalf("status %+v", rows[2])
	}
	a.ack(t)
	a.press("right", "enter", "ctrl+u")
	a.typeText("tom")
	a.press("enter")
	if rows := setOf[[]core.GridRow](t, a, "grid.rows"); deref(rows[3].Due) != "2026-10-19" {
		t.Fatalf("due %+v", rows[3])
	}
	a.ack(t)
	a.press("up", "delete")
	if rows := setOf[[]core.GridRow](t, a, "grid.rows"); rows[3].Due != nil {
		t.Fatal("Del clears due")
	}
	a.ack(t)
	a.press("insert")
	a.press("enter")
	rows = setOf[[]core.GridRow](t, a, "grid.rows")
	if len(rows) != 7 || rows[6].ID != 7 || rows[6].Item != "New item" || rows[6].Status != "todo" {
		t.Fatalf("insert %+v", rows[len(rows)-1])
	}
	a.ack(t)
	a.press("f8", "y")
	if rows := setOf[[]core.GridRow](t, a, "grid.rows"); len(rows) != 6 {
		t.Fatalf("delete: %d rows", len(rows))
	}
	// sorting is a view: no set
	n := a.setCount()
	a.press("ctrl+o", "ctrl+o")
	if a.setCount() != n || !strings.Contains(a.screen(), "▼") {
		t.Fatal("sort is local")
	}
}

func TestMenusTab(t *testing.T) {
	a := testApp(t, 115, 33)
	a.goTab(11)
	a.press("right", "enter", "enter", "enter") // Format ▸ Text ▸ Bold
	if a.status != "ℹ Picked Format ▸ Text ▸ Bold" {
		t.Fatalf("status %q", a.status)
	}
	a.press("tab", "enter")
	if _, ok := a.top().(*listOverlay); !ok {
		t.Fatal("Enter opens the context menu")
	}
	a.press("s") // Star
	if f := setOf[[]core.File](t, a, "menus.files"); !f[0].Starred {
		t.Fatal("star")
	}
	a.ack(t)
	a.press("down", "enter", "r", "ctrl+u")
	a.typeText("Map.pdf")
	a.press("enter")
	if f := setOf[[]core.File](t, a, "menus.files"); f[1].Name != "Map.pdf" {
		t.Fatalf("rename %+v", f[1])
	}
	a.ack(t)
	a.press("enter", "d")
	if f := setOf[[]core.File](t, a, "menus.files"); len(f) != 9 || f[2].Name != "Map copy.pdf" {
		t.Fatalf("duplicate %+v", f)
	}
}

func TestPaletteTabAndGlobal(t *testing.T) {
	a := testApp(t, 115, 33)
	a.goTab(12)
	a.typeText("gantt")
	a.press("enter")
	if a.tab != 15 {
		t.Fatalf("palette ran to tab %d", a.tab)
	}
	if v := setOf[string](t, a, "palette.last"); v != "Go to: Gantt" {
		t.Fatalf("palette.last %q", v)
	}
	a.ack(t)
	a.press("ctrl+k")
	a.typeText("theme day")
	a.press("enter")
	if themes[a.theme].Name != "Daylight" {
		t.Fatal("theme command")
	}
	if v := setOf[string](t, a, "palette.last"); v != "Theme: Daylight" {
		t.Fatalf("palette.last %q", v)
	}
}

func TestDialogs(t *testing.T) {
	a := testApp(t, 115, 33)
	a.goTab(13)
	a.press("enter", "n")
	if a.status != "ℹ Confirm: you chose Cancel" {
		t.Fatalf("status %q", a.status)
	}
	a.press("tab", "enter", "ctrl+u")
	a.typeText("Ada")
	a.press("tab", "ctrl+u")
	a.typeText("bad")
	a.press("ctrl+s")
	if !strings.Contains(a.screen(), "must look like name@example.com") {
		t.Fatalf("field error:\n%s", a.screen())
	}
	a.press("ctrl+u")
	a.typeText("ADA@example.com")
	a.press("ctrl+s")
	if p := setOf[core.Profile](t, a, "dialog.profile"); p.Name != "Ada" || p.Email != "ada@example.com" {
		t.Fatalf("profile %+v", p)
	}
	a.press("right", "right", "right", "enter")
	a.typeText("Lunch is here")
	a.press("enter")
	last := (*a.outbox)[len(*a.outbox)-1]
	if m, ok := last.(core.SendToastMsg); !ok || m.Text != "Lunch is here" {
		t.Fatalf("broadcast %#v", last)
	}
	a.onServerMsg(core.ToastMsg{Text: "Lunch is here", By: "alice", Conn: 1})
	if !strings.Contains(a.screen(), "alice: Lunch is here") {
		t.Fatal("toast shown")
	}
}

func TestChartsKanbanGantt(t *testing.T) {
	a := testApp(t, 115, 33)
	a.goTab(14)
	a.press("right", "up", "pgup")
	if s := setOf[[]int](t, a, "charts.series"); s[1] != 48 {
		// the second change is queued behind the first
		t.Logf("first set %v", s)
	}
	if a.vals.Series()[1] != 73 {
		t.Fatalf("Feb = %d", a.vals.Series()[1])
	}
	a.goTab(15)
	a.press("shift+right")
	if c := setOf[[]core.Card](t, a, "kanban.cards"); c[4].Col != "doing" {
		t.Fatalf("card 5 %+v", c[4])
	}
	a.ack(t)
	a.press("insert")
	a.typeText("Retro")
	a.press("enter")
	if c := setOf[[]core.Card](t, a, "kanban.cards"); c[8].Title != "Retro" || c[8].Col != "doing" || c[8].ID != 9 {
		t.Fatalf("new card %+v", c[8])
	}
	a.goTab(16)
	a.press(">")
	if b := setOf[[]core.Bar](t, a, "gantt.bars"); b[0].Start != "2026-10-20" {
		t.Fatalf("move %+v", b[0])
	}
	a.ack(t)
	a.press("]")
	if b := setOf[[]core.Bar](t, a, "gantt.bars"); b[0].Days != 11 {
		t.Fatalf("extend %+v", b[0])
	}
	if !strings.Contains(a.screen(), "Discovery · 20 Oct → 30 Oct 2026 · 11 days") {
		t.Fatalf("note:\n%s", a.screen())
	}
}

func TestResetTab(t *testing.T) {
	a := testApp(t, 115, 33)
	a.press("down", "ctrl+r", "y")
	last := (*a.outbox)[len(*a.outbox)-1]
	r, ok := last.(core.ResetMsg)
	if !ok || strings.Join(r.Keys, ",") != "number.hours,number.qty,number.percent" {
		t.Fatalf("reset %#v", last)
	}
}

func TestRemoteChangeFlashesAndShowsPresence(t *testing.T) {
	a := testApp(t, 115, 33)
	a.onServerMsg(core.ChangedMsg{Key: "text.email", Value: []byte(`"b@x.io"`), Version: 2, By: "bob", Conn: 2, At: "2026-10-18T09:58:00Z"})
	s := a.screen()
	if !strings.Contains(a.status, "bob changed text.email") || !strings.Contains(s, "v2 · by bob · 2m ago") {
		t.Fatalf("status %q\n%s", a.status, s)
	}
	if !strings.Contains(s, " AL ") || !strings.Contains(s, " BO ") || !strings.Contains(s, "online: 2") {
		t.Fatal("presence initials")
	}
	// peer text is made safe for the terminal
	a.onServerMsg(core.ToastMsg{Text: "hi\x1b[2J", By: "bob", Conn: 2})
	if strings.Contains(a.screen(), "\x1b") {
		t.Fatal("escape leaked")
	}
}

func TestMenuBarAndHelp(t *testing.T) {
	a := testApp(t, 115, 33)
	a.press("f10")
	if l, ok := a.top().(*listOverlay); !ok || l.menuBar != 0 {
		t.Fatal("F10 opens File")
	}
	a.press("right")
	if l := a.top().(*listOverlay); l.menuBar != 1 {
		t.Fatal("→ to View")
	}
	a.press("enter") // "1 Text"? first item
	a.press("alt+h", "enter")
	if _, ok := a.top().(*helpDlg); !ok {
		t.Fatal("Help ▸ Keyboard help")
	}
	if s := a.screen(); !strings.Contains(s, "This tab") || !strings.Contains(s, "Getting around") {
		t.Fatalf("help:\n%s", s)
	}
	a.typeText("conflict")
	if s := a.screen(); !strings.Contains(s, "How demo works") {
		t.Fatalf("search keeps matching sections:\n%s", s)
	}
	a.press("esc")
	if a.top() != nil {
		t.Fatal("Esc closes help")
	}
}

func TestMouse(t *testing.T) {
	a := testApp(t, 115, 33)
	m := Model{a}
	_, hits := a.frame()
	for _, h := range hits {
		if h.Kind == HitSideTab && h.A == 14 {
			a.click(h.R.X+2, h.R.Y)
		}
	}
	if a.tab != 14 {
		t.Fatal("sidebar click")
	}
	// drag card #5 (To do) onto Review
	card := a.hitAt(t, "card", 0)
	col := a.hitAt(t, "col", 2)
	m.Update(tea.MouseClickMsg{X: card.R.X + 2, Y: card.R.Y, Button: tea.MouseLeft})
	m.Update(tea.MouseMotionMsg{X: col.R.X + 3, Y: col.R.Y + 6, Button: tea.MouseLeft})
	m.Update(tea.MouseReleaseMsg{X: col.R.X + 3, Y: col.R.Y + 6, Button: tea.MouseLeft})
	if c := setOf[[]core.Card](t, a, "kanban.cards"); c[4].Col != "review" {
		t.Fatalf("drop %+v", c[4])
	}
	// slider: click the track's right end
	a.setTab(1)
	tr := a.hitAt(t, "track", -1)
	a.click(tr.R.Right()-2, tr.R.Y)
	if v := setOf[int](t, a, "number.percent"); v != 100 {
		t.Fatalf("slider %d", v)
	}
	// gantt: drag bar 1 three days right
	a.setTab(15)
	bar := a.hitAt(t, "bar", 0)
	m.Update(tea.MouseClickMsg{X: bar.R.X + 1, Y: bar.R.Y, Button: tea.MouseLeft})
	m.Update(tea.MouseMotionMsg{X: bar.R.X + 4, Y: bar.R.Y, Button: tea.MouseLeft})
	m.Update(tea.MouseReleaseMsg{X: bar.R.X + 4, Y: bar.R.Y, Button: tea.MouseLeft})
	if b := setOf[[]core.Bar](t, a, "gantt.bars"); b[0].Start != "2026-10-22" {
		t.Fatalf("gantt drag %+v", b[0])
	}
	// hint bar clicks run their key
	_, hits = a.frame()
	for _, h := range hits {
		if h.Kind == HitHint && h.S == "ctrl+k" {
			a.click(h.R.X, h.R.Y)
		}
	}
	if _, ok := a.top().(*listOverlay); !ok {
		t.Fatal("hint click should open the palette")
	}
}

func TestJoinForm(t *testing.T) {
	a := newApp(Options{Server: "http://127.0.0.1:1"})
	a.w, a.h = 115, 33
	if !strings.Contains(a.screen(), "Join demo") {
		t.Fatal("join form")
	}
	a.press("enter")
	if !strings.Contains(a.screen(), "start with a letter") {
		t.Fatalf("name error:\n%s", a.screen())
	}
	a.press("up", "ctrl+u")
	a.typeText("ftp://x")
	a.press("down")
	a.typeText("zoe")
	a.press("enter")
	if !strings.Contains(a.screen(), "server must look like") {
		t.Fatalf("url error:\n%s", a.screen())
	}
	a.cancel()
}
