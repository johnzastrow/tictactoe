//go:build js && wasm

package webapp

// The tab framework: every tab embeds tabBase, which owns its section, its
// controls (in Tab order, each tagged data-control / data-ci / data-key)
// and the focus among them. A tab builds its DOM once (state survives tab
// switches) and updates it in Render.

import (
	"strconv"
	"strings"
	"syscall/js"

	"git.wharf.fluidgrid.org/jcz/tictactoe/go/internal/demo/core"
)

// syncEv tells a tab how one of its keys changed.
type syncEv struct {
	by       string // who changed it
	mine     bool   // our own change coming back
	conflict bool   // our set lost: the value is now theirs
	rejected bool   // our set was refused (rolled back)
	welcome  bool   // (re)connected: every value replaced
}

type Tab interface {
	Info() core.Tab
	Mount(sec, area, vlist js.Value)
	Section() js.Value
	ValueList() js.Value
	Render()
	Key(k Key) bool
	Hints() []hint
	Focus(i int)
	FocusCur()
	SetCur(i int)
	Move(d int) bool
	CurKey() string
	Synced(key string, ev syncEv)
	Shown()
	Destroy()
}

type ctl struct {
	el   js.Value
	name string
	key  string // the value key it edits ("" for local-only controls)
}

type tabBase struct {
	a     *App
	info  core.Tab
	sec   js.Value
	vlist js.Value
	ctls  []ctl
	cur   int
	scope Scope
	build func(area js.Value)
}

func (b *tabBase) Info() core.Tab        { return b.info }
func (b *tabBase) Section() js.Value     { return b.sec }
func (b *tabBase) ValueList() js.Value   { return b.vlist }
func (b *tabBase) Hints() []hint         { return nil }
func (b *tabBase) Key(Key) bool          { return false }
func (b *tabBase) Render()               {}
func (b *tabBase) Synced(string, syncEv) {}
func (b *tabBase) Shown()                {}
func (b *tabBase) Destroy()              { b.scope.Release() }

func (b *tabBase) Mount(sec, area, vlist js.Value) {
	b.sec, b.vlist = sec, vlist
	if b.build != nil {
		b.build(area)
	}
}

// addCtl registers a focusable control; non-input elements get tabindex 0.
func (b *tabBase) addCtl(el js.Value, name, key string) int {
	i := len(b.ctls)
	b.ctls = append(b.ctls, ctl{el, name, key})
	Data(el, "control", name)
	Data(el, "ci", strconv.Itoa(i))
	if key != "" {
		Data(el, "key", key)
	}
	tag := strings.ToLower(el.Get("tagName").String())
	if tag != "input" && tag != "textarea" && tag != "button" {
		Attr(el, "tabindex", "0")
	}
	return i
}

func (b *tabBase) Focus(i int) {
	if len(b.ctls) == 0 {
		b.a.sidebar.Call("focus")
		return
	}
	i = max(0, min(i, len(b.ctls)-1))
	b.cur = i
	b.ctls[i].el.Call("focus", map[string]any{"preventScroll": false})
	b.a.Invalidate()
}

func (b *tabBase) FocusCur() { b.Focus(b.cur) }

func (b *tabBase) SetCur(i int) {
	if i >= 0 && i < len(b.ctls) && i != b.cur {
		b.cur = i
		b.a.closePopUnless(b.ctls[i].el)
	}
}

// Move goes to the next / previous control; false when Shift+Tab is
// pressed on the first one (the app then returns to the sidebar).
func (b *tabBase) Move(d int) bool {
	if len(b.ctls) == 0 {
		return false
	}
	n := b.cur + d
	if n < 0 {
		return false
	}
	b.a.closePop()
	b.Focus(n % len(b.ctls))
	return true
}

func (b *tabBase) CurKey() string {
	if b.cur < len(b.ctls) {
		return b.ctls[b.cur].key
	}
	return ""
}

// isCur: control i has the DOM focus (or contains it).
func (b *tabBase) isCur(i int) bool {
	if i >= len(b.ctls) || b.a.focusSide || b.a.cur().Info().ID != b.info.ID {
		return false
	}
	ae := doc.Get("activeElement")
	return b.cur == i && ae.Truthy() && b.ctls[i].el.Call("contains", ae).Bool()
}

// closePopUnless closes the popup unless it is anchored to el.
func (a *App) closePopUnless(el js.Value) {
	if a.pop != nil && !(a.pop.anchor.Truthy() && a.pop.anchor.Equal(el)) {
		a.closePop()
	}
}

// makeTabs builds the 16 tabs in display order.
func makeTabs(a *App) []Tab {
	var out []Tab
	for _, t := range core.Tabs {
		base := tabBase{a: a, info: t}
		var tab Tab
		switch t.ID {
		case "text":
			tab = newTextTab(base)
		case "number":
			tab = newNumberTab(base)
		case "date":
			tab = newDateTab(base)
		case "select":
			tab = newSelectTab(base)
		case "combo":
			tab = newComboTab(base)
		case "tags":
			tab = newTagsTab(base)
		case "textarea":
			tab = newTextareaTab(base)
		case "choice":
			tab = newChoiceTab(base)
		case "cascade":
			tab = newCascadeTab(base)
		case "grid":
			tab = newGridTab(base)
		case "menus":
			tab = newMenusTab(base)
		case "palette":
			tab = newPaletteTab(base)
		case "dialogs":
			tab = newDialogsTab(base)
		case "charts":
			tab = newChartsTab(base)
		case "kanban":
			tab = newKanbanTab(base)
		case "gantt":
			tab = newGanttTab(base)
		}
		out = append(out, tab)
	}
	return out
}

// field makes a labelled row: label, then the control area, then a message line.
func field(label string) (row, ctlArea, msg js.Value) {
	row = E("div", "frow2")
	ctlArea = E("div", "fctl")
	msg = E("div", "fmsg2")
	Add(row, ET("label", "flabel2", label), ctlArea, msg)
	return
}

// setMsg shows a field message: kind err (red), warn (⚠) or info.
func setMsg(el js.Value, kind, text string) {
	el.Set("className", "fmsg2 "+kind)
	el.Set("textContent", text)
}

// ---- dropdown ------------------------------------------------------------------------------------

// Dropdown is a closed control showing the value as a chip; it opens a list
// popup (data-popup="dropdown"): ↑↓ move, typing filters (the first letter
// jumps), Enter picks, Esc closes. Tabs 4 and 9 and the grid's Status use it.
type Dropdown struct {
	a           *App
	btn         js.Value // the closed control (focusable)
	anchor      js.Value // where the list opens (default btn)
	opts        []core.Choice
	value       string
	placeholder string
	disabled    string // non-empty: why it can't open (shown as ℹ)
	open        bool
	filter      string
	sel         int
	onPick      func(id string)
	onClose     func()
	sig, popSig string
	scope       *Scope
	ps          Scope // the list popup's listeners (rebuilt with it)
}

func newDropdown(a *App, scope *Scope, opts []core.Choice, onPick func(string)) *Dropdown {
	d := &Dropdown{a: a, opts: opts, onPick: onPick, scope: scope, placeholder: "— none —"}
	d.btn = E("div", "dropbtn")
	Attr(d.btn, "role", "combobox")
	scope.On(d.btn, "mousedown", func(ev js.Value) {
		if ev.Get("button").Int() != 0 {
			return
		}
		ev.Call("preventDefault")
		d.btn.Call("focus")
		if d.open {
			d.Close()
		} else {
			d.Open()
		}
	})
	return d
}

func (d *Dropdown) visible() []int {
	f := strings.ToLower(d.filter)
	var out []int
	for i, o := range d.opts {
		if f == "" || strings.Contains(strings.ToLower(o.Label), f) {
			out = append(out, i)
		}
	}
	return out
}

func (d *Dropdown) Open() {
	if d.disabled != "" {
		d.a.info(d.disabled)
		return
	}
	d.open, d.filter, d.sel = true, "", 0
	for i, o := range d.opts {
		if o.ID == d.value {
			d.sel = i
		}
	}
	d.popSig = ""
	d.a.Invalidate()
}

func (d *Dropdown) Close() {
	if !d.open {
		return
	}
	d.open = false
	d.a.ClearPop(d)
	if d.onClose != nil {
		d.onClose()
	}
	d.a.Invalidate()
}

func (d *Dropdown) pick(vi int) {
	vis := d.visible()
	if vi < 0 || vi >= len(vis) {
		return
	}
	id := d.opts[vis[vi]].ID
	d.open = false
	d.a.ClearPop(d)
	d.onPick(id)
	d.a.Invalidate()
}

// Key handles the dropdown's keys; closed it opens on Enter/Space/↓.
func (d *Dropdown) Key(k Key) bool {
	if !d.open {
		if k.Is("Enter", " ", "ArrowDown") {
			d.Open()
			return true
		}
		return false
	}
	vis := d.visible()
	// d.sel indexes vis while open
	switch {
	case k.Is("ArrowDown"):
		if len(vis) > 0 {
			d.sel = (d.sel + 1) % len(vis)
		}
	case k.Is("ArrowUp"):
		if len(vis) > 0 {
			d.sel = (d.sel - 1 + len(vis)) % len(vis)
		}
	case k.Is("Home"):
		d.sel = 0
	case k.Is("End"):
		d.sel = max(0, len(vis)-1)
	case k.Is("Enter", " ") && !(k.Is(" ") && d.filter != ""):
		d.pick(d.sel)
	case k.Is("Escape"):
		d.Close()
	case k.Is("Backspace"):
		if d.filter != "" {
			r := []rune(d.filter)
			d.filter = string(r[:len(r)-1])
			d.sel = 0
		}
	case k.Printable():
		d.filter += k.Name
		d.sel = 0
		// the first option starting with the filter wins
		f := strings.ToLower(d.filter)
		for i, oi := range d.visible() {
			if strings.HasPrefix(strings.ToLower(d.opts[oi].Label), f) {
				d.sel = i
				break
			}
		}
	case k.Is("Tab", "S-Tab"):
		d.Close()
		return false
	default:
		return true // swallow while open
	}
	d.a.Invalidate()
	return true
}

// chip renders a coloured value chip.
func chip(c core.Choice) js.Value {
	el := ET("span", "chip", c.Label)
	if c.Color != "" {
		Style(el, "--c", c.Color)
	} else {
		Style(el, "--c", "var(--accent)")
	}
	return el
}

// Render updates the closed control and the list popup.
func (d *Dropdown) Render() {
	cls := "dropbtn"
	if d.disabled != "" {
		cls += " disabled"
	}
	if d.open {
		cls += " open"
	}
	sig := d.value + "|" + cls + "|" + strconv.Itoa(len(d.opts))
	if sig != d.sig {
		d.sig = sig
		d.btn.Set("className", cls)
		Clear(d.btn)
		if c, ok := core.FindChoice(d.opts, d.value); ok {
			Add(d.btn, chip(c))
		} else {
			Add(d.btn, ET("span", "muted", d.placeholder))
		}
		Add(d.btn, ET("span", "caret", "▾"))
	}
	if !d.open {
		return
	}
	vis := d.visible()
	psig := d.filter + "|" + strconv.Itoa(d.sel) + "|" + d.value + "|" + strconv.Itoa(len(vis))
	if psig == d.popSig && d.a.PopOwner(d) {
		return
	}
	d.popSig = psig
	el := E("div", "choices")
	if d.filter != "" {
		Add(el, ET("div", "pfilter", "filter: "+d.filter))
	}
	for i, oi := range vis {
		o := d.opts[oi]
		cls := "choice"
		if i == d.sel {
			cls += " sel"
		}
		c := E("div", cls)
		Data(c, "i", strconv.Itoa(i))
		mark := " "
		if o.ID == d.value {
			mark = "✓"
		}
		Add(c, ET("span", "chk", mark), chip(o))
		Add(el, c)
	}
	if len(vis) == 0 {
		Add(el, ET("div", "choice empty", "nothing matches"))
	}
	Add(el, ET("div", "pophint", "↑↓ move · type to filter · Enter pick · Esc close"))
	d.ps.Release()
	d.ps.On(el, "mousedown", func(ev js.Value) {
		ev.Call("preventDefault")
		if c := Closest(ev.Get("target"), "[data-i]"); c.Truthy() {
			i, _ := strconv.Atoi(DataOf(c, "i"))
			d.pick(i)
		}
	})
	anchor := d.anchor
	if !anchor.Truthy() {
		anchor = d.btn
	}
	d.a.SetPop(d, "dropdown", anchor, el, func() {
		d.open = false
		if d.onClose != nil {
			d.onClose()
		}
	})
}

// dropHints are the hint bar entries for a dropdown.
func (d *Dropdown) Hints() []hint {
	if d.open {
		return []hint{{"↑↓", "Move", ""}, {"type", "Filter", ""}, {"Enter", "Pick", "Enter"}, {"Esc", "Close", "Escape"}}
	}
	return []hint{{"Enter", "Open", "Enter"}, {"Tab", "Next", "Tab"}}
}
