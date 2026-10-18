//go:build js && wasm

package webapp

// The shell (ui-spec §1): menu bar, sidebar, tab bodies with their Keys and
// Value panels, status line, hint bar, toasts, and the one popup slot.

import (
	"fmt"
	"strconv"
	"strings"
	"syscall/js"
	"time"

	"git.wharf.fluidgrid.org/jcz/tictactoe/go/internal/demo/core"
)

// ---- skeleton ---------------------------------------------------------------------

func (a *App) buildShell() {
	a.shellScope.Release()
	Clear(a.root)
	a.shell = E("div", "shell")
	a.menubar = E("div", "menubar")
	mid := E("div", "middle")
	a.sidebar = E("nav", "sidebar")
	Attr(a.sidebar, "tabindex", "0")
	Attr(a.sidebar, "aria-label", "tabs")
	a.main = E("main", "tabmain")
	a.statusEl = E("div", "statusline")
	a.hints = E("div", "hintbar")
	a.dialogLayer = E("div", "dialog-layer")
	a.toastLayer = E("div", "toasts")
	a.popLayer = E("div", "pop-layer")
	Add(mid, a.sidebar, a.main)
	Add(a.shell, a.menubar, mid, a.statusEl, a.hints)
	Add(a.root, a.shell, a.dialogLayer, a.toastLayer, a.popLayer)

	// one section per tab, built once: state (drafts, cursors) survives tab switches
	a.tabs = makeTabs(a)
	for _, t := range a.tabs {
		info := t.Info()
		sec := E("section", "tabbody hidden")
		Data(sec, "tab", info.ID)
		head := E("div", "thead")
		Add(head, ET("h2", "ttitle", fmt.Sprintf("%d  %s", info.Num, info.Title)), ET("p", "texplain", tabTexts[info.ID].Explain))
		area := E("div", "ctlarea")
		panels := E("div", "panels")
		keys := E("div", "panel keyspanel")
		Add(keys, ET("div", "phead", "Keys"))
		kl := E("div", "klist")
		for _, kv := range tabTexts[info.ID].Keys {
			Add(kl, ET("kbd", "", kv[0]), ET("span", "", kv[1]))
		}
		Add(keys, kl)
		val := E("div", "panel valpanel")
		Add(val, ET("div", "phead", "Value (shared)"))
		vl := E("div", "vlist")
		Add(val, vl)
		Add(panels, keys, val)
		Add(sec, head, area, panels)
		Add(a.main, sec)
		t.Mount(sec, area, vl)
	}

	// global listeners (document level, released on Leave)
	a.shellScope.On(doc, "keydown", a.onKeyDown)
	a.shellScope.On(doc, "focusin", a.onFocusIn)
	a.shellScope.On(doc, "mousedown", func(ev js.Value) {
		t := ev.Get("target")
		if a.menuOpen >= 0 && !Closest(t, ".menubar").Truthy() {
			a.menuOpen = -1
			a.Invalidate()
		}
		if a.pop != nil && !Closest(t, ".pop-layer").Truthy() &&
			!(a.pop.anchor.Truthy() && a.pop.anchor.Call("contains", t).Bool()) {
			a.closePop()
		}
	})
	a.shellScope.On(global, "resize", func(js.Value) { a.lastSig["pop"] = ""; a.Invalidate() })
	a.shellScope.On(a.main, "scroll", func(js.Value) { a.placePop() })
	// delegated clicks: menu bar, sidebar, hints, toasts
	a.shellScope.On(a.menubar, "mousedown", func(ev js.Value) {
		t := ev.Get("target")
		if it := Closest(t, "[data-item]"); it.Truthy() {
			ev.Call("preventDefault")
			if _, _, _, run := a.menuItem(DataOf(it, "item")); run != nil {
				a.menuOpen = -1
				run()
				After(0, a.focusCurrent)
			}
		} else if m := Closest(t, "[data-menu]"); m.Truthy() {
			ev.Call("preventDefault")
			i, _ := strconv.Atoi(DataOf(m, "menu"))
			if a.menuOpen == i {
				a.menuOpen = -1
			} else {
				a.closePop()
				a.menuOpen, a.menuSel = i, 0
			}
		}
		a.Invalidate()
	})
	a.shellScope.On(a.menubar, "mouseover", func(ev js.Value) {
		if a.menuOpen < 0 {
			return
		}
		if m := Closest(ev.Get("target"), "[data-menu]"); m.Truthy() {
			i, _ := strconv.Atoi(DataOf(m, "menu"))
			if i != a.menuOpen {
				a.menuOpen, a.menuSel = i, 0
				a.Invalidate()
			}
		}
	})
	a.shellScope.On(a.sidebar, "click", func(ev js.Value) {
		if t := Closest(ev.Get("target"), "[data-tab-link]"); t.Truthy() {
			id := DataOf(t, "tabLink")
			for i, tb := range a.tabs {
				if tb.Info().ID == id {
					a.focusSide = true
					a.SwitchTab(i)
				}
			}
		}
	})
	a.shellScope.On(a.hints, "mousedown", func(ev js.Value) {
		// keep the focus where it is so the hint acts on the current control
		ev.Call("preventDefault")
		if t := Closest(ev.Get("target"), "[data-act]"); t.Truthy() {
			a.runHint(DataOf(t, "act"))
		}
	})
	a.shellScope.On(a.toastLayer, "click", func(ev js.Value) {
		if t := Closest(ev.Get("target"), "[data-toast]"); t.Truthy() {
			id, _ := strconv.Atoi(DataOf(t, "toast"))
			for i := range a.toasts {
				if a.toasts[i].id == id {
					a.toasts = append(a.toasts[:i], a.toasts[i+1:]...)
					break
				}
			}
			a.Invalidate()
		}
	})
}

// ---- menu bar ------------------------------------------------------------------------------

func (a *App) renderMenubar() {
	ms := a.menus()
	srv := a.v.Server.Impl + " " + a.v.Server.Version
	sig := fmt.Sprintf("%d|%d|%d|%s|%s|%s|%v%v|%d", a.menuOpen, a.menuSel, a.conn, srv, a.theme, a.name, a.showKeys, a.showValue, a.tab)
	if !a.sigChanged("menubar", sig) {
		return
	}
	Clear(a.menubar)
	Add(a.menubar, ET("span", "logo", "demo"))
	for i, m := range ms {
		el := E("span", "menu")
		if i == a.menuOpen {
			el.Set("className", "menu open")
		}
		Data(el, "menu", strconv.Itoa(i))
		Data(el, "control", "menu-"+strings.ToLower(m.Title))
		Add(el, ET("u", "", m.Title[:1]), Txt(m.Title[1:]))
		if i == a.menuOpen {
			drop := E("div", "drop")
			Data(drop, "popup", "menu")
			for j, id := range m.Items {
				if id == "-" {
					Add(drop, E("div", "sep"))
					continue
				}
				label, key, checked, run := a.menuItem(id)
				cls := "item"
				if j == a.menuSel {
					cls += " sel"
				}
				if run == nil {
					cls += " disabled"
				}
				it := E("div", cls)
				Data(it, "item", id)
				chk := ""
				if checked {
					chk = "✓"
				}
				Add(it, ET("span", "chk", chk), ET("span", "il", label), ET("span", "ik", key))
				Add(drop, it)
			}
			Add(el, drop)
		}
		Add(a.menubar, el)
	}
	Add(a.menubar, E("span", "spacer"))
	dot, txt := "dot-wait", "connecting…"
	switch a.conn {
	case connOn:
		dot, txt = "dot-on", srv
	case connWaiting:
		dot, txt = "dot-wait", "reconnecting…"
	case connOffline:
		dot, txt = "dot-off", "offline"
	}
	right := E("span", "conn")
	Attr(right, "title", "server implementation and version")
	Add(right, ET("span", "brandname", "demo"), Txt(" · "), ET("span", "me", a.name), Txt(" · "), E("span", "cdot "+dot), Txt(" "+txt))
	Add(a.menubar, right)
}

// ---- sidebar ------------------------------------------------------------------------------

func (a *App) renderSidebar() {
	peers := a.v.OtherPeers()
	sig := fmt.Sprintf("%d|%v", a.tab, a.focusSide)
	for _, p := range peers {
		if p.Tab != nil {
			sig += "," + *p.Tab + p.Color
		}
	}
	if !a.sigChanged("sidebar", sig) {
		return
	}
	Clear(a.sidebar)
	for i, t := range a.tabs {
		info := t.Info()
		cls := "sitem"
		if i == a.tab {
			cls += " cur"
		}
		it := E("div", cls)
		Data(it, "tabLink", info.ID)
		mark := " "
		if i == a.tab {
			mark = "▸"
		}
		Add(it, ET("span", "smark", mark), ET("span", "snum", strconv.Itoa(info.Num)), ET("span", "sname", info.Title))
		for _, p := range peers {
			if p.Tab != nil && *p.Tab == info.ID {
				d := E("span", "pdot")
				Style(d, "background", safeColor(p.Color))
				Attr(d, "title", core.SafeText(p.Name)+" is here")
				Add(it, d)
			}
		}
		Add(a.sidebar, it)
	}
}

// safeColor accepts only #rrggbb (colours come from the server).
func safeColor(c string) string {
	if len(c) != 7 || c[0] != '#' {
		return "var(--muted)"
	}
	for _, r := range c[1:] {
		if !(r >= '0' && r <= '9' || r >= 'a' && r <= 'f' || r >= 'A' && r <= 'F') {
			return "var(--muted)"
		}
	}
	return c
}

// ---- tab body + panels -------------------------------------------------------------------

func (a *App) renderBody() {
	if a.sigChanged("tabshown", fmt.Sprintf("%d|%v|%v", a.tab, a.showKeys, a.showValue)) {
		for i, t := range a.tabs {
			cls := "tabbody"
			if i != a.tab {
				cls += " hidden"
			}
			if !a.showKeys {
				cls += " nokeys"
			}
			if !a.showValue {
				cls += " novalue"
			}
			t.Section().Set("className", cls)
		}
		a.main.Set("scrollTop", 0)
	}
	t := a.cur()
	t.Render()
	a.renderValues(t)
	a.applyFlash(t)
}

// renderValues fills the Value panel: `key = value` and "v4 · by bob · 2m ago".
func (a *App) renderValues(t Tab) {
	nowT := time.Now()
	focusKey := t.CurKey()
	var sig strings.Builder
	type line struct{ key, val, full, desc string }
	var lines []line
	for _, k := range t.Info().Keys {
		full := string(a.v.Raw(k))
		val := full
		if len([]rune(val)) > 220 {
			val = string([]rune(val)[:220]) + "…"
		}
		l := line{k, val, full, a.v.Describe(k, nowT)}
		if a.Busy(k) {
			l.desc += " · saving…"
		}
		lines = append(lines, l)
		fmt.Fprintf(&sig, "%s=%s|%s|", l.key, l.full, l.desc)
	}
	sig.WriteString(focusKey)
	if !a.sigChanged("values:"+t.Info().ID, sig.String()) {
		return
	}
	vl := t.ValueList()
	Clear(vl)
	for _, l := range lines {
		row := E("div", "vrow")
		if l.key == focusKey && len(lines) > 1 {
			row.Set("className", "vrow focus")
		}
		Data(row, "key", l.key)
		v := E("div", "vline")
		vv := ET("span", "vv", core.SafeText(l.val))
		Attr(vv, "title", core.SafeText(l.full)) // the whole value (the text may be cut)
		Add(v, ET("span", "vk", l.key), ET("span", "muted", " = "), vv)
		Add(row, v, ET("div", "vdesc", l.desc))
		Add(vl, row)
	}
}

// applyFlash re-triggers the flash animation on elements tagged with a
// key that changed (Value panel rows and controls carry data-key).
func (a *App) applyFlash(t Tab) {
	for k := range a.flash {
		if a.lastSig["flash:"+k] == fmt.Sprint(a.flash[k]) {
			continue
		}
		a.lastSig["flash:"+k] = fmt.Sprint(a.flash[k])
		els := t.Section().Call("querySelectorAll", `[data-key="`+k+`"]`)
		for i := 0; i < els.Length(); i++ {
			el := els.Index(i)
			el.Get("classList").Call("remove", "flash")
			_ = el.Get("offsetWidth") // reflow so the animation restarts
			el.Get("classList").Call("add", "flash")
		}
	}
}

// ---- status line ---------------------------------------------------------------------------------

func (a *App) renderStatus() {
	peers := a.v.Peers
	sig := fmt.Sprintf("%s|%s|%d", a.status.kind, a.status.text, a.conn)
	for _, p := range peers {
		sig += "," + p.Name + p.Color + p.Client
		if p.Tab != nil {
			sig += *p.Tab
		}
	}
	if !a.sigChanged("status", sig) {
		return
	}
	Clear(a.statusEl)
	Add(a.statusEl, ET("span", "smsg "+a.status.kind, a.status.text))
	Add(a.statusEl, E("span", "spacer"))
	avs := E("span", "avs")
	for _, p := range peers {
		name := core.SafeText(p.Name)
		av := ET("span", "av", core.Initials(name))
		Style(av, "background", safeColor(p.Color))
		where := "no tab yet"
		if p.Tab != nil {
			if t, ok := core.TabByID(*p.Tab); ok {
				where = "on " + t.Title
			}
		}
		me := ""
		if p.Conn == a.v.You.Conn {
			me = " (you)"
			av.Set("className", "av me")
		}
		Attr(av, "title", fmt.Sprintf("%s%s · %s · %s", name, me, core.SafeText(p.Client), where))
		Add(avs, av)
	}
	Add(a.statusEl, avs, ET("span", "muted online", fmt.Sprintf("online: %d", len(peers))))
}

// ---- hint bar --------------------------------------------------------------------------------------

// hint is one `KEY Label` entry; spec (a key spec) makes it clickable.
type hint struct {
	key, label, spec string
}

func (a *App) renderHints() {
	var hs []hint
	switch {
	case a.dialog != nil:
		hs = a.dialog.Hints()
	case a.menuOpen >= 0:
		hs = []hint{{"←→", "Menus", ""}, {"↑↓", "Items", ""}, {"Enter", "Run", "Enter"}, {"Esc", "Close", "Escape"}}
	case a.focusSide:
		hs = []hint{{"↑↓", "Tab", ""}, {"Enter", "Open", "Enter"}, {"Alt+↓↑", "Next/prev tab", ""}}
	default:
		hs = a.cur().Hints()
		escTaken := false
		for _, h := range hs {
			escTaken = escTaken || h.key == "Esc"
		}
		if !escTaken {
			hs = append(hs, hint{"Esc", "Sidebar", "Escape"})
		}
	}
	if a.dialog == nil {
		hs = append(hs, hint{"^K", "Commands", "C-k"}, hint{"F1", "Help", "F1"}, hint{"F10", "Menu", "F10"}, hint{"^R", "Reset tab", "C-r"})
	}
	sig := ""
	for _, h := range hs {
		sig += h.key + h.label + h.spec + "|"
	}
	if !a.sigChanged("hints", sig) {
		return
	}
	Clear(a.hints)
	row := E("div", "hrow")
	for _, h := range hs {
		cls := "hint"
		if h.spec != "" {
			cls += " click"
		}
		el := E("span", cls)
		if h.spec != "" {
			Data(el, "act", h.spec)
		}
		Add(el, ET("kbd", "", h.key), Txt(" "+h.label))
		Add(row, el)
	}
	Add(a.hints, row)
}

// ---- toasts ---------------------------------------------------------------------------------------

func (a *App) renderToasts() {
	sig := ""
	for _, t := range a.toasts {
		sig += strconv.Itoa(t.id) + ","
	}
	if !a.sigChanged("toasts", sig) {
		return
	}
	Clear(a.toastLayer)
	for _, t := range a.toasts {
		el := E("div", "toast "+t.kind)
		Data(el, "toast", strconv.Itoa(t.id))
		Add(el, ET("b", "", t.title), Txt(" "+t.body))
		Add(a.toastLayer, el)
	}
}

// ---- popup slot -------------------------------------------------------------------------------------

// popState is the one open popup (dropdown list, combobox list, tag
// suggestions, context menu, …). Its owner rebuilds el when its content
// changes; the shell positions it under the anchor (or at a point).
type popState struct {
	owner  any
	kind   string
	anchor js.Value
	x, y   float64 // used when anchor is null (context menu at the pointer)
	el     js.Value
	close  func()
}

// SetPop shows el as owner's popup (replacing any other popup).
func (a *App) SetPop(owner any, kind string, anchor, el js.Value, close func()) {
	if a.pop != nil && a.pop.owner != owner {
		a.closePop()
	}
	Data(el, "popup", kind)
	el.Get("classList").Call("add", "popup")
	a.pop = &popState{owner: owner, kind: kind, anchor: anchor, el: el, close: close}
	a.lastSig["pop"] = ""
}

// SetPopAt is SetPop at a viewport point.
func (a *App) SetPopAt(owner any, kind string, x, y float64, el js.Value, close func()) {
	a.SetPop(owner, kind, js.Null(), el, close)
	a.pop.x, a.pop.y = x, y
}

// ClearPop removes owner's popup without calling its close callback.
func (a *App) ClearPop(owner any) {
	if a.pop != nil && a.pop.owner == owner {
		a.pop = nil
		a.lastSig["pop"] = ""
		a.Invalidate()
	}
}

// PopOwner reports whether owner's popup is the one shown.
func (a *App) PopOwner(owner any) bool { return a.pop != nil && a.pop.owner == owner }

// closePop closes the popup, telling its owner.
func (a *App) closePop() {
	if a.pop == nil {
		return
	}
	c := a.pop.close
	a.pop = nil
	a.lastSig["pop"] = ""
	if c != nil {
		c()
	}
	a.Invalidate()
}

func (a *App) renderPop() {
	if a.pop == nil {
		if a.popLayer.Get("childElementCount").Int() > 0 {
			Clear(a.popLayer)
		}
		return
	}
	first := a.popLayer.Get("firstElementChild")
	if !first.Equal(a.pop.el) {
		Replace(a.popLayer, a.pop.el)
	}
	a.placePop()
}

// placePop positions the popup below its anchor, flipping above (or
// shifting left) when it would leave the window.
func (a *App) placePop() {
	if a.pop == nil {
		return
	}
	el := a.pop.el
	vw := global.Get("innerWidth").Float()
	vh := global.Get("innerHeight").Float()
	var left, top, bottomAlt float64
	minW := 0.0
	if a.pop.anchor.Truthy() {
		r := a.pop.anchor.Call("getBoundingClientRect")
		left, top = r.Get("left").Float(), r.Get("bottom").Float()+3
		bottomAlt = r.Get("top").Float() - 3
		minW = r.Get("width").Float()
	} else {
		left, top, bottomAlt = a.pop.x, a.pop.y, a.pop.y
	}
	Style(el, "position", "fixed")
	if minW > 0 && a.pop.kind != "context-menu" && a.pop.kind != "menu" {
		Style(el, "min-width", Px(minW))
	}
	w := el.Get("offsetWidth").Float()
	h := el.Get("offsetHeight").Float()
	if top+h > vh-8 && bottomAlt-h > 8 {
		top = bottomAlt - h
	}
	if left+w > vw-8 {
		left = max(8, vw-w-8)
	}
	Style(el, "left", Px(left))
	Style(el, "top", Px(top))
}
