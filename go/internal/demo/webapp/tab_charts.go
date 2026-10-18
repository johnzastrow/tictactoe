//go:build js && wasm

package webapp

// Tab 14 — Charts: charts.series drawn four ways (SVG bars, a sparkline, a
// gauge of the average and a heat strip). The selected month is
// highlighted in all four; keys or dragging a bar's top change it.

import (
	"fmt"
	"math"
	"strconv"
	"strings"
	"syscall/js"

	"git.wharf.fluidgrid.org/jcz/tictactoe/go/internal/demo/core"
)

const (
	chW, chH        = 640.0, 240.0
	chL, chR        = 34.0, 10.0
	chT, chB        = 20.0, 26.0
	chPlotH         = chH - chT - chB
	chSlot          = (chW - chL - chR) / 12
	chBarW          = chSlot * 0.62
	sparkW, sparkH  = 260.0, 60.0
	gaugeW, gaugeH  = 200.0, 120.0
	gaugeR, gaugeCX = 80.0, 100.0
	gaugeCY         = 100.0
)

type chartsTab struct {
	tabBase
	box   js.Value
	bars  js.Value // the bar chart's wrapper (pointer capture lives here)
	spark js.Value
	gauge js.Value
	heat  js.Value
	sel   int
	drag  bool
	dragV int64 // value shown for the selected month while dragging (-1 none)
	sig   string
}

func newChartsTab(b tabBase) Tab {
	t := &chartsTab{tabBase: b, dragV: -1}
	t.build = t.mount
	return t
}

func (t *chartsTab) mount(area js.Value) {
	t.box = E("div", "charts")
	t.addCtl(t.box, "charts", "charts.series")
	t.bars = E("div", "chart bars")
	side := E("div", "chartside")
	t.spark = E("div", "chart")
	t.gauge = E("div", "chart")
	Add(side, t.spark, t.gauge)
	t.heat = E("div", "heat")
	top := E("div", "chartrow")
	Add(top, t.bars, side)
	Add(t.box, top, t.heat)
	Add(area, t.box)

	t.scope.On(t.bars, "pointerdown", func(ev js.Value) {
		ev.Call("preventDefault")
		t.Focus(0)
		m, v, ok := t.hit(ev)
		if !ok {
			return
		}
		t.sel = m
		// a press near the bar's top (or above it) starts a drag
		cur := t.series()[m]
		if math.Abs(float64(v-cur)) <= 12 || v > cur {
			t.drag = true
			t.dragV = v
			t.bars.Call("setPointerCapture", ev.Get("pointerId"))
		}
		t.a.Invalidate()
	})
	t.scope.On(t.bars, "pointermove", func(ev js.Value) {
		if !t.drag {
			return
		}
		if _, v, ok := t.hit(ev); ok {
			t.dragV = v
			t.a.Invalidate()
		}
	})
	end := func(js.Value) {
		if !t.drag {
			return
		}
		t.drag = false
		v := t.dragV
		t.dragV = -1
		if v >= 0 && v != t.series()[t.sel] {
			t.set(t.sel, v)
		}
		t.a.Invalidate()
	}
	t.scope.On(t.bars, "pointerup", end)
	t.scope.On(t.bars, "pointercancel", end)
	t.scope.On(t.heat, "mousedown", func(ev js.Value) {
		if c := Closest(ev.Get("target"), "[data-m]"); c.Truthy() {
			ev.Call("preventDefault")
			t.sel, _ = strconv.Atoi(DataOf(c, "m"))
			t.Focus(0)
		}
	})
}

// hit maps a pointer to (month, value) in the bar chart.
func (t *chartsTab) hit(ev js.Value) (int, int64, bool) {
	svg := t.bars.Call("querySelector", "svg")
	if !svg.Truthy() {
		return 0, 0, false
	}
	r := svg.Call("getBoundingClientRect")
	x := ev.Get("clientX").Float() - r.Get("left").Float()
	y := ev.Get("clientY").Float() - r.Get("top").Float()
	m := int(math.Floor((x - chL) / chSlot))
	m = max(0, min(11, m))
	v := int64(math.Round((chT + chPlotH - y) / chPlotH * 100))
	return m, max(0, min(100, v)), true
}

func (t *chartsTab) series() []int64 {
	s := t.a.v.Series()
	if len(s) != 12 {
		s = make([]int64, 12)
	}
	return s
}

func (t *chartsTab) set(m int, v int64) {
	s := append([]int64(nil), t.series()...)
	s[m] = max(0, min(100, v))
	_ = t.a.Commit("charts.series", s)
}

func (t *chartsTab) Key(k Key) bool {
	s := t.series()
	switch {
	case k.Is("ArrowLeft"):
		t.sel = (t.sel + 11) % 12
	case k.Is("ArrowRight"):
		t.sel = (t.sel + 1) % 12
	case k.Is("Home"):
		t.sel = 0
	case k.Is("End"):
		t.sel = 11
	case k.Is("ArrowUp"):
		t.set(t.sel, s[t.sel]+5)
	case k.Is("ArrowDown"):
		t.set(t.sel, s[t.sel]-5)
	case k.Is("PageUp"):
		t.set(t.sel, s[t.sel]+20)
	case k.Is("PageDown"):
		t.set(t.sel, s[t.sel]-20)
	default:
		return false
	}
	t.a.Invalidate()
	return true
}

func gaugeColor(avg float64) string {
	switch {
	case avg < 60:
		return "var(--ok)"
	case avg < 80:
		return "var(--warn)"
	}
	return "var(--err)"
}

func f1(f float64) string { return strconv.FormatFloat(f, 'f', 1, 64) }

func (t *chartsTab) Render() {
	s := append([]int64(nil), t.series()...)
	if t.dragV >= 0 {
		s[t.sel] = t.dragV
	}
	sig := fmt.Sprint(s, t.sel, t.drag, t.isCur(0))
	if sig == t.sig {
		return
	}
	t.sig = sig
	var sum int64
	for _, v := range s {
		sum += v
	}
	avg := float64(sum) / 12

	// bars
	svg := S("svg", "width", f1(chW), "height", f1(chH), "viewBox", "0 0 "+f1(chW)+" "+f1(chH), "class", "barsvg")
	for _, g := range []int64{0, 25, 50, 75, 100} {
		y := chT + chPlotH - float64(g)/100*chPlotH
		Add(svg, S("line", "x1", f1(chL), "x2", f1(chW-chR), "y1", f1(y), "y2", f1(y), "class", "gridline"))
		tx := S("text", "x", f1(chL-6), "y", f1(y+3), "class", "axis", "text-anchor", "end")
		tx.Set("textContent", strconv.FormatInt(g, 10))
		Add(svg, tx)
	}
	for m, v := range s {
		x := chL + float64(m)*chSlot + (chSlot-chBarW)/2
		h := float64(v) / 100 * chPlotH
		y := chT + chPlotH - h
		cls := "bar"
		if m == t.sel {
			cls += " sel"
		}
		r := S("rect", "x", f1(x), "y", f1(y), "width", f1(chBarW), "height", f1(max(h, 1)), "rx", "3", "class", cls)
		Add(svg, r)
		if m == t.sel {
			// the drag handle on the selected bar's top
			Add(svg, S("rect", "x", f1(x-2), "y", f1(y-3), "width", f1(chBarW+4), "height", "6", "rx", "3", "class", "handle"))
		}
		vl := S("text", "x", f1(x+chBarW/2), "y", f1(y-6), "class", "vlabel", "text-anchor", "middle")
		vl.Set("textContent", strconv.FormatInt(v, 10))
		ml := S("text", "x", f1(x+chBarW/2), "y", f1(chH-8), "class", "axis", "text-anchor", "middle")
		ml.Set("textContent", core.MonthNames[m])
		Add(svg, vl, ml)
	}
	Replace(t.bars, ET("div", "ctitle2", "Monthly values"), svg)

	// sparkline
	sp := S("svg", "width", f1(sparkW), "height", f1(sparkH), "class", "sparksvg")
	var pts []string
	px := func(m int) float64 { return 6 + float64(m)*(sparkW-12)/11 }
	py := func(v int64) float64 { return sparkH - 6 - float64(v)/100*(sparkH-12) }
	for m, v := range s {
		pts = append(pts, f1(px(m))+","+f1(py(v)))
	}
	Add(sp, S("polyline", "points", strings.Join(pts, " "), "class", "sparkline"))
	Add(sp, S("circle", "cx", f1(px(t.sel)), "cy", f1(py(s[t.sel])), "r", "4", "class", "sparkdot"))
	Replace(t.spark, ET("div", "ctitle2", "Sparkline"), sp)

	// gauge of the average
	g := S("svg", "width", f1(gaugeW), "height", f1(gaugeH), "class", "gaugesvg")
	arc := func(frac float64) string {
		th := math.Pi * (1 - frac)
		x := gaugeCX + gaugeR*math.Cos(th)
		y := gaugeCY - gaugeR*math.Sin(th)
		return fmt.Sprintf("M %s %s A %s %s 0 0 1 %s %s", f1(gaugeCX-gaugeR), f1(gaugeCY), f1(gaugeR), f1(gaugeR), f1(x), f1(y))
	}
	Add(g, S("path", "d", arc(1), "class", "gaugebg"))
	fg := S("path", "d", arc(max(0.001, avg/100)), "class", "gaugefg")
	Style(fg, "stroke", gaugeColor(avg))
	gt := S("text", "x", f1(gaugeCX), "y", f1(gaugeCY-8), "class", "gaugeval", "text-anchor", "middle")
	gt.Set("textContent", f1(avg))
	gl := S("text", "x", f1(gaugeCX), "y", f1(gaugeCY+14), "class", "axis", "text-anchor", "middle")
	gl.Set("textContent", "average (<60 ok · <80 warn)")
	Add(g, fg, gt, gl)
	Replace(t.gauge, ET("div", "ctitle2", "Gauge"), g)

	// heat strip
	Clear(t.heat)
	Add(t.heat, ET("div", "ctitle2", "Heat strip"))
	strip := E("div", "strip")
	for m, v := range s {
		c := E("div", "hcell")
		if m == t.sel {
			c.Set("className", "hcell sel")
		}
		Data(c, "m", strconv.Itoa(m))
		Style(c, "--i", strconv.FormatInt(v, 10)+"%")
		Attr(c, "title", fmt.Sprintf("%s: %d", core.MonthNames[m], v))
		Add(c, ET("span", "hm", core.MonthNames[m]), ET("span", "hv", strconv.FormatInt(v, 10)))
		Add(strip, c)
	}
	Add(t.heat, strip, ET("div", "muted csel", fmt.Sprintf("Selected: %s = %d", core.MonthNames[t.sel], s[t.sel])))
}

func (t *chartsTab) Hints() []hint {
	return []hint{{"←→", "Month", ""}, {"↑", "+5", "ArrowUp"}, {"↓", "−5", "ArrowDown"}, {"PgUp", "+20", "PageUp"}, {"PgDn", "−20", "PageDown"}}
}
