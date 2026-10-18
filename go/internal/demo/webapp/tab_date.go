//go:build js && wasm

package webapp

// Tab 3 — Date: two natural-date fields (protocol §6) with a live preview
// and a month calendar for the focused field: today highlighted, the other
// date marked and the range between them shaded.

import (
	"fmt"
	"strconv"
	"syscall/js"

	"git.wharf.fluidgrid.org/jcz/tictactoe/go/internal/demo/core"
)

type dfield struct {
	key    string
	label  string
	in     js.Value
	msg    js.Value
	ci     int
	dirty  bool
	base   int64
	theirs string
	err    string
	cursor core.Date
}

type dateTab struct {
	tabBase
	fields [2]*dfield
	active int // the field the calendar belongs to
	cal    js.Value
	calSig string
}

func newDateTab(b tabBase) Tab {
	t := &dateTab{tabBase: b}
	t.build = t.mount
	return t
}

func (t *dateTab) mount(area js.Value) {
	wrap := E("div", "datewrap")
	form := E("div", "form2")
	for i, f := range []struct{ key, label, name string }{{"date.start", "Start", "start"}, {"date.due", "Due", "due"}} {
		i := i
		row, c, msg := field(f.label)
		df := &dfield{key: f.key, label: f.label, msg: msg}
		df.in = E("input", "ed2 date")
		Attr(df.in, "spellcheck", "false")
		Attr(df.in, "autocomplete", "off")
		Attr(df.in, "placeholder", "fri, +3d, 23 oct, eom…")
		df.ci = t.addCtl(df.in, f.name, f.key)
		tb := ET("button", "btn sm", "t")
		Attr(tb, "type", "button")
		Attr(tb, "tabindex", "-1")
		Attr(tb, "title", "today")
		Data(tb, "control", f.name+"-today")
		xb := ET("button", "btn sm ghost", "✕")
		Attr(xb, "type", "button")
		Attr(xb, "tabindex", "-1")
		Attr(xb, "title", "clear (Del)")
		Data(xb, "control", f.name+"-clear")
		Add(c, df.in, tb, xb)
		Add(form, row)
		t.fields[i] = df
		t.scope.On(df.in, "input", func(js.Value) { t.edited(df) })
		t.scope.On(df.in, "focus", func(js.Value) {
			t.activate(i)
			df.in.Call("select") // typing replaces the date (as the Rust web client)
		})
		t.scope.On(tb, "mousedown", func(ev js.Value) {
			ev.Call("preventDefault")
			t.Focus(df.ci)
			t.activate(i)
			t.commitDate(df, today().ISO())
		})
		t.scope.On(xb, "mousedown", func(ev js.Value) {
			ev.Call("preventDefault")
			t.Focus(df.ci)
			t.commitDate(df, "")
		})
	}
	t.cal = E("div", "calendar")
	Data(t.cal, "popup", "calendar")
	Add(wrap, form, t.cal)
	Add(area, wrap)
	t.scope.On(t.cal, "mousedown", func(ev js.Value) {
		ev.Call("preventDefault")
		df := t.fields[t.active]
		tgt := ev.Get("target")
		if nav := Closest(tgt, "[data-nav]"); nav.Truthy() {
			n, _ := strconv.Atoi(DataOf(nav, "nav"))
			df.cursor = df.cursor.AddMonths(n)
		} else if d := Closest(tgt, "[data-date]"); d.Truthy() {
			if day, ok := core.ParseISO(DataOf(d, "date")); ok {
				df.cursor = day
				t.commitDate(df, day.ISO())
			}
		}
		t.Focus(df.ci)
		t.a.Invalidate()
	})
}

// activate makes field i the calendar's, starting at its date (or today).
func (t *dateTab) activate(i int) {
	t.active = i
	df := t.fields[i]
	if d, ok := t.a.v.Date(df.key); ok && !df.dirty {
		df.cursor = d
	} else if df.cursor == 0 {
		df.cursor = today()
	}
	t.a.Invalidate()
}

func (t *dateTab) synced(df *dfield) string {
	if s := t.a.v.OptString(df.key); s != nil {
		return *s
	}
	return ""
}

func (t *dateTab) edited(df *dfield) {
	if !df.dirty {
		df.base = t.a.v.Version(df.key)
	}
	val := df.in.Get("value").String()
	df.dirty = val != t.synced(df)
	df.err = ""
	d, err := core.ParseNatural(val, today())
	switch {
	case err != nil:
		df.err = err.Error()
	case d != nil:
		df.cursor = *d
		if _, err := t.a.v.PrepareSet(df.key, d.ISO()); err != nil {
			df.err = err.Error()
		}
	}
	t.a.Invalidate()
}

// commitDate saves iso ("" = null) for the field.
func (t *dateTab) commitDate(df *dfield, iso string) {
	var v any
	if iso != "" {
		v = iso
	}
	base := int64(-1)
	if df.dirty {
		base = df.base
	}
	if err := t.a.CommitFrom(df.key, v, base, func(m core.AckMsg) {
		if m.OK {
			df.dirty, df.theirs, df.err = false, "", ""
			df.in.Set("value", t.synced(df))
		} else if !m.Conflict {
			df.err = m.Error
		}
	}); err != nil {
		df.err = err.Error()
		return
	}
	df.dirty, df.err = false, ""
	df.in.Set("value", iso)
}

func (t *dateTab) curField() *dfield {
	for _, f := range t.fields {
		if f.ci == t.cur {
			return f
		}
	}
	return nil
}

func (t *dateTab) Key(k Key) bool {
	df := t.curField()
	if df == nil {
		return false
	}
	if t.fields[t.active] != df {
		t.activate(1 - t.active)
	}
	switch {
	case k.Is("ArrowUp"):
		df.cursor = df.cursor.AddDays(-7)
	case k.Is("ArrowDown"):
		df.cursor = df.cursor.AddDays(7)
	case k.Is("C-ArrowLeft"):
		df.cursor = df.cursor.AddDays(-1)
	case k.Is("C-ArrowRight"):
		df.cursor = df.cursor.AddDays(1)
	case k.Is("PageUp"):
		df.cursor = df.cursor.AddMonths(-1)
	case k.Is("PageDown"):
		df.cursor = df.cursor.AddMonths(1)
	case k.Is("Enter"):
		// the typed text if any, else the calendar day
		if df.dirty {
			if df.err != "" {
				t.a.setStatus("err", "✗ "+df.key+": "+df.err)
				return true
			}
			d, _ := core.ParseNatural(df.in.Get("value").String(), today())
			iso := ""
			if d != nil {
				iso = d.ISO()
			}
			t.commitDate(df, iso)
		} else {
			t.commitDate(df, df.cursor.ISO())
		}
	case k.Is("C-u"):
		df.in.Set("value", "")
		t.edited(df)
	case k.Is("Delete") && (!df.dirty || df.in.Get("value").String() == ""):
		t.commitDate(df, "")
	case k.Is("Escape") && (df.dirty || df.err != "" || df.theirs != ""):
		df.dirty, df.err, df.theirs = false, "", ""
		df.in.Set("value", t.synced(df))
		t.activate(t.active)
	default:
		return false
	}
	t.a.Invalidate()
	return true
}

func (t *dateTab) Synced(key string, ev syncEv) {
	for i, df := range t.fields {
		if df.key != key {
			continue
		}
		switch {
		case ev.conflict:
			df.dirty, df.theirs, df.err = false, "", ""
			df.in.Set("value", t.synced(df))
			t.activate(i)
		case ev.welcome || ev.mine || ev.rejected:
		case df.dirty:
			df.theirs = ev.by
		}
	}
}

func (t *dateTab) Render() {
	td := today()
	for _, df := range t.fields {
		if !df.dirty && df.in.Get("value").String() != t.synced(df) {
			df.in.Set("value", t.synced(df))
		}
		cls := "ed2 date"
		val := df.in.Get("value").String()
		switch {
		case df.err != "":
			cls += " bad"
			setMsg(df.msg, "err", "✗ "+df.err)
		case df.theirs != "":
			setMsg(df.msg, "warn", "⚠ changed by "+df.theirs+" — Esc takes theirs")
		case val == "":
			setMsg(df.msg, "", "— none —")
		default:
			p, _ := core.DatePreview(val, td)
			if df.dirty {
				cls += " dirty"
				p += "   ● Enter saves"
			}
			setMsg(df.msg, "preview", p)
		}
		df.in.Set("className", cls)
	}
	t.renderCal(td)
}

func (t *dateTab) renderCal(td core.Date) {
	df := t.fields[t.active]
	if df.cursor == 0 {
		t.activate(t.active)
	}
	other := t.fields[1-t.active]
	start, sok := t.a.v.Date("date.start")
	due, dok := t.a.v.Date("date.due")
	val, vok := t.a.v.Date(df.key)
	oth, ook := t.a.v.Date(other.key)
	focused := t.isCur(df.ci)
	sig := fmt.Sprint(df.cursor, td, start, due, sok, dok, t.active, focused)
	if sig == t.calSig {
		return
	}
	t.calSig = sig
	Clear(t.cal)
	cls := "calendar"
	if focused {
		cls += " focused"
	}
	t.cal.Set("className", cls)
	first := df.cursor.FirstOfMonth()
	head := E("div", "cal-h")
	prev := ET("span", "calnav", "‹")
	Data(prev, "nav", "-1")
	next := ET("span", "calnav", "›")
	Data(next, "nav", "1")
	Add(head, prev, ET("span", "", core.MonthNames[first.Month()-1]+" "+strconv.Itoa(first.Year())), next)
	Add(t.cal, ET("div", "cal-for", df.label+" — "+df.cursor.Long()), head)
	g := E("div", "cal-g")
	for _, w := range core.WeekdayNames {
		Add(g, ET("span", "cal-wd", w[:2]))
	}
	day := first.WeekStart()
	for i := 0; i < 42; i++ {
		c := "cal-d"
		if day.Month() != first.Month() {
			c += " out"
		}
		if day.Weekday() >= 5 {
			c += " we"
		}
		if sok && dok && day > start && day < due {
			c += " range"
		}
		if ook && day == oth {
			c += " other"
		}
		if vok && day == val {
			c += " val"
		}
		if day == td {
			c += " today"
		}
		if day == df.cursor {
			c += " sel"
		}
		d := ET("span", c, strconv.Itoa(day.Day()))
		Data(d, "date", day.ISO())
		Add(g, d)
		day = day.AddDays(1)
	}
	Add(t.cal, g)
	legend := E("div", "cal-foot")
	Add(legend, ET("span", "lg today", "today"), ET("span", "lg val", df.label), ET("span", "lg other", other.label), ET("span", "lg range", "range"))
	Add(t.cal, legend)
}

func (t *dateTab) Hints() []hint {
	return []hint{{"Enter", "Save", "Enter"}, {"↑↓", "±week", ""}, {"^←→", "±day", ""}, {"PgUp/Dn", "±month", ""},
		{"Del", "Clear", "Delete"}, {"Esc", "Revert", "Escape"}, {"Tab", "Next", "Tab"}}
}
