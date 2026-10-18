//go:build js && wasm

package webapp

// Tab 8 — Choices: a checkbox, a checkbox group, a horizontal radio group
// and a toggle switch. Every change commits at once.

import (
	"fmt"
	"strconv"
	"strings"
	"syscall/js"

	"git.wharf.fluidgrid.org/jcz/tictactoe/go/internal/demo/core"
)

type choiceTab struct {
	tabBase
	notify, channels, size, dark js.Value
	chCur, sizeCur               int
	sig                          string
}

func newChoiceTab(b tabBase) Tab {
	t := &choiceTab{tabBase: b}
	t.build = t.mount
	return t
}

func (t *choiceTab) mount(area js.Value) {
	form := E("div", "form2")
	mk := func(label, name, key, cls, role string) js.Value {
		row, c, _ := field(label)
		el := E("div", cls)
		Attr(el, "role", role)
		t.addCtl(el, name, key)
		Add(c, el)
		Add(form, row)
		return el
	}
	t.notify = mk("Notify me", "notify", "choice.notify", "check", "checkbox")
	t.channels = mk("Channels", "channels", "choice.channels", "group", "group")
	t.size = mk("Size", "size", "choice.size", "group radios", "radiogroup")
	t.dark = mk("Dark mode", "dark", "choice.dark", "switch", "switch")
	Add(area, form)
	t.scope.On(t.notify, "click", func(js.Value) { t.toggleNotify() })
	t.scope.On(t.dark, "click", func(js.Value) { _ = t.a.Commit("choice.dark", !t.a.v.Bool("choice.dark")) })
	t.scope.On(t.channels, "click", func(ev js.Value) {
		if it := Closest(ev.Get("target"), "[data-i]"); it.Truthy() {
			t.chCur, _ = strconv.Atoi(DataOf(it, "i"))
			t.toggleChannel()
		}
	})
	t.scope.On(t.size, "click", func(ev js.Value) {
		if it := Closest(ev.Get("target"), "[data-i]"); it.Truthy() {
			t.sizeCur, _ = strconv.Atoi(DataOf(it, "i"))
			_ = t.a.Commit("choice.size", core.Sizes[t.sizeCur].ID)
		}
	})
}

func (t *choiceTab) toggleNotify() { _ = t.a.Commit("choice.notify", !t.a.v.Bool("choice.notify")) }

func (t *choiceTab) toggleChannel() {
	if !t.a.v.Bool("choice.notify") {
		t.a.info("turn on Notify first")
		return
	}
	id := core.Channels[t.chCur].ID
	cur := t.a.v.Strings("choice.channels")
	var out []string
	found := false
	for _, c := range cur {
		if c == id {
			found = true
		} else {
			out = append(out, c)
		}
	}
	if !found {
		out = append(out, id)
	}
	if out == nil {
		out = []string{}
	}
	_ = t.a.Commit("choice.channels", out)
}

func (t *choiceTab) Key(k Key) bool {
	prev, next := k.Is("ArrowLeft", "ArrowUp"), k.Is("ArrowRight", "ArrowDown")
	act := k.Is(" ", "Enter")
	switch t.cur {
	case 0:
		if act {
			t.toggleNotify()
			return true
		}
	case 1:
		n := len(core.Channels)
		switch {
		case prev:
			t.chCur = (t.chCur + n - 1) % n
		case next:
			t.chCur = (t.chCur + 1) % n
		case act:
			t.toggleChannel()
		default:
			return false
		}
		t.a.Invalidate()
		return true
	case 2:
		n := len(core.Sizes)
		switch {
		case prev:
			t.sizeCur = (t.sizeCur + n - 1) % n
		case next:
			t.sizeCur = (t.sizeCur + 1) % n
		case act:
			_ = t.a.Commit("choice.size", core.Sizes[t.sizeCur].ID)
		default:
			return false
		}
		t.a.Invalidate()
		return true
	case 3:
		if act {
			_ = t.a.Commit("choice.dark", !t.a.v.Bool("choice.dark"))
			return true
		}
	}
	return false
}

func (t *choiceTab) Shown() {
	// start the radio cursor on the stored size
	for i, s := range core.Sizes {
		if s.ID == t.a.v.String("choice.size") {
			t.sizeCur = i
		}
	}
}

func (t *choiceTab) Render() {
	notify := t.a.v.Bool("choice.notify")
	chans := t.a.v.Strings("choice.channels")
	size := t.a.v.String("choice.size")
	dark := t.a.v.Bool("choice.dark")
	sig := fmt.Sprint(notify, chans, size, dark, t.chCur, t.sizeCur, t.cur)
	if sig == t.sig {
		return
	}
	t.sig = sig

	box := "☐"
	if notify {
		box = "☑"
	}
	Replace(t.notify, ET("span", "box", box), Txt(" Send me notifications"))
	Attr(t.notify, "aria-checked", strconv.FormatBool(notify))

	Clear(t.channels)
	cls := "group"
	if !notify {
		cls += " disabled"
	}
	t.channels.Set("className", cls)
	for i, c := range core.Channels {
		on := strings.Contains(","+strings.Join(chans, ",")+",", ","+c.ID+",")
		it := E("span", "opt")
		if i == t.chCur {
			it.Set("className", "opt cur")
		}
		Data(it, "i", strconv.Itoa(i))
		mark := "☐"
		if on {
			mark = "☑"
		}
		Add(it, ET("span", "box", mark), Txt(" "+c.Label))
		Add(t.channels, it)
	}
	if !notify {
		Add(t.channels, ET("span", "muted hint2", "(Notify is off)"))
	}

	Clear(t.size)
	for i, s := range core.Sizes {
		it := E("span", "opt")
		if i == t.sizeCur {
			it.Set("className", "opt cur")
		}
		Data(it, "i", strconv.Itoa(i))
		mark := "○"
		if s.ID == size {
			mark = "◉"
		}
		Add(it, ET("span", "box", mark), Txt(" "+s.Label))
		Add(t.size, it)
	}

	Clear(t.dark)
	sw := E("span", "track")
	if dark {
		sw.Set("className", "track on")
	}
	Add(sw, E("span", "knob"))
	label := "Off"
	if dark {
		label = "On"
	}
	Add(t.dark, sw, ET("span", "swl", label))
	Attr(t.dark, "aria-checked", strconv.FormatBool(dark))
}

func (t *choiceTab) Hints() []hint {
	switch t.cur {
	case 1, 2:
		return []hint{{"←→", "Move", ""}, {"Space", "Select", " "}, {"Tab", "Next group", "Tab"}}
	}
	return []hint{{"Space", "Toggle", " "}, {"Tab", "Next", "Tab"}}
}
