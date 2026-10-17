//go:build js && wasm

package webapp

// The application: one App value owns the replica, the socket, the shell
// regions and the views. Every change goes through Invalidate(), which
// schedules one render on the next animation frame.

import (
	"encoding/json"
	"fmt"
	"sort"
	"strings"
	"syscall/js"

	"git.wharf.fluidgrid.org/jcz/tictactoe/go/internal/pt/core"
)

const (
	connConnecting = iota
	connOn
	connWaiting
)

type cellKey struct {
	e     core.Entity
	id    int64
	field string
}

type conflictInfo struct {
	current any
	by      *int64
}

type pendingReq struct {
	cells   []cellKey
	label   string
	onDone  func(core.ResultMsg)
	isUndo  bool
	created func(map[string]int64)
}

type toast struct {
	kind  string // ok warn err info
	title string
	body  string
	until float64
	id    int
}

type statusMsg struct {
	kind, text string
}

type App struct {
	r    *core.Replica
	sock Socket
	conn int

	// shell regions
	root, shell, menubar, tabs, qbar, qin, qback, qmsg, qcount, qind, qsugg js.Value
	offline, viewport, statusEl, hints, dialogLayer, toastLayer, popLayer       js.Value
	shellScope, statusScope, hintScope, tabScope, menuScope                  Scope

	views   []View
	view    int
	mounted int
	theme   string
	density string

	// menus
	menuOpen int
	menuSel  int

	dialog   Dialog
	ctx      *ContextMenu
	toasts   []toast
	toastSeq int
	status   statusMsg

	reqN      uint64
	pending   map[uint64]*pendingReq
	pendCells map[cellKey]int
	conflicts map[cellKey]conflictInfo
	flash     map[core.RowKey]float64

	clipboard   string
	renderQueue bool
	renderFn    js.Func
	lastFocus   string
	focusTimer  bool
	savedQ      []savedFilter
	qsel        int
	qSuggest    []suggestion
	qSuggestOn  bool

	lastSig map[string]string
	loaded  bool
	// bumped when custom fields change: grids rebuild their columns
	colsVersion int
}

var app *App

// Run is the WASM entry point (called from main).
func Run() {
	app = &App{
		pending:   map[uint64]*pendingReq{},
		pendCells: map[cellKey]int{},
		conflicts: map[cellKey]conflictInfo{},
		flash:     map[core.RowKey]float64{},
		menuOpen:  -1,
		lastSig:   map[string]string{},
		mounted:   -1,
	}
	app.theme = storageGet("pt.theme")
	if app.theme == "" {
		app.theme = "midnight"
	}
	app.density = storageGet("pt.density")
	app.applyTheme()
	app.root = doc.Call("getElementById", "app")
	if !app.root.Truthy() {
		app.root = doc.Get("body")
	}
	app.renderFn = js.FuncOf(func(js.Value, []js.Value) any {
		app.renderQueue = false
		safe(app.render)
		return nil
	})
	fetchText("GET", "/api/me", "", func(status int, text string) {
		if status == 200 {
			var u core.User
			if json.Unmarshal([]byte(text), &u) == nil {
				app.start()
				return
			}
		}
		app.showLogin("")
	})
}

func today() core.Date {
	d := global.Get("Date").New()
	t, _ := core.FromYMD(d.Call("getFullYear").Int(), d.Call("getMonth").Int()+1, d.Call("getDate").Int())
	return t
}

func nowSecs() int64 { return int64(nowMs() / 1000) }

func (a *App) applyTheme() {
	de := doc.Get("documentElement")
	de.Call("setAttribute", "data-theme", a.theme)
	if a.density == "compact" {
		de.Call("setAttribute", "data-density", "compact")
	} else {
		de.Call("removeAttribute", "data-density")
	}
}

// ---- startup ---------------------------------------------------------------

func (a *App) start() {
	a.r = core.NewReplica(today())
	a.loadSaved()
	a.buildShell()
	a.views = makeViews(a)
	a.sock.OnOpen = func() {
		a.conn = connOn
		a.Invalidate()
	}
	a.sock.OnMessage = a.onMessage
	a.sock.OnClose = func(code int, reason string) {
		a.conn = connWaiting
		a.Invalidate()
		// a closed session means log in again
		fetchText("GET", "/api/me", "", func(status int, _ string) {
			if status == 401 {
				a.sock.Stop()
				a.teardown()
				a.showLogin("Your session ended. Please sign in again.")
				return
			}
			a.sock.Retry()
		})
	}
	a.conn = connConnecting
	a.sock.Connect()
	a.Invalidate()
	// keep "today" fresh across midnight
	var tick js.Func
	tick = js.FuncOf(func(js.Value, []js.Value) any {
		if a.r != nil {
			a.r.SetToday(today())
		}
		a.Invalidate()
		return nil
	})
	global.Call("setInterval", tick, 60000)
}

func (a *App) teardown() {
	for _, v := range a.views {
		v.Unmount()
	}
	a.views = nil
	a.mounted = -1
	a.dialog = nil
	a.ctx = nil
	a.shellScope.Release()
	Clear(a.root)
}

// ---- server messages -----------------------------------------------------------

func (a *App) onMessage(text string) {
	msg, err := core.DecodeServerMsg([]byte(text))
	if err != nil {
		a.setStatus("err", "unreadable server message: "+err.Error())
		return
	}
	switch m := msg.(type) {
	case core.ResultMsg:
		a.onResult(m)
	case core.ErrorMsg:
		a.toast("err", "Server error", m.Message)
	case core.PongMsg:
	default:
		applied := a.r.Apply(msg)
		if applied.Snapshot {
			a.colsVersion++
			first := !a.loaded
			a.loaded = true
			if first {
				After(60, func() { a.curView().FocusDOM() })
			}
			a.conflicts = map[cellKey]conflictInfo{}
			for _, v := range a.views {
				v.DataChanged(true)
			}
			a.setStatus("ok", fmt.Sprintf("Loaded %d projects, %d tasks", len(a.r.Projects), len(a.r.Tasks)))
		} else if len(applied.Upserted)+len(applied.Deleted) > 0 || len(applied.Activity) > 0 {
			for _, k := range append(append([]core.RowKey{}, applied.Upserted...), applied.Deleted...) {
				if k.Entity == core.EntCustomField {
					a.colsVersion++
					break
				}
			}
			mine := applied.By != nil && *applied.By == a.r.MeID()
			now := nowMs()
			for _, k := range applied.Upserted {
				if !mine {
					a.flash[k] = now + 1200
				}
				// a fresh row from the server clears stale conflict marks on fields that now agree
			}
			if !mine && applied.By != nil && len(applied.Activity) > 0 {
				who := a.r.Username(applied.By)
				body := applied.Activity[0].Label
				if n := len(applied.Activity); n > 1 {
					body += fmt.Sprintf(" (+%d more)", n-1)
				}
				a.toast("info", who, body)
			}
			if !mine && len(applied.Upserted) > 0 {
				After(1300, a.Invalidate)
			}
			for _, v := range a.views {
				v.DataChanged(false)
			}
		}
		if applied.Presence {
			for _, v := range a.views {
				v.PresenceChanged()
			}
		}
		if _, ok := msg.(core.WelcomeMsg); ok {
			a.sendFocus(true)
		}
	}
	a.Invalidate()
}

// ---- requests ----------------------------------------------------------------------

func (a *App) send(m any) bool {
	b, err := core.EncodeClientMsg(m)
	if err != nil {
		return false
	}
	return a.sock.Send(b)
}

// Mutate sends ops; cells are marked pending until the result arrives.
func (a *App) Mutate(ops []core.Op, label string, onDone func(core.ResultMsg)) {
	if a.r == nil || !a.r.CanEdit() {
		a.toast("warn", "Read only", "Viewers can't make changes.")
		return
	}
	if len(ops) == 0 {
		return
	}
	a.reqN++
	p := &pendingReq{label: label, onDone: onDone}
	for _, op := range ops {
		if op.Kind == "set" {
			k := cellKey{op.Entity, op.ID, op.Field}
			p.cells = append(p.cells, k)
			a.pendCells[k]++
			delete(a.conflicts, k)
		}
	}
	if !a.send(core.MutateMsg{Req: a.reqN, Ops: ops}) {
		a.clearPending(p)
		a.toast("err", "Not connected", "The change wasn't sent; try again when the connection is back.")
		return
	}
	a.pending[a.reqN] = p
	a.Invalidate()
}

// SetField is the common single-cell edit.
func (a *App) SetField(e core.Entity, id int64, field string, base, value any) {
	if core.ValuesEqual(base, value) {
		return
	}
	a.Mutate([]core.Op{core.SetOp(e, id, field, base, value)}, "", nil)
}

func (a *App) clearPending(p *pendingReq) {
	for _, k := range p.cells {
		if a.pendCells[k] <= 1 {
			delete(a.pendCells, k)
		} else {
			a.pendCells[k]--
		}
	}
}

func (a *App) Undo() {
	a.reqN++
	a.pending[a.reqN] = &pendingReq{isUndo: true, label: "Undo"}
	if !a.send(core.UndoMsg{Req: a.reqN}) {
		delete(a.pending, a.reqN)
	}
}

func (a *App) Redo() {
	a.reqN++
	a.pending[a.reqN] = &pendingReq{isUndo: true, label: "Redo"}
	if !a.send(core.RedoMsg{Req: a.reqN}) {
		delete(a.pending, a.reqN)
	}
}

func (a *App) onResult(m core.ResultMsg) {
	p := a.pending[m.Req]
	delete(a.pending, m.Req)
	if p != nil {
		a.clearPending(p)
	}
	for _, c := range m.Conflicts {
		a.conflicts[cellKey{c.Entity, c.ID, c.Field}] = conflictInfo{current: c.Current, by: c.By}
	}
	switch {
	case m.Error != nil && len(m.Conflicts) > 0:
		a.toast("warn", "Conflict", m.Error.Message)
	case m.Error != nil:
		a.toast("err", "Not saved", m.Error.Message)
		a.setStatus("err", m.Error.Message)
	case len(m.Conflicts) > 0:
		c := m.Conflicts[0]
		who := "Someone"
		if c.By != nil {
			who = a.r.UserName(c.By)
		}
		field := c.Field
		if fm := core.Field(c.Entity, c.Field); fm != nil {
			field = fm.Label
		}
		a.toast("warn", "Conflict", fmt.Sprintf("%s changed %s to %s; your edit wasn't applied.", who, field,
			core.DisplayValue(a.r, c.Entity, c.Field, c.Current)))
		a.setStatus("warn", fmt.Sprintf("⚠ %d conflict(s): Enter on the cell edits from their value", len(m.Conflicts)))
	case p != nil && p.isUndo:
		a.setStatus("ok", "✓ "+p.label+" done")
	case m.OK:
		a.setStatus("ok", "✓ Saved")
	}
	if p != nil && p.onDone != nil {
		p.onDone(m)
	}
	a.Invalidate()
}

// ---- presence -------------------------------------------------------------------------

// sendFocus reports the current cell, debounced to 150 ms.
func (a *App) sendFocus(now bool) {
	if a.r == nil || len(a.views) == 0 {
		return
	}
	if !now {
		if a.focusTimer {
			return
		}
		a.focusTimer = true
		After(150, func() {
			a.focusTimer = false
			a.sendFocus(true)
		})
		return
	}
	v := a.views[a.view]
	m := core.FocusMsg{View: core.Ptr(v.ID())}
	if e, id, field := v.Focus(); id != 0 {
		m.Entity, m.ID = &e, &id
		if field != "" {
			m.Field = &field
		}
	}
	sig := fmt.Sprintf("%v|%v|%v|%v", *m.View, m.Entity, m.ID, m.Field)
	if m.Entity != nil {
		sig = fmt.Sprintf("%s|%s|%d|%v", *m.View, *m.Entity, *m.ID, m.Field)
		if m.Field != nil {
			sig += *m.Field
		}
	}
	if sig == a.lastFocus {
		return
	}
	a.lastFocus = sig
	a.send(m)
}

// peerAt: another user focusing (entity,id) — and field when given.
func (a *App) peersOn(e core.Entity, id int64) []core.Peer {
	if a.r == nil {
		return nil
	}
	var out []core.Peer
	for _, p := range a.r.OtherPeers() {
		if p.Focus != nil && p.Focus.Entity == e && p.Focus.ID == id {
			out = append(out, p)
		}
	}
	return out
}

func userColor(id int64) string {
	return fmt.Sprintf("var(--u%d)", id%8)
}

// ---- toasts & status ---------------------------------------------------------------

func (a *App) toast(kind, title, body string) {
	a.toastSeq++
	t := toast{kind: kind, title: title, body: body, until: nowMs() + 4000, id: a.toastSeq}
	a.toasts = append(a.toasts, t)
	if len(a.toasts) > 3 {
		a.toasts = a.toasts[len(a.toasts)-3:]
	}
	After(4100, a.Invalidate)
	a.Invalidate()
}

func (a *App) setStatus(kind, text string) {
	a.status = statusMsg{kind, text}
	a.Invalidate()
}

// ---- rendering ---------------------------------------------------------------------

func (a *App) Invalidate() {
	if a.renderQueue {
		return
	}
	a.renderQueue = true
	global.Call("requestAnimationFrame", a.renderFn)
}

func (a *App) render() {
	if a.r == nil || len(a.views) == 0 {
		return
	}
	now := nowMs()
	for k, until := range a.flash {
		if until < now {
			delete(a.flash, k)
		}
	}
	keep := a.toasts[:0]
	for _, t := range a.toasts {
		if t.until > now {
			keep = append(keep, t)
		}
	}
	a.toasts = keep
	a.ensureMounted()
	a.renderMenubar()
	a.renderTabs()
	// the view compiles its query, so it renders before the query bar shows errors/counts
	a.views[a.view].Render()
	a.renderQueryBar()
	a.renderStatus()
	a.renderHints()
	a.renderToasts()
	a.renderDialog()
	a.renderContext()
	a.sendFocus(false)
}

// ensureMounted swaps the DOM to the current view if it changed.
func (a *App) ensureMounted() {
	if a.mounted == a.view {
		return
	}
	if a.mounted >= 0 {
		a.views[a.mounted].Unmount()
	}
	Clear(a.viewport)
	a.views[a.view].Mount(a.viewport)
	a.mounted = a.view
	a.syncQueryInput()
}

// sigChanged: true when a region's signature differs from last time.
func (a *App) sigChanged(region, sig string) bool {
	if a.lastSig[region] == sig {
		return false
	}
	a.lastSig[region] = sig
	return true
}

// ---- views & navigation --------------------------------------------------------------

func (a *App) SwitchView(i int) {
	if i < 0 || i >= len(a.views) || i == a.view {
		return
	}
	if i == 8 && !a.r.IsAdmin() {
		// users view is readable by everyone; editing is admin-only
	}
	a.closeMenus()
	a.view = i
	a.Invalidate()
	After(30, func() { a.views[a.view].FocusDOM() })
}

func (a *App) curView() View { return a.views[a.view] }

func (a *App) closeMenus() {
	a.menuOpen = -1
	a.ctx = nil
}

// ---- keyboard ---------------------------------------------------------------------------

func keyFrom(ev js.Value) Key {
	k := Key{
		Name:  ev.Get("key").String(),
		Code:  ev.Get("code").String(),
		Ctrl:  ev.Get("ctrlKey").Bool() || ev.Get("metaKey").Bool(),
		Alt:   ev.Get("altKey").Bool(),
		Shift: ev.Get("shiftKey").Bool(),
	}
	t := ev.Get("target")
	if t.Truthy() {
		tag := strings.ToLower(t.Get("tagName").String())
		k.InInput = tag == "input" || tag == "textarea" || tag == "select"
	}
	return k
}

func (a *App) onKeyDown(ev js.Value) {
	if a.r == nil {
		return
	}
	k := keyFrom(ev)
	if k.Name == "Shift" || k.Name == "Control" || k.Name == "Alt" || k.Name == "Meta" {
		return
	}
	if a.handleKey(k) {
		ev.Call("preventDefault")
		ev.Call("stopPropagation")
		a.Invalidate()
	}
}

func (a *App) handleKey(k Key) bool {
	// modal layers first
	if a.dialog != nil {
		if a.dialog.Key(k) {
			return true
		}
		if k.Is("Escape") {
			a.CloseDialog()
			return true
		}
		// keep keys inside the dialog (but let text fields type)
		return !k.InInput && !k.Printable() && !k.Is("Tab", "S-Tab")
	}
	if a.ctx != nil {
		return a.ctx.Key(k)
	}
	if a.menuOpen >= 0 {
		return a.menuKey(k)
	}
	// native text editing shortcuts inside inputs
	if k.InInput && k.Ctrl && k.Is("C-c", "C-v", "C-x", "C-a", "C-z", "C-y", "C-S-z", "C-ArrowLeft", "C-ArrowRight") {
		if !a.qin.Equal(doc.Get("activeElement")) || !k.Is("C-z", "C-y") {
			return false
		}
	}
	if a.queryFocused() {
		if a.queryKey(k) {
			return true
		}
	}
	// the view (grid editor etc.) gets first claim on its own keys
	if a.curView().Key(k) {
		return true
	}
	// Alt+F/E/V/D/H open a menu
	if k.Alt && !k.Ctrl {
		for i, m := range a.menus() {
			if k.Is("A-" + m.Accel) {
				a.menuOpen, a.menuSel = i, 0
				return true
			}
		}
	}
	for _, act := range actions {
		if act.Run == nil || len(act.Keys) == 0 {
			continue
		}
		if k.Is(act.Keys...) && (act.When == nil || act.When(a)) {
			act.Run(a)
			return true
		}
	}
	return false
}

// ---- clipboard -----------------------------------------------------------------------------

func (a *App) copyText(s string) {
	a.clipboard = s
	defer func() { _ = recover() }()
	cb := global.Get("navigator").Get("clipboard")
	if cb.Truthy() && global.Get("isSecureContext").Bool() {
		var ok, fail js.Func
		ok = js.FuncOf(func(js.Value, []js.Value) any { ok.Release(); fail.Release(); return nil })
		fail = js.FuncOf(func(js.Value, []js.Value) any { ok.Release(); fail.Release(); return nil })
		cb.Call("writeText", s).Call("then", ok, fail)
		return
	}
	// insecure origin (plain http on the LAN): the copy-command fallback
	ta := E("textarea", "")
	ta.Set("value", s)
	Style(ta, "position", "fixed")
	Style(ta, "opacity", "0")
	Add(doc.Get("body"), ta)
	ta.Call("select")
	doc.Call("execCommand", "copy")
	ta.Call("remove")
}

func (a *App) onPaste(ev js.Value) {
	if a.r == nil || a.dialog != nil {
		return
	}
	t := ev.Get("target")
	if t.Truthy() {
		tag := strings.ToLower(t.Get("tagName").String())
		if tag == "input" || tag == "textarea" {
			return
		}
	}
	text := ev.Get("clipboardData").Call("getData", "text/plain").String()
	if text == "" {
		text = a.clipboard
	}
	if g := a.curView().ActiveGrid(); g != nil {
		ev.Call("preventDefault")
		g.Paste(text)
		a.Invalidate()
	}
}

// ---- saved filters ------------------------------------------------------------------------------

type savedFilter struct {
	Name  string `json:"name"`
	View  int    `json:"view"`
	Query string `json:"query"`
}

func (a *App) loadSaved() {
	_ = json.Unmarshal([]byte(storageGet("pt.saved")), &a.savedQ)
}

func (a *App) saveFilter(name string) {
	v := a.curView()
	a.savedQ = append(a.savedQ, savedFilter{Name: name, View: a.view, Query: v.Query()})
	sort.SliceStable(a.savedQ, func(i, j int) bool { return a.savedQ[i].Name < a.savedQ[j].Name })
	b, _ := json.Marshal(a.savedQ)
	storageSet("pt.saved", string(b))
	a.toast("ok", "Filter saved", name)
}

func (a *App) applySaved(f savedFilter) {
	a.SwitchView(f.View)
	a.views[f.View].SetQuery(f.Query)
	a.syncQueryInput()
	a.Invalidate()
}

// ---- logout -------------------------------------------------------------------------------------

func (a *App) Logout() {
	fetchText("POST", "/api/logout", "", func(int, string) {
		a.sock.Stop()
		a.teardown()
		a.r = nil
		a.showLogin("Signed out.")
	})
}
