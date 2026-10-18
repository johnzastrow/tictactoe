// Package core is demo's pure core (docs/demo/PLAN.md): the key catalogue
// with validation, normalisation and defaults, the built-in dataset, natural
// dates, fuzzy matching, the wire messages and the client-side value store.
// It does no I/O and compiles for GOOS=js GOARCH=wasm, so the server, the
// TUI and the web client all share it.
package core

import "strings"

// Version is the demo release (both implementations share it).
const Version = "0.5.0"

// ---- tabs (ui-spec §4) ----

// Tab is one tab of the gallery. ID is what `focus` messages carry.
type Tab struct {
	Num   int      // 1–16, display order
	ID    string   // e.g. "select"
	Title string   // e.g. "Dropdown"
	Keys  []string // the control keys this tab edits (may be empty)
}

// Tabs lists the 16 tabs in display order.
var Tabs = []Tab{
	{1, "text", "Text", []string{"text.name", "text.email", "text.code"}},
	{2, "number", "Number", []string{"number.hours", "number.qty", "number.percent"}},
	{3, "date", "Date", []string{"date.start", "date.due"}},
	{4, "select", "Dropdown", []string{"select.status", "select.priority"}},
	{5, "combo", "Combobox", []string{"combo.owner", "combo.reviewer"}},
	{6, "tags", "Tags", []string{"tags.tags"}},
	{7, "textarea", "Text area", []string{"textarea.notes"}},
	{8, "choice", "Choices", []string{"choice.notify", "choice.channels", "choice.size", "choice.dark"}},
	{9, "cascade", "Cascade", []string{"cascade.pick"}},
	{10, "grid", "Grid", []string{"grid.rows"}},
	{11, "menus", "Menus", []string{"menus.files"}},
	{12, "palette", "Palette", []string{"palette.last"}},
	{13, "dialogs", "Dialogs", []string{"dialog.profile"}},
	{14, "charts", "Charts", []string{"charts.series"}},
	{15, "kanban", "Kanban", []string{"kanban.cards"}},
	{16, "gantt", "Gantt", []string{"gantt.bars"}},
}

// TabByID returns the tab with this id.
func TabByID(id string) (Tab, bool) {
	for _, t := range Tabs {
		if t.ID == id {
			return t, true
		}
	}
	return Tab{}, false
}

// ValidTab reports whether id is one of the 16 tab ids.
func ValidTab(id string) bool { _, ok := TabByID(id); return ok }

// ---- people ----

type Person struct {
	Username string
	Name     string
	Team     string
}

// SearchText is what the combobox fuzzy-matches: "name username team".
func (p Person) SearchText() string { return p.Name + " " + p.Username + " " + p.Team }

// Initials: "Alice Nguyen" → "AN".
func Initials(name string) string {
	var b strings.Builder
	for _, w := range strings.Fields(name) {
		for _, r := range w {
			b.WriteString(strings.ToUpper(string(r)))
			break
		}
		if b.Len() >= 2 {
			break
		}
	}
	if b.Len() == 1 {
		// one word: first two letters ("bob" → "BO")
		rs := []rune(strings.TrimSpace(name))
		if len(rs) > 1 {
			b.WriteString(strings.ToUpper(string(rs[1])))
		}
	}
	return b.String()
}

var People = []Person{
	{"alice", "Alice Nguyen", "Design"},
	{"bob", "Bob Okafor", "Engineering"},
	{"carol", "Carol Diaz", "Engineering"},
	{"dave", "Dave Kowalski", "Product"},
	{"erin", "Erin Walsh", "QA"},
	{"frank", "Frank Moreau", "Operations"},
	{"grace", "Grace Liu", "Engineering"},
	{"heidi", "Heidi Schmidt", "Design"},
	{"ivan", "Ivan Petrov", "Support"},
}

// PersonByUsername looks a person up by username.
func PersonByUsername(u string) (Person, bool) {
	for _, p := range People {
		if p.Username == u {
			return p, true
		}
	}
	return Person{}, false
}

// ---- programs ▸ projects ▸ phases ----

type Project struct {
	Name   string
	Phases []string
}

type Program struct {
	Name     string
	Projects []Project
}

var Programs = []Program{
	{"Northwind Retail", []Project{
		{"POS refresh", []string{"Discover", "Build", "Pilot", "Rollout"}},
		{"Loyalty app", []string{"Design", "Build", "Launch"}},
		{"Store Wi-Fi", []string{"Survey", "Install", "Handover"}},
	}},
	{"Contoso Health", []Project{
		{"Patient portal", []string{"Discover", "Design", "Build", "Test", "Launch"}},
		{"Records migration", []string{"Plan", "Migrate", "Verify"}},
	}},
	{"Fabrikam Energy", []Project{
		{"Smart meters", []string{"Pilot", "Deploy", "Monitor"}},
		{"Grid analytics", []string{"Data", "Model", "Report"}},
		{"Field app", []string{"Design", "Build", "Test"}},
	}},
	{"Tailspin Air", []Project{
		{"Crew scheduling", []string{"Define", "Build", "Test", "Go-live"}},
		{"Baggage tracking", []string{"Pilot", "Rollout"}},
	}},
}

// ProgramByName looks a program up by its exact name.
func ProgramByName(name string) (Program, bool) {
	for _, p := range Programs {
		if p.Name == name {
			return p, true
		}
	}
	return Program{}, false
}

// ProjectsOf returns a program's projects (nil for an unknown program).
func ProjectsOf(program string) []Project {
	p, _ := ProgramByName(program)
	return p.Projects
}

// PhasesOf returns a project's phases (nil if either name is unknown or the
// project is not in that program).
func PhasesOf(program, project string) []string {
	for _, j := range ProjectsOf(program) {
		if j.Name == project {
			return j.Phases
		}
	}
	return nil
}

// ---- tags ----

type TagUse struct {
	Tag  string
	Uses int
}

// TagSuggestions are ranked by uses (descending), then name.
var TagSuggestions = []TagUse{
	{"urgent", 12}, {"frontend", 9}, {"backend", 8}, {"q4", 7}, {"design", 6},
	{"bug", 5}, {"research", 4}, {"docs", 3}, {"security", 3}, {"perf", 2},
}

// SuggestTags filters the suggestions for the tags input: those whose tag
// contains query (case-insensitive; all when query is empty), minus any in
// exclude, in suggestion rank order.
func SuggestTags(query string, exclude []string) []TagUse {
	q := strings.ToLower(strings.TrimSpace(query))
	var out []TagUse
	for _, t := range TagSuggestions {
		if contains(exclude, t.Tag) || !strings.Contains(t.Tag, q) {
			continue
		}
		out = append(out, t)
	}
	return out
}

func contains(list []string, s string) bool {
	for _, x := range list {
		if x == s {
			return true
		}
	}
	return false
}

// ---- enumerations ----

// Choice is one allowed value of an enumerated key: id (stored), label
// (shown) and an optional colour (#rrggbb).
type Choice struct {
	ID    string
	Label string
	Color string
}

var Statuses = []Choice{
	{"todo", "To do", "#9aa5b1"},
	{"in_progress", "In progress", "#61afef"},
	{"blocked", "Blocked", "#e06c75"},
	{"review", "Review", "#c678dd"},
	{"done", "Done", "#98c379"},
}

var Priorities = []Choice{
	{"low", "Low", "#7f848e"},
	{"medium", "Medium", "#61afef"},
	{"high", "High", "#e5c07b"},
	{"critical", "Critical", "#e06c75"},
}

// Channels: choice.channels, in canonical order.
var Channels = []Choice{{"email", "Email", ""}, {"sms", "SMS", ""}, {"push", "Push", ""}, {"slack", "Slack", ""}}

// Sizes: choice.size.
var Sizes = []Choice{{"s", "S", ""}, {"m", "M", ""}, {"l", "L", ""}, {"xl", "XL", ""}}

// KanbanColumns: kanban.cards col values, in board order.
var KanbanColumns = []Choice{{"todo", "To do", ""}, {"doing", "Doing", ""}, {"review", "Review", ""}, {"done", "Done", ""}}

// FindChoice returns the entry with this id.
func FindChoice(list []Choice, id string) (Choice, bool) {
	for _, c := range list {
		if c.ID == id {
			return c, true
		}
	}
	return Choice{}, false
}

func choiceIndex(list []Choice, id string) int {
	for i, c := range list {
		if c.ID == id {
			return i
		}
	}
	return -1
}

// ---- peers ----

// PeerColors are assigned by connection number (protocol §2).
var PeerColors = []string{"#e06c75", "#61afef", "#98c379", "#e5c07b", "#c678dd", "#56b6c2", "#d19a66", "#f472b6"}

// PeerColor is PEER_COLORS[(conn - 1) % 8]; conn starts at 1.
func PeerColor(conn uint64) string {
	if conn == 0 {
		return PeerColors[0]
	}
	return PeerColors[(conn-1)%uint64(len(PeerColors))]
}
