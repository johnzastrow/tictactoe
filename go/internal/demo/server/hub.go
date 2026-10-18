// Package server is the demo server: a single-goroutine hub that owns the
// store and every connection's outbound queue, plus the net/http +
// WebSocket adapter (docs/demo/protocol.md).
package server

import (
	"context"
	"encoding/json"
	"log/slog"
	"sort"
	"time"

	"git.wharf.fluidgrid.org/jcz/tictactoe/go/internal/demo/core"
	"git.wharf.fluidgrid.org/jcz/tictactoe/go/internal/demo/store"
)

// outboundDepth: a client that falls this far behind is disconnected (it
// reconnects and gets a fresh welcome).
const outboundDepth = 512

// toastEvery: at most one toast per connection per this interval (§1).
const toastEvery = 2 * time.Second

type conn struct {
	name      string
	color     string
	client    string
	tab       *string
	lastToast time.Time
	out       chan []byte
}

// state is owned by the hub goroutine; only commands touch it.
type state struct {
	store *store.Store
	info  core.ServerInfo
	conns map[uint64]*conn
	next  uint64
	log   *slog.Logger
	now   func() time.Time
}

// Hub is a handle to the hub goroutine. Commands are closures that run on
// it one at a time, in order, so nothing needs a lock.
type Hub struct {
	cmds chan func(*state)
	done chan struct{}
}

func StartHub(ctx context.Context, st *store.Store, info core.ServerInfo, log *slog.Logger) *Hub {
	h := &Hub{cmds: make(chan func(*state), 1024), done: make(chan struct{})}
	s := &state{store: st, info: info, conns: map[uint64]*conn{}, next: 1, log: log, now: time.Now}
	go func() {
		defer close(h.done)
		for {
			select {
			case <-ctx.Done():
				for id := range s.conns {
					s.drop(id)
				}
				return
			case cmd := <-h.cmds:
				cmd(s)
			}
		}
	}()
	return h
}

// do queues a command without waiting.
func (h *Hub) do(f func(*state)) {
	select {
	case h.cmds <- f:
	case <-h.done:
	}
}

// call runs a command and waits for it; false if the hub has stopped.
func (h *Hub) call(f func(*state)) bool {
	finished := make(chan struct{})
	select {
	case h.cmds <- func(s *state) { f(s); close(finished) }:
	case <-h.done:
		return false
	}
	select {
	case <-finished:
		return true
	case <-h.done:
		return false
	}
}

// Connect registers a socket: welcome is queued on out first, then
// presence goes to everyone (§2).
func (h *Hub) Connect(name, client string, out chan []byte) (id uint64, ok bool) {
	h.call(func(s *state) {
		values, err := s.store.All()
		if err != nil {
			s.log.Error("loading values", "err", err)
			return
		}
		id = s.next
		s.next++
		c := &conn{name: name, color: core.PeerColor(id), client: client, out: out}
		welcome, err := core.EncodeServerMsg(core.WelcomeMsg{
			Protocol: core.ProtocolVersion, Server: s.info,
			You:    core.You{Conn: id, Name: name, Color: c.color},
			Values: values,
		})
		if err != nil {
			s.log.Error("encode welcome", "err", err)
			return
		}
		out <- welcome
		s.conns[id] = c
		s.log.Info("connected", "conn", id, "name", name, "client", client)
		ok = true
		s.broadcastPresence()
	})
	return
}

func (h *Hub) Disconnect(id uint64) {
	h.do(func(s *state) {
		if _, ok := s.conns[id]; ok {
			s.drop(id)
			s.log.Info("disconnected", "conn", id)
			s.broadcastPresence()
		}
	})
}

// Client handles a parsed client message.
func (h *Hub) Client(id uint64, msg any) { h.do(func(s *state) { s.client(id, msg) }) }

// Reply sends one message to one connection, in mailbox order.
func (h *Hub) Reply(id uint64, msg any) { h.do(func(s *state) { s.sendTo(id, msg) }) }

// ---- on the hub goroutine ----

func (s *state) drop(id uint64) {
	if c, ok := s.conns[id]; ok {
		delete(s.conns, id)
		close(c.out) // ends the writer, which closes the socket
	}
}

func (s *state) client(id uint64, msg any) {
	c, ok := s.conns[id]
	if !ok {
		return
	}
	switch m := msg.(type) {
	case core.SetMsg:
		s.set(id, c, m)
	case core.ResetMsg:
		s.reset(id, c, m)
	case core.FocusMsg:
		// an unknown tab id is ignored (focus has no reply)
		if !core.ValidTab(m.Tab) {
			return
		}
		tab := m.Tab
		c.tab = &tab
		s.broadcastPresence()
	case core.SendToastMsg:
		text, err := core.CheckToast(m.Text)
		if err != nil {
			s.sendTo(id, ackErr(m.Req, err.Error()))
			return
		}
		now := s.now()
		if !c.lastToast.IsZero() && now.Sub(c.lastToast) < toastEvery {
			s.sendTo(id, ackErr(m.Req, "too many toasts: one every 2 seconds"))
			return
		}
		c.lastToast = now
		// broadcast first, then ack (§3)
		s.broadcast(core.ToastMsg{Text: text, By: c.name, Conn: id})
		s.sendTo(id, core.AckMsg{Req: m.Req, OK: true})
	case core.PingMsg:
		s.sendTo(id, core.PongMsg{})
	}
}

func ackErr(req uint64, msg string) core.AckMsg { return core.AckMsg{Req: req, Error: msg} }

// set applies protocol §3's set rules in order: unknown key, version
// conflict, invalid value, cross-key rule; then store, ack, broadcast.
func (s *state) set(id uint64, c *conn, m core.SetMsg) {
	if !core.IsKey(m.Key) {
		s.sendTo(id, ackErr(m.Req, "unknown key"))
		return
	}
	cur, err := s.store.Get(m.Key)
	if err != nil {
		s.log.Error("store", "err", err)
		s.sendTo(id, ackErr(m.Req, "internal error"))
		return
	}
	if !m.Force && m.Base != cur.Version {
		s.sendTo(id, core.AckMsg{Req: m.Req, Conflict: true, Current: &cur})
		return
	}
	value, err := core.Normalize(m.Key, m.Value)
	if err != nil {
		s.sendTo(id, ackErr(m.Req, err.Error()))
		return
	}
	other := func(key string) json.RawMessage {
		e, err := s.store.Get(key)
		if err != nil {
			return nil
		}
		return e.Value
	}
	if err := core.CheckCross(m.Key, value, other); err != nil {
		s.sendTo(id, ackErr(m.Req, err.Error()))
		return
	}
	next := core.Entry{Value: value, Version: cur.Version + 1, By: c.name, At: core.FormatTS(s.now())}
	if err := s.store.Put(m.Key, next); err != nil {
		s.log.Error("store", "err", err)
		s.sendTo(id, ackErr(m.Req, "internal error"))
		return
	}
	s.sendTo(id, core.AckMsg{Req: m.Req, OK: true, Version: &next.Version})
	s.broadcast(changed(m.Key, next, id))
}

// reset restores the listed keys (all when empty); any unknown key fails
// the whole request.
func (s *state) reset(id uint64, c *conn, m core.ResetMsg) {
	var keys []string
	seen := map[string]bool{}
	for _, k := range m.Keys {
		if !core.IsKey(k) {
			s.sendTo(id, ackErr(m.Req, "unknown key "+truncate(k, 40)))
			return
		}
		if !seen[k] {
			seen[k] = true
			keys = append(keys, k)
		}
	}
	if len(keys) == 0 {
		keys = core.Keys
	}
	entries, err := s.store.Reset(keys, c.name, core.FormatTS(s.now()))
	if err != nil {
		s.log.Error("store", "err", err)
		s.sendTo(id, ackErr(m.Req, "internal error"))
		return
	}
	s.sendTo(id, core.AckMsg{Req: m.Req, OK: true})
	for i, k := range keys {
		s.broadcast(changed(k, entries[i], id))
	}
}

func changed(key string, e core.Entry, conn uint64) core.ChangedMsg {
	return core.ChangedMsg{Key: key, Value: e.Value, Version: e.Version, By: e.By, Conn: conn, At: e.At}
}

func truncate(s string, n int) string {
	if r := []rune(s); len(r) > n {
		return string(r[:n]) + "…"
	}
	return s
}

func (s *state) sendTo(id uint64, msg any) {
	c, ok := s.conns[id]
	if !ok {
		return
	}
	b, err := core.EncodeServerMsg(msg)
	if err != nil {
		s.log.Error("encode", "err", err)
		return
	}
	select {
	case c.out <- b:
	default:
		s.drop(id)
		s.broadcastPresence()
	}
}

func (s *state) sortedConns() []uint64 {
	ids := make([]uint64, 0, len(s.conns))
	for id := range s.conns {
		ids = append(ids, id)
	}
	sort.Slice(ids, func(i, j int) bool { return ids[i] < ids[j] })
	return ids
}

func (s *state) broadcast(msg any) {
	b, err := core.EncodeServerMsg(msg)
	if err != nil {
		s.log.Error("encode", "err", err)
		return
	}
	var slow []uint64
	for _, id := range s.sortedConns() {
		select {
		case s.conns[id].out <- b:
		default:
			slow = append(slow, id)
		}
	}
	for _, id := range slow {
		s.log.Warn("client too slow or gone; disconnecting", "conn", id)
		s.drop(id)
	}
	if len(slow) > 0 {
		s.broadcastPresence()
	}
}

func (s *state) broadcastPresence() {
	peers := []core.Peer{}
	for _, id := range s.sortedConns() {
		c := s.conns[id]
		peers = append(peers, core.Peer{Conn: id, Name: c.name, Color: c.color, Client: c.client, Tab: c.tab})
	}
	b, _ := core.EncodeServerMsg(core.PresenceMsg{Peers: peers})
	// presence is best-effort: a full queue just misses this update
	for _, c := range s.conns {
		select {
		case c.out <- b:
		default:
		}
	}
}
