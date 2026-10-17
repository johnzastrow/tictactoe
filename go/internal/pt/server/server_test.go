package server_test

import (
	"context"
	"encoding/pem"
	"io"
	"log/slog"
	"net/http"
	"net/http/httptest"
	"os"
	"strings"
	"testing"
	"time"

	"github.com/coder/websocket"

	"git.wharf.fluidgrid.org/jcz/tictactoe/go/internal/pt/client"
	"git.wharf.fluidgrid.org/jcz/tictactoe/go/internal/pt/core"
	"git.wharf.fluidgrid.org/jcz/tictactoe/go/internal/pt/seed"
	"git.wharf.fluidgrid.org/jcz/tictactoe/go/internal/pt/server"
	"git.wharf.fluidgrid.org/jcz/tictactoe/go/internal/pt/store"
)

const pw = "server-test-pass"

func start(t *testing.T) string {
	t.Helper()
	st, err := store.OpenMemory()
	if err != nil {
		t.Fatal(err)
	}
	if _, err := seed.Seed(st, pw); err != nil {
		t.Fatal(err)
	}
	ctx, cancel := context.WithCancel(context.Background())
	log := slog.New(slog.DiscardHandler)
	srv := httptest.NewServer(server.App(ctx, st, false, log))
	t.Cleanup(func() { srv.Close(); cancel(); st.Close() })
	return srv.URL
}

type tc struct {
	t    *testing.T
	ctx  context.Context
	c    *client.Client
	conn *client.Conn
	r    *core.Replica
	req  uint64
}

func connect(t *testing.T, url, user string) *tc {
	t.Helper()
	ctx, cancel := context.WithTimeout(context.Background(), 30*time.Second)
	t.Cleanup(cancel)
	c, err := client.New(url, "")
	if err != nil {
		t.Fatal(err)
	}
	if _, err := c.Login(ctx, user, pw, "test"); err != nil {
		t.Fatal(user, err)
	}
	conn, err := c.Dial(ctx, "test")
	if err != nil {
		t.Fatal(err)
	}
	x := &tc{t: t, ctx: ctx, c: c, conn: conn, r: core.NewReplica(store.Today())}
	for !x.r.IsLoaded() {
		x.r.Apply(x.recv())
	}
	return x
}

func (x *tc) recv() any {
	x.t.Helper()
	m, err := x.conn.Recv(x.ctx)
	if err != nil {
		x.t.Fatal(err)
	}
	return m
}

func (x *tc) result(msg func(req uint64) any) core.ResultMsg {
	x.t.Helper()
	x.req++
	req := x.req
	if err := x.conn.Send(x.ctx, msg(req)); err != nil {
		x.t.Fatal(err)
	}
	for {
		if r, ok := x.recv().(core.ResultMsg); ok && r.Req == req {
			return r
		}
	}
}

func (x *tc) changes(group int64) core.Changes {
	x.t.Helper()
	for {
		if c, ok := x.recv().(core.Changes); ok && c.Group != nil && *c.Group == group {
			return c
		}
	}
}

func task(r *core.Replica, code string) *core.Task {
	for _, id := range core.SortedIDs(r.Tasks) {
		if r.ProjectCode(r.Tasks[id].ProjectID) == code {
			return r.Tasks[id]
		}
	}
	return nil
}

func TestHealthHeadersAndLogin(t *testing.T) {
	url := start(t)
	res, err := http.Get(url + "/api/health")
	if err != nil {
		t.Fatal(err)
	}
	body, _ := io.ReadAll(res.Body)
	res.Body.Close()
	if !strings.Contains(string(body), `"impl":"go"`) || res.Header.Get("X-Content-Type-Options") != "nosniff" ||
		!strings.Contains(res.Header.Get("Content-Security-Policy"), "frame-ancestors 'none'") {
		t.Fatalf("%s %v", body, res.Header)
	}
	c, _ := client.New(url, "")
	ctx := context.Background()
	if _, err := c.Login(ctx, "alice", "wrong-password", ""); !client.IsUnauthorized(err) {
		t.Fatal("bad password must be 401")
	}
	for i := 0; i < 5; i++ {
		_, _ = c.Login(ctx, "probe", "wrong-password", "")
	}
	if _, err := c.Login(ctx, "probe", "wrong-password", ""); err == nil || !strings.Contains(err.Error(), "429") {
		t.Fatalf("throttle: %v", err)
	}
	if _, err := c.Login(ctx, "bob", pw, ""); err != nil {
		t.Fatal(err)
	}
	if u, err := c.Me(ctx); err != nil || u.Username != "bob" {
		t.Fatal("me")
	}
	if err := c.Logout(ctx); err != nil {
		t.Fatal(err)
	}
	if _, err := c.Me(ctx); !client.IsUnauthorized(err) {
		t.Fatal("logged out")
	}
}

func TestWSAuthAndOrigin(t *testing.T) {
	url := start(t)
	ctx := context.Background()
	c, _ := client.New(url, "")
	if _, err := c.Dial(ctx, "x"); !client.IsUnauthorized(err) {
		t.Fatalf("no session: %v", err)
	}
	if _, err := c.Login(ctx, "carol", pw, ""); err != nil {
		t.Fatal(err)
	}
	h := http.Header{"Authorization": {"Bearer " + c.Token()}, "Origin": {"http://evil.example"}}
	_, res, err := websocket.Dial(ctx, "ws"+strings.TrimPrefix(url, "http")+"/ws", &websocket.DialOptions{HTTPHeader: h})
	if err == nil || res == nil || res.StatusCode != http.StatusForbidden {
		t.Fatalf("cross-origin must be 403: %v", err)
	}
}

func TestBroadcastConflictPresence(t *testing.T) {
	url := start(t)
	a, b := connect(t, url, "alice"), connect(t, url, "dave")
	tk := task(a.r, "NWR-002")
	r := a.result(func(req uint64) any {
		return core.MutateMsg{Req: req, Ops: []core.Op{core.SetOp(core.EntTask, tk.ID, "title", tk.Title, "Renamed")}}
	})
	if !r.OK || r.Group == nil {
		t.Fatalf("%+v", r)
	}
	ch := b.changes(*r.Group)
	b.r.Apply(ch)
	if b.r.Tasks[tk.ID].Title != "Renamed" {
		t.Fatal("broadcast")
	}
	r = b.result(func(req uint64) any {
		return core.MutateMsg{Req: req, Ops: []core.Op{core.SetOp(core.EntTask, tk.ID, "title", tk.Title, "Lost")}}
	})
	if len(r.Conflicts) != 1 || r.Conflicts[0].Current != "Renamed" {
		t.Fatalf("%+v", r)
	}
	view, field := "tasks", "status"
	ent, id := core.EntTask, tk.ID
	_ = a.conn.Send(a.ctx, core.FocusMsg{View: &view, Entity: &ent, ID: &id, Field: &field})
	for {
		if p, ok := b.recv().(core.PresenceMsg); ok {
			found := false
			for _, peer := range p.Peers {
				if peer.Focus != nil && peer.Focus.ID == tk.ID {
					found = true
				}
			}
			if found {
				break
			}
		}
	}
	_ = a.conn.Send(a.ctx, core.PingMsg{})
	for {
		if _, ok := a.recv().(core.PongMsg); ok {
			break
		}
	}
}

func TestBadFrameAndRateLimit(t *testing.T) {
	url := start(t)
	x := connect(t, url, "bob")
	c, _ := client.New(url, "")
	_, _ = c.Login(x.ctx, "bob", pw, "")
	ws, _, err := websocket.Dial(x.ctx, "ws"+strings.TrimPrefix(url, "http")+"/ws",
		&websocket.DialOptions{HTTPHeader: http.Header{"Authorization": {"Bearer " + c.Token()}}})
	if err != nil {
		t.Fatal(err)
	}
	ws.SetReadLimit(64 << 20)
	_ = ws.Write(x.ctx, websocket.MessageText, []byte(`{"type":"nope"}`))
	for {
		_, data, err := ws.Read(x.ctx)
		if err != nil {
			t.Fatal(err)
		}
		if strings.Contains(string(data), `"bad_request"`) {
			break
		}
	}
	// flood: more than the 100-message burst closes the socket with 1008
	for i := 0; i < 300; i++ {
		if ws.Write(x.ctx, websocket.MessageText, []byte(`{"type":"ping"}`)) != nil {
			break
		}
	}
	for {
		_, _, err := ws.Read(x.ctx)
		if err != nil {
			if websocket.CloseStatus(err) != websocket.StatusPolicyViolation {
				t.Fatalf("want 1008, got %v", err)
			}
			break
		}
	}
}

func TestOversizeFrameCloses(t *testing.T) {
	url := start(t)
	x := connect(t, url, "erin")
	big := `{"type":"ping","pad":"` + strings.Repeat("x", core.MaxFrame+10) + `"}`
	c := x.c
	raw, _, err := websocket.Dial(x.ctx, "ws"+strings.TrimPrefix(url, "http")+"/ws",
		&websocket.DialOptions{HTTPHeader: http.Header{"Authorization": {"Bearer " + c.Token()}}})
	if err != nil {
		t.Fatal(err)
	}
	raw.SetReadLimit(64 << 20)
	_ = raw.Write(x.ctx, websocket.MessageText, []byte(big))
	for {
		if _, _, err := raw.Read(x.ctx); err != nil {
			if s := websocket.CloseStatus(err); s != websocket.StatusMessageTooBig && s != websocket.StatusPolicyViolation {
				t.Fatalf("want close, got %v", err)
			}
			break
		}
	}
}

func TestDeactivationKicks(t *testing.T) {
	url := start(t)
	admin := connect(t, url, "admin")
	heidi := connect(t, url, "heidi")
	var hid int64
	for _, u := range admin.r.Users {
		if u.Username == "heidi" {
			hid = u.ID
		}
	}
	r := admin.result(func(req uint64) any {
		return core.MutateMsg{Req: req, Ops: []core.Op{core.SetOp(core.EntUser, hid, "active", true, false)}}
	})
	if !r.OK {
		t.Fatal(r.Error)
	}
	for {
		if _, err := heidi.conn.Recv(heidi.ctx); err != nil {
			break // socket closed
		}
	}
	if _, err := heidi.c.Me(heidi.ctx); !client.IsUnauthorized(err) {
		t.Fatal("deactivated user's session must be gone")
	}
}

func TestSetPassword(t *testing.T) {
	url := start(t)
	x := connect(t, url, "grace")
	me := x.r.MeID()
	wrong := "wrong"
	r := x.result(func(req uint64) any {
		return core.SetPasswordMsg{Req: req, UserID: me, Password: "new-password-1", OldPassword: &wrong}
	})
	if r.Error == nil || r.Error.Code != "invalid" {
		t.Fatalf("%+v", r)
	}
	old := pw
	r = x.result(func(req uint64) any {
		return core.SetPasswordMsg{Req: req, UserID: me, Password: "new-password-1", OldPassword: &old}
	})
	if !r.OK {
		t.Fatal(r.Error)
	}
	c, _ := client.New(url, "")
	if _, err := c.Login(x.ctx, "grace", "new-password-1", ""); err != nil {
		t.Fatal("new password must work", err)
	}
}

func TestRefusesPlaintextOnLAN(t *testing.T) {
	err := server.Serve(context.Background(), server.Config{DB: t.TempDir() + "/x.db", Bind: "0.0.0.0:0"}, slog.New(slog.DiscardHandler))
	if err == nil || !strings.Contains(err.Error(), "without TLS") {
		t.Fatal(err)
	}
}

func TestReconnectingSession(t *testing.T) {
	url := start(t)
	ctx, cancel := context.WithTimeout(context.Background(), 20*time.Second)
	defer cancel()
	c, _ := client.New(url, "")
	if _, err := c.Login(ctx, "alice", pw, ""); err != nil {
		t.Fatal(err)
	}
	s := c.Start(ctx, "test")
	r := core.NewReplica(store.Today())
	for ev := range s.Events() {
		if ev.Kind == client.Message {
			r.Apply(ev.Msg)
			if r.IsLoaded() {
				break
			}
		}
	}
	if err := s.Send(ctx, core.PingMsg{}); err != nil {
		t.Fatal(err)
	}
	_ = c.Logout(ctx)
	for ev := range s.Events() {
		if ev.Kind == client.AuthFailed {
			return // closed by logout, then the redial is refused
		}
	}
	t.Fatal("expected AuthFailed after logout")
}

func TestClientTrustsCACert(t *testing.T) {
	st, _ := store.OpenMemory()
	defer st.Close()
	if _, err := seed.Seed(st, pw); err != nil {
		t.Fatal(err)
	}
	ctx, cancel := context.WithCancel(context.Background())
	defer cancel()
	srv := httptest.NewTLSServer(server.App(ctx, st, true, slog.New(slog.DiscardHandler)))
	defer srv.Close()
	ca := t.TempDir() + "/ca.pem"
	if err := os.WriteFile(ca, pem.EncodeToMemory(&pem.Block{Type: "CERTIFICATE", Bytes: srv.Certificate().Raw}), 0o600); err != nil {
		t.Fatal(err)
	}
	plain, _ := client.New(srv.URL, "")
	if _, err := plain.Health(ctx); err == nil {
		t.Fatal("an unknown CA must be rejected")
	}
	c, err := client.New(srv.URL, ca)
	if err != nil {
		t.Fatal(err)
	}
	if _, err := c.Login(ctx, "alice", pw, "test"); err != nil {
		t.Fatal(err)
	}
	conn, err := c.Dial(ctx, "test")
	if err != nil {
		t.Fatal("wss dial:", err)
	}
	if m, err := conn.Recv(ctx); err != nil {
		t.Fatal(err)
	} else if _, ok := m.(core.WelcomeMsg); !ok {
		t.Fatalf("%T", m)
	}
	conn.Close()
}
