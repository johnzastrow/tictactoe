package tui

// The application model. Bubble Tea owns the loop; the model holds a
// pointer to App so the many sub-states (grids, editors, dialogs) can be
// updated in place. View only reads state (see render.go).

import (
	"context"
	"fmt"
	"strings"
	"time"

	tea "charm.land/bubbletea/v2"

	"git.wharf.fluidgrid.org/jcz/tictactoe/go/internal/pt/client"
	"git.wharf.fluidgrid.org/jcz/tictactoe/go/internal/pt/core"
)

const (
	tickEvery  = 200 * time.Millisecond
	toastFor   = 4 * time.Second
	focusDelay = 150 * time.Millisecond
	clientID   = "tui-go"
)

type phase int

const (
	phLogin phase = iota
	phConnecting
	phMain
)

type connState int

const (
	connConnecting connState = iota
	connOnline
	connReconnecting
)

const (
	vProjects = iota
	vTasks
	vBoard
	vTimeline
	vDashboard
	vActivity
	vTemplates
	vFields
	vUsers
)

var viewKeys = []string{"projects", "tasks", "board", "timeline", "dashboard", "activity", "templates", "fields", "users"}

type toast struct {
	text string
	kind int // 0 info, 1 ok, 2 warn, 3 err
	at   time.Time
}

type conflictInfo struct {
	current any
	by      *int64
}

type pendingReq struct {
	cells []cellKey
	label string
	onOK  func(a *App, res core.ResultMsg) tea.Cmd
	// conflicts are reported to this form instead of the grid
	form *form
}

// cellEdit is an open in-cell editor.
type cellEdit struct {
	in   *Input
	g    *Grid
	key  cellKey
	base any
}

type dragState struct {
	kind   HitKind
	x0, y0 int
	a, b   int
	moved  bool
	x, y   int
}

type App struct {
	opts      Options
	cl        *client.Client
	sess      *client.Session
	ctx       context.Context
	cancel    context.CancelFunc
	rep       *core.Replica
	w, h      int
	theme     int
	truecolor bool
	now       time.Time

	ph       phase
	login    *form
	loginErr string
	conn     connState
	connErr  string
	health   string

	view       int
	pane       int // 0 main, 1 detail
	filterOn   bool
	queries    [9]*queryBar
	detail     [9]bool
	grids      map[string]*Grid
	board      *boardState
	tl         *timelineState
	dash       *dashState
	editor     *cellEdit
	overlays   []overlay
	toasts     []toast
	status     string
	statusKind int

	req       uint64
	pending   map[uint64]*pendingReq
	pendCells map[cellKey]bool
	conflicts map[cellKey]conflictInfo
	flash     map[core.RowKey]time.Time

	clip       string
	focusSent  string
	focusAt    time.Time
	focusCur   string
	saved      []savedFilter
	drag       *dragState
	lastClick  time.Time
	lastClickH Hit
	quitting   bool
	// wantReveal: jump to a row once it arrives (results come before changes).
	wantReveal *revealReq
	// outbox captures sent messages when there is no session (tests).
	outbox *[]any
}

type Model struct{ a *App }

type revealReq struct {
	view int
	grid string
	id   int64
}

// messages
type (
	netMsg struct {
		ev client.Event
		ok bool
	}
	loginMsg struct {
		res core.LoginResponse
		err error
	}
	tickMsg    struct{}
	sendErrMsg struct{ err error }
	logoutMsg  struct{}
)

func newApp(opts Options, cl *client.Client) *App {
	ctx, cancel := context.WithCancel(context.Background())
	a := &App{
		opts: opts, cl: cl, ctx: ctx, cancel: cancel,
		theme: themeIndex(opts.Theme), truecolor: detectTruecolor(), now: time.Now(),
		pending: map[uint64]*pendingReq{}, pendCells: map[cellKey]bool{},
		conflicts: map[cellKey]conflictInfo{}, flash: map[core.RowKey]time.Time{},
		grids: map[string]*Grid{},
	}
	if opts.ForceColors == "256" {
		a.truecolor = false
	} else if opts.ForceColors == "truecolor" {
		a.truecolor = true
	}
	a.rep = core.NewReplica(todayLocal())
	for i := range a.queries {
		a.queries[i] = newQueryBar(i)
	}
	a.detail[vProjects], a.detail[vTasks], a.detail[vTemplates] = true, true, true
	a.saved = loadSaved()
	a.setupViews()
	if cl.Token() != "" {
		a.ph = phConnecting
	} else {
		a.ph = phLogin
	}
	a.login = newLoginForm(a)
	return a
}

func todayLocal() core.Date {
	y, m, d := time.Now().Date()
	t, _ := core.FromYMD(y, int(m), d)
	return t
}

func (m Model) Init() tea.Cmd {
	a := m.a
	cmds := []tea.Cmd{tick(), tea.RequestBackgroundColor}
	if a.ph == phConnecting {
		cmds = append(cmds, a.startSession())
	}
	return tea.Batch(cmds...)
}

func tick() tea.Cmd {
	return tea.Tick(tickEvery, func(time.Time) tea.Msg { return tickMsg{} })
}

func (a *App) startSession() tea.Cmd {
	if a.sess != nil {
		return nil
	}
	a.conn = connConnecting
	a.sess = a.cl.Start(a.ctx, clientID)
	return a.waitNet()
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

// send writes a client message off the UI loop.
func (a *App) send(msg any) tea.Cmd {
	s, ctx := a.sess, a.ctx
	if s == nil {
		if a.outbox != nil {
			*a.outbox = append(*a.outbox, msg)
		}
		return nil
	}
	return func() tea.Msg {
		if err := s.Send(ctx, msg); err != nil {
			return sendErrMsg{err}
		}
		return nil
	}
}

func (a *App) online() bool { return a.conn == connOnline && a.rep.IsLoaded() }

// mutate sends ops; cells show as pending until the result arrives.
func (a *App) mutate(ops []core.Op, cells []cellKey, label string, onOK func(*App, core.ResultMsg) tea.Cmd) tea.Cmd {
	if !a.online() {
		a.toast("Offline: changes are disabled until the connection is back", 3)
		return nil
	}
	if !a.rep.CanEdit() {
		a.toast("You have read-only access (viewer)", 2)
		return nil
	}
	a.req++
	a.pending[a.req] = &pendingReq{cells: cells, label: label, onOK: onOK}
	for _, c := range cells {
		a.pendCells[c] = true
		delete(a.conflicts, c)
	}
	return a.send(core.MutateMsg{Req: a.req, Ops: ops})
}

// mutateForm is mutate for a dialog: the form shows busy, errors and
// per-field conflicts, and closes itself on success.
func (a *App) mutateForm(f *form, ops []core.Op, cells []cellKey, label string, onOK func(*App, core.ResultMsg) tea.Cmd) tea.Cmd {
	before := a.req
	cmd := a.mutate(ops, cells, label, onOK)
	if a.req != before {
		a.pending[a.req].form = f
		f.busy = true
		f.msg = ""
	}
	return cmd
}

func (a *App) toast(text string, kind int) {
	a.toasts = append(a.toasts, toast{text, kind, a.now})
	if len(a.toasts) > 3 {
		a.toasts = a.toasts[len(a.toasts)-3:]
	}
	a.setStatus(text, kind)
}

func (a *App) setStatus(text string, kind int) {
	a.status, a.statusKind = text, kind
}

func (m Model) Update(msg tea.Msg) (tea.Model, tea.Cmd) {
	a := m.a
	cmd := a.update(msg)
	a.afterUpdate()
	return m, cmd
}

func (a *App) update(msg tea.Msg) tea.Cmd {
	switch msg := msg.(type) {
	case tea.WindowSizeMsg:
		a.w, a.h = msg.Width, msg.Height
	case tickMsg:
		a.now = time.Now()
		a.rep.SetToday(todayLocal())
		kept := a.toasts[:0]
		for _, t := range a.toasts {
			if a.now.Sub(t.at) < toastFor {
				kept = append(kept, t)
			}
		}
		a.toasts = kept
		for k, t := range a.flash {
			if a.now.Sub(t) > 2*time.Second {
				delete(a.flash, k)
			}
		}
		return tea.Batch(tick(), a.maybeSendFocus())
	case netMsg:
		if !msg.ok {
			a.sess = nil
			return nil
		}
		return tea.Batch(a.onNet(msg.ev), a.waitNet())
	case loginMsg:
		return a.onLogin(msg)
	case logoutMsg:
		return nil
	case sendErrMsg:
		a.toast("Not sent: "+msg.err.Error(), 3)
	case tea.PasteMsg:
		return a.onPaste(msg.Content)
	case tea.KeyPressMsg:
		return a.onKey(msg)
	case tea.MouseMsg:
		// Resolve against the same frame View draws (frame is pure), in order.
		_, hits := a.frame()
		mm := msg.Mouse()
		h, ok := topHit(hits, mm.X, mm.Y)
		return a.onMouse(hitMsg{m: mm, ev: msg, hit: h, ok: ok, hits: hits})
	case tea.ClipboardMsg:
		if msg.Content != "" {
			return a.onPaste(msg.Content)
		}
	}
	return nil
}

// onNet applies a server event.
func (a *App) onNet(ev client.Event) tea.Cmd {
	switch ev.Kind {
	case client.Connected:
		a.conn = connOnline
		a.connErr = ""
	case client.Disconnected:
		if a.conn == connOnline {
			a.toast("Connection lost; reconnecting…", 2)
		}
		a.conn = connReconnecting
		if ev.Err != nil {
			a.connErr = ev.Err.Error()
		}
		a.editor = nil
		// requests in flight will never be answered
		for req := range a.pending {
			delete(a.pending, req)
		}
		a.pendCells = map[cellKey]bool{}
	case client.AuthFailed:
		_ = client.ForgetToken(a.opts.Server)
		a.cl.SetToken("")
		a.sess = nil
		a.ph = phLogin
		a.loginErr = "Your session has expired. Please log in again."
		a.login = newLoginForm(a)
	case client.Message:
		return a.onServerMsg(ev.Msg)
	}
	return nil
}

func (a *App) onServerMsg(msg any) tea.Cmd {
	switch m := msg.(type) {
	case core.ResultMsg:
		return a.onResult(m)
	case core.ErrorMsg:
		a.toast("Server: "+m.Message, 3)
		return nil
	case core.PongMsg:
		return nil
	}
	applied := a.rep.Apply(msg)
	if _, ok := msg.(core.SnapshotMsg); ok {
		a.ph = phMain
		a.focusSent = ""
		a.refreshColumns()
	}
	if c, ok := msg.(core.Changes); ok {
		if len(c.Upserts.CustomFields) > 0 || len(c.Deletes.CustomFields) > 0 {
			a.refreshColumns()
		}
		mine := applied.By != nil && *applied.By == a.rep.MeID()
		for _, k := range applied.Upserted {
			if !mine {
				a.flash[k] = a.now
			}
			// a remote change resolves conflict markers on that row
			for ck := range a.conflicts {
				if ck.E == k.Entity && ck.ID == k.ID && !mine {
					_ = ck
				}
			}
		}
		if !mine && applied.By != nil {
			a.toastRemote(applied)
		}
	}
	a.rebuild()
	if w := a.wantReveal; w != nil && a.rep.Exists(a.grids[w.grid].Ent, w.id) {
		a.wantReveal = nil
		a.reveal(w.view, w.grid, w.id)
	}
	return nil
}

// toastRemote summarises someone else's change ("bob: NWR-004 #5 status: …").
func (a *App) toastRemote(ap core.Applied) {
	who := a.rep.Username(ap.By)
	for _, act := range ap.Activity {
		if act.State == core.StateDone && act.Label != "" {
			a.toasts = append(a.toasts, toast{who + ": " + act.Label, 0, a.now})
			if len(a.toasts) > 3 {
				a.toasts = a.toasts[len(a.toasts)-3:]
			}
			return
		}
	}
}

func (a *App) onResult(res core.ResultMsg) tea.Cmd {
	p := a.pending[res.Req]
	delete(a.pending, res.Req)
	if p != nil {
		for _, c := range p.cells {
			delete(a.pendCells, c)
		}
	}
	if len(res.Conflicts) > 0 {
		msgs := []string{}
		for _, c := range res.Conflicts {
			key := cellKey{c.Entity, c.ID, c.Field}
			if p != nil && p.form != nil {
				p.form.conflict(a, c)
			} else {
				a.conflicts[key] = conflictInfo{c.Current, c.By}
			}
			who := "someone"
			if c.By != nil {
				who = a.rep.Username(c.By)
			}
			label := c.Field
			if m := core.Field(c.Entity, c.Field); m != nil {
				label = m.Label
			} else if id, ok := core.CfID(c.Field); ok {
				if cf := a.rep.CustomFields[id]; cf != nil {
					label = cf.Name
				}
			}
			msgs = append(msgs, fmt.Sprintf("%s changed %s to %s; your edit wasn't applied", who, label,
				core.DisplayValue(a.rep, c.Entity, c.Field, c.Current)))
		}
		a.toast("⚠ "+msgs[0], 2)
	}
	if !res.OK {
		e := "failed"
		if res.Error != nil {
			e = res.Error.Message
			switch res.Error.Code {
			case "nothing_to_undo", "nothing_to_redo":
				a.toast(strings.ToUpper(e[:1])+e[1:], 0)
				return nil
			}
		}
		if p != nil && p.form != nil {
			p.form.msg = e
			p.form.busy = false
			return nil
		}
		a.toast("✗ "+e, 3)
		return nil
	}
	if p != nil && p.form != nil {
		p.form.busy = false
		if len(res.Conflicts) == 0 {
			a.closeOverlay(p.form)
		}
	}
	if len(res.Conflicts) == 0 && p != nil && p.label != "" {
		a.setStatus("✓ "+p.label, 1)
	}
	if p != nil && p.onOK != nil {
		return p.onOK(a, res)
	}
	return nil
}

// afterUpdate keeps scroll positions valid and tracks presence focus.
func (a *App) afterUpdate() {
	if a.ph != phMain {
		return
	}
	lay := a.layout()
	for name, r := range lay.grids {
		if g := a.grids[name]; g != nil {
			g.Scroll(r)
		}
	}
	f := a.focusString()
	if f != a.focusCur {
		a.focusCur = f
		a.focusAt = a.now
	}
}

// focusString encodes view/entity/id/field for presence.
func (a *App) focusString() string {
	view := viewKeys[a.view]
	e, id, field := a.currentFocus()
	return fmt.Sprintf("%s|%s|%d|%s", view, e, id, field)
}

func (a *App) currentFocus() (core.Entity, int64, string) {
	switch a.view {
	case vBoard:
		if id := a.board.curID(a); id != 0 {
			return core.EntTask, id, ""
		}
		return "", 0, ""
	case vTimeline:
		if r, ok := a.tl.cur(a); ok {
			return r.ent, r.id, ""
		}
		return "", 0, ""
	case vDashboard:
		return "", 0, ""
	}
	g := a.activeGrid()
	if g == nil || g.Ent == core.EntUser && g.Name == "activity" {
		return "", 0, ""
	}
	id := g.CurID()
	if id == 0 {
		return "", 0, ""
	}
	field := ""
	if c, ok := g.CurCol(); ok {
		field = c.Field
	}
	return g.Ent, id, field
}

func (a *App) maybeSendFocus() tea.Cmd {
	if a.ph != phMain || !a.online() || a.focusCur == a.focusSent || a.now.Sub(a.focusAt) < focusDelay {
		return nil
	}
	a.focusSent = a.focusCur
	view := viewKeys[a.view]
	e, id, field := a.currentFocus()
	msg := core.FocusMsg{View: &view}
	if id != 0 {
		ent := e
		msg.Entity, msg.ID = &ent, &id
		if field != "" {
			msg.Field = &field
		}
	}
	return a.send(msg)
}

func (a *App) onLogin(m loginMsg) tea.Cmd {
	if m.err != nil {
		a.loginErr = m.err.Error()
		if client.IsUnauthorized(m.err) {
			a.loginErr = "Wrong username or password."
		}
		a.login.busy = false
		return nil
	}
	a.loginErr = ""
	if a.login.remember() {
		if err := client.SaveToken(a.opts.Server, m.res.Token); err != nil {
			a.toast("Couldn't save the session: "+err.Error(), 2)
		}
	}
	a.ph = phConnecting
	return a.startSession()
}

// View renders the frame. Mouse events are resolved in Update against the
// same (pure) frame function, so clicks hit exactly what was drawn.
func (m Model) View() tea.View {
	a := m.a
	b, hits := a.frame()
	v := tea.NewView(b.Render(a.truecolor))
	v.AltScreen = true
	v.MouseMode = tea.MouseModeAllMotion
	v.ReportFocus = false
	v.WindowTitle = "pt — projects & tasks"
	_ = hits
	return v
}
