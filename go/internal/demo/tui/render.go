package tui

// frame draws the whole screen into a buffer and returns the hit regions.
// It only reads state (tabs may remember scroll offsets), so View and mouse
// handling can both call it.

import (
	"fmt"
	"strings"

	"git.wharf.fluidgrid.org/jcz/tictactoe/go/internal/demo/core"
)

const (
	minW, minH = 80, 24
	sideW      = 16 // sidebar width, the border is the next column
)

func (a *App) th() *Theme { return &themes[a.theme] }

type layoutT struct {
	menu, side, body, status, hints Rect
	// inside body:
	title, ctl, keys, value Rect
}

func (a *App) hintLines() int {
	if a.h >= 30 {
		return 2
	}
	return 1
}

// layout is a pure function of the window size and the current tab.
func (a *App) layout() layoutT {
	w, h := a.w, a.h
	var l layoutT
	hl := a.hintLines()
	l.menu = Rect{0, 0, w, 1}
	l.hints = Rect{0, h - hl, w, hl}
	l.status = Rect{0, h - hl - 1, w, 1}
	l.side = Rect{0, 1, sideW, h - hl - 2}
	l.body = Rect{sideW + 2, 1, w - sideW - 3, h - hl - 2}
	ex := wrap(a.curTab().explain(), l.body.W)
	l.title = Rect{l.body.X, l.body.Y, l.body.W, 1 + len(ex)}
	top := l.title.Bottom() + 1
	bottom := l.body.Bottom()
	ph := a.panelHeight(l.body.W)
	if ph > 0 {
		py := bottom - ph
		switch {
		case a.hideKeys:
			l.value = Rect{l.body.X, py, l.body.W, ph}
		case a.hideVal:
			l.keys = Rect{l.body.X, py, l.body.W, ph}
		default:
			kw := l.body.W * 9 / 20
			l.keys = Rect{l.body.X, py, kw, ph}
			l.value = Rect{l.body.X + kw + 1, py, l.body.W - kw - 1, ph}
		}
		bottom = py - 1
	}
	l.ctl = Rect{l.body.X, top, l.body.W, max(1, bottom-top)}
	return l
}

// panelHeight is the height (borders included) of the Keys / Value panels.
func (a *App) panelHeight(bodyW int) int {
	if a.hideKeys && a.hideVal {
		return 0
	}
	limit := 4
	if a.h >= 30 {
		limit = 5
	}
	need := len(a.curTabInfo().Keys)
	if !a.hideKeys {
		kw := bodyW * 9 / 20
		if a.hideVal {
			kw = bodyW
		}
		need = max(need, len(flowKeys(a.curTab().keyHelp(), kw-4)))
	}
	if !a.hideVal && 2*len(a.curTabInfo().Keys) <= limit {
		need = max(need, 2*len(a.curTabInfo().Keys))
	}
	return min(limit, max(2, need)) + 2
}

func (a *App) frame() (*Buf, []Hit) {
	t := a.th()
	w, h := max(1, a.w), max(1, a.h)
	b := NewBuf(w, h, t.base())
	var hits []Hit
	if a.w < minW || a.h < minH {
		msg := fmt.Sprintf("demo needs at least %d×%d — this window is %d×%d. Make the window bigger.", minW, minH, a.w, a.h)
		for i, l := range wrap(msg, max(10, w-2)) {
			b.Text(1, h/2-1+i, l, t.base().Fg(t.Warn).With(Bold), w-2)
		}
		return b, nil
	}
	switch a.ph {
	case phJoin:
		a.drawSplash(b)
		return b, a.join.draw(a, b)
	case phConnecting:
		a.drawSplash(b)
		msg := "Connecting to " + a.opts.Server + " as " + a.name + " …"
		if a.conn == connReconnecting && a.connErr != "" {
			msg = "Can't reach " + a.opts.Server + " (" + a.connErr + "); retrying…  ^Q quits"
		}
		for i, l := range wrap(msg, w-4) {
			b.Fit(0, h/2+3+i, l, t.base().Fg(t.Muted), w, 2)
		}
		return b, nil
	}
	lay := a.layout()
	hits = append(hits, a.drawMenuBar(b, lay.menu)...)
	hits = append(hits, a.drawSidebar(b, lay.side)...)
	hits = append(hits, a.drawBody(b, lay)...)
	a.drawStatus(b, lay.status)
	hits = append(hits, a.drawHints(b, lay.hints)...)
	for _, o := range a.overlays {
		hits = append(hits, o.draw(a, b)...)
	}
	hits = append(hits, a.drawToasts(b)...)
	return b, hits
}

func (a *App) drawSplash(b *Buf) {
	t := a.th()
	logo := []string{
		"██████╗ ███████╗███╗   ███╗ ██████╗ ",
		"██╔══██╗██╔════╝████╗ ████║██╔═══██╗",
		"██║  ██║█████╗  ██╔████╔██║██║   ██║",
		"██║  ██║██╔══╝  ██║╚██╔╝██║██║   ██║",
		"██████╔╝███████╗██║ ╚═╝ ██║╚██████╔╝",
		"╚═════╝ ╚══════╝╚═╝     ╚═╝ ╚═════╝ ",
	}
	y := max(0, b.H/2-11)
	for i, l := range logo {
		c := mix(t.Accent, t.Accent2, float64(i)/5)
		b.Fit(0, y+i, l, Sty{FG: c, BG: t.BG, A: Bold}, b.W, 2)
	}
	b.Fit(0, y+len(logo)+1, "a control gallery · Go TUI "+core.Version, t.base().Fg(t.Muted), b.W, 2)
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
	for _, l := range a.overlays {
		if lo, ok := l.(*listOverlay); ok && lo.menuBar >= 0 {
			open = lo.menuBar
		}
	}
	x := 1
	for i, m := range menuBar {
		s := st
		if i == open {
			s = Sty{FG: t.CellFG, BG: t.CellBG}
		}
		lbl := " " + m.title + " "
		b.Text(x, r.Y, lbl, s, -1)
		b.Set(x+1, r.Y, []rune(m.title)[0], s.With(Underline).Fg(choose(i == open, t.CellFG, t.Accent)))
		hits = append(hits, Hit{R: Rect{x, r.Y, StrW(lbl), 1}, Kind: HitMenuTitle, A: i})
		x += StrW(lbl) + 1
	}
	// right side: demo · name · ● impl version
	dc := t.OK
	switch a.conn {
	case connReconnecting:
		dc = t.Warn
	case connConnecting:
		dc = t.Muted
	}
	if a.sess == nil && a.outbox == nil {
		dc = t.Err
	}
	srv := "go " + core.Version
	if a.vals.Server.Impl != "" {
		srv = core.SafeText(a.vals.Server.Impl + " " + a.vals.Server.Version)
	}
	left := "demo · " + core.SafeText(a.myName()) + " · "
	rw := StrW(left) + 2 + StrW(srv) + 1
	rx := r.Right() - rw
	if rx > x+2 {
		b.Text(rx, r.Y, left, st.Fg(t.Muted), -1)
		b.Text(rx+StrW(left), r.Y, "●", st.Fg(dc).With(Bold), 1)
		b.Text(rx+StrW(left)+2, r.Y, srv, st.Fg(t.Muted), -1)
	}
	return hits
}

func choose[T any](c bool, a, b T) T {
	if c {
		return a
	}
	return b
}

func (a *App) drawSidebar(b *Buf, r Rect) []Hit {
	t := a.th()
	fill := Sty{FG: t.FG, BG: t.Panel}
	b.Fill(r, ' ', fill)
	for y := r.Y; y < r.Bottom(); y++ {
		b.Set(r.Right(), y, '│', Sty{FG: t.Border, BG: t.BG})
	}
	// peers per tab
	on := map[string][]core.Peer{}
	for _, p := range a.vals.OtherPeers() {
		if p.Tab != nil {
			on[*p.Tab] = append(on[*p.Tab], p)
		}
	}
	var hits []Hit
	for i, tb := range core.Tabs {
		y := r.Y + i
		if y >= r.Bottom() {
			break
		}
		row := Rect{r.X, y, r.W, 1}
		st := fill
		cur := i == a.tab
		if cur {
			if a.inBody {
				st = Sty{FG: t.FG, BG: t.SelBG, A: Bold}
			} else {
				st = Sty{FG: t.CellFG, BG: t.CellBG, A: Bold}
			}
		}
		b.Fill(row, ' ', st)
		mark := " "
		if cur {
			mark = "▸"
		}
		b.Text(r.X, y, mark, st.Fg(choose(cur && !a.inBody, t.CellFG, t.Accent)), 1)
		b.Text(r.X+1, y, fmt.Sprintf("%2d", tb.Num), st.Fg(choose(cur && !a.inBody, t.CellFG, t.Muted)), 2)
		b.Text(r.X+4, y, tb.Title, st, 9)
		x := r.X + 13
		for j, p := range on[tb.ID] {
			if j == 2 && len(on[tb.ID]) > 3 {
				b.Set(x, y, '+', st.Fg(t.Muted))
				break
			}
			b.Set(x, y, '●', st.Fg(hex(p.Color)))
			x++
		}
		hits = append(hits, Hit{R: row, Kind: HitSideTab, A: i})
	}
	if r.H > len(core.Tabs)+2 {
		b.Text(r.X+1, r.Bottom()-1, choose(a.inBody, "Esc sidebar", "Enter open"), fill.Fg(t.Muted), r.W-1)
	}
	return hits
}

func (a *App) drawBody(b *Buf, lay layoutT) []Hit {
	t := a.th()
	ti := a.curTabInfo()
	base := t.base()
	// title line: "4  Dropdown  select" and the tab's note on the right
	x := lay.title.X
	x += b.Text(x, lay.title.Y, ti.Title, base.Fg(t.Accent).With(Bold), -1)
	b.Text(x+2, lay.title.Y, "tab "+ti.ID, base.Fg(t.Muted), -1)
	if n := a.curTab().note(a); n != "" {
		nw := min(StrW(n), lay.title.W-StrW(ti.Title)-12)
		b.Text(lay.title.Right()-nw, lay.title.Y, n, base.Fg(t.Warn), nw)
	}
	for i, l := range wrap(a.curTab().explain(), lay.title.W) {
		b.Text(lay.title.X, lay.title.Y+1+i, l, base.Fg(t.Muted), lay.title.W)
	}
	if lay.keys.W > 0 {
		a.drawKeysPanel(b, lay.keys)
	}
	if lay.value.W > 0 {
		a.drawValuePanel(b, lay.value)
	}
	// controls last: their popups may cover the panels, never the sidebar
	saved := b.Clip
	b.Clip = Rect{lay.body.X - 1, lay.body.Y, lay.body.W + 2, lay.body.H}
	hits := a.curTab().draw(a, b, lay.ctl, a.inBody)
	b.Clip = saved
	return hits
}

// flowKeys packs "KEY label" pairs into lines of at most w cells.
func flowKeys(keys [][2]string, w int) [][][2]string {
	var lines [][][2]string
	var cur [][2]string
	used := 0
	for _, k := range keys {
		need := StrW(k[0]) + 1 + StrW(k[1]) + 2
		if used > 0 && used+need > w {
			lines = append(lines, cur)
			cur, used = nil, 0
		}
		cur = append(cur, k)
		used += need
	}
	if len(cur) > 0 {
		lines = append(lines, cur)
	}
	return lines
}

func (a *App) drawKeysPanel(b *Buf, r Rect) {
	t := a.th()
	fill := Sty{FG: t.FG, BG: t.Panel}
	b.Box(r, Sty{FG: t.Border, BG: t.Panel}, fill, "Keys", Sty{FG: t.Accent, BG: t.Panel, A: Bold})
	in := Rect{r.X + 2, r.Y + 1, r.W - 4, r.H - 2}
	lines := flowKeys(a.curTab().keyHelp(), in.W)
	for i, l := range lines {
		if i >= in.H {
			break
		}
		x := in.X
		for _, k := range l {
			x += b.Text(x, in.Y+i, k[0], fill.Fg(t.Warn).With(Bold), in.Right()-x)
			x++
			x += b.Text(x, in.Y+i, k[1], fill, in.Right()-x)
			x += 2
		}
	}
	if len(lines) > in.H {
		b.Text(r.Right()-10, r.Bottom()-1, " F1 more ", Sty{FG: t.Muted, BG: t.Panel}, 9)
	}
}

// valueText is a key's value for the Value panel: canonical JSON, with
// peer-supplied text made safe for the terminal.
func (a *App) valueText(key string) string {
	return core.SafeText(string(a.vals.Raw(key)))
}

func (a *App) drawValuePanel(b *Buf, r Rect) {
	t := a.th()
	fill := Sty{FG: t.FG, BG: t.Panel}
	border := Sty{FG: t.Border, BG: t.Panel}
	keys := a.curTabInfo().Keys
	flashing := false
	for _, k := range keys {
		if _, ok := a.flash[k]; ok {
			flashing = true
		}
	}
	if flashing {
		border = Sty{FG: t.Warn, BG: t.Panel, A: Bold}
	}
	b.Box(r, border, fill, "Value (shared)", Sty{FG: choose(flashing, t.Warn, t.Accent), BG: t.Panel, A: Bold})
	in := Rect{r.X + 2, r.Y + 1, r.W - 4, r.H - 2}
	two := 2*len(keys) <= in.H
	y := in.Y
	for i, k := range keys {
		if y >= in.Bottom() {
			break
		}
		if !two && i == in.H-1 && len(keys) > in.H {
			b.Text(in.X, y, fmt.Sprintf("+%d more (F1)", len(keys)-i), fill.Fg(t.Muted), in.W)
			break
		}
		st := fill
		if _, ok := a.flash[k]; ok {
			st = Sty{FG: t.BG, BG: t.Warn}
		}
		x := in.X
		x += b.Text(x, y, k, st.Fg(choose(st.BG == t.Warn, t.BG, t.Accent2)), in.W)
		x += b.Text(x, y, " = ", st, in.Right()-x)
		x += b.Text(x, y, a.valueText(k), st, in.Right()-x)
		desc := a.vals.Describe(k, a.now)
		if two {
			y++
			b.Text(in.X+2, y, desc, fill.Fg(t.Muted), in.W-2)
		} else if x+3 < in.Right() {
			b.Text(x+1, y, "· "+desc, fill.Fg(t.Muted), in.Right()-x-1)
		}
		y++
	}
}

func (a *App) drawStatus(b *Buf, r Rect) {
	t := a.th()
	st := Sty{FG: t.FG, BG: t.Panel2}
	b.Fill(r, ' ', st)
	msg, ms := a.status, st
	switch a.statKind {
	case kOK:
		ms = ms.Fg(t.OK)
	case kWarn:
		ms = ms.Fg(t.Warn)
	case kErr:
		ms = ms.Fg(t.Err)
	case kInfo:
		ms = ms.Fg(t.Info)
	}
	if a.conn == connReconnecting {
		msg, ms = "⚠ Offline — reconnecting; changes are not saved", st.Fg(t.Warn).With(Bold)
	}
	// right: presence initials in each peer's colour, then the count
	x := r.Right() - 1
	if n := len(a.inflight); n > 0 {
		s := fmt.Sprintf(" ⟳ %d", n)
		x -= StrW(s)
		b.Text(x, r.Y, s, st.Fg(t.Warn), -1)
	}
	peers := a.vals.Peers
	s := fmt.Sprintf(" online: %d ", len(peers))
	x -= StrW(s)
	b.Text(x, r.Y, s, st.Fg(t.Muted), -1)
	for i := len(peers) - 1; i >= 0 && x > r.W/2; i-- {
		p := peers[i]
		ini := " " + core.SafeText(core.Initials(p.Name)) + " "
		x -= StrW(ini) + 1
		b.Text(x, r.Y, ini, Sty{FG: t.BG, BG: hex(p.Color), A: Bold}, -1)
	}
	b.Text(1, r.Y, msg, ms, max(0, x-2))
}

// globalHints follow the context hints in the hint bar.
func globalHints(body bool) []hint {
	out := []hint{}
	if body {
		out = append(out, h("esc", "Sidebar"))
	}
	return append(out, h("ctrl+k", "Commands"), h("f1", "Help"), h("f10", "Menu"), h("ctrl+r", "Reset tab"), h("ctrl+q", "Quit"))
}

// currentHints adapts the hint bar to what has focus.
func (a *App) currentHints() []hint {
	if o := a.top(); o != nil {
		return o.hints(a)
	}
	if !a.inBody {
		return append([]hint{{"↑↓", "Tab", ""}, h("enter", "Open")}, globalHints(false)...)
	}
	return append(a.curTab().hints(a), globalHints(true)...)
}

func (a *App) drawHints(b *Buf, r Rect) []Hit {
	t := a.th()
	st := Sty{FG: t.FG, BG: t.BG}
	b.Fill(r, ' ', st)
	var hits []Hit
	x, y := 0, r.Y
	for _, hn := range a.currentHints() {
		kw, lw := StrW(hn.Key), StrW(hn.Label)
		need := kw + lw + 3
		if x+need > r.W {
			y++
			x = 0
			if y >= r.Bottom() {
				break
			}
		}
		b.Text(x, y, " "+hn.Key+" ", Sty{FG: t.BG, BG: t.Muted, A: Bold}, -1)
		b.Text(x+kw+2, y, hn.Label, st, -1)
		if hn.Press != "" {
			hits = append(hits, Hit{R: Rect{x, y, need - 1, 1}, Kind: HitHint, S: hn.Press})
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
		w := min(50, StrW(ts.text)+4)
		lines := wrap(ts.text, w-4)
		h := len(lines) + 2
		r := Rect{b.W - w - 2, y, w, h}
		c := t.Info
		switch ts.kind {
		case kOK:
			c = t.OK
		case kWarn:
			c = t.Warn
		case kErr:
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
		if y > b.H-8 {
			break
		}
	}
	return hits
}

func wrap(s string, w int) []string {
	var out []string
	for _, para := range strings.Split(s, "\n") {
		line := ""
		for _, word := range strings.Fields(para) {
			if line != "" && StrW(line)+1+StrW(word) > w {
				out = append(out, line)
				line = word
			} else if line == "" {
				line = word
			} else {
				line += " " + word
			}
		}
		out = append(out, line)
	}
	return out
}

// button draws "[ label ]" and returns its rect; focused buttons are filled.
func button(b *Buf, t *Theme, x, y int, label string, focused bool, _ Color) Rect {
	lbl := "[ " + label + " ]"
	st := Sty{FG: t.FG, BG: t.Panel2}
	if focused {
		st = Sty{FG: t.CellFG, BG: t.CellBG, A: Bold}
	}
	w := b.Text(x, y, lbl, st, -1)
	return Rect{x, y, w, 1}
}
