//go:build js && wasm

package webapp

// The command palette (ui-spec §3): one component, shown in a dialog by
// ^K / ^P and inline on tab 12. Fuzzy matching is core's (protocol §7);
// running a command also sets palette.last to its label.

import (
	"strconv"
	"syscall/js"

	"git.wharf.fluidgrid.org/jcz/tictactoe/go/internal/demo/core"
)

type palCmd struct {
	label, key string
	act        *action
}

// paletteCommands lists every command in the spec's order: Go to (16),
// resets, themes, Help, About, Open menu bar, Quit.
func paletteCommands() []palCmd {
	var out []palCmd
	add := func(id string) {
		if act := actionByID(id); act != nil {
			out = append(out, palCmd{act.Palette, keyLabel(act), act})
		}
	}
	for _, t := range core.Tabs {
		add("tab:" + t.ID)
	}
	add("reset_tab")
	add("reset_all")
	for _, th := range themes {
		add("theme:" + th)
	}
	for _, id := range []string{"help", "about", "menu", "leave"} {
		add(id)
	}
	return out
}

// paletteList is the search box and the ranked list.
type paletteList struct {
	app    *App
	cmds   []palCmd
	ranked []core.Ranked
	sel    int
	in     js.Value
	list   js.Value
	scope  *Scope
	sig    string
	onRun  func() // before running (the dialog closes itself)
}

func newPaletteList(a *App, scope *Scope) *paletteList {
	p := &paletteList{app: a, cmds: paletteCommands(), scope: scope}
	p.in = E("input", "ed search")
	Attr(p.in, "placeholder", "type a command…")
	Attr(p.in, "spellcheck", "false")
	Attr(p.in, "autocomplete", "off")
	p.list = E("div", "choices pal")
	scope.On(p.in, "input", func(js.Value) { p.sel = 0; p.fill() })
	scope.On(p.list, "mousedown", func(ev js.Value) {
		ev.Call("preventDefault")
		if c := Closest(ev.Get("target"), "[data-i]"); c.Truthy() {
			i, _ := strconv.Atoi(DataOf(c, "i"))
			p.runAt(i)
		}
	})
	p.fill()
	return p
}

func (p *paletteList) fill() {
	labels := make([]string, len(p.cmds))
	for i, c := range p.cmds {
		labels[i] = c.label
	}
	p.ranked = core.FuzzyRank(p.in.Get("value").String(), labels)
	if p.sel >= len(p.ranked) {
		p.sel = max(0, len(p.ranked)-1)
	}
	sig := strconv.Itoa(p.sel) + "|" + p.in.Get("value").String()
	if sig == p.sig {
		return
	}
	p.sig = sig
	Clear(p.list)
	for i, r := range p.ranked {
		c := p.cmds[r.Index]
		cls := "choice"
		if i == p.sel {
			cls += " sel"
		}
		el := E("div", cls)
		Data(el, "i", strconv.Itoa(i))
		lbl := E("span", "lbl")
		highlight(lbl, c.label, r.Match.Positions)
		Add(el, lbl)
		if c.key != "" {
			Add(el, ET("kbd", "pk", c.key))
		}
		Add(p.list, el)
	}
	if len(p.ranked) == 0 {
		Add(p.list, ET("div", "choice empty", "nothing matches"))
	}
	if s := p.list.Call("querySelector", ".choice.sel"); s.Truthy() {
		s.Call("scrollIntoView", map[string]any{"block": "nearest"})
	}
}

func (p *paletteList) runAt(i int) {
	if i < 0 || i >= len(p.ranked) {
		return
	}
	c := p.cmds[p.ranked[i].Index]
	if p.onRun != nil {
		p.onRun()
	}
	// remember it first (Quit tears the app down)
	_ = p.app.Commit("palette.last", c.label)
	c.act.Run(p.app)
	p.app.Invalidate()
}

// Key: ↑↓ move, Enter runs.
func (p *paletteList) Key(k Key) bool {
	n := len(p.ranked)
	switch {
	case k.Is("ArrowDown") && n > 0:
		p.sel = (p.sel + 1) % n
	case k.Is("ArrowUp") && n > 0:
		p.sel = (p.sel - 1 + n) % n
	case k.Is("Home") && !k.InInput:
		p.sel = 0
	case k.Is("Enter"):
		p.runAt(p.sel)
		return true
	default:
		return false
	}
	p.fill()
	return true
}

// highlight writes label into el with the runes at positions marked.
func highlight(el js.Value, label string, positions []int) {
	if len(positions) == 0 {
		el.Set("textContent", label)
		return
	}
	hit := map[int]bool{}
	for _, p := range positions {
		hit[p] = true
	}
	runes := []rune(label)
	start := 0
	for i := 0; i <= len(runes); i++ {
		if i == len(runes) || hit[i] != hit[start] {
			if i > start {
				s := string(runes[start:i])
				if hit[start] {
					Add(el, ET("span", "hl", s))
				} else {
					Add(el, Txt(s))
				}
			}
			start = i
		}
	}
}

// ---- dialog (^K / ^P) ----------------------------------------------------------------------------

type paletteDialog struct {
	app   *App
	p     *paletteList
	scope Scope
}

func newPaletteDialog(a *App) Dialog { return &paletteDialog{app: a} }

func (d *paletteDialog) Build() js.Value {
	box, body := dialogBox("Commands", "palette", "palette")
	d.p = newPaletteList(d.app, &d.scope)
	Attr(d.p.in, "id", "palette-input")
	Data(d.p.in, "control", "palette-search")
	d.p.onRun = func() { d.app.CloseDialog() }
	Add(body, d.p.in, d.p.list)
	return box
}

func (d *paletteDialog) AfterOpen()     { d.p.in.Call("focus") }
func (d *paletteDialog) Key(k Key) bool { return d.p.Key(k) }
func (d *paletteDialog) Hints() []hint {
	return []hint{{"type", "Search", ""}, {"↑↓", "Choose", ""}, {"Enter", "Run", "Enter"}, {"Esc", "Close", "Escape"}}
}
func (d *paletteDialog) Destroy() { d.scope.Release() }
