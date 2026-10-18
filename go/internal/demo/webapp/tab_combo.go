//go:build js && wasm

package webapp

// Tab 5 — Combobox: Owner and Reviewer, type-ahead over People with core's
// fuzzy ranking (protocol §7); matched letters are highlighted.

import (
	"strconv"
	"syscall/js"

	"git.wharf.fluidgrid.org/jcz/tictactoe/go/internal/demo/core"
)

type combo struct {
	key     string
	in, msg js.Value
	ci      int
	editing bool // the text is a query, not the value
	open    bool
	sel     int
	ranked  []core.Ranked
	popSig  string
	ps      Scope
}

type comboTab struct {
	tabBase
	boxes []*combo
}

func newComboTab(b tabBase) Tab {
	t := &comboTab{tabBase: b}
	t.build = t.mount
	return t
}

func (t *comboTab) mount(area js.Value) {
	form := E("div", "form2")
	for _, s := range []struct{ key, label, name string }{{"combo.owner", "Owner", "owner"}, {"combo.reviewer", "Reviewer", "reviewer"}} {
		row, c, msg := field(s.label)
		cb := &combo{key: s.key, msg: msg}
		cb.in = E("input", "ed2 combo")
		Attr(cb.in, "spellcheck", "false")
		Attr(cb.in, "autocomplete", "off")
		Attr(cb.in, "placeholder", "— none —")
		Attr(cb.in, "role", "combobox")
		cb.ci = t.addCtl(cb.in, s.name, s.key)
		xb := ET("button", "btn sm ghost", "✕")
		Attr(xb, "type", "button")
		Attr(xb, "tabindex", "-1")
		Attr(xb, "title", "clear (Del)")
		Data(xb, "control", s.name+"-clear")
		Add(c, cb.in, xb)
		Add(form, row)
		t.boxes = append(t.boxes, cb)
		t.scope.On(cb.in, "input", func(js.Value) {
			cb.editing, cb.open, cb.sel = true, true, 0
			t.a.Invalidate()
		})
		t.scope.On(cb.in, "focus", func(js.Value) {
			if !cb.editing {
				cb.in.Call("select") // typing replaces the name: a fresh search
			}
		})
		t.scope.On(cb.in, "mousedown", func(js.Value) {
			if !cb.open {
				t.openList(cb)
			}
		})
		t.scope.On(cb.in, "blur", func(js.Value) {
			After(120, func() {
				if !cb.in.Equal(doc.Get("activeElement")) {
					t.closeList(cb, true)
				}
			})
		})
		t.scope.On(xb, "mousedown", func(ev js.Value) {
			ev.Call("preventDefault")
			t.Focus(cb.ci)
			t.pick(cb, "")
		})
	}
	Add(area, form)
}

func (t *comboTab) display(cb *combo) string {
	u := t.a.v.OptString(cb.key)
	if u == nil {
		return ""
	}
	if p, ok := core.PersonByUsername(*u); ok {
		return p.Name
	}
	return *u
}

func (t *comboTab) openList(cb *combo) {
	cb.open, cb.sel = true, 0
	if !cb.editing {
		// start from the whole list, the current person selected
		u := t.a.v.OptString(cb.key)
		for i, p := range core.People {
			if u != nil && p.Username == *u {
				cb.sel = i
			}
		}
	}
	cb.popSig = ""
	t.a.Invalidate()
}

func (t *comboTab) closeList(cb *combo, revert bool) {
	cb.open = false
	t.a.ClearPop(cb)
	if revert && cb.editing {
		cb.editing = false
		cb.in.Set("value", t.display(cb))
	}
	t.a.Invalidate()
}

// pick commits a username ("" = null).
func (t *comboTab) pick(cb *combo, username string) {
	var v any
	if username != "" {
		v = username
	}
	cb.editing = false
	t.closeList(cb, false)
	if err := t.a.Commit(cb.key, v); err == nil {
		cb.in.Set("value", t.display(cb))
	}
}

func (t *comboTab) query(cb *combo) string {
	if !cb.editing {
		return ""
	}
	return cb.in.Get("value").String()
}

func (t *comboTab) curBox() *combo {
	for _, cb := range t.boxes {
		if cb.ci == t.cur {
			return cb
		}
	}
	return nil
}

func (t *comboTab) Key(k Key) bool {
	cb := t.curBox()
	if cb == nil {
		return false
	}
	n := len(cb.ranked)
	switch {
	case k.Is("ArrowDown"):
		if !cb.open {
			t.openList(cb)
		} else if n > 0 {
			cb.sel = (cb.sel + 1) % n
		}
	case k.Is("ArrowUp") && cb.open && n > 0:
		cb.sel = (cb.sel - 1 + n) % n
	case k.Is("Enter"):
		if cb.open && n > 0 {
			t.pick(cb, core.People[cb.ranked[cb.sel].Index].Username)
		} else if !cb.open {
			t.openList(cb)
		}
	case k.Is("Escape") && (cb.open || cb.editing):
		t.closeList(cb, true)
	case k.Is("Delete") && !cb.editing:
		t.pick(cb, "")
	case k.Is("Tab", "S-Tab"):
		t.closeList(cb, true)
		return false
	default:
		return false
	}
	t.a.Invalidate()
	return true
}

func (t *comboTab) Synced(key string, ev syncEv) {}

func (t *comboTab) Render() {
	for _, cb := range t.boxes {
		if !cb.editing && cb.in.Get("value").String() != t.display(cb) {
			cb.in.Set("value", t.display(cb))
		}
		if cb.editing {
			setMsg(cb.msg, "info", "Enter picks · Esc reverts")
		} else if u := t.a.v.OptString(cb.key); u != nil {
			if p, ok := core.PersonByUsername(*u); ok {
				setMsg(cb.msg, "", "@"+p.Username+" · "+p.Team)
			}
		} else {
			setMsg(cb.msg, "", "— none —")
		}
		cb.ranked = core.RankPeople(t.query(cb))
		if cb.sel >= len(cb.ranked) {
			cb.sel = 0
		}
		if !cb.open {
			continue
		}
		sig := t.query(cb) + "|" + strconv.Itoa(cb.sel)
		if sig == cb.popSig && t.a.PopOwner(cb) {
			continue
		}
		cb.popSig = sig
		t.a.SetPop(cb, "combobox", cb.in, t.buildList(cb), func() { cb.open = false })
	}
}

func (t *comboTab) buildList(cb *combo) js.Value {
	el := E("div", "choices")
	cur := t.a.v.OptString(cb.key)
	for i, r := range cb.ranked {
		p := core.People[r.Index]
		cls := "choice"
		if i == cb.sel {
			cls += " sel"
		}
		c := E("div", cls)
		Data(c, "i", strconv.Itoa(i))
		// positions index "name username team": split them per part
		nameLen := len([]rune(p.Name))
		userLen := len([]rune(p.Username))
		var np, up, tp []int
		for _, pos := range r.Match.Positions {
			switch {
			case pos < nameLen:
				np = append(np, pos)
			case pos > nameLen && pos <= nameLen+userLen:
				up = append(up, pos-nameLen-1)
			case pos > nameLen+userLen+1:
				tp = append(tp, pos-nameLen-userLen-2)
			}
		}
		mark := " "
		if cur != nil && *cur == p.Username {
			mark = "✓"
		}
		av := ET("span", "av", core.Initials(p.Name))
		Style(av, "background", core.PeerColor(uint64(r.Index+1)))
		lbl := E("span", "lbl")
		highlight(lbl, p.Name, np)
		user := E("span", "det")
		highlight(user, p.Username, up)
		team := E("span", "det")
		highlight(team, p.Team, tp)
		Add(c, ET("span", "chk", mark), av, lbl, user, team)
		Add(el, c)
	}
	if len(cb.ranked) == 0 {
		Add(el, ET("div", "choice empty", "nobody matches"))
	}
	Add(el, ET("div", "pophint", "↑↓ move · Enter pick · Esc close · Del clears"))
	cb.ps.Release()
	cb.ps.On(el, "mousedown", func(ev js.Value) {
		ev.Call("preventDefault")
		if c := Closest(ev.Get("target"), "[data-i]"); c.Truthy() {
			i, _ := strconv.Atoi(DataOf(c, "i"))
			if i < len(cb.ranked) {
				t.pick(cb, core.People[cb.ranked[i].Index].Username)
			}
		}
	})
	return el
}

func (t *comboTab) Hints() []hint {
	cb := t.curBox()
	if cb != nil && cb.open {
		return []hint{{"type", "Search", ""}, {"↑↓", "Move", ""}, {"Enter", "Pick", "Enter"}, {"Esc", "Close", "Escape"}}
	}
	return []hint{{"type", "Search", ""}, {"↓", "Open list", "ArrowDown"}, {"Del", "Clear", "Delete"}, {"Tab", "Next", "Tab"}}
}
