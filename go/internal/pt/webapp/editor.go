//go:build js && wasm

package webapp

// Cell editors, shared by the grid (floating over a cell) and dialog forms
// (embedded inline): text, number, date with calendar, select, fuzzy
// combobox, suggest, chips (tags/deps/options), notes, bool.

import (
	"fmt"
	"strconv"
	"strings"
	"syscall/js"

	"git.wharf.fluidgrid.org/jcz/tictactoe/go/internal/pt/core"
)

type EditorOpts struct {
	Kind        core.Editor
	Entity      core.Entity
	Field       string
	ID          int64
	ProjectHint *int64
	Initial     any
	InitialText string
	Seed        string
	Replace     bool
	Title       string
	Embedded    bool
	Choices     func() []core.Choice // override (dialogs)
	Placeholder string
	OnCommit    func(v any, move string)
	OnCancel    func()
	OnChange    func(v any)
}

type Editor struct {
	app  *App
	o    EditorOpts
	root js.Value
	in   js.Value // input or textarea
	box  js.Value // chips container
	pop  js.Value // popup (in the pop layer)

	choices  []core.Choice
	filtered []core.FilteredChoice
	sel      int
	navved   bool
	value    any   // select/combo/bool
	chips    []any // chips values
	cal      core.Date
	err      string
	focused     bool
	justFocused bool
	scope       Scope
	popScope Scope
	dead     bool
}

func listKind(k core.Editor) bool {
	return k == core.EdSelect || k == core.EdCombo || k == core.EdSuggest || k == core.EdChips
}

func NewEditor(a *App, o EditorOpts) *Editor {
	e := &Editor{app: a, o: o, value: o.Initial, sel: -1}
	cls := "edroot"
	if o.Embedded {
		cls += " embedded"
	}
	e.root = E("div", cls)
	switch o.Kind {
	case core.EdMultiline:
		e.in = E("textarea", "")
		e.in.Set("value", o.InitialText)
		if o.Replace {
			e.in.Set("value", o.Seed)
		}
		if o.Embedded {
			Add(e.root, e.in)
		} else {
			panel := E("div", "notesed")
			Add(panel, ET("div", "edtitle", o.Title+" — ^Enter or ^S saves, Esc cancels"), e.in)
			Add(e.root, panel)
		}
	case core.EdChips:
		e.box = E("div", "chips")
		e.in = E("input", "chipin")
		if l, ok := core.AsList(o.Initial); ok {
			e.chips = append(e.chips, l...)
		}
		if o.Replace {
			e.in.Set("value", o.Seed)
		}
		Add(e.root, e.box)
	default:
		e.in = E("input", "ed")
		text := o.InitialText
		if o.Replace {
			text = o.Seed
		}
		e.in.Set("value", text)
	}
	if !e.in.IsUndefined() && o.Kind != core.EdMultiline && o.Kind != core.EdChips {
		Add(e.root, e.in)
	}
	Attr(e.in, "spellcheck", "false")
	Attr(e.in, "autocomplete", "off")
	if o.Placeholder != "" {
		Attr(e.in, "placeholder", o.Placeholder)
	}
	if o.Kind == core.EdBool {
		e.in.Set("readOnly", true)
	}
	if listKind(o.Kind) || o.Kind == core.EdDate || o.Kind == core.EdBool {
		if o.Choices != nil {
			e.choices = o.Choices()
		} else if o.Kind == core.EdBool {
			e.choices = core.Choices(a.r, o.Entity, o.Field, o.ID, o.ProjectHint)
		} else if listKind(o.Kind) {
			e.choices = core.Choices(a.r, o.Entity, o.Field, o.ID, o.ProjectHint)
		}
	}
	e.pop = E("div", "popup hidden")
	Add(a.popLayer, e.pop)
	e.initCal()
	if o.Kind == core.EdSelect || o.Kind == core.EdCombo || o.Kind == core.EdBool {
		// start on the current value; typing filters from scratch
		for i, c := range e.choices {
			if core.ValuesEqual(c.Value, o.Initial) {
				e.sel = i
			}
		}
		if !o.Replace && !o.Embedded {
			e.in.Set("value", "")
			Attr(e.in, "placeholder", "type to filter… (current: "+o.InitialText+")")
		}
		if o.Embedded {
			e.in.Set("value", e.labelOf(o.Initial))
		}
	}
	e.refilter()
	if o.Kind == core.EdChips {
		Add(e.box, e.in)
		e.renderChips()
	}
	e.scope.On(e.in, "input", func(js.Value) {
		e.navved = false
		e.justFocused = false
		e.refilter()
		e.validate()
		e.renderPopup()
		if o.Embedded && o.OnChange != nil && !listKind(o.Kind) {
			if v, err := e.Value(); err == nil {
				o.OnChange(v)
			}
		}
	})
	e.scope.On(e.in, "focus", func(js.Value) {
		e.focused = true
		if o.Embedded && (o.Kind == core.EdSelect || o.Kind == core.EdCombo) {
			e.in.Call("select")
			e.justFocused = true
			e.filtered = core.FilterChoices(e.choices, "")
		}
		e.renderPopup()
	})
	// a click that focused the field would otherwise drop the selection on mouseup
	e.scope.On(e.in, "mouseup", func(ev js.Value) {
		if e.justFocused {
			e.justFocused = false
			ev.Call("preventDefault")
		}
	})
	e.scope.On(e.in, "blur", func(js.Value) {
		e.focused = false
		if o.Embedded {
			After(120, func() {
				if e.dead || e.focused {
					return
				}
				if o.Kind == core.EdSelect || o.Kind == core.EdCombo {
					e.in.Set("value", e.labelOf(e.value))
				}
				if o.Kind == core.EdChips && strings.TrimSpace(e.in.Get("value").String()) != "" {
					e.addTyped()
				}
				e.hidePopup()
			})
		}
	})
	if o.Kind == core.EdChips {
		e.scope.On(e.box, "mousedown", func(ev js.Value) {
			if x := Closest(ev.Get("target"), "[data-x]"); x.Truthy() {
				ev.Call("preventDefault")
				i, _ := strconv.Atoi(DataOf(x, "x"))
				if i < len(e.chips) {
					e.chips = append(e.chips[:i], e.chips[i+1:]...)
					e.renderChips()
					e.changed()
				}
			}
			After(0, func() { e.in.Call("focus") })
		})
	}
	e.scope.On(e.pop, "mousedown", func(ev js.Value) {
		ev.Call("preventDefault")
		t := ev.Get("target")
		if c := Closest(t, "[data-i]"); c.Truthy() {
			i, _ := strconv.Atoi(DataOf(c, "i"))
			e.choose(i)
		} else if d := Closest(t, "[data-day]"); d.Truthy() {
			n, _ := strconv.Atoi(DataOf(d, "day"))
			e.setDate(core.Date(n), true)
		} else if b := Closest(t, "[data-cal]"); b.Truthy() {
			switch DataOf(b, "cal") {
			case "today":
				e.setDate(today(), true)
			case "clear":
				e.in.Set("value", "")
				e.commitText("none")
			case "prev":
				e.cal = e.cal.AddMonths(-1)
				e.renderPopup()
			case "next":
				e.cal = e.cal.AddMonths(1)
				e.renderPopup()
			}
		}
	})
	e.validate()
	return e
}

func (e *Editor) initCal() {
	e.cal = today()
	if s, ok := e.o.Initial.(string); ok {
		if d, ok := core.ParseISO(s); ok {
			e.cal = d
		}
	}
}

func (e *Editor) labelOf(v any) string {
	if v == nil {
		if e.o.Kind == core.EdSelect || e.o.Kind == core.EdCombo {
			for _, c := range e.choices {
				if c.Value == nil {
					return c.Label
				}
			}
		}
		return ""
	}
	for _, c := range e.choices {
		if core.ValuesEqual(c.Value, v) {
			return c.Label
		}
	}
	return fmt.Sprint(v)
}

func (e *Editor) Focus() {
	e.in.Call("focus")
	if e.o.Kind == core.EdText || e.o.Kind == core.EdNumber || e.o.Kind == core.EdInt || e.o.Kind == core.EdDate || e.o.Kind == core.EdSuggest {
		if !e.o.Replace {
			e.in.Call("select")
		}
	}
	e.focused = true
	e.renderPopup()
}

// Place positions a floating (grid) editor over a cell.
func (e *Editor) Place(x, y, w, h float64) {
	Style(e.root, "left", Px(x))
	Style(e.root, "top", Px(y))
	minW := w
	if e.o.Kind == core.EdChips {
		minW = max(w, 300)
	}
	if e.o.Kind == core.EdCombo || e.o.Kind == core.EdSelect || e.o.Kind == core.EdDate {
		minW = max(w, 180)
	}
	Style(e.root, "width", Px(minW))
	Style(e.root, "min-height", Px(h))
	e.renderPopup()
}

func (e *Editor) Destroy() {
	e.dead = true
	e.scope.Release()
	e.popScope.Release()
	if e.pop.Truthy() {
		e.pop.Call("remove")
	}
	if e.root.Truthy() {
		e.root.Call("remove")
	}
}

func (e *Editor) text() string { return e.in.Get("value").String() }

func (e *Editor) refilter() {
	if !listKind(e.o.Kind) && e.o.Kind != core.EdBool {
		return
	}
	q := e.text()
	if e.o.Kind == core.EdBool {
		q = ""
	}
	e.filtered = core.FilterChoices(e.choices, q)
	if e.o.Kind == core.EdChips {
		// hide what's already chosen
		var keep []core.FilteredChoice
		for _, f := range e.filtered {
			taken := false
			for _, c := range e.chips {
				if core.ValuesEqual(c, e.choices[f.Index].Value) {
					taken = true
				}
			}
			if !taken {
				keep = append(keep, f)
			}
		}
		e.filtered = keep
	}
	if strings.TrimSpace(q) != "" {
		e.sel = 0
		if len(e.filtered) == 0 {
			e.sel = -1
		}
	} else if e.o.Kind == core.EdSelect || e.o.Kind == core.EdCombo || e.o.Kind == core.EdBool {
		// keep the current value selected
		e.sel = -1
		for i, f := range e.filtered {
			if core.ValuesEqual(e.choices[f.Index].Value, e.value) {
				e.sel = i
			}
		}
		if e.sel < 0 && len(e.filtered) > 0 {
			e.sel = 0
		}
	} else {
		e.sel = -1
	}
}

func (e *Editor) validate() {
	e.err = ""
	switch e.o.Kind {
	case core.EdText, core.EdNumber, core.EdInt, core.EdDate, core.EdSuggest:
		if _, err := e.parse(e.text()); err != nil {
			e.err = err.Error()
		}
	}
	cls := e.in.Get("className").String()
	cls = strings.ReplaceAll(cls, " bad", "")
	if e.err != "" {
		cls += " bad"
		Attr(e.in, "title", e.err)
	} else {
		Attr(e.in, "title", "")
	}
	e.in.Set("className", cls)
}

func (e *Editor) parse(text string) (any, error) {
	if e.o.Kind == core.EdNumber || e.o.Kind == core.EdInt {
		t := strings.TrimSpace(strings.TrimSuffix(strings.TrimSpace(text), "h"))
		if t != "" {
			if _, err := strconv.ParseFloat(t, 64); err != nil {
				return nil, fmt.Errorf("'%s' is not a number", text)
			}
		}
	}
	return core.ParseInput(e.app.r, e.o.Entity, e.o.Field, e.o.ID, text)
}

// Value: the editor's current value (for dialogs and commit).
func (e *Editor) Value() (any, error) {
	switch e.o.Kind {
	case core.EdSelect, core.EdCombo, core.EdBool:
		return e.value, nil
	case core.EdChips:
		out := make([]any, 0, len(e.chips))
		out = append(out, e.chips...)
		if t := strings.TrimSpace(e.text()); t != "" {
			if v := e.typedChip(t); v != nil {
				out = append(out, v)
			}
		}
		return out, nil
	case core.EdMultiline:
		return e.text(), nil
	}
	return e.parse(e.text())
}

func (e *Editor) changed() {
	if e.o.Embedded && e.o.OnChange != nil {
		if v, err := e.Value(); err == nil {
			e.o.OnChange(v)
		}
	}
}

func (e *Editor) choose(i int) {
	if i < 0 || i >= len(e.filtered) {
		return
	}
	c := e.choices[e.filtered[i].Index]
	switch e.o.Kind {
	case core.EdChips:
		e.chips = append(e.chips, c.Value)
		e.in.Set("value", "")
		e.refilter()
		e.renderChips()
		e.renderPopup()
		e.changed()
	case core.EdSuggest:
		e.in.Set("value", c.Label)
		if e.o.Embedded {
			e.changed()
			e.hidePopup()
			return
		}
		e.commitText(c.Label)
	default:
		e.value = c.Value
		if e.o.Embedded {
			e.in.Set("value", c.Label)
			e.changed()
			e.hidePopup()
			return
		}
		e.o.OnCommit(c.Value, "down")
	}
}

func (e *Editor) typedChip(t string) any {
	// an existing choice by exact label, else (tags/options) a new value
	for _, c := range e.choices {
		if strings.EqualFold(c.Label, t) || strings.EqualFold(fmt.Sprint(c.Value), t) {
			return c.Value
		}
	}
	switch e.o.Field {
	case "tags":
		return strings.ToLower(strings.TrimPrefix(t, "#"))
	case "options":
		return t
	}
	if e.o.Field == "deps" {
		v, err := core.ParseInput(e.app.r, e.o.Entity, "deps", e.o.ID, t)
		if l, ok := core.AsList(v); err == nil && ok && len(l) == 1 {
			return l[0]
		}
	}
	return nil
}

func (e *Editor) addTyped() {
	t := strings.TrimSpace(e.text())
	if t == "" {
		return
	}
	var v any
	if e.sel >= 0 && e.sel < len(e.filtered) {
		v = e.choices[e.filtered[e.sel].Index].Value
	} else {
		v = e.typedChip(t)
	}
	if v == nil {
		e.err = "no match for '" + t + "'"
		e.app.setStatus("warn", e.err)
		return
	}
	e.chips = append(e.chips, v)
	e.in.Set("value", "")
	e.refilter()
	e.renderChips()
	e.renderPopup()
	e.changed()
}

func (e *Editor) commitText(text string) {
	v, err := e.parse(text)
	if err != nil {
		e.err = err.Error()
		e.validate()
		e.app.setStatus("err", e.err)
		return
	}
	if e.o.Embedded {
		e.changed()
		return
	}
	e.o.OnCommit(v, "down")
}

func (e *Editor) setDate(d core.Date, commit bool) {
	e.cal = d
	e.in.Set("value", d.ISO())
	e.validate()
	if commit {
		if e.o.Embedded {
			e.changed()
			e.hidePopup()
			return
		}
		e.o.OnCommit(d.ISO(), "down")
		return
	}
	e.renderPopup()
	e.changed()
}

// CommitIfValid: clicking elsewhere keeps a valid text edit, drops the rest.
func (e *Editor) CommitIfValid() {
	switch e.o.Kind {
	case core.EdText, core.EdNumber, core.EdInt, core.EdDate, core.EdSuggest, core.EdMultiline:
		if v, err := e.Value(); err == nil {
			e.o.OnCommit(v, "none")
			return
		}
	case core.EdChips:
		v, _ := e.Value()
		e.o.OnCommit(v, "none")
		return
	}
	e.o.OnCancel()
}

func (e *Editor) Key(k Key) bool {
	kind := e.o.Kind
	listOpen := listKind(kind) || kind == core.EdBool
	move := func(d int) {
		if len(e.filtered) == 0 {
			return
		}
		e.navved = true
		if e.sel < 0 {
			e.sel = 0
		} else {
			e.sel = (e.sel + d + len(e.filtered)) % len(e.filtered)
		}
		e.renderPopup()
	}
	commit := func(dir string) bool {
		if e.o.Embedded {
			return false
		}
		switch kind {
		case core.EdSelect, core.EdCombo, core.EdBool:
			if e.sel >= 0 && e.sel < len(e.filtered) && (strings.TrimSpace(e.text()) != "" || e.navved) {
				e.o.OnCommit(e.choices[e.filtered[e.sel].Index].Value, dir)
			} else {
				e.o.OnCommit(e.value, dir)
			}
			return true
		case core.EdChips:
			if strings.TrimSpace(e.text()) != "" {
				e.addTyped()
			}
			v, _ := e.Value()
			e.o.OnCommit(v, dir)
			return true
		}
		v, err := e.Value()
		if err != nil {
			e.err = err.Error()
			e.validate()
			e.app.setStatus("err", e.err)
			return true
		}
		e.o.OnCommit(v, dir)
		return true
	}
	switch {
	case k.Is("Escape"):
		if e.o.Embedded {
			if !e.pop.Get("classList").Call("contains", "hidden").Bool() {
				e.hidePopup()
				return true
			}
			return false
		}
		e.o.OnCancel()
		return true
	case kind == core.EdMultiline:
		if k.Is("C-Enter", "C-s") {
			if e.o.Embedded {
				return false
			}
			e.o.OnCommit(e.text(), "none")
			return true
		}
		return false
	case k.Is("Tab"):
		if kind == core.EdChips && strings.TrimSpace(e.text()) != "" && e.o.Embedded {
			e.addTyped()
			return true
		}
		return commit("right")
	case k.Is("S-Tab"):
		return commit("left")
	case k.Is("C-Enter") && kind == core.EdChips:
		return commit("down")
	case k.Is("Enter"):
		switch kind {
		case core.EdChips:
			if strings.TrimSpace(e.text()) != "" {
				e.addTyped()
				return true
			}
			return commit("down")
		case core.EdSuggest:
			if e.navved && e.sel >= 0 {
				e.choose(e.sel)
				return true
			}
			if e.o.Embedded {
				e.changed()
				return false
			}
			return commit("down")
		case core.EdSelect, core.EdCombo:
			if e.o.Embedded {
				if e.sel >= 0 && !e.pop.Get("classList").Call("contains", "hidden").Bool() {
					e.choose(e.sel)
					return true
				}
				return false
			}
		}
		return commit("down")
	case k.Is("ArrowDown") && listOpen:
		move(1)
	case k.Is("ArrowUp") && listOpen:
		move(-1)
	case k.Is("PageDown") && listOpen:
		move(8)
	case k.Is("PageUp") && listOpen:
		move(-8)
	case kind == core.EdBool && k.Is(" "):
		b, _ := e.value.(bool)
		e.value = !b
		if !e.o.Embedded {
			e.o.OnCommit(e.value, "none")
		} else {
			e.in.Set("value", e.labelOf(e.value))
			e.changed()
		}
	case k.Is("Backspace") && kind == core.EdChips && e.text() == "":
		if len(e.chips) > 0 {
			e.chips = e.chips[:len(e.chips)-1]
			e.refilter()
			e.renderChips()
			e.renderPopup()
			e.changed()
		}
	case (kind == core.EdNumber || kind == core.EdInt) && k.Is("ArrowUp", "ArrowDown", "S-ArrowUp", "S-ArrowDown"):
		step := 1.0
		if k.Shift && kind == core.EdNumber {
			step = 0.25
		}
		if k.Is("ArrowDown", "S-ArrowDown") {
			step = -step
		}
		x, _ := strconv.ParseFloat(strings.TrimSpace(strings.TrimSuffix(e.text(), "h")), 64)
		x = max(0, x+step)
		e.in.Set("value", core.FmtHours(x))
		e.validate()
		e.changed()
	case kind == core.EdDate && k.Is("ArrowUp", "ArrowDown", "PageUp", "PageDown", "C-ArrowLeft", "C-ArrowRight"):
		d := e.cal
		if v, err := e.parse(e.text()); err == nil {
			if s, ok := v.(string); ok {
				if p, ok := core.ParseISO(s); ok {
					d = p
				}
			}
		}
		switch {
		case k.Is("ArrowUp"):
			d = d.AddDays(-7)
		case k.Is("ArrowDown"):
			d = d.AddDays(7)
		case k.Is("PageUp"):
			d = d.AddMonths(-1)
		case k.Is("PageDown"):
			d = d.AddMonths(1)
		case k.Is("C-ArrowLeft"):
			d = d.AddDays(-1)
		case k.Is("C-ArrowRight"):
			d = d.AddDays(1)
		}
		e.setDate(d, false)
	default:
		return false
	}
	return true
}

// ---- rendering ------------------------------------------------------------------

func (e *Editor) renderChips() {
	Clear(e.box)
	for i, c := range e.chips {
		ch := E("span", "chipx")
		Add(ch, Txt(core.DisplayValue(e.app.r, e.o.Entity, e.o.Field, []any{c})))
		x := ET("b", "", "×")
		Data(x, "x", strconv.Itoa(i))
		Add(ch, x)
		Add(e.box, ch)
	}
	Add(e.box, e.in)
	if !e.o.Embedded || e.focused {
		e.in.Call("focus")
	}
}

func (e *Editor) hidePopup() {
	e.pop.Set("className", "popup hidden")
}

func (e *Editor) renderPopup() {
	if e.dead {
		return
	}
	kind := e.o.Kind
	show := !e.o.Embedded || e.focused
	if !show || !(listKind(kind) || kind == core.EdDate || kind == core.EdBool) {
		e.hidePopup()
		return
	}
	Clear(e.pop)
	switch kind {
	case core.EdDate:
		e.pop.Set("className", "popup datepop")
		e.renderCalendar()
	default:
		if kind == core.EdSuggest && len(e.filtered) == 0 {
			e.hidePopup()
			return
		}
		e.pop.Set("className", "popup")
		list := E("div", "choices")
		n := 0
		for i, f := range e.filtered {
			if n >= 60 {
				Add(list, ET("div", "choice empty", fmt.Sprintf("… %d more — keep typing", len(e.filtered)-60)))
				break
			}
			n++
			c := e.choices[f.Index]
			cls := "choice"
			if i == e.sel {
				cls += " sel"
			}
			el := E("div", cls)
			Data(el, "i", strconv.Itoa(i))
			Add(el, choiceSwatch(c))
			lbl := E("span", "lbl")
			highlight(lbl, c.Label, f.Matched)
			Add(el, lbl)
			if c.Detail != "" {
				Add(el, ET("span", "det", c.Detail))
			}
			Add(list, el)
		}
		if len(e.filtered) == 0 {
			msg := "no matches"
			if kind == core.EdChips && strings.TrimSpace(e.text()) != "" && (e.o.Field == "tags" || e.o.Field == "options") {
				msg = "Enter creates '" + strings.TrimSpace(e.text()) + "'"
				Add(list, ET("div", "choice create", "✚ "+msg))
			} else {
				Add(list, ET("div", "choice empty", msg))
			}
		}
		Add(e.pop, list)
		hint := "↑↓ choose · Enter pick · Esc cancel"
		if kind == core.EdChips {
			hint = "Enter adds · Backspace removes · Tab/^Enter saves"
		}
		Add(e.pop, ET("div", "pophint", hint))
	}
	e.positionPopup()
	// keep the selection visible
	if s := e.pop.Call("querySelector", ".choice.sel"); s.Truthy() {
		s.Call("scrollIntoView", map[string]any{"block": "nearest"})
	}
}

func (e *Editor) positionPopup() {
	anchor := e.in
	if e.o.Kind == core.EdChips {
		anchor = e.box
	}
	rect := anchor.Call("getBoundingClientRect")
	left := rect.Get("left").Float()
	bottom := rect.Get("bottom").Float()
	top := rect.Get("top").Float()
	vh := global.Get("innerHeight").Float()
	vw := global.Get("innerWidth").Float()
	ph := e.pop.Get("offsetHeight").Float()
	pw := e.pop.Get("offsetWidth").Float()
	Style(e.pop, "min-width", Px(max(rect.Get("width").Float(), 200)))
	if bottom+ph+8 > vh && top-ph-4 > 0 {
		Style(e.pop, "top", Px(top-ph-2))
	} else {
		Style(e.pop, "top", Px(bottom+2))
	}
	if left+pw > vw-8 {
		left = max(8, vw-pw-8)
	}
	Style(e.pop, "left", Px(left))
}

func (e *Editor) renderCalendar() {
	t := e.text()
	preview := ""
	if strings.TrimSpace(t) == "" {
		preview = "(empty) — type fri, +3d, eom, 11/3 …"
	} else if v, err := e.parse(t); err != nil {
		preview = "✗ " + err.Error()
	} else if s, ok := v.(string); ok {
		if d, ok := core.ParseISO(s); ok {
			preview = t + " → " + d.Long()
			e.cal = d
		}
	}
	pv := ET("div", "preview", preview)
	if strings.HasPrefix(preview, "✗") {
		pv.Set("className", "preview c-err")
	}
	Add(e.pop, pv)
	y, m, _ := e.cal.YMD()
	head := E("div", "cal-h")
	prev := ET("span", "calnav", "‹")
	Data(prev, "cal", "prev")
	next := ET("span", "calnav", "›")
	Data(next, "cal", "next")
	Add(head, prev, ET("span", "", fmt.Sprintf("%s %d", core.MonthNames[m-1], y)), next)
	Add(e.pop, head)
	grid := E("div", "cal-g")
	for _, w := range core.WeekdayNames {
		Add(grid, ET("span", "cal-wd", w[:2]))
	}
	first, _ := core.FromYMD(y, m, 1)
	start := first.WeekStart()
	td := today()
	for i := 0; i < 42; i++ {
		d := start.AddDays(i)
		cls := "cal-d"
		if d.Month() != m {
			cls += " out"
		}
		if d.Weekday() >= 5 {
			cls += " we"
		}
		if d == td {
			cls += " today"
		}
		if d == e.cal {
			cls += " sel"
		}
		el := ET("span", cls, strconv.Itoa(d.Day()))
		Data(el, "day", strconv.Itoa(int(d)))
		Add(grid, el)
	}
	Add(e.pop, grid)
	foot := E("div", "cal-foot")
	tb := ET("span", "btn sm", "Today")
	Data(tb, "cal", "today")
	cb := ET("span", "btn sm ghost", "Clear")
	Data(cb, "cal", "clear")
	Add(foot, tb, cb, ET("span", "muted", "↑↓ week · PgUp/Dn month · ^←→ day"))
	Add(e.pop, foot)
}

// highlight writes label into el with matched rune indices wrapped in .hl.
func highlight(el js.Value, label string, matched []int) {
	if len(matched) == 0 {
		el.Set("textContent", label)
		return
	}
	set := map[int]bool{}
	for _, i := range matched {
		set[i] = true
	}
	var run []rune
	hl := false
	flush := func() {
		if len(run) == 0 {
			return
		}
		if hl {
			Add(el, ET("span", "hl", string(run)))
		} else {
			Add(el, Txt(string(run)))
		}
		run = run[:0]
	}
	for i, r := range []rune(label) {
		if set[i] != hl {
			flush()
			hl = set[i]
		}
		run = append(run, r)
	}
	flush()
}

func choiceSwatch(c core.Choice) js.Value {
	switch c.Style.Kind {
	case core.StProjectStatus:
		return E("span", "dot ps-"+c.Style.Value)
	case core.StTaskStatus:
		return E("span", "dot ts-"+c.Style.Value)
	case core.StPriority:
		return E("span", "dot pri-"+c.Style.Value)
	case core.StHealth:
		return E("span", "dot h-"+c.Style.Value)
	case core.StRole:
		return E("span", "dot role-"+c.Style.Value)
	case core.StColor:
		d := E("span", "dot")
		Style(d, "--c", c.Style.Value)
		return d
	case core.StUser:
		u := app.r.Users[c.Style.UserID]
		if u != nil {
			av := ET("span", "av", u.Initials())
			Style(av, "background", userColor(u.ID))
			return av
		}
	case core.StMuted:
		return E("span", "dot none")
	}
	return E("span", "dot none")
}
