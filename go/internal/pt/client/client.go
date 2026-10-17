// Package client is the pt protocol client used by the Go TUI: HTTP login /
// logout / health, the WebSocket connection with automatic reconnects, and
// the per-server session cache (os.UserConfigDir()/pt/sessions.json, 0600).
package client

import (
	"bytes"
	"context"
	"crypto/tls"
	"crypto/x509"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"net/http"
	"net/url"
	"os"
	"strings"
	"sync"
	"time"

	"github.com/coder/websocket"

	"git.wharf.fluidgrid.org/jcz/tictactoe/go/internal/pt/core"
)

// ReadLimit allows full snapshots (a few MB for the seed data).
const ReadLimit = 64 << 20

// HTTPError is a non-2xx API response.
type HTTPError struct {
	Status  int
	Message string
}

func (e *HTTPError) Error() string { return fmt.Sprintf("%d: %s", e.Status, e.Message) }

// IsUnauthorized reports a 401 (bad credentials or expired session).
func IsUnauthorized(err error) bool {
	var he *HTTPError
	return errors.As(err, &he) && he.Status == http.StatusUnauthorized
}

type Client struct {
	Base string // http(s)://host:port, no trailing slash
	HTTP *http.Client

	mu    sync.Mutex // guards token (the reconnect loop reads it)
	token string
}

// Token returns the session token ("" when logged out).
func (c *Client) Token() string {
	c.mu.Lock()
	defer c.mu.Unlock()
	return c.token
}

// SetToken installs a token, e.g. one loaded with LoadToken.
func (c *Client) SetToken(t string) {
	c.mu.Lock()
	c.token = t
	c.mu.Unlock()
}

// New creates a client; caCert (PEM file) adds a trusted CA for https.
func New(server, caCert string) (*Client, error) {
	base := strings.TrimRight(strings.TrimSpace(server), "/")
	u, err := url.Parse(base)
	if err != nil || (u.Scheme != "http" && u.Scheme != "https") || u.Host == "" {
		return nil, fmt.Errorf("server must look like http://host:port or https://host:port, got %q", server)
	}
	tr := http.DefaultTransport.(*http.Transport).Clone()
	if caCert != "" {
		pem, err := os.ReadFile(caCert)
		if err != nil {
			return nil, err
		}
		pool, err := x509.SystemCertPool()
		if err != nil || pool == nil {
			pool = x509.NewCertPool()
		}
		if !pool.AppendCertsFromPEM(pem) {
			return nil, fmt.Errorf("%s: no certificates found", caCert)
		}
		tr.TLSClientConfig = &tls.Config{RootCAs: pool, MinVersion: tls.VersionTLS12}
	}
	return &Client{Base: base, HTTP: &http.Client{Transport: tr, Timeout: 30 * time.Second}}, nil
}

func (c *Client) do(ctx context.Context, method, path string, body any, out any) error {
	var rd io.Reader
	if body != nil {
		b, err := json.Marshal(body)
		if err != nil {
			return err
		}
		rd = bytes.NewReader(b)
	}
	req, err := http.NewRequestWithContext(ctx, method, c.Base+path, rd)
	if err != nil {
		return err
	}
	if body != nil {
		req.Header.Set("Content-Type", "application/json")
	}
	if t := c.Token(); t != "" {
		req.Header.Set("Authorization", "Bearer "+t)
	}
	res, err := c.HTTP.Do(req)
	if err != nil {
		return err
	}
	defer res.Body.Close()
	data, _ := io.ReadAll(io.LimitReader(res.Body, 1<<20))
	if res.StatusCode/100 != 2 {
		var e struct {
			Error string `json:"error"`
		}
		_ = json.Unmarshal(data, &e)
		if e.Error == "" {
			e.Error = res.Status
		}
		return &HTTPError{Status: res.StatusCode, Message: e.Error}
	}
	if out != nil && len(data) > 0 {
		return json.Unmarshal(data, out)
	}
	return nil
}

func (c *Client) Health(ctx context.Context) (core.HealthInfo, error) {
	var h core.HealthInfo
	return h, c.do(ctx, http.MethodGet, "/api/health", nil, &h)
}

// Login stores the session token on success.
func (c *Client) Login(ctx context.Context, username, password, clientID string) (core.LoginResponse, error) {
	var r core.LoginResponse
	err := c.do(ctx, http.MethodPost, "/api/login", core.LoginRequest{Username: username, Password: password, Client: clientID}, &r)
	if err == nil {
		c.SetToken(r.Token)
	}
	return r, err
}

func (c *Client) Logout(ctx context.Context) error {
	err := c.do(ctx, http.MethodPost, "/api/logout", nil, nil)
	c.SetToken("")
	return err
}

func (c *Client) Me(ctx context.Context) (core.User, error) {
	var u core.User
	return u, c.do(ctx, http.MethodGet, "/api/me", nil, &u)
}

// Conn is one WebSocket connection.
type Conn struct {
	ws *websocket.Conn
	mu sync.Mutex
}

func (c *Client) wsURL(clientID string) string {
	u := c.Base
	if strings.HasPrefix(u, "https://") {
		u = "wss://" + strings.TrimPrefix(u, "https://")
	} else {
		u = "ws://" + strings.TrimPrefix(u, "http://")
	}
	return u + "/ws?client=" + url.QueryEscape(clientID)
}

// Dial opens /ws with the bearer token.
func (c *Client) Dial(ctx context.Context, clientID string) (*Conn, error) {
	h := http.Header{}
	h.Set("Authorization", "Bearer "+c.Token())
	ws, res, err := websocket.Dial(ctx, c.wsURL(clientID), &websocket.DialOptions{HTTPClient: c.HTTP, HTTPHeader: h})
	if err != nil {
		if res != nil && res.StatusCode != http.StatusSwitchingProtocols {
			return nil, &HTTPError{Status: res.StatusCode, Message: http.StatusText(res.StatusCode)}
		}
		return nil, err
	}
	ws.SetReadLimit(ReadLimit)
	return &Conn{ws: ws}, nil
}

// Send writes one client message.
func (c *Conn) Send(ctx context.Context, msg any) error {
	b, err := core.EncodeClientMsg(msg)
	if err != nil {
		return err
	}
	c.mu.Lock()
	defer c.mu.Unlock()
	return c.ws.Write(ctx, websocket.MessageText, b)
}

// Recv reads one server message.
func (c *Conn) Recv(ctx context.Context) (any, error) {
	for {
		typ, data, err := c.ws.Read(ctx)
		if err != nil {
			return nil, err
		}
		if typ == websocket.MessageText {
			return core.DecodeServerMsg(data)
		}
	}
}

func (c *Conn) Close() { c.ws.Close(websocket.StatusNormalClosure, "bye") }

// ---- reconnecting session ----

type EventKind int

const (
	Connected EventKind = iota
	Disconnected
	Message
	AuthFailed
)

type Event struct {
	Kind EventKind
	Msg  any   // Message
	Err  error // Disconnected / AuthFailed
}

// Session keeps a connection open, reconnecting with backoff (0.5 s → 5 s)
// until ctx ends or the server rejects the session (AuthFailed).
type Session struct {
	client   *Client
	clientID string
	events   chan Event
	mu       sync.Mutex
	conn     *Conn
}

var ErrOffline = errors.New("not connected")

func (c *Client) Start(ctx context.Context, clientID string) *Session {
	s := &Session{client: c, clientID: clientID, events: make(chan Event, 256)}
	go s.run(ctx)
	return s
}

func (s *Session) Events() <-chan Event { return s.events }

func (s *Session) emit(ctx context.Context, e Event) bool {
	select {
	case s.events <- e:
		return true
	case <-ctx.Done():
		return false
	}
}

func (s *Session) run(ctx context.Context) {
	defer close(s.events)
	backoff := 500 * time.Millisecond
	for ctx.Err() == nil {
		conn, err := s.client.Dial(ctx, s.clientID)
		if err != nil {
			if IsUnauthorized(err) {
				s.emit(ctx, Event{Kind: AuthFailed, Err: err})
				return
			}
			if !s.emit(ctx, Event{Kind: Disconnected, Err: err}) {
				return
			}
		} else {
			backoff = 500 * time.Millisecond
			s.mu.Lock()
			s.conn = conn
			s.mu.Unlock()
			if !s.emit(ctx, Event{Kind: Connected}) {
				conn.Close()
				return
			}
			for {
				msg, err := conn.Recv(ctx)
				if err != nil {
					s.mu.Lock()
					s.conn = nil
					s.mu.Unlock()
					conn.ws.CloseNow()
					if !s.emit(ctx, Event{Kind: Disconnected, Err: err}) {
						return
					}
					break
				}
				if !s.emit(ctx, Event{Kind: Message, Msg: msg}) {
					conn.Close()
					return
				}
			}
		}
		select {
		case <-ctx.Done():
			return
		case <-time.After(backoff):
		}
		backoff = min(backoff*2, 5*time.Second)
	}
}

// Send writes on the current connection, or returns ErrOffline.
func (s *Session) Send(ctx context.Context, msg any) error {
	s.mu.Lock()
	c := s.conn
	s.mu.Unlock()
	if c == nil {
		return ErrOffline
	}
	return c.Send(ctx, msg)
}

// ---- session cache ----

func sessionsPath() (string, error) {
	dir, err := os.UserConfigDir()
	if err != nil {
		return "", err
	}
	return dir + string(os.PathSeparator) + "pt" + string(os.PathSeparator) + "sessions.json", nil
}

// sessionsFile can be overridden in tests.
var sessionsFile = sessionsPath

func readSessions() (map[string]string, string, error) {
	p, err := sessionsFile()
	if err != nil {
		return nil, "", err
	}
	m := map[string]string{}
	b, err := os.ReadFile(p)
	if err == nil {
		_ = json.Unmarshal(b, &m)
	} else if !errors.Is(err, os.ErrNotExist) {
		return nil, p, err
	}
	return m, p, nil
}

func writeSessions(p string, m map[string]string) error {
	dir := p[:strings.LastIndexAny(p, `/\`)]
	if err := os.MkdirAll(dir, 0o700); err != nil {
		return err
	}
	b, _ := json.MarshalIndent(m, "", "  ")
	tmp := p + ".tmp"
	if err := os.WriteFile(tmp, b, 0o600); err != nil {
		return err
	}
	return os.Rename(tmp, p)
}

func normServer(s string) string { return strings.TrimRight(strings.TrimSpace(s), "/") }

// LoadToken returns the cached token for a server ("" if none).
func LoadToken(server string) string {
	m, _, err := readSessions()
	if err != nil {
		return ""
	}
	return m[normServer(server)]
}

func SaveToken(server, token string) error {
	m, p, err := readSessions()
	if err != nil {
		return err
	}
	m[normServer(server)] = token
	return writeSessions(p, m)
}

func ForgetToken(server string) error {
	m, p, err := readSessions()
	if err != nil {
		return err
	}
	if _, ok := m[normServer(server)]; !ok {
		return nil
	}
	delete(m, normServer(server))
	return writeSessions(p, m)
}
