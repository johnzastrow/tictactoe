package core

// The key catalogue (protocol §5): every control key with its tab, default
// and validator. Values travel as JSON; Normalize checks one against its
// key's rule and returns the canonical JSON the server stores (trimmed,
// lowercased, rounded, deduped, … as the table says).

import (
	"bytes"
	"encoding/json"
	"errors"
	"fmt"
	"math"
	"sort"
	"strconv"
	"strings"
	"unicode"
	"unicode/utf8"
)

// Typed shapes of the structured values. Field order is wire order.

// Cascade is cascade.pick: each level a name or nil.
type Cascade struct {
	Program *string `json:"program"`
	Project *string `json:"project"`
	Phase   *string `json:"phase"`
}

// GridRow is one row of grid.rows. Due is YYYY-MM-DD or nil.
type GridRow struct {
	ID     int64   `json:"id"`
	Item   string  `json:"item"`
	Qty    int64   `json:"qty"`
	Price  float64 `json:"price"`
	Status string  `json:"status"`
	Due    *string `json:"due"`
}

// File is one entry of menus.files.
type File struct {
	Name    string `json:"name"`
	Starred bool   `json:"starred"`
}

// Profile is dialog.profile.
type Profile struct {
	Name  string `json:"name"`
	Email string `json:"email"`
}

// Card is one kanban.cards card; Col is a KanbanColumns id.
type Card struct {
	ID    int64  `json:"id"`
	Title string `json:"title"`
	Col   string `json:"col"`
}

// Bar is one gantt.bars bar; Start is YYYY-MM-DD.
type Bar struct {
	ID    int64  `json:"id"`
	Label string `json:"label"`
	Start string `json:"start"`
	Days  int64  `json:"days"`
}

// DefaultNotes is textarea.notes' default.
const DefaultNotes = "Welcome to demo.\nEach tab shows one control on its own.\nChanges are shared live with everyone connected."

type keySpec struct {
	key   string
	tab   string
	def   string                   // canonical JSON
	check func(v any) (any, error) // decoded JSON → normalised Go value
}

// specs is §5 in table order.
var specs = []keySpec{
	{"text.name", "text", `"Ada Lovelace"`, func(v any) (any, error) { return str(v, 1, 80, true) }},
	{"text.email", "text", `"ada@example.com"`, func(v any) (any, error) { return email(v, false) }},
	{"text.code", "text", `"ENG-042"`, checkCode},
	{"number.hours", "number", `12.5`, func(v any) (any, error) { return num2dp(v, 0, 1000) }},
	{"number.qty", "number", `3`, func(v any) (any, error) { return integer(v, 0, 99) }},
	{"number.percent", "number", `40`, func(v any) (any, error) { return integer(v, 0, 100) }},
	{"date.start", "date", `"2026-11-02"`, func(v any) (any, error) { return dateOrNull(v) }},
	{"date.due", "date", `"2026-11-20"`, func(v any) (any, error) { return dateOrNull(v) }},
	{"select.status", "select", `"in_progress"`, func(v any) (any, error) { return oneOf(v, Statuses, "status") }},
	{"select.priority", "select", `"high"`, func(v any) (any, error) { return oneOf(v, Priorities, "priority") }},
	{"combo.owner", "combo", `"carol"`, personOrNull},
	{"combo.reviewer", "combo", `null`, personOrNull},
	{"tags.tags", "tags", `["frontend","q4"]`, checkTags},
	{"textarea.notes", "textarea", mustJSON(DefaultNotes), checkNotes},
	{"choice.notify", "choice", `true`, boolean},
	{"choice.channels", "choice", `["email","push"]`, checkChannels},
	{"choice.size", "choice", `"m"`, func(v any) (any, error) { return oneOf(v, Sizes, "size") }},
	{"choice.dark", "choice", `false`, boolean},
	{"cascade.pick", "cascade", `{"program":"Northwind Retail","project":"POS refresh","phase":"Build"}`, checkCascade},
	{"grid.rows", "grid", `[{"id":1,"item":"Laptop stand","qty":4,"price":39.5,"status":"done","due":"2026-10-30"},` +
		`{"id":2,"item":"USB-C dock","qty":2,"price":129,"status":"review","due":"2026-11-04"},` +
		`{"id":3,"item":"Monitor 27in","qty":3,"price":289.99,"status":"in_progress","due":"2026-11-12"},` +
		`{"id":4,"item":"Desk lamp","qty":6,"price":24.75,"status":"todo","due":null},` +
		`{"id":5,"item":"Keyboard","qty":5,"price":89,"status":"blocked","due":"2026-11-20"},` +
		`{"id":6,"item":"Headset","qty":2,"price":149,"status":"todo","due":"2026-12-01"}]`, checkGrid},
	{"menus.files", "menus", `[{"name":"Budget 2027.xlsx","starred":false},{"name":"Roadmap.pdf","starred":true},` +
		`{"name":"Team photo.png","starred":false},{"name":"Meeting notes.md","starred":true},` +
		`{"name":"Invoice 1042.pdf","starred":false},{"name":"Logo.svg","starred":false},` +
		`{"name":"Contracts.zip","starred":false},{"name":"Ideas.txt","starred":false}]`, checkFiles},
	{"palette.last", "palette", `""`, func(v any) (any, error) { return str(v, 0, 80, true) }},
	{"dialog.profile", "dialogs", `{"name":"Grace Hopper","email":"grace@example.com"}`, checkProfile},
	{"charts.series", "charts", `[35,48,52,61,58,70,76,72,64,55,43,38]`, checkSeries},
	{"kanban.cards", "kanban", `[{"id":1,"title":"Write brief","col":"done"},{"id":2,"title":"Sketch layouts","col":"review"},` +
		`{"id":3,"title":"Pick colours","col":"doing"},{"id":4,"title":"Build prototype","col":"doing"},` +
		`{"id":5,"title":"User test","col":"todo"},{"id":6,"title":"Fix feedback","col":"todo"},` +
		`{"id":7,"title":"Write docs","col":"todo"},{"id":8,"title":"Launch","col":"todo"}]`, checkCards},
	{"gantt.bars", "gantt", `[{"id":1,"label":"Discovery","start":"2026-10-19","days":10},` +
		`{"id":2,"label":"Design","start":"2026-10-26","days":14},{"id":3,"label":"Build","start":"2026-11-09","days":21},` +
		`{"id":4,"label":"Test","start":"2026-11-23","days":12},{"id":5,"label":"Pilot","start":"2026-12-07","days":7},` +
		`{"id":6,"label":"Launch","start":"2026-12-14","days":1}]`, checkBars},
}

var specByKey = func() map[string]*keySpec {
	m := make(map[string]*keySpec, len(specs))
	for i := range specs {
		m[specs[i].key] = &specs[i]
	}
	return m
}()

// Keys lists every control key in catalogue (§5) order.
var Keys = func() []string {
	out := make([]string, len(specs))
	for i, s := range specs {
		out[i] = s.key
	}
	return out
}()

// ErrUnknownKey is returned for a key not in the catalogue.
var ErrUnknownKey = errors.New("unknown key")

// IsKey reports whether key is in the catalogue.
func IsKey(key string) bool { _, ok := specByKey[key]; return ok }

// KeyTab returns the tab id a key belongs to ("" if unknown).
func KeyTab(key string) string {
	if s, ok := specByKey[key]; ok {
		return s.tab
	}
	return ""
}

// Default returns a key's default value as canonical JSON (nil if unknown).
// The slice is a fresh copy.
func Default(key string) json.RawMessage {
	if s, ok := specByKey[key]; ok {
		return json.RawMessage(s.def)
	}
	return nil
}

// Normalize validates value (JSON) against key's rule and returns its
// canonical JSON. Errors are short English phrases suitable for an ack.
// It never panics, whatever the input.
func Normalize(key string, value json.RawMessage) (json.RawMessage, error) {
	s, ok := specByKey[key]
	if !ok {
		return nil, ErrUnknownKey
	}
	d := json.NewDecoder(bytes.NewReader(value))
	d.UseNumber()
	var v any
	if err := d.Decode(&v); err != nil {
		return nil, errors.New("value is not valid JSON")
	}
	if d.More() {
		return nil, errors.New("value is not valid JSON")
	}
	out, err := s.check(v)
	if err != nil {
		return nil, err
	}
	return marshal(out)
}

// NormalizeValue is Normalize for a Go value (string, float64, []string,
// Cascade, []GridRow, nil, …), as the clients build them.
func NormalizeValue(key string, v any) (json.RawMessage, error) {
	b, err := marshal(v)
	if err != nil {
		return nil, err
	}
	return Normalize(key, b)
}

// ErrDueBeforeStart is the cross-key rule's error (protocol §3 rule 5).
var ErrDueBeforeStart = errors.New("due must be on or after start")

// CheckCross applies the cross-key rule to a normalised value about to be
// stored for key: date.due ≥ date.start when both are set. current returns
// another key's current (canonical) value.
func CheckCross(key string, value json.RawMessage, current func(key string) json.RawMessage) error {
	var start, due json.RawMessage
	switch key {
	case "date.start":
		start, due = value, current("date.due")
	case "date.due":
		start, due = current("date.start"), value
	default:
		return nil
	}
	var s, d *string
	if json.Unmarshal(start, &s) != nil || json.Unmarshal(due, &d) != nil {
		return nil // not dates: the other key's own rule covers it
	}
	// ISO dates compare correctly as strings.
	if s != nil && d != nil && *d < *s {
		return ErrDueBeforeStart
	}
	return nil
}

// marshal encodes without HTML escaping and without a trailing newline.
func marshal(v any) (json.RawMessage, error) {
	var buf bytes.Buffer
	e := json.NewEncoder(&buf)
	e.SetEscapeHTML(false)
	if err := e.Encode(v); err != nil {
		return nil, err
	}
	return bytes.TrimRight(buf.Bytes(), "\n"), nil
}

func mustJSON(v any) string {
	b, err := marshal(v)
	if err != nil {
		panic(err) // static default values only
	}
	return string(b)
}

// ---- primitive checks (v is decoded with UseNumber) ----

// hasControl: C0/C1 control characters (other than those allowed) would let
// one peer inject terminal escape sequences into another's TUI.
func hasControl(s string, allowNewline bool) bool {
	for _, r := range s {
		if unicode.IsControl(r) && !(allowNewline && (r == '\n' || r == '\t')) {
			return true
		}
	}
	return false
}

// ValidText reports whether s is free of control characters (single-line
// text); clients can use it before sending.
func ValidText(s string) bool { return !hasControl(s, false) }

func plural(n int, word string) string {
	if n == 1 {
		return fmt.Sprintf("%d %s", n, word)
	}
	return fmt.Sprintf("%d %ss", n, word)
}

// str: a trimmed string of lo–hi characters (runes) without control chars.
func str(v any, lo, hi int, trim bool) (string, error) {
	s, ok := v.(string)
	if !ok {
		return "", errors.New("must be a string")
	}
	if trim {
		s = strings.TrimSpace(s)
	}
	n := utf8.RuneCountInString(s)
	switch {
	case n < lo && lo == 1:
		return "", errors.New("must not be empty")
	case n < lo || n > hi:
		return "", fmt.Errorf("must be %d–%d characters", lo, hi)
	case hasControl(s, false):
		return "", errors.New("must not contain control characters")
	}
	return s, nil
}

// ValidEmail: ^[^@\s]+@[^@\s]+\.[^@\s]+$ (\s as Unicode whitespace).
func ValidEmail(s string) bool {
	at := strings.IndexByte(s, '@')
	if at <= 0 || strings.ContainsFunc(s, unicode.IsSpace) || strings.Count(s, "@") != 1 {
		return false
	}
	domain := s[at+1:]
	dot := strings.LastIndexByte(domain, '.')
	return dot > 0 && dot < len(domain)-1
}

func email(v any, required bool) (string, error) {
	s, ok := v.(string)
	if !ok {
		return "", errors.New("must be a string")
	}
	s = strings.ToLower(strings.TrimSpace(s))
	switch {
	case s == "" && !required:
		return "", nil
	case s == "":
		return "", errors.New("email is required")
	case utf8.RuneCountInString(s) > 120:
		return "", errors.New("email must be at most 120 characters")
	case !ValidEmail(s) || hasControl(s, false):
		return "", errors.New("must look like name@example.com")
	}
	return s, nil
}

func checkCode(v any) (any, error) {
	s, ok := v.(string)
	if !ok {
		return nil, errors.New("must be a string")
	}
	s = strings.ToUpper(strings.TrimSpace(s))
	if !ValidCode(s) {
		return nil, errors.New("must look like ABC-123")
	}
	return s, nil
}

// ValidCode: ^[A-Z]{3}-[0-9]{3}$.
func ValidCode(s string) bool {
	if len(s) != 7 || s[3] != '-' {
		return false
	}
	for i := 0; i < 7; i++ {
		c := s[i]
		switch {
		case i < 3 && (c < 'A' || c > 'Z'):
			return false
		case i > 3 && (c < '0' || c > '9'):
			return false
		}
	}
	return true
}

// number reads a finite JSON number.
func number(v any) (float64, error) {
	n, ok := v.(json.Number)
	if !ok {
		return 0, errors.New("must be a number")
	}
	f, err := strconv.ParseFloat(string(n), 64)
	if err != nil || math.IsInf(f, 0) || math.IsNaN(f) {
		return 0, errors.New("must be a finite number")
	}
	return f, nil
}

// Round2 rounds to 2 decimal places (half away from zero).
func Round2(f float64) float64 {
	r := math.Round(f*100) / 100
	if r == 0 {
		return 0 // no "-0"
	}
	return r
}

func num2dp(v any, lo, hi float64) (float64, error) {
	f, err := number(v)
	if err != nil {
		return 0, err
	}
	if f < lo || f > hi {
		return 0, fmt.Errorf("must be between %s and %s", fmtNum(lo), fmtNum(hi))
	}
	return Round2(f), nil
}

func fmtNum(f float64) string { return strconv.FormatFloat(f, 'f', -1, 64) }

// integer: a JSON number with no fractional part, lo ≤ n ≤ hi.
func integer(v any, lo, hi int64) (int64, error) {
	f, err := number(v)
	if err != nil {
		return 0, err
	}
	if f != math.Trunc(f) {
		return 0, errors.New("must be a whole number")
	}
	if f < float64(lo) || f > float64(hi) {
		return 0, fmt.Errorf("must be between %d and %d", lo, hi)
	}
	return int64(f), nil
}

func boolean(v any) (any, error) {
	b, ok := v.(bool)
	if !ok {
		return nil, errors.New("must be true or false")
	}
	return b, nil
}

// isoDate: a real calendar date as YYYY-MM-DD.
func isoDate(v any) (string, error) {
	s, ok := v.(string)
	if !ok {
		return "", errors.New("date must be a string YYYY-MM-DD")
	}
	s = strings.TrimSpace(s)
	if _, ok := ParseISO(s); !ok {
		return "", fmt.Errorf("%q is not a date (YYYY-MM-DD)", truncate(s, 20))
	}
	return s, nil
}

func dateOrNull(v any) (*string, error) {
	if v == nil {
		return nil, nil
	}
	s, err := isoDate(v)
	if err != nil {
		return nil, err
	}
	return &s, nil
}

func truncate(s string, n int) string {
	rs := []rune(s)
	if len(rs) > n {
		return string(rs[:n]) + "…"
	}
	return s
}

func oneOf(v any, list []Choice, what string) (string, error) {
	s, ok := v.(string)
	if !ok {
		return "", fmt.Errorf("%s must be a string", what)
	}
	s = strings.TrimSpace(s)
	if choiceIndex(list, s) < 0 {
		ids := make([]string, len(list))
		for i, c := range list {
			ids[i] = c.ID
		}
		return "", fmt.Errorf("%s must be one of %s", what, strings.Join(ids, ", "))
	}
	return s, nil
}

func personOrNull(v any) (any, error) {
	if v == nil {
		return nil, nil
	}
	s, ok := v.(string)
	if !ok {
		return nil, errors.New("must be a username or null")
	}
	s = strings.TrimSpace(s)
	if _, ok := PersonByUsername(s); !ok {
		return nil, fmt.Errorf("no such person %q", truncate(s, 24))
	}
	return s, nil
}

func list(v any, max int, what string) ([]any, error) {
	l, ok := v.([]any)
	if !ok {
		return nil, fmt.Errorf("%s must be a list", what)
	}
	if len(l) > max {
		return nil, fmt.Errorf("at most %s", plural(max, what))
	}
	return l, nil
}

func object(v any, what string) (map[string]any, error) {
	m, ok := v.(map[string]any)
	if !ok {
		return nil, fmt.Errorf("%s must be an object", what)
	}
	return m, nil
}

// field reads a required member.
func field(m map[string]any, name string) (any, error) {
	v, ok := m[name]
	if !ok {
		return nil, fmt.Errorf("missing %q", name)
	}
	return v, nil
}

// ---- structured values ----

// ValidTag: ^[a-z0-9][a-z0-9_-]{0,23}$ (after lowercasing).
func ValidTag(s string) bool {
	if len(s) == 0 || len(s) > 24 {
		return false
	}
	for i := 0; i < len(s); i++ {
		c := s[i]
		alnum := c >= 'a' && c <= 'z' || c >= '0' && c <= '9'
		if !alnum && (i == 0 || (c != '_' && c != '-')) {
			return false
		}
	}
	return true
}

func checkTags(v any) (any, error) {
	l, ok := v.([]any)
	if !ok {
		return nil, errors.New("tags must be a list")
	}
	seen := map[string]bool{}
	out := []string{}
	for _, x := range l {
		s, ok := x.(string)
		if !ok {
			return nil, errors.New("each tag must be a string")
		}
		s = strings.ToLower(strings.TrimSpace(s))
		if !ValidTag(s) {
			return nil, fmt.Errorf("bad tag %q: use a–z, 0–9, _ or -, up to 24", truncate(s, 24))
		}
		if !seen[s] {
			seen[s] = true
			out = append(out, s)
		}
	}
	if len(out) > 12 {
		return nil, errors.New("at most 12 tags")
	}
	sort.Strings(out)
	return out, nil
}

func checkNotes(v any) (any, error) {
	s, ok := v.(string)
	if !ok {
		return nil, errors.New("must be a string")
	}
	s = strings.ReplaceAll(s, "\r\n", "\n")
	if utf8.RuneCountInString(s) > 5000 {
		return nil, errors.New("must be at most 5000 characters")
	}
	if hasControl(s, true) {
		return nil, errors.New("must not contain control characters")
	}
	return s, nil
}

func checkChannels(v any) (any, error) {
	l, ok := v.([]any)
	if !ok {
		return nil, errors.New("channels must be a list")
	}
	have := make([]bool, len(Channels))
	for _, x := range l {
		s, ok := x.(string)
		if !ok {
			return nil, errors.New("each channel must be a string")
		}
		i := choiceIndex(Channels, strings.TrimSpace(s))
		if i < 0 {
			return nil, fmt.Errorf("unknown channel %q (email, sms, push, slack)", truncate(s, 16))
		}
		have[i] = true
	}
	out := []string{}
	for i, c := range Channels {
		if have[i] {
			out = append(out, c.ID)
		}
	}
	return out, nil
}

func optName(m map[string]any, name string) (*string, error) {
	v := m[name] // missing = null
	if v == nil {
		return nil, nil
	}
	s, ok := v.(string)
	if !ok {
		return nil, fmt.Errorf("%s must be a string or null", name)
	}
	s = strings.TrimSpace(s)
	return &s, nil
}

func checkCascade(v any) (any, error) {
	m, err := object(v, "cascade")
	if err != nil {
		return nil, err
	}
	var c Cascade
	if c.Program, err = optName(m, "program"); err != nil {
		return nil, err
	}
	if c.Project, err = optName(m, "project"); err != nil {
		return nil, err
	}
	if c.Phase, err = optName(m, "phase"); err != nil {
		return nil, err
	}
	return c, ValidateCascade(c)
}

// ValidateCascade checks the program ▸ project ▸ phase dependencies.
func ValidateCascade(c Cascade) error {
	if c.Program != nil {
		if _, ok := ProgramByName(*c.Program); !ok {
			return fmt.Errorf("no such program %q", truncate(*c.Program, 40))
		}
	}
	if c.Project != nil {
		if c.Program == nil {
			return errors.New("pick a program before a project")
		}
		if PhasesOf(*c.Program, *c.Project) == nil {
			return fmt.Errorf("%q is not a project of %s", truncate(*c.Project, 40), *c.Program)
		}
	}
	if c.Phase != nil {
		if c.Project == nil {
			return errors.New("pick a project before a phase")
		}
		if !contains(PhasesOf(*c.Program, *c.Project), *c.Phase) {
			return fmt.Errorf("%q is not a phase of %s", truncate(*c.Phase, 40), *c.Project)
		}
	}
	return nil
}

// rowID: an integer id ≥ 1, unique within its list.
func rowID(m map[string]any, seen map[int64]bool) (int64, error) {
	v, err := field(m, "id")
	if err != nil {
		return 0, err
	}
	id, err := integer(v, 1, 1<<53)
	if err != nil {
		return 0, errors.New("id must be a whole number ≥ 1")
	}
	if seen[id] {
		return 0, fmt.Errorf("duplicate id %d", id)
	}
	seen[id] = true
	return id, nil
}

// member reads a required member through a check, prefixing errors.
func member[T any](m map[string]any, name string, check func(any) (T, error)) (T, error) {
	var zero T
	v, err := field(m, name)
	if err != nil {
		return zero, err
	}
	out, err := check(v)
	if err != nil {
		return zero, fmt.Errorf("%s: %w", name, err)
	}
	return out, nil
}

func checkGrid(v any) (any, error) {
	l, err := list(v, 50, "row")
	if err != nil {
		return nil, err
	}
	seen := map[int64]bool{}
	out := make([]GridRow, 0, len(l))
	for i, x := range l {
		r, err := gridRow(x, seen)
		if err != nil {
			return nil, fmt.Errorf("row %d: %w", i+1, err)
		}
		out = append(out, r)
	}
	return out, nil
}

func gridRow(x any, seen map[int64]bool) (r GridRow, err error) {
	m, err := object(x, "row")
	if err != nil {
		return r, err
	}
	if r.ID, err = rowID(m, seen); err != nil {
		return r, err
	}
	if r.Item, err = member(m, "item", func(v any) (string, error) { return str(v, 1, 40, true) }); err != nil {
		return r, err
	}
	if r.Qty, err = member(m, "qty", func(v any) (int64, error) { return integer(v, 0, 9999) }); err != nil {
		return r, err
	}
	if r.Price, err = member(m, "price", func(v any) (float64, error) { return num2dp(v, 0, 1_000_000) }); err != nil {
		return r, err
	}
	if r.Status, err = member(m, "status", func(v any) (string, error) { return oneOf(v, Statuses, "status") }); err != nil {
		return r, err
	}
	if r.Due, err = dateOrNull(m["due"]); err != nil { // missing = null
		return r, fmt.Errorf("due: %w", err)
	}
	return r, nil
}

func checkFiles(v any) (any, error) {
	l, err := list(v, 30, "file")
	if err != nil {
		return nil, err
	}
	seen := map[string]bool{}
	out := make([]File, 0, len(l))
	for i, x := range l {
		m, err := object(x, "file")
		if err != nil {
			return nil, fmt.Errorf("file %d: %w", i+1, err)
		}
		var f File
		if f.Name, err = member(m, "name", func(v any) (string, error) { return str(v, 1, 40, true) }); err != nil {
			return nil, fmt.Errorf("file %d: %w", i+1, err)
		}
		st, err := member(m, "starred", boolean)
		if err != nil {
			return nil, fmt.Errorf("file %d: %w", i+1, err)
		}
		f.Starred = st.(bool)
		if seen[f.Name] {
			return nil, fmt.Errorf("duplicate file name %q", f.Name)
		}
		seen[f.Name] = true
		out = append(out, f)
	}
	return out, nil
}

func checkProfile(v any) (any, error) {
	m, err := object(v, "profile")
	if err != nil {
		return nil, err
	}
	var p Profile
	if p.Name, err = member(m, "name", func(v any) (string, error) { return str(v, 1, 80, true) }); err != nil {
		return nil, err
	}
	if p.Email, err = member(m, "email", func(v any) (string, error) { return email(v, true) }); err != nil {
		return nil, err
	}
	return p, nil
}

func checkSeries(v any) (any, error) {
	l, ok := v.([]any)
	if !ok || len(l) != 12 {
		return nil, errors.New("series must be a list of exactly 12 numbers")
	}
	out := make([]int64, 12)
	for i, x := range l {
		n, err := integer(x, 0, 100)
		if err != nil {
			return nil, fmt.Errorf("%s: %w", MonthNames[i], err)
		}
		out[i] = n
	}
	return out, nil
}

func checkCards(v any) (any, error) {
	l, err := list(v, 40, "card")
	if err != nil {
		return nil, err
	}
	seen := map[int64]bool{}
	out := make([]Card, 0, len(l))
	for i, x := range l {
		c, err := card(x, seen)
		if err != nil {
			return nil, fmt.Errorf("card %d: %w", i+1, err)
		}
		out = append(out, c)
	}
	return out, nil
}

func card(x any, seen map[int64]bool) (c Card, err error) {
	m, err := object(x, "card")
	if err != nil {
		return c, err
	}
	if c.ID, err = rowID(m, seen); err != nil {
		return c, err
	}
	if c.Title, err = member(m, "title", func(v any) (string, error) { return str(v, 1, 60, true) }); err != nil {
		return c, err
	}
	c.Col, err = member(m, "col", func(v any) (string, error) { return oneOf(v, KanbanColumns, "column") })
	return c, err
}

func checkBars(v any) (any, error) {
	l, err := list(v, 20, "bar")
	if err != nil {
		return nil, err
	}
	seen := map[int64]bool{}
	out := make([]Bar, 0, len(l))
	for i, x := range l {
		b, err := bar(x, seen)
		if err != nil {
			return nil, fmt.Errorf("bar %d: %w", i+1, err)
		}
		out = append(out, b)
	}
	return out, nil
}

func bar(x any, seen map[int64]bool) (b Bar, err error) {
	m, err := object(x, "bar")
	if err != nil {
		return b, err
	}
	if b.ID, err = rowID(m, seen); err != nil {
		return b, err
	}
	if b.Label, err = member(m, "label", func(v any) (string, error) { return str(v, 1, 40, true) }); err != nil {
		return b, err
	}
	if b.Start, err = member(m, "start", isoDate); err != nil {
		return b, err
	}
	b.Days, err = member(m, "days", func(v any) (int64, error) { return integer(v, 1, 365) })
	return b, err
}
