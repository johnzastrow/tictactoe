package tui

// Text editing widgets: lineInput (one line, with masking and an optional
// character filter) and areaInput (multi-line with soft word wrap). Adapted
// from pt's Input, without pt's data model: the tabs decide what the text
// means and when to commit.

import (
	"strings"
	"unicode"

	tea "charm.land/bubbletea/v2"
)

type lineInput struct {
	text []rune
	cur  int
	mask bool // show • for every character
	// upper shows and stores typed letters in upper case (text.code).
	upper bool
	// accept filters typed characters (nil = any printable).
	accept      func(r rune) bool
	placeholder string
	max         int // characters, 0 = no limit
}

func (in *lineInput) Text() string { return string(in.text) }

// SetText replaces the text and puts the cursor at the end.
func (in *lineInput) SetText(s string) {
	in.text = []rune(s)
	in.cur = len(in.text)
}

// Insert types s at the cursor (keys and pastes); newlines become spaces.
// It returns false when nothing could be inserted.
func (in *lineInput) Insert(s string) bool {
	s = strings.ReplaceAll(strings.ReplaceAll(s, "\r", ""), "\n", " ")
	var add []rune
	for _, r := range s {
		if unicode.IsControl(r) {
			continue
		}
		if in.upper {
			r = unicode.ToUpper(r)
		}
		if in.accept != nil && !in.accept(r) {
			continue
		}
		if in.max > 0 && len(in.text)+len(add) >= in.max {
			break
		}
		add = append(add, r)
	}
	if len(add) == 0 {
		return false
	}
	in.text = append(in.text[:in.cur], append(add, in.text[in.cur:]...)...)
	in.cur += len(add)
	return true
}

func isWord(r rune) bool { return unicode.IsLetter(r) || unicode.IsDigit(r) }

// Key applies an editing key. handled reports whether the key belongs to
// the input; changed whether the text changed.
func (in *lineInput) Key(k tea.KeyPressMsg) (handled, changed bool) {
	switch k.Keystroke() {
	case "left":
		in.cur = max(0, in.cur-1)
	case "right":
		in.cur = min(len(in.text), in.cur+1)
	case "home", "ctrl+a":
		in.cur = 0
	case "end", "ctrl+e":
		in.cur = len(in.text)
	case "ctrl+left", "alt+left", "alt+b":
		for in.cur > 0 && !isWord(in.text[in.cur-1]) {
			in.cur--
		}
		for in.cur > 0 && isWord(in.text[in.cur-1]) {
			in.cur--
		}
	case "ctrl+right", "alt+right", "alt+f":
		for in.cur < len(in.text) && !isWord(in.text[in.cur]) {
			in.cur++
		}
		for in.cur < len(in.text) && isWord(in.text[in.cur]) {
			in.cur++
		}
	case "backspace":
		if in.cur == 0 {
			return true, false
		}
		in.text = append(in.text[:in.cur-1], in.text[in.cur:]...)
		in.cur--
		return true, true
	case "delete":
		if in.cur >= len(in.text) {
			return true, false
		}
		in.text = append(in.text[:in.cur], in.text[in.cur+1:]...)
		return true, true
	case "ctrl+u":
		changed = len(in.text) > 0
		in.text, in.cur = nil, 0
		return true, changed
	case "ctrl+w", "ctrl+backspace", "alt+backspace":
		i := in.cur
		for i > 0 && !isWord(in.text[i-1]) {
			i--
		}
		for i > 0 && isWord(in.text[i-1]) {
			i--
		}
		changed = i != in.cur
		in.text = append(in.text[:i], in.text[in.cur:]...)
		in.cur = i
		return true, changed
	default:
		if k.Text == "" || k.Mod&(tea.ModCtrl|tea.ModAlt) != 0 {
			return false, false
		}
		return true, in.Insert(k.Text)
	}
	return true, false
}

// Draw renders the field in r: a tinted background, the text scrolled to
// keep the cursor visible, and a block cursor when focused.
func (in *lineInput) Draw(b *Buf, r Rect, t *Theme, focused bool, st Sty) {
	b.Fill(r, ' ', st)
	text := in.text
	if in.mask {
		text = []rune(strings.Repeat("•", len(text)))
	}
	if len(text) == 0 && in.placeholder != "" {
		b.Text(r.X+choose(focused, 1, 0), r.Y, in.placeholder, st.Fg(t.Muted).With(Italic), r.W-1)
	}
	start := 0
	if in.cur >= r.W-1 {
		start = in.cur - r.W + 2
	}
	used := 0
	for i := start; i < len(text) && used < r.W; i++ {
		used += b.Set(r.X+used, r.Y, text[i], st)
	}
	if focused {
		cx := r.X + StrW(string(text[start:min(in.cur, len(text))]))
		if cx < r.Right() {
			ch := ' '
			if in.cur < len(text) {
				ch = text[in.cur]
			}
			b.Set(cx, r.Y, ch, Sty{FG: st.BG, BG: t.FG})
		}
	}
}

// fieldStyle is the background of an input box (tinted when focused).
func fieldStyle(t *Theme, focused bool) Sty {
	bg := t.Panel2
	if focused {
		bg = mix(t.Panel2, t.Accent, 0.18)
	}
	return Sty{FG: t.FG, BG: bg}
}

// ================= areaInput =================

// areaInput is a multi-line editor with soft word wrap at the width it was
// last drawn with. The cursor is (row, col) in logical lines.
type areaInput struct {
	lines    [][]rune
	row, col int
	scroll   int // first visual row shown
	width    int // wrap width (set by Draw; 0 = no wrap yet)
	height   int // rows shown (set by Draw)
}

func newArea(s string) *areaInput {
	a := &areaInput{}
	a.SetText(s)
	return a
}

func (in *areaInput) SetText(s string) {
	in.lines = nil
	for _, l := range strings.Split(s, "\n") {
		in.lines = append(in.lines, []rune(l))
	}
	in.row, in.col, in.scroll = 0, 0, 0
}

func (in *areaInput) Text() string {
	parts := make([]string, len(in.lines))
	for i, l := range in.lines {
		parts[i] = string(l)
	}
	return strings.Join(parts, "\n")
}

// Runes counts characters (newlines included), as the 5000 limit does.
func (in *areaInput) Runes() int {
	n := len(in.lines) - 1
	for _, l := range in.lines {
		n += len(l)
	}
	return n
}

func (in *areaInput) Words() int { return len(strings.Fields(in.Text())) }

// vrow is one visual (wrapped) row: line index and rune range.
type vrow struct{ line, from, to int }

// wrapRows splits the text into visual rows of at most w cells, breaking
// after spaces where possible.
func (in *areaInput) wrapRows(w int) []vrow {
	var out []vrow
	for li, l := range in.lines {
		if w <= 0 || len(l) <= w {
			out = append(out, vrow{li, 0, len(l)})
			continue
		}
		from := 0
		for from < len(l) {
			to := min(len(l), from+w)
			if to < len(l) {
				// break after the last space in the row, if any
				for j := to; j > from+w/3; j-- {
					if l[j-1] == ' ' {
						to = j
						break
					}
				}
			}
			out = append(out, vrow{li, from, to})
			from = to
		}
	}
	return out
}

// cursorRow is the visual row holding the cursor.
func (in *areaInput) cursorRow(rows []vrow) int {
	for i, r := range rows {
		if r.line != in.row {
			continue
		}
		last := i+1 >= len(rows) || rows[i+1].line != in.row
		if in.col >= r.from && (in.col < r.to || last) {
			return i
		}
	}
	return 0
}

func (in *areaInput) Insert(s string) {
	for _, r := range s {
		switch {
		case r == '\n':
			in.newline()
		case r == '\t':
			in.Insert("    ")
		case unicode.IsControl(r):
		default:
			l := in.lines[in.row]
			l = append(l[:in.col], append([]rune{r}, l[in.col:]...)...)
			in.lines[in.row] = l
			in.col++
		}
	}
}

func (in *areaInput) newline() {
	l := in.lines[in.row]
	rest := append([]rune{}, l[in.col:]...)
	in.lines[in.row] = l[:in.col]
	in.lines = append(in.lines[:in.row+1], append([][]rune{rest}, in.lines[in.row+1:]...)...)
	in.row++
	in.col = 0
}

// moveRows moves the cursor n visual rows, keeping the column.
func (in *areaInput) moveRows(n int) {
	rows := in.wrapRows(in.width)
	cr := in.cursorRow(rows)
	x := in.col - rows[cr].from
	nr := max(0, min(len(rows)-1, cr+n))
	r := rows[nr]
	in.row = r.line
	in.col = min(r.from+x, r.to)
	if nr+1 < len(rows) && rows[nr+1].line == r.line && in.col == r.to && r.to > r.from {
		in.col = r.to - 1 // stay on this visual row
	}
}

// Key applies an editing key; changed reports a text change. Keys it
// doesn't use (Tab, Esc, ^S, …) return handled = false.
func (in *areaInput) Key(k tea.KeyPressMsg) (handled, changed bool) {
	l := in.lines[in.row]
	switch k.Keystroke() {
	case "enter":
		in.newline()
		return true, true
	case "left":
		if in.col > 0 {
			in.col--
		} else if in.row > 0 {
			in.row--
			in.col = len(in.lines[in.row])
		}
	case "right":
		if in.col < len(l) {
			in.col++
		} else if in.row < len(in.lines)-1 {
			in.row++
			in.col = 0
		}
	case "up":
		in.moveRows(-1)
	case "down":
		in.moveRows(1)
	case "pgup":
		in.moveRows(-max(1, in.height-1))
	case "pgdown":
		in.moveRows(max(1, in.height-1))
	case "home":
		rows := in.wrapRows(in.width)
		in.col = rows[in.cursorRow(rows)].from
	case "end":
		rows := in.wrapRows(in.width)
		r := rows[in.cursorRow(rows)]
		in.col = r.to
	case "ctrl+home":
		in.row, in.col = 0, 0
	case "ctrl+end":
		in.row = len(in.lines) - 1
		in.col = len(in.lines[in.row])
	case "backspace":
		if in.col > 0 {
			in.lines[in.row] = append(l[:in.col-1], l[in.col:]...)
			in.col--
		} else if in.row > 0 {
			prev := in.lines[in.row-1]
			in.col = len(prev)
			in.lines[in.row-1] = append(prev, l...)
			in.lines = append(in.lines[:in.row], in.lines[in.row+1:]...)
			in.row--
		} else {
			return true, false
		}
		return true, true
	case "delete":
		if in.col < len(l) {
			in.lines[in.row] = append(l[:in.col], l[in.col+1:]...)
		} else if in.row < len(in.lines)-1 {
			in.lines[in.row] = append(l, in.lines[in.row+1]...)
			in.lines = append(in.lines[:in.row+1], in.lines[in.row+2:]...)
		} else {
			return true, false
		}
		return true, true
	default:
		if k.Text == "" || k.Mod&(tea.ModCtrl|tea.ModAlt) != 0 {
			return false, false
		}
		in.Insert(k.Text)
		return true, true
	}
	return true, false
}

// Draw renders the wrapped text in r and scrolls to keep the cursor in view.
func (in *areaInput) Draw(b *Buf, r Rect, t *Theme, focused bool, st Sty) {
	in.width, in.height = max(1, r.W-1), r.H
	b.Fill(r, ' ', st)
	rows := in.wrapRows(in.width)
	cr := in.cursorRow(rows)
	if cr < in.scroll {
		in.scroll = cr
	}
	if cr >= in.scroll+r.H {
		in.scroll = cr - r.H + 1
	}
	in.scroll = max(0, min(in.scroll, max(0, len(rows)-r.H)))
	for i := 0; i < r.H && in.scroll+i < len(rows); i++ {
		vr := rows[in.scroll+i]
		l := in.lines[vr.line]
		b.Text(r.X, r.Y+i, string(l[vr.from:vr.to]), st, r.W-1)
		if focused && in.scroll+i == cr {
			cx := r.X + StrW(string(l[vr.from:min(in.col, vr.to)]))
			ch := ' '
			if in.col < len(l) && in.col < vr.to {
				ch = l[in.col]
			}
			b.Set(min(cx, r.Right()-1), r.Y+i, ch, Sty{FG: st.BG, BG: t.FG})
		}
	}
	if len(rows) > r.H {
		pos := r.Y + in.scroll*r.H/len(rows)
		b.Set(r.Right()-1, pos, '┃', st.Fg(t.Accent))
	}
}

// Pos is the 1-based line and column for the footer.
func (in *areaInput) Pos() (int, int) { return in.row + 1, in.col + 1 }

// ClickAt puts the cursor at a visual position inside the last drawn area.
func (in *areaInput) ClickAt(dx, dy int) {
	rows := in.wrapRows(in.width)
	i := in.scroll + dy
	if i >= len(rows) {
		i = len(rows) - 1
	}
	if i < 0 {
		return
	}
	vr := rows[i]
	in.row = vr.line
	in.col = min(vr.from+max(0, dx), vr.to)
}
