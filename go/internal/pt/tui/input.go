package tui

// Input is every editor control from ui-spec §3 in one type: text, password,
// number, date (+ calendar), select, fuzzy combobox, suggest, chips, bool and
// multi-line text. The same widget edits grid cells and dialog fields.

import (
	"fmt"
	"strconv"
	"strings"
	"unicode"

	tea "charm.land/bubbletea/v2"

	"git.wharf.fluidgrid.org/jcz/tictactoe/go/internal/pt/core"
)

type InputKind int

const (
	IText InputKind = iota
	IPassword
	INumber
	IInt
	IDate
	ISelect
	ICombo
	ISuggest
	IChips
	IBool
	IArea
)

func kindFor(e core.Editor) InputKind {
	switch e {
	case core.EdNumber:
		return INumber
	case core.EdInt:
		return IInt
	case core.EdDate:
		return IDate
	case core.EdSelect:
		return ISelect
	case core.EdCombo:
		return ICombo
	case core.EdSuggest:
		return ISuggest
	case core.EdChips:
		return IChips
	case core.EdBool:
		return IBool
	case core.EdMultiline:
		return IArea
	}
	return IText
}

func (k InputKind) isList() bool { return k == ISelect || k == ICombo || k == ISuggest || k == IChips }
func (k InputKind) isText() bool {
	return k == IText || k == IPassword || k == INumber || k == IInt || k == IDate || k == ISuggest
}

// InputResult tells the owner what a key did.
type InputResult int

const (
	RNone       InputResult = iota // handled, keep editing
	RIgnored                       // not handled
	RCommit                        // Enter
	RCommitNext                    // Tab
	RCommitPrev                    // Shift+Tab
	RCancel                        // Esc
)

const popupRows = 8

type Input struct {
	Kind        InputKind
	Label       string
	Placeholder string
	Required    bool
	Err         string
	Hint        string // shown under the field in forms

	// What it edits (for choices and parsing); empty for plain inputs.
	Ent         core.Entity
	Field       string
	RowID       int64
	ProjectHint *int64

	text []rune
	cur  int

	// list kinds
	ChoicesFn   func() []core.Choice
	choices     []core.Choice
	filtered    []core.FilteredChoice
	sel, top    int
	navigated   bool
	value       any
	hasValue    bool
	chips       []core.Choice
	AllowCreate bool // combo: offer "✚ Create …"

	// date
	cal  core.Date
	date *core.Date

	// area
	lines         [][]rune
	row, col, scr int
	Height        int

	today core.Date
}

func NewInput(kind InputKind, label string) *Input {
	return &Input{Kind: kind, Label: label, lines: [][]rune{{}}}
}

// SetText replaces the text (cursor at end).
func (in *Input) SetText(s string) {
	if in.Kind == IArea {
		in.lines = nil
		for _, l := range strings.Split(s, "\n") {
			in.lines = append(in.lines, []rune(l))
		}
		in.row, in.col = 0, 0
		return
	}
	in.text = []rune(s)
	in.cur = len(in.text)
	in.refilter()
}

func (in *Input) Text() string {
	if in.Kind == IArea {
		parts := make([]string, len(in.lines))
		for i, l := range in.lines {
			parts[i] = string(l)
		}
		return strings.Join(parts, "\n")
	}
	return string(in.text)
}

// SetValue sets a list/bool value and shows its label.
func (in *Input) SetValue(v any, choices []core.Choice) {
	in.value, in.hasValue = v, true
	if in.Kind == IChips {
		in.chips = nil
		if list, ok := core.AsList(v); ok {
			for _, x := range list {
				in.chips = append(in.chips, choiceFor(x, choices))
			}
		}
		return
	}
	if in.Kind == ISelect || in.Kind == ICombo {
		c := choiceFor(v, choices)
		in.text = []rune(c.Label)
		if v == nil {
			in.text = nil
		}
		in.cur = len(in.text)
	}
}

func choiceFor(v any, choices []core.Choice) core.Choice {
	for _, c := range choices {
		if core.ValuesEqual(c.Value, v) {
			return c
		}
	}
	return core.Choice{Value: v, Label: fmt.Sprint(v)}
}

// Value returns the selected value for list/bool kinds.
func (in *Input) Value() (any, bool) {
	switch in.Kind {
	case IChips:
		out := make([]any, len(in.chips))
		for i, c := range in.chips {
			out[i] = c.Value
		}
		return out, true
	case ISelect, ICombo:
		if in.navigated || (len(in.text) > 0 && !in.matchesValue()) {
			if c, ok := in.selected(); ok {
				return c.Value, true
			}
			if len(in.text) == 0 {
				return nil, true
			}
			return nil, false
		}
		if len(in.text) == 0 {
			return nil, true
		}
		return in.value, in.hasValue
	case IBool:
		return in.value, in.hasValue
	}
	return nil, false
}

// matchesValue: the text still shows the label of the current value.
func (in *Input) matchesValue() bool {
	if !in.hasValue {
		return false
	}
	return string(in.text) == choiceFor(in.value, in.allChoices()).Label
}

func (in *Input) allChoices() []core.Choice {
	if in.choices == nil && in.ChoicesFn != nil {
		in.choices = in.ChoicesFn()
	}
	return in.choices
}

// Reload re-fetches choices (cascading selects call this when a parent changes).
func (in *Input) Reload() {
	in.choices = nil
	in.refilter()
}

func (in *Input) query() string {
	if in.Kind == ISelect || in.Kind == ICombo {
		if in.matchesValue() {
			return ""
		}
	}
	return string(in.text)
}

func (in *Input) refilter() {
	if !in.Kind.isList() {
		return
	}
	ch := in.allChoices()
	in.filtered = core.FilterChoices(ch, in.query())
	if in.Kind == IChips {
		// hide chosen values
		var keep []core.FilteredChoice
		for _, f := range in.filtered {
			chosen := false
			for _, c := range in.chips {
				if core.ValuesEqual(c.Value, ch[f.Index].Value) {
					chosen = true
				}
			}
			if !chosen {
				keep = append(keep, f)
			}
		}
		in.filtered = keep
	}
	if in.sel >= len(in.filtered)+in.createRows() {
		in.sel = 0
	}
	if in.sel < in.top {
		in.top = in.sel
	}
}

func (in *Input) createRows() int {
	if in.AllowCreate && in.canCreate() {
		return 1
	}
	return 0
}

func (in *Input) canCreate() bool {
	q := strings.TrimSpace(string(in.text))
	if q == "" {
		return false
	}
	for _, c := range in.allChoices() {
		if strings.EqualFold(c.Label, q) {
			return false
		}
	}
	return true
}

// selected returns the highlighted choice.
func (in *Input) selected() (core.Choice, bool) {
	if in.sel < len(in.filtered) && in.sel >= 0 {
		return in.allChoices()[in.filtered[in.sel].Index], true
	}
	return core.Choice{}, false
}

// CreateRequested: the highlighted row is "✚ Create …".
func (in *Input) CreateRequested() (string, bool) {
	if in.createRows() == 1 && in.sel == len(in.filtered) {
		return strings.TrimSpace(string(in.text)), true
	}
	return "", false
}

// SetToday enables natural-date previews.
func (in *Input) SetToday(d core.Date) {
	in.today = d
	if in.cal == 0 {
		in.cal = d
	}
	in.parseDate()
}

func (in *Input) parseDate() {
	if in.Kind != IDate {
		return
	}
	d, err := core.ParseNatural(string(in.text), in.today)
	if err == nil {
		in.date = d
		if d != nil {
			in.cal = *d
		}
		in.Err = ""
	} else {
		in.date = nil
	}
}

// DatePreview is "fri → Fri 23 Oct 2026" or the parse error.
func (in *Input) DatePreview() (string, bool) {
	t := strings.TrimSpace(string(in.text))
	if t == "" {
		return "empty (no date)", true
	}
	d, err := core.ParseNatural(t, in.today)
	if err != nil {
		return err.Error(), false
	}
	if d == nil {
		return "empty", true
	}
	return t + " → " + d.Long(), true
}

func (in *Input) setCalDate(d core.Date) {
	in.cal = d
	in.text = []rune(d.ISO())
	in.cur = len(in.text)
	dd := d
	in.date = &dd
}

// ---- keys ----

func (in *Input) Key(k tea.KeyPressMsg) InputResult {
	key := k.Keystroke()
	switch key {
	case "esc":
		return RCancel
	case "tab":
		if in.Kind == IChips && in.addTyped() {
			return RNone
		}
		return RCommitNext
	case "shift+tab":
		return RCommitPrev
	}
	switch in.Kind {
	case IArea:
		return in.areaKey(k, key)
	case IBool:
		switch key {
		case "space", "enter", "left", "right", "y", "n":
			if key == "enter" {
				return RCommit
			}
			b, _ := in.value.(bool)
			switch key {
			case "y":
				b = true
			case "n":
				b = false
			default:
				b = !b
			}
			in.value, in.hasValue = b, true
			return RNone
		}
		return RIgnored
	case IDate:
		switch key {
		case "up":
			in.setCalDate(in.cal.AddDays(-7))
			return RNone
		case "down":
			in.setCalDate(in.cal.AddDays(7))
			return RNone
		case "pgup":
			in.setCalDate(in.cal.AddMonths(-1))
			return RNone
		case "pgdown":
			in.setCalDate(in.cal.AddMonths(1))
			return RNone
		case "ctrl+left":
			in.setCalDate(in.cal.AddDays(-1))
			return RNone
		case "ctrl+right":
			in.setCalDate(in.cal.AddDays(1))
			return RNone
		case "enter":
			if _, ok := in.DatePreview(); !ok {
				in.Err = "can't read that date"
				return RNone
			}
			return RCommit
		}
	case INumber, IInt:
		switch key {
		case "up", "down", "shift+up", "shift+down":
			step := 1.0
			if strings.HasPrefix(key, "shift") && in.Kind == INumber {
				step = 0.25
			}
			if strings.HasSuffix(key, "down") {
				step = -step
			}
			v, _ := strconv.ParseFloat(strings.TrimSpace(strings.TrimSuffix(string(in.text), "h")), 64)
			v = max(0, v+step)
			in.SetText(core.FmtHours(v))
			return RNone
		}
	}
	if in.Kind.isList() {
		n := len(in.filtered) + in.createRows()
		switch key {
		case "up":
			if in.sel > 0 {
				in.sel--
			}
			in.navigated = true
			in.scrollSel()
			return RNone
		case "down":
			if in.sel < n-1 {
				in.sel++
			}
			in.navigated = true
			in.scrollSel()
			return RNone
		case "pgup":
			in.sel = max(0, in.sel-popupRows)
			in.navigated = true
			in.scrollSel()
			return RNone
		case "pgdown":
			in.sel = max(0, min(n-1, in.sel+popupRows))
			in.navigated = true
			in.scrollSel()
			return RNone
		case "enter":
			if in.Kind == IChips {
				if in.addTyped() {
					return RNone
				}
				return RCommit
			}
			if in.Kind == ISuggest && !in.navigated {
				// autocomplete: "bu" + Enter → "Build" (free text otherwise)
				if c, ok := in.selected(); ok && len(in.text) > 0 &&
					strings.HasPrefix(strings.ToLower(c.Label), strings.ToLower(string(in.text))) {
					in.text = []rune(c.Label)
					in.cur = len(in.text)
				}
				return RCommit
			}
			if c, ok := in.selected(); ok && in.Kind == ISuggest {
				in.text = []rune(c.Label)
				in.cur = len(in.text)
			}
			return RCommit
		case "ctrl+enter":
			return RCommit
		case "backspace":
			if in.Kind == IChips && len(in.text) == 0 && len(in.chips) > 0 {
				in.chips = in.chips[:len(in.chips)-1]
				in.refilter()
				return RNone
			}
		}
	}
	if key == "enter" {
		return RCommit
	}
	if in.textKey(k, key) {
		in.navigated = false
		in.sel, in.top = 0, 0
		in.refilter()
		in.parseDate()
		return RNone
	}
	return RIgnored
}

func (in *Input) scrollSel() {
	if in.sel < in.top {
		in.top = in.sel
	}
	if in.sel >= in.top+popupRows {
		in.top = in.sel - popupRows + 1
	}
}

// addTyped adds the highlighted suggestion (or typed tag) as a chip.
func (in *Input) addTyped() bool {
	q := strings.TrimSpace(string(in.text))
	if c, ok := in.selected(); ok && (in.navigated || q != "") {
		in.chips = append(in.chips, c)
	} else if q != "" && in.Field == "tags" || q != "" && in.Field == "options" {
		in.chips = append(in.chips, core.Choice{Value: strings.ToLower(q), Label: strings.ToLower(q)})
		if in.Field == "options" {
			in.chips[len(in.chips)-1] = core.Choice{Value: q, Label: q}
		}
	} else {
		return false
	}
	in.text, in.cur, in.sel, in.top, in.navigated = nil, 0, 0, 0, false
	in.refilter()
	return true
}

// Insert types text at the cursor (also used for pastes and the first key).
func (in *Input) Insert(s string) {
	if in.Kind == IArea {
		for _, r := range s {
			if r == '\n' {
				in.newline()
			} else if r != '\r' {
				l := in.lines[in.row]
				l = append(l[:in.col], append([]rune{r}, l[in.col:]...)...)
				in.lines[in.row] = l
				in.col++
			}
		}
		return
	}
	s = strings.ReplaceAll(strings.ReplaceAll(s, "\r", ""), "\n", " ")
	r := []rune(s)
	in.text = append(in.text[:in.cur], append(r, in.text[in.cur:]...)...)
	in.cur += len(r)
	in.navigated = false
	in.sel, in.top = 0, 0
	in.refilter()
	in.parseDate()
}

func isWord(r rune) bool { return unicode.IsLetter(r) || unicode.IsDigit(r) }

func (in *Input) textKey(k tea.KeyPressMsg, key string) bool {
	switch key {
	case "left":
		if in.cur > 0 {
			in.cur--
		}
	case "right":
		if in.cur < len(in.text) {
			in.cur++
		}
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
		if (in.Kind == ISelect || in.Kind == ICombo) && in.matchesValue() {
			in.text, in.cur = nil, 0
		} else if in.cur > 0 {
			in.text = append(in.text[:in.cur-1], in.text[in.cur:]...)
			in.cur--
		}
	case "delete":
		if in.cur < len(in.text) {
			in.text = append(in.text[:in.cur], in.text[in.cur+1:]...)
		}
	case "ctrl+u":
		in.text, in.cur = nil, 0
	case "ctrl+w", "ctrl+backspace", "alt+backspace":
		i := in.cur
		for i > 0 && !isWord(in.text[i-1]) {
			i--
		}
		for i > 0 && isWord(in.text[i-1]) {
			i--
		}
		in.text = append(in.text[:i], in.text[in.cur:]...)
		in.cur = i
	default:
		if k.Text == "" || k.Mod&(tea.ModCtrl|tea.ModAlt) != 0 {
			return false
		}
		if (in.Kind == ISelect || in.Kind == ICombo) && in.matchesValue() {
			in.text, in.cur = nil, 0
		}
		if in.Kind == INumber || in.Kind == IInt {
			for _, r := range k.Text {
				if !(unicode.IsDigit(r) || r == '.' || r == '-' || r == 'h') {
					in.Err = "numbers only"
					return true
				}
			}
		}
		in.Err = ""
		in.Insert(k.Text)
		return true
	}
	return true
}

func (in *Input) newline() {
	l := in.lines[in.row]
	rest := append([]rune{}, l[in.col:]...)
	in.lines[in.row] = l[:in.col]
	in.lines = append(in.lines[:in.row+1], append([][]rune{rest}, in.lines[in.row+1:]...)...)
	in.row++
	in.col = 0
}

func (in *Input) areaKey(k tea.KeyPressMsg, key string) InputResult {
	l := in.lines[in.row]
	switch key {
	case "enter":
		in.newline()
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
		if in.row > 0 {
			in.row--
			in.col = min(in.col, len(in.lines[in.row]))
		}
	case "down":
		if in.row < len(in.lines)-1 {
			in.row++
			in.col = min(in.col, len(in.lines[in.row]))
		}
	case "home":
		in.col = 0
	case "end":
		in.col = len(l)
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
		}
	case "delete":
		if in.col < len(l) {
			in.lines[in.row] = append(l[:in.col], l[in.col+1:]...)
		} else if in.row < len(in.lines)-1 {
			in.lines[in.row] = append(l, in.lines[in.row+1]...)
			in.lines = append(in.lines[:in.row+1], in.lines[in.row+2:]...)
		}
	default:
		if k.Text == "" || k.Mod&(tea.ModCtrl|tea.ModAlt) != 0 {
			return RIgnored
		}
		in.Insert(k.Text)
	}
	return RNone
}

// ---- drawing ----

// Draw renders the one-line field (or the text area) in r.
func (in *Input) Draw(b *Buf, r Rect, t *Theme, focused bool) {
	bg := t.Panel2
	if focused {
		bg = mix(t.Panel2, t.Accent, 0.18)
	}
	st := Sty{FG: t.FG, BG: bg}
	b.Fill(r, ' ', st)
	if in.Kind == IArea {
		in.drawArea(b, r, st, t, focused)
		return
	}
	x := r.X
	w := r.W
	if in.Kind == IChips {
		for _, c := range in.chips {
			lbl := "[" + Ellipsis(c.Label, 18) + " ×]"
			cw := StrW(lbl)
			if cw+1 > w-4 {
				b.Text(x, r.Y, "…", st.Fg(t.Muted), 1)
				x++
				w--
				break
			}
			b.Text(x, r.Y, lbl, st.Fg(t.Accent2), cw)
			x += cw + 1
			w -= cw + 1
		}
	}
	if in.Kind == IBool {
		v, _ := in.value.(bool)
		lbl := "[ ] no"
		if v {
			lbl = "[✓] yes"
		}
		b.Text(x, r.Y, lbl, st, w)
		return
	}
	text := in.text
	if in.Kind == IPassword {
		text = []rune(strings.Repeat("•", len(text)))
	}
	if len(text) == 0 && !focused && in.Placeholder != "" {
		b.Text(x, r.Y, in.Placeholder, st.Fg(t.Muted), w)
	}
	// scroll horizontally to keep the cursor visible
	start := 0
	if in.cur >= w-1 {
		start = in.cur - w + 2
	}
	used := 0
	for i := start; i < len(text) && used < w; i++ {
		used += b.Set(x+used, r.Y, text[i], st)
	}
	if focused {
		cx := x + StrW(string(text[start:min(in.cur, len(text))]))
		if cx < r.Right() {
			ch := ' '
			if in.cur < len(text) {
				ch = text[in.cur]
			}
			b.Set(cx, r.Y, ch, Sty{FG: bg, BG: t.FG})
		}
	}
	if in.Kind.isList() && in.Kind != IChips {
		b.Set(r.Right()-1, r.Y, '▾', st.Fg(t.Muted))
	}
	if in.Err != "" {
		b.Restyle(r, func(s Sty) Sty { return s.ULine(t.Err) })
	}
}

func (in *Input) drawArea(b *Buf, r Rect, st Sty, t *Theme, focused bool) {
	if in.row < in.scr {
		in.scr = in.row
	}
	if in.row >= in.scr+r.H {
		in.scr = in.row - r.H + 1
	}
	for i := 0; i < r.H && in.scr+i < len(in.lines); i++ {
		l := in.lines[in.scr+i]
		b.Text(r.X, r.Y+i, string(l), st, r.W)
		if focused && in.scr+i == in.row {
			cx := r.X + StrW(string(l[:min(in.col, len(l))]))
			ch := ' '
			if in.col < len(l) {
				ch = l[in.col]
			}
			if cx < r.Right() {
				b.Set(cx, r.Y+i, ch, Sty{FG: st.BG, BG: t.FG})
			}
		}
	}
}

// PopupSize is the size of the list/calendar popup (0,0 if none).
func (in *Input) PopupSize() (int, int) {
	switch {
	case in.Kind == IDate:
		return 26, 11
	case in.Kind.isList():
		n := min(popupRows, len(in.filtered)+in.createRows())
		if n == 0 {
			n = 1
		}
		w := 24
		ch := in.allChoices()
		for _, f := range in.filtered {
			c := ch[f.Index]
			w = max(w, StrW(c.Label)+StrW(c.Detail)+6)
		}
		return min(w, 60), n + 2
	}
	return 0, 0
}

// DrawPopup draws the list or calendar at r and returns click targets.
func (in *Input) DrawPopup(b *Buf, r Rect, t *Theme) []Hit {
	var hits []Hit
	pst := Sty{FG: t.FG, BG: t.Panel}
	b.Box(r, Sty{FG: t.Accent, BG: t.Panel}, pst, "", pst)
	b.Shadow(r, t.Shadow)
	inner := r.Inset(1)
	if in.Kind == IDate {
		return in.drawCalendar(b, inner, t)
	}
	ch := in.allChoices()
	if len(in.filtered)+in.createRows() == 0 {
		b.Text(inner.X+1, inner.Y, "no matches", pst.Fg(t.Muted), inner.W-1)
		return nil
	}
	for i := 0; i < inner.H; i++ {
		idx := in.top + i
		y := inner.Y + i
		row := Rect{inner.X, y, inner.W, 1}
		if idx >= len(in.filtered)+in.createRows() {
			break
		}
		st := pst
		if idx == in.sel {
			st = Sty{FG: t.CellFG, BG: t.CellBG}
		}
		b.Fill(row, ' ', st)
		if idx == len(in.filtered) {
			b.Text(row.X+1, y, "✚ Create '"+strings.TrimSpace(string(in.text))+"'", st.Fg(t.OK).With(Bold), row.W-1)
			hits = append(hits, Hit{R: row, Kind: HitPopupItem, A: idx})
			continue
		}
		f := in.filtered[idx]
		c := ch[f.Index]
		cst, glyph := t.cellStyle(c.Style, st)
		if idx == in.sel {
			cst.BG = st.BG
			if c.Style.Kind != core.StMuted {
				cst.FG = st.FG
			}
		}
		x := row.X + 1
		x += b.Text(x, y, glyph, cst, row.W-2)
		matched := map[int]bool{}
		for _, m := range f.Matched {
			matched[m] = true
		}
		for i, rr := range []rune(c.Label) {
			s := cst
			if matched[i] {
				s = s.With(Bold | Underline)
			}
			if x >= row.Right()-1 {
				break
			}
			x += b.Set(x, y, rr, s)
		}
		if c.Detail != "" {
			dw := StrW(c.Detail)
			dx := row.Right() - dw - 1
			if dx > x+1 {
				b.Text(dx, y, c.Detail, st.Fg(t.Muted), dw)
			}
		}
		hits = append(hits, Hit{R: row, Kind: HitPopupItem, A: idx})
	}
	n := len(in.filtered) + in.createRows()
	if n > inner.H {
		// scroll indicator
		pos := inner.Y + in.top*inner.H/n
		b.Set(r.Right()-1, pos, '┃', Sty{FG: t.Accent, BG: t.Panel})
	}
	return hits
}

func (in *Input) drawCalendar(b *Buf, r Rect, t *Theme) []Hit {
	var hits []Hit
	pst := Sty{FG: t.FG, BG: t.Panel}
	prev, ok := in.DatePreview()
	pstyle := pst.Fg(t.OK)
	if !ok {
		pstyle = pst.Fg(t.Err)
	}
	b.Fit(r.X+1, r.Y, prev, pstyle, r.W-2, 0)
	first := in.cal.FirstOfMonth()
	y, m, _ := in.cal.YMD()
	title := fmt.Sprintf("%s %d", core.MonthNames[m-1], y)
	b.Text(r.X+1, r.Y+1, "◂", pst.Fg(t.Accent), 1)
	hits = append(hits, Hit{R: Rect{r.X + 1, r.Y + 1, 2, 1}, Kind: HitCalPrev})
	b.Fit(r.X+3, r.Y+1, title, pst.With(Bold), r.W-6, 2)
	b.Text(r.Right()-2, r.Y+1, "▸", pst.Fg(t.Accent), 1)
	hits = append(hits, Hit{R: Rect{r.Right() - 3, r.Y + 1, 2, 1}, Kind: HitCalNext})
	for i, d := range []string{"Mo", "Tu", "We", "Th", "Fr", "Sa", "Su"} {
		b.Text(r.X+1+i*3, r.Y+2, d, pst.Fg(t.Muted), 2)
	}
	d := first.WeekStart()
	for row := 0; row < 6; row++ {
		for col := 0; col < 7; col++ {
			x, yy := r.X+1+col*3, r.Y+3+row
			st := pst
			if d.Month() != m {
				st = st.Fg(t.Muted).With(Dim)
			}
			if col >= 5 {
				st = st.Fg(mix(st.FG, t.Muted, 0.4))
			}
			if d == in.today {
				st = st.Fg(t.Accent).With(Bold)
			}
			if in.date != nil && d == *in.date || in.date == nil && d == in.cal {
				st = Sty{FG: t.CellFG, BG: t.CellBG, A: Bold}
			}
			b.Fit(x, yy, strconv.Itoa(d.Day()), st, 2, 1)
			hits = append(hits, Hit{R: Rect{x, yy, 2, 1}, Kind: HitCalDay, A: int(d)})
			d = d.AddDays(1)
		}
	}
	return hits
}

// Click handles a popup hit; returns true if the editor should commit.
func (in *Input) Click(h Hit) bool {
	switch h.Kind {
	case HitPopupItem:
		in.sel = h.A
		in.navigated = true
		if in.Kind == IChips {
			in.addTyped()
			return false
		}
		if c, ok := in.selected(); ok && in.Kind == ISuggest {
			in.text = []rune(c.Label)
			in.cur = len(in.text)
		}
		return true
	case HitCalPrev:
		in.setCalDate(in.cal.AddMonths(-1))
	case HitCalNext:
		in.setCalDate(in.cal.AddMonths(1))
	case HitCalDay:
		in.setCalDate(core.Date(h.A))
		return true
	}
	return false
}

// Wheel scrolls the popup list.
func (in *Input) Wheel(delta int) {
	n := len(in.filtered) + in.createRows()
	in.sel = max(0, min(n-1, in.sel+delta))
	in.navigated = true
	in.scrollSel()
}

// Revert shows the current value's label again (after a non-matching entry).
func (in *Input) Revert() {
	if in.Kind != ISelect && in.Kind != ICombo {
		return
	}
	if in.hasValue && in.value != nil {
		in.text = []rune(choiceFor(in.value, in.allChoices()).Label)
	} else {
		in.text = nil
	}
	in.cur = len(in.text)
	in.navigated = false
	in.refilter()
}
