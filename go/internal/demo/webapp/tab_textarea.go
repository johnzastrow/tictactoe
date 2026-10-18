//go:build js && wasm

package webapp

// Tab 7 — Text area: a multi-line editor with explicit save (^S), a
// position / word / length footer, and a draft that survives leaving.

import (
	"fmt"
	"strings"
	"syscall/js"
	"unicode/utf16"
	"unicode/utf8"

	"git.wharf.fluidgrid.org/jcz/tictactoe/go/internal/demo/core"
)

type textareaTab struct {
	tabBase
	ta     js.Value
	foot   js.Value
	state  js.Value
	dirty  bool
	base   int64
	theirs string
	err    string
}

func newTextareaTab(b tabBase) Tab {
	t := &textareaTab{tabBase: b}
	t.build = t.mount
	return t
}

func (t *textareaTab) mount(area js.Value) {
	wrap := E("div", "tawrap")
	t.ta = E("textarea", "notes")
	Attr(t.ta, "spellcheck", "false")
	Attr(t.ta, "rows", "12")
	t.addCtl(t.ta, "notes", "textarea.notes")
	bar := E("div", "tafoot")
	t.state = E("span", "tastate")
	t.foot = E("span", "muted mono")
	Add(bar, t.state, E("span", "spacer"), t.foot)
	Add(wrap, t.ta, bar)
	Add(area, wrap)
	t.scope.On(t.ta, "input", func(js.Value) {
		if !t.dirty {
			t.base = t.a.v.Version("textarea.notes")
		}
		t.dirty = t.ta.Get("value").String() != t.a.v.String("textarea.notes")
		t.err = ""
		if _, err := core.NormalizeValue("textarea.notes", t.ta.Get("value").String()); err != nil {
			t.err = err.Error()
		}
		t.a.Invalidate()
	})
	// entering from the sidebar puts the caret at the very start (as the TUIs);
	// "focus" runs before the document's focusin, so focusSide is still set
	t.scope.On(t.ta, "focus", func(js.Value) {
		if t.a.focusSide {
			t.ta.Call("setSelectionRange", 0, 0)
			t.ta.Set("scrollTop", 0)
		}
	})
	for _, ev := range []string{"keyup", "click", "select"} {
		t.scope.On(t.ta, ev, func(js.Value) { t.a.Invalidate() })
	}
}

func (t *textareaTab) commit() {
	if !t.dirty {
		t.a.info("textarea.notes is unchanged")
		return
	}
	if t.err != "" {
		t.a.setStatus("err", "✗ textarea.notes: "+t.err)
		return
	}
	if err := t.a.CommitFrom("textarea.notes", t.ta.Get("value").String(), t.base, func(m core.AckMsg) {
		if m.OK {
			t.dirty, t.theirs = false, ""
		} else if !m.Conflict {
			t.err = m.Error
		}
	}); err != nil {
		t.err = err.Error()
	}
}

func (t *textareaTab) revert() {
	t.dirty, t.theirs, t.err = false, "", ""
	t.ta.Set("value", t.a.v.String("textarea.notes"))
	t.a.Invalidate()
}

func (t *textareaTab) Key(k Key) bool {
	switch {
	case k.Is("C-s"):
		t.commit()
	case k.Is("Escape") && t.dirty:
		t.a.OpenDialog(newConfirm(t.a, "Discard edits", "Revert the notes to the synced text?\nYour unsaved edits are lost.", "Revert", t.revert))
	case k.Is("Escape") && t.theirs != "":
		t.revert()
	default:
		return false
	}
	t.a.Invalidate()
	return true
}

func (t *textareaTab) Synced(key string, ev syncEv) {
	switch {
	case ev.conflict:
		t.revert()
	case ev.welcome || ev.mine || ev.rejected:
	case t.dirty:
		t.theirs = ev.by
	}
}

func (t *textareaTab) Render() {
	synced := t.a.v.String("textarea.notes")
	if !t.dirty && t.ta.Get("value").String() != synced {
		t.ta.Set("value", synced)
	}
	val := t.ta.Get("value").String()
	// caret position: selectionStart counts UTF-16 units
	u := utf16.Encode([]rune(val))
	pre := string(utf16.Decode(u[:min(len(u), t.ta.Get("selectionStart").Int())]))
	ln := strings.Count(pre, "\n") + 1
	col := utf8.RuneCountInString(pre[strings.LastIndexByte(pre, '\n')+1:]) + 1
	t.foot.Set("textContent", fmt.Sprintf("Ln %d, Col %d · %d words · %d/5000", ln, col,
		len(strings.Fields(val)), utf8.RuneCountInString(strings.ReplaceAll(val, "\r\n", "\n"))))
	cls := "notes"
	switch {
	case t.err != "":
		cls += " bad"
		setMsg(t.state, "err", "✗ "+t.err)
	case t.theirs != "":
		setMsg(t.state, "warn", "⚠ changed by "+t.theirs+" — Esc takes theirs")
	case t.dirty:
		cls += " dirty"
		setMsg(t.state, "info", "● unsaved — ^S saves, Esc reverts")
	default:
		setMsg(t.state, "ok", "✓ saved")
	}
	t.state.Set("className", t.state.Get("className").String()+" tastate")
	t.ta.Set("className", cls)
}

func (t *textareaTab) Hints() []hint {
	return []hint{{"^S", "Save", "C-s"}, {"Esc", "Revert", "Escape"}, {"Enter", "New line", ""}, {"Tab", "Leave (keeps draft)", "Tab"}}
}
