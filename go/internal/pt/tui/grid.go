package tui

// The reusable editable data grid (ui-spec §3): frozen columns, horizontal
// scrolling, multi-sort, collapsible grouping with subtotals, a totals
// footer, a selection gutter with presence initials, and per-cell states
// (current, conflict, pending, remote flash, someone else editing).

import (
	"fmt"
	"strings"
	"time"

	"git.wharf.fluidgrid.org/jcz/tictactoe/go/internal/pt/core"
)

const gutterW = 2

type cellKey struct {
	E  core.Entity
	ID int64
	F  string
}

type Grid struct {
	Name      string // view key, used for focus/presence
	Ent       core.Entity
	Cols      []core.Column // display order; Hidden columns are skipped
	Frozen    int           // leading visible columns that don't scroll
	Rows      []core.GridRow
	Sort      []core.SortSpec
	GroupBy   string
	Collapsed map[string]bool
	Row, Col  int // cursor: index into Rows, index into visible columns
	Top, Left int // first row shown; first scrolling visible column shown
	Marked    map[int64]bool
	anchor    int
	ReadOnly  bool
	IDs       func(r *core.Replica) []int64
	Totals    []*float64
	DataRows  int
	// Last rect the grid was laid out in (set by Update for scrolling).
	rect Rect
}

// tuneCols widens columns whose cells get a glyph prefix in the TUI.
func tuneCols(cols []core.Column) []core.Column {
	for i := range cols {
		switch cols[i].Key {
		case "status", "priority", "health":
			cols[i].Width += 2
		}
	}
	return cols
}

func NewGrid(name string, ent core.Entity, cols []core.Column, frozen int, ids func(*core.Replica) []int64) *Grid {
	return &Grid{Name: name, Ent: ent, Cols: tuneCols(cols), Frozen: frozen, IDs: ids,
		Collapsed: map[string]bool{}, Marked: map[int64]bool{}}
}

// vis returns the visible columns.
func (g *Grid) vis() []core.Column {
	out := make([]core.Column, 0, len(g.Cols))
	for _, c := range g.Cols {
		if !c.Hidden {
			out = append(out, c)
		}
	}
	return out
}

func (g *Grid) CurCol() (core.Column, bool) {
	v := g.vis()
	if g.Col >= 0 && g.Col < len(v) {
		return v[g.Col], true
	}
	return core.Column{}, false
}

// CurID is the id of the data row under the cursor (0 on a group row).
func (g *Grid) CurID() int64 {
	if g.Row >= 0 && g.Row < len(g.Rows) && !g.Rows[g.Row].IsGroup {
		return g.Rows[g.Row].ID
	}
	return 0
}

// SetColumns swaps the column set (custom fields changed), keeping
// visibility, order and widths the user chose.
func (g *Grid) SetColumns(cols []core.Column) {
	cols = tuneCols(cols)
	old := map[string]core.Column{}
	order := map[string]int{}
	for i, c := range g.Cols {
		old[c.Key] = c
		order[c.Key] = i
	}
	for i := range cols {
		if o, ok := old[cols[i].Key]; ok {
			cols[i].Hidden, cols[i].Width = o.Hidden, o.Width
		}
	}
	// keep the user's order for known columns, new ones go last
	known := []core.Column{}
	fresh := []core.Column{}
	for _, c := range cols {
		if _, ok := order[c.Key]; ok {
			known = append(known, c)
		} else {
			fresh = append(fresh, c)
		}
	}
	for i := 1; i < len(known); i++ {
		for j := i; j > 0 && order[known[j].Key] < order[known[j-1].Key]; j-- {
			known[j], known[j-1] = known[j-1], known[j]
		}
	}
	g.Cols = append(known, fresh...)
}

// Rebuild recomputes rows from the replica, keeping the cursor on the same id.
func (g *Grid) Rebuild(r *core.Replica, filter func(id int64) bool) {
	curID := g.CurID()
	curGroup := ""
	if g.Row >= 0 && g.Row < len(g.Rows) && g.Rows[g.Row].IsGroup {
		curGroup = g.Rows[g.Row].Key
	}
	ids := g.IDs(r)
	if filter != nil {
		kept := ids[:0:0]
		for _, id := range ids {
			if filter(id) {
				kept = append(kept, id)
			}
		}
		ids = kept
	}
	vis := g.vis()
	g.Rows = core.BuildRows(r, vis, ids, g.Sort, g.GroupBy, g.Collapsed)
	g.Totals = core.Totals(r, vis, ids)
	g.DataRows = len(ids)
	for id := range g.Marked {
		if !containsID(ids, id) {
			delete(g.Marked, id)
		}
	}
	found := false
	for i, row := range g.Rows {
		if (curID != 0 && !row.IsGroup && row.ID == curID) || (curGroup != "" && row.IsGroup && row.Key == curGroup) {
			g.Row, found = i, true
			break
		}
	}
	if !found {
		g.Row = min(g.Row, len(g.Rows)-1)
		if curID == 0 && curGroup == "" && g.Row == 0 && len(g.Rows) > 1 && g.Rows[0].IsGroup {
			g.Row = 1
		}
	}
	if g.Row < 0 {
		g.Row = 0
	}
	g.Col = max(0, min(g.Col, len(vis)-1))
}

func containsID(ids []int64, id int64) bool {
	for _, x := range ids {
		if x == id {
			return true
		}
	}
	return false
}

// ---- geometry ----

type colPos struct {
	vi   int // index into visible columns
	x, w int
}

// bodyRows is how many data rows fit in r (header + footer take 2).
func bodyRows(r Rect) int { return max(1, r.H-2) }

// scrollFor returns Top/Left so the cursor is visible in r (pure).
func (g *Grid) scrollFor(r Rect) (int, int) {
	top := g.Top
	n := bodyRows(r)
	if g.Row < top {
		top = g.Row
	}
	if g.Row >= top+n {
		top = g.Row - n + 1
	}
	top = max(0, min(top, max(0, len(g.Rows)-n)))
	vis := g.vis()
	frozen := min(g.Frozen, len(vis))
	left := max(g.Left, frozen)
	avail := r.W - gutterW - 1
	for i := 0; i < frozen; i++ {
		avail -= vis[i].Width + 1
	}
	if frozen > 0 {
		avail--
	}
	if g.Col >= frozen {
		if g.Col < left {
			left = g.Col
		}
		for left < g.Col {
			w := 0
			for i := left; i <= g.Col; i++ {
				w += vis[i].Width + 1
			}
			if w <= avail {
				break
			}
			left++
		}
	}
	return top, left
}

// Scroll persists scrollFor (Update calls this after moving the cursor).
func (g *Grid) Scroll(r Rect) {
	g.rect = r
	g.Top, g.Left = g.scrollFor(r)
}

func (g *Grid) layoutCols(r Rect, left int) []colPos {
	r.W-- // last column is the scrollbar
	vis := g.vis()
	frozen := min(g.Frozen, len(vis))
	x := r.X + gutterW
	var out []colPos
	for i := 0; i < frozen; i++ {
		w := min(vis[i].Width, r.Right()-x)
		if w <= 0 {
			break
		}
		out = append(out, colPos{i, x, w})
		x += vis[i].Width + 1
	}
	if frozen > 0 {
		x++ // frozen divider
	}
	for i := max(left, frozen); i < len(vis) && x < r.Right(); i++ {
		w := min(vis[i].Width, r.Right()-x)
		out = append(out, colPos{i, x, w})
		x += vis[i].Width + 1
	}
	return out
}

// ---- drawing ----

// gridEnv carries what the grid needs from the app to draw.
type gridEnv struct {
	r         *core.Replica
	t         *Theme
	focused   bool
	editing   bool // the current cell is under an editor
	conflicts map[cellKey]conflictInfo
	pending   map[cellKey]bool
	flash     map[core.RowKey]time.Time
	now       time.Time
	peers     map[int64][]core.Peer // row id → peers focusing it (this entity)
	relTime   bool                  // activity grid: "3m ago" instead of clock time
}

func (g *Grid) Draw(b *Buf, r Rect, env gridEnv) []Hit {
	t := env.t
	var hits []Hit
	if r.W < 4 || r.H < 2 {
		return nil
	}
	top, left := g.scrollFor(r)
	cols := g.layoutCols(r, left)
	vis := g.vis()
	frozen := min(g.Frozen, len(vis))

	// header
	hst := Sty{FG: t.FG, BG: t.Panel2, A: Bold}
	b.Fill(Rect{r.X, r.Y, r.W, 1}, ' ', hst)
	for _, cp := range cols {
		c := vis[cp.vi]
		title := c.Title
		for si, s := range g.Sort {
			if s.Key == c.Key {
				arrow := "▲"
				if s.Desc {
					arrow = "▼"
				}
				if len(g.Sort) > 1 {
					arrow += fmt.Sprint(si + 1)
				}
				title = Ellipsis(title, c.Width-StrW(arrow)) + arrow
			}
		}
		if c.Key == g.GroupBy {
			title = "⊞" + title
		}
		st := hst
		if cp.vi == g.Col && env.focused {
			st = st.Fg(t.Accent)
		}
		align := 0
		if c.Align == core.AlignRight {
			align = 1
		}
		b.Fit(cp.x, r.Y, title, st, cp.w, align)
		hits = append(hits, Hit{R: Rect{cp.x, r.Y, cp.w, 1}, Kind: HitHeader, A: cp.vi})
		// resize handle on the gap after the column
		hits = append(hits, Hit{R: Rect{cp.x + c.Width, r.Y, 1, 1}, Kind: HitColBorder, A: cp.vi})
	}
	if frozen > 0 && len(cols) > frozen {
		dx := cols[frozen-1].x + vis[frozen-1].Width
		for y := r.Y; y < r.Bottom()-1; y++ {
			b.Set(dx, y, '┃', Sty{FG: t.Border, BG: t.Panel2})
		}
	}

	// body
	n := bodyRows(r)
	for i := 0; i < n; i++ {
		ri := top + i
		y := r.Y + 1 + i
		rowRect := Rect{r.X, y, r.W, 1}
		if ri >= len(g.Rows) {
			b.Fill(rowRect, ' ', Sty{FG: t.FG, BG: t.Panel})
			continue
		}
		row := g.Rows[ri]
		if row.IsGroup {
			g.drawGroup(b, rowRect, row, cols, vis, ri == g.Row && env.focused, env)
			hits = append(hits, Hit{R: rowRect, Kind: HitGroup, A: ri})
			continue
		}
		bg := t.Panel
		if ri%2 == 1 {
			bg = t.Zebra
		}
		if g.Marked[row.ID] {
			bg = t.SelBG
		}
		if ri == g.Row {
			bg = t.CurBG
			if !env.focused {
				bg = mix(t.Panel, t.CurBG, 0.6)
			}
		}
		if ft, ok := env.flash[core.RowKey{Entity: g.Ent, ID: row.ID}]; ok && env.now.Sub(ft) < time.Second {
			bg = mix(bg, t.Accent2, 0.35)
		}
		base := Sty{FG: t.FG, BG: bg}
		b.Fill(rowRect, ' ', base)
		// gutter: mark, or another user's initial
		if ps := env.peers[row.ID]; len(ps) > 0 {
			u := env.r.Users[ps[0].UserID]
			ini := "?"
			if u != nil {
				ini = u.Initials()[:1]
			}
			b.Text(r.X, y, ini, Sty{FG: t.BG, BG: t.userColor(ps[0].UserID), A: Bold}, 1)
		}
		if g.Marked[row.ID] {
			b.Set(r.X+1, y, '●', base.Fg(t.Accent))
		}
		hits = append(hits, Hit{R: Rect{r.X, y, gutterW, 1}, Kind: HitGutter, A: ri})
		for _, cp := range cols {
			c := vis[cp.vi]
			cell := core.CellFor(env.r, &c, row.ID)
			if env.relTime && c.Key == "ts" {
				if act := env.r.Activity[row.ID]; act != nil {
					cell.Text = core.ActivityWhen(act, env.now.Unix())
				}
			}
			cur := ri == g.Row && cp.vi == g.Col
			st := base
			key := cellKey{g.Ent, row.ID, c.Field}
			conf, isConf := env.conflicts[key]
			if c.Field == "" {
				isConf = false
			}
			g.drawCell(b, cp, y, c, cell, st, cur && env.focused, isConf, env.pending[key] && c.Field != "", env)
			if isConf && cur {
				_ = conf
			}
			// someone else is on this cell
			for _, p := range env.peers[row.ID] {
				if p.Focus != nil && p.Focus.Field != nil && *p.Focus.Field == c.Field && c.Field != "" {
					b.Restyle(Rect{cp.x, y, cp.w, 1}, func(s Sty) Sty { return s.ULine(t.userColor(p.UserID)) })
				}
			}
			hits = append(hits, Hit{R: Rect{cp.x, y, cp.w, 1}, Kind: HitCell, A: ri, B: cp.vi})
		}
		if frozen > 0 && len(cols) > frozen {
			dx := cols[frozen-1].x + vis[frozen-1].Width
			b.Set(dx, y, '┃', Sty{FG: t.Border, BG: bg})
		}
	}

	// footer: row count + totals
	fy := r.Bottom() - 1
	fst := Sty{FG: t.Muted, BG: t.Panel2}
	b.Fill(Rect{r.X, fy, r.W, 1}, ' ', fst)
	label := fmt.Sprintf("Σ %d", g.DataRows)
	if nm := len(g.Marked); nm > 0 {
		label += fmt.Sprintf(" · %d marked", nm)
	}
	firstW := gutterW
	if len(cols) > 0 {
		firstW += cols[0].w
	}
	b.Fit(r.X, fy, label, fst.With(Bold), firstW, 0)
	for _, cp := range cols {
		if cp.vi == 0 {
			continue
		}
		if cp.vi < len(g.Totals) && g.Totals[cp.vi] != nil {
			b.Fit(cp.x, fy, core.FmtHours(*g.Totals[cp.vi]), fst.Fg(t.FG).With(Bold), cp.w, 1)
		}
	}
	// scrollbar
	if len(g.Rows) > n {
		pos := r.Y + 1 + top*(n-1)/max(1, len(g.Rows)-n)
		for y := r.Y + 1; y < r.Y+1+n; y++ {
			b.Set(r.Right()-1, y, '│', Sty{FG: t.Border, BG: t.Panel})
		}
		b.Set(r.Right()-1, min(pos, r.Y+n), '┃', Sty{FG: t.Accent, BG: t.Panel})
	}
	return hits
}

func (g *Grid) drawGroup(b *Buf, rr Rect, row core.GridRow, cols []colPos, vis []core.Column, cur bool, env gridEnv) {
	t := env.t
	st := Sty{FG: t.FG, BG: mix(t.Panel2, t.Accent, 0.08), A: Bold}
	if cur {
		st.BG = t.CurBG
	}
	b.Fill(rr, ' ', st)
	arrow := "▾ "
	if row.Collapsed {
		arrow = "▸ "
	}
	lst, glyph := t.cellStyle(row.Style, st)
	lst.A |= Bold
	label := arrow + glyph + row.Label
	x := rr.X
	x += b.Text(x, rr.Y, label, lst, rr.W-12)
	x += b.Text(x, rr.Y, fmt.Sprintf("  (%d)", row.Count), st.Fg(t.Muted), 8)
	// subtotals only where they don't collide with the label
	for _, cp := range cols {
		if cp.vi < len(row.Sums) && row.Sums[cp.vi] != nil && cp.x > x {
			b.Fit(cp.x, rr.Y, core.FmtHours(*row.Sums[cp.vi]), st.Fg(t.Accent), cp.w, 1)
		}
	}
}

func (g *Grid) drawCell(b *Buf, cp colPos, y int, c core.Column, cell core.Cell, base Sty, cur, conflict, pending bool, env gridEnv) {
	t := env.t
	st, glyph := t.cellStyle(cell.Style, base)
	if cur {
		st.BG = t.CellBG
		if cell.Style.Kind == core.StPlain || cell.Style.Kind == core.StStrong || cell.Style.Kind == core.StMuted {
			st.FG = t.CellFG
		}
		st.A |= Bold
		base.BG = t.CellBG
	}
	if pending {
		st.A |= Italic | Dim
	}
	if conflict {
		st = Sty{FG: t.CellFG, BG: t.Err, A: Bold}
		glyph = "⚠" + glyph
	}
	if cell.Style.Kind == core.StBar {
		drawBar(b, cp.x, y, cp.w, cell.Style.Pct, cell.Style.Level, cell.Text, st, t)
		return
	}
	text := cell.Text
	if c.Key == "tags" || c.Key == "options" {
		if text != "" {
			text = "#" + strings.ReplaceAll(text, " ", " #")
		}
	}
	align := 0
	if c.Align == core.AlignRight {
		align = 1
	}
	if glyph != "" && text != "" {
		text = glyph + text
	} else if conflict {
		text = glyph + text
	}
	fill := st
	if !cur && !conflict {
		fill = base
	}
	b.Fit(cp.x, y, "", fill, cp.w, 0)
	b.Fit(cp.x, y, text, st, cp.w, align)
}

// drawBar renders a progress bar with eighth-block resolution plus a label.
func drawBar(b *Buf, x, y, w int, pct float64, level int, label string, st Sty, t *Theme) {
	lw := StrW(label) + 1
	bw := w - lw
	color := t.OK
	switch level {
	case 1:
		color = t.Warn
	case 2:
		color = t.Err
	}
	if label == "" {
		bw = w
	}
	b.Fit(x, y, "", st, w, 0)
	if bw >= 3 {
		eighths := int(min(pct, 100)/100*float64(bw*8) + 0.5)
		blocks := []rune(" ▏▎▍▌▋▊▉")
		track := Sty{FG: color, BG: mix(st.BG, t.Muted, 0.25)}
		for i := 0; i < bw; i++ {
			fill := eighths - i*8
			switch {
			case fill >= 8:
				b.Set(x+i, y, '█', track)
			case fill > 0:
				b.Set(x+i, y, blocks[fill], track)
			default:
				b.Set(x+i, y, ' ', track)
			}
		}
		b.Text(x+bw+1, y, label, st, lw-1)
	} else {
		b.Fit(x, y, label, st.Fg(color), w, 1)
	}
}

// ---- cursor movement ----

func (g *Grid) Move(dr, dc int) {
	nv := len(g.vis())
	g.Row = max(0, min(len(g.Rows)-1, g.Row+dr))
	g.Col = max(0, min(nv-1, g.Col+dc))
}

// Next moves right, wrapping to the next row (Tab).
func (g *Grid) Next(back bool) {
	nv := len(g.vis())
	if back {
		if g.Col > 0 {
			g.Col--
		} else if g.Row > 0 {
			g.Row--
			g.Col = nv - 1
		}
		return
	}
	if g.Col < nv-1 {
		g.Col++
	} else if g.Row < len(g.Rows)-1 {
		g.Row++
		g.Col = 0
	}
}

// ToggleGroup collapses/expands the group under the cursor.
func (g *Grid) ToggleGroup(collapse *bool) bool {
	if g.Row >= len(g.Rows) || !g.Rows[g.Row].IsGroup {
		return false
	}
	k := g.Rows[g.Row].Key
	v := !g.Collapsed[k]
	if collapse != nil {
		v = *collapse
	}
	if v {
		g.Collapsed[k] = true
	} else {
		delete(g.Collapsed, k)
	}
	return true
}

// MarkedIDs in display order (or the cursor row when nothing is marked).
func (g *Grid) MarkedIDs() []int64 {
	var out []int64
	for _, row := range g.Rows {
		if !row.IsGroup && g.Marked[row.ID] {
			out = append(out, row.ID)
		}
	}
	if len(out) == 0 {
		if id := g.CurID(); id != 0 {
			out = []int64{id}
		}
	}
	return out
}

// CycleSort: ascending → descending → off for a column (ui-spec ^O).
func (g *Grid) CycleSort(key string, add bool) {
	for i, s := range g.Sort {
		if s.Key == key {
			if !s.Desc {
				g.Sort[i].Desc = true
			} else {
				g.Sort = append(g.Sort[:i], g.Sort[i+1:]...)
			}
			return
		}
	}
	spec := core.SortSpec{Key: key}
	if add {
		g.Sort = append(g.Sort, spec)
		if len(g.Sort) > 3 {
			g.Sort = g.Sort[1:]
		}
	} else {
		g.Sort = []core.SortSpec{spec}
	}
}

// ColIndex finds a visible column by key.
func (g *Grid) ColIndex(key string) int {
	for i, c := range g.vis() {
		if c.Key == key {
			return i
		}
	}
	return -1
}

// ResizeVisible sets the width of visible column vi.
func (g *Grid) ResizeVisible(vi, w int) {
	v := g.vis()
	if vi < 0 || vi >= len(v) {
		return
	}
	key := v[vi].Key
	for i := range g.Cols {
		if g.Cols[i].Key == key {
			g.Cols[i].Width = max(3, min(80, w))
		}
	}
}

// CellRect is where the current cell is drawn (for anchoring editors).
func (g *Grid) CellRect() (Rect, bool) {
	r := g.rect
	top, left := g.scrollFor(r)
	for _, cp := range g.layoutCols(r, left) {
		if cp.vi == g.Col {
			y := r.Y + 1 + g.Row - top
			if y > r.Y && y < r.Bottom()-1 {
				return Rect{cp.x, y, cp.w, 1}, true
			}
		}
	}
	return Rect{}, false
}
