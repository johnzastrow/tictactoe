//go:build js && wasm

// Package webapp is demo's Go web client (GOOS=js GOARCH=wasm): the same 16
// tabs, keys and help as the TUIs (docs/demo/ui-spec.md), drawn with a small
// hand-written DOM kit modelled on pt's web client.
package webapp

// The application: one App value owns the value store, the socket, the
// shell regions and the tabs. Every change goes through Invalidate(), which
// schedules one render on the next animation frame.

import (
	"encoding/json"
	"errors"
	"fmt"
	"strings"
	"syscall/js"

	"git.wharf.fluidgrid.org/jcz/tictactoe/go/internal/demo/core"
)

const (
	connConnecting = iota
	connOn
	connWaiting
	connOffline
)

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

// pendingReq is a request awaiting its ack.
type pendingReq struct {
	kind   string // set reset toast
	key    string
	value  json.RawMessage // set: the normalised value sent
	label  string          // reset: what was reset
	onDone func(core.AckMsg)
}

// queuedSet is the latest edit of a key whose previous set is still in
// flight; it goes out (based on the new version) when that ack arrives.
type queuedSet struct {
	value  json.RawMessage
	onDone func(core.AckMsg)
}

type App struct {
	v    *core.Values
	sock Socket
	conn int
	name string

	// shell regions
	root, shell, menubar, sidebar, main, statusEl, hints js.Value
	dialogLayer, toastLayer, popLayer                    js.Value
	shellScope                                           Scope

	tabs      []Tab
	tab       int  // index into tabs
	focusSide bool // the sidebar (not the tab body) has the focus
	theme     string
	showKeys  bool
	showValue bool

	menuOpen int
	menuSel  int
	dialog   Dialog
	pop      *popState
	toasts   []toast
	toastSeq int
	status   statusMsg

	reqN     uint64
	pending  map[uint64]*pendingReq
	inflight map[string]uint64 // key → req of its set in flight
	queued   map[string]queuedSet
	server   map[string]core.Entry // last server-confirmed entry per key (for rollback)
	flash    map[string]float64    // key → flash-until (ms)

	renderQueue bool
	renderFn    js.Func
	lastSig     map[string]string
	sentFocus   string
	started     bool
}

var app *App

// Run is the WASM entry point (called from main).
func Run() {
	app = &App{menuOpen: -1, lastSig: map[string]string{}, showKeys: true, showValue: true}
	app.theme = storageGet("demo.theme")
	if themeNames[app.theme] == "" {
		app.theme = "midnight"
	}
	app.showKeys = storageGet("demo.hideKeys") != "1"
	app.showValue = storageGet("demo.hideValue") != "1"
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
	// ?name=alice joins directly; otherwise the join page (name remembered).
	q := global.Get("URLSearchParams").New(global.Get("location").Get("search"))
	if n := q.Call("get", "name"); n.Truthy() {
		if name, ok := core.CleanName(n.String()); ok {
			storageSet("demo.name", name)
			app.join(name)
			return
		}
	}
	app.showJoin("")
}

func today() core.Date {
	d := global.Get("Date").New()
	t, _ := core.FromYMD(d.Call("getFullYear").Int(), d.Call("getMonth").Int()+1, d.Call("getDate").Int())
	return t
}

func (a *App) applyTheme() {
	doc.Get("documentElement").Call("setAttribute", "data-theme", a.theme)
}

// ---- join ----------------------------------------------------------------------

var joinScope Scope

func (a *App) showJoin(msg string) {
	joinScope.Release()
	Clear(a.root)
	wrap := E("div", "login-wrap")
	form := E("form", "login")
	Attr(form, "data-popup", "join")
	brand := ET("div", "brand", "demo ")
	Add(brand, ET("span", "", "control gallery · Go/WASM"))
	name := E("input", "")
	Attr(name, "id", "join-name")
	Attr(name, "name", "name")
	Attr(name, "autocomplete", "nickname")
	Attr(name, "maxlength", "24")
	Attr(name, "spellcheck", "false")
	name.Set("value", storageGet("demo.name"))
	errEl := ET("div", "lerr", msg)
	btn := ET("button", "btn primary", "Join")
	Attr(btn, "type", "submit")
	Attr(btn, "id", "join-btn")
	ln := ET("label", "", "Your name")
	Add(ln, name)
	Add(form, brand, ET("div", "lsub", "Every control on its own tab, shared live with everyone connected."),
		ln, errEl, btn, ET("div", "lfoot", "No password: the name is just a label others see."))
	Add(wrap, form)
	Add(a.root, wrap)
	name.Call("focus")
	name.Call("select")
	joinScope.On(form, "submit", func(ev js.Value) {
		ev.Call("preventDefault")
		n, ok := core.CleanName(name.Get("value").String())
		if !ok {
			errEl.Set("textContent", "1–24 letters, digits, space, _ . - starting with a letter.")
			name.Call("focus")
			return
		}
		storageSet("demo.name", n)
		joinScope.Release()
		a.join(n)
	})
}

// join builds the shell and opens the socket.
func (a *App) join(name string) {
	a.name = name
	a.v = core.NewValues()
	a.pending = map[uint64]*pendingReq{}
	a.inflight = map[string]uint64{}
	a.queued = map[string]queuedSet{}
	a.server = map[string]core.Entry{}
	a.flash = map[string]float64{}
	a.lastSig = map[string]string{}
	a.tab, a.focusSide, a.menuOpen, a.sentFocus = 0, true, -1, ""
	a.status = statusMsg{"info", "ℹ Connecting as " + name + "…"}
	a.buildShell()
	a.sock = Socket{Name: name}
	// green once the welcome has arrived (values and server info loaded)
	a.sock.OnOpen = func() { a.Invalidate() }
	a.sock.OnMessage = a.onMessage
	a.sock.OnClose = func(code int, reason string) {
		// three failures in a row (≈ 3.5 s) is "offline" (red), before that amber
		a.conn = connWaiting
		if a.sock.Failures >= 3 {
			a.conn = connOffline
		}
		a.pending = map[uint64]*pendingReq{}
		a.inflight = map[string]uint64{}
		a.queued = map[string]queuedSet{}
		a.sentFocus = ""
		a.Invalidate()
		a.sock.Retry()
	}
	a.conn = connConnecting
	a.sock.Connect()
	if !a.started {
		a.started = true
		// relative times ("2m ago") and toast expiry
		var tick js.Func
		tick = js.FuncOf(func(js.Value, []js.Value) any {
			if a.v != nil {
				a.Invalidate()
			}
			return nil
		})
		global.Call("setInterval", tick, 15000)
	}
	a.Invalidate()
	After(30, a.focusCurrent)
}

// Leave closes the socket and returns to the join page (File ▸ Leave).
func (a *App) Leave() {
	a.sock.Stop()
	a.closePop()
	a.dialog = nil
	dialogBuilt = nil
	for _, t := range a.tabs {
		t.Destroy()
	}
	a.tabs = nil
	a.shellScope.Release()
	a.v = nil
	// drop ?name= so a reload shows the join page too
	loc := global.Get("location")
	global.Get("history").Call("replaceState", js.Null(), "", loc.Get("pathname"))
	a.showJoin("")
}

// ---- server messages -----------------------------------------------------------

func (a *App) onMessage(text string) {
	if a.v == nil {
		return
	}
	msg, err := core.DecodeServerMsg([]byte(text))
	if err != nil {
		a.setStatus("err", "✗ unreadable server message")
		return
	}
	switch m := msg.(type) {
	case core.WelcomeMsg:
		a.conn = connOn
		a.v.ApplyWelcome(m)
		for _, k := range core.Keys {
			a.server[k] = a.v.Get(k)
		}
		for _, t := range a.tabs {
			for _, k := range t.Info().Keys {
				t.Synced(k, syncEv{welcome: true})
			}
		}
		a.setStatus("ok", fmt.Sprintf("✓ Connected to %s %s as %s", m.Server.Impl, m.Server.Version, core.SafeText(m.You.Name)))
		a.sentFocus = ""
		a.sendFocus()
	case core.ChangedMsg:
		if !a.v.ApplyChanged(m) {
			break
		}
		a.server[m.Key] = m.Entry()
		mine := m.Conn == a.v.You.Conn
		by := core.SafeText(m.By)
		if !mine {
			a.flashKey(m.Key)
			a.setStatus("info", by+" changed "+m.Key)
		}
		if t := a.tabFor(m.Key); t != nil {
			t.Synced(m.Key, syncEv{by: by, mine: mine})
		}
	case core.AckMsg:
		a.onAck(m)
	case core.PresenceMsg:
		a.v.ApplyPresence(m)
	case core.ToastMsg:
		a.toast("info", core.SafeText(m.By), core.SafeText(m.Text))
	case core.ErrorMsg:
		a.toast("err", "Server error", core.SafeText(m.Message))
	}
	a.Invalidate()
}

func (a *App) onAck(m core.AckMsg) {
	p := a.pending[m.Req]
	if p == nil {
		return
	}
	delete(a.pending, m.Req)
	switch p.kind {
	case "set":
		if a.inflight[p.key] == m.Req {
			delete(a.inflight, p.key)
		}
		q, hasQ := a.queued[p.key]
		delete(a.queued, p.key)
		switch {
		case m.OK && m.Version != nil:
			// apply our own value now (the `changed` that follows agrees)
			e := core.Entry{Value: p.value, Version: *m.Version, By: a.v.You.Name, At: core.NowTS()}
			a.v.ApplyEntry(p.key, e)
			a.server[p.key] = a.v.Get(p.key)
			if hasQ {
				a.sendSet(core.SetMsg{Key: p.key, Base: *m.Version, Value: q.value}, q.onDone)
			} else {
				a.setStatus("ok", "✓ Saved "+p.key)
			}
		case m.Conflict && m.Current != nil:
			a.v.ApplyEntry(p.key, *m.Current)
			a.server[p.key] = a.v.Get(p.key)
			by := core.SafeText(m.Current.By)
			if by == "" {
				by = "someone"
			}
			a.setStatus("warn", "⚠ "+by+" changed "+p.key+" first, showing theirs")
			a.flashKey(p.key)
			if t := a.tabFor(p.key); t != nil {
				t.Synced(p.key, syncEv{by: by, conflict: true})
			}
		default:
			// rejected: roll the optimistic value back
			if e, ok := a.server[p.key]; ok {
				a.v.ApplyEntry(p.key, e)
			}
			a.setStatus("err", "✗ "+p.key+": "+core.SafeText(m.Error))
			if t := a.tabFor(p.key); t != nil {
				t.Synced(p.key, syncEv{rejected: true})
			}
		}
	case "reset":
		if m.OK {
			a.setStatus("ok", "✓ Reset "+p.label)
		} else {
			a.setStatus("err", "✗ Reset failed: "+core.SafeText(m.Error))
		}
	case "toast":
		if m.OK {
			a.setStatus("ok", "✓ Broadcast sent")
		} else {
			a.setStatus("err", "✗ "+core.SafeText(m.Error))
		}
	}
	if p.onDone != nil {
		p.onDone(m)
	}
}

// ---- requests ----------------------------------------------------------------------

var errNotConnected = errors.New("not connected")

func (a *App) send(m any) bool {
	b, err := core.EncodeClientMsg(m)
	if err != nil {
		return false
	}
	return a.sock.Send(b)
}

// Commit validates value for key and sends a set based on the version held
// now. On a local validation error nothing is sent; the error is returned
// (and shown in the status line) for the control to display.
func (a *App) Commit(key string, value any) error { return a.CommitFrom(key, value, -1, nil) }

// CommitFrom is Commit with an explicit base (the version shown when editing
// began; -1 = current) and an ack callback.
func (a *App) CommitFrom(key string, value any, base int64, onDone func(core.AckMsg)) error {
	m, err := a.v.PrepareSet(key, value)
	if err != nil {
		a.setStatus("err", "✗ "+err.Error())
		return err
	}
	if base >= 0 {
		m.Base = base
	}
	if !a.sock.Open() {
		a.setStatus("err", "✗ Not connected: the change wasn't sent")
		return errNotConnected
	}
	// optimistic: show the new value at once (same version until the ack)
	cur := a.v.Get(key)
	a.v.ApplyEntry(key, core.Entry{Value: m.Value, Version: cur.Version, By: cur.By, At: cur.At})
	if _, busy := a.inflight[key]; busy {
		a.queued[key] = queuedSet{m.Value, onDone}
		a.Invalidate()
		return nil
	}
	a.sendSet(m, onDone)
	return nil
}

func (a *App) sendSet(m core.SetMsg, onDone func(core.AckMsg)) {
	a.reqN++
	m.Req = a.reqN
	if !a.send(m) {
		a.setStatus("err", "✗ Not connected: the change wasn't sent")
		return
	}
	a.pending[m.Req] = &pendingReq{kind: "set", key: m.Key, value: m.Value, onDone: onDone}
	a.inflight[m.Key] = m.Req
	a.Invalidate()
}

// Reset restores keys to their defaults (all keys when empty).
func (a *App) Reset(keys []string, label string) {
	a.reqN++
	if !a.send(core.ResetMsg{Req: a.reqN, Keys: keys}) {
		a.setStatus("err", "✗ Not connected")
		return
	}
	a.pending[a.reqN] = &pendingReq{kind: "reset", label: label}
}

// Broadcast sends a toast to everyone.
func (a *App) Broadcast(text string) error {
	t, err := core.CheckToast(text)
	if err != nil {
		return err
	}
	a.reqN++
	if !a.send(core.SendToastMsg{Req: a.reqN, Text: t}) {
		return errNotConnected
	}
	a.pending[a.reqN] = &pendingReq{kind: "toast"}
	return nil
}

// Busy reports whether a set for key is in flight (its control shows the
// optimistic value until the ack).
func (a *App) Busy(key string) bool { _, ok := a.inflight[key]; return ok }

// sendFocus tells peers which tab we're on (no reply; deduplicated).
func (a *App) sendFocus() {
	if a.v == nil || !a.v.Loaded() || len(a.tabs) == 0 {
		return
	}
	id := a.cur().Info().ID
	if id == a.sentFocus {
		return
	}
	if a.send(core.FocusMsg{Tab: id}) {
		a.sentFocus = id
	}
}

func (a *App) tabFor(key string) Tab {
	id := core.KeyTab(key)
	for _, t := range a.tabs {
		if t.Info().ID == id {
			return t
		}
	}
	return nil
}

// flashKey marks a key's Value panel line and control for a short flash.
func (a *App) flashKey(key string) {
	a.flash[key] = nowMs() + 1200
	After(1300, a.Invalidate)
}

// ---- toasts & status ---------------------------------------------------------------

func (a *App) toast(kind, title, body string) {
	a.toastSeq++
	a.toasts = append(a.toasts, toast{kind: kind, title: title, body: body, until: nowMs() + 4000, id: a.toastSeq})
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

func (a *App) info(text string) { a.setStatus("info", "ℹ "+text) }

// ---- rendering ---------------------------------------------------------------------

func (a *App) Invalidate() {
	if a.renderQueue {
		return
	}
	a.renderQueue = true
	global.Call("requestAnimationFrame", a.renderFn)
}

func (a *App) render() {
	if a.v == nil || len(a.tabs) == 0 {
		return
	}
	now := nowMs()
	keep := a.toasts[:0]
	for _, t := range a.toasts {
		if t.until > now {
			keep = append(keep, t)
		}
	}
	a.toasts = keep
	for k, until := range a.flash {
		if until < now {
			delete(a.flash, k)
		}
	}
	a.renderMenubar()
	a.renderSidebar()
	a.renderBody()
	a.renderStatus()
	a.renderHints()
	a.renderToasts()
	a.renderDialog()
	a.renderPop()
	a.sendFocus()
}

// sigChanged: true when a region's signature differs from last time.
func (a *App) sigChanged(region, sig string) bool {
	if a.lastSig[region] == sig {
		return false
	}
	a.lastSig[region] = sig
	return true
}

// ---- tabs & focus --------------------------------------------------------------

func (a *App) cur() Tab { return a.tabs[a.tab] }

// SwitchTab shows tab i (the sidebar keeps the focus if it had it).
func (a *App) SwitchTab(i int) {
	if i < 0 || i >= len(a.tabs) {
		return
	}
	a.closeMenus()
	a.closePop()
	if i != a.tab {
		a.tab = i
		a.cur().Shown()
	}
	a.Invalidate()
	After(0, a.focusCurrent)
}

// GoTab switches to tab i and moves the focus into its body.
func (a *App) GoTab(i int) {
	a.SwitchTab(i)
	a.focusSide = false
	After(0, a.focusCurrent)
}

// focusCurrent puts the DOM focus where the model says it is.
func (a *App) focusCurrent() {
	if a.v == nil || len(a.tabs) == 0 || a.dialog != nil {
		return
	}
	if a.focusSide {
		a.sidebar.Call("focus")
	} else {
		a.cur().FocusCur()
	}
}

func (a *App) focusSidebar() {
	a.closePop()
	a.focusSide = true
	a.sidebar.Call("focus")
	a.Invalidate()
}

func (a *App) focusBody() {
	// focus first: a control's focus handler can still see we came from the sidebar
	a.cur().Focus(0)
	a.focusSide = false
	a.Invalidate()
}

// onFocusIn keeps the model in step with clicks and native Tab moves.
func (a *App) onFocusIn(ev js.Value) {
	t := ev.Get("target")
	switch {
	case Closest(t, ".sidebar").Truthy():
		a.focusSide = true
	case Closest(t, "[data-tab]").Truthy():
		a.focusSide = false
		if c := Closest(t, "[data-ci]"); c.Truthy() {
			var i int
			fmt.Sscan(DataOf(c, "ci"), &i)
			a.cur().SetCur(i)
		}
	}
	a.Invalidate()
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
	k.InInput = isTextField(ev.Get("target"))
	return k
}

func isTextField(t js.Value) bool {
	if !t.Truthy() || t.Get("tagName").IsUndefined() {
		return false
	}
	tag := strings.ToLower(t.Get("tagName").String())
	return tag == "input" || tag == "textarea"
}

func (a *App) onKeyDown(ev js.Value) {
	if a.v == nil {
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
		// keep keys inside the dialog (but let text fields type and Tab move)
		return !k.InInput && !k.Printable() && !k.Is("Tab", "S-Tab", "Enter", " ")
	}
	if a.menuOpen >= 0 {
		return a.menuKey(k)
	}
	// global keys (none of them collides with a control's own keys)
	if k.Alt && !k.Ctrl {
		for i, m := range a.menus() {
			if k.Is("A-" + m.Accel) {
				a.closePop()
				a.menuOpen, a.menuSel = i, 0
				return true
			}
		}
	}
	for _, act := range actions {
		if len(act.Keys) > 0 && k.Is(act.Keys...) {
			act.Run(a)
			return true
		}
	}
	if a.focusSide {
		return a.sidebarKey(k)
	}
	// nothing focused (a click on plain text): give the control its focus back
	if ae := doc.Get("activeElement"); !ae.Truthy() || ae.Equal(doc.Get("body")) {
		a.cur().FocusCur()
		k.InInput = isTextField(doc.Get("activeElement"))
	}
	if a.cur().Key(k) {
		return true
	}
	switch {
	case k.Is("Tab"):
		a.cur().Move(1)
		return true
	case k.Is("S-Tab"):
		if !a.cur().Move(-1) {
			a.focusSidebar()
		}
		return true
	case k.Is("Escape"):
		if a.pop != nil {
			a.closePop()
		} else {
			a.focusSidebar()
		}
		return true
	}
	return false
}

func (a *App) sidebarKey(k Key) bool {
	n := len(a.tabs)
	switch {
	case k.Is("ArrowUp"):
		a.SwitchTab((a.tab + n - 1) % n)
	case k.Is("ArrowDown"):
		a.SwitchTab((a.tab + 1) % n)
	case k.Is("Home"):
		a.SwitchTab(0)
	case k.Is("End"):
		a.SwitchTab(n - 1)
	case k.Is("Enter", "ArrowRight", "Tab"):
		a.focusBody()
	case k.Is("S-Tab"):
		// stay: the sidebar is the first focus area
	default:
		return false
	}
	return true
}

// runHint runs a clicked hint's key spec as if it had been pressed.
func (a *App) runHint(spec string) {
	if a.dialog == nil && a.menuOpen < 0 {
		a.focusCurrent()
	}
	k := KeyFromSpec(spec)
	k.InInput = isTextField(doc.Get("activeElement"))
	a.handleKey(k)
	a.Invalidate()
}
