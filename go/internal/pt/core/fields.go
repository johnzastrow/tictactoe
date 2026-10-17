package core

// Field metadata and value validation (protocol §4). The server validates
// every write with this; clients use it to choose editors and parse input.
// Field and table names here are the only identifiers the server ever puts
// into SQL text.

import (
	"encoding/json"
	"fmt"
	"math"
	"sort"
	"strconv"
	"strings"
	"unicode/utf8"
)

type KindTag int

const (
	KText KindTag = iota
	KCode
	KUsername
	KPrefix
	KColor
	KEnum
	KRef
	KDate
	KHours
	KInt
	KTags
	KDeps
	KOptions
	KBool
	KPassword
)

type Kind struct {
	Tag      KindTag
	Min, Max int64    // Text lengths, Int range
	Names    []string // Enum
	Target   Entity   // Ref
}

type Perm int

const (
	PermMember Perm = iota
	PermAdmin
)

type FieldMeta struct {
	Name       string
	Label      string
	Kind       Kind
	Nullable   bool
	Perm       Perm
	CreateOnly bool
	Required   bool
}

// IsSideTable: stored outside the entity's own table.
func (m *FieldMeta) IsSideTable() bool {
	return m.Kind.Tag == KTags || m.Kind.Tag == KDeps || m.Kind.Tag == KPassword
}

var (
	ProjectStatusNames = projectStatusDef.names
	TaskStatusNames    = taskStatusDef.names
	PriorityNames      = priorityDef.names
	HealthNames        = healthDef.names
	RoleNames          = roleDef.names
	CfEntityNames      = cfEntityDef.names
	CfKindNames        = cfKindDef.names
)

func text(min, max int64) Kind { return Kind{Tag: KText, Min: min, Max: max} }
func enum(n []string) Kind     { return Kind{Tag: KEnum, Names: n} }
func ref(e Entity) Kind        { return Kind{Tag: KRef, Target: e} }
func intk(min, max int64) Kind { return Kind{Tag: KInt, Min: min, Max: max} }

type fopt int

const (
	oNull fopt = iota
	oAdmin
	oCreate
	oReq
)

func fm(name, label string, k Kind, opts ...fopt) FieldMeta {
	m := FieldMeta{Name: name, Label: label, Kind: k}
	for _, o := range opts {
		switch o {
		case oNull:
			m.Nullable = true
		case oAdmin:
			m.Perm = PermAdmin
		case oCreate:
			m.CreateOnly = true
		case oReq:
			m.Required = true
		}
	}
	return m
}

var notes = text(0, 20_000)

var fieldTable = map[Entity][]FieldMeta{
	EntProject: {
		fm("code", "Code", Kind{Tag: KCode}, oReq),
		fm("name", "Name", text(1, 200), oReq),
		fm("program_id", "Program", ref(EntProgram), oNull),
		fm("template_id", "Template", ref(EntTemplate), oNull, oCreate),
		fm("owner_id", "Owner", ref(EntUser), oNull),
		fm("status", "Status", enum(ProjectStatusNames)),
		fm("priority", "Priority", enum(PriorityNames)),
		fm("health", "Health", enum(HealthNames)),
		fm("start_date", "Start", Kind{Tag: KDate}, oNull),
		fm("due_date", "Due", Kind{Tag: KDate}, oNull),
		fm("budget_hours", "Budget h", Kind{Tag: KHours}, oNull),
		fm("notes", "Notes", notes),
		fm("tags", "Tags", Kind{Tag: KTags}),
	},
	EntTask: {
		fm("project_id", "Project", ref(EntProject), oCreate, oReq),
		fm("title", "Title", text(1, 200), oReq),
		fm("phase", "Phase", text(0, 60)),
		fm("status", "Status", enum(TaskStatusNames)),
		fm("priority", "Priority", enum(PriorityNames)),
		fm("assignee_id", "Assignee", ref(EntUser), oNull),
		fm("start_date", "Start", Kind{Tag: KDate}, oNull),
		fm("due_date", "Due", Kind{Tag: KDate}, oNull),
		fm("est_hours", "Est h", Kind{Tag: KHours}, oNull),
		fm("actual_hours", "Actual h", Kind{Tag: KHours}),
		fm("notes", "Notes", notes),
		fm("tags", "Tags", Kind{Tag: KTags}),
		fm("deps", "Deps", Kind{Tag: KDeps}),
	},
	EntProgram: {
		fm("name", "Name", text(1, 100), oReq, oAdmin),
		fm("client", "Client", text(0, 100), oAdmin),
		fm("prefix", "Prefix", Kind{Tag: KPrefix}, oAdmin),
		fm("color", "Colour", Kind{Tag: KColor}, oAdmin),
	},
	EntTemplate: {
		fm("name", "Name", text(1, 100), oReq, oAdmin),
		fm("description", "Description", text(0, 2000), oAdmin),
	},
	EntTemplateTask: {
		fm("template_id", "Template", ref(EntTemplate), oCreate, oReq, oAdmin),
		fm("seq", "Seq", intk(0, 100_000), oAdmin),
		fm("title", "Title", text(1, 200), oReq, oAdmin),
		fm("phase", "Phase", text(0, 60), oAdmin),
		fm("est_hours", "Est h", Kind{Tag: KHours}, oNull, oAdmin),
		fm("offset_days", "Offset d", intk(0, 3650), oAdmin),
		fm("duration_days", "Duration d", intk(1, 3650), oAdmin),
	},
	EntCustomField: {
		fm("entity", "Entity", enum(CfEntityNames), oCreate, oReq, oAdmin),
		fm("name", "Name", text(1, 40), oReq, oAdmin),
		fm("kind", "Kind", enum(CfKindNames), oCreate, oReq, oAdmin),
		fm("options", "Options", Kind{Tag: KOptions}, oAdmin),
		fm("seq", "Seq", intk(0, 100_000), oAdmin),
	},
	EntUser: {
		fm("username", "Username", Kind{Tag: KUsername}, oCreate, oReq, oAdmin),
		fm("display_name", "Name", text(1, 80), oReq, oAdmin),
		fm("role", "Role", enum(RoleNames), oAdmin),
		fm("active", "Active", Kind{Tag: KBool}, oAdmin),
		fm("password", "Password", Kind{Tag: KPassword}, oCreate, oReq, oAdmin),
	},
}

func Fields(e Entity) []FieldMeta { return fieldTable[e] }

func Field(e Entity, name string) *FieldMeta {
	fs := fieldTable[e]
	for i := range fs {
		if fs[i].Name == name {
			return &fs[i]
		}
	}
	return nil
}

// CfID: "cf.12" → 12.
func CfID(field string) (int64, bool) {
	s, ok := strings.CutPrefix(field, "cf.")
	if !ok {
		return 0, false
	}
	n, err := strconv.ParseInt(s, 10, 64)
	if err != nil || n <= 0 || strings.HasPrefix(s, "+") {
		return 0, false
	}
	return n, true
}

// Round2 rounds half away from zero like Rust's f64::round.
func Round2(x float64) float64 { return math.Round(x*100) / 100 }

// AsFloat converts any JSON-ish number.
func AsFloat(v any) (float64, bool) {
	switch n := v.(type) {
	case float64:
		return n, true
	case float32:
		return float64(n), true
	case int:
		return float64(n), true
	case int64:
		return float64(n), true
	case int32:
		return float64(n), true
	case uint64:
		return float64(n), true
	case json.Number:
		f, err := n.Float64()
		return f, err == nil
	}
	return 0, false
}

// AsInt converts an integral JSON number.
func AsInt(v any) (int64, bool) {
	switch n := v.(type) {
	case int64:
		return n, true
	case int:
		return int64(n), true
	case int32:
		return int64(n), true
	case uint64:
		return int64(n), true
	case json.Number:
		if i, err := n.Int64(); err == nil {
			return i, true
		}
	}
	if f, ok := AsFloat(v); ok && f == math.Trunc(f) && math.Abs(f) < 9e15 {
		return int64(f), true
	}
	return 0, false
}

// AsList returns the items of a JSON array in any of its Go forms.
func AsList(v any) ([]any, bool) {
	switch a := v.(type) {
	case []any:
		return a, true
	case []string:
		out := make([]any, len(a))
		for i, s := range a {
			out[i] = s
		}
		return out, true
	case []int64:
		out := make([]any, len(a))
		for i, s := range a {
			out[i] = s
		}
		return out, true
	}
	return nil, false
}

func runeLen(s string) int { return utf8.RuneCountInString(s) }

func ValidCode(s string) bool {
	if len(s) < 2 || len(s) > 16 {
		return false
	}
	c0 := s[0]
	if !(c0 >= 'A' && c0 <= 'Z' || c0 >= '0' && c0 <= '9') {
		return false
	}
	for i := 0; i < len(s); i++ {
		c := s[i]
		if !(c >= 'A' && c <= 'Z' || c >= '0' && c <= '9' || c == '-') {
			return false
		}
	}
	return true
}

func ValidUsername(s string) bool {
	if len(s) < 2 || len(s) > 32 || !(s[0] >= 'a' && s[0] <= 'z') {
		return false
	}
	for i := 0; i < len(s); i++ {
		c := s[i]
		if !(c >= 'a' && c <= 'z' || c >= '0' && c <= '9' || c == '_' || c == '.' || c == '-') {
			return false
		}
	}
	return true
}

func ValidTag(s string) bool {
	if len(s) < 1 || len(s) > 32 || !(s[0] >= 'a' && s[0] <= 'z' || s[0] >= '0' && s[0] <= '9') {
		return false
	}
	for i := 0; i < len(s); i++ {
		c := s[i]
		if !(c >= 'a' && c <= 'z' || c >= '0' && c <= '9' || c == '_' || c == '-') {
			return false
		}
	}
	return true
}

func isEmptyStr(v any) bool { s, ok := v.(string); return ok && s == "" }

// Normalize validates and normalises a built-in field's value. Reference
// existence and cross-field rules are the server's job.
func Normalize(m *FieldMeta, v any) (any, error) {
	label := m.Label
	if v == nil || (isEmptyStr(v) && m.Nullable) {
		if m.Nullable {
			return nil, nil
		}
		if m.Kind.Tag == KText && m.Kind.Min == 0 {
			return "", nil
		}
		switch m.Kind.Tag {
		case KTags, KOptions:
			return []string{}, nil
		case KDeps:
			return []int64{}, nil
		}
		return nil, fmt.Errorf("%s is required", label)
	}
	wantStr := func() (string, error) {
		s, ok := v.(string)
		if !ok {
			return "", fmt.Errorf("%s must be text", label)
		}
		return s, nil
	}
	switch m.Kind.Tag {
	case KText:
		s, err := wantStr()
		if err != nil {
			return nil, err
		}
		if m.Kind.Max <= 2000 {
			s = strings.TrimSpace(s)
		}
		n := int64(runeLen(s))
		if n < m.Kind.Min {
			return nil, fmt.Errorf("%s is required", label)
		}
		if n > m.Kind.Max {
			return nil, fmt.Errorf("%s is longer than %d characters", label, m.Kind.Max)
		}
		return s, nil
	case KCode:
		s, err := wantStr()
		if err != nil {
			return nil, err
		}
		s = strings.ToUpper(strings.TrimSpace(s))
		if !ValidCode(s) {
			return nil, fmt.Errorf("%s must be 2–16 of A–Z, 0–9 and '-'", label)
		}
		return s, nil
	case KUsername:
		s, err := wantStr()
		if err != nil {
			return nil, err
		}
		s = strings.ToLower(strings.TrimSpace(s))
		if !ValidUsername(s) {
			return nil, fmt.Errorf("username must be 2–32 of a–z, 0–9, '_', '.', '-' starting with a letter")
		}
		return s, nil
	case KPrefix:
		s, err := wantStr()
		if err != nil {
			return nil, err
		}
		s = strings.ToUpper(strings.TrimSpace(s))
		if len(s) > 5 {
			return nil, fmt.Errorf("prefix must be up to 5 letters")
		}
		for i := 0; i < len(s); i++ {
			if s[i] < 'A' || s[i] > 'Z' {
				return nil, fmt.Errorf("prefix must be up to 5 letters")
			}
		}
		return s, nil
	case KColor:
		s, err := wantStr()
		if err != nil {
			return nil, err
		}
		s = strings.ToLower(strings.TrimSpace(s))
		ok := len(s) == 7 && s[0] == '#'
		for i := 1; ok && i < 7; i++ {
			c := s[i]
			ok = c >= '0' && c <= '9' || c >= 'a' && c <= 'f'
		}
		if !ok {
			return nil, fmt.Errorf("colour must look like #3a7bd5")
		}
		return s, nil
	case KEnum:
		s, err := wantStr()
		if err != nil {
			return nil, err
		}
		for _, n := range m.Kind.Names {
			if n == s {
				return s, nil
			}
		}
		return nil, fmt.Errorf("%s must be one of %s", label, strings.Join(m.Kind.Names, ", "))
	case KRef:
		if n, ok := AsInt(v); ok && n > 0 {
			return n, nil
		}
		return nil, fmt.Errorf("%s must be an id", label)
	case KDate:
		s, err := wantStr()
		if err != nil {
			return nil, err
		}
		if d, ok := ParseISO(s); ok {
			return d.ISO(), nil
		}
		return nil, fmt.Errorf("%s must be a date (YYYY-MM-DD)", label)
	case KHours:
		if x, ok := AsFloat(v); ok && !math.IsInf(x, 0) && !math.IsNaN(x) && x >= 0 && x <= 1_000_000 {
			return Round2(x), nil
		}
		return nil, fmt.Errorf("%s must be a number of hours between 0 and 1,000,000", label)
	case KInt:
		if n, ok := AsInt(v); ok && n >= m.Kind.Min && n <= m.Kind.Max {
			return n, nil
		}
		return nil, fmt.Errorf("%s must be a whole number from %d to %d", label, m.Kind.Min, m.Kind.Max)
	case KTags:
		arr, ok := AsList(v)
		if !ok {
			return nil, fmt.Errorf("%s must be a list", label)
		}
		out := make([]string, 0, len(arr))
		for _, t := range arr {
			s, ok := t.(string)
			if !ok {
				return nil, fmt.Errorf("%s must be text", label)
			}
			s = strings.ToLower(strings.TrimSpace(s))
			if !ValidTag(s) {
				return nil, fmt.Errorf("tag '%s' must be 1–32 of a–z, 0–9, '_', '-'", s)
			}
			out = append(out, s)
		}
		sort.Strings(out)
		out = dedupStrings(out)
		if len(out) > 20 {
			return nil, fmt.Errorf("at most 20 tags")
		}
		return out, nil
	case KDeps:
		arr, ok := AsList(v)
		if !ok {
			return nil, fmt.Errorf("%s must be a list", label)
		}
		out := make([]int64, 0, len(arr))
		for _, t := range arr {
			n, ok := AsInt(t)
			if !ok || n <= 0 {
				return nil, fmt.Errorf("%s must be task ids", label)
			}
			out = append(out, n)
		}
		sort.Slice(out, func(i, j int) bool { return out[i] < out[j] })
		out = dedupInts(out)
		if len(out) > 50 {
			return nil, fmt.Errorf("at most 50 dependencies")
		}
		return out, nil
	case KOptions:
		arr, ok := AsList(v)
		if !ok {
			return nil, fmt.Errorf("%s must be a list", label)
		}
		out := []string{}
		for _, t := range arr {
			s, ok := t.(string)
			if !ok {
				return nil, fmt.Errorf("%s must be text", label)
			}
			s = strings.TrimSpace(s)
			if s == "" || runeLen(s) > 40 {
				return nil, fmt.Errorf("each option must be 1–40 characters")
			}
			if !containsStr(out, s) {
				out = append(out, s)
			}
		}
		if len(out) > 50 {
			return nil, fmt.Errorf("at most 50 options")
		}
		return out, nil
	case KBool:
		switch b := v.(type) {
		case bool:
			return b, nil
		}
		if n, ok := AsInt(v); ok && (n == 0 || n == 1) {
			return n == 1, nil
		}
		return nil, fmt.Errorf("%s must be true or false", label)
	case KPassword:
		s, err := wantStr()
		if err != nil {
			return nil, err
		}
		n := runeLen(s)
		if n < 8 || n > 256 {
			return nil, fmt.Errorf("password must be 8–256 characters")
		}
		return s, nil
	}
	return nil, fmt.Errorf("unsupported field")
}

// NormalizeCF validates a custom field value; nil means "remove".
func NormalizeCF(kind CfKind, options []string, name string, v any) (any, error) {
	if v == nil || isEmptyStr(v) {
		return nil, nil
	}
	switch kind {
	case CfText:
		s, ok := v.(string)
		if !ok {
			return nil, fmt.Errorf("%s must be text", name)
		}
		s = strings.TrimSpace(s)
		if runeLen(s) > 500 {
			return nil, fmt.Errorf("%s is longer than 500 characters", name)
		}
		if s == "" {
			return nil, nil
		}
		return s, nil
	case CfNumber:
		if x, ok := AsFloat(v); ok && !math.IsInf(x, 0) && !math.IsNaN(x) {
			return x, nil
		}
		return nil, fmt.Errorf("%s must be a number", name)
	case CfDate:
		if s, ok := v.(string); ok {
			if d, ok := ParseISO(s); ok {
				return d.ISO(), nil
			}
		}
		return nil, fmt.Errorf("%s must be a date (YYYY-MM-DD)", name)
	case CfEnum:
		s, ok := v.(string)
		if !ok {
			return nil, fmt.Errorf("%s must be text", name)
		}
		if containsStr(options, s) {
			return s, nil
		}
		return nil, fmt.Errorf("%s must be one of %s", name, strings.Join(options, ", "))
	}
	return nil, fmt.Errorf("unsupported kind")
}

func listKey(v any) string {
	if s, ok := v.(string); ok {
		return s
	}
	if f, ok := AsFloat(v); ok {
		return strconv.FormatFloat(f, 'g', -1, 64)
	}
	b, _ := json.Marshal(v)
	return string(b)
}

// ValuesEqual is the equality used for optimistic checks (protocol §5).
func ValuesEqual(a, b any) bool {
	if a == nil && b == nil {
		return true
	}
	if a == nil {
		s, ok := b.(string)
		return ok && s == ""
	}
	if b == nil {
		s, ok := a.(string)
		return ok && s == ""
	}
	if x, ok := AsFloat(a); ok {
		y, ok := AsFloat(b)
		return ok && math.Abs(x-y) < 1e-9
	}
	if xs, ok := AsList(a); ok {
		ys, ok := AsList(b)
		if !ok || len(xs) != len(ys) {
			return false
		}
		ka := make([]string, len(xs))
		kb := make([]string, len(ys))
		for i := range xs {
			ka[i] = listKey(xs[i])
			kb[i] = listKey(ys[i])
		}
		sort.Strings(ka)
		sort.Strings(kb)
		for i := range ka {
			if ka[i] != kb[i] {
				return false
			}
		}
		return true
	}
	switch x := a.(type) {
	case string:
		y, ok := b.(string)
		return ok && x == y
	case bool:
		y, ok := b.(bool)
		return ok && x == y
	}
	return false
}

func dedupStrings(s []string) []string {
	out := s[:0]
	for i, v := range s {
		if i == 0 || v != s[i-1] {
			out = append(out, v)
		}
	}
	return out
}

func dedupInts(s []int64) []int64 {
	out := s[:0]
	for i, v := range s {
		if i == 0 || v != s[i-1] {
			out = append(out, v)
		}
	}
	return out
}

func containsStr(xs []string, s string) bool {
	for _, x := range xs {
		if x == s {
			return true
		}
	}
	return false
}
