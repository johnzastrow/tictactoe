package server_test

import (
	"context"
	"encoding/json"
	"io"
	"log/slog"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"
	"time"

	"github.com/coder/websocket"

	"git.wharf.fluidgrid.org/jcz/tictactoe/go/internal/demo/client"
	"git.wharf.fluidgrid.org/jcz/tictactoe/go/internal/demo/core"
	"git.wharf.fluidgrid.org/jcz/tictactoe/go/internal/demo/server"
	"git.wharf.fluidgrid.org/jcz/tictactoe/go/internal/demo/store"
)

func start(t *testing.T) string {
	t.Helper()
	st, err := store.OpenMemory()
	if err != nil {
		t.Fatal(err)
	}
	ctx, cancel := context.WithCancel(context.Background())
	log := slog.New(slog.DiscardHandler)
	srv := httptest.NewServer(server.App(ctx, st, false, log))
	t.Cleanup(func() { srv.Close(); cancel(); st.Close() })
	return srv.URL
}

// tc is one test connection with its own copy of the values.
type tc struct {
	t    *testing.T
	ctx  context.Context
	conn *client.Conn
	v    *core.Values
	req  uint64
}

func connect(t *testing.T, url, name string) *tc {
	t.Helper()
	ctx, cancel := context.WithTimeout(context.Background(), 30*time.Second)
	t.Cleanup(cancel)
	c, err := client.New(url, "")
	if err != nil {
		t.Fatal(err)
	}
	conn, err := c.Dial(ctx, "conform", name)
	if err != nil {
		t.Fatal(name, err)
	}
	t.Cleanup(conn.Close)
	x := &tc{t: t, ctx: ctx, conn: conn, v: core.NewValues()}
	// welcome comes first, before any presence
	if _, ok := x.recv().(core.WelcomeMsg); !ok {
		t.Fatal("first message is not welcome")
	}
	return x
}

func (x *tc) recv() any {
	x.t.Helper()
	m, err := x.conn.Recv(x.ctx)
	if err != nil {
		x.t.Fatal(err)
	}
	x.v.Apply(m)
	return m
}

// next returns the next message that isn't presence.
func (x *tc) next() any {
	x.t.Helper()
	for {
		if m := x.recv(); m != nil {
			if _, ok := m.(core.PresenceMsg); !ok {
				return m
			}
		}
	}
}

// presence returns the next presence message.
func (x *tc) presence() core.PresenceMsg {
	x.t.Helper()
	for {
		if p, ok := x.recv().(core.PresenceMsg); ok {
			return p
		}
	}
}

func (x *tc) send(m any) {
	x.t.Helper()
	if err := x.conn.Send(x.ctx, m); err != nil {
		x.t.Fatal(err)
	}
}

func (x *tc) set(key string, base int64, value string) uint64 {
	x.req++
	x.send(core.SetMsg{Req: x.req, Key: key, Base: base, Value: json.RawMessage(value)})
	return x.req
}

func (x *tc) ack(req uint64) core.AckMsg {
	x.t.Helper()
	a, ok := x.next().(core.AckMsg)
	if !ok || a.Req != req {
		x.t.Fatalf("want ack %d, got %#v", req, a)
	}
	return a
}

func (x *tc) changed() core.ChangedMsg {
	x.t.Helper()
	c, ok := x.next().(core.ChangedMsg)
	if !ok {
		x.t.Fatalf("want changed, got %#v", c)
	}
	return c
}

func TestHTTP(t *testing.T) {
	url := start(t)
	res, err := http.Get(url + "/api/health")
	if err != nil {
		t.Fatal(err)
	}
	body, _ := io.ReadAll(res.Body)
	res.Body.Close()
	if strings.TrimSpace(string(body)) != `{"ok":true,"impl":"go","version":"0.5.0","protocol":1}` {
		t.Fatal(string(body))
	}
	h := res.Header
	if h.Get("X-Content-Type-Options") != "nosniff" || h.Get("X-Frame-Options") != "DENY" || h.Get("Referrer-Policy") != "same-origin" ||
		!strings.Contains(h.Get("Content-Security-Policy"), "frame-ancestors 'none'") || h.Get("Strict-Transport-Security") != "" {
		t.Fatal(h)
	}
	for path, want := range map[string]int{"/": 200, "/some/route": 200, "/missing.js": 404, "/api/login": 404, "/api/health/x": 404} {
		res, err := http.Get(url + path)
		if err != nil {
			t.Fatal(err)
		}
		res.Body.Close()
		if res.StatusCode != want {
			t.Errorf("%s: %d, want %d", path, res.StatusCode, want)
		}
	}
	res, _ = http.Post(url+"/api/health", "application/json", nil)
	res.Body.Close()
	if res.StatusCode != http.StatusMethodNotAllowed {
		t.Fatal("POST", res.StatusCode)
	}
}

func dialStatus(t *testing.T, url, query string, hdr http.Header) int {
	t.Helper()
	ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
	defer cancel()
	ws, res, err := websocket.Dial(ctx, strings.Replace(url, "http", "ws", 1)+"/ws"+query, &websocket.DialOptions{HTTPHeader: hdr})
	if err == nil {
		ws.CloseNow()
		return 101
	}
	if res == nil {
		t.Fatal(err)
	}
	return res.StatusCode
}

func TestUpgradeChecks(t *testing.T) {
	url := start(t)
	host := strings.TrimPrefix(url, "http://")
	cases := []struct {
		query  string
		origin string
		want   int
	}{
		{"?client=conform&name=alice", "", 101},
		{"?client=conform&name=%20%20Ada%20L%20", "", 101},
		{"?client=conform&name=alice", "http://" + host, 101},
		{"?client=conform&name=alice", "https://" + host, 101},
		{"?client=conform&name=alice", "http://evil.example", 403},
		{"?client=Conform&name=alice", "", 400},
		{"?client=&name=alice", "", 400},
		{"?name=alice", "", 400},
		{"?client=conform", "", 400},
		{"?client=conform&name=1alice", "", 400},
		{"?client=conform&name=" + strings.Repeat("a", 25), "", 400},
		{"?client=conform&name=al%3Cice", "", 400},
	}
	for _, c := range cases {
		h := http.Header{}
		if c.origin != "" {
			h.Set("Origin", c.origin)
		}
		if got := dialStatus(t, url, c.query, h); got != c.want {
			t.Errorf("%s origin=%q: %d, want %d", c.query, c.origin, got, c.want)
		}
	}
}

func TestEndToEnd(t *testing.T) {
	url := start(t)
	a := connect(t, url, "alice")
	if a.v.You.Conn != 1 || a.v.You.Color != "#e06c75" || a.v.You.Name != "alice" || a.v.Server.Impl != "go" {
		t.Fatalf("%+v %+v", a.v.You, a.v.Server)
	}
	for _, k := range core.Keys {
		e := a.v.Get(k)
		if e.Version != 1 || e.By != "" || string(e.Value) != string(core.Default(k)) {
			t.Fatalf("%s: %+v", k, e)
		}
	}
	if p := a.presence(); len(p.Peers) != 1 || p.Peers[0].Tab != nil {
		t.Fatal(p)
	}
	b := connect(t, url, "  bob  ")
	if b.v.You.Conn != 2 || b.v.You.Name != "bob" || b.v.You.Color != "#61afef" {
		t.Fatal(b.v.You)
	}
	// presence goes to everyone and includes the receiver
	for _, x := range []*tc{a, b} {
		p := x.presence()
		if len(p.Peers) != 2 || p.Peers[1].Name != "bob" || p.Peers[1].Client != "conform" {
			t.Fatal(p)
		}
	}

	// set: ack, then changed to everyone including the sender
	req := a.set("select.status", 1, `" done "`)
	if ack := a.ack(req); !ack.OK || ack.Version == nil || *ack.Version != 2 {
		t.Fatalf("%+v", ack)
	}
	for _, x := range []*tc{a, b} {
		c := x.changed()
		if c.Key != "select.status" || string(c.Value) != `"done"` || c.Version != 2 || c.By != "alice" || c.Conn != 1 || len(c.At) != 20 {
			t.Fatalf("%+v", c)
		}
	}
	// stale base → conflict with the current entry; nothing changes
	req = b.set("select.status", 1, `"review"`)
	ack := b.ack(req)
	if ack.OK || !ack.Conflict || ack.Current == nil || ack.Current.Version != 2 || string(ack.Current.Value) != `"done"` || ack.Current.By != "alice" {
		t.Fatalf("%+v", ack)
	}
	// force skips the check
	b.req++
	b.send(core.SetMsg{Req: b.req, Key: "select.status", Base: 1, Value: json.RawMessage(`"review"`), Force: true})
	if ack := b.ack(b.req); !ack.OK || *ack.Version != 3 {
		t.Fatalf("%+v", ack)
	}
	if c := a.changed(); c.Version != 3 || c.By != "bob" {
		t.Fatal(c)
	}
	b.changed()
	// unknown key / invalid value / cross-key rule → error acks
	for _, c := range []struct{ key, val string }{
		{"no.such", `1`},
		{"number.qty", `100`},
		{"date.due", `"2026-11-01"`},
		{"date.start", `"2026-11-21"`},
	} {
		req := a.set(c.key, 1, c.val)
		if ack := a.ack(req); ack.OK || ack.Error == "" || ack.Conflict {
			t.Fatalf("%s: %+v", c.key, ack)
		}
	}
	// conflict is checked before validity
	if ack := a.ack(a.set("number.qty", 9, `100`)); !ack.Conflict {
		t.Fatalf("%+v", ack)
	}
	// equal value still bumps the version
	if ack := a.ack(a.set("text.name", 1, `"Ada Lovelace"`)); !ack.OK || *ack.Version != 2 {
		t.Fatalf("%+v", ack)
	}
	a.changed()
	b.changed()
	// null date is valid, and lifts the cross-key rule
	if ack := a.ack(a.set("date.start", 1, `null`)); !ack.OK {
		t.Fatalf("%+v", ack)
	}
	a.changed()
	b.changed()
	if ack := a.ack(a.set("date.due", 1, `"2000-01-01"`)); !ack.OK {
		t.Fatalf("%+v", ack)
	}
	a.changed()
	b.changed()

	// reset: ack, then one changed per key
	a.req++
	a.send(core.ResetMsg{Req: a.req, Keys: []string{"select.status", "date.due", "select.status"}})
	if ack := a.ack(a.req); !ack.OK || ack.Version != nil {
		t.Fatalf("%+v", ack)
	}
	for _, x := range []*tc{a, b} {
		c1, c2 := x.changed(), x.changed()
		if c1.Key != "select.status" || c1.Version != 4 || string(c1.Value) != `"in_progress"` || c1.By != "alice" || c2.Key != "date.due" {
			t.Fatal(c1, c2)
		}
	}
	a.req++
	a.send(core.ResetMsg{Req: a.req, Keys: []string{"text.name", "nope"}})
	if ack := a.ack(a.req); ack.OK || ack.Error == "" {
		t.Fatalf("%+v", ack)
	}
	a.req++
	a.send(core.ResetMsg{Req: a.req})
	a.ack(a.req)
	for _, x := range []*tc{a, b} {
		for range core.Keys {
			x.changed()
		}
		for _, k := range core.Keys {
			if string(x.v.Raw(k)) != string(core.Default(k)) {
				t.Fatal(k)
			}
		}
	}

	// focus → presence to everyone with the tab; bad tab ignored
	b.send(core.FocusMsg{Tab: "nope"})
	b.send(core.FocusMsg{Tab: "date"})
	for _, x := range []*tc{a, b} {
		p := x.presence()
		if p.Peers[1].Tab == nil || *p.Peers[1].Tab != "date" {
			t.Fatal(p)
		}
	}

	// toast: broadcast to everyone (sender too), then ack; 1 per 2 s
	b.req++
	b.send(core.SendToastMsg{Req: b.req, Text: "  Lunch is here "})
	if m, ok := b.next().(core.ToastMsg); !ok || m.Text != "Lunch is here" || m.By != "bob" || m.Conn != 2 {
		t.Fatal(m)
	}
	if ack := b.ack(b.req); !ack.OK {
		t.Fatal(ack)
	}
	if m, ok := a.next().(core.ToastMsg); !ok || m.Text != "Lunch is here" {
		t.Fatal(m)
	}
	b.req++
	b.send(core.SendToastMsg{Req: b.req, Text: "again"})
	if ack := b.ack(b.req); ack.OK || ack.Error == "" {
		t.Fatal("toast rate", ack)
	}
	b.req++
	b.send(core.SendToastMsg{Req: b.req, Text: "   "})
	if ack := b.ack(b.req); ack.OK {
		t.Fatal("empty toast")
	}

	// ping / malformed frames: error message, socket stays open
	a.send(core.PingMsg{})
	if _, ok := a.next().(core.PongMsg); !ok {
		t.Fatal("pong")
	}
	for _, raw := range []string{`nope`, `{"type":"dance"}`, `{"type":"set","key":"x"}`} {
		if err := a.conn.SendRaw(a.ctx, []byte(raw)); err != nil {
			t.Fatal(err)
		}
		if m, ok := a.next().(core.ErrorMsg); !ok || m.Message == "" {
			t.Fatal(raw, m)
		}
	}
	a.send(core.PingMsg{})
	if _, ok := a.next().(core.PongMsg); !ok {
		t.Fatal("socket closed after bad frames")
	}

	// a disconnect updates presence
	b.conn.Close()
	if p := a.presence(); len(p.Peers) != 1 || p.Peers[0].Name != "alice" {
		t.Fatal(p)
	}
	// conn numbers keep increasing per server run
	c := connect(t, url, "carol")
	if c.v.You.Conn != 3 || c.v.Version("select.status") != 5 {
		t.Fatal(c.v.You, c.v.Version("select.status"))
	}
}

// closeCode reads until the socket closes and returns its close code.
func closeCode(t *testing.T, x *tc) int {
	t.Helper()
	for {
		_, err := x.conn.Recv(x.ctx)
		if err != nil {
			return client.CloseStatus(err)
		}
	}
}

func TestFrameLimit(t *testing.T) {
	url := start(t)
	ok := connect(t, url, "alice")
	// exactly MaxFrame is allowed (it's just a bad message)
	big := `{"type":"ping","pad":"` + strings.Repeat("x", core.MaxFrame-len(`{"type":"ping","pad":""}`)) + `"}`
	if len(big) != core.MaxFrame {
		t.Fatal(len(big))
	}
	if err := ok.conn.SendRaw(ok.ctx, []byte(big)); err != nil {
		t.Fatal(err)
	}
	if _, isPong := ok.next().(core.PongMsg); !isPong {
		t.Fatal("max-size frame")
	}
	x := connect(t, url, "bob")
	_ = x.conn.SendRaw(x.ctx, []byte(`{"type":"ping","pad":"`+strings.Repeat("x", core.MaxFrame)+`"}`))
	if code := closeCode(t, x); code != 1008 {
		t.Fatal("close code", code)
	}
}

func TestRateLimit(t *testing.T) {
	url := start(t)
	x := connect(t, url, "alice")
	for i := 0; i < 200; i++ {
		if x.conn.Send(x.ctx, core.PingMsg{}) != nil {
			break
		}
	}
	if code := closeCode(t, x); code != 1008 {
		t.Fatal("close code", code)
	}
}

func TestBinaryFrameCloses(t *testing.T) {
	url := start(t)
	ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
	defer cancel()
	ws, _, err := websocket.Dial(ctx, strings.Replace(url, "http", "ws", 1)+"/ws?client=conform&name=alice", nil)
	if err != nil {
		t.Fatal(err)
	}
	_ = ws.Write(ctx, websocket.MessageBinary, []byte("hi"))
	for {
		if _, _, err := ws.Read(ctx); err != nil {
			if websocket.CloseStatus(err) != websocket.StatusPolicyViolation {
				t.Fatal(err)
			}
			return
		}
	}
}

func TestSocketCap(t *testing.T) {
	url := start(t)
	for i := 0; i < core.MaxSockets; i++ {
		connect(t, url, "user")
	}
	if got := dialStatus(t, url, "?client=conform&name=late", nil); got != http.StatusServiceUnavailable {
		t.Fatal(got)
	}
}

func TestSession(t *testing.T) {
	url := start(t)
	ctx, cancel := context.WithTimeout(context.Background(), 20*time.Second)
	defer cancel()
	c, _ := client.New(url, "")
	s := c.Start(ctx, "tui-go", "alice")
	v := core.NewValues()
	ev := func() client.Event {
		e, ok := <-s.Events()
		if !ok {
			t.Fatal("events closed")
		}
		if e.Kind == client.Message {
			v.Apply(e.Msg)
		}
		return e
	}
	if e := ev(); e.Kind != client.Connected {
		t.Fatal(e)
	}
	for !v.Loaded() {
		ev()
	}
	set, err := v.PrepareSet("number.percent", 77)
	if err != nil {
		t.Fatal(err)
	}
	req, err := s.Set(ctx, set)
	if err != nil {
		t.Fatal(err)
	}
	for {
		e := ev()
		if a, ok := e.Msg.(core.AckMsg); ok {
			if a.Req != req || !a.OK {
				t.Fatal(a)
			}
			break
		}
	}
	for v.Int("number.percent") != 77 {
		ev()
	}
	if _, err := s.Toast(ctx, "hello"); err != nil || s.Focus(ctx, "number") != nil || s.Ping(ctx) != nil {
		t.Fatal("sends")
	}
	if _, err := s.Reset(ctx, "number.percent"); err != nil {
		t.Fatal(err)
	}
	for v.Int("number.percent") != 40 {
		ev()
	}

	// a bad name is rejected and the session stops
	r := c.Start(ctx, "tui-go", "9lives")
	e := <-r.Events()
	if e.Kind != client.Rejected || !client.IsRejected(e.Err) {
		t.Fatal(e)
	}
	if _, open := <-r.Events(); open {
		t.Fatal("rejected session kept running")
	}
}

func TestCheckBind(t *testing.T) {
	for _, c := range []struct {
		bind       string
		tls, plain bool
		ok         bool
	}{
		{"127.0.0.1:8790", false, false, true},
		{"localhost:8790", false, false, true},
		{"[::1]:8790", false, false, true},
		{"0.0.0.0:8790", false, false, false},
		{"0.0.0.0:8790", true, false, true},
		{"0.0.0.0:8790", false, true, true},
		{"nonsense", false, false, false},
	} {
		if err := server.CheckBind(c.bind, c.tls, c.plain); (err == nil) != c.ok {
			t.Errorf("%+v: %v", c, err)
		}
	}
}
