package core

// Front-end-neutral grid model (port of the Rust grid.rs): columns per
// view, cell text + style hints, sorting, grouping with subtotals, edit
// choices and text parsing for typed/pasted values.

import (
	"fmt"
	"math"
	"sort"
	"strconv"
	"strings"
)

type Align int

const (
	AlignLeft Align = iota
	AlignRight
	AlignCenter
)

type Editor int

const (
	EdNone Editor = iota
	EdText
	EdMultiline
	EdNumber
	EdInt
	EdDate
	EdSelect  // fixed list (enums)
	EdCombo   // fuzzy list (references)
	EdSuggest // free text with suggestions (phase)
	EdChips   // multi-value (tags, deps, options)
	EdBool
)

type Column struct {
	Key    string
	Title  string
	Width  int
	Align  Align
	Entity Entity
	Field  string // "" = computed/read-only
	Editor Editor
	Sum    bool
	Hidden bool
}

func (c *Column) Editable() bool { return c.Field != "" && c.Editor != EdNone }

func editorFor(k Kind, name string) Editor {
	switch k.Tag {
	case KText:
		if k.Max > 2000 {
			return EdMultiline
		}
		if name == "phase" {
			return EdSuggest
		}
		return EdText
	case KCode, KUsername, KPrefix, KColor:
		return EdText
	case KEnum:
		return EdSelect
	case KRef:
		return EdCombo
	case KDate:
		return EdDate
	case KHours:
		return EdNumber
	case KInt:
		return EdInt
	case KTags, KDeps, KOptions:
		return EdChips
	case KBool:
		return EdBool
	}
	return EdNone
}

func newCol(e Entity, key, title string, width int) Column {
	c := Column{Key: key, Title: title, Width: width, Entity: e}
	m := Field(e, key)
	if m == nil {
		return c
	}
	if m.Kind.Tag == KHours || m.Kind.Tag == KInt {
		c.Align = AlignRight
	}
	if !m.CreateOnly {
		c.Field = m.Name
		c.Editor = editorFor(m.Kind, key)
	}
	return c
}

func computedCol(e Entity, key, title string, width int, a Align) Column {
	return Column{Key: key, Title: title, Width: width, Align: a, Entity: e}
}

func (c Column) sum() Column            { c.Sum = true; return c }
func (c Column) hide() Column           { c.Hidden = true; return c }
func (c Column) editor(e Editor) Column { c.Editor = e; return c }

func cfColumns(r *Replica, e CfEntity) []Column {
	entity := EntTask
	if e == CfProject {
		entity = EntProject
	}
	var out []Column
	for _, cf := range r.CustomFieldsFor(e) {
		w := 12
		switch cf.Kind {
		case CfNumber:
			w = 9
		case CfDate:
			w = 10
		default:
			w = min(max(12, runeLen(cf.Name)+1), 20)
		}
		c := Column{Key: cf.Key(), Title: cf.Name, Width: w, Entity: entity, Field: cf.Key()}
		switch cf.Kind {
		case CfText:
			c.Editor = EdText
		case CfNumber:
			c.Editor, c.Align = EdNumber, AlignRight
		case CfDate:
			c.Editor = EdDate
		case CfEnum:
			c.Editor = EdSelect
		}
		out = append(out, c)
	}
	return out
}

func ProjectColumns(r *Replica) []Column {
	e := EntProject
	v := []Column{
		newCol(e, "code", "Code", 9),
		newCol(e, "name", "Name", 26),
		newCol(e, "program_id", "Program", 16),
		newCol(e, "owner_id", "Owner", 14),
		newCol(e, "status", "Status", 10),
		newCol(e, "priority", "Priority", 9),
		newCol(e, "health", "Health", 7),
		newCol(e, "start_date", "Start", 10),
		newCol(e, "due_date", "Due", 10),
		newCol(e, "budget_hours", "Budget h", 9).sum(),
		computedCol(e, "est", "Est h", 8, AlignRight).sum(),
		computedCol(e, "actual", "Actual h", 9, AlignRight).sum(),
		computedCol(e, "burn", "Burn", 12, AlignLeft),
		computedCol(e, "done", "Done", 14, AlignLeft),
		computedCol(e, "overdue", "Late", 5, AlignRight).sum(),
		newCol(e, "tags", "Tags", 18),
	}
	v = append(v, cfColumns(r, CfProject)...)
	return append(v, newCol(e, "notes", "Notes", 30).hide())
}

func TaskColumns(r *Replica, withProject bool) []Column {
	e := EntTask
	var v []Column
	if withProject {
		v = append(v, computedCol(e, "project", "Project", 9, AlignLeft))
	}
	v = append(v,
		computedCol(e, "seq", "#", 3, AlignRight),
		newCol(e, "title", "Title", 28),
		newCol(e, "phase", "Phase", 10),
		newCol(e, "status", "Status", 11),
		newCol(e, "priority", "Priority", 9),
		newCol(e, "assignee_id", "Assignee", 14),
		newCol(e, "start_date", "Start", 10),
		newCol(e, "due_date", "Due", 10),
		newCol(e, "est_hours", "Est h", 7).sum(),
		newCol(e, "actual_hours", "Act h", 7).sum(),
		computedCol(e, "var", "Var h", 7, AlignRight).sum(),
		newCol(e, "deps", "Deps", 10),
		newCol(e, "tags", "Tags", 14),
	)
	v = append(v, cfColumns(r, CfTask)...)
	return append(v, newCol(e, "notes", "Notes", 30).hide())
}

func TemplateColumns() []Column {
	e := EntTemplate
	return []Column{
		newCol(e, "name", "Template", 22),
		computedCol(e, "count", "Tasks", 6, AlignRight),
		computedCol(e, "hours", "Est h", 7, AlignRight),
		computedCol(e, "span", "Days", 5, AlignRight),
		newCol(e, "description", "Description", 40).editor(EdText),
	}
}

func TemplateTaskColumns() []Column {
	e := EntTemplateTask
	return []Column{
		newCol(e, "seq", "Seq", 4),
		newCol(e, "title", "Title", 28),
		newCol(e, "phase", "Phase", 12).editor(EdText),
		newCol(e, "est_hours", "Est h", 7).sum(),
		newCol(e, "offset_days", "Offset d", 8),
		newCol(e, "duration_days", "Dur d", 6),
	}
}

func CustomFieldColumns() []Column {
	e := EntCustomField
	return []Column{
		newCol(e, "entity", "Applies to", 10),
		newCol(e, "name", "Name", 20),
		newCol(e, "kind", "Kind", 8),
		newCol(e, "options", "Options (enum)", 36),
		newCol(e, "seq", "Order", 6),
	}
}

func UserColumns() []Column {
	e := EntUser
	return []Column{
		computedCol(e, "username", "Username", 12, AlignLeft),
		newCol(e, "display_name", "Name", 20),
		newCol(e, "role", "Role", 8),
		newCol(e, "active", "Active", 7),
		computedCol(e, "open_tasks", "Open tasks", 10, AlignRight).sum(),
		computedCol(e, "open_hours", "Open est h", 10, AlignRight).sum(),
		computedCol(e, "owned", "Projects", 8, AlignRight).sum(),
	}
}

func ProgramColumns() []Column {
	e := EntProgram
	return []Column{
		newCol(e, "name", "Program", 22),
		newCol(e, "client", "Client", 22),
		newCol(e, "prefix", "Prefix", 7),
		newCol(e, "color", "Colour", 8),
		computedCol(e, "projects", "Projects", 8, AlignRight).sum(),
	}
}

// ActivityColumns: activity is not an entity; EntUser is borrowed as a tag.
func ActivityColumns() []Column {
	e := EntUser
	return []Column{
		computedCol(e, "ts", "When", 9, AlignLeft),
		computedCol(e, "user", "Who", 14, AlignLeft),
		computedCol(e, "action", "Action", 7, AlignLeft),
		computedCol(e, "label", "Change", 60, AlignLeft),
		computedCol(e, "state", "State", 7, AlignLeft),
	}
}

type StyleKind int

const (
	StPlain StyleKind = iota
	StMuted
	StStrong
	StProjectStatus
	StTaskStatus
	StPriority
	StHealth
	StRole
	StOverdue
	StDueSoon
	StBar
	StBad
	StGood
	StTags
	StColor
	StUser
	StStruck
)

// Style is a visual hint; front ends map it to colours. Value carries the
// enum value or colour; Bar uses Pct and Level (0 ok, 1 warn, 2 bad).
type Style struct {
	Kind   StyleKind
	Value  string
	Pct    float64
	Level  int
	UserID int64
}

type SortKind int

const (
	SortNum SortKind = iota
	SortText
	SortNull
)

type SortKey struct {
	Kind SortKind
	Num  float64
	Text string
}

// CmpAsc orders numbers, then text, then nulls.
func (a SortKey) CmpAsc(b SortKey) int {
	if a.Kind == SortNum && b.Kind == SortNum {
		switch {
		case a.Num < b.Num:
			return -1
		case a.Num > b.Num:
			return 1
		}
		return 0
	}
	if a.Kind == SortText && b.Kind == SortText {
		return strings.Compare(a.Text, b.Text)
	}
	return int(a.Kind) - int(b.Kind)
}

func (a SortKey) GroupKey() string {
	switch a.Kind {
	case SortNull:
		return ""
	case SortNum:
		return strconv.FormatFloat(a.Num, 'f', -1, 64)
	}
	return a.Text
}

type Cell struct {
	Text  string
	Sort  SortKey
	Style Style
	Num   *float64
}

func textCell(s string) Cell {
	c := Cell{Text: s, Sort: SortKey{Kind: SortNull}}
	if s != "" {
		c.Sort = SortKey{Kind: SortText, Text: strings.ToLower(s)}
	}
	return c
}

func styled(s string, st Style) Cell {
	c := textCell(s)
	c.Style = st
	return c
}

func (c Cell) sorted(k SortKey) Cell { c.Sort = k; return c }

func numKey(f float64) SortKey { return SortKey{Kind: SortNum, Num: f} }

func hoursCell(h *float64) Cell {
	if h == nil {
		return Cell{Sort: SortKey{Kind: SortNull}}
	}
	v := *h
	return Cell{Text: FmtHours(v), Sort: numKey(v), Num: &v}
}

func intCell(n int64) Cell {
	f := float64(n)
	return Cell{Text: strconv.FormatInt(n, 10), Sort: numKey(f), Num: &f}
}

func emptyCell() Cell { return textCell("") }

// FmtHours: "16", "2.5", "1.25".
func FmtHours(h float64) string {
	h = Round2(h)
	if h == math.Trunc(h) {
		return strconv.FormatFloat(h, 'f', 0, 64)
	}
	if math.Abs(math.Mod(h*10, 1)) < 1e-9 {
		return strconv.FormatFloat(h, 'f', 1, 64)
	}
	return strconv.FormatFloat(h, 'f', 2, 64)
}

func dateCell(r *Replica, d *string, open bool) Cell {
	date, ok := parseDatePtr(d)
	if !ok {
		return emptyCell()
	}
	st := Style{}
	if open {
		if date < r.Today {
			st.Kind = StOverdue
		} else if int(date)-int(r.Today) <= 7 {
			st.Kind = StDueSoon
		}
	}
	return Cell{Text: date.ISO(), Sort: numKey(float64(date)), Style: st}
}

func plainDate(d *string) Cell {
	date, ok := parseDatePtr(d)
	if !ok {
		return emptyCell()
	}
	return textCell(date.ISO()).sorted(numKey(float64(date)))
}

func userCell(r *Replica, id *int64) Cell {
	if id != nil {
		if u, ok := r.Users[*id]; ok {
			return styled(u.DisplayName, Style{Kind: StUser, UserID: u.ID})
		}
	}
	return emptyCell()
}

func cfCell(r *Replica, cf CfValues, key string) Cell {
	id, ok := CfID(key)
	if !ok {
		return emptyCell()
	}
	def, ok := r.CustomFields[id]
	if !ok {
		return emptyCell()
	}
	v, has := cf[strconv.FormatInt(id, 10)]
	if !has || v == nil {
		return emptyCell()
	}
	switch def.Kind {
	case CfNumber:
		n, _ := AsFloat(v)
		return Cell{Text: FmtHours(n), Sort: numKey(n), Num: &n}
	case CfDate:
		s, _ := v.(string)
		return plainDate(&s)
	}
	s, _ := v.(string)
	return textCell(s)
}

func tagsCell(tags []string) Cell {
	st := Style{}
	if len(tags) > 0 {
		st.Kind = StTags
	}
	return styled(strings.Join(tags, " "), st)
}

func barCell(pct float64, level int) Cell {
	return Cell{Text: fmt.Sprintf("%.0f%%", math.Round(pct)), Sort: numKey(pct), Style: Style{Kind: StBar, Pct: pct, Level: level}}
}

func firstLine(s string) string {
	if i := strings.IndexByte(s, '\n'); i >= 0 {
		return strings.TrimSuffix(s[:i], "\r")
	}
	return s
}

// CellFor returns the cell for row id of col.Entity (or an activity id for
// activity columns).
func CellFor(r *Replica, col *Column, id int64) Cell {
	key := col.Key
	switch col.Entity {
	case EntProject:
		p, ok := r.Projects[id]
		if !ok {
			return emptyCell()
		}
		open := r.ProjectOpen(p)
		switch key {
		case "code":
			return styled(p.Code, Style{Kind: StStrong})
		case "name":
			return textCell(p.Name)
		case "program_id":
			if p.ProgramID != nil {
				if pg, ok := r.Programs[*p.ProgramID]; ok {
					return styled(pg.Name, Style{Kind: StColor, Value: pg.Color})
				}
			}
			return emptyCell()
		case "owner_id":
			return userCell(r, p.OwnerID)
		case "status":
			return styled(p.Status.Label(), Style{Kind: StProjectStatus, Value: string(p.Status)}).sorted(numKey(float64(p.Status.Index())))
		case "priority":
			return styled(p.Priority.Label(), Style{Kind: StPriority, Value: string(p.Priority)}).sorted(numKey(float64(p.Priority.Index())))
		case "health":
			return styled(p.Health.Label(), Style{Kind: StHealth, Value: string(p.Health)}).sorted(numKey(float64(p.Health.Index())))
		case "start_date":
			return plainDate(p.StartDate)
		case "due_date":
			return dateCell(r, p.DueDate, open)
		case "budget_hours":
			return hoursCell(p.BudgetHours)
		case "est":
			return hoursCell(Ptr(r.Metrics(id).EstTotal))
		case "actual":
			c := hoursCell(Ptr(r.Metrics(id).ActualTotal))
			if b, ok := r.BurnPct(p); ok && b > 100 {
				c.Style = Style{Kind: StBad}
			}
			return c
		case "burn":
			b, ok := r.BurnPct(p)
			if !ok {
				return emptyCell()
			}
			lvl := 0
			if b > 100 {
				lvl = 2
			} else if b > 90 {
				lvl = 1
			}
			return barCell(b, lvl)
		case "done":
			m := r.Metrics(id)
			c := barCell(m.PctDone(), 0)
			c.Text = fmt.Sprintf("%d/%d", m.TasksDone, m.TasksTotal)
			return c
		case "overdue":
			n := r.Metrics(id).TasksOverdue
			c := intCell(int64(n))
			if n == 0 {
				c.Text = ""
			} else {
				c.Style = Style{Kind: StOverdue}
			}
			return c
		case "tags":
			return tagsCell(p.Tags)
		case "notes":
			return textCell(firstLine(p.Notes))
		}
		if strings.HasPrefix(key, "cf.") {
			return cfCell(r, p.CF, key)
		}
		return emptyCell()
	case EntTask:
		t, ok := r.Tasks[id]
		if !ok {
			return emptyCell()
		}
		open := t.Status != TaskDone
		switch key {
		case "project":
			return styled(r.ProjectCode(t.ProjectID), Style{Kind: StStrong})
		case "seq":
			return intCell(t.Seq)
		case "title":
			st := Style{}
			if !open {
				st.Kind = StMuted
			}
			return styled(t.Title, st)
		case "phase":
			return textCell(t.Phase)
		case "status":
			st := Style{Kind: StTaskStatus, Value: string(t.Status)}
			if t.Status != TaskBlocked && r.TaskBlocked(t) {
				st.Value = string(TaskBlocked)
			}
			return styled(t.Status.Label(), st).sorted(numKey(float64(t.Status.Index())))
		case "priority":
			return styled(t.Priority.Label(), Style{Kind: StPriority, Value: string(t.Priority)}).sorted(numKey(float64(t.Priority.Index())))
		case "assignee_id":
			return userCell(r, t.AssigneeID)
		case "start_date":
			return plainDate(t.StartDate)
		case "due_date":
			return dateCell(r, t.DueDate, open)
		case "est_hours":
			return hoursCell(t.EstHours)
		case "actual_hours":
			c := hoursCell(Ptr(t.ActualHours))
			if t.ActualHours == 0 {
				c.Text = ""
			}
			if t.EstHours != nil && t.ActualHours > *t.EstHours {
				c.Style = Style{Kind: StBad}
			}
			return c
		case "var":
			if t.EstHours != nil && t.ActualHours > 0 {
				v := t.ActualHours - *t.EstHours
				c := hoursCell(&v)
				if v > 0 {
					c.Text = "+" + c.Text
					c.Style = Style{Kind: StBad}
				} else {
					c.Style = Style{Kind: StGood}
				}
				return c
			}
			return emptyCell()
		case "deps":
			var parts []string
			waiting := false
			for _, d := range t.Deps {
				if dt, ok := r.Tasks[d]; ok {
					parts = append(parts, fmt.Sprintf("#%d", dt.Seq))
				}
				if s, ok := r.TaskStatusOf(d); ok && s != TaskDone {
					waiting = true
				}
			}
			st := Style{Kind: StMuted}
			if waiting && open {
				st.Kind = StDueSoon
			}
			return styled(strings.Join(parts, " "), st)
		case "tags":
			return tagsCell(t.Tags)
		case "notes":
			return textCell(firstLine(t.Notes))
		}
		if strings.HasPrefix(key, "cf.") {
			return cfCell(r, t.CF, key)
		}
		return emptyCell()
	case EntTemplate:
		t, ok := r.Templates[id]
		if !ok {
			return emptyCell()
		}
		n, h, span := r.TemplateStats(id)
		switch key {
		case "name":
			return styled(t.Name, Style{Kind: StStrong})
		case "description":
			return textCell(t.Description)
		case "count":
			return intCell(int64(n))
		case "hours":
			return hoursCell(&h)
		case "span":
			return intCell(span)
		}
		return emptyCell()
	case EntTemplateTask:
		t, ok := r.TemplateTasks[id]
		if !ok {
			return emptyCell()
		}
		switch key {
		case "seq":
			return intCell(t.Seq)
		case "title":
			return textCell(t.Title)
		case "phase":
			return textCell(t.Phase)
		case "est_hours":
			return hoursCell(t.EstHours)
		case "offset_days":
			return intCell(t.OffsetDays)
		case "duration_days":
			return intCell(t.DurationDays)
		}
		return emptyCell()
	case EntCustomField:
		c, ok := r.CustomFields[id]
		if !ok {
			return emptyCell()
		}
		switch key {
		case "entity":
			return textCell(c.Entity.Label())
		case "name":
			return styled(c.Name, Style{Kind: StStrong})
		case "kind":
			return textCell(c.Kind.Label())
		case "options":
			return tagsCell(c.Options)
		case "seq":
			return intCell(c.Seq)
		}
		return emptyCell()
	case EntUser:
		switch key {
		case "ts", "user", "action", "label", "state":
			return ActivityCell(r, key, id)
		}
		u, ok := r.Users[id]
		if !ok {
			return emptyCell()
		}
		switch key {
		case "username":
			return styled(u.Username, Style{Kind: StUser, UserID: u.ID})
		case "display_name":
			return textCell(u.DisplayName)
		case "role":
			return styled(u.Role.Label(), Style{Kind: StRole, Value: string(u.Role)}).sorted(numKey(float64(u.Role.Index())))
		case "active":
			if u.Active {
				return styled("✓ yes", Style{Kind: StGood})
			}
			return styled("✗ no", Style{Kind: StMuted})
		case "open_tasks", "open_hours":
			n, hours := 0, 0.0
			for _, t := range r.Tasks {
				if t.AssigneeID != nil && *t.AssigneeID == id && t.Status != TaskDone {
					n++
					if t.EstHours != nil {
						hours += *t.EstHours
					}
				}
			}
			if key == "open_tasks" {
				return intCell(int64(n))
			}
			return hoursCell(&hours)
		case "owned":
			n := 0
			for _, p := range r.Projects {
				if p.OwnerID != nil && *p.OwnerID == id {
					n++
				}
			}
			return intCell(int64(n))
		}
		return emptyCell()
	case EntProgram:
		p, ok := r.Programs[id]
		if !ok {
			return emptyCell()
		}
		switch key {
		case "name":
			return styled(p.Name, Style{Kind: StColor, Value: p.Color})
		case "client":
			return textCell(p.Client)
		case "prefix":
			return styled(p.Prefix, Style{Kind: StStrong})
		case "color":
			return styled(p.Color, Style{Kind: StColor, Value: p.Color})
		case "projects":
			n := 0
			for _, x := range r.Projects {
				if x.ProgramID != nil && *x.ProgramID == id {
					n++
				}
			}
			return intCell(int64(n))
		}
	}
	return emptyCell()
}

// ActivityCell: activity rows are addressed by activity id.
func ActivityCell(r *Replica, key string, id int64) Cell {
	a, ok := r.Activity[id]
	if !ok {
		return emptyCell()
	}
	struck := a.State == StateUndone
	var c Cell
	switch key {
	case "ts":
		// today: "14:03"; earlier: "12 Oct"
		var text string
		if len(a.TS) >= 10 {
			if d, ok := ParseISO(a.TS[0:10]); ok {
				if d == r.Today && len(a.TS) >= 16 {
					text = a.TS[11:16]
				} else {
					text = strings.TrimSpace(d.Short(r.Today.Year()))
				}
			} else {
				text = a.TS
			}
		} else {
			text = a.TS
		}
		c = textCell(text).sorted(numKey(float64(a.ID)))
	case "user":
		c = userCell(r, a.UserID)
	case "action":
		c = textCell(a.Action.Label())
	case "label":
		c = textCell(a.Label)
	case "state":
		st := Style{}
		if struck {
			st.Kind = StMuted
		}
		c = styled(a.State.Label(), st)
	default:
		return emptyCell()
	}
	if struck && key == "label" {
		c.Style = Style{Kind: StStruck}
	}
	return c
}

// ActivityWhen: relative time given "now" in Unix seconds.
func ActivityWhen(a *Activity, nowSecs int64) string { return RelativeTime(a.TS, nowSecs) }

// ---- sorting & grouping ----

type SortSpec struct {
	Key  string
	Desc bool
}

type GridRow struct {
	IsGroup   bool
	ID        int64 // data rows
	Key       string
	Label     string
	Count     int
	Collapsed bool
	Sums      []*float64 // aligned with the columns passed to BuildRows
	Style     Style
}

func findCol(cols []Column, key string) *Column {
	for i := range cols {
		if cols[i].Key == key {
			return &cols[i]
		}
	}
	return nil
}

func SortIDs(r *Replica, cols []Column, ids []int64, spec []SortSpec) {
	if len(spec) == 0 {
		return
	}
	type sc struct {
		col  *Column
		desc bool
	}
	var specs []sc
	for _, s := range spec {
		if c := findCol(cols, s.Key); c != nil {
			specs = append(specs, sc{c, s.Desc})
		}
	}
	keys := make(map[int64][]SortKey, len(ids))
	for _, id := range ids {
		k := make([]SortKey, len(specs))
		for i, s := range specs {
			k[i] = CellFor(r, s.col, id).Sort
		}
		keys[id] = k
	}
	sort.SliceStable(ids, func(a, b int) bool {
		ka, kb := keys[ids[a]], keys[ids[b]]
		for i, s := range specs {
			o := ka[i].CmpAsc(kb[i])
			// nulls stay last in both directions
			if s.desc && ka[i].Kind != SortNull && kb[i].Kind != SortNull {
				o = -o
			}
			if o != 0 {
				return o < 0
			}
		}
		return false
	})
}

func sums(r *Replica, cols []Column, ids []int64) []*float64 {
	out := make([]*float64, len(cols))
	for i := range cols {
		if !cols[i].Sum {
			continue
		}
		total := 0.0
		for _, id := range ids {
			if n := CellFor(r, &cols[i], id).Num; n != nil {
				total += *n
			}
		}
		out[i] = &total
	}
	return out
}

// Totals: footer totals for the visible rows.
func Totals(r *Replica, cols []Column, ids []int64) []*float64 { return sums(r, cols, ids) }

// BuildRows sorts ids and optionally groups by a column, with group headers
// carrying counts and subtotals.
func BuildRows(r *Replica, cols []Column, ids []int64, spec []SortSpec, groupBy string, collapsed map[string]bool) []GridRow {
	ids = append([]int64(nil), ids...)
	gcol := findCol(cols, groupBy)
	if groupBy == "" || gcol == nil {
		SortIDs(r, cols, ids, spec)
		out := make([]GridRow, len(ids))
		for i, id := range ids {
			out[i] = GridRow{ID: id}
		}
		return out
	}
	full := []SortSpec{{Key: gcol.Key}}
	for _, s := range spec {
		if s.Key != gcol.Key {
			full = append(full, s)
		}
	}
	SortIDs(r, cols, ids, full)
	var out []GridRow
	for i := 0; i < len(ids); {
		c0 := CellFor(r, gcol, ids[i])
		key := c0.Sort.GroupKey()
		j := i + 1
		for j < len(ids) && CellFor(r, gcol, ids[j]).Sort.GroupKey() == key {
			j++
		}
		members := ids[i:j]
		label := "(none)"
		if c0.Text != "" {
			label = groupLabel(r, gcol, members[0], c0)
		}
		out = append(out, GridRow{
			IsGroup: true, Key: key, Label: label, Count: len(members),
			Collapsed: collapsed[key], Sums: sums(r, cols, members), Style: c0.Style,
		})
		if !collapsed[key] {
			for _, id := range members {
				out = append(out, GridRow{ID: id})
			}
		}
		i = j
	}
	return out
}

func groupLabel(r *Replica, col *Column, id int64, c Cell) string {
	if col.Entity == EntTask && col.Key == "project" {
		if t, ok := r.Tasks[id]; ok {
			if p, ok := r.Projects[t.ProjectID]; ok {
				return p.Code + "  " + p.Name
			}
		}
	}
	return c.Text
}

// ---- editing ----

type Choice struct {
	Value  any
	Label  string
	Detail string
	Style  Style
}

func noneChoice() Choice { return Choice{Value: nil, Label: "(none)", Style: Style{Kind: StMuted}} }

// Choices for Select / Combo / Suggest / Chips editors of field on entity
// row id (0 = a new row; projectHint gives the project for new tasks).
func Choices(r *Replica, e Entity, field string, id int64, projectHint *int64) []Choice {
	if cfid, ok := CfID(field); ok {
		cf, ok := r.CustomFields[cfid]
		if !ok {
			return nil
		}
		v := []Choice{noneChoice()}
		for _, o := range cf.Options {
			v = append(v, Choice{Value: o, Label: o})
		}
		return v
	}
	users := func() []Choice {
		v := []Choice{noneChoice()}
		var us []*User
		for _, u := range r.Users {
			if u.Active {
				us = append(us, u)
			}
		}
		sort.SliceStable(us, func(i, j int) bool {
			if us[i].DisplayName != us[j].DisplayName {
				return us[i].DisplayName < us[j].DisplayName
			}
			return us[i].ID < us[j].ID
		})
		for _, u := range us {
			v = append(v, Choice{Value: u.ID, Label: u.DisplayName, Detail: "@" + u.Username + " · " + u.Role.Label(), Style: Style{Kind: StUser, UserID: u.ID}})
		}
		return v
	}
	taskProject := func() *int64 {
		if e == EntTask {
			if t, ok := r.Tasks[id]; ok {
				return &t.ProjectID
			}
		}
		return projectHint
	}
	switch {
	case field == "status" && e == EntProject:
		var v []Choice
		for _, s := range AllProjectStatuses {
			v = append(v, Choice{Value: string(s), Label: s.Label(), Style: Style{Kind: StProjectStatus, Value: string(s)}})
		}
		return v
	case field == "status":
		var v []Choice
		for _, s := range AllTaskStatuses {
			v = append(v, Choice{Value: string(s), Label: s.Label(), Style: Style{Kind: StTaskStatus, Value: string(s)}})
		}
		return v
	case field == "priority":
		var v []Choice
		for _, s := range AllPriorities {
			v = append(v, Choice{Value: string(s), Label: s.Label(), Style: Style{Kind: StPriority, Value: string(s)}})
		}
		return v
	case field == "health":
		var v []Choice
		for _, s := range AllHealth {
			v = append(v, Choice{Value: string(s), Label: s.Label(), Style: Style{Kind: StHealth, Value: string(s)}})
		}
		return v
	case field == "role":
		var v []Choice
		for _, s := range AllRoles {
			v = append(v, Choice{Value: string(s), Label: s.Label(), Style: Style{Kind: StRole, Value: string(s)}})
		}
		return v
	case field == "kind":
		var v []Choice
		for _, s := range AllCfKinds {
			v = append(v, Choice{Value: string(s), Label: s.Label()})
		}
		return v
	case field == "entity":
		var v []Choice
		for _, s := range AllCfEntities {
			v = append(v, Choice{Value: string(s), Label: s.Label()})
		}
		return v
	case field == "active":
		return []Choice{{Value: true, Label: "✓ yes", Style: Style{Kind: StGood}}, {Value: false, Label: "✗ no", Style: Style{Kind: StMuted}}}
	case field == "owner_id" || field == "assignee_id":
		return users()
	case field == "program_id":
		v := []Choice{noneChoice()}
		var ps []*Program
		for _, p := range r.Programs {
			ps = append(ps, p)
		}
		sort.Slice(ps, func(i, j int) bool { return ps[i].ID < ps[j].ID })
		for _, p := range ps {
			v = append(v, Choice{Value: p.ID, Label: p.Name, Detail: p.Prefix + " · " + p.Client, Style: Style{Kind: StColor, Value: p.Color}})
		}
		return v
	case field == "template_id":
		v := []Choice{{Value: nil, Label: "(blank project)", Detail: "no tasks", Style: Style{Kind: StMuted}}}
		for _, tid := range SortedIDs(r.Templates) {
			t := r.Templates[tid]
			n, h, span := r.TemplateStats(t.ID)
			v = append(v, Choice{Value: t.ID, Label: t.Name, Detail: fmt.Sprintf("%d tasks, %s h, %d days", n, FmtHours(h), span)})
		}
		return v
	case field == "project_id":
		var v []Choice
		for _, pid := range SortedIDs(r.Projects) {
			p := r.Projects[pid]
			v = append(v, Choice{Value: p.ID, Label: p.Code + "  " + p.Name, Detail: r.ProgramName(p.ProgramID), Style: Style{Kind: StProjectStatus, Value: string(p.Status)}})
		}
		return v
	case field == "phase":
		var phases []string
		if e == EntTask {
			if pid := taskProject(); pid != nil {
				phases = r.PhasesForProject(*pid)
			}
		} else {
			set := map[string]bool{}
			for _, t := range r.TemplateTasks {
				if t.Phase != "" {
					set[t.Phase] = true
				}
			}
			for p := range set {
				phases = append(phases, p)
			}
			sort.Strings(phases)
		}
		var v []Choice
		for _, p := range phases {
			v = append(v, Choice{Value: p, Label: p})
		}
		return v
	case field == "tags":
		te := EntTask
		if e == EntProject {
			te = EntProject
		}
		var v []Choice
		for _, tc := range r.TagCounts(te) {
			v = append(v, Choice{Value: tc.Tag, Label: tc.Tag, Detail: fmt.Sprintf("%d×", tc.Count), Style: Style{Kind: StTags}})
		}
		return v
	case field == "deps":
		pid := taskProject()
		if pid == nil {
			return nil
		}
		var v []Choice
		for _, tid := range r.ProjectTasks(*pid) {
			if tid == id {
				continue
			}
			t := r.Tasks[tid]
			v = append(v, Choice{Value: t.ID, Label: fmt.Sprintf("#%d %s", t.Seq, t.Title), Detail: t.Status.Label(), Style: Style{Kind: StTaskStatus, Value: string(t.Status)}})
		}
		return v
	case field == "options":
		var v []Choice
		if c, ok := r.CustomFields[id]; ok {
			for _, o := range c.Options {
				v = append(v, Choice{Value: o, Label: o})
			}
		}
		return v
	}
	return nil
}

type FilteredChoice struct {
	Index   int
	Matched []int
}

// FilterChoices fuzzy-filters by label (and detail), best first; an empty
// query keeps the order.
func FilterChoices(choices []Choice, query string) []FilteredChoice {
	if strings.TrimSpace(query) == "" {
		out := make([]FilteredChoice, len(choices))
		for i := range choices {
			out[i] = FilteredChoice{Index: i, Matched: []int{}}
		}
		return out
	}
	type scored struct {
		i, score int
		m        []int
	}
	var out []scored
	for i, c := range choices {
		if m := FuzzyMatch(query, c.Label); m != nil {
			out = append(out, scored{i, m.Score, m.Indices})
		} else if m := FuzzyMatch(query, c.Detail); m != nil {
			out = append(out, scored{i, m.Score - 30, []int{}})
		}
	}
	sort.SliceStable(out, func(a, b int) bool { return out[a].score > out[b].score })
	res := make([]FilteredChoice, len(out))
	for i, s := range out {
		res[i] = FilteredChoice{s.i, s.m}
	}
	return res
}

func i64(v any) *int64 {
	if n, ok := AsInt(v); ok {
		return &n
	}
	return nil
}

// DisplayValue renders an arbitrary field value (conflicts, activity, chips).
func DisplayValue(r *Replica, e Entity, field string, v any) string {
	if v == nil {
		return "(empty)"
	}
	if b, ok := v.(bool); ok {
		if b {
			return "yes"
		}
		return "no"
	}
	if list, ok := AsList(v); ok {
		parts := make([]string, 0, len(list))
		for _, x := range list {
			if field == "deps" {
				id, ok := AsInt(x)
				if !ok {
					continue
				}
				if t, ok := r.Tasks[id]; ok {
					parts = append(parts, fmt.Sprintf("#%d", t.Seq))
				} else {
					parts = append(parts, fmt.Sprintf("task %d", id))
				}
			} else if s, ok := x.(string); ok {
				parts = append(parts, s)
			} else {
				parts = append(parts, listKey(x))
			}
		}
		return strings.Join(parts, " ")
	}
	if f, ok := AsFloat(v); ok {
		switch field {
		case "owner_id", "assignee_id":
			return r.UserName(i64(v))
		case "program_id":
			return r.ProgramName(i64(v))
		case "template_id":
			if id := i64(v); id != nil {
				if t, ok := r.Templates[*id]; ok {
					return t.Name
				}
			}
			return listKey(v)
		case "project_id":
			if id := i64(v); id != nil {
				return r.ProjectCode(*id)
			}
			return listKey(v)
		}
		return FmtHours(f)
	}
	if s, ok := v.(string); ok {
		switch {
		case e == EntProject && field == "status":
			if x, ok := ParseProjectStatus(s); ok {
				return x.Label()
			}
		case e == EntTask && field == "status":
			if x, ok := ParseTaskStatus(s); ok {
				return x.Label()
			}
		case field == "priority":
			if x, ok := ParsePriority(s); ok {
				return x.Label()
			}
		case field == "health":
			if x, ok := ParseHealth(s); ok {
				return x.Label()
			}
		}
		return s
	}
	return "…"
}

// EditText is what a text editor shows when editing starts (also used for copy).
func EditText(r *Replica, e Entity, field string, id int64) string {
	v := r.FieldValue(e, id, field)
	switch x := v.(type) {
	case nil:
		return ""
	case string:
		return x
	case bool:
		if x {
			return "yes"
		}
		return "no"
	}
	if _, ok := AsList(v); ok {
		return DisplayValue(r, e, field, v)
	}
	if f, ok := AsFloat(v); ok {
		switch field {
		case "owner_id", "assignee_id":
			return r.Username(i64(v))
		case "program_id":
			return r.ProgramName(i64(v))
		}
		return FmtHours(f)
	}
	return ""
}

// ParseInput turns typed or pasted text into a field value. It accepts
// labels, usernames, display names, natural dates, #seq deps and
// space/comma separated lists.
func ParseInput(r *Replica, e Entity, field string, id int64, text string) (any, error) {
	t := strings.TrimSpace(text)
	if cfid, ok := CfID(field); ok {
		cf, ok := r.CustomFields[cfid]
		if !ok {
			return nil, fmt.Errorf("unknown custom field")
		}
		if t == "" {
			return nil, nil
		}
		switch cf.Kind {
		case CfText:
			return t, nil
		case CfNumber:
			f, ok := parseNum(t)
			if !ok {
				return nil, fmt.Errorf("'%s' is not a number", t)
			}
			return f, nil
		case CfDate:
			d, err := ParseNatural(t, r.Today)
			if err != nil {
				return nil, err
			}
			if d == nil {
				return nil, nil
			}
			return d.ISO(), nil
		}
		for _, o := range cf.Options {
			if strings.EqualFold(o, t) {
				return o, nil
			}
		}
		for _, o := range cf.Options {
			if strings.HasPrefix(strings.ToLower(o), strings.ToLower(t)) {
				return o, nil
			}
		}
		return nil, fmt.Errorf("%s must be one of %s", cf.Name, strings.Join(cf.Options, ", "))
	}
	m := Field(e, field)
	if m == nil {
		return nil, fmt.Errorf("unknown field %s", field)
	}
	if t == "" {
		switch m.Kind.Tag {
		case KTags, KOptions:
			return []string{}, nil
		case KDeps:
			return []int64{}, nil
		case KText:
			return "", nil
		case KHours:
			if !m.Nullable {
				return 0.0, nil
			}
		}
		return nil, nil
	}
	lower := strings.ToLower(t)
	switch m.Kind.Tag {
	case KText, KCode, KUsername, KPrefix, KColor, KPassword:
		// notes keep their surrounding whitespace
		if m.Kind.Tag == KText && m.Kind.Max > 2000 {
			return text, nil
		}
		return t, nil
	case KEnum:
		ch := Choices(r, e, field, id, nil)
		norm := strings.ReplaceAll(lower, " ", "_")
		for _, c := range ch {
			s, _ := c.Value.(string)
			if strings.EqualFold(c.Label, t) || strings.EqualFold(s, norm) {
				return c.Value, nil
			}
		}
		for _, c := range ch {
			if strings.HasPrefix(strings.ToLower(c.Label), lower) {
				return c.Value, nil
			}
		}
		labels := make([]string, len(ch))
		for i, c := range ch {
			labels[i] = c.Label
		}
		return nil, fmt.Errorf("%s must be one of %s", m.Label, strings.Join(labels, ", "))
	case KRef:
		ch := Choices(r, e, field, id, nil)
		var hit any
		switch m.Kind.Target {
		case EntUser:
			at := strings.TrimPrefix(lower, "@")
			ids := SortedIDs(r.Users)
			for _, uid := range ids {
				if r.Users[uid].Username == at {
					hit = uid
					break
				}
			}
			if hit == nil {
				for _, uid := range ids {
					if strings.ToLower(r.Users[uid].DisplayName) == lower {
						hit = uid
						break
					}
				}
			}
			if hit == nil {
				for _, uid := range ids {
					if strings.HasPrefix(strings.ToLower(r.Users[uid].DisplayName), lower) {
						hit = uid
						break
					}
				}
			}
		case EntProgram:
			ids := SortedIDs(r.Programs)
			for _, pid := range ids {
				p := r.Programs[pid]
				if strings.ToLower(p.Name) == lower || strings.ToLower(p.Prefix) == lower {
					hit = pid
					break
				}
			}
			if hit == nil {
				for _, pid := range ids {
					if strings.HasPrefix(strings.ToLower(r.Programs[pid].Name), lower) {
						hit = pid
						break
					}
				}
			}
		case EntProject:
			for _, pid := range SortedIDs(r.Projects) {
				if strings.ToLower(r.Projects[pid].Code) == lower {
					hit = pid
					break
				}
			}
		case EntTemplate:
			for _, tid := range SortedIDs(r.Templates) {
				if strings.ToLower(r.Templates[tid].Name) == lower {
					hit = tid
					break
				}
			}
		}
		if lower == "none" || lower == "(none)" {
			return nil, nil
		}
		if hit != nil {
			return hit, nil
		}
		if f := FilterChoices(ch, t); len(f) > 0 && ch[f[0].Index].Value != nil {
			return ch[f[0].Index].Value, nil
		}
		return nil, fmt.Errorf("no %s matches '%s'", m.Kind.Target.Label(), t)
	case KDate:
		d, err := ParseNatural(t, r.Today)
		if err != nil {
			return nil, err
		}
		if d == nil {
			return nil, nil
		}
		return d.ISO(), nil
	case KHours:
		f, ok := parseNum(strings.TrimSpace(strings.TrimRight(t, "hH")))
		if !ok {
			return nil, fmt.Errorf("'%s' is not a number of hours", t)
		}
		return Round2(f), nil
	case KInt:
		n, err := strconv.ParseInt(t, 10, 64)
		if err != nil {
			return nil, fmt.Errorf("'%s' is not a whole number", t)
		}
		return n, nil
	case KBool:
		switch lower {
		case "y", "yes", "true", "1", "✓", "✓ yes", "on":
			return true, nil
		case "n", "no", "false", "0", "✗", "✗ no", "off":
			return false, nil
		}
		return nil, fmt.Errorf("type yes or no")
	case KTags:
		tags := []string{}
		for _, s := range strings.FieldsFunc(t, func(c rune) bool { return c == ' ' || c == ',' || c == ';' }) {
			tags = append(tags, strings.ToLower(strings.TrimLeft(s, "#")))
		}
		for _, x := range tags {
			if !ValidTag(x) {
				return nil, fmt.Errorf("tag '%s' must be a–z, 0–9, '_', '-'", x)
			}
		}
		return tags, nil
	case KOptions:
		out := []string{}
		for _, s := range strings.Split(t, ",") {
			if s = strings.TrimSpace(s); s != "" {
				out = append(out, s)
			}
		}
		return out, nil
	case KDeps:
		task, ok := r.Tasks[id]
		if !ok {
			return nil, fmt.Errorf("unknown task")
		}
		out := []int64{}
		for _, part := range strings.FieldsFunc(t, func(c rune) bool { return c == ' ' || c == ',' || c == ';' }) {
			seq, err := strconv.ParseInt(strings.TrimLeft(part, "#"), 10, 64)
			if err != nil {
				return nil, fmt.Errorf("'%s' is not a task number like #3", part)
			}
			found := false
			for _, tid := range r.ProjectTasks(task.ProjectID) {
				if r.Tasks[tid].Seq == seq {
					out = append(out, tid)
					found = true
					break
				}
			}
			if !found {
				return nil, fmt.Errorf("no task #%d in this project", seq)
			}
		}
		return out, nil
	}
	return nil, fmt.Errorf("unsupported field")
}
