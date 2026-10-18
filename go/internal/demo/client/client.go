// Package client is the demo protocol client used by the Go TUI: health
// check, the WebSocket connection (client id + display name, no login) and
// a Session that reconnects with backoff and delivers server messages on a
// channel.
package client

import (
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
	"sync/atomic"
	"time"

	"github.com/coder/websocket"

	"git.wharf.fluidgrid.org/jcz/tictactoe/go/internal/demo/core"
)

// ReadLimit bounds one server frame (a welcome is a few tens of KB).
const ReadLimit = 4 << 20

// HTTPError is a non-2xx response (also a refused WebSocket upgrade).
type HTTPError struct {
	Status  int
	Message string
}

func (e *HTTPError) Error() string { return fmt.Sprintf("%d: %s", e.Status, e.Message) }

// IsRejected reports a refusal that retrying won't fix: 400 (bad client id
// or name) or 403 (origin).
func IsRejected(err error) bool {
	var he *HTTPError
	return errors.As(err, &he) && (he.Status == http.StatusBadRequest || he.Status == http.StatusForbidden)
}

type Client struct {
	Base string // http(s)://host:port, no trailing slash
	HTTP *http.Client
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

// Health is GET /api/health.
func (c *Client) Health(ctx context.Context) (core.HealthInfo, error) {
	var h core.HealthInfo
	req, err := http.NewRequestWithContext(ctx, http.MethodGet, c.Base+"/api/health", nil)
	if err != nil {
		return h, err
	}
	res, err := c.HTTP.Do(req)
	if err != nil {
		return h, err
	}
	defer res.Body.Close()
	data, _ := io.ReadAll(io.LimitReader(res.Body, 1<<20))
	if res.StatusCode/100 != 2 {
		return h, &HTTPError{Status: res.StatusCode, Message: errorBody(data, res.Status)}
	}
	return h, json.Unmarshal(data, &h)
}

func errorBody(data []byte, fallback string) string {
	var e struct {
		Error string `json:"error"`
	}
	if json.Unmarshal(data, &e) == nil && e.Error != "" {
		return e.Error
	}
	return fallback
}

// WSURL is the socket URL for a client id and display name.
func (c *Client) WSURL(clientID, name string) string {
	u := c.Base
	if strings.HasPrefix(u, "https://") {
		u = "wss://" + strings.TrimPrefix(u, "https://")
	} else {
		u = "ws://" + strings.TrimPrefix(u, "http://")
	}
	q := url.Values{"client": {clientID}, "name": {name}}
	return u + "/ws?" + q.Encode()
}

// Conn is one WebSocket connection.
type Conn struct {
	ws *websocket.Conn
	mu sync.Mutex
}

// Dial opens /ws. A refused upgrade is an *HTTPError (400 bad name or id,
// 403 origin, 503 server full).
func (c *Client) Dial(ctx context.Context, clientID, name string) (*Conn, error) {
	ws, res, err := websocket.Dial(ctx, c.WSURL(clientID, name), &websocket.DialOptions{HTTPClient: c.HTTP})
	if err != nil {
		if res != nil && res.StatusCode != http.StatusSwitchingProtocols {
			msg := http.StatusText(res.StatusCode)
			if res.Body != nil {
				data, _ := io.ReadAll(io.LimitReader(res.Body, 4096))
				msg = errorBody(data, msg)
			}
			return nil, &HTTPError{Status: res.StatusCode, Message: msg}
		}
		return nil, err
	}
	ws.SetReadLimit(ReadLimit)
	return &Conn{ws: ws}, nil
}

// Send writes one client message (core.SetMsg, ResetMsg, FocusMsg,
// SendToastMsg, PingMsg).
func (c *Conn) Send(ctx context.Context, msg any) error {
	b, err := core.EncodeClientMsg(msg)
	if err != nil {
		return err
	}
	return c.SendRaw(ctx, b)
}

// SendRaw writes one text frame as is (tests).
func (c *Conn) SendRaw(ctx context.Context, b []byte) error {
	c.mu.Lock()
	defer c.mu.Unlock()
	return c.ws.Write(ctx, websocket.MessageText, b)
}

// Recv reads the next server message (core.WelcomeMsg, ChangedMsg, AckMsg,
// PresenceMsg, ToastMsg, PongMsg or ErrorMsg, by value). Frames it can't
// decode are skipped, so a newer server's extra messages are harmless.
func (c *Conn) Recv(ctx context.Context) (any, error) {
	for {
		typ, data, err := c.ws.Read(ctx)
		if err != nil {
			return nil, err
		}
		if typ != websocket.MessageText {
			continue
		}
		if m, err := core.DecodeServerMsg(data); err == nil {
			return m, nil
		}
	}
}

func (c *Conn) Close() { c.ws.Close(websocket.StatusNormalClosure, "bye") }

// CloseStatus is the close code of a Recv error (-1 if it isn't a close).
func CloseStatus(err error) int { return int(websocket.CloseStatus(err)) }

// ---- reconnecting session ----

type EventKind int

const (
	Connected    EventKind = iota // a socket is open; a welcome follows as a Message
	Disconnected                  // the socket dropped or a dial failed; retrying
	Message                       // Msg holds a server message
	Rejected                      // the server refused us (bad name/id); the session has stopped
)

type Event struct {
	Kind EventKind
	Msg  any   // Message
	Err  error // Disconnected / Rejected
}

// Session keeps a connection open, reconnecting with backoff (0.5 s → 5 s)
// until ctx ends or the server rejects the client (Rejected). Request ids
// (`req`) are assigned per session, increasing from 1.
type Session struct {
	client   *Client
	clientID string
	name     string
	events   chan Event
	req      atomic.Uint64
	mu       sync.Mutex
	conn     *Conn
}

// ErrOffline is returned by sends while no socket is open.
var ErrOffline = errors.New("not connected")

// Start connects in the background; read Events() until it is closed.
func (c *Client) Start(ctx context.Context, clientID, name string) *Session {
	s := &Session{client: c, clientID: clientID, name: name, events: make(chan Event, 256)}
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

func (s *Session) setConn(c *Conn) {
	s.mu.Lock()
	s.conn = c
	s.mu.Unlock()
}

func (s *Session) run(ctx context.Context) {
	defer close(s.events)
	backoff := 500 * time.Millisecond
	for ctx.Err() == nil {
		conn, err := s.client.Dial(ctx, s.clientID, s.name)
		if err != nil {
			if IsRejected(err) {
				s.emit(ctx, Event{Kind: Rejected, Err: err})
				return
			}
			if !s.emit(ctx, Event{Kind: Disconnected, Err: err}) {
				return
			}
		} else {
			backoff = 500 * time.Millisecond
			s.setConn(conn)
			if !s.emit(ctx, Event{Kind: Connected}) {
				conn.Close()
				return
			}
			for {
				msg, err := conn.Recv(ctx)
				if err != nil {
					s.setConn(nil)
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

// Online reports whether a socket is open now.
func (s *Session) Online() bool {
	s.mu.Lock()
	defer s.mu.Unlock()
	return s.conn != nil
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

// NextReq returns a fresh request id.
func (s *Session) NextReq() uint64 { return s.req.Add(1) }

// Set sends m (e.g. from core.Values.PrepareSet) with a fresh req, which it
// returns so the caller can match the ack.
func (s *Session) Set(ctx context.Context, m core.SetMsg) (uint64, error) {
	m.Req = s.NextReq()
	return m.Req, s.Send(ctx, m)
}

// SetValue marshals value and sends a set for key based on version base.
func (s *Session) SetValue(ctx context.Context, key string, base int64, value any, force bool) (uint64, error) {
	m, err := core.NewSet(0, key, base, value)
	if err != nil {
		return 0, err
	}
	m.Force = force
	return s.Set(ctx, m)
}

// Reset restores keys to their defaults (every key when none are given).
func (s *Session) Reset(ctx context.Context, keys ...string) (uint64, error) {
	req := s.NextReq()
	return req, s.Send(ctx, core.ResetMsg{Req: req, Keys: keys})
}

// Focus tells peers which tab this client shows (no reply).
func (s *Session) Focus(ctx context.Context, tab string) error {
	return s.Send(ctx, core.FocusMsg{Tab: tab})
}

// Toast broadcasts a toast to everyone (1–120 characters).
func (s *Session) Toast(ctx context.Context, text string) (uint64, error) {
	req := s.NextReq()
	return req, s.Send(ctx, core.SendToastMsg{Req: req, Text: text})
}

// Ping asks for a pong.
func (s *Session) Ping(ctx context.Context) error { return s.Send(ctx, core.PingMsg{}) }
