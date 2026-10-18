package core

// Wire messages (protocol §2–3), byte-compatible with the Rust serde types.
// Every frame is a JSON object with a "type" tag; values are raw JSON.

import (
	"bytes"
	"encoding/json"
	"errors"
	"fmt"
	"strings"
	"unicode/utf8"
)

const (
	ProtocolVersion = 1
	DefaultPort     = 8790
	DefaultBind     = "127.0.0.1:8790"
	DefaultServer   = "http://127.0.0.1:8790"
	MaxFrame        = 256 * 1024 // bytes per text frame
	MaxSockets      = 64
	MaxToast        = 120 // characters, after trimming
)

// ServerInfo identifies a server implementation.
type ServerInfo struct {
	Impl    string `json:"impl"`
	Version string `json:"version"`
}

// HealthInfo is GET /api/health.
type HealthInfo struct {
	OK       bool   `json:"ok"`
	Impl     string `json:"impl"`
	Version  string `json:"version"`
	Protocol int    `json:"protocol"`
}

// Entry is one key's synced state: its value (canonical JSON), version
// (starts at 1, +1 per set/reset), who changed it ("" for a default never
// changed) and when (UTC, "YYYY-MM-DDTHH:MM:SSZ").
type Entry struct {
	Value   json.RawMessage `json:"value"`
	Version int64           `json:"version"`
	By      string          `json:"by"`
	At      string          `json:"at"`
}

// ---- client → server ----

// SetMsg changes one key. Base is the version the edit started from; Force
// skips the conflict check.
type SetMsg struct {
	Req   uint64          `json:"req"`
	Key   string          `json:"key"`
	Base  int64           `json:"base"`
	Value json.RawMessage `json:"value"`
	Force bool            `json:"force,omitempty"`
}

// ResetMsg restores keys to their defaults; empty Keys = every key.
type ResetMsg struct {
	Req  uint64   `json:"req"`
	Keys []string `json:"keys"`
}

// FocusMsg tells peers which tab this client is on (no reply).
type FocusMsg struct {
	Tab string `json:"tab"`
}

// SendToastMsg asks the server to broadcast a toast (1–120 chars).
type SendToastMsg struct {
	Req  uint64 `json:"req"`
	Text string `json:"text"`
}

// PingMsg asks for a PongMsg.
type PingMsg struct{}

// NewSet builds a SetMsg from a Go value (it is marshalled, not validated).
func NewSet(req uint64, key string, base int64, value any) (SetMsg, error) {
	b, err := marshal(value)
	if err != nil {
		return SetMsg{}, err
	}
	return SetMsg{Req: req, Key: key, Base: base, Value: b}, nil
}

// EncodeClientMsg serialises a client message with its "type" tag.
func EncodeClientMsg(m any) ([]byte, error) {
	switch v := m.(type) {
	case SetMsg:
		if len(v.Value) == 0 {
			v.Value = json.RawMessage("null")
		}
		return withType("set", v)
	case ResetMsg:
		if v.Keys == nil {
			v.Keys = []string{}
		}
		return withType("reset", v)
	case FocusMsg:
		return withType("focus", v)
	case SendToastMsg:
		return withType("toast", v)
	case PingMsg:
		return []byte(`{"type":"ping"}`), nil
	}
	return nil, fmt.Errorf("not a client message: %T", m)
}

// ErrBadMessage wraps every DecodeClientMsg failure (the server answers
// with an "error" message and keeps the socket open).
var ErrBadMessage = errors.New("bad message")

func badMsg(f string, a ...any) error {
	return fmt.Errorf("%w: %s", ErrBadMessage, fmt.Sprintf(f, a...))
}

// DecodeClientMsg parses one client frame into SetMsg, ResetMsg, FocusMsg,
// SendToastMsg or PingMsg. Missing required fields and wrong types are
// errors; unknown extra fields are ignored. A set without "base" gets base
// 0, which never matches (versions start at 1), so it conflicts unless
// forced.
func DecodeClientMsg(b []byte) (any, error) {
	var env struct {
		Type  *string         `json:"type"`
		Req   *uint64         `json:"req"`
		Key   *string         `json:"key"`
		Base  *int64          `json:"base"`
		Value json.RawMessage `json:"value"` // a literal null arrives as "null"; missing as empty
		Force *bool           `json:"force"`
		Keys  []string        `json:"keys"`
		Tab   *string         `json:"tab"`
		Text  *string         `json:"text"`
	}
	if err := json.Unmarshal(b, &env); err != nil {
		return nil, badMsg("%v", err)
	}
	if env.Type == nil {
		return nil, badMsg("missing field `type`")
	}
	needReq := func() error {
		if env.Req == nil {
			return badMsg("%s needs `req`", *env.Type)
		}
		return nil
	}
	switch *env.Type {
	case "set":
		if err := needReq(); err != nil {
			return nil, err
		}
		if env.Key == nil || len(env.Value) == 0 {
			return nil, badMsg("set needs `key` and `value`")
		}
		m := SetMsg{Req: *env.Req, Key: *env.Key, Value: env.Value}
		if env.Base != nil {
			m.Base = *env.Base
		}
		if env.Force != nil {
			m.Force = *env.Force
		}
		return m, nil
	case "reset":
		if err := needReq(); err != nil {
			return nil, err
		}
		return ResetMsg{Req: *env.Req, Keys: env.Keys}, nil
	case "focus":
		if env.Tab == nil {
			return nil, badMsg("focus needs `tab`")
		}
		return FocusMsg{Tab: *env.Tab}, nil
	case "toast":
		if err := needReq(); err != nil {
			return nil, err
		}
		if env.Text == nil {
			return nil, badMsg("toast needs `text`")
		}
		return SendToastMsg{Req: *env.Req, Text: *env.Text}, nil
	case "ping":
		return PingMsg{}, nil
	}
	return nil, badMsg("unknown type %q", truncate(*env.Type, 32))
}

// CheckToast trims a toast and checks its length (1–120 characters) and
// that it holds no control characters; it returns the text to broadcast.
func CheckToast(text string) (string, error) {
	t := strings.TrimSpace(text)
	n := utf8.RuneCountInString(t)
	switch {
	case n == 0:
		return "", errors.New("toast must not be empty")
	case n > MaxToast:
		return "", fmt.Errorf("toast must be at most %d characters", MaxToast)
	case hasControl(t, false):
		return "", errors.New("toast must not contain control characters")
	}
	return t, nil
}

// ---- server → client ----

// You is the receiving connection, in welcome.
type You struct {
	Conn  uint64 `json:"conn"`
	Name  string `json:"name"`
	Color string `json:"color"`
}

// WelcomeMsg is the first message on a socket; Values holds every key.
type WelcomeMsg struct {
	Protocol int              `json:"protocol"`
	Server   ServerInfo       `json:"server"`
	You      You              `json:"you"`
	Values   map[string]Entry `json:"values"`
}

// ChangedMsg is broadcast to everyone (the sender too) after a set/reset.
type ChangedMsg struct {
	Key     string          `json:"key"`
	Value   json.RawMessage `json:"value"`
	Version int64           `json:"version"`
	By      string          `json:"by"`
	Conn    uint64          `json:"conn"`
	At      string          `json:"at"`
}

// Entry returns the change as the key's new Entry.
func (c ChangedMsg) Entry() Entry {
	return Entry{Value: c.Value, Version: c.Version, By: c.By, At: c.At}
}

// AckMsg answers set, reset and toast. OK with Version (set only); or
// Error; or Conflict with Current (set only).
type AckMsg struct {
	Req      uint64 `json:"req"`
	OK       bool   `json:"ok"`
	Version  *int64 `json:"version,omitempty"`
	Error    string `json:"error,omitempty"`
	Conflict bool   `json:"conflict,omitempty"`
	Current  *Entry `json:"current,omitempty"`
}

// Peer is one connection in presence. Tab is nil until the peer sends focus.
type Peer struct {
	Conn   uint64  `json:"conn"`
	Name   string  `json:"name"`
	Color  string  `json:"color"`
	Client string  `json:"client"`
	Tab    *string `json:"tab"`
}

// PresenceMsg lists every connection (the receiver included), by conn.
type PresenceMsg struct {
	Peers []Peer `json:"peers"`
}

// ToastMsg is a broadcast toast.
type ToastMsg struct {
	Text string `json:"text"`
	By   string `json:"by"`
	Conn uint64 `json:"conn"`
}

// PongMsg answers ping.
type PongMsg struct{}

// ErrorMsg reports a frame the server could not read (socket stays open).
type ErrorMsg struct {
	Message string `json:"message"`
}

// EncodeServerMsg serialises a server message with its "type" tag.
func EncodeServerMsg(m any) ([]byte, error) {
	switch v := m.(type) {
	case WelcomeMsg:
		if v.Values == nil {
			v.Values = map[string]Entry{}
		}
		return withType("welcome", v)
	case ChangedMsg:
		return withType("changed", v)
	case AckMsg:
		return withType("ack", v)
	case PresenceMsg:
		if v.Peers == nil {
			v.Peers = []Peer{}
		}
		return withType("presence", v)
	case ToastMsg:
		return withType("toast", v)
	case PongMsg:
		return []byte(`{"type":"pong"}`), nil
	case ErrorMsg:
		return withType("error", v)
	}
	return nil, fmt.Errorf("not a server message: %T", m)
}

// DecodeServerMsg parses one server frame into one of the message types
// above (by value).
func DecodeServerMsg(b []byte) (any, error) {
	var env struct {
		Type *string `json:"type"`
	}
	if err := json.Unmarshal(b, &env); err != nil {
		return nil, err
	}
	if env.Type == nil {
		return nil, errors.New("missing field `type`")
	}
	var (
		msg any
		err error
	)
	switch *env.Type {
	case "welcome":
		var m WelcomeMsg
		err, msg = json.Unmarshal(b, &m), &m
	case "changed":
		var m ChangedMsg
		err, msg = json.Unmarshal(b, &m), &m
	case "ack":
		var m AckMsg
		err, msg = json.Unmarshal(b, &m), &m
	case "presence":
		var m PresenceMsg
		err, msg = json.Unmarshal(b, &m), &m
	case "toast":
		var m ToastMsg
		err, msg = json.Unmarshal(b, &m), &m
	case "pong":
		return PongMsg{}, nil
	case "error":
		var m ErrorMsg
		err, msg = json.Unmarshal(b, &m), &m
	default:
		return nil, fmt.Errorf("unknown server message %q", *env.Type)
	}
	if err != nil {
		return nil, err
	}
	return deref(msg), nil
}

func deref(p any) any {
	switch m := p.(type) {
	case *WelcomeMsg:
		return *m
	case *ChangedMsg:
		return *m
	case *AckMsg:
		return *m
	case *PresenceMsg:
		return *m
	case *ToastMsg:
		return *m
	case *ErrorMsg:
		return *m
	}
	return p
}

// withType marshals v and inserts "type" as the first key.
func withType(tag string, v any) ([]byte, error) {
	b, err := marshal(v)
	if err != nil {
		return nil, err
	}
	head := []byte(`{"type":"` + tag + `"`)
	if bytes.Equal(b, []byte("{}")) {
		return append(head, '}'), nil
	}
	return append(append(head, ','), b[1:]...), nil
}

// ValidClientID: [a-z0-9-]{1,24}.
func ValidClientID(s string) bool {
	if len(s) == 0 || len(s) > 24 {
		return false
	}
	for i := 0; i < len(s); i++ {
		c := s[i]
		if !(c >= 'a' && c <= 'z' || c >= '0' && c <= '9' || c == '-') {
			return false
		}
	}
	return true
}

// CleanName trims a display name and checks it against
// ^[A-Za-z][A-Za-z0-9 _.-]{0,23}$; ok is false when it doesn't match.
func CleanName(name string) (string, bool) {
	s := strings.Trim(name, " ")
	if len(s) == 0 || len(s) > 24 {
		return s, false
	}
	for i := 0; i < len(s); i++ {
		c := s[i]
		letter := c >= 'A' && c <= 'Z' || c >= 'a' && c <= 'z'
		if i == 0 && !letter {
			return s, false
		}
		if !letter && !(c >= '0' && c <= '9') && c != ' ' && c != '_' && c != '.' && c != '-' {
			return s, false
		}
	}
	return s, true
}
