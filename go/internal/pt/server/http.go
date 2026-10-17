package server

// HTTP + WebSocket adapter (protocol §1–2): login/logout/me/health, the /ws
// socket, the embedded web client, and the security controls (session
// auth, same-origin checks, login throttling, frame size and message-rate
// limits, security headers).

import (
	"context"
	"crypto/sha256"
	"encoding/base64"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"io/fs"
	"log/slog"
	"mime"
	"net"
	"net/http"
	"path"
	"strings"
	"sync"
	"time"

	"github.com/coder/websocket"

	"git.wharf.fluidgrid.org/jcz/tictactoe/go/internal/pt/auth"
	"git.wharf.fluidgrid.org/jcz/tictactoe/go/internal/pt/core"
)

const CookieName = "pt_session"

type app struct {
	hub     *Hub
	tls     bool
	csp     string
	assets  fs.FS
	limiter *limiter
	log     *slog.Logger
}

// NewHandler builds the HTTP handler for a running hub.
func NewHandler(h *Hub, assets fs.FS, tls bool, log *slog.Logger) http.Handler {
	a := &app{hub: h, tls: tls, assets: assets, limiter: newLimiter(), log: log}
	a.csp = buildCSP(assets)
	mux := http.NewServeMux()
	mux.HandleFunc("GET /api/health", a.health)
	mux.HandleFunc("POST /api/login", a.login)
	mux.HandleFunc("POST /api/logout", a.logout)
	mux.HandleFunc("GET /api/me", a.me)
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
		"; style-src 'self' 'unsafe-inline'; img-src 'self' data:; connect-src 'self'; font-src 'self'; " +
		"frame-ancestors 'none'; base-uri 'none'; form-action 'self'; object-src 'none'"
}

func (a *app) securityHeaders(next http.Handler) http.Handler {
	return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		h := w.Header()
		h.Set("X-Content-Type-Options", "nosniff")
		h.Set("Referrer-Policy", "no-referrer")
		h.Set("X-Frame-Options", "DENY")
		h.Set("Content-Security-Policy", a.csp)
		if a.tls {
			h.Set("Strict-Transport-Security", "max-age=31536000")
		}
		next.ServeHTTP(w, r)
	})
}

// originOK: an Origin header, if present, must match Host.
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

func sessionToken(r *http.Request) string {
	if v := r.Header.Get("Authorization"); strings.HasPrefix(v, "Bearer ") {
		return strings.TrimSpace(v[len("Bearer "):])
	}
	if c, err := r.Cookie(CookieName); err == nil {
		return c.Value
	}
	return ""
}

func (a *app) authed(r *http.Request) (*core.User, string) {
	token := sessionToken(r)
	if token == "" || len(token) > 128 {
		return nil, ""
	}
	h := auth.TokenHash(token)
	return a.hub.Session(h), h
}

func (a *app) setCookie(w http.ResponseWriter, token string, maxAge int) {
	v := fmt.Sprintf("%s=%s; Path=/; HttpOnly; SameSite=Strict; Max-Age=%d", CookieName, token, maxAge)
	if a.tls {
		v += "; Secure"
	}
	w.Header().Add("Set-Cookie", v)
}

// ---- login throttling ----

type limiter struct {
	mu     sync.Mutex
	byIP   map[string][]time.Time
	byUser map[string][]time.Time
}

func newLimiter() *limiter {
	return &limiter{byIP: map[string][]time.Time{}, byUser: map[string][]time.Time{}}
}

func prune(ts []time.Time, now time.Time, window time.Duration) []time.Time {
	out := ts[:0]
	for _, t := range ts {
		if now.Sub(t) < window {
			out = append(out, t)
		}
	}
	return out
}

// allow is false if this IP (20 failures/minute) or username (5 failures/15
// minutes) is over its limit. Successful logins never count.
func (l *limiter) allow(ip, user string) bool {
	l.mu.Lock()
	defer l.mu.Unlock()
	now := time.Now()
	l.byIP[ip] = prune(l.byIP[ip], now, time.Minute)
	l.byUser[user] = prune(l.byUser[user], now, 15*time.Minute)
	ok := len(l.byIP[ip]) < 20 && len(l.byUser[user]) < 5
	// bound memory if someone sprays usernames or addresses
	for _, m := range []map[string][]time.Time{l.byIP, l.byUser} {
		if len(m) > 10_000 {
			for k, v := range m {
				if len(v) == 0 {
					delete(m, k)
				}
			}
		}
	}
	return ok
}

func (l *limiter) failed(ip, user string) {
	l.mu.Lock()
	defer l.mu.Unlock()
	l.byIP[ip] = append(l.byIP[ip], time.Now())
	l.byUser[user] = append(l.byUser[user], time.Now())
}

func (l *limiter) succeeded(user string) {
	l.mu.Lock()
	defer l.mu.Unlock()
	delete(l.byUser, user)
}

func clientIP(r *http.Request) string {
	host, _, err := net.SplitHostPort(r.RemoteAddr)
	if err != nil {
		return r.RemoteAddr
	}
	return host
}

// ---- handlers ----

func (a *app) health(w http.ResponseWriter, r *http.Request) {
	writeJSON(w, http.StatusOK, core.HealthInfo{OK: true, Impl: "go", Version: core.Version, Protocol: core.ProtocolVersion})
}

func (a *app) login(w http.ResponseWriter, r *http.Request) {
	if !originOK(r) {
		jsonErr(w, http.StatusForbidden, "cross-origin request refused")
		return
	}
	body, err := io.ReadAll(http.MaxBytesReader(w, r.Body, 4096))
	if err != nil {
		jsonErr(w, http.StatusRequestEntityTooLarge, "request too large")
		return
	}
	var req core.LoginRequest
	if json.Unmarshal(body, &req) != nil {
		jsonErr(w, http.StatusBadRequest, `expected {"username", "password"}`)
		return
	}
	username := strings.ToLower(strings.TrimSpace(req.Username))
	if username == "" || len(username) > 64 || len(req.Password) > 256 {
		jsonErr(w, http.StatusUnauthorized, "invalid credentials")
		return
	}
	ip := clientIP(r)
	if !a.limiter.allow(ip, username) {
		a.log.Warn("login throttled", "ip", ip, "user", username)
		jsonErr(w, http.StatusTooManyRequests, "too many attempts")
		return
	}
	user, hash := a.hub.LoginLookup(username)
	if user == nil {
		hash = auth.DummyHash() // unknown users still pay for a hash
	}
	ok := auth.VerifyPassword(req.Password, hash)
	if user == nil || !ok || !user.Active {
		a.limiter.failed(ip, username)
		a.log.Info("login failed", "ip", ip, "user", username)
		jsonErr(w, http.StatusUnauthorized, "invalid credentials")
		return
	}
	a.limiter.succeeded(username)
	token := auth.NewToken()
	client := ""
	if core.ValidClientID(req.Client) {
		client = req.Client
	}
	if !a.hub.CreateSession(auth.TokenHash(token), user.ID, client) {
		jsonErr(w, http.StatusInternalServerError, "internal error")
		return
	}
	a.log.Info("login", "ip", ip, "user", user.Username)
	a.setCookie(w, token, auth.SessionTTLSecs)
	writeJSON(w, http.StatusOK, core.LoginResponse{Token: token, User: *user})
}

func (a *app) logout(w http.ResponseWriter, r *http.Request) {
	if !originOK(r) {
		jsonErr(w, http.StatusForbidden, "cross-origin request refused")
		return
	}
	if t := sessionToken(r); t != "" {
		a.hub.Logout(auth.TokenHash(t))
	}
	a.setCookie(w, "", 0)
	w.WriteHeader(http.StatusNoContent)
}

func (a *app) me(w http.ResponseWriter, r *http.Request) {
	u, _ := a.authed(r)
	if u == nil {
		jsonErr(w, http.StatusUnauthorized, "not logged in")
		return
	}
	writeJSON(w, http.StatusOK, u)
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
	// Serve a precompressed copy (made by `make web`) when the browser accepts it.
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

func (a *app) ws(w http.ResponseWriter, r *http.Request) {
	if !originOK(r) {
		jsonErr(w, http.StatusForbidden, "cross-origin WebSocket refused")
		return
	}
	user, session := a.authed(r)
	if user == nil {
		jsonErr(w, http.StatusUnauthorized, "not logged in")
		return
	}
	client := r.URL.Query().Get("client")
	if !core.ValidClientID(client) {
		client = "unknown"
	}
	// Origin was checked above against Host.
	c, err := websocket.Accept(w, r, &websocket.AcceptOptions{InsecureSkipVerify: true})
	if err != nil {
		return
	}
	c.SetReadLimit(core.MaxFrame)
	a.runSocket(r.Context(), c, *user, client, session)
}

// bucket: 50 msg/s sustained, burst 100 (protocol §1).
type bucket struct {
	tokens float64
	last   time.Time
}

func (b *bucket) take() bool {
	now := time.Now()
	b.tokens = min(b.tokens+now.Sub(b.last).Seconds()*50, 100)
	b.last = now
	if b.tokens >= 1 {
		b.tokens--
		return true
	}
	return false
}

func (a *app) runSocket(parent context.Context, c *websocket.Conn, user core.User, client, session string) {
	ctx, cancel := context.WithCancel(context.WithoutCancel(parent))
	defer cancel()
	out := make(chan []byte, outboundDepth)
	id, ok := a.hub.Connect(user, client, session, out)
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
					c.Close(websocket.StatusPolicyViolation, "session ended")
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

	b := &bucket{tokens: 100, last: time.Now()}
	reason := ""
	for {
		typ, data, err := c.Read(ctx)
		if err != nil {
			if errors.Is(err, websocket.ErrMessageTooBig) {
				reason = "message too big"
			}
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
			a.hub.Reply(id, core.ErrorMsg{Code: "bad_request", Message: "unreadable message: " + err.Error()})
			continue
		}
		if sp, ok := msg.(core.SetPasswordMsg); ok {
			a.setPassword(id, sp)
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

// setPassword (protocol §5): authorise, verify the old password and hash
// the new one here, off the hub goroutine; the hub re-checks and stores it.
func (a *app) setPassword(id uint64, m core.SetPasswordMsg) {
	fail := func(e *core.ErrorBody) {
		a.hub.Reply(id, core.ResultMsg{Req: m.Req, Error: e})
	}
	actor, hash, found := a.hub.PasswordInfo(id, m.UserID)
	if actor == nil {
		return
	}
	if actor.ID != m.UserID && actor.Role != core.RoleAdmin {
		fail(core.NewError("forbidden", "you can only change your own password"))
		return
	}
	if !found {
		fail(core.NewError("not_found", "no such user"))
		return
	}
	if n := len([]rune(m.Password)); n < 8 || n > 256 {
		fail(core.NewError("invalid", "password must be 8–256 characters"))
		return
	}
	if actor.Role != core.RoleAdmin {
		old := ""
		if m.OldPassword != nil {
			old = *m.OldPassword
		}
		if !auth.VerifyPassword(old, hash) {
			fail(core.NewError("invalid", "current password is wrong"))
			return
		}
	}
	h, err := auth.HashPassword(m.Password)
	if err != nil {
		fail(core.NewError("internal", "internal error"))
		return
	}
	a.hub.SetPasswordHash(id, m.Req, m.UserID, h)
}
