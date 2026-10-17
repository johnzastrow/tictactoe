package tui

// Timeline / Gantt (ui-spec §4.4): a project → task tree on the left, a
// zoomable time axis on the right with weekend shading, a today line,
// progress-filled bars, milestones and dependency markers. Bars move with
// Shift+←/→ and resize with [ ]; the mouse drags them.

import (
	"fmt"
	"math"
	"sort"
	"strings"

	tea "charm.land/bubbletea/v2"

	"git.wharf.fluidgrid.org/jcz/tictactoe/go/internal/pt/core"
)

const tlLabelW = 34

var zoomNames = []string{"Day", "Week", "Month"}
var zoomScale = []float64{2, 1, 0.25} // cells per day
var zoomUnit = []int{1, 7, 30}        // days per Shift+arrow step

type tlRow struct {
	ent   core.Entity
	id    int64
	depth int
}

type timelineState struct {
	projects  []int64
	expanded  map[int64]bool
	rows      []tlRow
	sel, top  int
	zoom      int
	origin    core.Date
	originSet bool
}

func newTimeline() *timelineState {
	return &timelineState{expanded: map[int64]bool{}, zoom: 1}
}

func (tl *timelineState) rebuild(a *App) {
	r := a.rep
	q := a.queries[vTimeline]
	if q.q == nil {
		q.compile(r)
	}
	var curKey tlRow
	if tl.sel < len(tl.rows) {
		curKey = tl.rows[tl.sel]
	}
	tl.projects = tl.projects[:0]
	for _, id := range core.SortedIDs(r.Projects) {
		if q.q.IsEmpty() || q.q.MatchesProject(r.Projects[id], r) {
			tl.projects = append(tl.projects, id)
		}
	}
	sort.SliceStable(tl.projects, func(i, j int) bool {
		pi, pj := r.Projects[tl.projects[i]], r.Projects[tl.projects[j]]
		si, sj := strPtr(pi.StartDate), strPtr(pj.StartDate)
		if si != sj {
			return si < sj
		}
		return pi.Code < pj.Code
	})
	tl.rows = tl.rows[:0]
	for _, pid := range tl.projects {
		tl.rows = append(tl.rows, tlRow{core.EntProject, pid, 0})
		if tl.expanded[pid] {
			for _, tid := range r.ProjectTasks(pid) {
				tl.rows = append(tl.rows, tlRow{core.EntTask, tid, 1})
			}
		}
	}
	for i, row := range tl.rows {
		if row == curKey {
			tl.sel = i
		}
	}
	tl.sel = max(0, min(tl.sel, len(tl.rows)-1))
	if !tl.originSet && r.IsLoaded() {
		tl.origin = r.Today.AddDays(-14)
		tl.originSet = true
	}
}

func (tl *timelineState) cur(a *App) (tlRow, bool) {
	if tl.sel >= 0 && tl.sel < len(tl.rows) {
		return tl.rows[tl.sel], true
	}
	return tlRow{}, false
}

// dates of a row's bar.
func (tl *timelineState) dates(a *App, row tlRow) (s, d *core.Date) {
	var sd, dd *string
	if row.ent == core.EntProject {
		p := a.rep.Projects[row.id]
		if p == nil {
			return nil, nil
		}
		sd, dd = p.StartDate, p.DueDate
	} else {
		t := a.rep.Tasks[row.id]
		if t == nil {
			return nil, nil
		}
		sd, dd = t.StartDate, t.DueDate
	}
	if sd != nil {
		if x, ok := core.ParseISO(*sd); ok {
			s = &x
		}
	}
	if dd != nil {
		if x, ok := core.ParseISO(*dd); ok {
			d = &x
		}
	}
	if s == nil && d != nil {
		s = d
	}
	if d == nil && s != nil {
		d = s
	}
	return
}

func (tl *timelineState) xOf(r Rect, d core.Date) int {
	return r.X + int(math.Floor(float64(int(d)-int(tl.origin))*zoomScale[tl.zoom]))
}

func (tl *timelineState) draw(a *App, b *Buf, r Rect) []Hit {
	t := a.th()
	rep := a.rep
	var hits []Hit
	fill := Sty{FG: t.FG, BG: t.Panel}
	b.Fill(r, ' ', fill)
	left := Rect{r.X, r.Y, min(tlLabelW, r.W/3), r.H}
	area := Rect{left.Right() + 1, r.Y, r.W - left.W - 1, r.H}
	sc := zoomScale[tl.zoom]
	days := int(float64(area.W) / sc)
	// header: months and days/weeks
	hst := Sty{FG: t.FG, BG: t.Panel2, A: Bold}
	b.Fill(Rect{r.X, r.Y, r.W, 2}, ' ', hst)
	b.Text(left.X+1, r.Y, fmt.Sprintf("Zoom: %s (+/-)", zoomNames[tl.zoom]), hst.Fg(t.Muted), left.W-2)
	b.Text(left.X+1, r.Y+1, "t today · < > scroll", hst.Fg(t.Muted).With(0), left.W-2)
	for i := 0; i <= days; i++ {
		d := tl.origin.AddDays(i)
		x := tl.xOf(area, d)
		if x >= area.Right() {
			break
		}
		y, m, dd := d.YMD()
		if dd == 1 || i == 0 {
			lbl := core.MonthNames[m-1]
			if m == 1 || i == 0 {
				lbl += fmt.Sprintf(" %d", y)
			}
			b.Text(x, r.Y, "▏"+lbl, hst.Fg(t.Accent), area.Right()-x)
		}
		switch tl.zoom {
		case 0:
			b.Text(x, r.Y+1, fmt.Sprintf("%2d", dd), hst.Fg(choose(d.Weekday() >= 5, t.Muted, t.FG)).With(0), 2)
		case 1:
			if d.Weekday() == 0 {
				b.Text(x, r.Y+1, fmt.Sprintf("▏%d", dd), hst.Fg(t.FG).With(0), area.Right()-x)
			}
		}
	}
	// body
	bodyY := r.Y + 2
	n := r.H - 2
	top := max(0, min(tl.top, max(0, len(tl.rows)-n)))
	if tl.sel < top {
		top = tl.sel
	}
	if tl.sel >= top+n {
		top = tl.sel - n + 1
	}
	todayX := tl.xOf(area, rep.Today)
	for i := 0; i < n; i++ {
		y := bodyY + i
		ri := top + i
		rowBG := t.Panel
		if ri%2 == 1 {
			rowBG = t.Zebra
		}
		if ri == tl.sel {
			rowBG = t.CurBG
		}
		b.Fill(Rect{r.X, y, r.W, 1}, ' ', Sty{FG: t.FG, BG: rowBG})
		// weekend shading + today line
		if tl.zoom < 2 {
			for dd := 0; dd <= days; dd++ {
				d := tl.origin.AddDays(dd)
				if d.Weekday() >= 5 {
					x := tl.xOf(area, d)
					w := max(1, int(sc))
					b.Fill(Rect{x, y, min(w, area.Right()-x), 1}, ' ', Sty{FG: t.FG, BG: mix(rowBG, t.BG, 0.6)})
				}
			}
		}
		if todayX >= area.X && todayX < area.Right() {
			b.Set(todayX, y, '│', Sty{FG: t.Accent, BG: rowBG})
		}
		b.Set(left.Right(), y, '│', Sty{FG: t.Border, BG: rowBG})
		if ri >= len(tl.rows) {
			continue
		}
		row := tl.rows[ri]
		hits = append(hits, Hit{R: Rect{r.X, y, left.W, 1}, Kind: HitTLRow, A: ri})
		hits = append(hits, Hit{R: Rect{area.X, y, area.W, 1}, Kind: HitTLArea, A: ri})
		lst := Sty{FG: t.FG, BG: rowBG}
		var label string
		var barColor Color
		var pct float64
		milestone, waits := false, false
		if row.ent == core.EntProject {
			p := rep.Projects[row.id]
			arrow := "▸ "
			if tl.expanded[row.id] {
				arrow = "▾ "
			}
			label = arrow + p.Code + " " + p.Name
			lst = lst.With(Bold)
			barColor = t.health(p.Health)
			pct = rep.Metrics(p.ID).PctDone()
		} else {
			tk := rep.Tasks[row.id]
			label = fmt.Sprintf("   #%d %s", tk.Seq, tk.Title)
			barColor = t.taskStatus(tk.Status)
			if tk.Status == core.TaskDone {
				pct = 100
			} else if tk.EstHours != nil && *tk.EstHours > 0 {
				pct = min(100, tk.ActualHours*100 / *tk.EstHours)
			}
			if tk.StartDate != nil && tk.DueDate != nil && *tk.StartDate == *tk.DueDate && (tk.EstHours == nil || *tk.EstHours == 0) {
				milestone = true
			}
			for _, d := range tk.Deps {
				if s, ok := rep.TaskStatusOf(d); ok && s != core.TaskDone {
					waits = true
				}
			}
			if rep.TaskOverdue(tk) {
				lst = lst.Fg(t.Err)
			}
		}
		b.Fit(r.X+1, y, label, lst, left.W-2, 0)
		s, d := tl.dates(a, row)
		if s == nil {
			continue
		}
		x0 := tl.xOf(area, *s)
		x1 := tl.xOf(area, d.AddDays(1)) - 1
		if x1 < x0 {
			x1 = x0
		}
		if milestone {
			if x0 >= area.X && x0 < area.Right() {
				b.Set(x0, y, '◆', Sty{FG: t.Warn, BG: rowBG, A: Bold})
				hits = append(hits, Hit{R: Rect{x0, y, 1, 1}, Kind: HitTLBar, A: ri})
			}
			continue
		}
		done := x0 + int(float64(x1-x0+1)*pct/100)
		for x := max(x0, area.X); x <= x1 && x < area.Right(); x++ {
			ch, st := '█', Sty{FG: barColor, BG: rowBG}
			if x >= done {
				ch, st = '▓', Sty{FG: mix(barColor, rowBG, 0.45), BG: rowBG}
			}
			b.Set(x, y, ch, st)
		}
		if x1 >= area.X && x0 < area.Right() {
			bx := max(x0, area.X)
			hits = append(hits, Hit{R: Rect{bx, y, max(1, min(x1, area.Right()-1)-bx), 1}, Kind: HitTLBar, A: ri})
			if x1 < area.Right() {
				hits = append(hits, Hit{R: Rect{x1, y, 1, 1}, Kind: HitTLBarEnd, A: ri})
			}
		}
		if waits && x0-1 >= area.X && x0-1 < area.Right() {
			b.Set(x0-1, y, '◂', Sty{FG: t.Err, BG: rowBG, A: Bold})
		}
		if row.ent == core.EntProject && x1+2 < area.Right() && x1+2 >= area.X {
			b.Text(x1+2, y, fmt.Sprintf("%.0f%%", math.Round(pct)), Sty{FG: t.Muted, BG: rowBG}, 5)
		}
	}
	// drag preview
	if dr := a.drag; dr != nil && (dr.kind == HitTLBar || dr.kind == HitTLBarEnd) && dr.moved {
		delta := int(math.Round(float64(dr.x-dr.x0) / sc))
		verb := "move"
		if dr.kind == HitTLBarEnd {
			verb = "due"
		}
		msg := fmt.Sprintf(" %s %+d days ", verb, delta)
		b.Text(min(dr.x+1, r.Right()-StrW(msg)), max(bodyY, dr.y-1), msg, Sty{FG: t.CellFG, BG: t.Accent, A: Bold}, -1)
	}
	return hits
}

func (tl *timelineState) key(a *App, k tea.KeyPressMsg) tea.Cmd {
	n := max(1, a.layout().main.H-3)
	switch k.Keystroke() {
	case "up":
		tl.sel--
	case "down":
		tl.sel++
	case "pgup":
		tl.sel -= n
	case "pgdown":
		tl.sel += n
	case "home", "ctrl+home":
		tl.sel = 0
	case "end", "ctrl+end":
		tl.sel = len(tl.rows) - 1
	case "right", "enter":
		if row, ok := tl.cur(a); ok {
			if row.ent == core.EntProject {
				tl.expanded[row.id] = true
				tl.rebuild(a)
			} else if k.Keystroke() == "enter" {
				a.recordForm(core.EntTask, row.id)
			}
		}
	case "left":
		if row, ok := tl.cur(a); ok {
			if row.ent == core.EntProject {
				delete(tl.expanded, row.id)
			} else if t := a.rep.Tasks[row.id]; t != nil {
				delete(tl.expanded, t.ProjectID)
			}
			cur := row
			if cur.ent == core.EntTask {
				if t := a.rep.Tasks[row.id]; t != nil {
					cur = tlRow{core.EntProject, t.ProjectID, 0}
				}
			}
			tl.rebuild(a)
			for i, r := range tl.rows {
				if r == cur {
					tl.sel = i
				}
			}
		}
	case "space":
		if row, ok := tl.cur(a); ok && row.ent == core.EntProject {
			tl.expanded[row.id] = !tl.expanded[row.id]
			tl.rebuild(a)
		}
	}
	tl.sel = max(0, min(len(tl.rows)-1, tl.sel))
	tl.scrollTo(a)
	return nil
}

func (tl *timelineState) scrollTo(a *App) {
	n := max(1, a.layout().main.H-2)
	if tl.sel < tl.top {
		tl.top = tl.sel
	}
	if tl.sel >= tl.top+n {
		tl.top = tl.sel - n + 1
	}
}

func (tl *timelineState) visibleDays(a *App) int {
	w := a.layout().main.W - min(tlLabelW, a.layout().main.W/3) - 1
	return int(float64(w) / zoomScale[tl.zoom])
}

func (tl *timelineState) action(a *App, ac Action) tea.Cmd {
	unit := zoomUnit[tl.zoom]
	switch ac {
	case AZoomIn:
		center := tl.origin.AddDays(tl.visibleDays(a) / 2)
		tl.zoom = max(0, tl.zoom-1)
		tl.origin = center.AddDays(-tl.visibleDays(a) / 2)
	case AZoomOut:
		center := tl.origin.AddDays(tl.visibleDays(a) / 2)
		tl.zoom = min(2, tl.zoom+1)
		tl.origin = center.AddDays(-tl.visibleDays(a) / 2)
	case AToday:
		tl.origin = a.rep.Today.AddDays(-tl.visibleDays(a) / 4)
	case ATimeBack:
		tl.origin = tl.origin.AddDays(-max(1, tl.visibleDays(a)/4))
	case ATimeFwd:
		tl.origin = tl.origin.AddDays(max(1, tl.visibleDays(a)/4))
	case ABarLeft:
		return tl.shift(a, -unit, -unit)
	case ABarRight:
		return tl.shift(a, unit, unit)
	case AShrinkDue:
		return tl.shift(a, 0, -unit)
	case AExtendDue:
		return tl.shift(a, 0, unit)
	}
	return nil
}

// shift moves the current bar's start by ds days and due by dd days.
func (tl *timelineState) shift(a *App, ds, dd int) tea.Cmd {
	row, ok := tl.cur(a)
	if !ok {
		return nil
	}
	return tl.shiftRow(a, row, ds, dd)
}

func (tl *timelineState) shiftRow(a *App, row tlRow, ds, dd int) tea.Cmd {
	if ds == 0 && dd == 0 {
		return nil
	}
	r := a.rep
	var sd, due *string
	if row.ent == core.EntProject {
		p := r.Projects[row.id]
		sd, due = p.StartDate, p.DueDate
	} else {
		t := r.Tasks[row.id]
		sd, due = t.StartDate, t.DueDate
	}
	var ops []core.Op
	var cells []cellKey
	mv := func(field string, cur *string, by int) {
		if cur == nil || by == 0 {
			return
		}
		d, ok := core.ParseISO(*cur)
		if !ok {
			return
		}
		ops = append(ops, core.SetOp(row.ent, row.id, field, *cur, d.AddDays(by).ISO()))
		cells = append(cells, cellKey{row.ent, row.id, field})
	}
	mv("start_date", sd, ds)
	mv("due_date", due, dd)
	if len(ops) == 0 {
		a.toast("This bar has no dates to move", 2)
		return nil
	}
	if sd != nil && due != nil {
		s, _ := core.ParseISO(*sd)
		d, _ := core.ParseISO(*due)
		if d.AddDays(dd) < s.AddDays(ds) {
			a.toast("The due date can't be before the start", 2)
			return nil
		}
	}
	desc := []string{}
	if ds != 0 {
		desc = append(desc, fmt.Sprintf("start %+dd", ds))
	}
	if dd != 0 {
		desc = append(desc, fmt.Sprintf("due %+dd", dd))
	}
	return a.mutate(ops, cells, r.RowLabel(row.ent, row.id)+": "+strings.Join(desc, ", "), nil)
}

func (tl *timelineState) click(a *App, h Hit, c tea.MouseClickMsg, dbl bool) tea.Cmd {
	switch h.Kind {
	case HitTLRow, HitTLArea, HitTLBar, HitTLBarEnd:
		tl.sel = h.A
		if dbl {
			if row, ok := tl.cur(a); ok {
				if row.ent == core.EntProject && h.Kind == HitTLRow {
					tl.expanded[row.id] = !tl.expanded[row.id]
					tl.rebuild(a)
				} else {
					a.recordForm(row.ent, row.id)
				}
			}
			return nil
		}
		if c.Button == tea.MouseLeft && (h.Kind == HitTLBar || h.Kind == HitTLBarEnd) {
			a.drag = &dragState{kind: h.Kind, x0: c.X, y0: c.Y, x: c.X, y: c.Y, a: h.A}
		}
		if h.Kind == HitTLRow && c.X <= 3 {
			if row, ok := tl.cur(a); ok && row.ent == core.EntProject {
				tl.expanded[row.id] = !tl.expanded[row.id]
				tl.rebuild(a)
			}
		}
	}
	return nil
}

func (tl *timelineState) drop(a *App, d *dragState) tea.Cmd {
	if !d.moved || d.a >= len(tl.rows) {
		return nil
	}
	delta := int(math.Round(float64(d.x-d.x0) / zoomScale[tl.zoom]))
	if delta == 0 {
		return nil
	}
	row := tl.rows[d.a]
	if d.kind == HitTLBarEnd {
		return tl.shiftRow(a, row, 0, delta)
	}
	return tl.shiftRow(a, row, delta, delta)
}

func (tl *timelineState) wheel(a *App, m hitMsg, d int) {
	if w, ok := m.ev.(tea.MouseWheelMsg); ok {
		switch {
		case w.Button == tea.MouseWheelLeft || w.Mod&tea.ModShift != 0 && w.Button == tea.MouseWheelUp:
			tl.origin = tl.origin.AddDays(-zoomUnit[tl.zoom])
			return
		case w.Button == tea.MouseWheelRight || w.Mod&tea.ModShift != 0 && w.Button == tea.MouseWheelDown:
			tl.origin = tl.origin.AddDays(zoomUnit[tl.zoom])
			return
		}
	}
	tl.top = max(0, tl.top+3*d)
	tl.sel = max(tl.top, min(tl.sel, tl.top+a.layout().main.H-3))
	tl.sel = max(0, min(len(tl.rows)-1, tl.sel))
}
