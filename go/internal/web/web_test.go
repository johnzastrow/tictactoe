package web

import (
	"bytes"
	"context"
	"encoding/json"
	"net"
	"net/http"
	"os"
	"path/filepath"
	"strconv"
	"strings"
	"testing"
	"time"

	"github.com/coder/websocket"

	"git.wharf.fluidgrid.org/jcz/tictactoe/go/internal/core"
	"git.wharf.fluidgrid.org/jcz/tictactoe/go/internal/hub"
)

func start(t *testing.T) int {
	t.Helper()
	ln, err := net.Listen("tcp", "127.0.0.1:0")
	if err != nil {
		t.Fatal(err)
	}
	h := hub.Start(t.Context(), core.None, core.Hard, 0)
	go Serve(t.Context(), ln, h)
	return ln.Addr().(*net.TCPAddr).Port
}

func dial(t *testing.T, port int, origin string) (*websocket.Conn, *http.Response, error) {
	ctx, cancel := context.WithTimeout(t.Context(), 2*time.Second)
	defer cancel()
	hdr := http.Header{}
	if origin != "" {
		hdr.Set("Origin", origin)
	}
	return websocket.Dial(ctx, "ws://127.0.0.1:"+strconv.Itoa(port)+"/ws",
		&websocket.DialOptions{HTTPHeader: hdr})
}

type anyMsg struct {
	Type    string     `json:"type"`
	Message string     `json:"message"`
	Board   [9]*string `json:"board"`
}

func next(t *testing.T, c *websocket.Conn) anyMsg {
	t.Helper()
	ctx, cancel := context.WithTimeout(t.Context(), 2*time.Second)
	defer cancel()
	_, b, err := c.Read(ctx)
	if err != nil {
		t.Fatal(err)
	}
	var m anyMsg
	json.Unmarshal(b, &m)
	return m
}

func send(t *testing.T, c *websocket.Conn, s string) {
	t.Helper()
	if err := c.Write(t.Context(), websocket.MessageText, []byte(s)); err != nil {
		t.Fatal(err)
	}
}

func TestMoveIsBroadcastToAllClients(t *testing.T) {
	port := start(t)
	origin := "http://127.0.0.1:" + strconv.Itoa(port)
	a, _, err := dial(t, port, origin)
	if err != nil {
		t.Fatal(err)
	}
	b, _, err := dial(t, port, origin)
	if err != nil {
		t.Fatal(err)
	}
	next(t, a)
	next(t, b)
	send(t, a, `{"type":"move","cell":4}`)
	for { // b may first see the client-count update
		if m := next(t, b); m.Type == "state" && m.Board[4] != nil {
			return
		}
	}
}

func TestBadInputGetsErrorReply(t *testing.T) {
	port := start(t)
	c, _, err := dial(t, port, "")
	if err != nil {
		t.Fatal(err)
	}
	next(t, c)
	send(t, c, `{"type":"move","cell":42}`)
	send(t, c, `not json`)
	var errs []string
	for len(errs) < 2 {
		if m := next(t, c); m.Type == "error" {
			errs = append(errs, m.Message)
		}
	}
	if !strings.Contains(errs[0], "out of range") || errs[1] != "malformed message" {
		t.Fatalf("got %q", errs)
	}
}

func TestForeignOriginRejected(t *testing.T) {
	port := start(t)
	_, resp, err := dial(t, port, "https://evil.example")
	if err == nil || resp == nil || resp.StatusCode != http.StatusForbidden {
		t.Fatalf("expected 403, got %v %v", resp, err)
	}
}

func TestForeignHostRejected(t *testing.T) {
	port := start(t)
	req, _ := http.NewRequest("GET", "http://127.0.0.1:"+strconv.Itoa(port)+"/", nil)
	req.Host = "evil.example"
	resp, err := http.DefaultClient.Do(req)
	if err != nil {
		t.Fatal(err)
	}
	resp.Body.Close()
	if resp.StatusCode != http.StatusMisdirectedRequest {
		t.Fatalf("got %d", resp.StatusCode)
	}
}

func TestRefusesNonLoopback(t *testing.T) {
	ln, err := net.Listen("tcp", "0.0.0.0:0")
	if err != nil {
		t.Skip(err)
	}
	defer ln.Close()
	if err := Serve(t.Context(), ln, hub.Start(t.Context(), core.None, core.Hard, 0)); err == nil {
		t.Fatal("served on 0.0.0.0")
	}
}

// The web page is shared with the Rust implementation; catch accidental drift.
func TestAssetsMatchRustCopy(t *testing.T) {
	for _, name := range []string{"index.html", "app.js", "style.css"} {
		rust, err := os.ReadFile(filepath.Join("..", "..", "..", "rust", "crates", "ttt-server", "web", name))
		if err != nil {
			t.Skip("rust tree not present")
		}
		ours, _ := assets.ReadFile("assets/" + name)
		if !bytes.Equal(rust, ours) {
			t.Errorf("%s differs from the Rust copy", name)
		}
	}
}

func TestSetDifficultyIsBroadcast(t *testing.T) {
	port := start(t)
	c, _, err := dial(t, port, "")
	if err != nil {
		t.Fatal(err)
	}
	read := func() map[string]any {
		ctx, cancel := context.WithTimeout(t.Context(), 2*time.Second)
		defer cancel()
		_, b, err := c.Read(ctx)
		if err != nil {
			t.Fatal(err)
		}
		var m map[string]any
		json.Unmarshal(b, &m)
		return m
	}
	if m := read(); m["difficulty"] != "hard" {
		t.Fatalf("initial state: %v", m)
	}
	send(t, c, `{"type":"set_difficulty","difficulty":"easy"}`)
	for m := read(); m["difficulty"] != "easy"; m = read() {
	}
	send(t, c, `{"type":"set_difficulty","difficulty":"impossible"}`)
	for m := read(); m["type"] != "error"; m = read() {
	}
}
