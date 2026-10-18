package core

import (
	"encoding/json"
	"reflect"
	"strings"
	"testing"
	"time"
)

func TestCatalogue(t *testing.T) {
	if len(Keys) != 26 {
		t.Fatalf("%d keys", len(Keys))
	}
	if len(Tabs) != 16 {
		t.Fatalf("%d tabs", len(Tabs))
	}
	inTabs := 0
	for _, tab := range Tabs {
		for _, k := range tab.Keys {
			inTabs++
			if KeyTab(k) != tab.ID {
				t.Errorf("%s: tab %q, want %q", k, KeyTab(k), tab.ID)
			}
		}
	}
	if inTabs != len(Keys) {
		t.Fatalf("tabs list %d keys", inTabs)
	}
	// Every default is valid and already canonical.
	for _, k := range Keys {
		d := Default(k)
		n, err := Normalize(k, d)
		if err != nil {
			t.Errorf("%s default invalid: %v", k, err)
		} else if string(n) != string(d) {
			t.Errorf("%s default not canonical:\n %s\n %s", k, d, n)
		}
	}
	if !IsKey("text.name") || IsKey("nope") || Default("nope") != nil {
		t.Fatal("IsKey/Default")
	}
	if _, err := Normalize("nope", json.RawMessage(`1`)); err != ErrUnknownKey {
		t.Fatal(err)
	}
	if string(Default("textarea.notes")) != `"Welcome to demo.\nEach tab shows one control on its own.\nChanges are shared live with everyone connected."` {
		t.Fatal(string(Default("textarea.notes")))
	}
	if string(Default("combo.reviewer")) != "null" {
		t.Fatal("reviewer")
	}
}

// norm returns the normalised JSON or "ERR".
func norm(key, value string) string {
	b, err := Normalize(key, json.RawMessage(value))
	if err != nil {
		return "ERR"
	}
	return string(b)
}

func TestValidation(t *testing.T) {
	long := func(n int) string { return `"` + strings.Repeat("x", n) + `"` }
	cases := []struct{ key, in, want string }{
		{"text.name", `"  Ada  "`, `"Ada"`},
		{"text.name", `"   "`, "ERR"},
		{"text.name", long(80), long(80)},
		{"text.name", long(81), "ERR"},
		{"text.name", `"Ünïcødé ✓"`, `"Ünïcødé ✓"`},
		{"text.name", `"esc\u001b[2J"`, "ERR"},
		{"text.name", `5`, "ERR"},
		{"text.name", `null`, "ERR"},
		{"text.email", `""`, `""`},
		{"text.email", `" Bob@Example.COM "`, `"bob@example.com"`},
		{"text.email", `"bob@example"`, "ERR"},
		{"text.email", `"bo b@example.com"`, "ERR"},
		{"text.email", `"a@b@c.d"`, "ERR"},
		{"text.email", `"` + strings.Repeat("a", 110) + `@example.com"`, "ERR"},
		{"text.code", `" eng-001 "`, `"ENG-001"`},
		{"text.code", `"EN-001"`, "ERR"},
		{"text.code", `"ENG-0012"`, "ERR"},
		{"number.hours", `12.345`, `12.35`},
		{"number.hours", `0`, `0`},
		{"number.hours", `1000`, `1000`},
		{"number.hours", `1000.01`, "ERR"},
		{"number.hours", `-0.5`, "ERR"},
		{"number.hours", `"12"`, "ERR"},
		{"number.hours", `1e999`, "ERR"},
		{"number.qty", `99`, `99`},
		{"number.qty", `100`, "ERR"},
		{"number.qty", `3.5`, "ERR"},
		{"number.qty", `3.0`, `3`},
		{"number.percent", `100`, `100`},
		{"number.percent", `-1`, "ERR"},
		{"date.start", `null`, `null`},
		{"date.start", `"2026-02-28"`, `"2026-02-28"`},
		{"date.start", `"2026-02-29"`, "ERR"},
		{"date.start", `"2028-02-29"`, `"2028-02-29"`},
		{"date.start", `"2026-2-3"`, "ERR"},
		{"date.start", `"tomorrow"`, "ERR"},
		{"select.status", `"done"`, `"done"`},
		{"select.status", `"Done"`, "ERR"},
		{"select.priority", `"critical"`, `"critical"`},
		{"select.priority", `"urgent"`, "ERR"},
		{"combo.owner", `null`, `null`},
		{"combo.owner", `"ivan"`, `"ivan"`},
		{"combo.owner", `"mallory"`, "ERR"},
		{"tags.tags", `["Q4","bug","q4"," design "]`, `["bug","design","q4"]`},
		{"tags.tags", `[]`, `[]`},
		{"tags.tags", `["-x"]`, "ERR"},
		{"tags.tags", `["a b"]`, "ERR"},
		{"tags.tags", `["` + strings.Repeat("a", 25) + `"]`, "ERR"},
		{"tags.tags", `["a","b","c","d","e","f","g","h","i","j","k","l"]`, `["a","b","c","d","e","f","g","h","i","j","k","l"]`},
		{"tags.tags", `["a","b","c","d","e","f","g","h","i","j","k","l","m"]`, "ERR"},
		{"tags.tags", `"bug"`, "ERR"},
		{"textarea.notes", `"  a\r\nb  "`, `"  a\nb  "`},
		{"textarea.notes", long(5000), long(5000)},
		{"textarea.notes", long(5001), "ERR"},
		{"textarea.notes", `"tab\there"`, `"tab\there"`},
		{"choice.notify", `false`, `false`},
		{"choice.notify", `"false"`, "ERR"},
		{"choice.channels", `["slack","email","slack"]`, `["email","slack"]`},
		{"choice.channels", `[]`, `[]`},
		{"choice.channels", `["fax"]`, "ERR"},
		{"choice.size", `"xl"`, `"xl"`},
		{"choice.size", `"xxl"`, "ERR"},
		{"choice.dark", `true`, `true`},
		{"cascade.pick", `{"program":null,"project":null,"phase":null}`, `{"program":null,"project":null,"phase":null}`},
		{"cascade.pick", `{}`, `{"program":null,"project":null,"phase":null}`},
		{"cascade.pick", `{"program":"Tailspin Air"}`, `{"program":"Tailspin Air","project":null,"phase":null}`},
		{"cascade.pick", `{"program":"Tailspin Air","project":"Baggage tracking","phase":"Rollout"}`, `{"program":"Tailspin Air","project":"Baggage tracking","phase":"Rollout"}`},
		{"cascade.pick", `{"program":"Tailspin Air","project":"POS refresh"}`, "ERR"},
		{"cascade.pick", `{"project":"POS refresh"}`, "ERR"},
		{"cascade.pick", `{"program":"Northwind Retail","phase":"Build"}`, "ERR"},
		{"cascade.pick", `{"program":"Northwind Retail","project":"POS refresh","phase":"Launch"}`, "ERR"},
		{"cascade.pick", `{"program":"Nowhere"}`, "ERR"},
		{"cascade.pick", `[]`, "ERR"},
		{"grid.rows", `[]`, `[]`},
		{"grid.rows", `[{"id":7,"item":" Pen ","qty":1,"price":1.005,"status":"todo"}]`, `[{"id":7,"item":"Pen","qty":1,"price":1,"status":"todo","due":null}]`},
		{"grid.rows", `[{"id":1,"item":"a","qty":1,"price":0,"status":"todo","due":null},{"id":1,"item":"b","qty":1,"price":0,"status":"todo","due":null}]`, "ERR"},
		{"grid.rows", `[{"id":0,"item":"a","qty":1,"price":0,"status":"todo","due":null}]`, "ERR"},
		{"grid.rows", `[{"id":1,"item":"","qty":1,"price":0,"status":"todo","due":null}]`, "ERR"},
		{"grid.rows", `[{"id":1,"item":"a","qty":10000,"price":0,"status":"todo","due":null}]`, "ERR"},
		{"grid.rows", `[{"id":1,"item":"a","qty":1,"price":1000001,"status":"todo","due":null}]`, "ERR"},
		{"grid.rows", `[{"id":1,"item":"a","qty":1,"price":0,"status":"doing","due":null}]`, "ERR"},
		{"grid.rows", `[{"id":1,"item":"a","qty":1,"price":0,"status":"todo","due":"2026-13-01"}]`, "ERR"},
		{"grid.rows", `[{"id":1,"item":"a","price":0,"status":"todo"}]`, "ERR"},
		{"menus.files", `[{"name":" a.txt ","starred":true}]`, `[{"name":"a.txt","starred":true}]`},
		{"menus.files", `[{"name":"a","starred":true},{"name":"a","starred":false}]`, "ERR"},
		{"menus.files", `[{"name":"a"}]`, "ERR"},
		{"menus.files", `[{"name":"","starred":false}]`, "ERR"},
		{"palette.last", `""`, `""`},
		{"palette.last", `" Go to: Text "`, `"Go to: Text"`},
		{"palette.last", long(81), "ERR"},
		{"dialog.profile", `{"name":" G ","email":"G@X.IO"}`, `{"name":"G","email":"g@x.io"}`},
		{"dialog.profile", `{"name":"G","email":""}`, "ERR"},
		{"dialog.profile", `{"name":"","email":"g@x.io"}`, "ERR"},
		{"charts.series", `[0,1,2,3,4,5,6,7,8,9,10,100]`, `[0,1,2,3,4,5,6,7,8,9,10,100]`},
		{"charts.series", `[0,1,2,3,4,5,6,7,8,9,10]`, "ERR"},
		{"charts.series", `[0,1,2,3,4,5,6,7,8,9,10,101]`, "ERR"},
		{"charts.series", `[0,1,2,3,4,5,6,7,8,9,10,1.5]`, "ERR"},
		{"kanban.cards", `[{"id":9,"title":" T ","col":"doing"}]`, `[{"id":9,"title":"T","col":"doing"}]`},
		{"kanban.cards", `[{"id":9,"title":"T","col":"in_progress"}]`, "ERR"},
		{"kanban.cards", `[{"id":9,"title":"` + strings.Repeat("x", 61) + `","col":"todo"}]`, "ERR"},
		{"gantt.bars", `[{"id":1,"label":"x","start":"2026-01-01","days":365}]`, `[{"id":1,"label":"x","start":"2026-01-01","days":365}]`},
		{"gantt.bars", `[{"id":1,"label":"x","start":"2026-01-01","days":0}]`, "ERR"},
		{"gantt.bars", `[{"id":1,"label":"x","start":"2026-01-01","days":366}]`, "ERR"},
		{"gantt.bars", `[{"id":1,"label":"x","start":null,"days":3}]`, "ERR"},
		{"text.name", `"a" "b"`, "ERR"},
		{"text.name", `{`, "ERR"},
	}
	for _, c := range cases {
		if got := norm(c.key, c.in); got != c.want {
			t.Errorf("%s %s: got %s, want %s", c.key, c.in, got, c.want)
		}
	}
	// list limits
	rows := make([]GridRow, 51)
	for i := range rows {
		rows[i] = GridRow{ID: int64(i + 1), Item: "x", Status: "todo"}
	}
	if _, err := NormalizeValue("grid.rows", rows); err == nil {
		t.Fatal("51 rows")
	}
	if _, err := NormalizeValue("grid.rows", rows[:50]); err != nil {
		t.Fatal(err)
	}
	cards := make([]Card, 41)
	for i := range cards {
		cards[i] = Card{ID: int64(i + 1), Title: "x", Col: "todo"}
	}
	if _, err := NormalizeValue("kanban.cards", cards); err == nil {
		t.Fatal("41 cards")
	}
}

func TestCrossRule(t *testing.T) {
	cur := map[string]string{"date.start": `"2026-11-02"`, "date.due": `"2026-11-20"`}
	get := func(k string) json.RawMessage { return json.RawMessage(cur[k]) }
	cases := []struct {
		key, val string
		ok       bool
	}{
		{"date.due", `"2026-11-02"`, true},
		{"date.due", `"2026-11-01"`, false},
		{"date.due", `null`, true},
		{"date.start", `"2026-11-21"`, false},
		{"date.start", `"2026-11-20"`, true},
		{"text.name", `"x"`, true},
	}
	for _, c := range cases {
		if err := CheckCross(c.key, json.RawMessage(c.val), get); (err == nil) != c.ok {
			t.Errorf("%s=%s: %v", c.key, c.val, err)
		}
	}
	cur["date.start"] = "null"
	if CheckCross("date.due", json.RawMessage(`"2000-01-01"`), get) != nil {
		t.Fatal("start null")
	}
}

func d(s string) Date {
	r, ok := ParseISO(s)
	if !ok {
		panic(s)
	}
	return r
}

func TestDates(t *testing.T) {
	if d("1970-01-01") != 0 || d("2026-10-23").Weekday() != 4 || d("2026-10-23").Long() != "Fri 23 Oct 2026" {
		t.Fatal("basics")
	}
	for _, s := range []string{"2026-1-01", "+026-01-01", "2026-00-10", "2026-02-30", "abcd-ef-gh", "0000-01-01"} {
		if _, ok := ParseISO(s); ok {
			t.Error(s)
		}
	}
	thu := d("2026-10-22") // a Thursday
	sat := d("2026-10-24")
	cases := []struct {
		in    string
		today Date
		want  string
	}{
		{"today", thu, "2026-10-22"},
		{" T ", thu, "2026-10-22"},
		{"tomorrow", thu, "2026-10-23"},
		{"tom", thu, "2026-10-23"},
		{"yesterday", thu, "2026-10-21"},
		{"fri", thu, "2026-10-23"},
		{"Friday", thu, "2026-10-23"},
		{"thu", thu, "2026-10-29"}, // today excluded
		{"mon", thu, "2026-10-26"},
		{"+3d", thu, "2026-10-25"},
		{"+3", thu, "2026-10-25"},
		{"-2w", thu, "2026-10-08"},
		{"+1m", d("2026-01-31"), "2026-02-28"},
		{"-1m", thu, "2026-09-22"},
		{"eow", thu, "2026-10-23"},
		{"eow", d("2026-10-23"), "2026-10-23"},
		{"eow", sat, "2026-10-30"},
		{"eow", d("2026-10-25"), "2026-10-30"},
		{"eom", thu, "2026-10-31"},
		{"eom", d("2028-02-03"), "2028-02-29"},
		{"2027-03-04", thu, "2027-03-04"},
		{"23 oct", thu, "2026-10-23"},
		{"oct 23", thu, "2026-10-23"},
		{"Oct 3", thu, "2026-10-03"},
		{"3 october", thu, "2026-10-03"},
	}
	for _, c := range cases {
		got, err := ParseNatural(c.in, c.today)
		if err != nil || got == nil || got.String() != c.want {
			t.Errorf("%q: got %v %v, want %s", c.in, got, err, c.want)
		}
	}
	for _, bad := range []string{"someday", "3d", "+d", "+3x", "++3", "31 feb", "oct", "23 oct 2026", "nov 3 x", "1/2", "+99999d", "fr", "now"} {
		if got, err := ParseNatural(bad, thu); err == nil {
			t.Errorf("%q should be invalid, got %v", bad, got)
		}
	}
	if got, err := ParseNatural("   ", thu); got != nil || err != nil {
		t.Fatal("empty = null")
	}
	if p, ok := DatePreview("fri", thu); !ok || p != "fri → Fri 23 Oct 2026" {
		t.Fatal(p)
	}
	if _, ok := DatePreview("zzz", thu); ok {
		t.Fatal("preview of bad input")
	}
	if DateOf(time.Date(2026, 10, 18, 23, 0, 0, 0, time.UTC)).String() != "2026-10-18" {
		t.Fatal("DateOf")
	}
	if FormatTS(time.Date(2026, 10, 18, 9, 3, 11, 5, time.FixedZone("x", 3600))) != "2026-10-18T08:03:11Z" {
		t.Fatal("FormatTS")
	}
	now := time.Date(2026, 10, 18, 9, 10, 0, 0, time.UTC)
	if RelativeTime("2026-10-18T09:08:00Z", now) != "2m ago" || RelativeTime("2026-10-18T09:09:30Z", now) != "just now" {
		t.Fatal("relative")
	}
}

func TestFuzzy(t *testing.T) {
	m, ok := FuzzyMatch("gl", "Grace Liu")
	if !ok || !reflect.DeepEqual(m.Positions, []int{0, 6}) {
		t.Fatalf("%+v", m)
	}
	// G(10+8) + L(10+8), nothing skipped
	if m.Score != 36 {
		t.Fatal(m.Score)
	}
	m, _ = FuzzyMatch("ali", "Alice Nguyen alice Design")
	if !reflect.DeepEqual(m.Positions, []int{0, 1, 2}) || m.Score != 10*3+8+6+6 {
		t.Fatalf("%+v", m)
	}
	m, _ = FuzzyMatch("ce", "Alice")
	if m.Score != 10+10+6-3 {
		t.Fatalf("%+v", m)
	}
	if _, ok := FuzzyMatch("xyz", "Alice"); ok {
		t.Fatal("no match")
	}
	if _, ok := FuzzyMatch("aa", "a"); ok {
		t.Fatal("longer query")
	}
	if m, ok := FuzzyMatch(" ", "x"); !ok || m.Score != 0 || len(m.Positions) != 0 {
		t.Fatal("empty query")
	}
	// Unicode positions are rune indices
	if m, _ := FuzzyMatch("e", "éé e"); !reflect.DeepEqual(m.Positions, []int{3}) {
		t.Fatalf("%+v", m)
	}
	// empty query keeps order; ties keep order
	r := FuzzyRank("", []string{"b", "a"})
	if len(r) != 2 || r[0].Index != 0 {
		t.Fatal(r)
	}
	r = RankPeople("eng")
	if len(r) == 0 {
		t.Fatal("eng")
	}
	r = RankPeople("grace")
	if People[r[0].Index].Username != "grace" {
		t.Fatal(r)
	}
	r = RankPeople("qa")
	if People[r[0].Index].Username != "erin" {
		t.Fatal(r)
	}
}

func TestDataset(t *testing.T) {
	if len(People) != 9 || len(Programs) != 4 || len(TagSuggestions) != 10 {
		t.Fatal("sizes")
	}
	if got := PhasesOf("Contoso Health", "Patient portal"); len(got) != 5 {
		t.Fatal(got)
	}
	if PhasesOf("Contoso Health", "POS refresh") != nil {
		t.Fatal("wrong program")
	}
	// suggestions ranked by uses, then name
	for i := 1; i < len(TagSuggestions); i++ {
		a, b := TagSuggestions[i-1], TagSuggestions[i]
		if a.Uses < b.Uses || (a.Uses == b.Uses && a.Tag > b.Tag) {
			t.Fatal("rank", a, b)
		}
	}
	s := SuggestTags("e", []string{"frontend"})
	if len(s) == 0 || s[0].Tag != "urgent" {
		t.Fatal(s)
	}
	for _, x := range s {
		if x.Tag == "frontend" {
			t.Fatal("excluded")
		}
	}
	if PeerColor(1) != "#e06c75" || PeerColor(8) != "#f472b6" || PeerColor(9) != "#e06c75" {
		t.Fatal("colors")
	}
	if Initials("Alice Nguyen") != "AN" || Initials("bob") != "BO" || Initials("x") != "X" {
		t.Fatal(Initials("Alice Nguyen"), Initials("bob"))
	}
	if !ValidTab("gantt") || ValidTab("Gantt") {
		t.Fatal("tabs")
	}
}

func TestNames(t *testing.T) {
	good := map[string]string{"alice": "alice", "  Bob Smith  ": "Bob Smith", "a": "a", "x.y-z_1": "x.y-z_1", "A23456789012345678901234": "A23456789012345678901234"}
	for in, want := range good {
		if got, ok := CleanName(in); !ok || got != want {
			t.Errorf("%q → %q %v", in, got, ok)
		}
	}
	for _, bad := range []string{"", "   ", "1abc", "_x", "a@b", "A234567890123456789012345", "émile", "a\tb"} {
		if _, ok := CleanName(bad); ok {
			t.Errorf("%q accepted", bad)
		}
	}
	for _, id := range []string{"tui-go", "conform", "web-rust"} {
		if !ValidClientID(id) {
			t.Error(id)
		}
	}
	for _, id := range []string{"", "TUI", "a_b", strings.Repeat("a", 25)} {
		if ValidClientID(id) {
			t.Error(id)
		}
	}
	if _, err := CheckToast("  hi  "); err != nil {
		t.Fatal(err)
	}
	if _, err := CheckToast("   "); err == nil {
		t.Fatal("empty toast")
	}
	if _, err := CheckToast(strings.Repeat("é", 121)); err == nil {
		t.Fatal("long toast")
	}
	if s, err := CheckToast(strings.Repeat("é", 120)); err != nil || len([]rune(s)) != 120 {
		t.Fatal("120 toast")
	}
}

func TestProtocolShapes(t *testing.T) {
	v := int64(4)
	cases := []struct {
		msg  any
		want string
	}{
		{ChangedMsg{Key: "select.status", Value: json.RawMessage(`"done"`), Version: 4, By: "bob", Conn: 8, At: "2026-10-18T09:03:11Z"},
			`{"type":"changed","key":"select.status","value":"done","version":4,"by":"bob","conn":8,"at":"2026-10-18T09:03:11Z"}`},
		{AckMsg{Req: 12, OK: true, Version: &v}, `{"type":"ack","req":12,"ok":true,"version":4}`},
		{AckMsg{Req: 15, OK: true}, `{"type":"ack","req":15,"ok":true}`},
		{AckMsg{Req: 13, Error: "due must be on or after start"}, `{"type":"ack","req":13,"ok":false,"error":"due must be on or after start"}`},
		{AckMsg{Req: 14, Conflict: true, Current: &Entry{Value: json.RawMessage(`"review"`), Version: 5, By: "bob", At: "t"}},
			`{"type":"ack","req":14,"ok":false,"conflict":true,"current":{"value":"review","version":5,"by":"bob","at":"t"}}`},
		{PresenceMsg{}, `{"type":"presence","peers":[]}`},
		{PresenceMsg{Peers: []Peer{{Conn: 7, Name: "alice", Color: "#e06c75", Client: "tui-rust"}}},
			`{"type":"presence","peers":[{"conn":7,"name":"alice","color":"#e06c75","client":"tui-rust","tab":null}]}`},
		{ToastMsg{Text: "Lunch & <tea>", By: "bob", Conn: 8}, `{"type":"toast","text":"Lunch & <tea>","by":"bob","conn":8}`},
		{PongMsg{}, `{"type":"pong"}`},
		{ErrorMsg{Message: "bad message"}, `{"type":"error","message":"bad message"}`},
	}
	for _, c := range cases {
		b, err := EncodeServerMsg(c.msg)
		if err != nil || string(b) != c.want {
			t.Errorf("got  %s %v\nwant %s", b, err, c.want)
			continue
		}
		back, err := DecodeServerMsg(b)
		if err != nil {
			t.Error(err)
			continue
		}
		again, _ := EncodeServerMsg(back)
		if string(again) != c.want {
			t.Errorf("round trip: %s", again)
		}
	}
	w := WelcomeMsg{Protocol: 1, Server: ServerInfo{"go", Version}, You: You{7, "alice", "#e06c75"},
		Values: map[string]Entry{"text.name": {Value: Default("text.name"), Version: 1, By: "", At: "x"}}}
	b, _ := EncodeServerMsg(w)
	if string(b) != `{"type":"welcome","protocol":1,"server":{"impl":"go","version":"0.5.0"},"you":{"conn":7,"name":"alice","color":"#e06c75"},"values":{"text.name":{"value":"Ada Lovelace","version":1,"by":"","at":"x"}}}` {
		t.Fatal(string(b))
	}
}

func TestClientMessages(t *testing.T) {
	set, _ := NewSet(12, "select.status", 3, "done")
	cases := []struct {
		msg  any
		want string
	}{
		{set, `{"type":"set","req":12,"key":"select.status","base":3,"value":"done"}`},
		{SetMsg{Req: 1, Key: "date.start", Base: 2, Value: json.RawMessage("null"), Force: true}, `{"type":"set","req":1,"key":"date.start","base":2,"value":null,"force":true}`},
		{ResetMsg{Req: 15, Keys: []string{"date.start", "date.due"}}, `{"type":"reset","req":15,"keys":["date.start","date.due"]}`},
		{ResetMsg{Req: 15}, `{"type":"reset","req":15,"keys":[]}`},
		{FocusMsg{Tab: "date"}, `{"type":"focus","tab":"date"}`},
		{SendToastMsg{Req: 16, Text: "Lunch is here"}, `{"type":"toast","req":16,"text":"Lunch is here"}`},
		{PingMsg{}, `{"type":"ping"}`},
	}
	for _, c := range cases {
		b, err := EncodeClientMsg(c.msg)
		if err != nil || string(b) != c.want {
			t.Errorf("got  %s %v\nwant %s", b, err, c.want)
			continue
		}
		back, err := DecodeClientMsg(b)
		if err != nil {
			t.Error(err)
			continue
		}
		again, _ := EncodeClientMsg(back)
		if string(again) != c.want {
			t.Errorf("round trip: %s", again)
		}
	}
	// set with value null is a value, not a missing field
	m, err := DecodeClientMsg([]byte(`{"type":"set","req":1,"key":"date.due","base":1,"value":null}`))
	if err != nil || string(m.(SetMsg).Value) != "null" {
		t.Fatal(m, err)
	}
	// missing base → 0
	m, _ = DecodeClientMsg([]byte(`{"type":"set","req":1,"key":"k","value":1}`))
	if m.(SetMsg).Base != 0 {
		t.Fatal("base")
	}
	if m, _ := DecodeClientMsg([]byte(`{"type":"reset","req":2}`)); len(m.(ResetMsg).Keys) != 0 {
		t.Fatal("reset all")
	}
	for _, bad := range []string{``, `nope`, `[]`, `{}`, `{"type":"nope"}`, `{"type":5}`, `{"type":"set","key":"k","value":1}`,
		`{"type":"set","req":1,"value":1}`, `{"type":"set","req":1,"key":"k"}`, `{"type":"set","req":-1,"key":"k","value":1}`,
		`{"type":"set","req":1,"key":"k","base":"3","value":1}`, `{"type":"reset"}`, `{"type":"reset","req":1,"keys":"x"}`,
		`{"type":"focus"}`, `{"type":"toast","req":1}`, `{"type":"toast","text":"x"}`} {
		if _, err := DecodeClientMsg([]byte(bad)); err == nil {
			t.Errorf("%s accepted", bad)
		}
	}
}

func TestValues(t *testing.T) {
	v := NewValues()
	if v.Loaded() || v.String("text.name") != "Ada Lovelace" || v.Version("text.name") != 0 {
		t.Fatal("defaults")
	}
	w := WelcomeMsg{Protocol: 1, Server: ServerInfo{"rust", Version}, You: You{Conn: 2, Name: "bob"}, Values: map[string]Entry{}}
	for _, k := range Keys {
		w.Values[k] = Entry{Value: Default(k), Version: 3, At: "2026-10-18T09:00:00Z"}
	}
	w.Values["junk"] = Entry{Value: json.RawMessage(`1`)}
	v.Apply(w)
	if !v.Loaded() || v.Version("date.due") != 3 || v.Server.Impl != "rust" || v.Get("junk").Version != 0 {
		t.Fatal("welcome")
	}
	if v.ApplyChanged(ChangedMsg{Key: "select.status", Value: json.RawMessage(`"done"`), Version: 2}) {
		t.Fatal("older version applied")
	}
	if !v.Apply(ChangedMsg{Key: "select.status", Value: json.RawMessage(`"done"`), Version: 4, By: "amy", At: "2026-10-18T09:00:00Z"}) || v.String("select.status") != "done" {
		t.Fatal("changed")
	}
	if !strings.HasPrefix(v.Describe("select.status", time.Date(2026, 10, 18, 9, 2, 0, 0, time.UTC)), "v4 · by amy · 2m ago") {
		t.Fatal(v.Describe("select.status", time.Now()))
	}
	if v.Describe("text.name", time.Now()) != "v3 · default" {
		t.Fatal(v.Describe("text.name", time.Now()))
	}
	// typed getters on defaults
	if v.Float("number.hours") != 12.5 || v.Int("number.qty") != 3 || !v.Bool("choice.notify") || v.OptString("combo.reviewer") != nil ||
		len(v.Strings("tags.tags")) != 2 || *v.Cascade().Phase != "Build" || len(v.GridRows()) != 6 || v.GridRows()[3].Due != nil ||
		len(v.Files()) != 8 || !v.Files()[1].Starred || v.Profile().Email != "grace@example.com" || len(v.Series()) != 12 ||
		len(v.Cards()) != 8 || v.Bars()[5].Days != 1 || v.GridRows()[2].Price != 289.99 {
		t.Fatal("getters")
	}
	if dd, ok := v.Date("date.start"); !ok || dd.String() != "2026-11-02" {
		t.Fatal("date")
	}
	// a malformed held value falls back to the default
	v.ApplyEntry("number.qty", Entry{Value: json.RawMessage(`"x"`), Version: 9})
	if v.Int("number.qty") != 3 {
		t.Fatal("fallback")
	}
	// PrepareSet validates and bases on the held version
	s, err := v.PrepareSet("text.code", "abc-123")
	if err != nil || s.Base != 3 || string(s.Value) != `"ABC-123"` {
		t.Fatal(s, err)
	}
	if _, err := v.PrepareSet("date.due", "2026-11-01"); err != ErrDueBeforeStart {
		t.Fatal(err)
	}
	if _, err := v.PrepareSet("number.qty", 100); err == nil {
		t.Fatal("qty")
	}
	if s, err := v.PrepareSet("date.due", nil); err != nil || string(s.Value) != "null" {
		t.Fatal(err)
	}
	v.ApplyPresence(PresenceMsg{Peers: []Peer{{Conn: 5, Name: "x"}, {Conn: 2, Name: "bob"}}})
	if v.Peers[0].Conn != 2 || len(v.OtherPeers()) != 1 {
		t.Fatal("presence")
	}
	if NextID([]int64{3, 9, 2}) != 10 || NextID(nil) != 1 {
		t.Fatal("NextID")
	}
	if SafeText("a\x1b[2Jb\nc") != "a�[2Jb\nc" || SafeText("plain") != "plain" {
		t.Fatal(SafeText("a\x1b[2Jb\nc"))
	}
}

// FuzzNormalize: no input may panic the validators.
func FuzzNormalize(f *testing.F) {
	for _, k := range Keys {
		f.Add(k, string(Default(k)))
	}
	f.Add("grid.rows", `[{"id":1e300}]`)
	f.Fuzz(func(t *testing.T, key, value string) {
		out, err := Normalize(key, json.RawMessage(value))
		if err == nil {
			// canonical output is stable
			again, err := Normalize(key, out)
			if err != nil || string(again) != string(out) {
				t.Fatalf("%s: %s → %s → %s %v", key, value, out, again, err)
			}
		}
		_, _ = DecodeClientMsg([]byte(value))
		_, _ = ParseNatural(value, 20000)
		_, _ = FuzzyMatch(key, value)
	})
}
