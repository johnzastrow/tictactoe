//go:build js && wasm

package webapp

import (
	"encoding/json"
	"fmt"
	"strconv"
	"strings"
	"syscall/js"
	"unicode/utf8"

	"git.wharf.fluidgrid.org/jcz/tictactoe/go/internal/pt/core"
)

var viewNames = []string{"Projects", "Tasks", "Board", "Timeline", "Dashboard", "Activity", "Templates", "Fields", "Users"}

// ---- login ----------------------------------------------------------------------

var loginScope Scope

func (a *App) showLogin(msg string) {
	loginScope.Release()
	Clear(a.root)
	wrap := E("div", "login-wrap")
	form := E("form", "login")
	Attr(form, "autocomplete", "on")
	brand := ET("div", "brand", "pt ")
	Add(brand, ET("span", "", "projects & tasks · Go/WASM"))
	user := E("input", "")
	Attr(user, "name", "username")
	Attr(user, "autocomplete", "username")
	Attr(user, "id", "login-user")
	pass := E("input", "")
	Attr(pass, "type", "password")
	Attr(pass, "name", "password")
	Attr(pass, "autocomplete", "current-password")
	Attr(pass, "id", "login-pass")
	errEl := ET("div", "lerr", msg)
	if strings.HasPrefix(msg, "Signed out") {
		errEl.Set("className", "lfoot")
	}
	btn := ET("button", "btn primary", "Sign in")
	Attr(btn, "type", "submit")
	lu := ET("label", "", "Username")
	Add(lu, user)
	lp := ET("label", "", "Password")
	Add(lp, pass)
	Add(form, brand, lu, lp, errEl, btn, ET("div", "lfoot", "Keyboard first: Tab between fields, Enter to sign in."))
	Add(wrap, form)
	Add(a.root, wrap)
	user.Call("focus")
	loginScope.On(form, "submit", func(ev js.Value) {
		ev.Call("preventDefault")
		btn.Set("disabled", true)
		errEl.Set("className", "lerr")
		errEl.Set("textContent", "")
		body, _ := json.Marshal(core.LoginRequest{
			Username: user.Get("value").String(), Password: pass.Get("value").String(), Client: "web-go",
		})
		fetchText("POST", "/api/login", string(body), func(status int, text string) {
			btn.Set("disabled", false)
			switch status {
			case 200:
				loginScope.Release()
				Clear(a.root)
				a.start()
			case 401:
				errEl.Set("textContent", "Wrong username or password.")
				pass.Set("value", "")
				pass.Call("focus")
			case 429:
				errEl.Set("textContent", "Too many attempts. Wait a minute and try again.")
			case 0:
				errEl.Set("textContent", "Can't reach the server.")
			default:
				errEl.Set("textContent", fmt.Sprintf("Sign-in failed (%d).", status))
			}
		})
	})
}

// ---- shell skeleton -------------------------------------------------------------------

func (a *App) buildShell() {
	a.shellScope.Release()
	Clear(a.root)
	a.shell = E("div", "shell")
	a.menubar = E("div", "menubar")
	a.tabs = E("div", "tabs")
	a.qbar = E("div", "querybar")
	a.offline = E("div", "offline hidden")
	a.viewport = E("div", "viewport")
	a.statusEl = E("div", "statusline")
	a.hints = E("div", "hintbar")
	a.dialogLayer = E("div", "dialog-layer")
	a.toastLayer = E("div", "toasts")
	a.popLayer = E("div", "pop-layer")
	Add(a.shell, a.menubar, a.tabs, a.qbar, a.offline, a.viewport, a.statusEl, a.hints)
	Add(a.root, a.shell, a.dialogLayer, a.toastLayer, a.popLayer)

	// query bar: input over a backdrop that carries the error underlines
	lbl := ET("span", "qlabel", "Filter ▸")
	wrap := E("div", "qwrap")
	a.qback = E("div", "qback")
	a.qin = E("input", "qin")
	Attr(a.qin, "id", "query")
	Attr(a.qin, "spellcheck", "false")
	Attr(a.qin, "autocomplete", "off")
	Attr(a.qin, "placeholder", "type to filter — status:active @me due<+14d #tag is:overdue  (^F)")
	a.qsugg = E("div", "qsugg hidden")
	Add(wrap, a.qback, a.qin, a.qsugg)
	a.qmsg = E("span", "qmsg")
	a.qind = E("span", "qind hidden")
	a.qcount = E("span", "qcount")
	Add(a.qbar, lbl, wrap, a.qmsg, a.qind, a.qcount)
	a.shellScope.On(a.qin, "input", func(js.Value) {
		a.curView().SetQuery(a.qin.Get("value").String())
		a.qSuggestOn = true
		a.qsel = -1
		a.Invalidate()
	})
	a.shellScope.On(a.qin, "focus", func(js.Value) { a.qSuggestOn = true; a.Invalidate() })
	a.shellScope.On(a.qin, "blur", func(js.Value) {
		After(150, func() { a.qSuggestOn = false; a.Invalidate() })
	})
	a.shellScope.On(a.qin, "scroll", func(js.Value) { a.qback.Set("scrollLeft", a.qin.Get("scrollLeft")) })
	a.shellScope.On(a.qsugg, "mousedown", func(ev js.Value) {
		ev.Call("preventDefault")
		if el := Closest(ev.Get("target"), "[data-i]"); el.Truthy() {
			i, _ := strconv.Atoi(DataOf(el, "i"))
			a.acceptSuggestion(i)
		}
	})

	// global listeners (document level, released on teardown)
	a.shellScope.On(doc, "keydown", a.onKeyDown)
	a.shellScope.On(doc, "paste", a.onPaste)
	a.shellScope.On(doc, "mousedown", func(ev js.Value) {
		t := ev.Get("target")
		if a.menuOpen >= 0 && !Closest(t, ".menubar").Truthy() {
			a.menuOpen = -1
			a.Invalidate()
		}
		if a.ctx != nil && !Closest(t, ".ctxmenu").Truthy() {
			a.ctx = nil
			a.Invalidate()
		}
	})
	a.shellScope.On(global, "resize", func(js.Value) {
		for _, v := range a.views {
			v.DataChanged(false)
		}
		a.Invalidate()
	})
	// delegated clicks for menubar, tabs, hints, toasts
	a.shellScope.On(a.menubar, "mousedown", func(ev js.Value) {
		t := ev.Get("target")
		if it := Closest(t, "[data-item]"); it.Truthy() {
			ev.Call("preventDefault")
			if _, _, _, run := a.menuItem(DataOf(it, "item")); run != nil {
				a.menuOpen = -1
				run()
			}
		} else if m := Closest(t, "[data-menu]"); m.Truthy() {
			ev.Call("preventDefault")
			i, _ := strconv.Atoi(DataOf(m, "menu"))
			if a.menuOpen == i {
				a.menuOpen = -1
			} else {
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
	a.shellScope.On(a.tabs, "click", func(ev js.Value) {
		if t := Closest(ev.Get("target"), "[data-view]"); t.Truthy() {
			i, _ := strconv.Atoi(DataOf(t, "view"))
			a.SwitchView(i)
		}
	})
	a.shellScope.On(a.hints, "click", func(ev js.Value) {
		if t := Closest(ev.Get("target"), "[data-act]"); t.Truthy() {
			act := DataOf(t, "act")
			if strings.HasPrefix(act, "key:") {
				a.handleKey(Key{Name: strings.TrimPrefix(act, "key:")})
			} else {
				a.run(act)
			}
			a.curView().FocusDOM()
			a.Invalidate()
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
	a.shellScope.On(a.statusEl, "click", func(ev js.Value) {
		if t := Closest(ev.Get("target"), "[data-act]"); t.Truthy() {
			a.run(DataOf(t, "act"))
		}
	})
	a.lastSig = map[string]string{}
}

// ---- menubar ------------------------------------------------------------------------------

func (a *App) renderMenubar() {
	ms := a.menus()
	srv := ""
	if a.r.Server != nil {
		srv = a.r.Server.Impl + " " + a.r.Server.Version
	}
	me := a.r.Me
	sig := fmt.Sprintf("%d|%d|%d|%s|%s|%v|%d|%s|%d", a.menuOpen, a.menuSel, a.conn, srv, a.theme, me != nil, len(a.savedQ), a.density, a.view)
	if me != nil {
		sig += me.Username + string(me.Role)
	}
	if !a.sigChanged("menubar", sig) {
		return
	}
	Clear(a.menubar)
	Add(a.menubar, ET("span", "logo", "pt"))
	for i, m := range ms {
		el := E("span", "menu")
		if i == a.menuOpen {
			el.Set("className", "menu open")
		}
		Data(el, "menu", strconv.Itoa(i))
		Add(el, ET("u", "", m.Title[:1]), Txt(m.Title[1:]))
		if i == a.menuOpen {
			drop := E("div", "drop")
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
	if me != nil {
		who := E("span", "who")
		av := ET("span", "av", me.Initials())
		Style(av, "background", userColor(me.ID))
		Add(who, av, ET("span", "", me.DisplayName), ET("span", "chip role-"+string(me.Role), me.Role.Label()))
		Add(a.menubar, who)
	}
	dot := "dot-wait"
	txt := "connecting…"
	switch a.conn {
	case connOn:
		dot, txt = "dot-on", srv
	case connWaiting:
		dot, txt = "dot-wait", "reconnecting…"
	}
	conn := E("span", "conn")
	Attr(conn, "title", "server implementation and version")
	Add(conn, E("span", "cdot "+dot), Txt(txt))
	Add(a.menubar, conn)
}

// ---- tabs ------------------------------------------------------------------------------------

func (a *App) renderTabs() {
	over := 0
	for _, t := range a.r.Tasks {
		if a.r.TaskOverdue(t) {
			over++
		}
	}
	if !a.sigChanged("tabs", fmt.Sprintf("%d|%d", a.view, over)) {
		return
	}
	Clear(a.tabs)
	for i, n := range viewNames {
		cls := "tab"
		if i == a.view {
			cls += " active"
		}
		t := E("div", cls)
		Data(t, "view", strconv.Itoa(i))
		Attr(t, "title", "Alt+"+strconv.Itoa(i+1))
		Add(t, ET("span", "tn", strconv.Itoa(i+1)+" "), Txt(n))
		if i == 1 && over > 0 {
			Add(t, ET("span", "tb", strconv.Itoa(over)))
		}
		Add(a.tabs, t)
	}
}

// ---- query bar ---------------------------------------------------------------------------------

type suggestion struct {
	label, detail, insert string
}

func (a *App) queryFocused() bool {
	return a.qin.Truthy() && a.qin.Equal(doc.Get("activeElement"))
}

func (a *App) focusQuery() {
	a.qin.Call("focus")
	a.qin.Call("select")
	a.qSuggestOn = true
}

func (a *App) syncQueryInput() {
	if a.qin.Truthy() && a.curView() != nil {
		a.qin.Set("value", a.curView().Query())
	}
}

func (a *App) queryKey(k Key) bool {
	open := a.qSuggestOn && len(a.qSuggest) > 0
	switch {
	case k.Is("ArrowDown") && open:
		a.qsel = (a.qsel + 1) % len(a.qSuggest)
	case k.Is("ArrowUp") && open:
		a.qsel = (a.qsel - 1 + len(a.qSuggest)) % len(a.qSuggest)
	case k.Is("Tab") && open:
		i := a.qsel
		if i < 0 {
			i = 0
		}
		a.acceptSuggestion(i)
	case k.Is("Enter") && open && a.qsel >= 0:
		a.acceptSuggestion(a.qsel)
	case k.Is("Enter", "ArrowDown"):
		a.qSuggestOn = false
		a.curView().FocusDOM()
	case k.Is("Escape"):
		if open {
			a.qSuggestOn = false
		} else {
			a.curView().FocusDOM()
		}
	default:
		return false
	}
	return true
}

// currentToken: the token under the caret (start rune offset, text).
func (a *App) currentToken() (int, int, string) {
	v := a.qin.Get("value").String()
	caret := a.qin.Get("selectionStart").Int() // UTF-16 units; queries are ASCII in practice
	if caret > len(v) {
		caret = len(v)
	}
	start := strings.LastIndexAny(v[:caret], " ") + 1
	end := caret
	for end < len(v) && v[end] != ' ' {
		end++
	}
	return start, end, v[start:end]
}

func (a *App) buildSuggestions() []suggestion {
	tgt, ok := a.curView().Target()
	if !ok || !a.queryFocused() {
		return nil
	}
	_, _, tok := a.currentToken()
	neg := ""
	if strings.HasPrefix(tok, "-") {
		neg, tok = "-", tok[1:]
	}
	lt := strings.ToLower(tok)
	var out []suggestion
	add := func(label, detail, insert string) {
		if len(out) < 12 {
			out = append(out, suggestion{label, detail, neg + insert})
		}
	}
	switch {
	case strings.HasPrefix(tok, "@"):
		p := strings.TrimPrefix(lt, "@")
		if strings.HasPrefix("me", p) {
			add("@me", "you", "@me ")
		}
		for _, id := range core.SortedIDs(a.r.Users) {
			u := a.r.Users[id]
			if strings.HasPrefix(u.Username, p) || strings.Contains(strings.ToLower(u.DisplayName), p) {
				add("@"+u.Username, u.DisplayName, "@"+u.Username+" ")
			}
		}
	case strings.HasPrefix(tok, "#"):
		p := strings.TrimPrefix(lt, "#")
		e := core.EntTask
		if tgt == core.TargetProjects {
			e = core.EntProject
		}
		for _, tc := range a.r.TagCounts(e) {
			if strings.HasPrefix(tc.Tag, p) {
				add("#"+tc.Tag, fmt.Sprintf("%d×", tc.Count), "#"+tc.Tag+" ")
			}
		}
	case strings.ContainsAny(tok, ":<>="):
		i := strings.IndexAny(tok, ":<>=")
		field, val := strings.ToLower(tok[:i]), strings.ToLower(tok[i+1:])
		op := tok[i : i+1]
		if i+1 < len(tok) && tok[i+1] == '=' {
			op = tok[i : i+2]
			val = strings.ToLower(tok[i+2:])
		}
		// complete the last comma-separated value
		prefix := ""
		if c := strings.LastIndex(val, ","); c >= 0 {
			prefix, val = val[:c+1], val[c+1:]
		}
		for _, v := range a.queryValues(tgt, field) {
			if strings.HasPrefix(strings.ToLower(v[0]), val) {
				add(field+op+prefix+v[0], v[1], field+op+prefix+v[0]+" ")
			}
		}
	default:
		for _, f := range core.FieldNames(tgt) {
			if strings.HasPrefix(f, lt) {
				add(f+":", "field", f+":")
			}
		}
		if tgt != core.TargetActivity {
			e := core.CfTask
			if tgt == core.TargetProjects {
				e = core.CfProject
			}
			for _, cf := range a.r.CustomFieldsFor(e) {
				if q := cf.QueryName(); strings.HasPrefix(q, lt) {
					add(q+":", "custom field", q+":")
				}
			}
		}
	}
	if len(out) == 1 && strings.TrimSpace(out[0].insert) == neg+tok {
		return nil
	}
	return out
}

// queryValues: completion values for field: [value, detail].
func (a *App) queryValues(tgt core.Target, field string) [][2]string {
	var out [][2]string
	add := func(v, d string) { out = append(out, [2]string{v, d}) }
	switch field {
	case "is":
		for _, v := range core.IsValues(tgt) {
			add(v, "")
		}
	case "has":
		for _, v := range core.HasValues(tgt) {
			add(v, "")
		}
	case "status":
		if tgt == core.TargetProjects {
			for _, s := range core.AllProjectStatuses {
				add(string(s), s.Label())
			}
		} else {
			for _, s := range core.AllTaskStatuses {
				add(string(s), s.Label())
			}
		}
	case "priority":
		for _, s := range core.AllPriorities {
			add(string(s), s.Label())
		}
	case "health":
		for _, s := range core.AllHealth {
			add(string(s), s.Label())
		}
	case "owner", "assignee", "who", "user", "by":
		add("me", "you")
		add("none", "unassigned")
		for _, id := range core.SortedIDs(a.r.Users) {
			u := a.r.Users[id]
			add(u.Username, u.DisplayName)
		}
	case "program":
		for _, id := range core.SortedIDs(a.r.Programs) {
			p := a.r.Programs[id]
			add(strings.ToLower(p.Prefix), p.Name)
		}
	case "project":
		for _, id := range core.SortedIDs(a.r.Projects) {
			p := a.r.Projects[id]
			add(strings.ToLower(p.Code), p.Name)
		}
	case "tag", "tags":
		e := core.EntTask
		if tgt == core.TargetProjects {
			e = core.EntProject
		}
		for _, tc := range a.r.TagCounts(e) {
			add(tc.Tag, fmt.Sprintf("%d×", tc.Count))
		}
	case "due", "start":
		for _, v := range []string{"today", "+7d", "+14d", "eow", "eom", "-7d", "none"} {
			add(v, "")
		}
	case "action":
		for _, v := range []string{"set", "create", "delete"} {
			add(v, "")
		}
	default:
		e := core.CfTask
		if tgt == core.TargetProjects {
			e = core.CfProject
		}
		for _, cf := range a.r.CustomFieldsFor(e) {
			if cf.QueryName() == field {
				for _, o := range cf.Options {
					add(strings.ToLower(o), cf.Name)
				}
			}
		}
	}
	return out
}

func (a *App) acceptSuggestion(i int) {
	if i < 0 || i >= len(a.qSuggest) {
		return
	}
	start, end, _ := a.currentToken()
	v := a.qin.Get("value").String()
	ins := a.qSuggest[i].insert
	nv := v[:start] + ins + strings.TrimLeft(v[end:], " ")
	a.qin.Set("value", nv)
	caret := start + len(ins)
	a.qin.Call("setSelectionRange", caret, caret)
	a.curView().SetQuery(nv)
	a.qsel = -1
	a.qin.Call("focus")
	a.Invalidate()
}

func (a *App) renderQueryBar() {
	v := a.curView()
	tgt, has := v.Target()
	q := v.Query()
	errs := v.QueryErrors()
	shown, total := v.Count()
	ind := v.Indicators()
	if has {
		a.qbar.Get("classList").Call("remove", "hidden")
	} else {
		a.qbar.Get("classList").Call("add", "hidden")
	}
	_ = tgt
	if !a.queryFocused() && a.qin.Get("value").String() != q {
		a.qin.Set("value", q)
	}
	// suggestions
	if a.qSuggestOn {
		a.qSuggest = a.buildSuggestions()
	} else {
		a.qSuggest = nil
	}
	if a.qsel >= len(a.qSuggest) {
		a.qsel = -1
	}
	sugSig := fmt.Sprint(a.qsel, len(a.qSuggest))
	for _, s := range a.qSuggest {
		sugSig += s.label
	}
	if a.sigChanged("qsugg", sugSig) {
		Clear(a.qsugg)
		if len(a.qSuggest) == 0 {
			a.qsugg.Set("className", "qsugg hidden")
		} else {
			a.qsugg.Set("className", "qsugg choices")
			for i, s := range a.qSuggest {
				cls := "choice"
				if i == a.qsel {
					cls += " sel"
				}
				c := E("div", cls)
				Data(c, "i", strconv.Itoa(i))
				Add(c, ET("span", "lbl", s.label), ET("span", "det", s.detail))
				Add(a.qsugg, c)
			}
			Add(a.qsugg, ET("div", "choice empty", "Tab accepts · ↑↓ choose · Enter applies"))
		}
	}
	errSig := q
	for _, e := range errs {
		errSig += fmt.Sprint(e.Start, e.End, e.Message)
	}
	if a.sigChanged("qerr", errSig) {
		// rebuild the backdrop: same text, error spans underlined
		Clear(a.qback)
		runes := []rune(q)
		pos := 0
		for _, e := range errs {
			if e.Start > pos && e.Start <= len(runes) {
				Add(a.qback, Txt(string(runes[pos:e.Start])))
			}
			s, en := max(e.Start, pos), min(e.End, len(runes))
			if en > s {
				Add(a.qback, ET("span", "qerr", string(runes[s:en])))
			}
			pos = max(pos, en)
		}
		if pos < len(runes) {
			Add(a.qback, Txt(string(runes[pos:])))
		}
		msg := ""
		if len(errs) > 0 {
			msg = "⚠ " + errs[0].Message
			if len(errs) > 1 {
				msg += fmt.Sprintf(" (+%d)", len(errs)-1)
			}
		}
		a.qmsg.Set("textContent", msg)
	}
	if a.sigChanged("qcount", fmt.Sprint(shown, total, ind)) {
		a.qcount.Set("textContent", fmt.Sprintf("[ %d / %d ]", shown, total))
		if ind == "" {
			a.qind.Set("className", "qind hidden")
		} else {
			a.qind.Set("className", "qind")
			a.qind.Set("textContent", ind)
		}
	}
	_ = utf8.RuneLen
}

// ---- status line ---------------------------------------------------------------------------------

func (a *App) renderStatus() {
	online := a.r.OnlineUsers()
	sig := fmt.Sprintf("%s|%s|%d|%d|%d", a.status.kind, a.status.text, len(a.pending), len(a.conflicts), a.conn)
	for _, u := range online {
		sig += "," + u.Username
	}
	if !a.sigChanged("status", sig) {
		return
	}
	Clear(a.statusEl)
	Add(a.statusEl, ET("span", "smsg "+a.status.kind, a.status.text))
	if n := len(a.conflicts); n > 0 {
		Add(a.statusEl, ET("span", "sconf", fmt.Sprintf("⚠ %d conflict", n)))
	}
	Add(a.statusEl, E("span", "spacer"))
	avs := E("span", "avs")
	for _, u := range online {
		av := ET("span", "av", u.Initials())
		Style(av, "background", userColor(u.ID))
		Attr(av, "title", u.DisplayName+" (@"+u.Username+") is online")
		Add(avs, av)
	}
	Add(a.statusEl, avs, ET("span", "muted", fmt.Sprintf("online: %d", len(online))))
	pc := "pend"
	if len(a.pending) > 0 {
		pc += " busy"
	}
	Add(a.statusEl, ET("span", pc, fmt.Sprintf("⟳ %d pending", len(a.pending))))
	if a.conn != connOn {
		a.offline.Set("className", "offline")
		a.offline.Set("textContent", "Connection lost — reconnecting. Edits are paused until it's back.")
	} else {
		a.offline.Set("className", "offline hidden")
	}
}

// ---- hint bar --------------------------------------------------------------------------------------

type hint struct {
	key, label, act string
}

func (a *App) renderHints() {
	var hs []hint
	switch {
	case a.dialog != nil:
		hs = a.dialog.Hints()
	case a.menuOpen >= 0:
		hs = []hint{{"←→", "Menus", ""}, {"↑↓", "Items", ""}, {"Enter", "Run", ""}, {"Esc", "Close", ""}}
	case a.queryFocused():
		hs = []hint{{"Tab", "Accept suggestion", ""}, {"↑↓", "Choose", ""}, {"Enter", "Apply & go to grid", ""}, {"Esc", "Back", ""},
			{"@", "Person", ""}, {"#", "Tag", ""}, {"-", "Negate", ""}, {"F1", "Help", "help"}}
	default:
		hs = a.curView().Hints()
		hs = append(hs, hint{"^K", "Commands", "palette"}, hint{"^F", "Filter", "filter"}, hint{"^Z", "Undo", "undo"},
			hint{"F1", "Help", "help"}, hint{"F10", "Menu", "menu"})
	}
	sig := ""
	for _, h := range hs {
		sig += h.key + h.label + "|"
	}
	if !a.sigChanged("hints", sig) {
		return
	}
	Clear(a.hints)
	row := E("div", "hrow")
	for _, h := range hs {
		cls := "hint"
		if h.act != "" {
			cls += " click"
		}
		el := E("span", cls)
		if h.act != "" {
			Data(el, "act", h.act)
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
