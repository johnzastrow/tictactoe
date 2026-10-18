//go:build js && wasm

package webapp

// Tab 1 — Text: three synced single-line fields with live validation, plus
// a masked, local-only password field.

import (
	"strings"
	"syscall/js"

	"git.wharf.fluidgrid.org/jcz/tictactoe/go/internal/demo/core"
)

// tfield is one synced text input and its edit state.
type tfield struct {
	key    string
	in     js.Value
	msg    js.Value
	ci     int
	dirty  bool   // the text differs from the synced value
	base   int64  // version when editing began
	theirs string // someone else changed it while we were editing
	err    string
	upper  bool // text.code: uppercase as typed
}

type textTab struct {
	tabBase
	fields []*tfield
	pw     js.Value
	pwShow bool
	eye    js.Value
}

func newTextTab(b tabBase) Tab {
	t := &textTab{tabBase: b}
	t.build = t.mount
	return t
}

func (t *textTab) mount(area js.Value) {
	form := E("div", "form2")
	for _, f := range []struct{ key, label, ph string }{
		{"text.name", "Name", "1–80 characters"},
		{"text.email", "Email", "name@example.com"},
		{"text.code", "Code", "ABC-123"},
	} {
		row, c, msg := field(f.label)
		in := E("input", "ed2")
		Attr(in, "spellcheck", "false")
		Attr(in, "autocomplete", "off")
		Attr(in, "placeholder", f.ph)
		tf := &tfield{key: f.key, in: in, msg: msg, upper: f.key == "text.code"}
		tf.ci = t.addCtl(in, strings.TrimPrefix(f.key, "text."), f.key)
		Add(c, in)
		Add(form, row)
		t.fields = append(t.fields, tf)
		t.scope.On(in, "input", func(js.Value) { t.edited(tf) })
	}
	// password: masked, local only, never sent
	row, c, msg := field("Password")
	t.pw = E("input", "ed2")
	Attr(t.pw, "type", "password")
	Attr(t.pw, "autocomplete", "new-password")
	Attr(t.pw, "placeholder", "local only — never sent")
	t.addCtl(t.pw, "password", "")
	t.eye = ET("button", "btn eye", "👁 show")
	Attr(t.eye, "type", "button")
	Attr(t.eye, "tabindex", "-1")
	Data(t.eye, "control", "password-show")
	t.scope.On(t.eye, "mousedown", func(ev js.Value) { ev.Call("preventDefault"); t.togglePw() })
	setMsg(msg, "info", "Masked with •; ^O or 👁 shows it. It stays in this page.")
	Add(c, t.pw, t.eye)
	Add(form, row)
	Add(area, form)
}

func (t *textTab) togglePw() {
	t.pwShow = !t.pwShow
	if t.pwShow {
		Attr(t.pw, "type", "text")
		t.eye.Set("textContent", "👁 hide")
	} else {
		Attr(t.pw, "type", "password")
		t.eye.Set("textContent", "👁 show")
	}
}

// edited runs on every keystroke: uppercase the code, track dirtiness, validate.
func (t *textTab) edited(f *tfield) {
	val := f.in.Get("value").String()
	if f.upper && val != strings.ToUpper(val) {
		s, e := f.in.Get("selectionStart"), f.in.Get("selectionEnd")
		val = strings.ToUpper(val)
		f.in.Set("value", val)
		f.in.Call("setSelectionRange", s, e)
	}
	synced := t.a.v.String(f.key)
	if !f.dirty {
		f.base = t.a.v.Version(f.key)
	}
	f.dirty = val != synced
	f.err = ""
	if _, err := core.NormalizeValue(f.key, val); err != nil {
		f.err = err.Error()
	}
	t.a.Invalidate()
}

func (t *textTab) commit(f *tfield) {
	if !f.dirty {
		t.a.info(f.key + " is unchanged")
		return
	}
	if f.err != "" {
		t.a.setStatus("err", "✗ "+f.key+": "+f.err)
		return
	}
	val := f.in.Get("value").String()
	err := t.a.CommitFrom(f.key, val, f.base, func(m core.AckMsg) {
		if m.OK {
			f.dirty, f.theirs = false, ""
			// show the normalised value (trimmed, lowercased email)
			f.in.Set("value", t.a.v.String(f.key))
		} else if !m.Conflict {
			f.err = m.Error
		}
	})
	if err != nil {
		f.err = err.Error()
	}
}

func (t *textTab) revert(f *tfield) {
	f.dirty, f.theirs, f.err = false, "", ""
	f.in.Set("value", t.a.v.String(f.key))
}

func (t *textTab) curField() *tfield {
	for _, f := range t.fields {
		if f.ci == t.cur {
			return f
		}
	}
	return nil
}

func (t *textTab) Key(k Key) bool {
	if k.Is("C-o") {
		t.togglePw()
		return true
	}
	f := t.curField()
	if f == nil {
		if k.Is("C-u") {
			t.pw.Set("value", "")
			return true
		}
		return false
	}
	switch {
	case k.Is("Enter"):
		t.commit(f)
	case k.Is("Escape") && (f.dirty || f.theirs != "" || f.err != ""):
		t.revert(f)
	case k.Is("C-u"):
		f.in.Set("value", "")
		t.edited(f)
	default:
		return false
	}
	t.a.Invalidate()
	return true
}

func (t *textTab) Synced(key string, ev syncEv) {
	for _, f := range t.fields {
		if f.key != key {
			continue
		}
		switch {
		case ev.conflict:
			// lost: show theirs
			f.dirty, f.theirs, f.err = false, "", ""
			f.in.Set("value", t.a.v.String(key))
		case ev.welcome || ev.mine || ev.rejected:
		case f.dirty:
			f.theirs = ev.by // keep the user's text; Esc takes theirs
		}
	}
}

func (t *textTab) Render() {
	for _, f := range t.fields {
		synced := t.a.v.String(f.key)
		if !f.dirty && f.in.Get("value").String() != synced {
			f.in.Set("value", synced)
		}
		cls := "ed2"
		switch {
		case f.err != "":
			cls += " bad"
			setMsg(f.msg, "err", "✗ "+f.err)
		case f.theirs != "":
			setMsg(f.msg, "warn", "⚠ changed by "+f.theirs+" — Esc takes theirs")
		case f.dirty:
			cls += " dirty"
			setMsg(f.msg, "info", "● unsaved — Enter saves, Esc reverts")
		default:
			setMsg(f.msg, "", "")
		}
		f.in.Set("className", cls)
	}
}

func (t *textTab) Hints() []hint {
	if t.curField() == nil {
		return []hint{{"^O", "Show/hide", "C-o"}, {"^U", "Clear", "C-u"}, {"Tab", "Next", "Tab"}}
	}
	return []hint{{"Enter", "Save", "Enter"}, {"Esc", "Revert", "Escape"}, {"^U", "Clear", "C-u"}, {"Tab", "Next field", "Tab"}, {"⇧Tab", "Back", "S-Tab"}}
}
