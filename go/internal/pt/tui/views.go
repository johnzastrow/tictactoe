package tui

// View wiring: which grids exist, how each is filtered by its query bar,
// and how master/detail grids follow each other.

import (
	"sort"
	"strings"

	"git.wharf.fluidgrid.org/jcz/tictactoe/go/internal/pt/core"
)

// queryBar is a view's filter: text input, compiled query, autocomplete.
type queryBar struct {
	view      int
	in        *Input
	q         *core.Query
	target    core.Target
	hasTarget bool
	sugg      []string
	sel       int
}

func newQueryBar(view int) *queryBar {
	qb := &queryBar{view: view, in: NewInput(IText, "Filter")}
	switch view {
	case vProjects, vTimeline:
		qb.target, qb.hasTarget = core.TargetProjects, true
	case vTasks, vBoard:
		qb.target, qb.hasTarget = core.TargetTasks, true
	case vActivity:
		qb.target, qb.hasTarget = core.TargetActivity, true
	}
	qb.in.Placeholder = "type to filter — e.g. status:active @me due<+14d #q4  (^F)"
	if !qb.hasTarget {
		qb.in.Placeholder = "type to filter (^F)"
	}
	return qb
}

func (qb *queryBar) compile(r *core.Replica) {
	if qb.hasTarget {
		qb.q = core.CompileQuery(qb.in.Text(), qb.target, r)
	}
}

func (qb *queryBar) text() string { return strings.TrimSpace(qb.in.Text()) }

func allIDs[V any](m map[int64]V) []int64 { return core.SortedIDs(m) }

func (a *App) setupViews() {
	r := a.rep
	g := NewGrid("projects", core.EntProject, core.ProjectColumns(r), 2, func(r *core.Replica) []int64 { return allIDs(r.Projects) })
	g.Sort = []core.SortSpec{{Key: "code"}}
	a.grids["projects"] = g

	pt := NewGrid("projtasks", core.EntTask, core.TaskColumns(r, false), 2, func(r *core.Replica) []int64 {
		pid := a.grids["projects"].CurID()
		return append([]int64(nil), r.ProjectTasks(pid)...)
	})
	pt.Sort = []core.SortSpec{{Key: "seq"}}
	a.grids["projtasks"] = pt

	t := NewGrid("tasks", core.EntTask, core.TaskColumns(r, true), 3, func(r *core.Replica) []int64 { return allIDs(r.Tasks) })
	t.Sort = []core.SortSpec{{Key: "seq"}}
	t.GroupBy = "project"
	a.grids["tasks"] = t

	act := NewGrid("activity", core.EntUser, core.ActivityColumns(), 1, func(r *core.Replica) []int64 { return allIDs(r.Activity) })
	act.Sort = []core.SortSpec{{Key: "ts", Desc: true}}
	act.ReadOnly = true
	a.grids["activity"] = act

	tm := NewGrid("templates", core.EntTemplate, core.TemplateColumns(), 1, func(r *core.Replica) []int64 { return allIDs(r.Templates) })
	tm.Sort = []core.SortSpec{{Key: "name"}}
	a.grids["templates"] = tm

	tt := NewGrid("tmpltasks", core.EntTemplateTask, core.TemplateTaskColumns(), 2, func(r *core.Replica) []int64 {
		var out []int64
		for _, x := range r.TemplateTasksOf(a.grids["templates"].CurID()) {
			out = append(out, x.ID)
		}
		return out
	})
	tt.Sort = []core.SortSpec{{Key: "seq"}}
	a.grids["tmpltasks"] = tt

	f := NewGrid("fields", core.EntCustomField, core.CustomFieldColumns(), 2, func(r *core.Replica) []int64 { return allIDs(r.CustomFields) })
	f.Sort = []core.SortSpec{{Key: "entity"}, {Key: "seq"}}
	a.grids["fields"] = f

	u := NewGrid("users", core.EntUser, core.UserColumns(), 1, func(r *core.Replica) []int64 { return allIDs(r.Users) })
	u.Sort = []core.SortSpec{{Key: "username"}}
	a.grids["users"] = u

	pg := NewGrid("programs", core.EntProgram, core.ProgramColumns(), 1, func(r *core.Replica) []int64 { return allIDs(r.Programs) })
	pg.Sort = []core.SortSpec{{Key: "name"}}
	a.grids["programs"] = pg

	a.board = newBoard()
	a.tl = newTimeline()
	a.dash = &dashState{}
}

// refreshColumns picks up custom-field changes.
func (a *App) refreshColumns() {
	a.grids["projects"].SetColumns(core.ProjectColumns(a.rep))
	a.grids["projtasks"].SetColumns(core.TaskColumns(a.rep, false))
	a.grids["tasks"].SetColumns(core.TaskColumns(a.rep, true))
}

// gridFilter returns the row filter for a grid from its view's query.
func (a *App) gridFilter(name string) func(int64) bool {
	r := a.rep
	switch name {
	case "projects":
		q := a.queries[vProjects]
		q.compile(r)
		if q.q.IsEmpty() {
			return nil
		}
		return func(id int64) bool { return q.q.MatchesProject(r.Projects[id], r) }
	case "tasks":
		q := a.queries[vTasks]
		q.compile(r)
		if q.q.IsEmpty() {
			return nil
		}
		return func(id int64) bool { return q.q.MatchesTask(r.Tasks[id], r) }
	case "activity":
		q := a.queries[vActivity]
		q.compile(r)
		if q.q.IsEmpty() {
			return nil
		}
		return func(id int64) bool { return q.q.MatchesActivity(r.Activity[id], r) }
	case "templates", "fields", "users":
		v := map[string]int{"templates": vTemplates, "fields": vFields, "users": vUsers}[name]
		text := strings.ToLower(a.queries[v].text())
		if text == "" {
			return nil
		}
		g := a.grids[name]
		return func(id int64) bool {
			for _, c := range g.vis() {
				if strings.Contains(strings.ToLower(core.CellFor(r, &c, id).Text), text) {
					return true
				}
			}
			return false
		}
	}
	return nil
}

// rebuild recomputes every grid from the replica (masters before details).
func (a *App) rebuild() {
	for _, name := range []string{"projects", "projtasks", "tasks", "activity", "templates", "tmpltasks", "fields", "users", "programs"} {
		a.grids[name].Rebuild(a.rep, a.gridFilter(name))
	}
	a.queries[vBoard].compile(a.rep)
	a.queries[vTimeline].compile(a.rep)
	a.board.clamp(a)
	a.tl.rebuild(a)
}

// rebuildDetail refreshes a detail grid after its master's cursor moved.
func (a *App) rebuildDetail() {
	switch a.view {
	case vProjects:
		a.grids["projtasks"].Rebuild(a.rep, nil)
	case vTemplates:
		a.grids["tmpltasks"].Rebuild(a.rep, nil)
	}
}

// activeGrid is the grid that has keyboard focus (nil on non-grid views).
func (a *App) activeGrid() *Grid {
	detail := a.pane == 1 && a.detail[a.view]
	switch a.view {
	case vProjects:
		if detail {
			return a.grids["projtasks"]
		}
		return a.grids["projects"]
	case vTasks:
		return a.grids["tasks"]
	case vActivity:
		return a.grids["activity"]
	case vTemplates:
		if detail {
			return a.grids["tmpltasks"]
		}
		return a.grids["templates"]
	case vFields:
		return a.grids["fields"]
	case vUsers:
		if a.pane == 1 {
			return a.grids["programs"]
		}
		return a.grids["users"]
	}
	return nil
}

// hasPanes: views where F6 switches between two grids.
func (a *App) hasPanes() bool {
	switch a.view {
	case vProjects, vTemplates:
		return a.detail[a.view]
	case vUsers:
		return true
	}
	return false
}

// ---- query autocomplete ----

// currentToken returns the token under the cursor and its rune span.
func (qb *queryBar) currentToken() (string, int, int) {
	t := qb.in.text
	start := qb.in.cur
	for start > 0 && t[start-1] != ' ' {
		start--
	}
	end := qb.in.cur
	for end < len(t) && t[end] != ' ' {
		end++
	}
	return string(t[start:end]), start, end
}

// suggestions for the token under the cursor (fields, then values).
func (a *App) querySuggestions(qb *queryBar) []string {
	tok, _, _ := qb.currentToken()
	neg := ""
	if strings.HasPrefix(tok, "-") {
		neg, tok = "-", tok[1:]
	}
	r := a.rep
	low := strings.ToLower(tok)
	var cands []string
	addPrefixed := func(prefix string, vals []string) {
		for _, v := range vals {
			v = strings.ReplaceAll(strings.ToLower(v), " ", "_")
			cands = append(cands, neg+prefix+v)
		}
	}
	switch {
	case strings.HasPrefix(tok, "@"):
		users := []string{"me"}
		for _, u := range r.Users {
			users = append(users, u.Username)
		}
		sort.Strings(users[1:])
		addPrefixed("@", users)
	case strings.HasPrefix(tok, "#"):
		ent := core.EntTask
		if qb.target == core.TargetProjects {
			ent = core.EntProject
		}
		for _, tc := range r.TagCounts(ent) {
			cands = append(cands, neg+"#"+tc.Tag)
		}
	case strings.ContainsAny(tok, ":<>="):
		i := strings.IndexAny(tok, ":<>=")
		field, op := tok[:i], tok[i:i+1]
		if !qb.hasTarget {
			return nil
		}
		var vals []string
		switch field {
		case "status":
			if qb.target == core.TargetProjects {
				for _, s := range core.AllProjectStatuses {
					vals = append(vals, string(s))
				}
			} else {
				for _, s := range core.AllTaskStatuses {
					vals = append(vals, string(s))
				}
			}
		case "priority":
			for _, s := range core.AllPriorities {
				vals = append(vals, string(s))
			}
		case "health":
			for _, s := range core.AllHealth {
				vals = append(vals, string(s))
			}
		case "owner", "assignee", "user", "who":
			vals = append(vals, "me", "none")
			for _, u := range r.Users {
				vals = append(vals, u.Username)
			}
		case "program":
			for _, p := range r.Programs {
				vals = append(vals, strings.ToLower(p.Prefix))
			}
		case "project":
			for _, p := range r.Projects {
				vals = append(vals, strings.ToLower(p.Code))
			}
		case "tag":
			for _, tc := range r.TagCounts(core.EntTask) {
				vals = append(vals, tc.Tag)
			}
		case "is":
			vals = core.IsValues(qb.target)
		case "has":
			vals = core.HasValues(qb.target)
		case "due", "start":
			vals = []string{"today", "+7d", "+14d", "eow", "eom", "-7d", "none"}
		default:
			for _, cf := range r.CustomFields {
				if cf.QueryName() == field && cf.Kind == core.CfEnum {
					vals = cf.Options
				}
			}
		}
		sort.SliceStable(vals, func(i, j int) bool { return false })
		addPrefixed(field+op, vals)
	default:
		if !qb.hasTarget {
			return nil
		}
		for _, f := range core.FieldNames(qb.target) {
			cands = append(cands, neg+f+":")
		}
		ent := core.CfTask
		if qb.target == core.TargetProjects {
			ent = core.CfProject
		}
		if qb.target != core.TargetActivity {
			for _, cf := range r.CustomFieldsFor(ent) {
				cands = append(cands, neg+cf.QueryName()+":")
			}
			cands = append(cands, neg+"@me", neg+"is:overdue", neg+"is:open")
		}
	}
	full := neg + low
	var out []string
	seen := map[string]bool{}
	for _, c := range cands {
		lc := strings.ToLower(c)
		if strings.HasPrefix(lc, full) && lc != full && !seen[lc] {
			out = append(out, c)
			seen[lc] = true
		}
	}
	if len(out) > 40 {
		out = out[:40]
	}
	return out
}

// acceptSuggestion replaces the current token.
func (qb *queryBar) acceptSuggestion(s string) {
	_, start, end := qb.currentToken()
	t := qb.in.text
	nt := append(append(append([]rune{}, t[:start]...), []rune(s)...), t[end:]...)
	cur := start + len([]rune(s))
	if !strings.HasSuffix(s, ":") && (cur >= len(nt) || nt[cur] != ' ') {
		nt = append(nt[:cur], append([]rune{' '}, nt[cur:]...)...)
		cur++
	}
	qb.in.text, qb.in.cur = nt, cur
}
