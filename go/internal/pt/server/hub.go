// Package server is the pt server: a single-goroutine hub that owns the
// store and every connection's outbound queue, plus the net/http +
// WebSocket adapter (docs/pt/protocol.md).
package server

import (
	"context"
	"log/slog"
	"sort"

	"git.wharf.fluidgrid.org/jcz/tictactoe/go/internal/pt/core"
	"git.wharf.fluidgrid.org/jcz/tictactoe/go/internal/pt/store"
)

// outboundDepth: a client that falls this far behind is disconnected (it
// reconnects and gets a fresh snapshot).
const outboundDepth = 512

type conn struct {
	user    core.User
	client  string
	session string
	view    *string
	focus   *core.Focus
	out     chan []byte
}

// state is owned by the hub goroutine; only commands touch it.
type state struct {
	store *store.Store
	info  core.ServerInfo
	conns map[uint64]*conn
	next  uint64
	log   *slog.Logger
}

// Hub is a handle to the hub goroutine. Commands are closures that run on
// it one at a time, in order, so nothing needs a lock.
type Hub struct {
	cmds chan func(*state)
	done chan struct{}
}

func StartHub(ctx context.Context, st *store.Store, info core.ServerInfo, log *slog.Logger) *Hub {
	h := &Hub{cmds: make(chan func(*state), 1024), done: make(chan struct{})}
	s := &state{store: st, info: info, conns: map[uint64]*conn{}, next: 1, log: log}
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

func (h *Hub) LoginLookup(username string) (u *core.User, hash string) {
	h.call(func(s *state) {
		var err error
		if u, hash, err = s.store.UserForLogin(username); err != nil {
			s.log.Error("login lookup", "err", err)
			u = nil
		}
	})
	return
}

func (h *Hub) CreateSession(tokenHash string, userID int64, client string) (ok bool) {
	h.call(func(s *state) { ok = s.store.CreateSession(tokenHash, userID, client) == nil })
	return
}

func (h *Hub) Session(tokenHash string) (u *core.User) {
	h.call(func(s *state) {
		var err error
		if u, err = s.store.SessionUser(tokenHash); err != nil {
			u = nil
		}
	})
	return
}

func (h *Hub) Logout(tokenHash string) {
	h.do(func(s *state) {
		_ = s.store.DeleteSession(tokenHash)
		changed := false
		for id, c := range s.conns {
			if c.session == tokenHash {
				s.drop(id)
				changed = true
			}
		}
		if changed {
			s.broadcastPresence()
		}
	})
}

// Connect registers a socket; welcome and snapshot are queued on out first.
func (h *Hub) Connect(user core.User, client, session string, out chan []byte) (id uint64, ok bool) {
	h.call(func(s *state) {
		snap, err := s.store.Snapshot()
		if err != nil {
			s.log.Error("snapshot failed", "err", err)
			return
		}
		id = s.next
		s.next++
		welcome, _ := core.EncodeServerMsg(core.WelcomeMsg{Protocol: core.ProtocolVersion, Server: s.info, Me: user, Conn: id})
		snapshot, _ := core.EncodeServerMsg(core.SnapshotMsg{Data: snap})
		out <- welcome
		out <- snapshot
		s.log.Info("connected", "conn", id, "user", user.Username, "client", client)
		s.conns[id] = &conn{user: user, client: client, session: session, out: out}
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

// PasswordInfo: the actor's fresh user row and the target's hash.
func (h *Hub) PasswordInfo(id uint64, target int64) (actor *core.User, hash string, found bool) {
	h.call(func(s *state) {
		c, ok := s.conns[id]
		if !ok {
			return
		}
		actor, _ = s.store.User(c.user.ID)
		hash, found, _ = s.store.PasswordHash(target)
	})
	return
}

func (h *Hub) SetPasswordHash(id uint64, req uint64, target int64, hash string) {
	h.do(func(s *state) {
		c, ok := s.conns[id]
		if !ok {
			return
		}
		res := core.ResultMsg{Req: req}
		switch {
		case c.user.ID != target && c.user.Role != core.RoleAdmin:
			res.Error = core.NewError("forbidden", "you can only change your own password")
		case s.store.SetPasswordHash(target, hash) != nil:
			res.Error = core.NewError("internal", "internal error")
		default:
			// end the target's other sessions (the caller's own stays)
			_ = s.store.DeleteUserSessions(target, c.session)
			s.kick(target, c.session)
			res.OK = true
		}
		s.sendTo(id, res)
	})
}

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
	user := c.user
	switch m := msg.(type) {
	case core.MutateMsg:
		if len(m.Ops) > core.MaxOps {
			s.sendTo(id, core.ResultMsg{Req: m.Req, Error: core.NewError("bad_request", "at most 2000 ops per mutate")})
			return
		}
		s.finish(id, m.Req, s.store.Mutate(&user, m.Ops))
	case core.UndoMsg:
		s.finish(id, m.Req, s.store.Undo(&user))
	case core.RedoMsg:
		s.finish(id, m.Req, s.store.Redo(&user))
	case core.FocusMsg:
		view := m.View
		if view != nil && len(*view) > 32 {
			view = nil
		}
		field := m.Field
		if field != nil && len(*field) > 64 {
			field = nil
		}
		var focus *core.Focus
		if m.Entity != nil && m.ID != nil {
			focus = &core.Focus{Entity: *m.Entity, ID: *m.ID, Field: field}
		}
		if eqStr(c.view, view) && eqFocus(c.focus, focus) {
			return
		}
		c.view, c.focus = view, focus
		s.broadcastPresence()
	case core.ResyncMsg:
		snap, err := s.store.Snapshot()
		if err != nil {
			s.log.Error("snapshot failed", "err", err)
			return
		}
		s.sendTo(id, core.SnapshotMsg{Data: snap})
	case core.PingMsg:
		s.sendTo(id, core.PongMsg{})
	}
}

func eqStr(a, b *string) bool {
	return (a == nil && b == nil) || (a != nil && b != nil && *a == *b)
}

func eqFocus(a, b *core.Focus) bool {
	if a == nil || b == nil {
		return a == nil && b == nil
	}
	return a.Entity == b.Entity && a.ID == b.ID && eqStr(a.Field, b.Field)
}

func (s *state) finish(id uint64, req uint64, out store.Outcome) {
	out.Result.Req = req
	s.sendTo(id, out.Result)
	if out.Changes != nil {
		// role / name changes take effect on live connections immediately
		for _, u := range out.Changes.Upserts.Users {
			for _, c := range s.conns {
				if c.user.ID == u.ID {
					c.user = u
				}
			}
		}
		s.broadcast(*out.Changes)
	}
	for _, k := range out.Kick {
		s.kick(k.UserID, k.Keep)
	}
}

// kick closes every connection of a user except one session.
func (s *state) kick(userID int64, keep string) {
	changed := false
	for id, c := range s.conns {
		if c.user.ID == userID && (keep == "" || c.session != keep) {
			s.drop(id)
			changed = true
		}
	}
	if changed {
		s.broadcastPresence()
	}
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
		peers = append(peers, core.Peer{Conn: id, UserID: c.user.ID, Client: c.client, View: c.view, Focus: c.focus})
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
