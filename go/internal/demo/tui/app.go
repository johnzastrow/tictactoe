package tui

// The application model. Bubble Tea owns the loop; the model holds a
// pointer to App so tabs, dialogs and drafts can be updated in place. View
// only reads state (see render.go).
//
// Values flow as the protocol says: every commit is a `set` with the version
// the edit was based on; the server's `changed` broadcasts keep the local
// core.Values current. Commits are shown optimistically and coalesced per
// key (one request in flight, the newest value queued behind it), so a key
// held down on a slider doesn't conflict with itself.

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"time"

	tea "charm.land/bubbletea/v2"

	"git.wharf.fluidgrid.org/jcz/tictactoe/go/internal/demo/client"
	"git.wharf.fluidgrid.org/jcz/tictactoe/go/internal/demo/core"
)

const (
	tickEvery  = 200 * time.Millisecond
	toastFor   = 4 * time.Second
	flashFor   = 1500 * time.Millisecond
	focusDelay = 150 * time.Millisecond
	clientID   = "tui-go"
)

type phase int

const (
	phJoin phase = iota
	phConnecting
	phMain
)

type connState int

const (
	connConnecting connState = iota
	connOnline
	connReconnecting
)

// message kinds (status line and toasts)
const (
	kInfo = iota
	kOK
	kWarn
	kErr
)

type toast struct {
	text string
	kind int
	at   time.Time
}

// inflight is a `set` waiting for its ack.
type inflight struct {
	req   uint64
	key   string
	value json.RawMessage
	prev  core.Entry // to restore if the server refuses the value
}

// changeKind tells a tab what happened to one of its keys.
type changeKind int

const (
	chSaved    changeKind = iota // my commit was acked
	chConflict                   // my commit lost to someone else's
	chRemote                     // another peer changed it
	chError                      // the server refused my value
	chReset                      // ^R / reconnect: drop drafts
)

// keyWatcher is implemented by tabs that keep drafts of their keys.
type keyWatcher interface {
	changed(a *App, key string, kind changeKind, by string)
}

type dragState struct {
	x0, y0 int
	x, y   int
	moved  bool
	move   func(a *App, x, y int)
	drop   func(a *App, x, y int, hits []Hit)
}

type App struct {
	opts      Options
	cl        *client.Client
	sess      *client.Session
	ctx       context.Context
	cancel    context.CancelFunc
	vals      *core.Values
	w, h      int
	theme     int
	truecolor bool
	now       time.Time
	today     core.Date

	ph      phase
	join    *joinForm
	conn    connState
	connErr string
	name    string // my display name (from the welcome, else the one asked for)

	tab      int // 0–15
	inBody   bool
	tabs     []tabCtl
	overlays []overlay
	toasts   []toast
	status   string
	statKind int
	hideKeys bool
	hideVal  bool
	flash    map[string]time.Time
	drag     *dragState

	req      uint64
	inflight map[string]*inflight // by key
	byReq    map[uint64]*inflight
	queued   map[string]json.RawMessage
	resetReq map[uint64]string // reset acks → label
	toastReq map[uint64]bool

	focusSent string
	focusAt   time.Time
	lastClick time.Time
	lastHit   Hit
	quitting  bool
	cmds      []tea.Cmd
	// outbox captures sent messages when there is no session (tests).
	outbox *[]any
}

type Model struct{ a *App }

// messages
type (
	netMsg struct {
		ev client.Event
		ok bool
	}
	tickMsg    struct{}
	sendErrMsg struct{ err error }
)

var errOffline = errors.New("offline")

func newApp(opts Options) *App {
	ctx, cancel := context.WithCancel(context.Background())
	a := &App{
		opts: opts, ctx: ctx, cancel: cancel, vals: core.NewValues(),
		theme: themeIndex(opts.Theme), truecolor: detectTruecolor(), now: time.Now(), today: core.Today(),
		flash: map[string]time.Time{}, inflight: map[string]*inflight{}, byReq: map[uint64]*inflight{},
		queued: map[string]json.RawMessage{}, resetReq: map[uint64]string{}, toastReq: map[uint64]bool{},
		name: opts.Name,
	}
	switch opts.Colors {
	case "256":
		a.truecolor = false
	case "truecolor":
		a.truecolor = true
	}
	a.tabs = newTabs()
	a.join = newJoinForm(opts.Server, opts.Name)
	return a
}

func (m Model) Init() tea.Cmd {
	a := m.a
	cmds := []tea.Cmd{tick()}
	if a.ph == phConnecting {
		cmds = append(cmds, a.waitNet())
	}
	return tea.Batch(cmds...)
}

func tick() tea.Cmd {
	return tea.Tick(tickEvery, func(time.Time) tea.Msg { return tickMsg{} })
}

// connect starts a session for the join form's (or the flags') server and
// name. It returns an error for a bad URL or CA file.
func (a *App) connect(server, name string) error {
	cl, err := client.New(server, a.opts.CACert)
	if err != nil {
		return err
	}
	a.cl = cl
	a.opts.Server = cl.Base
	a.name = name
	a.conn = connConnecting
	a.connErr = ""
	a.sess = cl.Start(a.ctx, clientID, name)
	a.ph = phConnecting
	return nil
}

func (a *App) waitNet() tea.Cmd {
	s := a.sess
	if s == nil {
		return nil
	}
	return func() tea.Msg {
		ev, ok := <-s.Events()
		return netMsg{ev, ok}
	}
}

// queue adds a command to run after this update.
func (a *App) queue(c tea.Cmd) {
	if c != nil {
		a.cmds = append(a.cmds, c)
	}
}

// send writes a client message off the UI loop.
func (a *App) send(msg any) {
	s, ctx := a.sess, a.ctx
	if s == nil {
		if a.outbox != nil {
			*a.outbox = append(*a.outbox, msg)
		}
		return
	}
	a.queue(func() tea.Msg {
		if err := s.Send(ctx, msg); err != nil {
			return sendErrMsg{err}
		}
		return nil
	})
}

func (a *App) nextReq() uint64 {
	if a.sess != nil {
		return a.sess.NextReq()
	}
	a.req++
	return a.req
}

func (a *App) online() bool {
	if a.sess == nil {
		return a.outbox != nil
	}
	return a.conn == connOnline && a.vals.Loaded()
}

func (a *App) myName() string {
	if a.vals.You.Name != "" {
		return a.vals.You.Name
	}
	return a.name
}

// commit sends a `set` for key based on the version held now.
func (a *App) commit(key string, value any) error { return a.commitBase(key, value, -1) }

// commitBase validates value (the server's rules, cross-key date rule
// included), shows it at once and sends it with base (−1 = the version held
// now; text fields pass the version they started editing from). On error
// nothing is sent and the error is for the control to show.
func (a *App) commitBase(key string, value any, base int64) error {
	msg, err := a.vals.PrepareSet(key, value)
	if err != nil {
		return err
	}
	if !a.online() {
		a.toast("✗ Offline: not saved (reconnecting…)", kErr)
		return errOffline
	}
	if base >= 0 {
		msg.Base = base
	}
	prev := a.vals.Get(key)
	// optimistic: show it now, keeping the version so later bases stay right
	a.vals.ApplyEntry(key, core.Entry{Value: msg.Value, Version: prev.Version, By: a.myName(), At: core.FormatTS(a.now)})
	if a.inflight[key] != nil {
		a.queued[key] = msg.Value
		return nil
	}
	a.sendSet(msg, prev)
	return nil
}

func (a *App) sendSet(msg core.SetMsg, prev core.Entry) {
	msg.Req = a.nextReq()
	p := &inflight{req: msg.Req, key: msg.Key, value: msg.Value, prev: prev}
	a.inflight[msg.Key] = p
	a.byReq[msg.Req] = p
	a.send(msg)
}

// resetKeys restores keys to their defaults (all keys when none given).
func (a *App) resetKeys(label string, keys ...string) {
	if !a.online() {
		a.toast("✗ Offline: can't reset now", kErr)
		return
	}
	req := a.nextReq()
	a.resetReq[req] = label
	for _, k := range keys {
		a.notify(k, chReset, "")
	}
	if len(keys) == 0 {
		for _, k := range core.Keys {
			a.notify(k, chReset, "")
		}
	}
	a.send(core.ResetMsg{Req: req, Keys: keys})
}

// broadcast sends a toast to everyone.
func (a *App) broadcast(text string) error {
	text, err := core.CheckToast(text)
	if err != nil {
		return err
	}
	if !a.online() {
		return errOffline
	}
	req := a.nextReq()
	a.toastReq[req] = true
	a.send(core.SendToastMsg{Req: req, Text: text})
	return nil
}

func (a *App) toast(text string, kind int) {
	a.toasts = append(a.toasts, toast{text, kind, a.now})
	if len(a.toasts) > 3 {
		a.toasts = a.toasts[len(a.toasts)-3:]
	}
	a.setStatus(text, kind)
}

func (a *App) setStatus(text string, kind int) { a.status, a.statKind = text, kind }

// info is the ℹ status message demo controls use for local actions.
func (a *App) info(text string) { a.setStatus("ℹ "+text, kInfo) }

func (a *App) curTab() tabCtl       { return a.tabs[a.tab] }
func (a *App) curTabInfo() core.Tab { return core.Tabs[a.tab] }

func (m Model) Update(msg tea.Msg) (tea.Model, tea.Cmd) {
	a := m.a
	cmd := a.update(msg)
	cmds := append(a.cmds, cmd)
	a.cmds = nil
	return m, tea.Batch(cmds...)
}

func (a *App) update(msg tea.Msg) tea.Cmd {
	switch msg := msg.(type) {
	case tea.WindowSizeMsg:
		a.w, a.h = msg.Width, msg.Height
	case tickMsg:
		a.now = time.Now()
		a.today = core.DateOf(a.now)
		kept := a.toasts[:0]
		for _, t := range a.toasts {
			if a.now.Sub(t.at) < toastFor {
				kept = append(kept, t)
			}
		}
		a.toasts = kept
		for k, t := range a.flash {
			if a.now.Sub(t) > flashFor {
				delete(a.flash, k)
			}
		}
		a.maybeSendFocus()
		return tick()
	case netMsg:
		if !msg.ok {
			a.sess = nil
			return nil
		}
		a.onNet(msg.ev)
		return a.waitNet()
	case sendErrMsg:
		a.setStatus("✗ Not sent: "+msg.err.Error(), kErr)
	case tea.PasteMsg:
		a.onPaste(msg.Content)
	case tea.KeyPressMsg:
		return a.onKey(msg)
	case tea.MouseMsg:
		// resolve against the same (pure) frame View draws
		_, hits := a.frame()
		mm := msg.Mouse()
		h, ok := topHit(hits, mm.X, mm.Y)
		return a.onMouse(hitMsg{m: mm, ev: msg, hit: h, ok: ok, hits: hits})
	}
	return nil
}

// onNet applies a session event.
func (a *App) onNet(ev client.Event) {
	switch ev.Kind {
	case client.Connected:
		a.conn = connOnline
		a.connErr = ""
	case client.Disconnected:
		if a.conn == connOnline && a.ph == phMain {
			a.toast("⚠ Connection lost; reconnecting…", kWarn)
		}
		a.conn = connReconnecting
		if ev.Err != nil {
			a.connErr = ev.Err.Error()
		}
		// requests in flight will never be answered; the welcome on
		// reconnect replaces every value
		a.inflight = map[string]*inflight{}
		a.byReq = map[uint64]*inflight{}
		a.queued = map[string]json.RawMessage{}
	case client.Rejected:
		a.sess = nil
		a.ph = phJoin
		msg := "the server refused this name"
		if ev.Err != nil {
			msg = "Refused: " + ev.Err.Error()
		}
		a.join.err = msg
	case client.Message:
		a.onServerMsg(ev.Msg)
	}
}

func (a *App) onServerMsg(msg any) {
	switch m := msg.(type) {
	case core.WelcomeMsg:
		a.vals.ApplyWelcome(m)
		a.name = m.You.Name
		a.ph = phMain
		a.focusSent = "" // tell the server where we are again
		a.focusAt = time.Time{}
		a.maybeSendFocus()
	case core.ChangedMsg:
		a.onChanged(m)
	case core.AckMsg:
		a.onAck(m)
	case core.PresenceMsg:
		a.vals.ApplyPresence(m)
	case core.ToastMsg:
		a.toast(core.SafeText(m.By)+": "+core.SafeText(m.Text), kInfo)
	case core.ErrorMsg:
		a.toast("✗ Server: "+core.SafeText(m.Message), kErr)
	}
}

func (a *App) onChanged(m core.ChangedMsg) {
	mine := m.Conn == a.vals.You.Conn && a.vals.You.Conn != 0
	if mine && a.inflight[m.Key] != nil {
		return // the echo of a set whose newer successor is in flight
	}
	if !a.vals.ApplyChanged(m) {
		return
	}
	if mine {
		return
	}
	by := core.SafeText(m.By)
	a.flash[m.Key] = a.now
	a.setStatus(by+" changed "+m.Key, kInfo)
	a.notify(m.Key, chRemote, by)
}

func (a *App) onAck(m core.AckMsg) {
	if label, ok := a.resetReq[m.Req]; ok {
		delete(a.resetReq, m.Req)
		if m.OK {
			a.setStatus("✓ Reset "+label, kOK)
		} else {
			a.setStatus("✗ Reset failed: "+core.SafeText(m.Error), kErr)
		}
		return
	}
	if a.toastReq[m.Req] {
		delete(a.toastReq, m.Req)
		if !m.OK {
			a.setStatus("✗ Broadcast: "+core.SafeText(m.Error), kErr)
		}
		return
	}
	p := a.byReq[m.Req]
	if p == nil {
		return
	}
	delete(a.byReq, m.Req)
	if a.inflight[p.key] == p {
		delete(a.inflight, p.key)
	}
	q, queued := a.queued[p.key]
	delete(a.queued, p.key)
	switch {
	case m.OK:
		v := p.prev.Version + 1
		if m.Version != nil {
			v = *m.Version
		}
		e := core.Entry{Value: p.value, Version: v, By: a.myName(), At: core.FormatTS(a.now)}
		if queued {
			// the newest value goes next, based on the version just made
			e.Value = q
			a.vals.ApplyEntry(p.key, e)
			a.sendSet(core.SetMsg{Key: p.key, Base: v, Value: q}, e)
			return
		}
		a.vals.ApplyEntry(p.key, e)
		a.setStatus("✓ Saved "+p.key, kOK)
		a.notify(p.key, chSaved, "")
	case m.Conflict:
		who := "someone"
		if m.Current != nil {
			a.vals.ApplyEntry(p.key, *m.Current)
			if m.Current.By != "" {
				who = core.SafeText(m.Current.By)
			}
		}
		a.flash[p.key] = a.now
		a.setStatus(fmt.Sprintf("⚠ %s changed %s first, showing theirs", who, p.key), kWarn)
		a.notify(p.key, chConflict, who)
	default:
		a.vals.ApplyEntry(p.key, p.prev)
		a.setStatus("✗ "+p.key+": "+core.SafeText(m.Error), kErr)
		a.notify(p.key, chError, m.Error)
	}
}

// notify tells the tab owning key what happened.
func (a *App) notify(key string, kind changeKind, by string) {
	tab := core.KeyTab(key)
	for i, t := range core.Tabs {
		if t.ID == tab {
			if w, ok := a.tabs[i].(keyWatcher); ok {
				w.changed(a, key, kind, by)
			}
		}
	}
}

// maybeSendFocus tells the server which tab we show, once the tab has been
// stable for focusDelay (scrolling through tabs mustn't flood the server).
func (a *App) maybeSendFocus() {
	if a.ph != phMain || !a.online() {
		return
	}
	id := a.curTabInfo().ID
	if id == a.focusSent || a.now.Sub(a.focusAt) < focusDelay {
		return
	}
	a.focusSent = id
	a.send(core.FocusMsg{Tab: id})
}

// setTab switches tabs (the body keeps or loses focus as the caller says).
func (a *App) setTab(i int) {
	i = (i + len(a.tabs)) % len(a.tabs)
	if i == a.tab {
		return
	}
	if l, ok := a.curTab().(interface{ leave(*App) }); ok {
		l.leave(a)
	}
	a.tab = i
	a.focusAt = a.now
	a.drag = nil
}

// enterBody moves focus into the tab body at control i (−1 = last).
func (a *App) enterBody(i int) {
	t := a.curTab()
	n := t.controls()
	if n == 0 {
		return
	}
	if i < 0 {
		i = n - 1
	}
	a.inBody = true
	t.focusCtl(a, i)
}

// View renders the frame. Mouse events are resolved in Update against the
// same (pure) frame function, so clicks hit exactly what was drawn.
func (m Model) View() tea.View {
	a := m.a
	b, _ := a.frame()
	v := tea.NewView(b.Render(a.truecolor))
	v.AltScreen = true
	v.MouseMode = tea.MouseModeAllMotion
	v.WindowTitle = "demo — control gallery"
	return v
}
