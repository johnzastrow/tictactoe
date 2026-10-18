//go:build js && wasm

package webapp

// Tab 16 — Gantt: gantt.bars over a time axis (week ticks, weekend shading,
// a today line). Keys move, resize and zoom; the mouse drags a bar to move
// it or its right edge to resize it (committed on release).

import (
	"fmt"
	"math"
	"strconv"
	"syscall/js"

	"git.wharf.fluidgrid.org/jcz/tictactoe/go/internal/demo/core"
)

const (
	gHead = 42.0 // axis header height
	gRow  = 32.0
)

var zoomPPD = []float64{30, 13, 4.5} // pixels per day: day ▸ week ▸ month scale
var zoomNames = []string{"day", "week", "month"}

type ganttTab struct {
	tabBase
	box    js.Value
	info   js.Value
	labels js.Value
	tl     js.Value // horizontal scroller holding the SVG
	sel    int
	zoom   int
	sig    string
	origin core.Date

	// drag
	dragID     int64
	dragResize bool
	dragX      float64
	dragDelta  int
	scrollTo   string // "today" / "sel" after the next rebuild
}

func newGanttTab(b tabBase) Tab {
	t := &ganttTab{tabBase: b, zoom: 1, scrollTo: "sel"}
	t.build = t.mount
	return t
}

func (t *ganttTab) mount(area js.Value) {
	t.info = E("div", "ginfo")
	Data(t.info, "key", "gantt.bars")
	t.box = E("div", "gantt")
	t.addCtl(t.box, "gantt", "gantt.bars")
	t.labels = E("div", "glabels")
	t.tl = E("div", "gtl")
	Add(t.box, t.labels, t.tl)
	Add(area, t.info, t.box)
	t.scope.On(t.labels, "mousedown", func(ev js.Value) {
		if r := Closest(ev.Get("target"), "[data-b]"); r.Truthy() {
			ev.Call("preventDefault")
			t.sel, _ = strconv.Atoi(DataOf(r, "b"))
			t.Focus(0)
		}
	})
	t.scope.On(t.tl, "pointerdown", func(ev js.Value) {
		if ev.Get("button").Int() != 0 {
			return
		}
		tgt := ev.Get("target")
		b := Closest(tgt, "[data-b]")
		t.Focus(0)
		if !b.Truthy() {
			return
		}
		ev.Call("preventDefault")
		t.sel, _ = strconv.Atoi(DataOf(b, "b"))
		bars := t.a.v.Bars()
		if t.sel < len(bars) {
			t.dragID = bars[t.sel].ID
			t.dragResize = tgt.Call("getAttribute", "class").String() == "grip"
			t.dragX = ev.Get("clientX").Float()
			t.dragDelta = 0
			t.tl.Call("setPointerCapture", ev.Get("pointerId"))
		}
		t.a.Invalidate()
	})
	t.scope.On(t.tl, "pointermove", func(ev js.Value) {
		if t.dragID == 0 {
			return
		}
		d := int(math.Round((ev.Get("clientX").Float() - t.dragX) / zoomPPD[t.zoom]))
		if d != t.dragDelta {
			t.dragDelta = d
			t.a.Invalidate()
		}
	})
	end := func(js.Value) {
		if t.dragID == 0 {
			return
		}
		id, d, resize := t.dragID, t.dragDelta, t.dragResize
		t.dragID, t.dragDelta = 0, 0
		if d != 0 {
			if resize {
				t.change(id, 0, d)
			} else {
				t.change(id, d, 0)
			}
		}
		t.sig = ""
		t.a.Invalidate()
	}
	t.scope.On(t.tl, "pointerup", end)
	t.scope.On(t.tl, "pointercancel", end)
}

// bars returns the bars with any drag in progress applied.
func (t *ganttTab) bars() []core.Bar {
	bars := append([]core.Bar(nil), t.a.v.Bars()...)
	if t.dragID != 0 && t.dragDelta != 0 {
		for i := range bars {
			if bars[i].ID == t.dragID {
				bars[i] = shiftBar(bars[i], t.dragDelta, 0, t.dragResize)
			}
		}
	}
	return bars
}

// shiftBar moves a bar by move days, or (resize) changes its length.
func shiftBar(b core.Bar, d, _ int, resize bool) core.Bar {
	if resize {
		b.Days = max(1, min(365, b.Days+int64(d)))
		return b
	}
	if s, ok := core.ParseISO(b.Start); ok {
		b.Start = s.AddDays(d).ISO()
	}
	return b
}

// change commits a move (move days) or a resize (grow days) of bar id.
func (t *ganttTab) change(id int64, move, grow int) {
	bars := append([]core.Bar(nil), t.a.v.Bars()...)
	for i := range bars {
		if bars[i].ID != id {
			continue
		}
		if move != 0 {
			bars[i] = shiftBar(bars[i], move, 0, false)
		}
		if grow != 0 {
			nb := shiftBar(bars[i], grow, 0, true)
			if nb.Days == bars[i].Days {
				t.a.info("a bar is 1–365 days long")
				return
			}
			bars[i] = nb
		}
	}
	_ = t.a.Commit("gantt.bars", bars)
}

func (t *ganttTab) Key(k Key) bool {
	bars := t.a.v.Bars()
	n := len(bars)
	var id int64
	if t.sel < n {
		id = bars[t.sel].ID
	}
	switch {
	case k.Is("ArrowUp"):
		t.sel = max(0, t.sel-1)
	case k.Is("ArrowDown"):
		t.sel = min(n-1, t.sel+1)
	case k.Is("S-ArrowLeft", "<"):
		t.change(id, -1, 0)
	case k.Is("S-ArrowRight", ">"):
		t.change(id, 1, 0)
	case k.Is("["):
		t.change(id, 0, -1)
	case k.Is("]"):
		t.change(id, 0, 1)
	case k.Is("+", "="):
		t.setZoom(t.zoom - 1)
	case k.Is("-"):
		t.setZoom(t.zoom + 1)
	case k.Is("t"):
		t.scrollTo = "today"
		t.sig = ""
	case k.Is("ArrowLeft"):
		t.tl.Set("scrollLeft", t.tl.Get("scrollLeft").Float()-7*zoomPPD[t.zoom])
	case k.Is("ArrowRight"):
		t.tl.Set("scrollLeft", t.tl.Get("scrollLeft").Float()+7*zoomPPD[t.zoom])
	default:
		return false
	}
	t.a.Invalidate()
	return true
}

func (t *ganttTab) setZoom(z int) {
	z = max(0, min(len(zoomPPD)-1, z))
	if z != t.zoom {
		t.zoom, t.scrollTo, t.sig = z, "sel", ""
	}
	t.a.info("Zoom: " + zoomNames[t.zoom] + " scale")
}

func (t *ganttTab) Render() {
	bars := t.bars()
	if t.sel >= len(bars) {
		t.sel = max(0, len(bars)-1)
	}
	td := today()
	// selected bar's dates, shown above the chart
	if t.sel < len(bars) {
		b := bars[t.sel]
		s, _ := core.ParseISO(b.Start)
		e := s.AddDays(int(b.Days) - 1)
		t.info.Set("textContent", fmt.Sprintf("%s · %s → %s · %s · zoom: %s", core.SafeText(b.Label), s.Long(), e.Long(),
			plural(int(b.Days), "day"), zoomNames[t.zoom]))
	} else {
		t.info.Set("textContent", "no bars")
	}
	sig := fmt.Sprint(bars, t.sel, t.zoom, td, t.isCur(0), t.dragID != 0)
	if sig == t.sig {
		return
	}
	t.sig = sig

	// time range: a week before the first start (or today) to 3 weeks after the last end
	lo, hi := td, td
	for _, b := range bars {
		if s, ok := core.ParseISO(b.Start); ok {
			lo = min(lo, s)
			hi = max(hi, s.AddDays(int(b.Days)))
		}
	}
	t.origin = lo.AddDays(-7).WeekStart()
	end := hi.AddDays(21)
	ppd := zoomPPD[t.zoom]
	days := int(end - t.origin)
	w := float64(days) * ppd
	h := gHead + float64(len(bars))*gRow + 8
	x := func(d core.Date) float64 { return float64(d-t.origin) * ppd }

	svg := S("svg", "width", f1(w), "height", f1(h), "class", "gsvg")
	// weekends, month labels, week ticks
	for i := 0; i < days; i++ {
		d := t.origin.AddDays(i)
		if d.Weekday() == 5 && t.zoom < 2 {
			Add(svg, S("rect", "x", f1(x(d)), "y", f1(gHead), "width", f1(2*ppd), "height", f1(h-gHead), "class", "gwe"))
		}
		if d.Day() == 1 || i == 0 {
			lbl := S("text", "x", f1(x(d)+3), "y", "14", "class", "gmonth")
			lbl.Set("textContent", core.MonthNames[d.Month()-1]+" "+strconv.Itoa(d.Year()))
			Add(svg, lbl, S("line", "x1", f1(x(d)), "x2", f1(x(d)), "y1", "0", "y2", f1(h), "class", "gmline"))
		}
		if d.Weekday() == 0 {
			Add(svg, S("line", "x1", f1(x(d)), "x2", f1(x(d)), "y1", "20", "y2", f1(h), "class", "gwline"))
			if t.zoom < 2 || d.Day() <= 7 {
				tk := S("text", "x", f1(x(d)+3), "y", "34", "class", "gtick")
				tk.Set("textContent", d.Short())
				Add(svg, tk)
			}
		}
	}
	Add(svg, S("line", "x1", "0", "x2", f1(w), "y1", f1(gHead), "y2", f1(gHead), "class", "gaxis"))
	// bars
	for i, b := range bars {
		s, ok := core.ParseISO(b.Start)
		if !ok {
			continue
		}
		y := gHead + float64(i)*gRow
		if i == t.sel {
			Add(svg, S("rect", "x", "0", "y", f1(y), "width", f1(w), "height", f1(gRow), "class", "gselrow"))
		}
		g := S("g", "class", "gbar")
		if i == t.sel {
			g.Call("setAttribute", "class", "gbar sel")
		}
		g.Call("setAttribute", "data-b", strconv.Itoa(i))
		bx, bw := x(s), float64(b.Days)*ppd
		Add(g, S("rect", "x", f1(bx), "y", f1(y+6), "width", f1(max(bw, 3)), "height", f1(gRow-12), "rx", "4", "class", "gbr"))
		lbl := S("text", "x", f1(bx+bw+6), "y", f1(y+gRow/2+4), "class", "gbl")
		lbl.Set("textContent", core.SafeText(b.Label))
		Add(g, lbl, S("rect", "x", f1(bx+bw-5), "y", f1(y+6), "width", "8", "height", f1(gRow-12), "class", "grip"))
		Add(svg, g)
	}
	// today line
	tx := x(td) + ppd/2
	Add(svg, S("line", "x1", f1(tx), "x2", f1(tx), "y1", "20", "y2", f1(h), "class", "gtoday"))
	tt := S("text", "x", f1(tx+3), "y", f1(h-4), "class", "gtodayt")
	tt.Set("textContent", "today")
	Add(svg, tt)
	Replace(t.tl, svg)

	// labels column (rows aligned with the SVG's)
	Clear(t.labels)
	Add(t.labels, ET("div", "glhead", "Bar"))
	focused := t.isCur(0)
	for i, b := range bars {
		cls := "glrow"
		if i == t.sel {
			cls += " sel"
			if !focused {
				cls += " dim"
			}
		}
		r := E("div", cls)
		Data(r, "b", strconv.Itoa(i))
		Add(r, ET("span", "", core.SafeText(b.Label)), ET("span", "muted gdays", strconv.FormatInt(b.Days, 10)+"d"))
		Add(t.labels, r)
	}

	switch t.scrollTo {
	case "today":
		t.tl.Set("scrollLeft", tx-t.tl.Get("clientWidth").Float()/2)
	case "sel":
		if t.sel < len(bars) {
			if s, ok := core.ParseISO(bars[t.sel].Start); ok {
				t.tl.Set("scrollLeft", x(s)-60)
			}
		}
	}
	t.scrollTo = ""
}

func (t *ganttTab) Hints() []hint {
	return []hint{{"↑↓", "Select", ""}, {"⇧←→", "Move", ""}, {"[", "Shrink", "["}, {"]", "Extend", "]"},
		{"+", "Zoom in", "+"}, {"-", "Zoom out", "-"}, {"t", "Today", "t"}, {"←→", "Scroll", ""}}
}
