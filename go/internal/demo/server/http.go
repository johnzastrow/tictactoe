package server

// HTTP + WebSocket adapter (protocol §1): /api/health, the /ws socket, the
// embedded web client, and the security controls (same-origin check on
// /ws, parameter validation, a socket cap, frame size and message-rate
// limits, security headers with a strict CSP).

import (
	"context"
	"crypto/sha256"
	"encoding/base64"
	"encoding/json"
	"errors"
	"io"
	"io/fs"
	"log/slog"
	"mime"
	"net/http"
	"path"
	"strings"
	"sync/atomic"
	"time"

	"github.com/coder/websocket"

	"git.wharf.fluidgrid.org/jcz/tictactoe/go/internal/demo/core"
)

type app struct {
	hub     *Hub
	tls     bool
	csp     string
	assets  fs.FS
	sockets atomic.Int64 // open (or opening) sockets, for the 64 cap
	log     *slog.Logger
}

// NewHandler builds the HTTP handler for a running hub.
func NewHandler(h *Hub, assets fs.FS, tls bool, log *slog.Logger) http.Handler {
	a := &app{hub: h, tls: tls, assets: assets, log: log}
	a.csp = buildCSP(assets)
	mux := http.NewServeMux()
	mux.HandleFunc("GET /api/health", a.health)
	mux.HandleFunc("GET /api/", func(w http.ResponseWriter, r *http.Request) { jsonErr(w, http.StatusNotFound, "not found") })
	mux.HandleFunc("GET /ws", a.ws)
	mux.HandleFunc("GET /", a.static)
	return a.securityHeaders(mux)
}

// buildCSP adds the hashes of any inline <script> in index.html (WASM loaders).
func buildCSP(assets fs.FS) string {
	var hashes strings.Builder
	if b, err := fs.ReadFile(assets, "index.html"); err == nil {
		rest := string(b)
		for {
			start := strings.Index(rest, "<script")
			if start < 0 {
				break
			}
			after := rest[start:]
			openEnd := strings.IndexByte(after, '>')
			if openEnd < 0 {
				break
			}
			tag := after[:openEnd]
			body := after[openEnd+1:]
			end := strings.Index(body, "</script>")
			if end < 0 {
				break
			}
			if !strings.Contains(tag, " src=") && strings.TrimSpace(body[:end]) != "" {
				sum := sha256.Sum256([]byte(body[:end]))
				hashes.WriteString(" 'sha256-" + base64.StdEncoding.EncodeToString(sum[:]) + "'")
			}
			rest = body[end:]
		}
	}
	return "default-src 'self'; script-src 'self' 'wasm-unsafe-eval'" + hashes.String() +
		"; style-src 'self' 'unsafe-inline'; connect-src 'self'; img-src 'self' data:; font-src 'self'; " +
		"frame-ancestors 'none'; base-uri 'none'; form-action 'self'; object-src 'none'"
}

func (a *app) securityHeaders(next http.Handler) http.Handler {
	return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		h := w.Header()
		h.Set("X-Content-Type-Options", "nosniff")
		h.Set("Referrer-Policy", "same-origin")
		h.Set("X-Frame-Options", "DENY")
		h.Set("Content-Security-Policy", a.csp)
		if a.tls {
			h.Set("Strict-Transport-Security", "max-age=31536000")
		}
		next.ServeHTTP(w, r)
	})
}

// originOK: an Origin header, if present, must be http(s)://<Host>.
func originOK(r *http.Request) bool {
	o := r.Header.Get("Origin")
	if o == "" {
		return true
	}
	return o == "http://"+r.Host || o == "https://"+r.Host
}

func writeJSON(w http.ResponseWriter, status int, v any) {
	w.Header().Set("Content-Type", "application/json")
	w.WriteHeader(status)
	_ = json.NewEncoder(w).Encode(v)
}

func jsonErr(w http.ResponseWriter, status int, msg string) {
	writeJSON(w, status, map[string]string{"error": msg})
}

// ---- handlers ----

func (a *app) health(w http.ResponseWriter, r *http.Request) {
	writeJSON(w, http.StatusOK, core.HealthInfo{OK: true, Impl: "go", Version: core.Version, Protocol: core.ProtocolVersion})
}

func (a *app) static(w http.ResponseWriter, r *http.Request) {
	p := strings.TrimPrefix(path.Clean("/"+r.URL.Path), "/")
	if p == "" {
		p = "index.html"
	}
	b, err := fs.ReadFile(a.assets, p)
	if err != nil {
		// client-side routes fall back to the app shell
		if strings.Contains(p, ".") {
			http.NotFound(w, r)
			return
		}
		if b, err = fs.ReadFile(a.assets, "index.html"); err != nil {
			http.NotFound(w, r)
			return
		}
		p = "index.html"
	}
	ct := mime.TypeByExtension(path.Ext(p))
	if strings.HasSuffix(p, ".wasm") {
		ct = "application/wasm"
	}
	if ct == "" {
		ct = "application/octet-stream"
	}
	w.Header().Set("Content-Type", ct)
	// Serve a precompressed copy (made by `make demo-web`) when the browser accepts it.
	if strings.Contains(r.Header.Get("Accept-Encoding"), "gzip") {
		if gz, err := fs.ReadFile(a.assets, p+".gz"); err == nil {
			b = gz
			w.Header().Set("Content-Encoding", "gzip")
		}
	}
	w.Header().Set("Vary", "Accept-Encoding")
	if p == "index.html" {
		w.Header().Set("Cache-Control", "no-cache")
	} else {
		w.Header().Set("Cache-Control", "public, max-age=3600")
	}
	_, _ = w.Write(b)
}

// ws: GET /ws?client=<id>&name=<name>. Origin → 403, bad parameters → 400,
// more than MaxSockets open → 503.
func (a *app) ws(w http.ResponseWriter, r *http.Request) {
	if !originOK(r) {
		jsonErr(w, http.StatusForbidden, "cross-origin WebSocket refused")
		return
	}
	q := r.URL.Query()
	client := q.Get("client")
	if !core.ValidClientID(client) {
		jsonErr(w, http.StatusBadRequest, "client must match [a-z0-9-]{1,24}")
		return
	}
	name, ok := core.CleanName(q.Get("name"))
	if !ok {
		jsonErr(w, http.StatusBadRequest, "name must be 1–24 letters, digits, space, _ . - starting with a letter")
		return
	}
	if a.sockets.Add(1) > core.MaxSockets {
		a.sockets.Add(-1)
		a.log.Warn("socket limit reached", "client", client)
		jsonErr(w, http.StatusServiceUnavailable, "server full")
		return
	}
	defer a.sockets.Add(-1)
	// Origin was checked above against Host.
	c, err := websocket.Accept(w, r, &websocket.AcceptOptions{InsecureSkipVerify: true})
	if err != nil {
		return
	}
	// The frame limit is enforced in runSocket so it can close with 1008
	// (the library's own limit closes with 1009).
	c.SetReadLimit(-1)
	a.runSocket(r.Context(), c, name, client)
}

// bucket: 20 msg/s sustained, burst 40 (protocol §1).
type bucket struct {
	tokens float64
	last   time.Time
}

func (b *bucket) take() bool {
	now := time.Now()
	b.tokens = min(b.tokens+now.Sub(b.last).Seconds()*20, 40)
	b.last = now
	if b.tokens >= 1 {
		b.tokens--
		return true
	}
	return false
}

// readFrame reads one message of at most core.MaxFrame bytes; tooBig is
// true (and the rest left unread) when it is larger.
func readFrame(ctx context.Context, c *websocket.Conn) (typ websocket.MessageType, data []byte, tooBig bool, err error) {
	typ, rd, err := c.Reader(ctx)
	if err != nil {
		return typ, nil, false, err
	}
	data, err = io.ReadAll(io.LimitReader(rd, core.MaxFrame+1))
	if err != nil {
		return typ, nil, false, err
	}
	if len(data) > core.MaxFrame {
		return typ, nil, true, nil
	}
	return typ, data, false, nil
}

func (a *app) runSocket(parent context.Context, c *websocket.Conn, name, client string) {
	ctx, cancel := context.WithCancel(context.WithoutCancel(parent))
	defer cancel()
	out := make(chan []byte, outboundDepth)
	id, ok := a.hub.Connect(name, client, out)
	if !ok {
		c.Close(websocket.StatusInternalError, "internal error")
		return
	}

	// Writer: hub output + keepalive pings. Ends when the hub closes out.
	writerDone := make(chan struct{})
	go func() {
		defer close(writerDone)
		ping := time.NewTicker(30 * time.Second)
		defer ping.Stop()
		for {
			select {
			case b, ok := <-out:
				if !ok {
					c.Close(websocket.StatusPolicyViolation, "disconnected by server")
					cancel()
					return
				}
				wctx, wc := context.WithTimeout(ctx, 15*time.Second)
				err := c.Write(wctx, websocket.MessageText, b)
				wc()
				if err != nil {
					cancel()
					return
				}
			case <-ping.C:
				go func() {
					pctx, pc := context.WithTimeout(ctx, 20*time.Second)
					defer pc()
					if c.Ping(pctx) != nil {
						cancel()
					}
				}()
			case <-ctx.Done():
				return
			}
		}
	}()

	b := &bucket{tokens: 40, last: time.Now()}
	reason := ""
	for {
		typ, data, tooBig, err := readFrame(ctx, c)
		if err != nil {
			break
		}
		if tooBig {
			reason = "message too big"
			break
		}
		if typ != websocket.MessageText {
			reason = "binary frames are not supported"
			break
		}
		if !b.take() {
			reason = "rate limit exceeded"
			break
		}
		msg, err := core.DecodeClientMsg(data)
		if err != nil {
			a.hub.Reply(id, core.ErrorMsg{Message: errorText(err)})
			continue
		}
		a.hub.Client(id, msg)
	}
	a.hub.Disconnect(id)
	if reason != "" {
		a.log.Warn("closing socket", "conn", id, "reason", reason)
		c.Close(websocket.StatusPolicyViolation, reason)
	} else {
		c.CloseNow()
	}
	cancel()
	<-writerDone
}

// errorText keeps decode errors short (they may echo client input).
func errorText(err error) string {
	s := err.Error()
	if !errors.Is(err, core.ErrBadMessage) {
		s = "bad message: " + s
	}
	return truncate(s, 160)
}
