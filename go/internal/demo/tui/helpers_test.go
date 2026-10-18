package tui

import (
	"encoding/json"
	"os"
	"path/filepath"
	"testing"
	"time"

	tea "charm.land/bubbletea/v2"

	"git.wharf.fluidgrid.org/jcz/tictactoe/go/internal/demo/core"
)

// testApp is a joined app with every key at its default (version 1), a
// fixed clock and no network: sent messages land in the outbox.
func testApp(t *testing.T, w, h int) *App {
	t.Helper()
	a := newApp(Options{Server: "http://127.0.0.1:1", Name: "alice", Colors: "truecolor"})
	a.w, a.h = w, h
	a.now = time.Date(2026, 10, 18, 10, 0, 0, 0, time.UTC)
	a.today, _ = core.FromYMD(2026, 10, 18)
	vals := map[string]core.Entry{}
	for _, k := range core.Keys {
		vals[k] = core.Entry{Value: core.Default(k), Version: 1, At: "2026-10-18T09:00:00Z"}
	}
	a.onServerMsg(core.WelcomeMsg{Protocol: 1, Server: core.ServerInfo{Impl: "go", Version: core.Version},
		You: core.You{Conn: 1, Name: "alice", Color: core.PeerColor(1)}, Values: vals})
	tab := "text"
	a.onServerMsg(core.PresenceMsg{Peers: []core.Peer{
		{Conn: 1, Name: "alice", Color: core.PeerColor(1), Client: "tui-go", Tab: &tab},
		{Conn: 2, Name: "bob", Color: core.PeerColor(2), Client: "web-go", Tab: &tab},
	}})
	a.conn = connOnline
	a.outbox = &[]any{}
	a.cmds = nil
	return a
}

func (a *App) screen() string {
	b, _ := a.frame()
	return b.String()
}

// press sends keys through Update ("enter", "ctrl+s", "x", …).
func (a *App) press(keys ...string) {
	m := Model{a}
	for _, k := range keys {
		m.Update(synthKey(k))
	}
}

func (a *App) typeText(s string) {
	for _, r := range s {
		a.press(string(r))
	}
}

// goTab focuses the body of tab n (1-based) from the sidebar, with keys.
func (a *App) goTab(n int) {
	a.inBody = false
	a.press("home")
	for i := 1; i < n; i++ {
		a.press("down")
	}
	a.press("enter")
}

// lastSet returns the last set sent (failing if none).
func (a *App) lastSet(t *testing.T) core.SetMsg {
	t.Helper()
	for i := len(*a.outbox) - 1; i >= 0; i-- {
		if m, ok := (*a.outbox)[i].(core.SetMsg); ok {
			return m
		}
	}
	t.Fatalf("no set sent; outbox: %#v", *a.outbox)
	return core.SetMsg{}
}

func (a *App) setCount() int {
	n := 0
	for _, m := range *a.outbox {
		if _, ok := m.(core.SetMsg); ok {
			n++
		}
	}
	return n
}

// ack answers the last set as the server would (ok, new version).
// ack answers every set in flight as the server would (ok, new version,
// then the changed echo), including sets queued behind them.
func (a *App) ack(t *testing.T) {
	t.Helper()
	for i := 0; len(a.inflight) > 0 && i < 20; i++ {
		for key, p := range a.inflight {
			v := a.vals.Version(key) + 1
			a.onServerMsg(core.AckMsg{Req: p.req, OK: true, Version: &v})
			a.onServerMsg(core.ChangedMsg{Key: key, Value: p.value, Version: v, By: "alice", Conn: 1, At: "2026-10-18T10:00:00Z"})
			break
		}
	}
}

func decode[T any](t *testing.T, raw json.RawMessage) T {
	t.Helper()
	var v T
	if err := json.Unmarshal(raw, &v); err != nil {
		t.Fatalf("decode %s: %v", raw, err)
	}
	return v
}

// hitAt finds a tab-control hit with part name s (and A when a ≥ 0).
func (a *App) hitAt(t *testing.T, s string, idx int) Hit {
	t.Helper()
	_, hits := a.frame()
	for _, h := range hits {
		if h.Kind == HitCtl && h.S == s && (idx < 0 || h.A == idx) {
			return h
		}
	}
	t.Fatalf("no %s hit %d", s, idx)
	return Hit{}
}

func (a *App) click(x, y int) {
	m := Model{a}
	m.Update(tea.MouseClickMsg{X: x, Y: y, Button: tea.MouseLeft})
	m.Update(tea.MouseReleaseMsg{X: x, Y: y, Button: tea.MouseLeft})
}

// dump writes a frame to $DEMO_TUI_DUMP/name.txt for eyeballing.
func dump(t *testing.T, a *App, name string) {
	dir := os.Getenv("DEMO_TUI_DUMP")
	if dir == "" {
		return
	}
	_ = os.WriteFile(filepath.Join(dir, name+".txt"), []byte(a.screen()), 0o644)
	b, _ := a.frame()
	_ = os.WriteFile(filepath.Join(dir, name+".ans"), []byte(b.Render(true)), 0o644)
}
