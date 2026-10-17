package core

// The filter-bar query language (protocol §9). Compile tokenises the
// input, resolves fields and values against the replica once, and records
// errors with rune spans for underlining. Failed terms are ignored.

import (
	"fmt"
	"math"
	"strconv"
	"strings"
	"unicode"
)

type Target int

const (
	TargetProjects Target = iota
	TargetTasks
	TargetActivity
)

type Cmp int

const (
	CmpColon Cmp = iota
	CmpEq
	CmpGt
	CmpGe
	CmpLt
	CmpLe
)

type QueryError struct {
	Start, End int // rune offsets
	Message    string
}

type predKind int

const (
	pText predKind = iota
	pTextField
	pEnum
	pIDs
	pNum
	pDate
	pTag
	pCf
	pIs
	pHas
)

type pred struct {
	kind   predKind
	text   string
	field  string // text/enum/ref/num/date field name, is/has value
	cmp    Cmp
	values []string
	set    map[string]bool
	ids    map[int64]bool
	none   bool
	num    *float64
	date   *Date
	cfID   int64
	cfKind CfKind
}

type term struct {
	neg  bool
	pred pred
}

type Query struct {
	terms  []term
	Errors []QueryError
	Source string
}

func FieldNames(t Target) []string {
	switch t {
	case TargetProjects:
		return []string{"code", "name", "status", "priority", "health", "owner", "program", "tag", "start", "due", "budget", "est", "actual", "pct", "notes", "is", "has"}
	case TargetTasks:
		return []string{"project", "title", "phase", "status", "priority", "assignee", "program", "tag", "start", "due", "est", "actual", "var", "notes", "is", "has"}
	}
	return []string{"user", "action", "entity"}
}

func IsValues(t Target) []string {
	if t == TargetActivity {
		return []string{"mine"}
	}
	return []string{"open", "done", "overdue", "blocked", "mine", "unassigned"}
}

func HasValues(t Target) []string {
	switch t {
	case TargetTasks:
		return []string{"notes", "deps", "due", "start", "tags"}
	case TargetProjects:
		return []string{"notes", "due", "start", "tags", "budget"}
	}
	return nil
}

type token struct {
	text       string
	start, end int
}

func tokenize(input string) []token {
	chars := []rune(input)
	var out []token
	i := 0
	for i < len(chars) {
		if unicode.IsSpace(chars[i]) {
			i++
			continue
		}
		start := i
		var b strings.Builder
		inQ := false
		for i < len(chars) && (inQ || !unicode.IsSpace(chars[i])) {
			if chars[i] == '"' {
				inQ = !inQ
			} else {
				b.WriteRune(chars[i])
			}
			i++
		}
		out = append(out, token{b.String(), start, i})
	}
	return out
}

func splitOp(tok string) (string, Cmp, string, bool) {
	pos := strings.IndexAny(tok, ":><=")
	if pos < 0 {
		return "", 0, "", false
	}
	field := tok[:pos]
	if field == "" {
		return "", 0, "", false
	}
	for _, c := range field {
		if !(unicode.IsLetter(c) || unicode.IsDigit(c) || c == '_' || c == '.') {
			return "", 0, "", false
		}
	}
	rest := tok[pos:]
	cmp, n := CmpEq, 1
	switch {
	case strings.HasPrefix(rest, ">="):
		cmp, n = CmpGe, 2
	case strings.HasPrefix(rest, "<="):
		cmp, n = CmpLe, 2
	case rest[0] == ':':
		cmp = CmpColon
	case rest[0] == '>':
		cmp = CmpGt
	case rest[0] == '<':
		cmp = CmpLt
	}
	return field, cmp, rest[n:], true
}

type nameLabel struct{ name, label string }

func enumNames(field string, t Target) []nameLabel {
	var d enumDef
	switch {
	case field == "status" && t == TargetProjects:
		d = projectStatusDef
	case field == "status":
		d = taskStatusDef
	case field == "priority":
		d = priorityDef
	default:
		d = healthDef
	}
	out := make([]nameLabel, len(d.names))
	for i := range d.names {
		out[i] = nameLabel{d.names[i], d.labels[i]}
	}
	return out
}

func (q *Query) IsEmpty() bool { return len(q.terms) == 0 }

func CompileQuery(input string, target Target, r *Replica) *Query {
	q := &Query{Source: input}
	for _, tk := range tokenize(input) {
		neg, body := false, tk.text
		if b, ok := strings.CutPrefix(tk.text, "-"); ok && b != "" {
			neg, body = true, b
		}
		p, ok, err := compileTerm(body, target, r)
		if err != nil {
			q.Errors = append(q.Errors, QueryError{tk.start, tk.end, err.Error()})
		} else if ok {
			q.terms = append(q.terms, term{neg, p})
		}
	}
	return q
}

func splitValues(v string) []string {
	var out []string
	for _, s := range strings.Split(v, ",") {
		if s != "" {
			out = append(out, strings.ToLower(s))
		}
	}
	return out
}

func parseNum(v string) (float64, bool) {
	f, err := strconv.ParseFloat(v, 64)
	if err != nil || math.IsNaN(f) {
		return 0, false
	}
	return f, true
}

func compileTerm(body string, target Target, r *Replica) (pred, bool, error) {
	if tag, ok := strings.CutPrefix(body, "#"); ok {
		if tag == "" {
			return pred{}, false, nil
		}
		var vals []string
		for _, s := range strings.Split(tag, ",") {
			vals = append(vals, strings.ToLower(s))
		}
		return pred{kind: pTag, values: vals}, true, nil
	}
	if who, ok := strings.CutPrefix(body, "@"); ok {
		if who == "" {
			return pred{}, false, nil
		}
		ids, none, err := resolveUsers(who, r)
		if err != nil {
			return pred{}, false, err
		}
		return pred{kind: pIDs, field: "person", ids: ids, none: none}, true, nil
	}
	field, cmp, value, ok := splitOp(body)
	if !ok {
		return pred{kind: pText, text: strings.ToLower(body)}, true, nil
	}
	field = strings.ToLower(field)
	if value == "" {
		return pred{}, false, nil
	}
	values := splitValues(value)
	colonOrEq := cmp == CmpColon || cmp == CmpEq
	onlyColon := func() error { return fmt.Errorf("'%s' only supports ':'", field) }
	textf := func(f string) (pred, bool, error) {
		if !colonOrEq {
			return pred{}, false, onlyColon()
		}
		return pred{kind: pTextField, field: f, cmp: cmp, values: values}, true, nil
	}
	switch field {
	case "is":
		switch strings.ToLower(value) {
		case "open":
			return pred{kind: pIs, field: "open"}, true, nil
		case "done", "closed":
			return pred{kind: pIs, field: "done"}, true, nil
		case "overdue", "late":
			return pred{kind: pIs, field: "overdue"}, true, nil
		case "blocked":
			return pred{kind: pIs, field: "blocked"}, true, nil
		case "mine":
			return pred{kind: pIs, field: "mine"}, true, nil
		case "unassigned":
			return pred{kind: pIs, field: "unassigned"}, true, nil
		}
		return pred{}, false, fmt.Errorf("unknown is:%s", strings.ToLower(value))
	case "has":
		switch v := strings.ToLower(value); v {
		case "notes", "deps", "due", "start", "budget":
			return pred{kind: pHas, field: v}, true, nil
		case "tags", "tag":
			return pred{kind: pHas, field: "tags"}, true, nil
		}
		return pred{}, false, fmt.Errorf("unknown has:%s", strings.ToLower(value))
	}
	if target == TargetActivity {
		switch field {
		case "user", "by":
			ids, none, err := resolveUsers(value, r)
			if err != nil {
				return pred{}, false, err
			}
			return pred{kind: pIDs, field: "person", ids: ids, none: none}, true, nil
		case "action":
			return textf("action")
		case "entity":
			return textf("entity")
		}
		return pred{}, false, fmt.Errorf("unknown field '%s'", field)
	}
	switch {
	case target == TargetProjects && field == "code":
		return textf("code")
	case target == TargetProjects && field == "name":
		return textf("name")
	case target == TargetTasks && (field == "title" || field == "name"):
		return textf("title")
	case target == TargetTasks && field == "phase":
		return textf("phase")
	case field == "notes":
		return textf("notes")
	case field == "status" || field == "priority" || field == "health":
		if field == "health" && target == TargetTasks {
			return pred{}, false, fmt.Errorf("tasks have no health; try project health in the Projects view")
		}
		if !colonOrEq {
			return pred{}, false, onlyColon()
		}
		names := enumNames(field, target)
		set := map[string]bool{}
		for _, v := range values {
			norm := strings.ReplaceAll(v, " ", "_")
			var hits []string
			for _, nl := range names {
				if nl.name == norm || strings.ToLower(nl.label) == v {
					hits = append(hits, nl.name)
				}
			}
			if len(hits) == 0 {
				for _, nl := range names {
					if strings.HasPrefix(nl.name, norm) || strings.HasPrefix(strings.ToLower(nl.label), v) {
						hits = append(hits, nl.name)
					}
				}
			}
			if len(hits) == 0 {
				return pred{}, false, fmt.Errorf("unknown %s '%s'", field, v)
			}
			for _, h := range hits {
				set[h] = true
			}
		}
		return pred{kind: pEnum, field: field, set: set}, true, nil
	case field == "owner" || field == "assignee" || field == "who":
		if !colonOrEq {
			return pred{}, false, onlyColon()
		}
		ids, none, err := resolveUsers(value, r)
		if err != nil {
			return pred{}, false, err
		}
		return pred{kind: pIDs, field: "person", ids: ids, none: none}, true, nil
	case field == "program":
		if !colonOrEq {
			return pred{}, false, fmt.Errorf("'program' only supports ':'")
		}
		ids := map[int64]bool{}
		none := false
		for _, v := range values {
			if v == "none" {
				none = true
				continue
			}
			n := 0
			for _, p := range r.Programs {
				if strings.ToLower(p.Prefix) == v || strings.Contains(strings.ToLower(p.Name), v) || strings.Contains(strings.ToLower(p.Client), v) {
					ids[p.ID] = true
					n++
				}
			}
			if n == 0 {
				return pred{}, false, fmt.Errorf("no program matches '%s'", v)
			}
		}
		return pred{kind: pIDs, field: "program", ids: ids, none: none}, true, nil
	case target == TargetTasks && field == "project":
		if !colonOrEq {
			return pred{}, false, fmt.Errorf("'project' only supports ':'")
		}
		ids := map[int64]bool{}
		for _, v := range values {
			var exact []int64
			for _, p := range r.Projects {
				if strings.ToLower(p.Code) == v {
					exact = append(exact, p.ID)
				}
			}
			hit := exact
			if len(exact) == 0 {
				for _, p := range r.Projects {
					if strings.HasPrefix(strings.ToLower(p.Code), v) || strings.Contains(strings.ToLower(p.Name), v) {
						hit = append(hit, p.ID)
					}
				}
			}
			if len(hit) == 0 {
				return pred{}, false, fmt.Errorf("no project matches '%s'", v)
			}
			for _, id := range hit {
				ids[id] = true
			}
		}
		return pred{kind: pIDs, field: "project", ids: ids}, true, nil
	case field == "tag" || field == "tags":
		if !colonOrEq {
			return pred{}, false, onlyColon()
		}
		return pred{kind: pTag, values: values}, true, nil
	case field == "start" || field == "due":
		v := strings.ToLower(value)
		p := pred{kind: pDate, field: field, cmp: cmp}
		if v == "none" {
			if !colonOrEq {
				return pred{}, false, fmt.Errorf("'none' only works with ':'")
			}
		} else {
			d, err := ParseNatural(v, r.Today)
			if err != nil {
				return pred{}, false, err
			}
			if d == nil {
				return pred{}, false, fmt.Errorf("missing date")
			}
			p.date = d
		}
		return p, true, nil
	case field == "budget" || field == "est" || field == "actual" || field == "var" || field == "pct":
		valid := field == "est" || field == "actual" ||
			(field == "budget" && target == TargetProjects) ||
			(field == "var" && target == TargetTasks) ||
			(field == "pct" && target == TargetProjects)
		if !valid {
			return pred{}, false, fmt.Errorf("unknown field '%s'", field)
		}
		p := pred{kind: pNum, field: field, cmp: cmp}
		if !strings.EqualFold(value, "none") {
			n, ok := parseNum(strings.TrimRight(value, "h%"))
			if !ok {
				return pred{}, false, fmt.Errorf("'%s' is not a number", value)
			}
			p.num = &n
		}
		return p, true, nil
	}
	ent := CfTask
	if target == TargetProjects {
		ent = CfProject
	}
	var cf *CustomField
	for _, c := range r.CustomFieldsFor(ent) {
		if c.QueryName() == field {
			cf = c
			break
		}
	}
	if cf == nil {
		return pred{}, false, fmt.Errorf("unknown field '%s'", field)
	}
	p := pred{kind: pCf, cfID: cf.ID, cfKind: cf.Kind, cmp: cmp, values: values}
	switch cf.Kind {
	case CfNumber:
		n, ok := parseNum(value)
		if !ok {
			return pred{}, false, fmt.Errorf("'%s' is not a number", value)
		}
		p.num = &n
	case CfDate:
		d, err := ParseNatural(value, r.Today)
		if err != nil {
			return pred{}, false, err
		}
		if d == nil {
			return pred{}, false, fmt.Errorf("missing date")
		}
		p.date = d
	default:
		if !colonOrEq {
			return pred{}, false, fmt.Errorf("'%s' only supports ':'", field)
		}
	}
	return p, true, nil
}

func resolveUsers(value string, r *Replica) (map[int64]bool, bool, error) {
	ids := map[int64]bool{}
	none := false
	for _, v := range strings.Split(value, ",") {
		if v == "" {
			continue
		}
		v = strings.ToLower(v)
		switch v {
		case "me":
			ids[r.MeID()] = true
		case "none", "nobody":
			none = true
		default:
			n := 0
			for _, u := range r.Users {
				if u.Username == v {
					ids[u.ID] = true
					n++
				}
			}
			if n == 0 {
				for _, u := range r.Users {
					match := strings.HasPrefix(u.Username, v)
					for _, w := range strings.Fields(strings.ToLower(u.DisplayName)) {
						if strings.HasPrefix(w, v) {
							match = true
						}
					}
					if match {
						ids[u.ID] = true
						n++
					}
				}
			}
			if n == 0 {
				return nil, false, fmt.Errorf("no user matches '%s'", v)
			}
		}
	}
	return ids, none, nil
}

func cmpNum(a float64, c Cmp, b float64) bool {
	switch c {
	case CmpGt:
		return a > b
	case CmpGe:
		return a >= b
	case CmpLt:
		return a < b
	case CmpLe:
		return a <= b
	}
	return math.Abs(a-b) < 1e-9
}

func cmpDate(a *Date, c Cmp, b *Date) bool {
	switch {
	case a == nil && b == nil:
		return c == CmpColon || c == CmpEq
	case a != nil && b != nil:
		switch c {
		case CmpGt:
			return *a > *b
		case CmpGe:
			return *a >= *b
		case CmpLt:
			return *a < *b
		case CmpLe:
			return *a <= *b
		}
		return *a == *b
	}
	return false
}

func textMatch(hay string, c Cmp, values []string) bool {
	h := strings.ToLower(hay)
	for _, v := range values {
		if c == CmpEq && h == v || c != CmpEq && strings.Contains(h, v) {
			return true
		}
	}
	return false
}

func datePtr(s *string) *Date {
	if d, ok := parseDatePtr(s); ok {
		return &d
	}
	return nil
}

func cfMatch(cf CfValues, p pred) bool {
	v, has := cf[strconv.FormatInt(p.cfID, 10)]
	switch p.cfKind {
	case CfNumber:
		a, ok := AsFloat(v)
		return has && ok && p.num != nil && cmpNum(a, p.cmp, *p.num)
	case CfDate:
		var d *Date
		if s, ok := v.(string); ok {
			if dd, ok := ParseISO(s); ok {
				d = &dd
			}
		}
		return cmpDate(d, p.cmp, p.date)
	case CfEnum:
		s, ok := v.(string)
		if !ok {
			return false
		}
		for _, x := range p.values {
			if strings.ToLower(s) == x {
				return true
			}
		}
		return false
	}
	s, ok := v.(string)
	return ok && textMatch(s, p.cmp, p.values)
}

func tagsHave(tags []string, want []string) bool {
	for _, w := range want {
		if containsStr(tags, w) {
			return true
		}
	}
	return false
}

func idsMatch(v *int64, p pred) bool {
	if v == nil {
		return p.none
	}
	return p.ids[*v]
}

func (q *Query) MatchesProject(p *Project, r *Replica) bool {
	for _, t := range q.terms {
		if evalProject(t.pred, p, r) == t.neg {
			return false
		}
	}
	return true
}

func (q *Query) MatchesTask(t *Task, r *Replica) bool {
	for _, tm := range q.terms {
		if evalTask(tm.pred, t, r) == tm.neg {
			return false
		}
	}
	return true
}

func (q *Query) MatchesActivity(a *Activity, r *Replica) bool {
	for _, t := range q.terms {
		if evalActivity(t.pred, a, r) == t.neg {
			return false
		}
	}
	return true
}

func numMatch(a *float64, p pred) bool {
	switch {
	case a != nil && p.num != nil:
		return cmpNum(*a, p.cmp, *p.num)
	case a == nil && p.num == nil:
		return true
	}
	return false
}

func evalProject(p pred, pr *Project, r *Replica) bool {
	switch p.kind {
	case pText:
		t := p.text
		if strings.Contains(strings.ToLower(pr.Code), t) || strings.Contains(strings.ToLower(pr.Name), t) ||
			strings.Contains(strings.ToLower(pr.Notes), t) || strings.Contains(strings.ToLower(r.ProgramName(pr.ProgramID)), t) {
			return true
		}
		for _, g := range pr.Tags {
			if strings.Contains(g, t) {
				return true
			}
		}
		return false
	case pTextField:
		switch p.field {
		case "code":
			return textMatch(pr.Code, p.cmp, p.values)
		case "name", "title":
			return textMatch(pr.Name, p.cmp, p.values)
		case "notes":
			return textMatch(pr.Notes, p.cmp, p.values)
		}
		return false
	case pEnum:
		switch p.field {
		case "status":
			return p.set[string(pr.Status)]
		case "priority":
			return p.set[string(pr.Priority)]
		}
		return p.set[string(pr.Health)]
	case pIDs:
		switch p.field {
		case "person":
			return idsMatch(pr.OwnerID, p)
		case "program":
			return idsMatch(pr.ProgramID, p)
		}
		return idsMatch(&pr.ID, p)
	case pNum:
		m := r.Metrics(pr.ID)
		var a *float64
		switch p.field {
		case "budget":
			a = pr.BudgetHours
		case "est":
			a = Ptr(m.EstTotal)
		case "actual":
			a = Ptr(m.ActualTotal)
		case "pct":
			a = Ptr(m.PctDone())
		case "var":
			a = Ptr(m.ActualTotal - m.EstTotal)
		}
		return numMatch(a, p)
	case pDate:
		if p.field == "start" {
			return cmpDate(datePtr(pr.StartDate), p.cmp, p.date)
		}
		return cmpDate(datePtr(pr.DueDate), p.cmp, p.date)
	case pTag:
		return tagsHave(pr.Tags, p.values)
	case pCf:
		return cfMatch(pr.CF, p)
	case pIs:
		switch p.field {
		case "open":
			return r.ProjectOpen(pr)
		case "done":
			return pr.Status == ProjectDone
		case "overdue":
			return r.ProjectOverdue(pr)
		case "blocked":
			return r.Metrics(pr.ID).TasksBlocked > 0
		case "mine":
			return pr.OwnerID != nil && *pr.OwnerID == r.MeID()
		}
		return pr.OwnerID == nil
	case pHas:
		switch p.field {
		case "notes":
			return strings.TrimSpace(pr.Notes) != ""
		case "due":
			return pr.DueDate != nil
		case "start":
			return pr.StartDate != nil
		case "tags":
			return len(pr.Tags) > 0
		case "budget":
			return pr.BudgetHours != nil
		}
		return false
	}
	return false
}

func evalTask(p pred, t *Task, r *Replica) bool {
	project := r.Projects[t.ProjectID]
	switch p.kind {
	case pText:
		s := p.text
		if strings.Contains(strings.ToLower(t.Title), s) || strings.Contains(strings.ToLower(t.Phase), s) ||
			strings.Contains(strings.ToLower(t.Notes), s) || (project != nil && strings.Contains(strings.ToLower(project.Code), s)) {
			return true
		}
		for _, g := range t.Tags {
			if strings.Contains(g, s) {
				return true
			}
		}
		return false
	case pTextField:
		switch p.field {
		case "title", "name":
			return textMatch(t.Title, p.cmp, p.values)
		case "phase":
			return textMatch(t.Phase, p.cmp, p.values)
		case "notes":
			return textMatch(t.Notes, p.cmp, p.values)
		case "code":
			return project != nil && textMatch(project.Code, p.cmp, p.values)
		}
		return false
	case pEnum:
		switch p.field {
		case "status":
			return p.set[string(t.Status)]
		case "priority":
			return p.set[string(t.Priority)]
		}
		return project != nil && p.set[string(project.Health)]
	case pIDs:
		switch p.field {
		case "person":
			return idsMatch(t.AssigneeID, p)
		case "program":
			if project == nil {
				return idsMatch(nil, p)
			}
			return idsMatch(project.ProgramID, p)
		}
		return idsMatch(&t.ProjectID, p)
	case pNum:
		var a *float64
		switch p.field {
		case "est":
			a = t.EstHours
		case "actual":
			a = Ptr(t.ActualHours)
		case "var":
			if t.EstHours != nil {
				a = Ptr(t.ActualHours - *t.EstHours)
			}
		}
		return numMatch(a, p)
	case pDate:
		if p.field == "start" {
			return cmpDate(datePtr(t.StartDate), p.cmp, p.date)
		}
		return cmpDate(datePtr(t.DueDate), p.cmp, p.date)
	case pTag:
		return tagsHave(t.Tags, p.values)
	case pCf:
		return cfMatch(t.CF, p)
	case pIs:
		switch p.field {
		case "open":
			return t.Status != TaskDone
		case "done":
			return t.Status == TaskDone
		case "overdue":
			return r.TaskOverdue(t)
		case "blocked":
			return r.TaskBlocked(t)
		case "mine":
			return t.AssigneeID != nil && *t.AssigneeID == r.MeID()
		}
		return t.AssigneeID == nil
	case pHas:
		switch p.field {
		case "notes":
			return strings.TrimSpace(t.Notes) != ""
		case "deps":
			return len(t.Deps) > 0
		case "due":
			return t.DueDate != nil
		case "start":
			return t.StartDate != nil
		case "tags":
			return len(t.Tags) > 0
		}
		return false
	}
	return false
}

func evalActivity(p pred, a *Activity, r *Replica) bool {
	switch p.kind {
	case pText:
		return strings.Contains(strings.ToLower(a.Label), p.text) || strings.Contains(r.Username(a.UserID), p.text)
	case pTextField:
		switch p.field {
		case "action":
			return textMatch(string(a.Action), p.cmp, p.values)
		case "entity":
			return textMatch(a.Entity, p.cmp, p.values)
		}
		return true
	case pIDs:
		return idsMatch(a.UserID, p)
	case pIs:
		if p.field == "mine" {
			return a.UserID != nil && *a.UserID == r.MeID()
		}
	}
	return true
}
