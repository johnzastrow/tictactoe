package tui

// Dashboard (ui-spec §4.5): KPI tiles, status charts, hours by owner, a
// workload heatmap, an activity sparkline and at-risk projects. Arrows/Tab
// move between tiles; Enter drills into the matching filtered view.

import (
	"fmt"
	"math"
	"sort"
	"strings"

	tea "charm.land/bubbletea/v2"

	"git.wharf.fluidgrid.org/jcz/tictactoe/go/internal/pt/core"
)

type dashState struct {
	focus int
}

type tile struct {
	title string
	r     Rect
	drill func(a *App)
	draw  func(a *App, b *Buf, r Rect, st Sty)
}

// drillTo opens a view with a filter, sort and grouping.
func drillTo(view int, grid, query string, sort []core.SortSpec, group string) func(a *App) {
	return func(a *App) {
		a.setView(view)
		a.queries[view].in.SetText(query)
		if g := a.grids[grid]; g != nil {
			if sort != nil {
				g.Sort = sort
			}
			g.GroupBy = group
			g.Collapsed = map[string]bool{}
		}
		a.rebuild()
		a.setStatus("Drill-down: "+viewNames[view]+" "+query, 0)
	}
}

func (ds *dashState) tiles(a *App, r Rect) []tile {
	rep := a.rep
	var tiles []tile
	// KPIs
	active, overdueT, blockedT, dueWeek := 0, 0, 0, 0
	budget, actual := 0.0, 0.0
	eow, _ := core.ParseNatural("eow", rep.Today)
	for _, p := range rep.Projects {
		if p.Status == core.ProjectActive {
			active++
		}
		if rep.ProjectOpen(p) {
			if p.BudgetHours != nil {
				budget += *p.BudgetHours
			}
			actual += rep.Metrics(p.ID).ActualTotal
		}
	}
	for _, t := range rep.Tasks {
		if rep.TaskOverdue(t) {
			overdueT++
		}
		if rep.TaskBlocked(t) {
			blockedT++
		}
		if t.Status != core.TaskDone && t.DueDate != nil && eow != nil {
			if d, ok := core.ParseISO(*t.DueDate); ok && d >= rep.Today && d <= *eow {
				dueWeek++
			}
		}
	}
	type kpi struct {
		label, value, sub string
		color             Color
		drill             func(a *App)
	}
	th := a.th()
	burn := 0.0
	if budget > 0 {
		burn = actual * 100 / budget
	}
	kpis := []kpi{
		{"Active projects", fmt.Sprint(active), fmt.Sprintf("of %d", len(rep.Projects)), th.OK,
			drillTo(vProjects, "projects", "status:active", []core.SortSpec{{Key: "due_date"}}, "")},
		{"Overdue tasks", fmt.Sprint(overdueT), "past due, not done", th.Err,
			drillTo(vTasks, "tasks", "is:overdue", []core.SortSpec{{Key: "due_date"}}, "project")},
		{"Blocked tasks", fmt.Sprint(blockedT), "blocked or waiting", th.Warn,
			drillTo(vTasks, "tasks", "is:blocked", []core.SortSpec{{Key: "due_date"}}, "project")},
		{"Hours used", fmt.Sprintf("%.0f%%", burn), fmt.Sprintf("%s of %s h", core.FmtHours(math.Round(actual)), core.FmtHours(math.Round(budget))), th.Info,
			drillTo(vProjects, "projects", "is:open has:budget", []core.SortSpec{{Key: "burn", Desc: true}}, "")},
		{"Due this week", fmt.Sprint(dueWeek), "open tasks", th.Accent2,
			drillTo(vTasks, "tasks", "is:open due>=today due<=eow", []core.SortSpec{{Key: "due_date"}}, "")},
	}
	kw := r.W / len(kpis)
	for i, k := range kpis {
		k := k
		tiles = append(tiles, tile{title: k.label, r: Rect{r.X + i*kw, r.Y, kw - 1, 5}, drill: k.drill,
			draw: func(a *App, b *Buf, rr Rect, st Sty) {
				b.Text(rr.X+1, rr.Y, k.value, st.Fg(k.color).With(Bold), rr.W-2)
				if k.label == "Hours used" {
					lvl := 0
					if burn > 100 {
						lvl = 2
					} else if burn > 90 {
						lvl = 1
					}
					drawBar(b, rr.X+1, rr.Y+1, rr.W-2, burn, lvl, "", st, a.th())
					b.Text(rr.X+1, rr.Y+2, k.sub, st.Fg(a.th().Muted), rr.W-2)
					return
				}
				b.Text(rr.X+1, rr.Y+1, k.sub, st.Fg(a.th().Muted), rr.W-2)
			}})
	}
	rest := Rect{r.X, r.Y + 5, r.W, r.H - 5}
	ncol := 3
	if r.W < 120 {
		ncol = 2
	}
	cw := rest.W / ncol
	big := []tile{
		{title: "Projects by status", drill: drillTo(vProjects, "projects", "", []core.SortSpec{{Key: "code"}}, "status"), draw: ds.projectsByStatus},
		{title: "Tasks by status", drill: func(a *App) { a.setView(vBoard) }, draw: ds.tasksByStatus},
		{title: "Hours by owner: actual vs budget", drill: drillTo(vProjects, "projects", "is:open", []core.SortSpec{{Key: "burn", Desc: true}}, "owner_id"), draw: ds.hoursByOwner},
		{title: "Workload: open est. hours by week", drill: drillTo(vTasks, "tasks", "is:open due>=today", []core.SortSpec{{Key: "due_date"}}, "assignee_id"), draw: ds.heatmap},
		{title: "Activity (30 days)", drill: func(a *App) { a.setView(vActivity) }, draw: ds.activity},
		{title: "At-risk projects", drill: drillTo(vProjects, "projects", "is:open health:red,amber", []core.SortSpec{{Key: "burn", Desc: true}}, ""), draw: ds.atRisk},
	}
	nrow := (len(big) + ncol - 1) / ncol
	rh := rest.H / nrow
	for i := range big {
		cx, cy := i%ncol, i/ncol
		w := cw - 1
		if cx == ncol-1 {
			w = rest.W - cx*cw
		}
		big[i].r = Rect{rest.X + cx*cw, rest.Y + cy*rh, w, rh}
		if cy == nrow-1 {
			big[i].r.H = rest.Bottom() - big[i].r.Y
		}
	}
	return append(tiles, big...)
}

func (ds *dashState) draw(a *App, b *Buf, r Rect) []Hit {
	t := a.th()
	b.Fill(r, ' ', Sty{FG: t.FG, BG: t.BG})
	var hits []Hit
	tiles := ds.tiles(a, r)
	ds.focus = max(0, min(len(tiles)-1, ds.focus))
	for i, tl := range tiles {
		border := Sty{FG: t.Border, BG: t.Panel}
		title := Sty{FG: t.Muted, BG: t.Panel, A: Bold}
		if i == ds.focus {
			border = Sty{FG: t.Accent, BG: t.Panel}
			title = Sty{FG: t.Accent, BG: t.Panel, A: Bold}
		}
		b.Box(tl.r, border, Sty{FG: t.FG, BG: t.Panel}, tl.title, title)
		if tl.r.W > 4 && tl.r.H > 2 {
			inner := tl.r.Inset(1)
			old := b.Clip
			b.Clip = inner
			tl.draw(a, b, inner, Sty{FG: t.FG, BG: t.Panel})
			b.Clip = old
		}
		if i == ds.focus {
			hint := " Enter ▸ "
			b.Text(tl.r.Right()-StrW(hint)-1, tl.r.Bottom()-1, hint, Sty{FG: t.Accent, BG: t.Panel}, -1)
		}
		hits = append(hits, Hit{R: tl.r, Kind: HitTile, A: i})
	}
	return hits
}

func (ds *dashState) projectsByStatus(a *App, b *Buf, r Rect, st Sty) {
	t := a.th()
	counts := map[core.ProjectStatus]int{}
	for _, p := range a.rep.Projects {
		counts[p.Status]++
	}
	mx := 1
	for _, n := range counts {
		mx = max(mx, n)
	}
	for i, s := range core.AllProjectStatuses {
		if i >= r.H {
			break
		}
		y := r.Y + i
		b.Fit(r.X+1, y, s.Label(), st, 11, 0)
		bw := r.W - 18
		n := counts[s]
		w := int(math.Round(float64(n) / float64(mx) * float64(bw)))
		for x := 0; x < w; x++ {
			b.Set(r.X+13+x, y, '█', st.Fg(t.projectStatus(s)))
		}
		b.Text(r.X+14+w, y, fmt.Sprint(n), st.Fg(t.Muted), 4)
	}
}

func (ds *dashState) tasksByStatus(a *App, b *Buf, r Rect, st Sty) {
	t := a.th()
	counts := map[core.TaskStatus]int{}
	for _, tk := range a.rep.Tasks {
		counts[tk.Status]++
	}
	total := max(1, len(a.rep.Tasks))
	x := r.X + 1
	w := r.W - 2
	used := 0
	for i, s := range core.AllTaskStatuses {
		n := counts[s]
		sw := int(math.Round(float64(n) / float64(total) * float64(w)))
		if i == len(core.AllTaskStatuses)-1 {
			sw = w - used
		}
		for j := 0; j < sw; j++ {
			b.Set(x+used+j, r.Y, '█', st.Fg(t.taskStatus(s)))
			if r.H > 4 {
				b.Set(x+used+j, r.Y+1, '█', st.Fg(t.taskStatus(s)))
			}
		}
		used += sw
	}
	y := r.Y + 2
	if r.H <= 4 {
		y = r.Y + 1
	}
	ncol := 2
	if r.W < 44 {
		ncol = 1
	}
	for i, s := range core.AllTaskStatuses {
		col := i % ncol
		row := i / ncol
		yy := y + row
		if yy >= r.Bottom() {
			break
		}
		xx := r.X + 1 + col*(r.W/ncol)
		b.Text(xx, yy, taskGlyph(s)+s.Label(), st.Fg(t.taskStatus(s)), 14)
		pct := float64(counts[s]) * 100 / float64(total)
		b.Text(xx+15, yy, fmt.Sprintf("%4d %3.0f%%", counts[s], pct), st.Fg(t.Muted), 10)
	}
}

func (ds *dashState) hoursByOwner(a *App, b *Buf, r Rect, st Sty) {
	t := a.th()
	rep := a.rep
	type agg struct {
		id             int64
		budget, actual float64
	}
	m := map[int64]*agg{}
	for _, p := range rep.Projects {
		if !rep.ProjectOpen(p) || p.OwnerID == nil {
			continue
		}
		g := m[*p.OwnerID]
		if g == nil {
			g = &agg{id: *p.OwnerID}
			m[*p.OwnerID] = g
		}
		if p.BudgetHours != nil {
			g.budget += *p.BudgetHours
		}
		g.actual += rep.Metrics(p.ID).ActualTotal
	}
	var list []*agg
	mx := 1.0
	for _, g := range m {
		list = append(list, g)
		mx = math.Max(mx, math.Max(g.budget, g.actual))
	}
	sort.Slice(list, func(i, j int) bool { return list[i].budget > list[j].budget })
	bw := r.W - 24
	for i, g := range list {
		if i >= r.H {
			break
		}
		y := r.Y + i
		name := rep.UserName(&g.id)
		b.Fit(r.X+1, y, name, st.Fg(t.userColor(g.id)), 12, 0)
		bl := int(g.budget / mx * float64(bw))
		al := int(g.actual / mx * float64(bw))
		for x := 0; x < max(bl, al); x++ {
			ch, s := '░', st.Fg(t.Muted)
			if x < al {
				ch = '█'
				s = st.Fg(t.Info)
				if x >= bl {
					s = st.Fg(t.Err)
				}
			}
			b.Set(r.X+14+x, y, ch, s)
		}
		b.Text(r.X+15+max(bl, al), y, fmt.Sprintf("%s/%s", core.FmtHours(math.Round(g.actual)), core.FmtHours(g.budget)), st.Fg(t.Muted), 12)
	}
}

func (ds *dashState) heatmap(a *App, b *Buf, r Rect, st Sty) {
	t := a.th()
	rep := a.rep
	weeks := 8
	start := rep.Today.WeekStart()
	load := map[int64]*[8]float64{}
	for _, tk := range rep.Tasks {
		if tk.Status == core.TaskDone || tk.DueDate == nil || tk.AssigneeID == nil {
			continue
		}
		d, ok := core.ParseISO(*tk.DueDate)
		if !ok || d < start {
			continue
		}
		w := (int(d) - int(start)) / 7
		if w >= weeks {
			continue
		}
		l := load[*tk.AssigneeID]
		if l == nil {
			l = &[8]float64{}
			load[*tk.AssigneeID] = l
		}
		if tk.EstHours != nil {
			l[w] += math.Max(0, *tk.EstHours-tk.ActualHours)
		}
	}
	var ids []int64
	for id := range load {
		ids = append(ids, id)
	}
	sort.Slice(ids, func(i, j int) bool { return rep.UserName(&ids[i]) < rep.UserName(&ids[j]) })
	cw := max(3, min(7, (r.W-14)/weeks))
	for w := 0; w < weeks; w++ {
		d := start.AddDays(w * 7)
		lbl := fmt.Sprintf("%s%d", core.MonthNames[d.Month()-1][:1], d.Day())
		if cw >= 6 {
			lbl = fmt.Sprintf("%s %d", core.MonthNames[d.Month()-1], d.Day())
		}
		b.Fit(r.X+13+w*cw, r.Y, lbl, st.Fg(t.Muted), cw, 0)
	}
	shade := func(h float64) (rune, Color) {
		switch {
		case h <= 0:
			return '·', t.Muted
		case h < 8:
			return '░', t.OK
		case h < 20:
			return '▒', t.OK
		case h < 40:
			return '▓', t.Warn
		}
		return '█', t.Err
	}
	for i, id := range ids {
		y := r.Y + 1 + i
		if y >= r.Bottom() {
			break
		}
		uid := id
		b.Fit(r.X+1, y, rep.UserName(&uid), st.Fg(t.userColor(id)), 11, 0)
		for w := 0; w < weeks; w++ {
			ch, c := shade(load[id][w])
			for x := 0; x < cw-1; x++ {
				b.Set(r.X+13+w*cw+x, y, ch, st.Fg(c))
			}
		}
	}
}

func (ds *dashState) activity(a *App, b *Buf, r Rect, st Sty) {
	t := a.th()
	rep := a.rep
	counts := make([]int, 30)
	start := rep.Today.AddDays(-29)
	for _, ac := range rep.Activity {
		if len(ac.TS) < 10 {
			continue
		}
		d, ok := core.ParseISO(ac.TS[:10])
		if !ok || d < start || d > rep.Today {
			continue
		}
		counts[int(d)-int(start)]++
	}
	mx := 1
	for _, c := range counts {
		mx = max(mx, c)
	}
	bars := []rune("▁▂▃▄▅▆▇█")
	w := min(30, r.W-2)
	off := 30 - w
	for i := 0; i < w; i++ {
		c := counts[off+i]
		idx := int(math.Round(float64(c) / float64(mx) * 7))
		ch := bars[idx]
		if c == 0 {
			ch = ' '
		}
		b.Set(r.X+1+i, r.Y, ch, st.Fg(t.Accent))
	}
	total := 0
	for _, c := range counts {
		total += c
	}
	b.Text(r.X+w+2, r.Y, fmt.Sprintf("%d changes", total), st.Fg(t.Muted), r.W-w-3)
	ids := core.SortedIDs(rep.Activity)
	y := r.Y + 1
	for i := len(ids) - 1; i >= 0 && y < r.Bottom(); i-- {
		ac := rep.Activity[ids[i]]
		who := rep.Username(ac.UserID)
		line := fmt.Sprintf("%-7s %s", who, ac.Label)
		s := st
		if ac.State == core.StateUndone {
			s = s.With(Strike).Fg(t.Muted)
		}
		b.Text(r.X+1, y, line, s.Fg(t.userColor(ptrVal(ac.UserID))), 8)
		b.Text(r.X+9, y, ac.Label, s, r.W-10)
		y++
	}
}

func (ds *dashState) atRisk(a *App, b *Buf, r Rect, st Sty) {
	t := a.th()
	rep := a.rep
	type risk struct {
		p    *core.Project
		burn float64
	}
	var list []risk
	for _, p := range rep.Projects {
		if !rep.ProjectOpen(p) {
			continue
		}
		burn, _ := rep.BurnPct(p)
		if p.Health != core.HealthGreen || burn > 90 {
			list = append(list, risk{p, burn})
		}
	}
	sort.Slice(list, func(i, j int) bool {
		hi, hj := list[i].p.Health.Index(), list[j].p.Health.Index()
		if hi != hj {
			return hi > hj
		}
		return list[i].burn > list[j].burn
	})
	for i, x := range list {
		if i >= r.H || i >= 10 {
			break
		}
		y := r.Y + i
		b.Text(r.X+1, y, "●", st.Fg(t.health(x.p.Health)), 1)
		b.Fit(r.X+3, y, x.p.Code, st.With(Bold), 8, 0)
		nameW := max(4, r.W-24)
		b.Fit(r.X+12, y, x.p.Name, st, nameW, 0)
		lvl := 0
		if x.burn > 100 {
			lvl = 2
		} else if x.burn > 90 {
			lvl = 1
		}
		c := []Color{t.Muted, t.Warn, t.Err}[lvl]
		b.Fit(r.Right()-9, y, fmt.Sprintf("%.0f%%", x.burn), st.Fg(c), 8, 1)
	}
	if len(list) == 0 {
		b.Text(r.X+1, r.Y, "Nothing at risk 🎉", st.Fg(t.OK), r.W-2)
	}
}

func (ds *dashState) key(a *App, k tea.KeyPressMsg) tea.Cmd {
	n := len(ds.tiles(a, a.layout().main))
	switch k.Keystroke() {
	case "left", "up", "shift+tab":
		ds.focus = (ds.focus + n - 1) % n
	case "right", "down", "tab":
		ds.focus = (ds.focus + 1) % n
	case "home":
		ds.focus = 0
	case "end":
		ds.focus = n - 1
	}
	return nil
}

func (ds *dashState) action(a *App, ac Action) tea.Cmd {
	if ac == ADrill {
		tiles := ds.tiles(a, a.layout().main)
		if ds.focus < len(tiles) && tiles[ds.focus].drill != nil {
			tiles[ds.focus].drill(a)
		}
	}
	return nil
}

func (ds *dashState) click(a *App, h Hit, dbl bool) tea.Cmd {
	if h.Kind == HitTile {
		ds.focus = h.A
		if dbl {
			return ds.action(a, ADrill)
		}
	}
	return nil
}

var _ = strings.TrimSpace
