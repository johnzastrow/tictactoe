package tui

// Tabs 14 Charts, 15 Kanban and 16 Gantt: the drawn controls.

import (
	"fmt"
	"math"
	"strconv"

	tea "charm.land/bubbletea/v2"

	"git.wharf.fluidgrid.org/jcz/tictactoe/go/internal/demo/core"
)

// ================= 14 Charts =================

type chartsTab struct {
	ctlBase
	month int
}

func newChartsTab() *chartsTab { return &chartsTab{} }

func (t *chartsTab) explain() string {
	return "One series of 12 monthly values drawn four ways: bars, a sparkline, a gauge of the average and a heat strip."
}

func (t *chartsTab) controls() int { return 1 }

func (t *chartsTab) keyHelp() [][2]string {
	return [][2]string{{"←→", "month"}, {"↑↓", "±5"}, {"PgUp/PgDn", "±20"}, {"Home/End", "Jan / Dec"}, {"click", "select a bar"}}
}

func (t *chartsTab) hints(a *App) []hint {
	return []hint{{"←→", "Month", ""}, {"↑↓", "±5", ""}, {"PgUp/Dn", "±20", ""}}
}

func (t *chartsTab) bump(a *App, d int64) {
	s := a.vals.Series()
	if len(s) != 12 {
		return
	}
	v := max(0, min(100, s[t.month]+d))
	if v == s[t.month] {
		return
	}
	s[t.month] = v
	if err := a.commit("charts.series", s); err != nil && err != errOffline {
		a.setStatus("✗ "+err.Error(), kErr)
	}
}

func (t *chartsTab) key(a *App, k tea.KeyPressMsg) bool {
	switch k.Keystroke() {
	case "left":
		t.month = (t.month + 11) % 12
	case "right":
		t.month = (t.month + 1) % 12
	case "home":
		t.month = 0
	case "end":
		t.month = 11
	case "up", "+":
		t.bump(a, 5)
	case "down", "-":
		t.bump(a, -5)
	case "pgup":
		t.bump(a, 20)
	case "pgdown":
		t.bump(a, -20)
	default:
		return false
	}
	return true
}

var eighths = []rune(" ▁▂▃▄▅▆▇█")

// gaugeColor: green < 60 ≤ amber < 80 ≤ red.
func gaugeColor(th *Theme, v int64) Color {
	switch {
	case v >= 80:
		return th.Err
	case v >= 60:
		return th.Warn
	}
	return th.OK
}

func (t *chartsTab) draw(a *App, b *Buf, r Rect, focused bool) []Hit {
	th := a.th()
	s := a.vals.Series()
	if len(s) != 12 {
		return nil
	}
	var hits []Hit
	base := th.base()
	_, flashing := a.flash["charts.series"]
	ch := max(3, r.H-4) // bar rows (value labels sit on top of the bars)
	bw := max(2, min(5, (r.W-6)/12-1))
	x0 := r.X + 4
	// y axis: 0, 50, 100
	for _, v := range []int{0, 50, 100} {
		y := r.Y + ch - 1 - (ch-1)*v/100
		b.Text(r.X, y, fmt.Sprintf("%3d", v), base.Fg(th.Muted), 3)
	}
	for i, v := range s {
		x := x0 + i*(bw+1)
		sel := i == t.month
		c := choose(sel, th.Accent, mix(th.Accent, th.BG, 0.45))
		if sel && flashing {
			c = th.Warn
		}
		units := int(v) * (ch - 1) * 8 / 100 // leave the top row for the label
		for row := 0; row < ch; row++ {
			y := r.Y + ch - 1 - row
			n := max(0, min(8, units-row*8))
			if n == 0 {
				if row > 0 && (units+7)/8 == row {
					b.Fit(x, y, strconv.FormatInt(v, 10), base.Fg(choose(sel, th.FG, th.Muted)).With(choose(sel, Bold, Attr(0))), bw, 2)
				}
				continue
			}
			for dx := 0; dx < bw; dx++ {
				b.Set(x+dx, y, eighths[n], base.Fg(c))
			}
		}
		if units == 0 {
			b.Fit(x, r.Y+ch-2, strconv.FormatInt(v, 10), base.Fg(th.Muted), bw, 2)
		}
		ms := base.Fg(choose(sel, th.Accent, th.Muted))
		if sel {
			ms = ms.With(Bold | Underline)
		}
		b.Fit(x, r.Y+ch, core.MonthNames[i][:min(bw, 3)], ms, bw, 2)
		hits = append(hits, Hit{R: Rect{x, r.Y, bw + 1, ch + 1}, Kind: HitCtl, S: "bar", A: i})
	}
	// sparkline, heat strip and gauge underneath
	y := r.Y + ch + 1
	lbl := func(y int, s string) int { return b.Text(r.X, y, s, base.Fg(th.Muted), 9) }
	if y < r.Bottom() {
		lbl(y, "Trend")
		for i, v := range s {
			st := base.Fg(choose(i == t.month, th.Warn, th.Accent2))
			b.Set(r.X+10+i, y, eighths[max(1, min(8, int(v)*8/100+1))], st)
		}
		sel := fmt.Sprintf("%s = %d", core.MonthNames[t.month], s[t.month])
		b.Text(r.X+24, y, sel, base.With(Bold), 12)
		// heat strip on the same row when there is room
		hx := r.X + 38
		if hx+12*3 <= r.Right() {
			b.Text(hx, y, "Heat", base.Fg(th.Muted), 5)
			for i, v := range s {
				c := mix(th.Panel2, th.Err, float64(v)/100)
				st := Sty{FG: th.FG, BG: c}
				if i == t.month {
					st = st.With(Bold | Underline)
				}
				b.Fit(hx+5+i*3, y, choose(i == t.month, "▴", ""), st, 3, 2)
				hits = append(hits, Hit{R: Rect{hx + 5 + i*3, y, 3, 1}, Kind: HitCtl, S: "bar", A: i})
			}
		} else if y+2 < r.Bottom() {
			y++
			lbl(y, "Heat")
			for i, v := range s {
				b.Fit(r.X+10+i*3, y, "", Sty{FG: th.FG, BG: mix(th.Panel2, th.Err, float64(v)/100)}, 3, 0)
			}
		}
	}
	y++
	if y < r.Bottom() {
		var sum int64
		for _, v := range s {
			sum += v
		}
		avg := float64(sum) / 12
		lbl(y, "Average")
		gw := max(10, min(40, r.W-24))
		fill := int(math.Round(avg * float64(gw) / 100))
		gc := gaugeColor(th, int64(math.Round(avg)))
		for i := 0; i < gw; i++ {
			b.Set(r.X+10+i, y, choose(i < fill, '█', '░'), base.Fg(choose(i < fill, gc, th.Border)))
		}
		b.Text(r.X+11+gw, y, fmt.Sprintf("%.1f", avg), base.Fg(gc).With(Bold), 6)
	}
	return hits
}

func (t *chartsTab) mouse(a *App, m hitMsg) {
	if d, ok := isWheel(m); ok {
		t.bump(a, int64(-5*d))
		return
	}
	if isClick(m) && m.hit.S == "bar" {
		t.month = m.hit.A
	}
}

// ================= 15 Kanban =================

type kanbanTab struct {
	ctlBase
	col    int
	card   [4]int // selected card per column
	target int    // drag: column under the pointer (−1 none)
	colR   [4]Rect
}

func newKanbanTab() *kanbanTab { return &kanbanTab{target: -1} }

func (t *kanbanTab) explain() string {
	return "A board of cards in four columns: move cards with ⇧←/⇧→ or by dragging; Ins adds, Enter renames, F8 deletes."
}

func (t *kanbanTab) controls() int { return 1 }

func (t *kanbanTab) keyHelp() [][2]string {
	return [][2]string{
		{"←→", "column"}, {"↑↓", "card"}, {"⇧←/⇧→ < >", "move card"}, {"Ins", "new card"}, {"Enter", "rename"},
		{"F8 ^Del", "delete"}, {"drag", "move card"}, {"click", "select"},
	}
}

func (t *kanbanTab) hints(a *App) []hint {
	return []hint{{"⇧←→", "Move card", ""}, {"<", "Move ◂", "<"}, {">", "Move ▸", ">"}, {"Ins", "New", "insert"}, h("enter", "Rename"), {"F8", "Delete", "f8"}}
}

// column returns the cards of board column c (stored order).
func column(cards []core.Card, c int) []core.Card {
	var out []core.Card
	for _, cd := range cards {
		if cd.Col == core.KanbanColumns[c].ID {
			out = append(out, cd)
		}
	}
	return out
}

func colIndex(id string) int {
	for i, c := range core.KanbanColumns {
		if c.ID == id {
			return i
		}
	}
	return 0
}

func (t *kanbanTab) cur(a *App) (core.Card, bool) {
	cs := column(a.vals.Cards(), t.col)
	if len(cs) == 0 {
		return core.Card{}, false
	}
	t.card[t.col] = max(0, min(t.card[t.col], len(cs)-1))
	return cs[t.card[t.col]], true
}

func (t *kanbanTab) save(a *App, cards []core.Card) {
	if cards == nil {
		cards = []core.Card{}
	}
	if err := a.commit("kanban.cards", cards); err != nil && err != errOffline {
		a.setStatus("✗ "+err.Error(), kErr)
	}
}

// moveCard puts card id in column c (keeping the stored order) and selects it.
func (t *kanbanTab) moveCard(a *App, id int64, c int) {
	c = max(0, min(3, c))
	cards := a.vals.Cards()
	for i := range cards {
		if cards[i].ID == id {
			if colIndex(cards[i].Col) == c {
				return
			}
			cards[i].Col = core.KanbanColumns[c].ID
		}
	}
	t.save(a, cards)
	t.col = c
	for i, cd := range column(a.vals.Cards(), c) {
		if cd.ID == id {
			t.card[c] = i
		}
	}
}

func (t *kanbanTab) key(a *App, k tea.KeyPressMsg) bool {
	cd, ok := t.cur(a)
	switch keyName(k) {
	case "left":
		t.col = max(0, t.col-1)
	case "right":
		t.col = min(3, t.col+1)
	case "up":
		t.card[t.col] = max(0, t.card[t.col]-1)
	case "down":
		t.card[t.col]++
		t.cur(a)
	case "home":
		t.card[t.col] = 0
	case "end":
		t.card[t.col] = 1 << 20
		t.cur(a)
	case "shift+left", "<", ",":
		if ok {
			t.moveCard(a, cd.ID, t.col-1)
		}
	case "shift+right", ">", ".":
		if ok {
			t.moveCard(a, cd.ID, t.col+1)
		}
	case "insert", "ctrl+n":
		col := t.col
		a.prompt("New card in "+core.KanbanColumns[col].Label, "Title", "", func(a *App, s string) error {
			cards := a.vals.Cards()
			ids := make([]int64, len(cards))
			for i, c := range cards {
				ids[i] = c.ID
			}
			cards = append(cards, core.Card{ID: core.NextID(ids), Title: s, Col: core.KanbanColumns[col].ID})
			if _, err := a.vals.PrepareSet("kanban.cards", cards); err != nil {
				return err
			}
			t.save(a, cards)
			t.card[col] = len(column(cards, col)) - 1
			return nil
		})
	case "enter", "f2":
		if !ok {
			return true
		}
		id := cd.ID
		a.prompt("Rename card", "Title", core.SafeText(cd.Title), func(a *App, s string) error {
			cards := a.vals.Cards()
			for i := range cards {
				if cards[i].ID == id {
					cards[i].Title = s
				}
			}
			if _, err := a.vals.PrepareSet("kanban.cards", cards); err != nil {
				return err
			}
			t.save(a, cards)
			return nil
		})
	case "f8", "ctrl+delete", "delete":
		if !ok {
			return true
		}
		id := cd.ID
		a.confirm("Delete card", fmt.Sprintf("Delete card %q?", core.SafeText(cd.Title)), "Delete", func(a *App) {
			var out []core.Card
			for _, c := range a.vals.Cards() {
				if c.ID != id {
					out = append(out, c)
				}
			}
			t.save(a, out)
		})
	default:
		return false
	}
	return true
}

func (t *kanbanTab) draw(a *App, b *Buf, r Rect, focused bool) []Hit {
	th := a.th()
	cards := a.vals.Cards()
	var hits []Hit
	cw := (r.W - 3) / 4
	_, flashing := a.flash["kanban.cards"]
	for c := 0; c < 4; c++ {
		cr := Rect{r.X + c*(cw+1), r.Y, cw, r.H}
		t.colR[c] = cr
		cs := column(cards, c)
		hs := Sty{FG: th.FG, BG: th.Panel2, A: Bold}
		if c == t.col && focused {
			hs = Sty{FG: th.CellFG, BG: th.CellBG, A: Bold}
		}
		if c == t.target {
			hs = Sty{FG: th.BG, BG: th.Warn, A: Bold}
		}
		b.Fill(Rect{cr.X, cr.Y, cr.W, 1}, ' ', hs)
		b.Text(cr.X+1, cr.Y, core.KanbanColumns[c].Label, hs, cr.W-6)
		cnt := strconv.Itoa(len(cs))
		b.Text(cr.Right()-len(cnt)-1, cr.Y, cnt, hs, len(cnt))
		body := Rect{cr.X, cr.Y + 1, cr.W, cr.H - 1}
		b.Fill(body, ' ', Sty{FG: th.FG, BG: th.Panel})
		hits = append(hits, Hit{R: cr, Kind: HitCtl, S: "col", A: c})
		step := choose(len(cs)*2 <= body.H, 2, 1)
		sel := max(0, min(t.card[c], len(cs)-1))
		top := 0
		if rows := body.H / step; sel >= rows {
			top = sel - rows + 1
		}
		colColor := []Color{th.Muted, th.Info, th.Accent2, th.OK}[c]
		for i := top; i < len(cs); i++ {
			y := body.Y + (i-top)*step
			if y >= body.Bottom() {
				b.Text(cr.Right()-3, body.Bottom()-1, "▼", Sty{FG: th.Accent, BG: th.Panel}, 1)
				break
			}
			cd := cs[i]
			cur := c == t.col && i == sel
			st := Sty{FG: th.FG, BG: th.Panel2}
			if cur {
				st = choose(focused, th.sel().With(Bold), Sty{FG: th.FG, BG: th.SelBG, A: Bold})
			}
			if flashing && !cur {
				st = st.Bg(mix(st.BG, th.Warn, 0.2))
			}
			row := Rect{cr.X + 1, y, cr.W - 2, 1}
			b.Fill(row, ' ', st)
			b.Set(row.X, y, '▌', st.Fg(colColor))
			id := "#" + strconv.FormatInt(cd.ID, 10)
			if cw < 20 {
				id = "" // narrow columns: the title matters more
			}
			b.Text(row.X+2, y, core.SafeText(cd.Title), st, row.W-len(id)-3)
			b.Text(row.Right()-len(id)-1, y, id, st.Fg(choose(cur && focused, th.CellFG, th.Muted)), len(id))
			hits = append(hits, Hit{R: row, Kind: HitCtl, S: "card", A: c, B: i})
		}
		if len(cs) == 0 {
			b.Text(body.X+2, body.Y, "(empty)", Sty{FG: th.Muted, BG: th.Panel, A: Italic}, cw-4)
		}
	}
	return hits
}

func (t *kanbanTab) mouse(a *App, m hitMsg) {
	h := m.hit
	if !isClick(m) || h.S != "card" {
		if isClick(m) && h.S == "col" {
			t.col = h.A
		}
		return
	}
	t.col, t.card[h.A] = h.A, h.B
	cd, ok := t.cur(a)
	if !ok {
		return
	}
	id := cd.ID
	a.drag = &dragState{x0: m.m.X, y0: m.m.Y,
		move: func(a *App, x, y int) {
			t.target = -1
			for c, r := range t.colR {
				if r.Contains(x, y) {
					t.target = c
				}
			}
		},
		drop: func(a *App, x, y int, hits []Hit) {
			t.target = -1
			for c, r := range t.colR {
				if r.Contains(x, y) {
					t.moveCard(a, id, c)
				}
			}
		}}
}

// ================= 16 Gantt =================

type ganttTab struct {
	ctlBase
	sel      int
	zoom     int // 0 day, 1 week, 2 month
	origin   core.Date
	set      bool
	area     Rect // timeline area (last drawn)
	dragID   int64
	dragMove int // preview: days moved
	dragSize int // preview: days added
}

func newGanttTab() *ganttTab { return &ganttTab{zoom: 1} }

var zoomNames = []string{"day", "week", "month"}

// cpd is columns per day at the zoom level.
func (t *ganttTab) cpd() float64 { return []float64{3, 1, 0.25}[t.zoom] }

func (t *ganttTab) explain() string {
	return "Bars on a time axis with week ticks, weekend shading and a today line: ⇧←/⇧→ move, [ ] resize, + - zoom, drag with the mouse."
}

func (t *ganttTab) controls() int { return 1 }

func (t *ganttTab) keyHelp() [][2]string {
	return [][2]string{
		{"↑↓", "bar"}, {"⇧←/⇧→ < >", "move ±1 day"}, {"[ ]", "shrink / extend"}, {"+ -", "zoom"},
		{"t", "today"}, {"←→", "scroll"}, {"drag", "move"}, {"drag end", "resize"},
	}
}

func (t *ganttTab) hints(a *App) []hint {
	return []hint{{"↑↓", "Bar", ""}, {"<", "Move ◂", "<"}, {">", "Move ▸", ">"}, {"[", "Shrink", "["}, {"]", "Extend", "]"},
		{"+", "Zoom in", "+"}, {"-", "Zoom out", "-"}, {"t", "Today", "t"}}
}

func (t *ganttTab) note(a *App) string {
	bars := a.vals.Bars()
	if t.sel >= len(bars) || len(bars) == 0 {
		return ""
	}
	bar := bars[t.sel]
	s, ok := core.ParseISO(bar.Start)
	if !ok {
		return ""
	}
	e := s.AddDays(int(bar.Days) - 1)
	return fmt.Sprintf("%s · %s → %s %d · %d day%s", core.SafeText(bar.Label), s.Short(), e.Short(), e.Year(), bar.Days, choose(bar.Days == 1, "", "s"))
}

func (t *ganttTab) save(a *App, bars []core.Bar) {
	if err := a.commit("gantt.bars", bars); err != nil && err != errOffline {
		a.setStatus("✗ "+err.Error(), kErr)
	}
}

// change shifts the selected bar's start by move days and its length by size.
func (t *ganttTab) change(a *App, id int64, move, size int) {
	bars := a.vals.Bars()
	for i := range bars {
		if bars[i].ID != id {
			continue
		}
		s, ok := core.ParseISO(bars[i].Start)
		if !ok {
			return
		}
		bars[i].Start = s.AddDays(move).ISO()
		bars[i].Days = max(1, min(365, bars[i].Days+int64(size)))
	}
	t.save(a, bars)
}

func (t *ganttTab) selID(a *App) (int64, bool) {
	bars := a.vals.Bars()
	if len(bars) == 0 {
		return 0, false
	}
	t.sel = max(0, min(t.sel, len(bars)-1))
	return bars[t.sel].ID, true
}

// scrollToday puts today a quarter of the way into the view.
func (t *ganttTab) scrollToday(a *App) {
	days := int(float64(max(20, t.area.W)) / t.cpd())
	t.origin = a.today.AddDays(-days / 4).WeekStart()
	t.set = true
}

func (t *ganttTab) key(a *App, k tea.KeyPressMsg) bool {
	id, ok := t.selID(a)
	page := int(float64(max(8, t.area.W)) / t.cpd() / 4)
	switch keyName(k) {
	case "up":
		t.sel = max(0, t.sel-1)
	case "down":
		t.sel = min(len(a.vals.Bars())-1, t.sel+1)
	case "shift+left", "<", ",":
		if ok {
			t.change(a, id, -1, 0)
		}
	case "shift+right", ">", ".":
		if ok {
			t.change(a, id, 1, 0)
		}
	case "[":
		if ok {
			t.change(a, id, 0, -1)
		}
	case "]":
		if ok {
			t.change(a, id, 0, 1)
		}
	case "+", "=":
		t.zoom = max(0, t.zoom-1)
		a.info("Zoom: " + zoomNames[t.zoom])
	case "-", "_":
		t.zoom = min(2, t.zoom+1)
		a.info("Zoom: " + zoomNames[t.zoom])
	case "t", "T":
		t.scrollToday(a)
	case "left":
		t.origin = t.origin.AddDays(-max(1, page))
	case "right":
		t.origin = t.origin.AddDays(max(1, page))
	case "home":
		t.set = false
	default:
		return false
	}
	return true
}

func (t *ganttTab) draw(a *App, b *Buf, r Rect, focused bool) []Hit {
	th := a.th()
	bars := a.vals.Bars()
	var hits []Hit
	base := th.base()
	labelW := min(16, r.W/5)
	area := Rect{r.X + labelW + 1, r.Y, r.W - labelW - 1, r.H}
	t.area = area
	if !t.set {
		first := a.today
		for _, bar := range bars {
			if s, ok := core.ParseISO(bar.Start); ok && s < first {
				first = s
			}
		}
		t.origin = first.AddDays(-3).WeekStart()
		t.set = true
	}
	cpd := t.cpd()
	dayAt := func(x int) core.Date { return t.origin.AddDays(int(math.Floor(float64(x-area.X) / cpd))) }
	xOf := func(d core.Date) int { return area.X + int(math.Floor(float64(int(d)-int(t.origin))*cpd)) }
	// axis: labels at week (month) starts, ticks under them
	b.Text(r.X, r.Y, "zoom: "+zoomNames[t.zoom], base.Fg(th.Muted), labelW)
	lastLabel := -100
	for x := area.X; x < area.Right(); x++ {
		d := dayAt(x)
		if x > area.X && dayAt(x-1) == d {
			continue // same day (day zoom: 3 columns per day)
		}
		tick := false
		label := ""
		switch t.zoom {
		case 2:
			if d.Day() <= 4 && (x == area.X || dayAt(x-1).Month() != d.Month()) {
				tick, label = true, core.MonthNames[d.Month()-1]
			}
		default:
			if d.Weekday() == 0 {
				tick, label = true, d.Short()
			}
		}
		if t.zoom == 0 {
			b.Text(x+1, r.Y+1, string("MTWTFSS"[d.Weekday()]), base.Fg(choose(d.Weekday() >= 5, th.Muted, th.FG)), 1)
		} else if tick {
			b.Set(x, r.Y+1, '╷', base.Fg(th.Border))
		}
		if tick && x-lastLabel > StrW(label) {
			b.Text(x, r.Y, label, base.Fg(th.Muted), area.Right()-x)
			lastLabel = x
		}
	}
	// rows
	body := Rect{r.X, r.Y + 2, r.W, r.H - 2}
	_, flashing := a.flash["gantt.bars"]
	for i, bar := range bars {
		y := body.Y + i
		if y >= body.Bottom() {
			break
		}
		cur := i == t.sel
		ls := base.Fg(choose(cur, th.Accent, th.FG))
		if cur {
			ls = ls.With(Bold)
		}
		if cur && focused {
			ls = th.sel().With(Bold)
		}
		b.Fit(r.X, y, " "+core.SafeText(bar.Label), ls, labelW, 0)
		hits = append(hits, Hit{R: Rect{r.X, y, labelW, 1}, Kind: HitCtl, S: "label", A: i})
		// background: weekend shading, zebra
		for x := area.X; x < area.Right(); x++ {
			d := dayAt(x)
			bg := choose(i%2 == 1, th.Zebra, th.BG)
			if t.zoom < 2 && d.Weekday() >= 5 {
				bg = th.Panel2
			}
			if cur {
				bg = mix(bg, th.SelBG, 0.6)
			}
			b.Set(x, y, ' ', Sty{FG: th.FG, BG: bg})
		}
		s, ok := core.ParseISO(bar.Start)
		if !ok {
			continue
		}
		days := int(bar.Days)
		if bar.ID == t.dragID && a.drag != nil {
			s = s.AddDays(t.dragMove)
			days = max(1, days+t.dragSize)
		}
		x0, x1 := xOf(s), xOf(s.AddDays(days))
		x1 = max(x1, x0+1)
		col := th.Users[int(bar.ID-1+8)%8]
		if cur {
			col = choose(focused, th.Accent, mix(th.Accent, th.FG, 0.2))
		}
		if flashing && cur {
			col = th.Warn
		}
		for x := max(x0, area.X); x < min(x1, area.Right()); x++ {
			_, cs := b.At(x, y)
			b.Set(x, y, '█', cs.Fg(col))
		}
		if x1-1 >= area.X && x1-1 < area.Right() && cur {
			b.Set(x1-1, y, '▐', Sty{FG: th.Warn, BG: col}) // the resize handle
		}
		dl := fmt.Sprintf("%dd", days)
		if x1+1+len(dl) < area.Right() && x1 >= area.X {
			b.Text(x1+1, y, dl, base.Fg(th.Muted), len(dl))
		}
		if x1 > area.X && x0 < area.Right() {
			vis := Rect{max(x0, area.X), y, min(x1, area.Right()) - max(x0, area.X), 1}
			hits = append(hits, Hit{R: vis, Kind: HitCtl, S: "bar", A: i})
			if x1-1 < area.Right() {
				hits = append(hits, Hit{R: Rect{x1 - 1, y, 1, 1}, Kind: HitCtl, S: "end", A: i})
			}
		}
	}
	// today line over everything
	if tx := xOf(a.today); tx >= area.X && tx < area.Right() {
		for y := r.Y + 1; y < min(body.Bottom(), body.Y+max(1, len(bars))); y++ {
			_, cs := b.At(tx, y)
			ch, _ := b.At(tx, y)
			if ch == ' ' || ch == '╷' || y == r.Y+1 {
				b.Set(tx, y, '│', cs.Fg(th.Err))
			} else {
				b.Restyle(Rect{tx, y, 1, 1}, func(s Sty) Sty { return s.Fg(th.Err) })
			}
		}
		b.Text(min(tx, area.Right()-5), body.Y+len(bars), "today", base.Fg(th.Err), 5)
	}
	hits = append([]Hit{{R: area, Kind: HitCtl, S: "area"}}, hits...)
	return hits
}

func (t *ganttTab) mouse(a *App, m hitMsg) {
	h := m.hit
	if d, ok := isWheel(m); ok {
		t.origin = t.origin.AddDays(d * 7)
		return
	}
	if !isClick(m) {
		return
	}
	switch h.S {
	case "label":
		t.sel = h.A
	case "bar", "end":
		t.sel = h.A
		bars := a.vals.Bars()
		if h.A >= len(bars) {
			return
		}
		id := bars[h.A].ID
		resize := h.S == "end"
		cpd := t.cpd()
		x0 := m.m.X
		t.dragID, t.dragMove, t.dragSize = id, 0, 0
		days := func(x int) int { return int(math.Round(float64(x-x0) / cpd)) }
		a.drag = &dragState{x0: m.m.X, y0: m.m.Y,
			move: func(a *App, x, _ int) {
				if resize {
					t.dragSize = days(x)
				} else {
					t.dragMove = days(x)
				}
			},
			drop: func(a *App, x, _ int, _ []Hit) {
				mv, sz := t.dragMove, t.dragSize
				t.dragID, t.dragMove, t.dragSize = 0, 0, 0
				if resize {
					sz = days(x)
				} else {
					mv = days(x)
				}
				if mv != 0 || sz != 0 {
					t.change(a, id, mv, sz) // drags commit on release
				}
			}}
	}
}
