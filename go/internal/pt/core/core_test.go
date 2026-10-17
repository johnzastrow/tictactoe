package core

// Ports of the Rust pt-core unit tests, so both cores agree.

import (
	"encoding/json"
	"reflect"
	"strings"
	"testing"
)

func d(s string) Date {
	r, ok := ParseISO(s)
	if !ok {
		panic(s)
	}
	return r
}

func TestCivilRoundtrip(t *testing.T) {
	for days := -700_000; days < 2_900_000; days += 997 {
		dt := Date(days)
		y, m, dd := dt.YMD()
		if r, ok := FromYMD(y, m, dd); !ok || r != dt {
			t.Fatalf("roundtrip %d -> %d-%d-%d", days, y, m, dd)
		}
	}
	if r, _ := FromYMD(1970, 1, 1); r != 0 {
		t.Fatal("epoch")
	}
	if d("2026-10-17").Weekday() != 5 {
		t.Fatal("saturday")
	}
}

func TestMonthMath(t *testing.T) {
	cases := [][2]string{{"2026-01-31", "2026-02-28"}, {"2028-01-31", "2028-02-29"}, {"2026-12-15", "2027-01-15"}}
	for _, c := range cases {
		if got := d(c[0]).AddMonths(1); got != d(c[1]) {
			t.Errorf("%s +1m = %s", c[0], got)
		}
	}
	if d("2026-01-15").AddMonths(-1) != d("2025-12-15") {
		t.Error("-1m")
	}
}

func TestNaturalDates(t *testing.T) {
	today := d("2026-10-17")
	p := func(s string) string {
		r, err := ParseNatural(s, today)
		if err != nil || r == nil {
			t.Fatalf("%q: %v", s, err)
		}
		return r.ISO()
	}
	cases := map[string]string{
		"today": "2026-10-17", "tom": "2026-10-18", "fri": "2026-10-23", "sat": "2026-10-24",
		"monday": "2026-10-19", "+3d": "2026-10-20", "-2w": "2026-10-03", "1m": "2026-11-17",
		"eow": "2026-10-23", "eom": "2026-10-31", "eoy": "2026-12-31", "11/3": "2026-11-03",
		"11/3/2027": "2027-11-03", "nov 3": "2026-11-03", "3 Nov": "2026-11-03",
		"november 3rd 2027": "2027-11-03", "2026-02-28": "2026-02-28",
	}
	for in, want := range cases {
		if got := p(in); got != want {
			t.Errorf("%q = %s, want %s", in, got, want)
		}
	}
	if r, err := ParseNatural("  ", today); r != nil || err != nil {
		t.Error("empty")
	}
	for _, bad := range []string{"2026-02-30", "blah", "13/1"} {
		if _, err := ParseNatural(bad, today); err == nil {
			t.Errorf("%q should fail", bad)
		}
	}
	if r, _ := ParseNatural("eow", d("2026-10-23")); r.ISO() != "2026-10-23" {
		t.Error("eow on friday")
	}
}

func TestTimestamps(t *testing.T) {
	ts, ok := ParseRFC3339("2026-10-17T14:03:05Z")
	if !ok || RFC3339(ts) != "2026-10-17T14:03:05Z" {
		t.Fatal("rfc3339")
	}
	if RelativeTime("2026-10-17T14:03:05Z", ts+125) != "2m ago" {
		t.Fatal("relative")
	}
}

func TestFuzzy(t *testing.T) {
	a := FuzzyMatch("ali", "Alice Nguyen")
	b := FuzzyMatch("ali", "Alpha Lima India")
	if a.Score <= b.Score || !reflect.DeepEqual(a.Indices, []int{0, 1, 2}) {
		t.Fatal("substring beats subsequence")
	}
	m := FuzzyMatch("nwr", "Northwind Retail")
	if m.Indices[0] != 0 || len(m.Indices) != 3 {
		t.Fatal("word starts")
	}
	if m := FuzzyMatch("pr", "POS refresh"); !reflect.DeepEqual(m.Indices, []int{0, 4}) {
		t.Fatalf("pr: %v", m.Indices)
	}
	if FuzzyMatch("xyz", "Northwind") != nil || FuzzyMatch("", "x") == nil {
		t.Fatal("edge")
	}
	r := Rank("bob", []string{"Alice", "Bob Smith", "Robert Bobson"})
	if r[0].Index != 1 || len(r) != 2 {
		t.Fatal("rank")
	}
}

func norm(t *testing.T, e Entity, f string, v any) (any, error) {
	t.Helper()
	return Normalize(Field(e, f), v)
}

func eqJSON(a, b any) bool {
	x, _ := json.Marshal(a)
	y, _ := json.Marshal(b)
	return string(x) == string(y)
}

func TestNormalisation(t *testing.T) {
	ok := func(e Entity, f string, in, want any) {
		t.Helper()
		got, err := norm(t, e, f, in)
		if err != nil || !eqJSON(got, want) {
			t.Errorf("%s.%s(%v) = %v, %v; want %v", e, f, in, got, err, want)
		}
	}
	bad := func(e Entity, f string, in any) {
		t.Helper()
		if _, err := norm(t, e, f, in); err == nil {
			t.Errorf("%s.%s(%v) should fail", e, f, in)
		}
	}
	ok(EntProject, "code", " nwr-004 ", "NWR-004")
	bad(EntProject, "code", "X")
	ok(EntTask, "tags", []any{"B", "a", "b"}, []string{"a", "b"})
	bad(EntTask, "tags", []any{"bad tag"})
	ok(EntTask, "est_hours", 1.234, 1.23)
	bad(EntTask, "est_hours", -1.0)
	ok(EntTask, "est_hours", nil, nil)
	bad(EntTask, "actual_hours", nil)
	ok(EntTask, "notes", nil, "")
	bad(EntTask, "title", "  ")
	ok(EntTask, "status", "done", "done")
	bad(EntTask, "status", "Done")
	ok(EntTask, "due_date", "", nil)
	bad(EntTask, "due_date", "2026-02-30")
	ok(EntTask, "deps", []any{3.0, 1.0, 3.0}, []int64{1, 3})
	ok(EntUser, "active", 1.0, true)
	ok(EntProgram, "color", "#AABBCC", "#aabbcc")
}

func TestCustomValues(t *testing.T) {
	opts := []string{"North", "South"}
	if v, err := NormalizeCF(CfEnum, opts, "Region", "North"); err != nil || v != "North" {
		t.Error("enum ok")
	}
	if _, err := NormalizeCF(CfEnum, opts, "Region", "East"); err == nil {
		t.Error("enum bad")
	}
	if v, _ := NormalizeCF(CfNumber, nil, "Risk", 4.5); v != 4.5 {
		t.Error("number")
	}
	if v, _ := NormalizeCF(CfText, nil, "X", ""); v != nil {
		t.Error("empty text")
	}
}

func TestEquality(t *testing.T) {
	cases := []struct {
		a, b any
		eq   bool
	}{
		{int64(1), 1.0, true},
		{[]any{"b", "a"}, []string{"a", "b"}, true},
		{[]any{2.0, 1.0}, []int64{1, 2}, true},
		{"a", nil, false},
		{"", nil, true},
		{"todo", "done", false},
	}
	for _, c := range cases {
		if ValuesEqual(c.a, c.b) != c.eq {
			t.Errorf("ValuesEqual(%v, %v) != %v", c.a, c.b, c.eq)
		}
	}
}

func TestOpWireFormat(t *testing.T) {
	m, err := DecodeClientMsg([]byte(`{"type":"mutate","req":7,"ops":[
	  {"op":"set","entity":"task","id":301,"field":"status","base":"todo","value":"done"},
	  {"op":"create","entity":"task","ref":"n1","values":{"project_id":12,"title":"Retro"}},
	  {"op":"delete","entity":"task","id":299}]}`))
	if err != nil {
		t.Fatal(err)
	}
	mm := m.(MutateMsg)
	if mm.Req != 7 || len(mm.Ops) != 3 || mm.Ops[1].Kind != "create" || *mm.Ops[1].Ref != "n1" {
		t.Fatalf("%+v", mm)
	}
	b, _ := EncodeClientMsg(mm)
	var back map[string]any
	json.Unmarshal(b, &back)
	op0 := back["ops"].([]any)[0].(map[string]any)
	if op0["field"] != "status" || op0["force"] != nil {
		t.Fatalf("%s", b)
	}
	if _, err := DecodeClientMsg([]byte(`{"type":"nope"}`)); err == nil {
		t.Fatal("unknown type must fail")
	}
}

func TestServerMsgs(t *testing.T) {
	b, _ := EncodeServerMsg(ResultMsg{Req: 3, OK: true})
	if string(b) != `{"type":"result","req":3,"ok":true}` {
		t.Fatalf("%s", b)
	}
	m, _ := DecodeServerMsg([]byte(`{"type":"pong"}`))
	if _, ok := m.(PongMsg); !ok {
		t.Fatal("pong")
	}
	b, _ = EncodeServerMsg(WelcomeMsg{Protocol: 1, Server: ServerInfo{"go", "x"}, Me: User{ID: 1, Username: "a", DisplayName: "A", Role: RoleAdmin, Active: true}, Conn: 2})
	if !strings.Contains(string(b), `"server":{"impl":"go"`) || !strings.Contains(string(b), `"role":"admin"`) {
		t.Fatalf("%s", b)
	}
	// rows never serialise null lists
	b, _ = json.Marshal(Task{ID: 1, Status: TaskTodo, Priority: PriorityLow})
	if !strings.Contains(string(b), `"deps":[]`) || !strings.Contains(string(b), `"cf":{}`) {
		t.Fatalf("%s", b)
	}
}

// sample mirrors replica::tests::sample in Rust.
func sample() *Replica {
	r := NewReplica(d("2026-10-17"))
	u := func(id int64, name, disp string) User {
		return User{ID: id, Username: name, DisplayName: disp, Role: RoleMember, Active: true}
	}
	task := func(id, pid, seq int64, title string, st TaskStatus, due *string, est *float64, act float64, as *int64) Task {
		return Task{ID: id, ProjectID: pid, Seq: seq, Title: title, Phase: "Build", Status: st, Priority: PriorityMedium,
			AssigneeID: as, DueDate: due, EstHours: est, ActualHours: act, CF: CfValues{}}
	}
	me := u(1, "alice", "Alice Nguyen")
	r.Me = &me
	r.Load(Tables{
		Users:    []User{u(1, "alice", "Alice Nguyen"), u(2, "bob", "Bob Stone")},
		Programs: []Program{{ID: 1, Name: "Northwind Retail", Client: "Northwind", Prefix: "NWR", Color: "#4f9dde"}},
		Projects: []Project{{ID: 10, Code: "NWR-004", Name: "POS refresh", ProgramID: Ptr[int64](1), OwnerID: Ptr[int64](1),
			Status: ProjectActive, Priority: PriorityHigh, Health: HealthAmber, StartDate: Ptr("2026-09-01"),
			DueDate: Ptr("2026-12-15"), BudgetHours: Ptr(100.0), Tags: []string{"retail"}, CF: CfValues{}}},
		Tasks: []Task{
			task(100, 10, 1, "Kickoff", TaskDone, Ptr("2026-09-02"), Ptr(4.0), 5, Ptr[int64](1)),
			task(101, 10, 2, "Load test checkout", TaskInProgress, Ptr("2026-10-10"), Ptr(16.0), 9.5, Ptr[int64](2)),
			task(102, 10, 3, "Go live", TaskTodo, Ptr("2026-12-01"), nil, 0, nil),
		},
	})
	return r
}

func TestMetricsAndLookups(t *testing.T) {
	r := sample()
	m := r.Metrics(10)
	if m.TasksTotal != 3 || m.TasksDone != 1 || m.TasksOverdue != 1 || m.EstTotal != 20 || m.ActualTotal != 14.5 {
		t.Fatalf("%+v", m)
	}
	if b, _ := r.BurnPct(r.Projects[10]); b < 14.49 || b > 14.51 {
		t.Fatal("burn")
	}
	if !reflect.DeepEqual(r.ProjectTasks(10), []int64{100, 101, 102}) || r.NextCode(Ptr[int64](1)) != "NWR-005" {
		t.Fatal("lookups")
	}
	if r.FieldValue(EntTask, 101, "status") != "in_progress" || r.TaskRef(r.Tasks[102]) != "NWR-004 #3" {
		t.Fatal("field value")
	}
}

func TestChangesApply(t *testing.T) {
	r := sample()
	tk := *r.Tasks[102]
	tk.Status = TaskDone
	a := r.Apply(Changes{By: Ptr[int64](2), Group: Ptr[int64](1), Upserts: Tables{Tasks: []Task{tk}}, Deletes: Deletes{Tasks: []int64{100}}})
	if !reflect.DeepEqual(a.Upserted, []RowKey{{EntTask, 102}}) || !reflect.DeepEqual(a.Deleted, []RowKey{{EntTask, 100}}) {
		t.Fatalf("%+v", a)
	}
	if m := r.Metrics(10); m.TasksTotal != 2 || m.TasksDone != 1 {
		t.Fatal("metrics")
	}
}

func tasksMatching(t *testing.T, q string) []int64 {
	t.Helper()
	r := sample()
	cq := CompileQuery(q, TargetTasks, r)
	if len(cq.Errors) > 0 {
		t.Fatalf("%q: %+v", q, cq.Errors)
	}
	out := []int64{}
	for _, id := range SortedIDs(r.Tasks) {
		if cq.MatchesTask(r.Tasks[id], r) {
			out = append(out, id)
		}
	}
	return out
}

func TestQueryBasics(t *testing.T) {
	cases := map[string][]int64{
		"": {100, 101, 102}, "load": {101}, "status:done": {100}, "status:in": {101},
		"status:todo,done": {100, 102}, "-status:done": {101, 102}, "@me": {100}, "@bob": {101},
		"assignee:none": {102}, "is:overdue": {101}, "is:open due<+30d": {101}, "est>=10": {101},
		"est:none": {102}, "project:nwr": {100, 101, 102}, `"load test"`: {101}, "due:none": {},
	}
	for q, want := range cases {
		if got := tasksMatching(t, q); !reflect.DeepEqual(got, want) {
			t.Errorf("%q = %v, want %v", q, got, want)
		}
	}
}

func TestQueryErrors(t *testing.T) {
	r := sample()
	q := CompileQuery("status:zzz frob:1 ok", TargetTasks, r)
	if len(q.Errors) != 2 || q.Errors[0].Start != 0 || q.Errors[0].End != 10 || q.Errors[1].Start != 11 || q.Errors[1].End != 17 {
		t.Fatalf("%+v", q.Errors)
	}
	for _, tk := range r.Tasks {
		if q.MatchesTask(tk, r) {
			t.Fatal("'ok' matches nothing")
		}
	}
}

func TestQueryProjects(t *testing.T) {
	r := sample()
	q := CompileQuery("program:northwind health:amber pct>30 #retail", TargetProjects, r)
	if len(q.Errors) > 0 || !q.MatchesProject(r.Projects[10], r) {
		t.Fatalf("%+v", q.Errors)
	}
	if CompileQuery("pct>50", TargetProjects, r).MatchesProject(r.Projects[10], r) {
		t.Fatal("pct>50")
	}
}

func col(cols []Column, k string) *Column { return findCol(cols, k) }

func TestCells(t *testing.T) {
	r := sample()
	cols := TaskColumns(r, true)
	c := func(k string, id int64) Cell { return CellFor(r, col(cols, k), id) }
	if c("project", 101).Text != "NWR-004" || c("status", 101).Text != "In progress" || c("due_date", 101).Style.Kind != StOverdue {
		t.Fatal("task cells")
	}
	if c("var", 100).Text != "+1" || c("var", 101).Text != "-6.5" || c("assignee_id", 101).Text != "Bob Stone" {
		t.Fatalf("var %q %q", c("var", 100).Text, c("var", 101).Text)
	}
	pc := ProjectColumns(r)
	if CellFor(r, col(pc, "burn"), 10).Text != "15%" || CellFor(r, col(pc, "done"), 10).Text != "1/3" {
		t.Fatal("project cells")
	}
}

func TestSortAndGroup(t *testing.T) {
	r := sample()
	cols := TaskColumns(r, true)
	rows := BuildRows(r, cols, []int64{100, 101, 102}, []SortSpec{{"est_hours", true}}, "", nil)
	if rows[0].ID != 101 || rows[1].ID != 100 || rows[2].ID != 102 {
		t.Fatalf("%+v", rows)
	}
	rows = BuildRows(r, cols, []int64{100, 101, 102}, nil, "project", nil)
	g := rows[0]
	est := 0
	for i := range cols {
		if cols[i].Key == "est_hours" {
			est = i
		}
	}
	if !g.IsGroup || g.Label != "NWR-004  POS refresh" || g.Count != 3 || *g.Sums[est] != 20 {
		t.Fatalf("%+v", g)
	}
	if len(BuildRows(r, cols, []int64{100, 101, 102}, nil, "project", map[string]bool{"nwr-004": true})) != 1 {
		t.Fatal("collapse")
	}
}

func TestParsing(t *testing.T) {
	r := sample()
	ok := func(f string, id int64, in string, want any) {
		t.Helper()
		got, err := ParseInput(r, EntTask, f, id, in)
		if err != nil || !eqJSON(got, want) {
			t.Errorf("%s(%q) = %v, %v; want %v", f, in, got, err, want)
		}
	}
	ok("status", 101, "done", "done")
	ok("status", 101, "In progress", "in_progress")
	ok("status", 101, "rev", "review")
	ok("assignee_id", 101, "bob", 2)
	ok("assignee_id", 101, "Alice Nguyen", 1)
	ok("due_date", 101, "fri", "2026-10-23")
	ok("est_hours", 101, "2.5h", 2.5)
	ok("deps", 102, "#1 #2", []int64{100, 101})
	ok("tags", 102, "a, #B", []string{"a", "b"})
	if _, err := ParseInput(r, EntTask, "deps", 102, "#9"); err == nil {
		t.Error("deps #9")
	}
	if EditText(r, EntTask, "assignee_id", 101) != "bob" {
		t.Error("edit text")
	}
	if DisplayValue(r, EntTask, "status", "in_progress") != "In progress" {
		t.Error("display")
	}
}

func TestChoiceFiltering(t *testing.T) {
	r := sample()
	ch := Choices(r, EntTask, "assignee_id", 101, nil)
	if ch[0].Label != "(none)" {
		t.Fatal("none first")
	}
	f := FilterChoices(ch, "bo")
	if ch[f[0].Index].Label != "Bob Stone" {
		t.Fatal("bo")
	}
}

func TestHours(t *testing.T) {
	for in, want := range map[float64]string{16: "16", 2.5: "2.5", 1.25: "1.25", -6.5: "-6.5", 0.3: "0.3"} {
		if FmtHours(in) != want {
			t.Errorf("FmtHours(%v) = %s", in, FmtHours(in))
		}
	}
}
