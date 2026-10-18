//go:build js && wasm

package webapp

// Tab 6 — Tags: chips plus an input with ranked suggestions (protocol §4)
// and a "Create" entry for new tags. Every add and remove commits.

import (
	"strconv"
	"strings"
	"syscall/js"

	"git.wharf.fluidgrid.org/jcz/tictactoe/go/internal/demo/core"
)

type tagItem struct {
	tag    string
	uses   int
	create bool
}

type tagsTab struct {
	tabBase
	box     js.Value
	chips   js.Value
	in      js.Value
	msg     js.Value
	open    bool
	sel     int
	chipSel int // -1: none (←/→ in the empty input select a chip)
	items   []tagItem
	sig     string
	popSig  string
	ps      Scope
}

func newTagsTab(b tabBase) Tab {
	t := &tagsTab{tabBase: b, chipSel: -1}
	t.build = t.mount
	return t
}

func (t *tagsTab) mount(area js.Value) {
	row, c, msg := field("Tags")
	t.msg = msg
	t.box = E("div", "tagbox")
	t.chips = E("span", "tagchips")
	t.in = E("input", "chipin")
	Attr(t.in, "spellcheck", "false")
	Attr(t.in, "autocomplete", "off")
	Attr(t.in, "placeholder", "add a tag…")
	Attr(t.in, "maxlength", "24")
	t.addCtl(t.in, "tags", "tags.tags")
	Data(t.box, "key", "tags.tags")
	Add(t.box, t.chips, t.in)
	Add(c, t.box)
	form := E("div", "form2")
	Add(form, row)
	Add(area, form, ET("p", "muted note", "Up to 12 tags: lowercase letters, digits, - and _. Stored sorted."))
	t.scope.On(t.in, "input", func(js.Value) { t.open, t.sel, t.chipSel = true, 0, -1; t.a.Invalidate() })
	t.scope.On(t.in, "focus", func(js.Value) { t.open = true; t.a.Invalidate() })
	t.scope.On(t.in, "blur", func(js.Value) {
		After(120, func() {
			if !t.in.Equal(doc.Get("activeElement")) {
				t.open, t.chipSel = false, -1
				t.a.ClearPop(t)
			}
		})
	})
	t.scope.On(t.box, "mousedown", func(ev js.Value) {
		tgt := ev.Get("target")
		if x := Closest(tgt, "[data-x]"); x.Truthy() {
			ev.Call("preventDefault")
			i, _ := strconv.Atoi(DataOf(x, "x"))
			t.remove(i)
			t.in.Call("focus")
			return
		}
		if !tgt.Equal(t.in) {
			ev.Call("preventDefault")
			t.in.Call("focus")
		}
	})
}

func (t *tagsTab) query() string { return strings.TrimSpace(t.in.Get("value").String()) }

func (t *tagsTab) buildItems() {
	cur := t.a.v.Strings("tags.tags")
	q := strings.ToLower(t.query())
	t.items = t.items[:0]
	exact := false
	for _, s := range core.SuggestTags(q, cur) {
		t.items = append(t.items, tagItem{tag: s.Tag, uses: s.Uses})
		exact = exact || s.Tag == q
	}
	present := false
	for _, c := range cur {
		present = present || c == q
	}
	if q != "" && !exact && !present {
		t.items = append(t.items, tagItem{tag: q, create: true})
	}
	if t.sel >= len(t.items) {
		t.sel = max(0, len(t.items)-1)
	}
}

func (t *tagsTab) add(tag string) {
	if !core.ValidTag(tag) {
		t.a.setStatus("err", "✗ tags: "+strconv.Quote(tag)+" is not a valid tag")
		return
	}
	list := append(append([]string{}, t.a.v.Strings("tags.tags")...), tag)
	if t.a.Commit("tags.tags", list) == nil {
		t.in.Set("value", "")
		t.sel = 0
	}
}

func (t *tagsTab) remove(i int) {
	cur := t.a.v.Strings("tags.tags")
	if i < 0 || i >= len(cur) {
		return
	}
	list := append(append([]string{}, cur[:i]...), cur[i+1:]...)
	_ = t.a.Commit("tags.tags", list)
	if t.chipSel >= len(list) {
		t.chipSel = len(list) - 1
	}
}

func (t *tagsTab) Key(k Key) bool {
	if t.cur != 0 {
		return false
	}
	empty := t.in.Get("value").String() == ""
	n := len(t.items)
	cur := t.a.v.Strings("tags.tags")
	switch {
	case k.Is("ArrowDown") && n > 0:
		t.open = true
		t.sel = (t.sel + 1) % n
	case k.Is("ArrowUp") && n > 0:
		t.sel = (t.sel - 1 + n) % n
	case k.Is("Enter"):
		if t.open && n > 0 {
			t.add(t.items[t.sel].tag)
		} else if q := strings.ToLower(t.query()); q != "" {
			t.add(q)
		}
	case k.Is("Backspace") && empty && len(cur) > 0:
		if t.chipSel >= 0 {
			t.remove(t.chipSel)
		} else {
			t.remove(len(cur) - 1)
		}
	case k.Is("ArrowLeft") && empty && len(cur) > 0:
		if t.chipSel < 0 {
			t.chipSel = len(cur) - 1
		} else {
			t.chipSel = max(0, t.chipSel-1)
		}
	case k.Is("ArrowRight") && empty && t.chipSel >= 0:
		t.chipSel++
		if t.chipSel >= len(cur) {
			t.chipSel = -1
		}
	case k.Is("Delete") && t.chipSel >= 0:
		t.remove(t.chipSel)
	case k.Is("Escape") && t.chipSel >= 0:
		t.chipSel = -1
	case k.Is("Escape") && t.open:
		t.open = false
		t.a.ClearPop(t)
	default:
		return false
	}
	t.a.Invalidate()
	return true
}

func (t *tagsTab) Render() {
	cur := t.a.v.Strings("tags.tags")
	sig := strings.Join(cur, ",") + "|" + strconv.Itoa(t.chipSel)
	if sig != t.sig {
		t.sig = sig
		Clear(t.chips)
		for i, tag := range cur {
			cls := "chipx"
			if i == t.chipSel {
				cls += " sel"
			}
			ch := E("span", cls)
			x := ET("b", "", "×")
			Data(x, "x", strconv.Itoa(i))
			Attr(x, "title", "remove")
			Add(ch, Txt(core.SafeText(tag)), x)
			Add(t.chips, ch)
		}
	}
	q := strings.ToLower(t.query())
	bad := q != "" && !core.ValidTag(q)
	if bad {
		t.box.Set("className", "tagbox bad")
		setMsg(t.msg, "err", "✗ a tag is a-z, 0-9, - or _, starting with a letter or digit (max 24)")
	} else {
		t.box.Set("className", "tagbox")
		setMsg(t.msg, "", strconv.Itoa(len(cur))+" / 12 tags")
	}
	t.buildItems()
	if !t.open || !t.isCur(0) {
		t.a.ClearPop(t)
		return
	}
	psig := q + "|" + strconv.Itoa(t.sel) + "|" + strings.Join(cur, ",")
	if psig == t.popSig && t.a.PopOwner(t) {
		return
	}
	t.popSig = psig
	el := E("div", "choices")
	for i, it := range t.items {
		cls := "choice"
		if i == t.sel {
			cls += " sel"
		}
		c := E("div", cls)
		Data(c, "i", strconv.Itoa(i))
		if it.create {
			c.Set("className", cls+" create")
			if !core.ValidTag(it.tag) {
				c.Set("className", cls+" create invalid")
			}
			Add(c, ET("span", "lbl", "✚ Create \""+it.tag+"\""))
		} else {
			Add(c, ET("span", "lbl", it.tag), ET("span", "det", strconv.Itoa(it.uses)+" uses"))
		}
		Add(el, c)
	}
	if len(t.items) == 0 {
		Add(el, ET("div", "choice empty", "no more suggestions"))
	}
	Add(el, ET("div", "pophint", "↑↓ choose · Enter add · Backspace removes the last chip"))
	t.ps.Release()
	t.ps.On(el, "mousedown", func(ev js.Value) {
		ev.Call("preventDefault")
		if c := Closest(ev.Get("target"), "[data-i]"); c.Truthy() {
			i, _ := strconv.Atoi(DataOf(c, "i"))
			if i < len(t.items) {
				t.add(t.items[i].tag)
				t.a.Invalidate()
			}
		}
	})
	t.a.SetPop(t, "tags", t.box, el, func() { t.open = false })
}

func (t *tagsTab) Hints() []hint {
	if t.chipSel >= 0 {
		return []hint{{"←→", "Select chip", ""}, {"Del", "Remove", "Delete"}, {"Esc", "Back to input", "Escape"}}
	}
	return []hint{{"type", "Filter", ""}, {"↑↓", "Choose", ""}, {"Enter", "Add", "Enter"}, {"⌫", "Remove last", "Backspace"}, {"←", "Select chip", "ArrowLeft"}}
}
