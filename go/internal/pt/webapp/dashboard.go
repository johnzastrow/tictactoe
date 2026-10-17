//go:build js && wasm

package webapp

import (
	"fmt"
	"math"
	"sort"
	"strconv"
	"syscall/js"

	"git.wharf.fluidgrid.org/jcz/tictactoe/go/internal/pt/core"
)

type drill struct {
	view  int
	query string
}

type dashboardView struct {
	app    *App
	root   js.Value
	sig    string
	drills []drill
	sel    int
	scope  Scope
}

func newDashboardView(a *App) *dashboardView { return &dashboardView{app: a} }

func (v *dashboardView) ID() string                    { return "dashboard" }
func (v *dashboardView) Target() (core.Target, bool)    { return 0, false }
func (v *dashboardView) Query() string                  { return "" }
func (v *dashboardView) QueryErrors() []core.QueryError { return nil }
func (v *dashboardView) SetQuery(string)                {}
func (v *dashboardView) DataChanged(bool)               { v.sig = "" }
func (v *dashboardView) PresenceChanged()               {}
func (v *dashboardView) ActiveGrid() *Grid              { return nil }
func (v *dashboardView) Count() (int, int)              { return 0, 0 }
func (v *dashboardView) Indicators() string             { return "" }
func (v *dashboardView) NextPane(d int)                 { v.moveSel(d) }
func (v *dashboardView) ToggleDetail()                  {}
func (v *dashboardView) New()                           { v.app.OpenDialog(newProjectDialog(v.app)) }
func (v *dashboardView) Focus() (core.Entity, int64, string) { return "", 0, "" }
func (v *dashboardView) FocusDOM()                      { v.root.Call("focus", map[string]any{"preventScroll": true}) }

func (v *dashboardView) Hints() []hint {
	return []hint{{"←→↑↓/Tab", "Move between tiles", ""}, {"Enter", "Drill down", ""}, {"click", "Drill down", ""}}
}

func (v *dashboardView) Mount(parent js.Value) {
	v.scope.Release()
	v.root = E("div", "dash")
	Attr(v.root, "tabindex", "0")
	Add(parent, v.root)
	v.sig = ""
	v.scope.On(v.root, "click", func(ev js.Value) {
		if d := Closest(ev.Get("target"), "[data-drill]"); d.Truthy() {
			i, _ := strconv.Atoi(DataOf(d, "drill"))
			v.go_(i)
		}
	})
}

func (v *dashboardView) Unmount() { v.scope.Release() }

func (v *dashboardView) go_(i int) {
	if i < 0 || i >= len(v.drills) {
		return
	}
	d := v.drills[i]
	a := v.app
	a.views[d.view].SetQuery(d.query)
	a.SwitchView(d.view)
	a.syncQueryInput()
	a.setStatus("info", "Drill-down: "+viewNames[d.view]+" ▸ "+d.query)
}

func (v *dashboardView) moveSel(d int) {
	if len(v.drills) == 0 {
		return
	}
	v.sel = (v.sel + d + len(v.drills)) % len(v.drills)
	v.sig = ""
}

func (v *dashboardView) Key(k Key) bool {
	if !v.root.Equal(doc.Get("activeElement")) {
		return false
	}
	switch {
	case k.Is("ArrowRight", "ArrowDown", "Tab"):
		v.moveSel(1)
	case k.Is("ArrowLeft", "ArrowUp", "S-Tab"):
		v.moveSel(-1)
	case k.Is("Enter", " "):
		v.go_(v.sel)
	default:
		return false
	}
	return true
}

// drillEl makes an element a drill-down target.
func (v *dashboardView) drillEl(el js.Value, view int, q string) js.Value {
	i := len(v.drills)
	v.drills = append(v.drills, drill{view, q})
	Data(el, "drill", strconv.Itoa(i))
	cls := el.Get("className").String() + " drill"
	if i == v.sel {
		cls += " sel"
	}
	el.Set("className", cls)
	Attr(el, "title", viewNames[view]+" ▸ "+q)
	return el
}

func (v *dashboardView) Render() {
	a := v.app
	sig := fmt.Sprint(a.r.Version, v.sel, a.theme)
	if sig == v.sig {
		return
	}
	v.sig = sig
	r := a.r
	v.drills = v.drills[:0]
	Clear(v.root)

	// ---- KPIs ----
	active, overdue, blocked, dueWeek := 0, 0, 0, 0
	budget, actual := 0.0, 0.0
	for _, p := range r.Projects {
		if p.Status == core.ProjectActive {
			active++
		}
		if r.ProjectOpen(p) {
			if p.BudgetHours != nil {
				budget += *p.BudgetHours
			}
			actual += r.Metrics(p.ID).ActualTotal
		}
	}
	weekEnd := r.Today.AddDays(7)
	for _, t := range r.Tasks {
		if r.TaskOverdue(t) {
			overdue++
		}
		if r.TaskBlocked(t) {
			blocked++
		}
		if d := parseDP(t.DueDate); d != nil && t.Status != core.TaskDone && *d >= r.Today && *d <= weekEnd {
			dueWeek++
		}
	}
	kpis := E("div", "kpis")
	kpi := func(label, value, sub, cls string, view int, q string) {
		el := E("div", "tile kpi "+cls)
		Add(el, ET("div", "kl", label), ET("div", "kv", value), ET("div", "ks muted", sub))
		Add(kpis, v.drillEl(el, view, q))
	}
	kpi("Active projects", strconv.Itoa(active), fmt.Sprintf("of %d", len(r.Projects)), "", 0, "status:active")
	kpi("Overdue tasks", strconv.Itoa(overdue), "past due, not done", "bad", 1, "is:overdue")
	kpi("Blocked tasks", strconv.Itoa(blocked), "blocked or waiting on deps", "warn", 1, "is:blocked")
	pct := 0.0
	if budget > 0 {
		pct = actual * 100 / budget
	}
	g := E("div", "tile kpi gauge")
	Add(g, ET("div", "kl", "Hours used / budget"), ET("div", "kv", fmt.Sprintf("%.0f%%", pct)),
		ET("div", "ks muted", fmt.Sprintf("%s of %s h (open projects)", core.FmtHours(math.Round(actual)), core.FmtHours(budget))))
	bar := E("div", "bar wide")
	fill := E("span", "fill")
	if pct > 100 {
		fill.Set("className", "fill l2")
	} else if pct > 90 {
		fill.Set("className", "fill l1")
	}
	Style(fill, "width", fmt.Sprintf("%.0f%%", math.Min(pct, 100)))
	Add(bar, fill)
	Add(g, bar)
	Add(kpis, v.drillEl(g, 0, "is:open budget>0"))
	kpi("Due this week", strconv.Itoa(dueWeek), "open tasks due in 7 days", "", 1, "is:open due>=today due<=+7d")
	Add(v.root, kpis)

	tiles := E("div", "tiles")
	Add(v.root, tiles)
	tile := func(title, cls string) js.Value {
		t := E("div", "tile "+cls)
		Add(t, ET("div", "tt", title))
		Add(tiles, t)
		return t
	}

	// ---- projects by status ----
	pt := tile("Projects by status", "")
	pc := map[core.ProjectStatus]int{}
	for _, p := range r.Projects {
		pc[p.Status]++
	}
	maxN := 1
	for _, n := range pc {
		maxN = max(maxN, n)
	}
	for _, s := range core.AllProjectStatuses {
		row := E("div", "hbar")
		fillEl := E("span", "hfill ps-"+string(s))
		Style(fillEl, "width", fmt.Sprintf("%.1f%%", float64(pc[s])*100/float64(maxN)))
		track := E("span", "htrack")
		Add(track, fillEl)
		Add(row, ET("span", "hlab", s.Label()), track, ET("span", "hn", strconv.Itoa(pc[s])))
		Add(pt, v.drillEl(row, 0, "status:"+string(s)))
	}

	// ---- tasks by status (stacked) ----
	tt := tile("Tasks by status", "")
	tc := map[core.TaskStatus]int{}
	for _, t := range r.Tasks {
		tc[t.Status]++
	}
	stack := E("div", "stack")
	legend := E("div", "legend")
	for _, s := range core.AllTaskStatuses {
		seg := E("span", "seg ts-"+string(s))
		Style(seg, "flex", strconv.Itoa(max(tc[s], 0)))
		Attr(seg, "title", fmt.Sprintf("%s: %d", s.Label(), tc[s]))
		Add(stack, v.drillEl(seg, 1, "status:"+string(s)))
		li := E("span", "li")
		Add(li, E("span", "dot ts-"+string(s)), Txt(fmt.Sprintf("%s %d", s.Label(), tc[s])))
		Add(legend, li)
	}
	Add(tt, stack, legend)

	// ---- hours by owner: budget vs actual ----
	ht := tile("Hours by owner — budget vs actual", "wide")
	type ownerH struct {
		id             int64
		budget, actual float64
	}
	byOwner := map[int64]*ownerH{}
	for _, p := range r.Projects {
		if p.OwnerID == nil || !r.ProjectOpen(p) {
			continue
		}
		o := byOwner[*p.OwnerID]
		if o == nil {
			o = &ownerH{id: *p.OwnerID}
			byOwner[*p.OwnerID] = o
		}
		if p.BudgetHours != nil {
			o.budget += *p.BudgetHours
		}
		o.actual += r.Metrics(p.ID).ActualTotal
	}
	var owners []*ownerH
	maxH := 1.0
	for _, o := range byOwner {
		owners = append(owners, o)
		maxH = math.Max(maxH, math.Max(o.budget, o.actual))
	}
	sort.Slice(owners, func(i, j int) bool { return owners[i].budget > owners[j].budget })
	for _, o := range owners {
		row := E("div", "pair")
		u := r.Users[o.id]
		name := "?"
		if u != nil {
			name = u.DisplayName
		}
		bars := E("span", "pbars")
		b1 := E("span", "pb budget")
		Style(b1, "width", fmt.Sprintf("%.1f%%", o.budget*100/maxH))
		b2 := E("span", "pb actual")
		if o.actual > o.budget {
			b2.Set("className", "pb actual over")
		}
		Style(b2, "width", fmt.Sprintf("%.1f%%", o.actual*100/maxH))
		Add(bars, b1, b2)
		Add(row, ET("span", "hlab", name), bars, ET("span", "hn", fmt.Sprintf("%s / %s h", core.FmtHours(math.Round(o.actual)), core.FmtHours(o.budget))))
		if u != nil {
			v.drillEl(row, 0, "is:open owner:"+u.Username)
		}
		Add(ht, row)
	}
	lg := E("div", "legend")
	Add(lg, ET("span", "li", "▮ budget"), ET("span", "li c-ok", "▮ actual"))
	Add(ht, lg)

	// ---- workload heatmap: assignee × next 8 weeks ----
	hm := tile("Workload — open estimated hours by week due", "wide")
	ws := r.Today.WeekStart()
	load := map[int64]*[8]float64{}
	for _, t := range r.Tasks {
		if t.Status == core.TaskDone || t.AssigneeID == nil || t.EstHours == nil {
			continue
		}
		d := parseDP(t.DueDate)
		if d == nil {
			continue
		}
		w := (int(*d) - int(ws)) / 7
		if *d < ws {
			w = 0 // overdue work lands in this week
		}
		if w < 0 || w >= 8 {
			continue
		}
		l := load[*t.AssigneeID]
		if l == nil {
			l = &[8]float64{}
			load[*t.AssigneeID] = l
		}
		l[w] += *t.EstHours
	}
	maxCell := 1.0
	for _, l := range load {
		for _, h := range l {
			maxCell = math.Max(maxCell, h)
		}
	}
	table := E("div", "heat")
	Add(table, E("span", "hh"))
	for w := 0; w < 8; w++ {
		d := ws.AddDays(w * 7)
		Add(table, ET("span", "hh", d.Short(r.Today.Year())))
	}
	uids := make([]int64, 0, len(load))
	for id := range load {
		uids = append(uids, id)
	}
	sort.Slice(uids, func(i, j int) bool { return r.UserName(&uids[i]) < r.UserName(&uids[j]) })
	for _, id := range uids {
		id := id
		Add(table, ET("span", "hu", r.UserName(&id)))
		for w := 0; w < 8; w++ {
			h := load[id][w]
			c := ET("span", "hc", "")
			if h > 0 {
				c.Set("textContent", core.FmtHours(math.Round(h)))
			}
			// shade relative to the busiest cell; flag weeks far over capacity
			Style(c, "--i", fmt.Sprintf("%.2f", math.Min(1, h/maxCell)))
			if h > 160 {
				c.Set("className", "hc over")
				Attr(c, "title", "more than 4 person-weeks of estimated work due")
			}
			from := ws.AddDays(w * 7)
			q := fmt.Sprintf("is:open @%s due>=%s due<=%s", r.Username(&id), from.ISO(), from.AddDays(6).ISO())
			if w == 0 {
				q = fmt.Sprintf("is:open @%s due<=%s", r.Username(&id), from.AddDays(6).ISO())
			}
			Add(table, v.drillEl(c, 1, q))
		}
	}
	Add(hm, table)

	// ---- activity sparkline ----
	at := tile("Activity — changes per day (30 days)", "")
	counts := make([]int, 30)
	now := nowSecs()
	for _, act := range r.Activity {
		ts, ok := core.ParseRFC3339(act.TS)
		if !ok {
			continue
		}
		d := int((now - ts) / 86400)
		if d >= 0 && d < 30 {
			counts[29-d]++
		}
	}
	mx := 1
	for _, c := range counts {
		mx = max(mx, c)
	}
	const sw, sh = 300.0, 60.0
	svg := S("svg", "width", fmt.Sprint(sw), "height", fmt.Sprint(sh+4), "class", "spark")
	pts := ""
	for i, c := range counts {
		x := float64(i) * sw / 29
		y := sh - float64(c)*sh/float64(mx) + 2
		pts += fmt.Sprintf("%.1f,%.1f ", x, y)
	}
	Add(svg, S("polyline", "points", fmt.Sprintf("0,%.1f %s%.1f,%.1f", sh+2, pts, sw, sh+2), "class", "sparkfill"))
	Add(svg, S("polyline", "points", pts, "class", "sparkline"))
	total := 0
	for _, c := range counts {
		total += c
	}
	Add(at, svg, ET("div", "muted", fmt.Sprintf("%d changes · busiest day %d · today %d", total, mx, counts[29])))
	ids := core.SortedIDs(r.Activity)
	for i := len(ids) - 1; i >= 0 && i >= len(ids)-8; i-- {
		act := r.Activity[ids[i]]
		line := E("div", "actline")
		Add(line, ET("span", "muted", core.RelativeTime(act.TS, now)+" · "+r.Username(act.UserID)+" "), Txt(truncate(act.Label, 70)))
		Add(at, line)
	}
	v.drillEl(at, 5, "")

	// ---- at-risk projects ----
	rt := tile("Top 10 at-risk projects", "wide")
	type risk struct {
		p     *core.Project
		score float64
	}
	var risks []risk
	for _, p := range r.Projects {
		if !r.ProjectOpen(p) {
			continue
		}
		burn, _ := r.BurnPct(p)
		if p.Health == core.HealthGreen && burn <= 90 {
			continue
		}
		s := burn / 10
		if p.Health == core.HealthRed {
			s += 20
		} else if p.Health == core.HealthAmber {
			s += 10
		}
		s += float64(r.Metrics(p.ID).TasksOverdue) * 2
		risks = append(risks, risk{p, s})
	}
	sort.Slice(risks, func(i, j int) bool { return risks[i].score > risks[j].score })
	rtab := E("div", "risk")
	for _, h := range []string{"Project", "Owner", "Health", "Burn", "Late", "Due"} {
		Add(rtab, ET("span", "rh", h))
	}
	for i, rk := range risks {
		if i >= 10 {
			break
		}
		p := rk.p
		burn, ok := r.BurnPct(p)
		bs := "—"
		bcls := "rc"
		if ok {
			bs = fmt.Sprintf("%.0f%%", burn)
			if burn > 100 {
				bcls += " c-err"
			} else if burn > 90 {
				bcls += " c-warn"
			}
		}
		cells := []js.Value{
			ET("span", "rc strong", p.Code+" "+truncate(p.Name, 24)),
			ET("span", "rc", r.UserName(p.OwnerID)),
			ET("span", "rc health h-"+string(p.Health), p.Health.Label()),
			ET("span", bcls, bs),
			ET("span", "rc", strconv.Itoa(r.Metrics(p.ID).TasksOverdue)),
			ET("span", "rc", deref(p.DueDate)),
		}
		row := E("div", "rrow")
		Add(row, cells...)
		Add(rtab, v.drillEl(row, 1, "project:"+p.Code+" is:open"))
	}
	if len(risks) == 0 {
		Add(rtab, ET("span", "muted", "Nothing at risk 🎉"))
	}
	Add(rt, rtab)
	if v.sel >= len(v.drills) {
		v.sel = 0
	}
	if s := v.root.Call("querySelector", ".drill.sel"); s.Truthy() {
		s.Call("scrollIntoView", map[string]any{"block": "nearest"})
	}
}
