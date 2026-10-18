package tui

// Tab 3 Date: two natural-date fields (protocol §6) with a live preview and
// a month calendar for the focused one. The calendar marks today, the other
// date of the pair and shades the range between them.

import (
	"fmt"

	tea "charm.land/bubbletea/v2"

	"git.wharf.fluidgrid.org/jcz/tictactoe/go/internal/demo/core"
)

type dateTab struct {
	ctlBase
	fields [2]*syncField
}

func newDateTab() *dateTab {
	t := &dateTab{}
	for i, kl := range [][2]string{{"date.start", "Start"}, {"date.due", "Due"}} {
		key := kl[0]
		f := newSyncField(key, kl[1])
		f.show = func(a *App) string {
			if s := a.vals.OptString(key); s != nil {
				return core.SafeText(*s)
			}
			return ""
		}
		f.parse = func(a *App, s string) (any, error) {
			d, err := core.ParseNatural(s, a.today)
			if err != nil {
				return nil, err
			}
			if d == nil {
				return nil, nil
			}
			return d.ISO(), nil
		}
		f.in.max = 30
		f.in.placeholder = "e.g. fri, +3d"
		t.fields[i] = f
	}
	return t
}

func (t *dateTab) explain() string {
	return "Natural dates (today, fri, +3d, eom, 23 oct, 2026-11-02) with a live preview and a calendar; due can't be before start."
}

func (t *dateTab) controls() int { return 2 }

func (t *dateTab) keyHelp() [][2]string {
	return [][2]string{
		{"type", "a date"}, {"↑↓", "±1 week"}, {"^←/^→", "±1 day"}, {"PgUp/PgDn", "±1 month"},
		{"^T", "today"}, {"Enter", "save"}, {"Del", "clear (at end)"}, {"Esc", "revert"}, {"Tab", "next field"},
	}
}

func (t *dateTab) hints(a *App) []hint {
	return []hint{h("enter", "Save"), {"↑↓", "±week", ""}, {"PgUp/Dn", "±month", ""}, {"^←→", "±day", ""},
		h("ctrl+t", "Today"), {"Del", "Clear", "delete"}, {"Esc", "Revert", "esc"}}
}

func (t *dateTab) changed(a *App, key string, kind changeKind, by string) {
	for _, f := range t.fields {
		if f.key == key {
			f.changed(a, kind, by)
		}
	}
}

// date is the date a field shows: its parsed draft, else its value.
func (t *dateTab) date(a *App, i int) (core.Date, bool) {
	f := t.fields[i]
	f.sync(a)
	d, err := core.ParseNatural(f.in.Text(), a.today)
	if err == nil && d != nil {
		return *d, true
	}
	if f.dirty {
		return 0, false
	}
	return a.vals.Date(f.key)
}

// moveCal moves the focused field's date by days / months.
func (t *dateTab) moveCal(a *App, days, months int) {
	d, ok := t.date(a, t.cur)
	if !ok {
		d = a.today
	} else {
		d = d.AddMonths(months).AddDays(days)
	}
	t.setDate(a, d)
}

func (t *dateTab) setDate(a *App, d core.Date) {
	f := t.fields[t.cur]
	f.in.SetText(d.ISO())
	f.touch(a)
}

func (t *dateTab) key(a *App, k tea.KeyPressMsg) bool {
	f := t.fields[t.cur]
	f.sync(a)
	switch k.Keystroke() {
	case "up":
		t.moveCal(a, -7, 0)
	case "down":
		t.moveCal(a, 7, 0)
	case "ctrl+left":
		t.moveCal(a, -1, 0)
	case "ctrl+right":
		t.moveCal(a, 1, 0)
	case "pgup":
		t.moveCal(a, 0, -1)
	case "pgdown":
		t.moveCal(a, 0, 1)
	case "ctrl+t":
		t.setDate(a, a.today)
	case "enter":
		f.commit(a)
	case "esc":
		return f.revert(a)
	case "delete":
		if f.in.cur >= len(f.in.text) {
			f.in.SetText("")
			f.touch(a)
			f.commit(a)
			return true
		}
		return f.edit(a, k)
	default:
		return f.edit(a, k)
	}
	return true
}

func (t *dateTab) paste(a *App, s string) { t.fields[t.cur].insert(a, s) }

func (t *dateTab) draw(a *App, b *Buf, r Rect, focused bool) []Hit {
	th := a.th()
	var hits []Hit
	gap := choose(r.H >= 7, 3, 2)
	for i, f := range t.fields {
		y := r.Y + i*gap
		fr := f.drawField(a, b, r.X, y, 7, 16, focused && t.cur == i, r)
		tb := button(b, th, fr.Right()+1, y, "t", false, th.CellBG)
		hits = append(hits, Hit{R: Rect{r.X, y, fr.Right() - r.X, 1}, Kind: HitCtl, S: "field", A: i},
			Hit{R: tb, Kind: HitCtl, S: "today", A: i})
		// preview / error under the field
		msg, st := "", th.base().Fg(th.OK)
		switch {
		case f.err != "":
			msg, st = "✗ "+f.err, th.base().Fg(th.Err)
		case f.theirs != "":
			msg, st = f.theirs+" · Esc takes theirs", th.base().Fg(th.Warn)
		case f.in.Text() == "":
			msg, st = "— no date —", th.base().Fg(th.Muted)
		default:
			p, ok := core.DatePreview(f.in.Text(), a.today)
			msg = p
			if !ok {
				st = th.base().Fg(th.Err)
			}
		}
		b.Text(r.X+7, y+1, msg, st, 33)
	}
	// calendar beside the fields
	cx := r.X + 38
	if cx+22 > r.Right() {
		return hits
	}
	return append(hits, t.drawCalendar(a, b, Rect{cx, r.Y, 22, min(r.H, 8)})...)
}

func (t *dateTab) drawCalendar(a *App, b *Buf, r Rect) []Hit {
	th := a.th()
	var hits []Hit
	base := th.base()
	cur, ok := t.date(a, t.cur)
	if !ok {
		cur = a.today
	}
	other, hasOther := t.date(a, 1-t.cur)
	start, okS := t.date(a, 0)
	due, okD := t.date(a, 1)
	y, m, _ := cur.YMD()
	b.Text(r.X, r.Y, "◂", base.Fg(th.Accent).With(Bold), 1)
	hits = append(hits, Hit{R: Rect{r.X, r.Y, 2, 1}, Kind: HitCtl, S: "calprev", A: t.cur})
	b.Fit(r.X+2, r.Y, fmt.Sprintf("%s %d", core.MonthNames[m-1], y), base.With(Bold), r.W-4, 2)
	b.Text(r.Right()-2, r.Y, "▸", base.Fg(th.Accent).With(Bold), 1)
	hits = append(hits, Hit{R: Rect{r.Right() - 3, r.Y, 2, 1}, Kind: HitCtl, S: "calnext", A: t.cur})
	for i, d := range []string{"Mo", "Tu", "We", "Th", "Fr", "Sa", "Su"} {
		b.Text(r.X+i*3, r.Y+1, d, base.Fg(th.Muted), 2)
	}
	d := cur.FirstOfMonth().WeekStart()
	for row := 0; row < 6 && r.Y+2+row < r.Bottom(); row++ {
		for col := 0; col < 7; col++ {
			x, yy := r.X+col*3, r.Y+2+row
			st := base
			if okS && okD && d > start && d < due {
				st = Sty{FG: th.FG, BG: th.SelBG} // the range between the dates
				if col < 6 {
					b.Set(x+2, yy, ' ', st)
				}
			}
			if d.Month() != m {
				st = st.Fg(th.Muted).With(Dim)
			} else if col >= 5 {
				st = st.Fg(mix(th.FG, th.Muted, 0.5))
			}
			if d == a.today {
				st = st.Fg(th.Accent).With(Bold | Underline)
			}
			if hasOther && d == other {
				st = Sty{FG: th.BG, BG: th.Accent2, A: Bold}
			}
			if d == cur {
				st = Sty{FG: th.CellFG, BG: th.CellBG, A: Bold}
			}
			b.Fit(x, yy, fmt.Sprint(d.Day()), st, 2, 1)
			hits = append(hits, Hit{R: Rect{x, yy, 2, 1}, Kind: HitCtl, S: "day", A: t.cur, B: int(d)})
			d = d.AddDays(1)
		}
	}
	return hits
}

func (t *dateTab) mouse(a *App, m hitMsg) {
	h := m.hit
	if dd, ok := isWheel(m); ok && (h.S == "day" || h.S == "calprev" || h.S == "calnext") {
		t.moveCal(a, 0, dd)
		return
	}
	if !isClick(m) {
		return
	}
	t.cur = h.A
	f := t.fields[t.cur]
	switch h.S {
	case "today":
		t.setDate(a, a.today)
		f.commit(a)
	case "calprev":
		t.moveCal(a, 0, -1)
	case "calnext":
		t.moveCal(a, 0, 1)
	case "day":
		t.setDate(a, core.Date(h.B))
		f.commit(a)
	}
}
