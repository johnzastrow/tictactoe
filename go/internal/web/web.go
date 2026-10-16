// Package web serves the embedded page and bridges WebSocket clients to the
// hub. See docs/protocol.md for the wire format.
package web

import (
	"context"
	"embed"
	"encoding/json"
	"errors"
	"fmt"
	"log/slog"
	"net"
	"net/http"
	"slices"
	"strconv"
	"time"

	"github.com/coder/websocket"

	"git.wharf.fluidgrid.org/jcz/tictactoe/go/internal/core"
	"git.wharf.fluidgrid.org/jcz/tictactoe/go/internal/hub"
)

//go:embed assets
var assets embed.FS

const (
	maxMessageBytes = 4 << 10
	maxMsgsPerSec   = 20
)

// ClientMsg is any message a browser may send. Fields unused by a given
// type are ignored.
type ClientMsg struct {
	Type       string           `json:"type"`
	Cell       *int             `json:"cell"`
	Player     *core.Player     `json:"player"`
	Difficulty *core.Difficulty `json:"difficulty"`
}

type errorMsg struct {
	Type    string `json:"type"`
	Message string `json:"message"`
}

type stateMsg struct {
	Type string `json:"type"`
	hub.StateView
}

// Serve runs the web server on ln until ctx is cancelled. Taking a listener
// lets callers bind first (clean "port in use" errors) and use port 0 in tests.
func Serve(ctx context.Context, ln net.Listener, h *hub.Hub) error {
	addr, ok := ln.Addr().(*net.TCPAddr)
	// Refuse anything but loopback: there is no authentication.
	if !ok || !addr.IP.IsLoopback() {
		return fmt.Errorf("refusing to serve on non-loopback address %s", ln.Addr())
	}
	srv := &http.Server{
		Handler:           Handler(h, addr.Port),
		ReadHeaderTimeout: 5 * time.Second,
		BaseContext:       func(net.Listener) context.Context { return ctx },
	}
	go func() {
		<-ctx.Done()
		shutdownCtx, cancel := context.WithTimeout(context.Background(), 2*time.Second)
		defer cancel()
		srv.Shutdown(shutdownCtx)
	}()
	if err := srv.Serve(ln); !errors.Is(err, http.ErrServerClosed) {
		return err
	}
	return nil
}

// Handler builds the HTTP handler for a server listening on port.
func Handler(h *hub.Hub, port int) http.Handler {
	p := strconv.Itoa(port)
	hosts := []string{"127.0.0.1:" + p, "localhost:" + p}
	origins := []string{"http://" + hosts[0], "http://" + hosts[1]}

	mux := http.NewServeMux()
	mux.HandleFunc("GET /{$}", asset("assets/index.html", "text/html; charset=utf-8"))
	mux.HandleFunc("GET /app.js", asset("assets/app.js", "text/javascript; charset=utf-8"))
	mux.HandleFunc("GET /style.css", asset("assets/style.css", "text/css; charset=utf-8"))
	mux.HandleFunc("GET /ws", func(w http.ResponseWriter, r *http.Request) {
		// Browsers always send Origin on WebSocket handshakes. Without this
		// check any web page you visit could connect to localhost and play.
		if o := r.Header.Get("Origin"); o != "" && !slices.Contains(origins, o) {
			slog.Warn("rejected WebSocket from foreign origin", "origin", o)
			http.Error(w, "origin not allowed", http.StatusForbidden)
			return
		}
		// Origin is already verified above (and Host by the middleware).
		c, err := websocket.Accept(w, r, &websocket.AcceptOptions{OriginPatterns: hosts})
		if err != nil {
			return
		}
		session(r.Context(), c, h)
	})
	return guardHost(hosts, mux)
}

func asset(name, contentType string) http.HandlerFunc {
	body, err := assets.ReadFile(name)
	if err != nil {
		panic(err) // embedded at build time; missing means a broken build
	}
	return func(w http.ResponseWriter, _ *http.Request) {
		w.Header().Set("Content-Type", contentType)
		w.Header().Set("Cache-Control", "no-cache")
		w.Write(body)
	}
}

// guardHost rejects unexpected Host headers (DNS-rebinding defense) and adds
// security headers to every response.
func guardHost(hosts []string, next http.Handler) http.Handler {
	return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if !slices.Contains(hosts, r.Host) {
			http.Error(w, "unexpected Host header", http.StatusMisdirectedRequest)
			return
		}
		h := w.Header()
		h.Set("Content-Security-Policy",
			"default-src 'self'; connect-src 'self'; frame-ancestors 'none'; base-uri 'none'; form-action 'none'")
		h.Set("X-Content-Type-Options", "nosniff")
		h.Set("Referrer-Policy", "no-referrer")
		h.Set("X-Frame-Options", "DENY")
		next.ServeHTTP(w, r)
	})
}

func session(ctx context.Context, c *websocket.Conn, h *hub.Hub) {
	defer c.CloseNow()
	c.SetReadLimit(maxMessageBytes)
	ctx, cancel := context.WithCancel(ctx)
	defer cancel()

	updates, unsubscribe := h.Subscribe(true)
	defer unsubscribe()

	// Writer: push every state change. coder/websocket allows concurrent
	// writes, so the reader below can also send error replies.
	go func() {
		defer cancel()
		for {
			select {
			case <-ctx.Done():
				return
			case v, ok := <-updates:
				if !ok || write(ctx, c, stateMsg{Type: "state", StateView: v}) != nil {
					return
				}
			}
		}
	}()

	// Reader. Fixed one-second window rate limit: crude but enough to stop
	// a runaway client.
	windowStart, count := time.Now(), 0
	for {
		typ, data, err := c.Read(ctx)
		if err != nil {
			return
		}
		if typ != websocket.MessageText {
			continue
		}
		if time.Since(windowStart) >= time.Second {
			windowStart, count = time.Now(), 0
		}
		if count++; count > maxMsgsPerSec {
			slog.Warn("closing WebSocket: rate limit exceeded")
			c.Close(websocket.StatusPolicyViolation, "rate limit exceeded")
			return
		}
		if err := dispatch(h, data); err != nil {
			if write(ctx, c, errorMsg{Type: "error", Message: err.Error()}) != nil {
				return
			}
		}
	}
}

// dispatch parses and executes one message. Errors are user-facing and
// never include internal details.
func dispatch(h *hub.Hub, data []byte) error {
	var m ClientMsg
	if err := json.Unmarshal(data, &m); err != nil {
		return errors.New("malformed message")
	}
	switch {
	case m.Type == "move" && m.Cell != nil:
		return h.Play(*m.Cell)
	case m.Type == "new_game":
		return h.NewGame()
	case m.Type == "set_ai":
		p := core.None
		if m.Player != nil {
			p = *m.Player
		}
		return h.SetAI(p)
	case m.Type == "set_difficulty" && m.Difficulty != nil:
		return h.SetDifficulty(*m.Difficulty)
	}
	return errors.New("malformed message")
}

func write(ctx context.Context, c *websocket.Conn, v any) error {
	b, err := json.Marshal(v)
	if err != nil {
		return err
	}
	ctx, cancel := context.WithTimeout(ctx, 5*time.Second)
	defer cancel()
	return c.Write(ctx, websocket.MessageText, b)
}
