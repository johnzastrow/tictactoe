package store_test

import (
	"os"
	"testing"

	"git.wharf.fluidgrid.org/jcz/tictactoe/go/internal/pt/core"
	"git.wharf.fluidgrid.org/jcz/tictactoe/go/internal/pt/seed"
	"git.wharf.fluidgrid.org/jcz/tictactoe/go/internal/pt/store"
)

// The Go server embeds a copy of docs/pt/schema.sql; it must not drift.
func TestSchemaMatchesDocs(t *testing.T) {
	doc, err := os.ReadFile("../../../../docs/pt/schema.sql")
	if err != nil {
		t.Fatal(err)
	}
	if string(doc) != store.Schema {
		t.Fatal("go/internal/pt/store/schema.sql differs from docs/pt/schema.sql — copy it over")
	}
}

func seeded(t *testing.T) (*store.Store, *core.Replica) {
	t.Helper()
	st, err := store.OpenMemory()
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { st.Close() })
	if _, err := seed.Seed(st, "store-test-pass"); err != nil {
		t.Fatal(err)
	}
	snap, err := st.Snapshot()
	if err != nil {
		t.Fatal(err)
	}
	r := core.NewReplica(store.Today())
	r.Load(snap)
	return st, r
}

func user(t *testing.T, st *store.Store, name string) *core.User {
	t.Helper()
	u, err := st.UserByName(name)
	if err != nil || u == nil {
		t.Fatal(name, err)
	}
	return u
}

func firstTask(r *core.Replica, code string) *core.Task {
	for _, id := range core.SortedIDs(r.Tasks) {
		if r.ProjectCode(r.Tasks[id].ProjectID) == code {
			return r.Tasks[id]
		}
	}
	return nil
}

func TestSeedShape(t *testing.T) {
	_, r := seeded(t)
	if len(r.Projects) != 50 || len(r.Users) != 10 || len(r.Tasks) < 500 || len(r.CustomFields) != 5 {
		t.Fatalf("projects %d users %d tasks %d", len(r.Projects), len(r.Users), len(r.Tasks))
	}
}

func TestSetConflictUndoRedo(t *testing.T) {
	st, r := seeded(t)
	alice, bob := user(t, st, "alice"), user(t, st, "bob")
	tk := firstTask(r, "NWR-001")
	out := st.Mutate(alice, []core.Op{core.SetOp(core.EntTask, tk.ID, "title", tk.Title, "Renamed")})
	if !out.Result.OK || out.Result.Group == nil || out.Changes.Upserts.Tasks[0].Title != "Renamed" {
		t.Fatalf("%+v", out.Result)
	}
	if a := out.Changes.Activity; len(a) != 1 || a[0].New != "Renamed" || a[0].Label == "" {
		t.Fatalf("activity %+v", a)
	}
	// stale base → conflict, nothing applied
	out = st.Mutate(bob, []core.Op{core.SetOp(core.EntTask, tk.ID, "title", tk.Title, "Lost")})
	if !out.Result.OK || out.Result.Group != nil || len(out.Result.Conflicts) != 1 || *out.Result.Conflicts[0].By != alice.ID {
		t.Fatalf("%+v", out.Result)
	}
	if out := st.Undo(alice); !out.Result.OK || out.Changes.Upserts.Tasks[0].Title != tk.Title {
		t.Fatalf("undo %+v", out.Result)
	}
	if out := st.Redo(alice); !out.Result.OK || out.Changes.Upserts.Tasks[0].Title != "Renamed" {
		t.Fatalf("redo %+v", out.Result)
	}
	if out := st.Redo(alice); out.Result.Error.Code != "nothing_to_redo" {
		t.Fatal("redo twice")
	}
}

func TestValidationRollsBack(t *testing.T) {
	st, r := seeded(t)
	erin := user(t, st, "erin")
	tk := firstTask(r, "FAB-001")
	out := st.Mutate(erin, []core.Op{
		core.SetOp(core.EntTask, tk.ID, "phase", tk.Phase, "Changed"),
		core.SetOp(core.EntTask, tk.ID, "status", string(tk.Status), "nonsense"),
	})
	if out.Result.OK || out.Result.Error.Code != "invalid" || *out.Result.Error.Index != 1 {
		t.Fatalf("%+v", out.Result)
	}
	if out := st.Mutate(erin, []core.Op{core.SetOp(core.EntTask, tk.ID, "phase", tk.Phase, tk.Phase+"!")}); len(out.Result.Conflicts) != 0 {
		t.Fatal("first op should have rolled back")
	}
	if out := st.Mutate(erin, []core.Op{core.SetOp(core.EntTask, tk.ID, "due_date", *tk.DueDate, "1999-01-01")}); out.Result.Error == nil {
		t.Fatal("due before start must fail")
	}
}

func TestCreateDeleteUndo(t *testing.T) {
	st, r := seeded(t)
	frank := user(t, st, "frank")
	var pid int64
	for _, p := range r.Projects {
		if p.Code == "GIS-003" {
			pid = p.ID
		}
	}
	out := st.Mutate(frank, []core.Op{core.CreateOp(core.EntTask, "t", map[string]any{"project_id": float64(pid), "title": "New"})})
	if !out.Result.OK {
		t.Fatalf("%+v", out.Result.Error)
	}
	id := out.Result.Created["t"]
	out = st.Mutate(frank, []core.Op{core.DeleteOp(core.EntTask, id)})
	if len(out.Changes.Deletes.Tasks) != 1 {
		t.Fatal("delete")
	}
	if out := st.Undo(frank); !out.Result.OK || out.Changes.Upserts.Tasks[0].ID != id {
		t.Fatalf("undo delete %+v", out.Result)
	}
	if out := st.Undo(frank); !out.Result.OK || out.Changes.Deletes.Tasks[0] != id {
		t.Fatalf("undo create %+v", out.Result)
	}
}

func TestProjectFromTemplateAndAuthz(t *testing.T) {
	st, r := seeded(t)
	admin, bob, victor := user(t, st, "admin"), user(t, st, "bob"), user(t, st, "victor")
	var tpl int64
	for _, tp := range r.Templates {
		if tp.Name == "Data Migration" {
			tpl = tp.ID
		}
	}
	out := st.Mutate(admin, []core.Op{core.CreateOp(core.EntProject, "p", map[string]any{
		"code": "cnf-001", "name": "C", "template_id": float64(tpl), "start_date": "2027-01-04", "tags": []any{"Pilot"},
	})})
	if !out.Result.OK || len(out.Changes.Upserts.Tasks) != 9 || out.Changes.Upserts.Projects[0].Code != "CNF-001" {
		t.Fatalf("%+v", out.Result)
	}
	if out := st.Mutate(victor, nil); out.Result.Error.Code != "forbidden" {
		t.Fatal("viewer")
	}
	out = st.Mutate(bob, []core.Op{core.CreateOp(core.EntProgram, "", map[string]any{"name": "X"})})
	if out.Result.Error == nil || out.Result.Error.Code != "forbidden" {
		t.Fatal("member creating program")
	}
}

func TestDepsCycle(t *testing.T) {
	st, r := seeded(t)
	alice := user(t, st, "alice")
	tk := firstTask(r, "CTH-004")
	ids := r.ProjectTasks(tk.ProjectID)
	a, b := r.Tasks[ids[0]], r.Tasks[ids[1]]
	if out := st.Mutate(alice, []core.Op{core.SetOp(core.EntTask, b.ID, "deps", b.Deps, []any{float64(a.ID)})}); !out.Result.OK {
		t.Fatal(out.Result.Error)
	}
	out := st.Mutate(alice, []core.Op{core.SetOp(core.EntTask, a.ID, "deps", a.Deps, []any{float64(b.ID)})})
	if out.Result.Error == nil || out.Result.Error.Code != "invalid" {
		t.Fatal("cycle must fail")
	}
}

func TestSessions(t *testing.T) {
	st, _ := seeded(t)
	alice := user(t, st, "alice")
	if err := st.CreateSession("h1", alice.ID, "x"); err != nil {
		t.Fatal(err)
	}
	if u, _ := st.SessionUser("h1"); u == nil || u.ID != alice.ID {
		t.Fatal("session")
	}
	_ = st.DeleteSession("h1")
	if u, _ := st.SessionUser("h1"); u != nil {
		t.Fatal("deleted session")
	}
}
