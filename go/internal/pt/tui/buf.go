package tui

// A cell buffer, like ratatui's: every frame is drawn into a grid of styled
// cells (later layers overwrite earlier ones, which makes popups, shadows
// and highlights trivial) and then encoded to ANSI once. Colours are
// emitted as 24-bit or mapped to the nearest xterm-256 colour.

import (
	"strconv"
	"strings"

	"github.com/mattn/go-runewidth"
)

// Color is an RGB colour; the zero value means "terminal default".
type Color struct {
	R, G, B uint8
	Set     bool
}

func hex(s string) Color {
	s = strings.TrimPrefix(s, "#")
	if len(s) != 6 {
		return Color{}
	}
	v, err := strconv.ParseUint(s, 16, 32)
	if err != nil {
		return Color{}
	}
	return Color{uint8(v >> 16), uint8(v >> 8), uint8(v), true}
}

// mix blends a toward b by t (0..1).
func mix(a, b Color, t float64) Color {
	if !a.Set || !b.Set {
		return a
	}
	f := func(x, y uint8) uint8 { return uint8(float64(x) + (float64(y)-float64(x))*t + 0.5) }
	return Color{f(a.R, b.R), f(a.G, b.G), f(a.B, b.B), true}
}

type Attr uint8

const (
	Bold Attr = 1 << iota
	Dim
	Italic
	Underline
	Reverse
	Strike
)

// Sty is a cell style. UL colours the underline (curly-underline capable
// terminals show it; others just underline).
type Sty struct {
	FG, BG, UL Color
	A          Attr
}

func (s Sty) Fg(c Color) Sty    { s.FG = c; return s }
func (s Sty) Bg(c Color) Sty    { s.BG = c; return s }
func (s Sty) With(a Attr) Sty   { s.A |= a; return s }
func (s Sty) ULine(c Color) Sty { s.A |= Underline; s.UL = c; return s }

type cell struct {
	r    rune // 0 = right half of a wide rune
	s    Sty
	wide bool
}

type Rect struct{ X, Y, W, H int }

func (r Rect) Contains(x, y int) bool { return x >= r.X && x < r.X+r.W && y >= r.Y && y < r.Y+r.H }
func (r Rect) Inset(n int) Rect       { return Rect{r.X + n, r.Y + n, max(0, r.W-2*n), max(0, r.H-2*n)} }
func (r Rect) Right() int             { return r.X + r.W }
func (r Rect) Bottom() int            { return r.Y + r.H }

type Buf struct {
	W, H  int
	cells []cell
	// Clip limits all drawing (set by views that draw inside a pane).
	Clip Rect
}

func NewBuf(w, h int, s Sty) *Buf {
	b := &Buf{W: w, H: h, cells: make([]cell, w*h), Clip: Rect{0, 0, w, h}}
	for i := range b.cells {
		b.cells[i] = cell{r: ' ', s: s}
	}
	return b
}

func (b *Buf) ok(x, y int) bool {
	return x >= 0 && y >= 0 && x < b.W && y < b.H && b.Clip.Contains(x, y)
}

func runeW(r rune) int {
	if r < 0x80 {
		return 1
	}
	w := runewidth.RuneWidth(r)
	if w < 1 {
		return 0
	}
	return w
}

// StrW is the display width of s.
func StrW(s string) int {
	n := 0
	for _, r := range s {
		n += runeW(r)
	}
	return n
}

// Set writes one rune; returns its width.
func (b *Buf) Set(x, y int, r rune, s Sty) int {
	w := runeW(r)
	if w == 0 {
		return 0
	}
	if !b.ok(x, y) || (w == 2 && !b.ok(x+1, y)) {
		return w
	}
	i := y*b.W + x
	// overwriting half of a wide rune leaves a blank behind
	if b.cells[i].r == 0 && x > 0 {
		b.cells[i-1] = cell{r: ' ', s: b.cells[i-1].s}
	}
	if b.cells[i].wide && x+1 < b.W {
		b.cells[i+1] = cell{r: ' ', s: b.cells[i+1].s}
	}
	b.cells[i] = cell{r: r, s: s, wide: w == 2}
	if w == 2 {
		b.cells[i+1] = cell{r: 0, s: s}
	}
	return w
}

// Text draws s clipped to maxW cells (maxW < 0: no limit); returns width used.
func (b *Buf) Text(x, y int, s string, st Sty, maxW int) int {
	used := 0
	for _, r := range s {
		if r == '\n' || r == '\t' {
			r = ' '
		}
		w := runeW(r)
		if maxW >= 0 && used+w > maxW {
			break
		}
		b.Set(x+used, y, r, st)
		used += w
	}
	return used
}

// Fit draws s in exactly w cells: ellipsised if longer, padded if shorter.
// align: 0 left, 1 right, 2 centre.
func (b *Buf) Fit(x, y int, s string, st Sty, w int, align int) {
	if w <= 0 {
		return
	}
	s = Ellipsis(s, w)
	sw := StrW(s)
	pad := w - sw
	off := 0
	switch align {
	case 1:
		off = pad
	case 2:
		off = pad / 2
	}
	for i := 0; i < w; i++ {
		b.Set(x+i, y, ' ', st)
	}
	b.Text(x+off, y, s, st, w)
}

// Ellipsis shortens s to at most w cells, ending in "…" when cut.
func Ellipsis(s string, w int) string {
	if StrW(s) <= w {
		return s
	}
	if w <= 0 {
		return ""
	}
	var sb strings.Builder
	used := 0
	for _, r := range s {
		rw := runeW(r)
		if used+rw > w-1 {
			break
		}
		sb.WriteRune(r)
		used += rw
	}
	sb.WriteRune('…')
	return sb.String()
}

func (b *Buf) Fill(r Rect, ch rune, s Sty) {
	for y := r.Y; y < r.Y+r.H; y++ {
		for x := r.X; x < r.X+r.W; x++ {
			b.Set(x, y, ch, s)
		}
	}
}

// Restyle changes the style of existing cells (keeps their runes).
func (b *Buf) Restyle(r Rect, f func(Sty) Sty) {
	for y := r.Y; y < r.Y+r.H; y++ {
		for x := r.X; x < r.X+r.W; x++ {
			if b.ok(x, y) {
				c := &b.cells[y*b.W+x]
				c.s = f(c.s)
			}
		}
	}
}

// Box draws a rounded border with an optional title.
func (b *Buf) Box(r Rect, border, fill Sty, title string, titleSty Sty) {
	if r.W < 2 || r.H < 2 {
		return
	}
	b.Fill(Rect{r.X + 1, r.Y + 1, r.W - 2, r.H - 2}, ' ', fill)
	for x := r.X + 1; x < r.Right()-1; x++ {
		b.Set(x, r.Y, '─', border)
		b.Set(x, r.Bottom()-1, '─', border)
	}
	for y := r.Y + 1; y < r.Bottom()-1; y++ {
		b.Set(r.X, y, '│', border)
		b.Set(r.Right()-1, y, '│', border)
	}
	b.Set(r.X, r.Y, '╭', border)
	b.Set(r.Right()-1, r.Y, '╮', border)
	b.Set(r.X, r.Bottom()-1, '╰', border)
	b.Set(r.Right()-1, r.Bottom()-1, '╯', border)
	if title != "" {
		t := " " + Ellipsis(title, r.W-6) + " "
		b.Text(r.X+2, r.Y, t, titleSty, r.W-4)
	}
}

// Shadow darkens the cells to the right of and below r (a drop shadow).
func (b *Buf) Shadow(r Rect, shade Color) {
	dark := func(s Sty) Sty {
		if shade.Set {
			s.BG = shade
			s.FG = mix(s.FG, shade, 0.6)
		} else {
			s.A |= Dim
		}
		return s
	}
	b.Restyle(Rect{r.Right(), r.Y + 1, 2, r.H}, dark)
	b.Restyle(Rect{r.X + 2, r.Bottom(), r.W, 1}, dark)
}

// Line returns row y as plain text (tests and tmux-free assertions).
func (b *Buf) Line(y int) string {
	var sb strings.Builder
	for x := 0; x < b.W; x++ {
		c := b.cells[y*b.W+x]
		if c.r != 0 {
			sb.WriteRune(c.r)
		}
	}
	return sb.String()
}

func (b *Buf) String() string {
	lines := make([]string, b.H)
	for y := range lines {
		lines[y] = strings.TrimRight(b.Line(y), " ")
	}
	return strings.Join(lines, "\n")
}

// At returns the rune and style at x,y (tests).
func (b *Buf) At(x, y int) (rune, Sty) {
	c := b.cells[y*b.W+x]
	return c.r, c.s
}

// ---- ANSI encoding ----

var cube = [6]int{0, 95, 135, 175, 215, 255}

// to256 maps a colour to the nearest xterm-256 index (cube or grey ramp).
func to256(c Color) int {
	near := func(v uint8) int {
		best, bd := 0, 1<<30
		for i, x := range cube {
			if d := (int(v) - x) * (int(v) - x); d < bd {
				best, bd = i, d
			}
		}
		return best
	}
	ri, gi, bi := near(c.R), near(c.G), near(c.B)
	cr, cg, cb := cube[ri], cube[gi], cube[bi]
	dist := func(r, g, b2 int) int {
		dr, dg, db := int(c.R)-r, int(c.G)-g, int(c.B)-b2
		return dr*dr*2 + dg*dg*4 + db*db*3
	}
	cubeIdx, cubeD := 16+36*ri+6*gi+bi, dist(cr, cg, cb)
	avg := (int(c.R) + int(c.G) + int(c.B)) / 3
	gi2 := max(0, min(23, (avg-8+5)/10))
	gv := 8 + gi2*10
	if d := dist(gv, gv, gv); d < cubeD {
		return 232 + gi2
	}
	return cubeIdx
}

func sgrColor(sb *strings.Builder, c Color, fg, truecolor bool) {
	base := "38"
	if !fg {
		base = "48"
	}
	if !c.Set {
		if fg {
			sb.WriteString(";39")
		} else {
			sb.WriteString(";49")
		}
		return
	}
	if truecolor {
		sb.WriteString(";" + base + ";2;" + strconv.Itoa(int(c.R)) + ";" + strconv.Itoa(int(c.G)) + ";" + strconv.Itoa(int(c.B)))
	} else {
		sb.WriteString(";" + base + ";5;" + strconv.Itoa(to256(c)))
	}
}

func sgr(sb *strings.Builder, s Sty, truecolor bool) {
	sb.WriteString("\x1b[0")
	if s.A&Bold != 0 {
		sb.WriteString(";1")
	}
	if s.A&Dim != 0 {
		sb.WriteString(";2")
	}
	if s.A&Italic != 0 {
		sb.WriteString(";3")
	}
	if s.A&Underline != 0 {
		sb.WriteString(";4")
	}
	if s.A&Reverse != 0 {
		sb.WriteString(";7")
	}
	if s.A&Strike != 0 {
		sb.WriteString(";9")
	}
	sgrColor(sb, s.FG, true, truecolor)
	sgrColor(sb, s.BG, false, truecolor)
	if s.A&Underline != 0 && s.UL.Set {
		if truecolor {
			sb.WriteString(";58;2;" + strconv.Itoa(int(s.UL.R)) + ";" + strconv.Itoa(int(s.UL.G)) + ";" + strconv.Itoa(int(s.UL.B)))
		} else {
			sb.WriteString(";58;5;" + strconv.Itoa(to256(s.UL)))
		}
	}
	sb.WriteString("m")
}

// Render encodes the buffer as lines of ANSI text.
func (b *Buf) Render(truecolor bool) string {
	var sb strings.Builder
	sb.Grow(b.W * b.H * 4)
	for y := 0; y < b.H; y++ {
		if y > 0 {
			sb.WriteByte('\n')
		}
		var last Sty
		first := true
		for x := 0; x < b.W; x++ {
			c := b.cells[y*b.W+x]
			if c.r == 0 {
				continue
			}
			if first || c.s != last {
				sgr(&sb, c.s, truecolor)
				last, first = c.s, false
			}
			sb.WriteRune(c.r)
		}
		sb.WriteString("\x1b[0m")
	}
	return sb.String()
}
