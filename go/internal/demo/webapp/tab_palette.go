//go:build js && wasm

package webapp

// Tab 12 — Palette: the ^K palette shown inline and always open; tab 13 —
// Dialogs: buttons opening each kind of popup.

import (
	"strings"
	"syscall/js"

	"git.wharf.fluidgrid.org/jcz/tictactoe/go/internal/demo/core"
)

type paletteTab struct {
	tabBase
	p    *paletteList
	last js.Value
}

func newPaletteTab(b tabBase) Tab {
	t := &paletteTab{tabBase: b}
	t.build = t.mount
	return t
}

func (t *paletteTab) mount(area js.Value) {
	t.last = E("div", "lastcmd")
	Data(t.last, "key", "palette.last")
	box := E("div", "inlinepal")
	Data(box, "popup", "palette")
	t.p = newPaletteList(t.a, &t.scope)
	t.addCtl(t.p.in, "palette-search", "palette.last")
	t.p.onRun = func() { t.p.in.Set("value", ""); t.p.sel = 0; t.p.sig = ""; t.p.fill() }
	Add(box, t.p.in, t.p.list)
	Add(area, t.last, box)
}

// Shown: the search box takes the focus on entering the tab.
func (t *paletteTab) Shown() {
	After(0, func() {
		if !t.a.focusSide {
			t.Focus(0)
		}
	})
}

func (t *paletteTab) Key(k Key) bool { return t.p.Key(k) }

func (t *paletteTab) Render() {
	last := t.a.v.String("palette.last")
	e := t.a.v.Get("palette.last")
	Clear(t.last)
	Add(t.last, ET("span", "muted", "Last command: "))
	if last == "" {
		Add(t.last, ET("span", "muted", "— none yet —"))
	} else {
		Add(t.last, ET("b", "", core.SafeText(last)))
		if e.By != "" {
			Add(t.last, ET("span", "muted", " · by "+core.SafeText(e.By)))
		}
	}
}

func (t *paletteTab) Hints() []hint {
	return []hint{{"type", "Search", ""}, {"↑↓", "Move", ""}, {"Enter", "Run", "Enter"}}
}

// ---- tab 13 -------------------------------------------------------------------------------------

type dialogsTab struct {
	tabBase
	profile js.Value
	result  js.Value
}

func newDialogsTab(b tabBase) Tab {
	t := &dialogsTab{tabBase: b}
	t.build = t.mount
	return t
}

var dialogButtons = []struct{ label, name string }{
	{"Confirm…", "confirm"}, {"Form…", "form"}, {"Message", "message"}, {"Toast", "toast"}, {"Broadcast…", "broadcast"},
}

func (t *dialogsTab) mount(area js.Value) {
	row := E("div", "btnrow")
	for i, b := range dialogButtons {
		i := i
		btn := ET("button", "btn big", b.label)
		Attr(btn, "type", "button")
		key := ""
		if b.name == "form" {
			key = "dialog.profile"
		}
		t.addCtl(btn, b.name, key)
		t.scope.On(btn, "click", func(js.Value) { t.cur = i; t.open(i) })
		Add(row, btn)
	}
	t.profile = E("div", "profilecard")
	Data(t.profile, "key", "dialog.profile")
	Add(area, row, t.profile)
}

func (t *dialogsTab) open(i int) {
	a := t.a
	switch dialogButtons[i].name {
	case "confirm":
		d := newConfirm(a, "Confirm", "Delete 3 files?\nThis is a demo: nothing is deleted.", "Delete", func() {
			a.info("Confirm: you chose Delete")
		})
		d.cancel = func() { a.info("Confirm: you chose Cancel") }
		a.OpenDialog(d)
	case "form":
		a.OpenDialog(newProfileDialog(a))
	case "message":
		a.OpenDialog(newMessage(a, "Message", "This is an information box.\nIt has a single OK button."))
	case "toast":
		a.toast("ok", "Toast", "a local toast — only you see it")
	case "broadcast":
		a.OpenDialog(newPrompt(a, "Broadcast", "Message", "", "Send", core.MaxToast, func(v string) error {
			return a.Broadcast(v)
		}))
	}
}

func (t *dialogsTab) Key(k Key) bool {
	n := len(dialogButtons)
	switch {
	case k.Is("ArrowLeft"):
		t.Focus((t.cur + n - 1) % n)
	case k.Is("ArrowRight"):
		t.Focus((t.cur + 1) % n)
	case k.Is("Enter", " "):
		t.open(t.cur)
	default:
		return false
	}
	return true
}

func (t *dialogsTab) Render() {
	p := t.a.v.Profile()
	Clear(t.profile)
	Add(t.profile, ET("div", "phead", "dialog.profile"))
	kv := E("dl", "kv")
	Add(kv, ET("dt", "", "Name"), ET("dd", "", core.SafeText(p.Name)), ET("dt", "", "Email"), ET("dd", "", core.SafeText(p.Email)))
	Add(t.profile, kv)
}

func (t *dialogsTab) Hints() []hint {
	return []hint{{"←→", "Choose", ""}, {"Enter", "Open " + strings.TrimSuffix(dialogButtons[t.cur].label, "…"), "Enter"}, {"Tab", "Next", "Tab"}}
}
