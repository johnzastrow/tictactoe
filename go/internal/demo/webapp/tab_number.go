//go:build js && wasm

package webapp

// Tab 2 — Number: a decimal spinner (Hours), an integer spinner (Quantity)
// and a slider (Percent) that commits as it moves (debounced 150 ms).

import (
	"math"
	"strconv"
	"strings"
	"syscall/js"

	"git.wharf.fluidgrid.org/jcz/tictactoe/go/internal/demo/core"
)

type spinner struct {
	key     string
	integer bool
	in, msg js.Value
	ci      int
	dirty   bool
	base    int64
	theirs  string
	err     string
}

type numberTab struct {
	tabBase
	spins  []*spinner
	slider js.Value
	track  js.Value
	fill   js.Value
	thumb  js.Value
	sval   js.Value
	sci    int
	local  int64 // slider value being dragged / debounced (-1 = none)
	seq    int
	drag   bool
}

func newNumberTab(b tabBase) Tab {
	t := &numberTab{tabBase: b, local: -1}
	t.build = t.mount
	return t
}

func fmtNum(f float64) string { return strconv.FormatFloat(f, 'f', -1, 64) }

func (t *numberTab) mount(area js.Value) {
	form := E("div", "form2")
	for _, s := range []struct {
		key, label, name string
		integer          bool
	}{{"number.hours", "Hours", "hours", false}, {"number.qty", "Quantity", "qty", true}} {
		row, c, msg := field(s.label)
		sp := &spinner{key: s.key, integer: s.integer, msg: msg}
		wrap := E("div", "spin")
		sp.in = E("input", "ed2 num")
		Attr(sp.in, "inputmode", "decimal")
		Attr(sp.in, "spellcheck", "false")
		Attr(sp.in, "autocomplete", "off")
		sp.ci = t.addCtl(sp.in, s.name, s.key)
		btns := E("div", "spinbtns")
		up := ET("button", "sbtn", "▲")
		dn := ET("button", "sbtn", "▼")
		for _, b := range []js.Value{up, dn} {
			Attr(b, "type", "button")
			Attr(b, "tabindex", "-1")
		}
		Data(up, "control", s.name+"-up")
		Data(dn, "control", s.name+"-down")
		Add(btns, up, dn)
		Add(wrap, sp.in, btns)
		Add(c, wrap)
		if s.integer {
			Add(c, ET("span", "muted unit", "0–99, whole numbers"))
		} else {
			Add(c, ET("span", "muted unit", "0–1000, 2 decimals"))
		}
		Add(form, row)
		t.spins = append(t.spins, sp)
		t.scope.On(sp.in, "input", func(js.Value) { t.edited(sp) })
		// mouse path: a click steps and saves
		t.scope.On(up, "mousedown", func(ev js.Value) { ev.Call("preventDefault"); t.Focus(sp.ci); t.step(sp, 1); t.commit(sp) })
		t.scope.On(dn, "mousedown", func(ev js.Value) { ev.Call("preventDefault"); t.Focus(sp.ci); t.step(sp, -1); t.commit(sp) })
	}

	// slider
	row, c, _ := field("Percent")
	t.slider = E("div", "slider")
	Attr(t.slider, "role", "slider")
	Attr(t.slider, "aria-valuemin", "0")
	Attr(t.slider, "aria-valuemax", "100")
	t.sci = t.addCtl(t.slider, "percent", "number.percent")
	t.track = E("div", "strack")
	t.fill = E("div", "sfill")
	t.thumb = E("div", "sthumb")
	Add(t.track, t.fill, t.thumb)
	t.sval = ET("span", "sval", "")
	Add(t.slider, t.track, t.sval)
	Add(c, t.slider)
	Add(form, row)
	Add(area, form)

	t.scope.On(t.track, "pointerdown", func(ev js.Value) {
		ev.Call("preventDefault")
		t.Focus(t.sci)
		t.drag = true
		t.track.Call("setPointerCapture", ev.Get("pointerId"))
		t.dragTo(ev)
	})
	t.scope.On(t.track, "pointermove", func(ev js.Value) {
		if t.drag {
			t.dragTo(ev)
		}
	})
	end := func(ev js.Value) {
		if t.drag {
			t.drag = false
			t.commitSlider()
		}
	}
	t.scope.On(t.track, "pointerup", end)
	t.scope.On(t.track, "pointercancel", end)
}

func (t *numberTab) dragTo(ev js.Value) {
	r := t.track.Call("getBoundingClientRect")
	x := ev.Get("clientX").Float() - r.Get("left").Float()
	w := r.Get("width").Float()
	if w <= 0 {
		return
	}
	t.setSlider(int64(math.Round(max(0, min(1, x/w)) * 100)))
}

// setSlider shows v at once and commits it 150 ms after the last change.
func (t *numberTab) setSlider(v int64) {
	v = max(0, min(100, v))
	t.local = v
	t.seq++
	seq := t.seq
	After(150, func() {
		if seq == t.seq && !t.drag {
			t.commitSlider()
		}
	})
	t.a.Invalidate()
}

func (t *numberTab) commitSlider() {
	if t.local < 0 {
		return
	}
	v := t.local
	t.local = -1
	t.seq++
	if v != t.a.v.Int("number.percent") {
		_ = t.a.Commit("number.percent", v)
	}
	t.a.Invalidate()
}

func (t *numberTab) sliderValue() int64 {
	if t.local >= 0 {
		return t.local
	}
	return t.a.v.Int("number.percent")
}

func (t *numberTab) synced(sp *spinner) string {
	if sp.integer {
		return strconv.FormatInt(t.a.v.Int(sp.key), 10)
	}
	return fmtNum(t.a.v.Float(sp.key))
}

// parse reads the draft; ok false when it isn't a number.
func (sp *spinner) parse() (float64, bool) {
	f, err := strconv.ParseFloat(strings.TrimSpace(sp.in.Get("value").String()), 64)
	return f, err == nil && !math.IsInf(f, 0) && !math.IsNaN(f)
}

func (t *numberTab) edited(sp *spinner) {
	if !sp.dirty {
		sp.base = t.a.v.Version(sp.key)
	}
	sp.dirty = sp.in.Get("value").String() != t.synced(sp)
	sp.err = ""
	f, ok := sp.parse()
	switch {
	case !ok:
		sp.err = "must be a number"
	default:
		if _, err := core.NormalizeValue(sp.key, f); err != nil {
			sp.err = err.Error()
		}
	}
	t.a.Invalidate()
}

func (t *numberTab) step(sp *spinner, d float64) {
	f, ok := sp.parse()
	if !ok {
		f = 0
	}
	f = math.Round((f+d)*100) / 100
	if sp.integer {
		f = math.Round(f)
		f = max(0, min(99, f))
	} else {
		f = max(0, min(1000, f))
	}
	sp.in.Set("value", fmtNum(f))
	t.edited(sp)
}

func (t *numberTab) commit(sp *spinner) {
	if !sp.dirty {
		return
	}
	if sp.err != "" {
		t.a.setStatus("err", "✗ "+sp.key+": "+sp.err)
		return
	}
	f, _ := sp.parse()
	var v any = f
	if sp.integer {
		v = int64(f)
	}
	if err := t.a.CommitFrom(sp.key, v, sp.base, func(m core.AckMsg) {
		if m.OK {
			sp.dirty, sp.theirs = false, ""
			sp.in.Set("value", t.synced(sp))
		} else if !m.Conflict {
			sp.err = m.Error
		}
	}); err != nil {
		sp.err = err.Error()
	}
}

func (t *numberTab) curSpin() *spinner {
	for _, sp := range t.spins {
		if sp.ci == t.cur {
			return sp
		}
	}
	return nil
}

func (t *numberTab) Key(k Key) bool {
	if sp := t.curSpin(); sp != nil {
		switch {
		case k.Is("ArrowUp"):
			t.step(sp, 1)
		case k.Is("ArrowDown"):
			t.step(sp, -1)
		case k.Is("S-ArrowUp") && !sp.integer:
			t.step(sp, 0.25)
		case k.Is("S-ArrowDown") && !sp.integer:
			t.step(sp, -0.25)
		case k.Is("Enter"):
			t.commit(sp)
		case k.Is("Escape") && (sp.dirty || sp.err != "" || sp.theirs != ""):
			sp.dirty, sp.err, sp.theirs = false, "", ""
			sp.in.Set("value", t.synced(sp))
		default:
			return false
		}
		t.a.Invalidate()
		return true
	}
	if t.cur == t.sci {
		v := t.sliderValue()
		switch {
		case k.Is("ArrowLeft", "ArrowDown"):
			t.setSlider(v - 1)
		case k.Is("ArrowRight", "ArrowUp"):
			t.setSlider(v + 1)
		case k.Is("PageDown"):
			t.setSlider(v - 10)
		case k.Is("PageUp"):
			t.setSlider(v + 10)
		case k.Is("Home"):
			t.setSlider(0)
		case k.Is("End"):
			t.setSlider(100)
		default:
			return false
		}
		return true
	}
	return false
}

func (t *numberTab) Synced(key string, ev syncEv) {
	for _, sp := range t.spins {
		if sp.key != key {
			continue
		}
		switch {
		case ev.conflict:
			sp.dirty, sp.theirs, sp.err = false, "", ""
			sp.in.Set("value", t.synced(sp))
		case ev.welcome || ev.mine || ev.rejected:
		case sp.dirty:
			sp.theirs = ev.by
		}
	}
}

func (t *numberTab) Render() {
	for _, sp := range t.spins {
		if !sp.dirty && sp.in.Get("value").String() != t.synced(sp) {
			sp.in.Set("value", t.synced(sp))
		}
		cls := "ed2 num"
		switch {
		case sp.err != "":
			cls += " bad"
			setMsg(sp.msg, "err", "✗ "+sp.err)
		case sp.theirs != "":
			setMsg(sp.msg, "warn", "⚠ changed by "+sp.theirs+" — Esc takes theirs")
		case sp.dirty:
			cls += " dirty"
			setMsg(sp.msg, "info", "● unsaved — Enter saves, Esc reverts")
		default:
			setMsg(sp.msg, "", "")
		}
		sp.in.Set("className", cls)
	}
	v := t.sliderValue()
	pct := strconv.FormatInt(v, 10) + "%"
	Style(t.fill, "width", pct)
	Style(t.thumb, "left", pct)
	t.sval.Set("textContent", pct)
	Attr(t.slider, "aria-valuenow", strconv.FormatInt(v, 10))
}

func (t *numberTab) Hints() []hint {
	if sp := t.curSpin(); sp != nil {
		hs := []hint{{"↑↓", "±1", ""}}
		if !sp.integer {
			hs = append(hs, hint{"⇧↑↓", "±0.25", ""})
		}
		return append(hs, hint{"Enter", "Save", "Enter"}, hint{"Esc", "Revert", "Escape"}, hint{"Tab", "Next", "Tab"})
	}
	return []hint{{"←→", "±1", ""}, {"PgUp", "+10", "PageUp"}, {"PgDn", "−10", "PageDown"}, {"Home", "0", "Home"}, {"End", "100", "End"}}
}
