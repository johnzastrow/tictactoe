package tui

import (
	"fmt"
	"image/color"
	"strconv"
	"strings"

	tea "charm.land/bubbletea/v2"
	"charm.land/lipgloss/v2"
	"github.com/charmbracelet/x/ansi"

	"git.wharf.fluidgrid.org/johnzastrow/tictactoe/go/internal/core"
)

const (
	minW, minH = 34, 20
	headerH    = 3
	sideW      = 34
)

var (
	spinner   = []string{"⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"}
	cellNames = []string{"top-left", "top", "top-right", "left", "centre", "right",
		"bottom-left", "bottom", "bottom-right"}
	xBig = []string{"██   ██", " ██ ██ ", "  ███  ", " ██ ██ ", "██   ██"}
	oBig = []string{" █████ ", "██   ██", "██   ██", "██   ██", " █████ "}
	xMid = []string{"╲ ╱", " ╳ ", "╱ ╲"}
	oMid = []string{"╭─╮", "│ │", "╰─╯"}
	// (cell width, cell height) options, largest first.
	sizes = [][2]int{{11, 5}, {7, 3}, {3, 1}}
)

// geom is the screen layout for a given terminal size. It is a pure
// function of (w, h) so Update can hit-test mouse clicks against exactly
// what View drew, without View having to mutate the model.
type geom struct {
	tooSmall, side        bool
	boardW, bodyH, innerW int
	gridAreaH             int
	cw, ch, gw, gh        int
	gridX, gridY          int // screen position of the grid's top-left corner
}

func layoutFor(w, h int) geom {
	g := geom{tooSmall: w < minW || h < minH}
	if g.tooSmall {
		return g
	}
	g.bodyH = h - headerH - 1
	g.side = w >= 76 // drop the sidebar rather than squash the board
	g.boardW = w
	if g.side {
		g.boardW = w - sideW
	}
	g.innerW = g.boardW - 2
	g.gridAreaH = g.bodyH - 2 - 3 // borders and 3 status lines
	g.cw, g.ch = sizes[2][0], sizes[2][1]
	for _, s := range sizes {
		if 3*s[0]+4 <= g.innerW && 3*s[1]+4 <= g.gridAreaH {
			g.cw, g.ch = s[0], s[1]
			break
		}
	}
	g.gw, g.gh = 3*g.cw+4, 3*g.ch+4
	g.gridX = 1 + max(0, (g.innerW-g.gw)/2)
	g.gridY = headerH + 1 + max(0, (g.gridAreaH-g.gh)/2)
	return g
}

func (g geom) cellAt(x, y int) (int, bool) {
	if g.tooSmall {
		return 0, false
	}
	for i := range 9 {
		cx := g.gridX + 1 + (i%3)*(g.cw+1)
		cy := g.gridY + 1 + (i/3)*(g.ch+1)
		if x >= cx && x < cx+g.cw && y >= cy && y < cy+g.ch {
			return i, true
		}
	}
	return 0, false
}

func (m Model) View() tea.View {
	t := themes[m.theme]
	v := tea.NewView(m.render(t))
	v.AltScreen = true
	v.MouseMode = tea.MouseModeAllMotion
	v.WindowTitle = "Tic-Tac-Toe"
	if t.truecolor {
		v.BackgroundColor = t.BG
	}
	return v
}

// --- small styling helpers -------------------------------------------------

func st(fg, bg color.Color) lipgloss.Style {
	return lipgloss.NewStyle().Foreground(fg).Background(bg)
}

// fit truncates or pads (with fill's background) s to exactly w cells.
func fit(s string, w int, fill lipgloss.Style) string {
	if lipgloss.Width(s) > w {
		s = ansi.Truncate(s, w, "")
	}
	if d := w - lipgloss.Width(s); d > 0 {
		s += fill.Render(strings.Repeat(" ", d))
	}
	return s
}

func center(s string, w int, fill lipgloss.Style) string {
	d := w - lipgloss.Width(s)
	if d <= 0 {
		return fit(s, w, fill)
	}
	return fill.Render(strings.Repeat(" ", d/2)) + s + fill.Render(strings.Repeat(" ", d-d/2))
}

// panel draws a rounded box of exactly w×h with titles in the borders. We
// draw borders ourselves (rather than lipgloss.Border) because the grid
// geometry must match layoutFor() cell-for-cell for mouse hit-testing.
func panel(t Theme, title, topRight, bottomRight string, inner []string, w, h int) []string {
	bs := st(t.Grid, t.Panel)
	ts := st(t.Muted, t.Panel).Bold(true)
	fill := st(t.FG, t.Panel)
	edge := func(left, right, label, rlabel string, ls lipgloss.Style) string {
		l := ""
		if label != "" {
			l = ls.Render(" " + label + " ")
		}
		r := ""
		if rlabel != "" {
			r = st(t.Muted, t.Panel).Render(" " + rlabel + " ")
		}
		dashes := max(0, w-2-lipgloss.Width(l)-lipgloss.Width(r))
		return bs.Render(left) + l + bs.Render(strings.Repeat("─", dashes)) + r + bs.Render(right)
	}
	out := []string{edge("╭", "╮", title, topRight, ts)}
	for i := range h - 2 {
		line := ""
		if i < len(inner) {
			line = inner[i]
		}
		out = append(out, bs.Render("│")+fit(line, w-2, fill)+bs.Render("│"))
	}
	return append(out, edge("╰", "╯", "", bottomRight, ts))
}

func playerColor(t Theme, p core.Player) color.Color {
	if p == core.X {
		return t.X
	}
	return t.O
}

// --- screen -----------------------------------------------------------------

func (m Model) render(t Theme) string {
	if m.w == 0 {
		return "" // no WindowSizeMsg yet
	}
	g := layoutFor(m.w, m.h)
	if g.tooSmall {
		msg := fmt.Sprintf("Terminal too small\n%d×%d (need %d×%d)", m.w, m.h, minW, minH)
		return lipgloss.Place(m.w, m.h, lipgloss.Center, lipgloss.Center, msg)
	}

	lines := m.header(t)
	board := m.boardPanel(t, g)
	if g.side {
		side := m.sidebar(t, g.bodyH)
		for i := range board {
			board[i] += side[i]
		}
	}
	lines = append(lines, board...)
	lines = append(lines, m.footer(t))
	screen := strings.Join(lines, "\n")

	if m.help {
		// Lip Gloss v2 layers: composite the popup over the finished screen.
		pop := m.helpBox(t)
		pw, ph := lipgloss.Size(pop)
		screen = lipgloss.NewCompositor(
			lipgloss.NewLayer(screen),
			lipgloss.NewLayer(pop).X(max(0, (m.w-pw)/2)).Y(max(0, (m.h-ph)/2)).Z(1),
		).Render()
	}
	return screen
}

func (m Model) header(t Theme) []string {
	bg := t.Panel
	title := st(t.X, bg).Bold(true).Render("✕") + st(t.FG, bg).Render(" ") +
		st(t.O, bg).Bold(true).Render("◯") + st(t.FG, bg).Bold(true).Render("  TIC-TAC-TOE")
	var web string
	if m.webURL != "" {
		n := m.state.WebClients
		plural := "s"
		if n == 1 {
			plural = ""
		}
		web = st(t.Accent, bg).Render(" ● ") + st(t.Muted, bg).Render("web ") +
			st(t.FG, bg).Underline(true).Render(m.webURL) +
			st(t.Muted, bg).Render(fmt.Sprintf("  ·  %d browser%s connected", n, plural))
	} else {
		web = st(t.Muted, bg).Render(" ○ web interface disabled")
	}
	// Title is pre-styled, so pass it through panel's label slot raw.
	box := panel(t, "", fmt.Sprintf("game #%d", m.state.GameNo), "", []string{web}, m.w, headerH)
	bs := st(t.Grid, t.Panel)
	rest := m.w - 2 - lipgloss.Width(title) - 2 - lipgloss.Width(fmt.Sprintf(" game #%d ", m.state.GameNo))
	box[0] = bs.Render("╭ ") + title + bs.Render(" "+strings.Repeat("─", max(0, rest))) +
		st(t.Muted, bg).Render(fmt.Sprintf(" game #%d ", m.state.GameNo)) + bs.Render("╮")
	return box
}

func (m Model) boardPanel(t Theme, g geom) []string {
	fill := st(t.FG, t.Panel)
	grid := m.grid(t, g)
	inner := make([]string, 0, g.bodyH-2)
	top := (g.gridAreaH - g.gh) / 2
	for range max(0, top) {
		inner = append(inner, "")
	}
	pad := fill.Render(strings.Repeat(" ", g.gridX-1))
	for _, l := range grid {
		inner = append(inner, pad+l)
	}
	for len(inner) < g.gridAreaH {
		inner = append(inner, "")
	}
	inner = inner[:g.gridAreaH]
	for _, l := range m.status(t) {
		inner = append(inner, center(l, g.innerW, fill))
	}
	return panel(t, "Board", "", "", inner, g.boardW, g.bodyH)
}

func (m Model) grid(t Theme, g geom) []string {
	ls := st(t.Grid, t.Panel)
	var out []string
	for dy := range g.gh {
		if dy%(g.ch+1) == 0 {
			var b strings.Builder
			for dx := range g.gw {
				b.WriteString(junction(dx, dy, g))
			}
			out = append(out, ls.Render(b.String()))
			continue
		}
		row, r := dy/(g.ch+1), dy%(g.ch+1)-1
		line := ls.Render("│")
		for col := range 3 {
			line += m.cellLine(t, g, row*3+col, r) + ls.Render("│")
		}
		out = append(out, line)
	}
	return out
}

func junction(dx, dy int, g geom) string {
	onV := dx%(g.cw+1) == 0
	top, bottom := dy == 0, dy == g.gh-1
	left, right := dx == 0, dx == g.gw-1
	switch {
	case !onV:
		return "─"
	case top && left:
		return "╭"
	case top && right:
		return "╮"
	case bottom && left:
		return "╰"
	case bottom && right:
		return "╯"
	case top:
		return "┬"
	case bottom:
		return "┴"
	case left:
		return "├"
	case right:
		return "┤"
	}
	return "┼"
}

// cellLine renders row r (0-based) of cell i, exactly g.cw wide.
func (m Model) cellLine(t Theme, g geom, i, r int) string {
	s := m.state
	interactive := !s.Status.Over() && s.AI != s.Turn
	winning := false
	for _, c := range s.Status.Line {
		winning = winning || c == i
	}
	lit := winning && (m.ticks/4)%2 == 0
	bg := t.Panel
	switch {
	case lit:
		bg = t.Win
	case i == m.cursor && interactive:
		bg = t.Cursor
	}

	var art []string
	glyph := st(t.Muted, bg).Faint(true)
	switch p := s.Board[i]; {
	case p != core.None:
		art = artFor(p, g.ch)
		fg := playerColor(t, p)
		if lit {
			fg = t.WinFG
		}
		glyph = st(fg, bg).Bold(true)
	case i == m.cursor && interactive:
		art = artFor(s.Turn, g.ch) // ghost preview of the mark you'd place
	default:
		art = []string{strconv.Itoa(i + 1)}
	}

	text := ""
	if top := (g.ch - len(art)) / 2; r >= top && r < top+len(art) {
		text = art[r-top]
	}
	runes := []rune(strings.Repeat(" ", g.cw))
	left := (g.cw - len([]rune(text))) / 2
	if len(art) == 1 && len([]rune(text)) == 1 {
		left = g.cw / 2 // match Rust: single chars sit at the exact centre
	}
	copy(runes[left:], []rune(text))

	last := len(s.History) > 0 && s.History[len(s.History)-1] == i
	if last && r == 0 && g.cw > 3 {
		return glyph.Render(string(runes[:g.cw-1])) + st(t.Accent, bg).Render("•")
	}
	return glyph.Render(string(runes))
}

func artFor(p core.Player, h int) []string {
	switch {
	case h >= 5 && p == core.X:
		return xBig
	case h >= 5:
		return oBig
	case h >= 3 && p == core.X:
		return xMid
	case h >= 3:
		return oMid
	}
	return []string{p.String()}
}

func (m Model) status(t Theme) []string {
	s, bg := m.state, t.Panel
	var head string
	switch {
	case s.Status.Kind == core.Won:
		who := ""
		if s.AI == s.Status.Winner {
			who = " (AI)"
		}
		head = st(t.Win, bg).Bold(true).Render(fmt.Sprintf("★ %s WINS%s ★", s.Status.Winner, who)) +
			st(t.Muted, bg).Render("   n = new game")
	case s.Status.Kind == core.Draw:
		head = st(t.FG, bg).Bold(true).Render("Draw") + st(t.Muted, bg).Render("   n = new game")
	case s.AI == s.Turn:
		head = st(t.Accent, bg).Render(spinner[m.ticks%len(spinner)]+" ") +
			st(playerColor(t, s.Turn), bg).Bold(true).Render("AI ("+s.Turn.String()+")") +
			st(t.Muted, bg).Render(" is thinking…")
	default:
		head = st(playerColor(t, s.Turn), bg).Bold(true).Render(s.Turn.String()) +
			st(t.FG, bg).Render(" to move")
	}
	sub := st(t.Muted, bg).Render("arrows/hjkl + enter · 1-9 · or click")
	if text, isErr, ok := m.activeToast(); ok {
		fg := t.Accent
		if isErr {
			fg = t.Error
		}
		sub = st(fg, bg).Render(text)
	}
	return []string{"", head, sub}
}

func (m Model) sidebar(t Theme, h int) []string {
	out := m.score(t)
	out = append(out, m.mode(t)...)
	return append(out, m.history(t, h-len(out))...)
}

func (m Model) score(t Theme) []string {
	sc, bg, w := m.state.Score, t.Panel, sideW-2
	third := w / 3
	col := func(s string, fg color.Color, bold bool, width int) string {
		return center(st(fg, bg).Bold(bold).Render(s), width, st(fg, bg))
	}
	labels := col("X", t.Muted, false, third) + col("draws", t.Muted, false, third) + col("O", t.Muted, false, w-2*third)
	nums := col(strconv.Itoa(sc.X), t.X, true, third) + col(strconv.Itoa(sc.Draws), t.Muted, true, third) +
		col(strconv.Itoa(sc.O), t.O, true, w-2*third)

	// Proportional bar: X | draws | O.
	var bar string
	if total := sc.X + sc.O + sc.Draws; total == 0 {
		bar = st(t.Grid, bg).Render(strings.Repeat("░", w))
	} else {
		xw, ow := sc.X*w/total, sc.O*w/total
		bar = st(t.X, bg).Render(strings.Repeat("█", xw)) +
			st(t.Grid, bg).Render(strings.Repeat("█", w-xw-ow)) +
			st(t.O, bg).Render(strings.Repeat("█", ow))
	}
	return panel(t, "Score", "", "", []string{labels, nums, "", bar}, sideW, 6)
}

func (m Model) mode(t Theme) []string {
	opts := []struct {
		p     core.Player
		label string
	}{{core.None, "Two humans"}, {core.O, "AI plays O"}, {core.X, "AI plays X"}}
	var lines []string
	for _, o := range opts {
		if m.state.AI == o.p {
			lines = append(lines, st(t.Accent, t.Panel).Bold(true).Render(" ◉ "+o.label))
		} else {
			lines = append(lines, st(t.Muted, t.Panel).Render(" ○ "+o.label))
		}
	}
	return panel(t, "Opponent", "", "a to change", lines, sideW, 5)
}

func (m Model) history(t Theme, h int) []string {
	bg, hist := t.Panel, m.state.History
	if len(hist) == 0 {
		return panel(t, "Moves", "", "", []string{st(t.Muted, bg).Render(" no moves yet")}, sideW, h)
	}
	// Show the newest moves that fit; X always starts, so even plies are X.
	start := max(0, len(hist)-(h-2))
	var lines []string
	for n := start; n < len(hist); n++ {
		p := core.X
		if n%2 == 1 {
			p = core.O
		}
		lbg, marker := bg, "  "
		if n == len(hist)-1 {
			lbg, marker = t.Cursor, "› "
		}
		line := st(t.FG, lbg).Render(marker) +
			st(t.Muted, lbg).Render(fmt.Sprintf("%2d. ", n+1)) +
			st(playerColor(t, p), lbg).Bold(true).Render(p.String()) +
			st(t.FG, lbg).Render("  "+cellNames[hist[n]])
		lines = append(lines, fit(line, sideW-2, st(t.FG, lbg)))
	}
	return panel(t, "Moves", "", "", lines, sideW, h)
}

func (m Model) footer(t Theme) string {
	keys := [][2]string{{"←↑↓→", "move"}, {"⏎", "play"}, {"1-9", "cell"}, {"n", "new"},
		{"a", "opponent"}, {"t", "theme"}, {"?", "help"}, {"q", "quit"}}
	line := st(t.Muted, t.BG).Render(" ")
	for _, k := range keys {
		line += st(t.BG, t.Muted).Bold(true).Render(" "+k[0]+" ") + st(t.Muted, t.BG).Render(" "+k[1]+"  ")
	}
	return fit(line, m.w, st(t.FG, t.BG))
}

func (m Model) helpBox(t Theme) string {
	bg := t.Panel
	key := func(k, d string) string {
		return st(t.Accent, bg).Bold(true).Render(fmt.Sprintf("  %-14s", k)) + st(t.FG, bg).Render(d)
	}
	muted := func(s string) string { return st(t.Muted, bg).Render(s) }
	lines := []string{
		"",
		key("arrows / hjkl", "move the cursor"),
		key("enter / space", "place your mark"),
		key("1 - 9", "place mark in that cell (1 = top-left)"),
		key("mouse", "hover to aim, click to play"),
		key("n", "new game (score is kept)"),
		key("a", "cycle opponent: humans / AI O / AI X"),
		key("t", "cycle colour theme"),
		key("q / esc", "quit"),
		"",
		muted("  The browser and this terminal share one live game:"),
		muted("  moves made in either show up instantly in both."),
		"",
		center(muted("press any key to close"), 56, st(t.FG, bg)),
	}
	return strings.Join(panel(t, "Help", "", "", lines, 58, 17), "\n")
}
