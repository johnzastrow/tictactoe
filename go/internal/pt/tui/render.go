package tui

// frame draws the whole screen into a buffer and returns the hit regions.
// It only reads state, so View and mouse handling can both call it.

import (
	"fmt"
	"strings"

	"git.wharf.fluidgrid.org/jcz/tictactoe/go/internal/pt/core"
)

const minW, minH = 80, 24

func (a *App) th() *Theme { return &themes[a.theme] }

type layoutT struct {
	menu, tabs, query, main, detail, status, hints Rect
	grids                                          map[string]Rect
	detailRight                                    bool
}

func (a *App) hintLines() int {
	if a.h >= 32 {
		return 2
	}
	return 1
}

func (a *App) showQuery() bool { return a.view != vDashboard }

// layout is a pure function of the window size and view state.
func (a *App) layout() layoutT {
	w, h := a.w, a.h
	l := layoutT{grids: map[string]Rect{}}
	l.menu = Rect{0, 0, w, 1}
	l.tabs = Rect{0, 1, w, 1}
	y := 2
	if a.showQuery() {
		l.query = Rect{0, 2, w, 1}
		y = 3
	}
	hl := a.hintLines()
	l.hints = Rect{0, h - hl, w, hl}
	l.status = Rect{0, h - hl - 1, w, 1}
	body := Rect{0, y, w, max(1, h-hl-1-y)}
	l.main = body
	split := func(frac float64, minMain int) {
		if w >= 160 {
			mw := int(float64(w) * (1 - frac))
			l.main = Rect{0, body.Y, mw, body.H}
			l.detail = Rect{mw, body.Y, w - mw, body.H}
			l.detailRight = true
		} else {
			dh := max(5, int(float64(body.H)*frac))
			if body.H-dh < minMain {
				dh = max(0, body.H-minMain)
			}
			l.main = Rect{0, body.Y, w, body.H - dh}
			l.detail = Rect{0, body.Y + body.H - dh, w, dh}
		}
	}
	switch a.view {
	case vProjects:
		if a.detail[vProjects] {
			split(0.42, 6)
			l.grids["projtasks"] = l.detail.paneInner()
		}
		l.grids["projects"] = l.main
	case vTasks:
		if a.detail[vTasks] {
			if w >= 160 {
				split(0.30, 6)
			} else {
				split(0.30, 8)
			}
		}
		l.grids["tasks"] = l.main
	case vActivity:
		l.grids["activity"] = l.main
	case vTemplates:
		th := max(5, body.H*2/5)
		l.main = Rect{0, body.Y, w, th}
		l.grids["templates"] = l.main
		if a.detail[vTemplates] {
			l.detail = Rect{0, body.Y + th, w, body.H - th}
			// the grid gets what it needs; the schedule preview the rest
			n := len(a.rep.TemplateTasksOf(a.grids["templates"].CurID()))
			gh := max(4, min(n+2, l.detail.H-1-min(n+1, (l.detail.H-1)/2)))
			l.grids["tmpltasks"] = Rect{0, l.detail.Y + 1, w, gh}
		}
	case vFields:
		l.grids["fields"] = l.main
	case vUsers:
		uh := max(5, body.H*11/20)
		l.main = Rect{0, body.Y, w, uh}
		l.detail = Rect{0, body.Y + uh, w, body.H - uh}
		l.grids["users"] = l.main
		l.grids["programs"] = l.detail.paneInner()
	}
	return l
}

// paneInner leaves a title line at the top of a detail pane.
func (r Rect) paneInner() Rect { return Rect{r.X, r.Y + 1, r.W, max(1, r.H-1)} }

func (a *App) frame() (*Buf, []Hit) {
	t := a.th()
	w, h := max(1, a.w), max(1, a.h)
	b := NewBuf(w, h, t.base())
	var hits []Hit
	if a.w < minW || a.h < minH {
		msg := fmt.Sprintf("pt needs at least %d×%d — this window is %d×%d. Make it bigger.", minW, minH, a.w, a.h)
		for i, l := range wrap(msg, max(10, w-2)) {
			b.Text(1, h/2-1+i, l, t.base().Fg(t.Warn).With(Bold), w-2)
		}
		return b, nil
	}
	switch a.ph {
	case phLogin:
		a.drawSplash(b)
		hits = append(hits, a.login.draw(a, b)...)
		if a.loginErr != "" {
			r := a.login.rect
			msg := " " + a.loginErr + " "
			b.Fit(r.X+2, r.Y+r.H, msg, Sty{FG: t.CellFG, BG: t.Err, A: Bold}, min(r.W-4, StrW(msg)), 0)
		}
		return b, hits
	case phConnecting:
		a.drawSplash(b)
		msg := "Connecting to " + a.opts.Server + " …"
		if a.conn == connReconnecting && a.connErr != "" {
			msg = "Can't reach " + a.opts.Server + " (" + a.connErr + "); retrying…  ^Q quits"
		}
		b.Fit(0, h/2+3, msg, t.base().Fg(t.Muted), w, 2)
		return b, nil
	}
	lay := a.layout()
	hits = append(hits, a.drawMenuBar(b, lay.menu)...)
	hits = append(hits, a.drawTabs(b, lay.tabs)...)
	if a.showQuery() {
		hits = append(hits, a.drawQuery(b, lay.query)...)
	}
	hits = append(hits, a.drawView(b, lay)...)
	a.drawStatus(b, lay.status)
	hits = append(hits, a.drawHints(b, lay.hints)...)
	if a.filterOn && a.showQuery() {
		hits = append(hits, a.drawQuerySuggestions(b, lay.query)...)
	}
	if a.editor != nil {
		hits = append(hits, a.drawEditor(b)...)
	}
	for _, o := range a.overlays {
		hits = append(hits, o.draw(a, b)...)
	}
	hits = append(hits, a.drawToasts(b)...)
	return b, hits
}

func (a *App) drawSplash(b *Buf) {
	t := a.th()
	logo := []string{
		"██████╗ ████████╗",
		"██╔══██╗╚══██╔══╝",
		"██████╔╝   ██║   ",
		"██╔═══╝    ██║   ",
		"██║        ██║   ",
		"╚═╝        ╚═╝   ",
	}
	y := max(0, b.H/2-12)
	for i, l := range logo {
		c := mix(t.Accent, t.Accent2, float64(i)/5)
		b.Fit(0, y+i, l, Sty{FG: c, BG: t.BG, A: Bold}, b.W, 2)
	}
	b.Fit(0, y+len(logo)+1, "projects & tasks · Go TUI", t.base().Fg(t.Muted), b.W, 2)
}

func (a *App) drawMenuBar(b *Buf, r Rect) []Hit {
	t := a.th()
	st := Sty{FG: t.FG, BG: t.Panel2}
	b.Fill(r, ' ', st)
	var hits []Hit
	open := -1
	if l, ok := a.top().(*listOverlay); ok && l.menuBar >= 0 {
		open = l.menuBar
	}
	for i, x := range menuTitleX() {
		m := menuBar[i]
		s := st
		if i == open {
			s = Sty{FG: t.CellFG, BG: t.CellBG}
		}
		lbl := " " + m.title + " "
		b.Text(x-1, r.Y, lbl, s, -1)
		b.Set(x, r.Y, []rune(m.title)[0], s.With(Underline).Fg(choose(i == open, t.CellFG, t.Accent)))
		hits = append(hits, Hit{R: Rect{x - 1, r.Y, StrW(lbl), 1}, Kind: HitMenuTitle, A: i})
	}
	// right side: who, where
	r2 := a.rep
	who := ""
	if r2.Me != nil {
		who = fmt.Sprintf("%s (%s)", r2.Me.Username, strings.ToLower(r2.Me.Role.Label()))
	}
	dot, dc := "●", t.OK
	switch a.conn {
	case connReconnecting:
		dc = t.Warn
	case connConnecting:
		dc = t.Muted
	}
	srv := ""
	if r2.Server != nil {
		srv = r2.Server.Impl + " " + r2.Server.Version
	}
	right := fmt.Sprintf("pt · %s · ", who)
	rw := StrW(right) + 2 + StrW(srv) + 1
	x := r.Right() - rw
	if x > 40 {
		b.Text(x, r.Y, right, st.Fg(t.Muted), -1)
		b.Text(x+StrW(right), r.Y, dot, st.Fg(dc).With(Bold), 1)
		b.Text(x+StrW(right)+2, r.Y, srv, st.Fg(t.Muted), -1)
	}
	return hits
}

func choose[T any](c bool, a, b T) T {
	if c {
		return a
	}
	return b
}

func (a *App) drawTabs(b *Buf, r Rect) []Hit {
	t := a.th()
	st := Sty{FG: t.Muted, BG: t.BG}
	b.Fill(r, ' ', st)
	var hits []Hit
	// pick the most descriptive labels that fit: full, short, numbers
	short := []string{"Proj", "Task", "Board", "Gantt", "Dash", "Log", "Tmpl", "Fld", "User"}
	label := func(i, mode int) string {
		switch {
		case mode == 0 || i == a.view:
			return fmt.Sprintf(" %d %s ", i+1, viewNames[i])
		case mode == 1:
			return fmt.Sprintf(" %d %s ", i+1, short[i])
		}
		return fmt.Sprintf(" %d ", i+1)
	}
	mode := 0
	for ; mode < 2; mode++ {
		w, gap := 1, 1
		if mode > 0 {
			gap = 0
		}
		for i := range viewNames {
			w += StrW(label(i, mode)) + gap
		}
		if w <= r.W {
			break
		}
	}
	gap := 1
	if mode > 0 {
		gap = 0
	}
	x := 1
	for i := range viewNames {
		lbl := label(i, mode)
		s := st
		if i == a.view {
			s = Sty{FG: t.CellFG, BG: t.Accent, A: Bold}
		} else if mode > 0 && i%2 == 1 {
			s = Sty{FG: t.Muted, BG: t.Panel}
		}
		if x+StrW(lbl) > r.W {
			break
		}
		b.Text(x, r.Y, lbl, s, -1)
		b.Set(x+1, r.Y, rune('1'+i), s.With(Bold).Fg(choose(i == a.view, t.CellFG, t.Warn)))
		hits = append(hits, Hit{R: Rect{x, r.Y, StrW(lbl), 1}, Kind: HitTab, A: i})
		x += StrW(lbl) + gap
	}
	return hits
}

// shownTotal returns "[shown / total]" for the view.
func (a *App) shownTotal() (int, int) {
	r := a.rep
	switch a.view {
	case vProjects:
		return a.grids["projects"].DataRows, len(r.Projects)
	case vTasks:
		return a.grids["tasks"].DataRows, len(r.Tasks)
	case vActivity:
		return a.grids["activity"].DataRows, len(r.Activity)
	case vBoard:
		return a.board.count(a), len(r.Tasks)
	case vTimeline:
		return len(a.tl.projects), len(r.Projects)
	case vTemplates:
		return a.grids["templates"].DataRows, len(r.Templates)
	case vFields:
		return a.grids["fields"].DataRows, len(r.CustomFields)
	case vUsers:
		return a.grids["users"].DataRows, len(r.Users)
	}
	return 0, 0
}

func (a *App) drawQuery(b *Buf, r Rect) []Hit {
	t := a.th()
	qb := a.queries[a.view]
	st := Sty{FG: t.FG, BG: t.BG}
	b.Fill(r, ' ', st)
	lbl := " Filter ▸ "
	b.Text(0, r.Y, lbl, st.Fg(choose(a.filterOn, t.Accent, t.Muted)).With(Bold), -1)
	shown, total := a.shownTotal()
	info := fmt.Sprintf("[ %d / %d ]", shown, total)
	if g := a.activeGrid(); g != nil {
		if len(g.Sort) > 0 {
			col := g.Sort[0].Key
			for _, c := range g.Cols {
				if c.Key == col {
					col = c.Title
				}
			}
			arrow := "▲"
			if g.Sort[0].Desc {
				arrow = "▼"
			}
			info += "  ⇅ " + col + " " + arrow
		}
		if g.GroupBy != "" {
			col := g.GroupBy
			for _, c := range g.Cols {
				if c.Key == col {
					col = c.Title
				}
			}
			info += "  ⊞ " + col
		}
	}
	iw := StrW(info)
	fr := Rect{StrW(lbl), r.Y, max(10, r.W-StrW(lbl)-iw-2), 1}
	qb.in.Draw(b, fr, t, a.filterOn)
	// underline bad terms
	if qb.q != nil {
		for _, e := range qb.q.Errors {
			start := StrW(string(qb.in.text[:min(e.Start, len(qb.in.text))]))
			end := StrW(string(qb.in.text[:min(e.End, len(qb.in.text))]))
			b.Restyle(Rect{fr.X + start, r.Y, end - start, 1}, func(s Sty) Sty { return s.ULine(t.Err).Fg(t.Err) })
		}
	}
	b.Text(r.Right()-iw-1, r.Y, info, st.Fg(t.Muted), iw)
	return []Hit{{R: fr, Kind: HitFilter}}
}

func (a *App) drawQuerySuggestions(b *Buf, r Rect) []Hit {
	qb := a.queries[a.view]
	if len(qb.sugg) == 0 {
		return nil
	}
	t := a.th()
	n := min(8, len(qb.sugg))
	w := 24
	for _, s := range qb.sugg {
		w = max(w, StrW(s)+4)
	}
	_, start, _ := qb.currentToken()
	x := 10 + StrW(string(qb.in.text[:start]))
	pr := Rect{min(x, b.W-w-1), r.Y + 1, w, n + 2}
	fill := Sty{FG: t.FG, BG: t.Panel}
	b.Box(pr, Sty{FG: t.Border, BG: t.Panel}, fill, "", fill)
	b.Shadow(pr, t.Shadow)
	var hits []Hit
	top := max(0, qb.sel-n+1)
	for i := 0; i < n && top+i < len(qb.sugg); i++ {
		st := fill
		if top+i == qb.sel {
			st = Sty{FG: t.CellFG, BG: t.CellBG}
		}
		row := Rect{pr.X + 1, pr.Y + 1 + i, pr.W - 2, 1}
		b.Fill(row, ' ', st)
		b.Text(row.X+1, row.Y, qb.sugg[top+i], st, row.W-2)
		hits = append(hits, Hit{R: row, Kind: HitPopupItem, A: top + i, S: "query"})
	}
	return hits
}

func (a *App) gridEnv(focused bool) gridEnv {
	peers := map[int64][]core.Peer{}
	return gridEnv{r: a.rep, t: a.th(), focused: focused && !a.filterOn, conflicts: a.conflicts,
		pending: a.pendCells, flash: a.flash, now: a.now, peers: peers}
}

// peersFor indexes other users' focus by row for one entity.
func (a *App) peersFor(ent core.Entity) map[int64][]core.Peer {
	out := map[int64][]core.Peer{}
	for _, p := range a.rep.OtherPeers() {
		if p.Focus != nil && p.Focus.Entity == ent {
			out[p.Focus.ID] = append(out[p.Focus.ID], p)
		}
	}
	return out
}

func (a *App) drawGrid(b *Buf, name string, r Rect, focused bool, pane int) []Hit {
	g := a.grids[name]
	env := a.gridEnv(focused)
	env.peers = a.peersFor(g.Ent)
	env.relTime = name == "activity"
	if a.editor != nil && a.editor.g == g {
		env.editing = true
	}
	hits := g.Draw(b, r, env)
	for i := range hits {
		hits[i].Pane = pane
		hits[i].S = name
	}
	return append([]Hit{{R: r, Kind: HitGridBody, Pane: pane, S: name}}, hits...)
}

func (a *App) paneTitle(b *Buf, r Rect, title string, focused bool, pane int) Hit {
	t := a.th()
	st := Sty{FG: t.Muted, BG: t.Panel2}
	if focused {
		st = Sty{FG: t.Accent, BG: t.Panel2, A: Bold}
	}
	b.Fill(Rect{r.X, r.Y, r.W, 1}, '─', Sty{FG: t.Border, BG: t.Panel2})
	hint := " F6 switch pane · ^B hide "
	if r.W < 60 {
		hint = " F6 · ^B "
	}
	b.Text(r.X+1, r.Y, " "+Ellipsis(title, r.W-StrW(hint)-6)+" ", st, r.W-2)
	b.Text(r.Right()-StrW(hint)-1, r.Y, hint, Sty{FG: t.Muted, BG: t.Panel2}, -1)
	return Hit{R: Rect{r.X, r.Y, r.W, 1}, Kind: HitPane, A: pane}
}

func (a *App) drawView(b *Buf, lay layoutT) []Hit {
	var hits []Hit
	r := a.rep
	switch a.view {
	case vProjects:
		hits = append(hits, a.drawGrid(b, "projects", lay.main, a.pane == 0, 0)...)
		if a.detail[vProjects] {
			title := "Tasks"
			if p := r.Projects[a.grids["projects"].CurID()]; p != nil {
				m := r.Metrics(p.ID)
				title = fmt.Sprintf("Tasks of %s %s — %d/%d done · %s / %s h", p.Code, p.Name, m.TasksDone, m.TasksTotal,
					core.FmtHours(m.ActualTotal), core.FmtHours(m.EstTotal))
			}
			hits = append(hits, a.paneTitle(b, lay.detail, title, a.pane == 1, 1))
			hits = append(hits, a.drawGrid(b, "projtasks", lay.grids["projtasks"], a.pane == 1, 1)...)
		}
	case vTasks:
		hits = append(hits, a.drawGrid(b, "tasks", lay.main, true, 0)...)
		if a.detail[vTasks] {
			a.drawTaskCard(b, lay.detail)
		}
	case vActivity:
		hits = append(hits, a.drawGrid(b, "activity", lay.main, true, 0)...)
	case vTemplates:
		hits = append(hits, a.drawGrid(b, "templates", lay.main, a.pane == 0, 0)...)
		if a.detail[vTemplates] {
			title := "Template tasks"
			if tp := r.Templates[a.grids["templates"].CurID()]; tp != nil {
				title = "Tasks of " + tp.Name
			}
			hits = append(hits, a.paneTitle(b, lay.detail, title, a.pane == 1, 1))
			gr := lay.grids["tmpltasks"]
			hits = append(hits, a.drawGrid(b, "tmpltasks", gr, a.pane == 1, 1)...)
			a.drawMiniGantt(b, Rect{0, gr.Bottom(), lay.detail.W, lay.detail.Bottom() - gr.Bottom()})
		}
	case vFields:
		hits = append(hits, a.drawGrid(b, "fields", lay.main, true, 0)...)
	case vUsers:
		hits = append(hits, a.drawGrid(b, "users", lay.main, a.pane == 0, 0)...)
		hits = append(hits, a.paneTitle(b, lay.detail, "Programs", a.pane == 1, 1))
		hits = append(hits, a.drawGrid(b, "programs", lay.grids["programs"], a.pane == 1, 1)...)
	case vBoard:
		hits = append(hits, a.board.draw(a, b, lay.main)...)
	case vTimeline:
		hits = append(hits, a.tl.draw(a, b, lay.main)...)
	case vDashboard:
		hits = append(hits, a.dash.draw(a, b, lay.main)...)
	}
	return hits
}

// drawTaskCard is the Tasks view's read-only record card.
func (a *App) drawTaskCard(b *Buf, r Rect) {
	t := a.th()
	rep := a.rep
	fill := Sty{FG: t.FG, BG: t.Panel}
	b.Fill(r, ' ', fill)
	b.Fill(Rect{r.X, r.Y, r.W, 1}, '─', Sty{FG: t.Border, BG: t.Panel2})
	task := rep.Tasks[a.grids["tasks"].CurID()]
	if task == nil {
		b.Text(r.X+2, r.Y, " Record ", Sty{FG: t.Muted, BG: t.Panel2}, -1)
		b.Text(r.X+2, r.Y+2, "Select a task to see its details.", fill.Fg(t.Muted), r.W-4)
		return
	}
	b.Text(r.X+1, r.Y, " "+rep.TaskRef(task)+" ", Sty{FG: t.Accent, BG: t.Panel2, A: Bold}, -1)
	y := r.Y + 1
	x := r.X + 2
	w := r.W - 4
	line := func(label, value string, st Sty) {
		if y >= r.Bottom() {
			return
		}
		b.Fit(x, y, label, fill.Fg(t.Muted), 10, 0)
		b.Text(x+11, y, value, st, w-11)
		y++
	}
	b.Text(x, y, task.Title, fill.With(Bold), w)
	y++
	ss, g := t.cellStyle(core.Style{Kind: core.StTaskStatus, Value: string(task.Status)}, fill)
	twoCol := r.W >= 70 && !(r.W > 0 && r.H > 14 && r.W < 90)
	_ = twoCol
	line("Status", g+task.Status.Label(), ss)
	ps, pg := t.cellStyle(core.Style{Kind: core.StPriority, Value: string(task.Priority)}, fill)
	line("Priority", pg+task.Priority.Label(), ps)
	line("Phase", task.Phase, fill)
	line("Assignee", rep.UserName(task.AssigneeID), fill.Fg(t.userColor(ptrVal(task.AssigneeID))))
	dates := fmt.Sprintf("%s → %s", strPtr(task.StartDate), strPtr(task.DueDate))
	ds := fill
	if rep.TaskOverdue(task) {
		ds = ds.Fg(t.Err)
	}
	line("Dates", dates, ds)
	est := "—"
	if task.EstHours != nil {
		est = core.FmtHours(*task.EstHours)
	}
	line("Hours", fmt.Sprintf("%s actual / %s est", core.FmtHours(task.ActualHours), est), fill)
	if len(task.Tags) > 0 {
		line("Tags", "#"+strings.Join(task.Tags, " #"), fill.Fg(t.Accent2))
	}
	if len(task.Deps) > 0 {
		var parts []string
		for _, d := range task.Deps {
			if dt := rep.Tasks[d]; dt != nil {
				parts = append(parts, fmt.Sprintf("#%d %s %s", dt.Seq, strings.TrimSpace(taskGlyph(dt.Status)), dt.Title))
			}
		}
		line("Waits on", strings.Join(parts, " · "), fill)
	}
	for _, cf := range rep.CustomFieldsFor(core.CfTask) {
		if v, ok := task.CF[fmt.Sprint(cf.ID)]; ok && v != nil {
			line(cf.Name, core.DisplayValue(rep, core.EntTask, cf.Key(), v), fill)
		}
	}
	if task.Notes != "" && y < r.Bottom() {
		y++
		for _, l := range wrap(task.Notes, w) {
			if y >= r.Bottom() {
				break
			}
			b.Text(x, y, l, fill.With(Italic), w)
			y++
		}
	}
	// recent activity on this task
	if y < r.Bottom()-1 {
		y++
		b.Text(x, y, "Recent changes", fill.Fg(t.Accent).With(Bold), w)
		y++
		ids := core.SortedIDs(rep.Activity)
		for i := len(ids) - 1; i >= 0 && y < r.Bottom(); i-- {
			ac := rep.Activity[ids[i]]
			if ac.Entity != "task" || ac.EntityID != task.ID {
				continue
			}
			b.Text(x, y, fmt.Sprintf("%-8s %-8s %s", core.RelativeTime(ac.TS, a.now.Unix()), rep.Username(ac.UserID), ac.Label), fill.Fg(t.Muted), w)
			y++
		}
	}
}

func strPtr(s *string) string {
	if s == nil {
		return "—"
	}
	return *s
}

func ptrVal(p *int64) int64 {
	if p == nil {
		return 0
	}
	return *p
}

// drawMiniGantt previews the selected template's schedule.
func (a *App) drawMiniGantt(b *Buf, r Rect) {
	t := a.th()
	fill := Sty{FG: t.FG, BG: t.Panel}
	b.Fill(r, ' ', fill)
	if r.H < 2 {
		return
	}
	tid := a.grids["templates"].CurID()
	tts := a.rep.TemplateTasksOf(tid)
	if len(tts) == 0 {
		return
	}
	span := int64(1)
	for _, tt := range tts {
		span = max(span, tt.OffsetDays+tt.DurationDays)
	}
	b.Text(r.X+1, r.Y, fmt.Sprintf("Schedule preview — %d days", span), fill.Fg(t.Accent).With(Bold), r.W-2)
	labelW := 22
	cw := r.W - labelW - 3
	scale := float64(cw) / float64(span)
	phases := []string{}
	for _, tt := range tts {
		found := false
		for _, p := range phases {
			if p == tt.Phase {
				found = true
			}
		}
		if !found {
			phases = append(phases, tt.Phase)
		}
	}
	rows := r.H - 1
	step := 1
	if len(tts) > rows {
		step = (len(tts) + rows - 1) / rows
	}
	y := r.Y + 1
	for i := 0; i < len(tts) && y < r.Bottom(); i += step {
		tt := tts[i]
		pi := 0
		for j, p := range phases {
			if p == tt.Phase {
				pi = j
			}
		}
		col := t.Users[pi%len(t.Users)]
		b.Fit(r.X+1, y, tt.Title, fill.Fg(t.Muted), labelW, 0)
		x0 := r.X + labelW + 2 + int(float64(tt.OffsetDays)*scale)
		x1 := r.X + labelW + 2 + int(float64(tt.OffsetDays+tt.DurationDays)*scale)
		for x := x0; x <= max(x0, x1-1) && x < r.Right(); x++ {
			b.Set(x, y, '▬', fill.Fg(col))
		}
		y++
	}
}

func (a *App) drawStatus(b *Buf, r Rect) {
	t := a.th()
	st := Sty{FG: t.FG, BG: t.Panel2}
	b.Fill(r, ' ', st)
	msg := a.status
	ms := st
	switch a.statusKind {
	case 1:
		ms = ms.Fg(t.OK)
	case 2:
		ms = ms.Fg(t.Warn)
	case 3:
		ms = ms.Fg(t.Err)
	}
	if a.conn == connReconnecting {
		msg, ms = "⚠ Offline — reconnecting; edits are disabled", st.Fg(t.Warn).With(Bold)
	}
	if !a.rep.CanEdit() && msg == "" {
		msg, ms = "ℹ Read-only: you are a viewer", st.Fg(t.Info)
	}
	if a.filterOn {
		if qb := a.queries[a.view]; qb.q != nil && len(qb.q.Errors) > 0 {
			msg, ms = "✗ "+qb.q.Errors[0].Message, st.Fg(t.Err)
		} else {
			msg, ms = "Filtering — Enter/Esc back to the grid · Tab completes · ↑↓ choose", st.Fg(t.Muted)
		}
	}
	// right: presence
	x := r.Right() - 1
	if n := len(a.pending); n > 0 {
		s := fmt.Sprintf(" ⟳ %d", n)
		x -= StrW(s)
		b.Text(x, r.Y, s, st.Fg(t.Warn), -1)
	}
	users := a.rep.OnlineUsers()
	s := fmt.Sprintf(" online: %d ", len(users))
	x -= StrW(s)
	b.Text(x, r.Y, s, st.Fg(t.Muted), -1)
	for i := len(users) - 1; i >= 0; i-- {
		u := users[i]
		ini := " " + u.Initials() + " "
		x -= StrW(ini) + 1
		b.Text(x, r.Y, ini, Sty{FG: t.BG, BG: t.userColor(u.ID), A: Bold}, -1)
	}
	b.Text(1, r.Y, msg, ms, max(0, x-2))
}

func (a *App) keyCtx() Ctx {
	switch a.view {
	case vBoard:
		return CtxBoard
	case vTimeline:
		return CtxTimeline
	case vDashboard:
		return CtxDashboard
	}
	return CtxGrid
}

// currentHints adapts the hint bar to what has focus (editor, filter, …).
func (a *App) currentHints() []hint {
	if a.editor != nil {
		h := []hint{{"Enter", "Save ↓", ANone}, {"Tab", "Save →", ANone}, {"Esc", "Cancel", ANone}}
		switch a.editor.in.Kind {
		case IDate:
			h = append(h, hint{"↑↓", "±week", ANone}, hint{"PgUp/Dn", "±month", ANone}, hint{"^←→", "±day", ANone})
		case ISelect, ICombo, ISuggest:
			h = append(h, hint{"type", "filter", ANone}, hint{"↑↓", "choose", ANone})
		case IChips:
			h = []hint{{"Enter", "Add", ANone}, {"⌫", "Remove last", ANone}, {"Tab", "Save", ANone}, {"Esc", "Cancel", ANone}}
		case INumber:
			h = append(h, hint{"↑↓", "±1", ANone}, hint{"⇧↑↓", "±0.25", ANone})
		}
		return h
	}
	if a.filterOn {
		return []hint{{"Enter", "Apply", ANone}, {"Tab", "Complete", ANone}, {"Esc", "Back", ANone}, {"^U", "Clear", ANone}, {"F1", "Query help", AHelp}}
	}
	return hintsFor(a.keyCtx())
}

func (a *App) drawHints(b *Buf, r Rect) []Hit {
	t := a.th()
	st := Sty{FG: t.FG, BG: t.BG}
	b.Fill(r, ' ', st)
	var hits []Hit
	x, y := 0, r.Y
	for _, h := range a.currentHints() {
		kw, lw := StrW(h.Key), StrW(h.Label)
		need := kw + lw + 3
		if x+need > r.W {
			y++
			x = 0
			if y >= r.Bottom() {
				break
			}
		}
		b.Text(x, y, " "+h.Key+" ", Sty{FG: t.BG, BG: t.Muted, A: Bold}, -1)
		b.Text(x+kw+2, y, h.Label, st, -1)
		if h.Action != ANone {
			hits = append(hits, Hit{R: Rect{x, y, need - 1, 1}, Kind: HitHint, A: int(h.Action)})
		}
		x += need
	}
	return hits
}

func (a *App) drawToasts(b *Buf) []Hit {
	t := a.th()
	var hits []Hit
	y := 2
	for i := len(a.toasts) - 1; i >= 0; i-- {
		ts := a.toasts[i]
		w := min(60, StrW(ts.text)+4)
		lines := wrap(ts.text, w-4)
		h := len(lines) + 2
		r := Rect{b.W - w - 1, y, w, h}
		c := t.Info
		switch ts.kind {
		case 1:
			c = t.OK
		case 2:
			c = t.Warn
		case 3:
			c = t.Err
		}
		fill := Sty{FG: t.FG, BG: t.Panel2}
		b.Box(r, Sty{FG: c, BG: t.Panel2}, fill, "", fill)
		b.Shadow(r, t.Shadow)
		for j, l := range lines {
			b.Text(r.X+2, r.Y+1+j, l, fill, w-4)
		}
		hits = append(hits, Hit{R: r, Kind: HitToast, A: i})
		y += h
		if y > b.H-6 {
			break
		}
	}
	return hits
}

// drawEditor draws the in-cell editor over its cell plus its popup.
func (a *App) drawEditor(b *Buf) []Hit {
	e := a.editor
	t := a.th()
	cr, ok := e.g.CellRect()
	if !ok {
		return nil
	}
	w := cr.W
	switch e.in.Kind {
	case IChips:
		w = max(w, 44)
	case IText, ISuggest, IDate, ICombo, ISelect:
		w = max(w, 22)
	}
	fr := Rect{cr.X, cr.Y, min(w, b.W-cr.X), 1}
	e.in.Draw(b, fr, t, true)
	b.Restyle(fr, func(s Sty) Sty { return s.ULine(t.Accent) })
	var hits []Hit
	pw, ph := e.in.PopupSize()
	if pw > 0 {
		pw = max(pw, fr.W)
		pr := Rect{fr.X, fr.Y + 1, pw, ph}
		if pr.Bottom() > b.H-2 {
			pr.Y = fr.Y - ph
		}
		if pr.Right() > b.W {
			pr.X = max(0, b.W-pw)
		}
		hits = append(hits, Hit{R: fr, Kind: HitField, S: "editor"})
		for _, h := range e.in.DrawPopup(b, pr, t) {
			h.S = "editor"
			hits = append(hits, h)
		}
	}
	if e.in.Err != "" {
		msg := " ⚠ " + e.in.Err + " "
		b.Text(fr.X, fr.Y-1, msg, Sty{FG: t.CellFG, BG: t.Err}, b.W-fr.X)
	}
	return hits
}
