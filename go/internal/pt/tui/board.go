package tui

// Board (kanban, ui-spec §4.3): status columns, cascading Program ▸ Project
// scope, optional swimlanes by assignee, keyboard card moves and mouse
// drag-and-drop between columns.

import (
	"fmt"
	"sort"
	"strings"

	tea "charm.land/bubbletea/v2"

	"git.wharf.fluidgrid.org/jcz/tictactoe/go/internal/pt/core"
)

const cardH = 4 // 3 lines + gap

type boardState struct {
	program, project *int64
	lanes            bool
	col, idx         int
	scroll           [5]int
	laneScroll       int
}

func newBoard() *boardState { return &boardState{} }

type laneDef struct {
	id    int64 // 0 = unassigned
	label string
}

// columns returns the task ids per status (lanes concatenated in lane order).
func (bs *boardState) columns(a *App) ([5][]int64, []laneDef) {
	r := a.rep
	q := a.queries[vBoard]
	if q.q == nil {
		q.compile(r)
	}
	var cols [5][]int64
	for _, id := range core.SortedIDs(r.Tasks) {
		t := r.Tasks[id]
		if bs.project != nil && t.ProjectID != *bs.project {
			continue
		}
		if bs.program != nil {
			p := r.Projects[t.ProjectID]
			if p == nil || p.ProgramID == nil || *p.ProgramID != *bs.program {
				continue
			}
		}
		if !q.q.IsEmpty() && !q.q.MatchesTask(t, r) {
			continue
		}
		ci := t.Status.Index()
		cols[ci] = append(cols[ci], id)
	}
	laneOf := func(id int64) int64 { return ptrVal(r.Tasks[id].AssigneeID) }
	var lanes []laneDef
	if bs.lanes {
		seen := map[int64]bool{}
		for _, c := range cols {
			for _, id := range c {
				seen[laneOf(id)] = true
			}
		}
		for uid := range seen {
			name := "Unassigned"
			if u := r.Users[uid]; u != nil {
				name = u.DisplayName
			}
			lanes = append(lanes, laneDef{uid, name})
		}
		sort.Slice(lanes, func(i, j int) bool {
			if (lanes[i].id == 0) != (lanes[j].id == 0) {
				return lanes[j].id == 0
			}
			return lanes[i].label < lanes[j].label
		})
	}
	laneIdx := map[int64]int{}
	for i, l := range lanes {
		laneIdx[l.id] = i
	}
	for ci := range cols {
		c := cols[ci]
		sort.SliceStable(c, func(i, j int) bool {
			ti, tj := r.Tasks[c[i]], r.Tasks[c[j]]
			if bs.lanes {
				li, lj := laneIdx[laneOf(c[i])], laneIdx[laneOf(c[j])]
				if li != lj {
					return li < lj
				}
			}
			if ti.Priority.Index() != tj.Priority.Index() {
				return ti.Priority.Index() > tj.Priority.Index()
			}
			di, dj := strPtr(ti.DueDate), strPtr(tj.DueDate)
			if di != dj {
				return di < dj
			}
			return ti.ID < tj.ID
		})
	}
	return cols, lanes
}

func (bs *boardState) count(a *App) int {
	cols, _ := bs.columns(a)
	n := 0
	for _, c := range cols {
		n += len(c)
	}
	return n
}

func (bs *boardState) curID(a *App) int64 {
	cols, _ := bs.columns(a)
	if bs.col < 5 && bs.idx >= 0 && bs.idx < len(cols[bs.col]) {
		return cols[bs.col][bs.idx]
	}
	return 0
}

func (bs *boardState) curStatus() core.TaskStatus { return core.AllTaskStatuses[bs.col] }

func (bs *boardState) clamp(a *App) {
	cols, _ := bs.columns(a)
	bs.col = max(0, min(4, bs.col))
	bs.idx = max(0, min(len(cols[bs.col])-1, bs.idx))
}

// cardY returns each card's y (content-relative) in lanes mode, and lane header rows.
type cardPos struct {
	col, idx, y int
	id          int64
}

func (bs *boardState) positions(a *App, cols [5][]int64, lanes []laneDef) ([]cardPos, [][2]int, int) {
	r := a.rep
	var out []cardPos
	var headers [][2]int // (y, lane index)
	if !bs.lanes {
		h := 0
		for ci, c := range cols {
			for i, id := range c {
				out = append(out, cardPos{ci, i, i * cardH, id})
			}
			h = max(h, len(c)*cardH)
		}
		return out, nil, h
	}
	y := 0
	start := [5]int{}
	for li, l := range lanes {
		headers = append(headers, [2]int{y, li})
		y++
		maxN := 0
		for ci, c := range cols {
			n := 0
			for i := start[ci]; i < len(c); i++ {
				if ptrVal(r.Tasks[c[i]].AssigneeID) != l.id {
					break
				}
				out = append(out, cardPos{ci, i, y + n*cardH, c[i]})
				n++
			}
			start[ci] += n
			maxN = max(maxN, n)
		}
		y += max(1, maxN) * cardH
	}
	return out, headers, y
}

// ensureVisible scrolls so the cursor card is on screen.
func (bs *boardState) ensureVisible(a *App) {
	r := a.layout().main
	view := r.H - 3
	cols, lanes := bs.columns(a)
	pos, _, _ := bs.positions(a, cols, lanes)
	for _, p := range pos {
		if p.col == bs.col && p.idx == bs.idx {
			s := &bs.scroll[bs.col]
			if bs.lanes {
				s = &bs.laneScroll
			}
			if p.y < *s {
				*s = p.y
			}
			if p.y+cardH-1 > *s+view {
				*s = p.y + cardH - 1 - view
			}
		}
	}
}

func (bs *boardState) draw(a *App, b *Buf, r Rect) []Hit {
	t := a.th()
	rep := a.rep
	var hits []Hit
	fill := Sty{FG: t.FG, BG: t.BG}
	b.Fill(r, ' ', fill)
	// scope bar
	prog, proj := "All programs", "All projects"
	if bs.program != nil {
		prog = rep.ProgramName(bs.program)
	}
	if bs.project != nil {
		if p := rep.Projects[*bs.project]; p != nil {
			proj = p.Code + " " + p.Name
		}
	}
	sx := 1
	sx += b.Text(sx, r.Y, "Scope ", fill.Fg(t.Muted), -1)
	scope := "[" + prog + " ▾] ▸ [" + proj + " ▾]"
	b.Text(sx, r.Y, scope, Sty{FG: t.Accent, BG: t.Panel2, A: Bold}, r.W-40)
	hits = append(hits, Hit{R: Rect{sx, r.Y, StrW(scope), 1}, Kind: HitScope})
	lanes := "off"
	if bs.lanes {
		lanes = "by assignee"
	}
	info := "swimlanes: " + lanes + " (^R) · S scope"
	b.Text(r.Right()-StrW(info)-1, r.Y, info, fill.Fg(t.Muted), -1)

	cols, laneDefs := bs.columns(a)
	pos, headers, _ := bs.positions(a, cols, laneDefs)
	cw := (r.W - 1) / 5
	top := r.Y + 1
	bodyY := top + 2
	bodyH := r.Bottom() - bodyY
	others := map[int64][]core.Peer{}
	for _, p := range rep.OtherPeers() {
		if p.Focus != nil && p.Focus.Entity == core.EntTask {
			others[p.Focus.ID] = append(others[p.Focus.ID], p)
		}
	}
	dragTarget := -1
	if a.drag != nil && a.drag.kind == HitCard && a.drag.moved {
		dragTarget = min(4, max(0, (a.drag.x-r.X)/max(1, cw)))
	}
	for ci := 0; ci < 5; ci++ {
		x := r.X + ci*cw
		colR := Rect{x, top, cw - 1, r.Bottom() - top}
		st := core.AllTaskStatuses[ci]
		sc := t.taskStatus(st)
		bg := t.Panel
		if ci == bs.col {
			bg = mix(t.Panel, t.Accent, 0.06)
		}
		if ci == dragTarget {
			bg = mix(t.Panel, t.Accent, 0.25)
		}
		b.Fill(colR, ' ', Sty{FG: t.FG, BG: bg})
		hours := 0.0
		for _, id := range cols[ci] {
			if e := rep.Tasks[id].EstHours; e != nil {
				hours += *e
			}
		}
		head := fmt.Sprintf("%s%s · %d · %sh", taskGlyph(st), st.Label(), len(cols[ci]), core.FmtHours(hours))
		b.Fill(Rect{x, top, cw - 1, 1}, ' ', Sty{FG: sc, BG: t.Panel2, A: Bold})
		b.Text(x+1, top, head, Sty{FG: sc, BG: t.Panel2, A: Bold}, cw-3)
		b.Fill(Rect{x, top + 1, cw - 1, 1}, '▔', Sty{FG: sc, BG: bg})
		hits = append(hits, Hit{R: colR, Kind: HitBoardCol, A: ci})
	}
	// lane headers
	scrollOf := func(ci int) int {
		if bs.lanes {
			return bs.laneScroll
		}
		return bs.scroll[ci]
	}
	for _, h := range headers {
		y := bodyY + h[0] - bs.laneScroll
		if y < bodyY || y >= r.Bottom() {
			continue
		}
		l := laneDefs[h[1]]
		c := t.userColor(l.id)
		b.Fill(Rect{r.X, y, r.W - 1, 1}, '─', Sty{FG: t.Border, BG: t.BG})
		b.Text(r.X+1, y, " ▾ "+l.label+" ", Sty{FG: c, BG: t.BG, A: Bold}, r.W-4)
	}
	for _, p := range pos {
		y := bodyY + p.y - scrollOf(p.col)
		if y < bodyY || y+2 >= r.Bottom()+0 || y+2 > bodyY+bodyH-1 {
			continue
		}
		cr := Rect{r.X + p.col*cw + 1, y, cw - 3, 3}
		sel := p.col == bs.col && p.idx == bs.idx
		bs.drawCard(a, b, cr, rep.Tasks[p.id], sel, others[p.id])
		hits = append(hits, Hit{R: cr, Kind: HitCard, A: p.col, B: p.idx})
	}
	// overflow indicators
	for ci := 0; ci < 5; ci++ {
		if bs.lanes {
			break
		}
		if n := len(cols[ci]); n*cardH-bs.scroll[ci] > bodyH {
			b.Text(r.X+ci*cw+cw/2-3, r.Bottom()-1, "▾ more", Sty{FG: t.Muted, BG: t.Panel}, 6)
		}
		if bs.scroll[ci] > 0 {
			b.Text(r.X+ci*cw+cw/2-3, bodyY-1, "▴ more", Sty{FG: t.Muted, BG: t.Panel}, 6)
		}
	}
	return hits
}

func (bs *boardState) drawCard(a *App, b *Buf, r Rect, task *core.Task, sel bool, peers []core.Peer) {
	t := a.th()
	rep := a.rep
	if task == nil {
		return
	}
	bg := t.Panel2
	if sel {
		bg = mix(t.Panel2, t.Accent, 0.30)
	}
	st := Sty{FG: t.FG, BG: bg}
	b.Fill(r, ' ', st)
	edge := t.taskStatus(task.Status)
	if rep.TaskBlocked(task) {
		edge = t.Err
	}
	for y := r.Y; y < r.Bottom(); y++ {
		b.Set(r.X, y, '▌', Sty{FG: edge, BG: bg})
	}
	x := r.X + 2
	w := r.W - 3
	pm := ""
	switch task.Priority {
	case core.PriorityCritical:
		pm = "◆"
	case core.PriorityHigh:
		pm = "▲"
	}
	if pm != "" {
		x += b.Text(x, r.Y, pm+" ", st.Fg(t.priority(task.Priority)).With(Bold), 2)
	}
	b.Text(x, r.Y, fmt.Sprintf("#%d %s", task.Seq, task.Title), st.With(Bold), r.Right()-x-1)
	// line 2: project · assignee · due, fitted to the card width
	due, ds := "", st.Fg(t.Muted)
	if task.DueDate != nil {
		if d, ok := core.ParseISO(*task.DueDate); ok {
			if rep.TaskOverdue(task) {
				ds = st.Fg(t.Err).With(Bold)
			} else if int(d)-int(rep.Today) <= 7 && task.Status != core.TaskDone {
				ds = st.Fg(t.Warn)
			}
			due = strings.TrimSpace(d.Short(rep.Today.Year()))
			if w >= 20 {
				due = "⏱ " + due
			}
		}
	}
	ini := ""
	u := rep.Users[ptrVal(task.AssigneeID)]
	if u != nil {
		ini = u.Initials()
	}
	avail := w - StrW(due) - 1
	code := rep.ProjectCode(task.ProjectID)
	x2 := r.X + 2
	if ini != "" && avail >= StrW(code)+4 {
		x2 += b.Text(x2, r.Y+1, code, st.Fg(t.Muted), StrW(code)) + 1
		b.Text(x2, r.Y+1, ini, Sty{FG: t.BG, BG: t.userColor(u.ID), A: Bold}, 2)
	} else if ini != "" && avail >= 3 {
		b.Text(x2, r.Y+1, ini, Sty{FG: t.BG, BG: t.userColor(u.ID), A: Bold}, 2)
	} else {
		b.Text(x2, r.Y+1, code, st.Fg(t.Muted), max(0, avail))
	}
	if due != "" {
		b.Text(r.Right()-StrW(due)-1, r.Y+1, due, ds, -1)
	}
	// line 3: hours bar
	est := 0.0
	if task.EstHours != nil {
		est = *task.EstHours
	}
	label := fmt.Sprintf("%s/%sh", core.FmtHours(task.ActualHours), core.FmtHours(est))
	pct, lvl := 0.0, 0
	if est > 0 {
		pct = task.ActualHours * 100 / est
		if pct > 100 {
			lvl = 2
		} else if pct > 90 {
			lvl = 1
		}
	}
	open := 0
	for _, d := range task.Deps {
		if s, ok := rep.TaskStatusOf(d); ok && s != core.TaskDone {
			open++
		}
	}
	bw := min(w, 22)
	if open > 0 {
		bw = min(w-5, 22)
		b.Text(r.X+2+bw+1, r.Y+2, fmt.Sprintf("⛓%d", open), st.Fg(t.Warn), 4)
	}
	drawBar(b, r.X+2, r.Y+2, bw, pct, lvl, label, st, t)
	if len(peers) > 0 {
		u := rep.Users[peers[0].UserID]
		if u != nil {
			b.Text(r.Right()-3, r.Y, u.Initials(), Sty{FG: t.BG, BG: t.userColor(u.ID), A: Bold}, 2)
		}
	}
}

func (bs *boardState) key(a *App, k tea.KeyPressMsg) tea.Cmd {
	cols, _ := bs.columns(a)
	switch k.Keystroke() {
	case "left":
		bs.col = max(0, bs.col-1)
	case "right":
		bs.col = min(4, bs.col+1)
	case "up":
		bs.idx--
	case "down":
		bs.idx++
	case "home":
		bs.idx = 0
	case "end":
		bs.idx = len(cols[bs.col]) - 1
	case "pgup":
		bs.idx -= 5
	case "pgdown":
		bs.idx += 5
	case "esc":
		return nil
	default:
		return nil
	}
	bs.idx = max(0, min(len(cols[bs.col])-1, bs.idx))
	bs.ensureVisible(a)
	return nil
}

func (bs *boardState) action(a *App, ac Action) tea.Cmd {
	switch ac {
	case ACardLeft, ACardRight:
		d := -1
		if ac == ACardRight {
			d = 1
		}
		return bs.moveCard(a, bs.curID(a), bs.col+d)
	case ASwimlanes:
		bs.lanes = !bs.lanes
		bs.laneScroll = 0
		bs.ensureVisible(a)
	case AScope:
		bs.scopeDialog(a)
	}
	return nil
}

// moveCard changes a task's status to column `to` and follows it.
func (bs *boardState) moveCard(a *App, id int64, to int) tea.Cmd {
	t := a.rep.Tasks[id]
	if t == nil || to < 0 || to > 4 || to == t.Status.Index() {
		return nil
	}
	ns := core.AllTaskStatuses[to]
	cmd := a.mutate([]core.Op{core.SetOp(core.EntTask, id, "status", string(t.Status), string(ns))},
		[]cellKey{{core.EntTask, id, "status"}}, fmt.Sprintf("%s → %s", a.rep.TaskRef(t), ns.Label()),
		func(a *App, res core.ResultMsg) tea.Cmd {
			// follow the card into its new column
			cols, _ := bs.columns(a)
			for i, x := range cols[to] {
				if x == id {
					bs.col, bs.idx = to, i
				}
			}
			bs.ensureVisible(a)
			return nil
		})
	return cmd
}

func (bs *boardState) scopeDialog(a *App) {
	f := newForm("Board scope", 66)
	prog := f.add(a.fieldInput(core.EntProject, "program_id", "Program", core.EdCombo, 0, nil))
	prog.AllowCreate = false
	proj := f.add(a.fieldInput(core.EntTask, "project_id", "Project", core.EdCombo, 0, nil))
	r := a.rep
	proj.ChoicesFn = func() []core.Choice {
		all := append([]core.Choice{{Value: nil, Label: "(all projects)", Style: core.Style{Kind: core.StMuted}}},
			core.Choices(r, core.EntTask, "project_id", 0, nil)...)
		pv, _ := prog.Value()
		pid := asIDPtr(pv)
		if pid == nil {
			return all
		}
		out := all[:1]
		for _, c := range all[1:] {
			if p := r.Projects[int64(must(core.AsFloat(c.Value)))]; p != nil && p.ProgramID != nil && *p.ProgramID == *pid {
				out = append(out, c)
			}
		}
		return out
	}
	var pv any
	if bs.program != nil {
		pv = *bs.program
	}
	prog.SetValue(pv, prog.allChoices())
	var jv any
	if bs.project != nil {
		jv = *bs.project
	}
	proj.SetValue(jv, proj.allChoices())
	prog.Hint = "(none) = all programs"
	f.onChange = func(a *App, f *form, i int) {
		if i == 0 {
			proj.Reload()
			proj.SetValue(nil, nil)
			proj.SetText("")
		}
	}
	f.buttons = []string{"Apply", "Cancel"}
	f.onSave = func(a *App, f *form) tea.Cmd {
		pv, _ := prog.Value()
		jv, _ := proj.Value()
		bs.program, bs.project = asIDPtr(pv), asIDPtr(jv)
		bs.col, bs.idx, bs.scroll, bs.laneScroll = 0, 0, [5]int{}, 0
		a.closeOverlay(f)
		return nil
	}
	a.push(f)
}

func (bs *boardState) click(a *App, h Hit, c tea.MouseClickMsg, dbl bool) tea.Cmd {
	switch h.Kind {
	case HitScope:
		bs.scopeDialog(a)
	case HitCard:
		bs.col, bs.idx = h.A, h.B
		if dbl {
			if id := bs.curID(a); id != 0 {
				a.recordForm(core.EntTask, id)
			}
			return nil
		}
		if c.Button == tea.MouseLeft {
			a.drag = &dragState{kind: HitCard, x0: c.X, y0: c.Y, x: c.X, y: c.Y, a: h.A, b: h.B}
		}
	case HitBoardCol:
		bs.col = h.A
		bs.clamp(a)
	}
	return nil
}

// drop completes a card drag onto another column.
func (bs *boardState) drop(a *App, d *dragState, hits []Hit) tea.Cmd {
	if !d.moved {
		return nil
	}
	h, ok := hitOfKind(hits, d.x, d.y, HitBoardCol)
	if !ok || h.A == d.a {
		return nil
	}
	cols, _ := bs.columns(a)
	if d.b >= len(cols[d.a]) {
		return nil
	}
	return bs.moveCard(a, cols[d.a][d.b], h.A)
}

func (bs *boardState) wheel(a *App, m hitMsg, d int) {
	if bs.lanes {
		bs.laneScroll = max(0, bs.laneScroll+d*2)
		return
	}
	if h, ok := hitOfKind(m.hits, m.m.X, m.m.Y, HitBoardCol); ok {
		bs.scroll[h.A] = max(0, bs.scroll[h.A]+d*2)
	}
}
