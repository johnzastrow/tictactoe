package core

// Wire messages (protocol §5–6), byte-compatible with the Rust serde types.

import (
	"bytes"
	"encoding/json"
	"errors"
	"fmt"
)

const (
	ProtocolVersion = 1
	DefaultPort     = 8788
	MaxFrame        = 512 * 1024
	MaxOps          = 2000
	Version         = "0.4.0"
)

// Op is one mutate operation. Kind is "set", "create" or "delete".
type Op struct {
	Kind   string
	Entity Entity
	ID     int64
	Field  string
	Base   any
	Value  any
	Force  bool
	Ref    *string
	Values map[string]any
}

func SetOp(e Entity, id int64, field string, base, value any) Op {
	return Op{Kind: "set", Entity: e, ID: id, Field: field, Base: base, Value: value}
}

func CreateOp(e Entity, ref string, values map[string]any) Op {
	op := Op{Kind: "create", Entity: e, Values: values}
	if ref != "" {
		op.Ref = &ref
	}
	return op
}

func DeleteOp(e Entity, id int64) Op { return Op{Kind: "delete", Entity: e, ID: id} }

func (o Op) MarshalJSON() ([]byte, error) {
	switch o.Kind {
	case "set":
		m := map[string]any{"op": "set", "entity": o.Entity, "id": o.ID, "field": o.Field, "base": o.Base, "value": o.Value}
		if o.Force {
			m["force"] = true
		}
		return json.Marshal(m)
	case "create":
		vals := o.Values
		if vals == nil {
			vals = map[string]any{}
		}
		m := map[string]any{"op": "create", "entity": o.Entity, "values": vals}
		if o.Ref != nil {
			m["ref"] = *o.Ref
		}
		return json.Marshal(m)
	case "delete":
		return json.Marshal(map[string]any{"op": "delete", "entity": o.Entity, "id": o.ID})
	}
	return nil, fmt.Errorf("unknown op %q", o.Kind)
}

func (o *Op) UnmarshalJSON(b []byte) error {
	var raw struct {
		Op     *string         `json:"op"`
		Entity *Entity         `json:"entity"`
		ID     *int64          `json:"id"`
		Field  *string         `json:"field"`
		Base   any             `json:"base"`
		Value  any             `json:"value"`
		Force  bool            `json:"force"`
		Ref    *string         `json:"ref"`
		Values *map[string]any `json:"values"`
	}
	if err := decodeNumbers(b, &raw); err != nil {
		return err
	}
	if raw.Op == nil {
		return errors.New("missing field `op`")
	}
	if raw.Entity == nil {
		return errors.New("missing field `entity`")
	}
	*o = Op{Kind: *raw.Op, Entity: *raw.Entity}
	switch *raw.Op {
	case "set":
		if raw.ID == nil || raw.Field == nil {
			return errors.New("set needs `id` and `field`")
		}
		o.ID, o.Field, o.Base, o.Value, o.Force = *raw.ID, *raw.Field, raw.Base, raw.Value, raw.Force
	case "create":
		o.Ref = raw.Ref
		o.Values = map[string]any{}
		if raw.Values != nil && *raw.Values != nil {
			o.Values = *raw.Values
		}
	case "delete":
		if raw.ID == nil {
			return errors.New("delete needs `id`")
		}
		o.ID = *raw.ID
	default:
		return fmt.Errorf("unknown op %q", *raw.Op)
	}
	return nil
}

// decodeNumbers decodes JSON numbers as float64 for `any` fields (like
// serde_json::Value they keep integers exact up to 2^53).
func decodeNumbers(b []byte, v any) error {
	return json.Unmarshal(b, v)
}

// ---- client → server ----

type MutateMsg struct {
	Req uint64
	Ops []Op
}
type UndoMsg struct{ Req uint64 }
type RedoMsg struct{ Req uint64 }
type FocusMsg struct {
	View   *string `json:"view,omitempty"`
	Entity *Entity `json:"entity,omitempty"`
	ID     *int64  `json:"id,omitempty"`
	Field  *string `json:"field,omitempty"`
}
type SetPasswordMsg struct {
	Req         uint64  `json:"req"`
	UserID      int64   `json:"user_id"`
	Password    string  `json:"password"`
	OldPassword *string `json:"old_password,omitempty"`
}
type ResyncMsg struct{}
type PingMsg struct{}

// EncodeClientMsg serialises any client message with its "type" tag.
func EncodeClientMsg(m any) ([]byte, error) {
	switch v := m.(type) {
	case MutateMsg:
		ops := v.Ops
		if ops == nil {
			ops = []Op{}
		}
		return json.Marshal(map[string]any{"type": "mutate", "req": v.Req, "ops": ops})
	case UndoMsg:
		return json.Marshal(map[string]any{"type": "undo", "req": v.Req})
	case RedoMsg:
		return json.Marshal(map[string]any{"type": "redo", "req": v.Req})
	case FocusMsg:
		return withType("focus", v)
	case SetPasswordMsg:
		return withType("set_password", v)
	case ResyncMsg:
		return []byte(`{"type":"resync"}`), nil
	case PingMsg:
		return []byte(`{"type":"ping"}`), nil
	}
	return nil, fmt.Errorf("not a client message: %T", m)
}

// withType marshals v and inserts "type" as the first key.
func withType(tag string, v any) ([]byte, error) {
	b, err := json.Marshal(v)
	if err != nil {
		return nil, err
	}
	head := []byte(`{"type":"` + tag + `"`)
	if bytes.Equal(b, []byte("{}")) {
		return append(head, '}'), nil
	}
	return append(append(head, ','), b[1:]...), nil
}

func peekType(b []byte) (string, error) {
	var env struct {
		Type *string `json:"type"`
	}
	if err := json.Unmarshal(b, &env); err != nil {
		return "", err
	}
	if env.Type == nil {
		return "", errors.New("missing field `type`")
	}
	return *env.Type, nil
}

// DecodeClientMsg parses one client frame into a typed message.
func DecodeClientMsg(b []byte) (any, error) {
	t, err := peekType(b)
	if err != nil {
		return nil, err
	}
	switch t {
	case "mutate":
		var raw struct {
			Req *uint64 `json:"req"`
			Ops *[]Op   `json:"ops"`
		}
		if err := json.Unmarshal(b, &raw); err != nil {
			return nil, err
		}
		if raw.Req == nil || raw.Ops == nil {
			return nil, errors.New("mutate needs `req` and `ops`")
		}
		return MutateMsg{Req: *raw.Req, Ops: *raw.Ops}, nil
	case "undo", "redo":
		var raw struct {
			Req *uint64 `json:"req"`
		}
		if err := json.Unmarshal(b, &raw); err != nil {
			return nil, err
		}
		if raw.Req == nil {
			return nil, errors.New("missing field `req`")
		}
		if t == "undo" {
			return UndoMsg{*raw.Req}, nil
		}
		return RedoMsg{*raw.Req}, nil
	case "focus":
		var m FocusMsg
		return m, json.Unmarshal(b, &m)
	case "set_password":
		var raw struct {
			Req         *uint64 `json:"req"`
			UserID      *int64  `json:"user_id"`
			Password    *string `json:"password"`
			OldPassword *string `json:"old_password"`
		}
		if err := json.Unmarshal(b, &raw); err != nil {
			return nil, err
		}
		if raw.Req == nil || raw.UserID == nil || raw.Password == nil {
			return nil, errors.New("set_password needs `req`, `user_id`, `password`")
		}
		return SetPasswordMsg{*raw.Req, *raw.UserID, *raw.Password, raw.OldPassword}, nil
	case "resync":
		return ResyncMsg{}, nil
	case "ping":
		return PingMsg{}, nil
	}
	return nil, fmt.Errorf("unknown variant `%s`", t)
}

// ---- server → client ----

type ServerInfo struct {
	Impl    string `json:"impl"`
	Version string `json:"version"`
}

type Conflict struct {
	Index   int    `json:"index"`
	Entity  Entity `json:"entity"`
	ID      int64  `json:"id"`
	Field   string `json:"field"`
	Current any    `json:"current"`
	By      *int64 `json:"by,omitempty"`
}

type ErrorBody struct {
	Code    string `json:"code"`
	Message string `json:"message"`
	Index   *int   `json:"index,omitempty"`
}

func NewError(code, msg string) *ErrorBody { return &ErrorBody{Code: code, Message: msg} }

func (e *ErrorBody) At(i int) *ErrorBody {
	e.Index = &i
	return e
}

type ResultMsg struct {
	Req       uint64           `json:"req"`
	OK        bool             `json:"ok"`
	Group     *int64           `json:"group,omitempty"`
	Created   map[string]int64 `json:"created,omitempty"`
	Conflicts []Conflict       `json:"conflicts,omitempty"`
	Error     *ErrorBody       `json:"error,omitempty"`
}

type Focus struct {
	Entity Entity  `json:"entity"`
	ID     int64   `json:"id"`
	Field  *string `json:"field,omitempty"`
}

type Peer struct {
	Conn   uint64  `json:"conn"`
	UserID int64   `json:"user_id"`
	Client string  `json:"client"`
	View   *string `json:"view,omitempty"`
	Focus  *Focus  `json:"focus,omitempty"`
}

type Changes struct {
	By       *int64     `json:"by,omitempty"`
	Group    *int64     `json:"group,omitempty"`
	Upserts  Tables     `json:"upserts"`
	Deletes  Deletes    `json:"deletes"`
	Activity []Activity `json:"activity,omitempty"`
}

type WelcomeMsg struct {
	Protocol uint32     `json:"protocol"`
	Server   ServerInfo `json:"server"`
	Me       User       `json:"me"`
	Conn     uint64     `json:"conn"`
}
type SnapshotMsg struct {
	Data Tables `json:"data"`
}
type PresenceMsg struct {
	Peers []Peer `json:"peers"`
}
type ErrorMsg struct {
	Code    string `json:"code"`
	Message string `json:"message"`
}
type PongMsg struct{}

// EncodeServerMsg serialises any server message with its "type" tag.
func EncodeServerMsg(m any) ([]byte, error) {
	switch v := m.(type) {
	case WelcomeMsg:
		return withType("welcome", v)
	case SnapshotMsg:
		return withType("snapshot", v)
	case Changes:
		return withType("changes", v)
	case *Changes:
		return withType("changes", *v)
	case ResultMsg:
		return withType("result", v)
	case PresenceMsg:
		if v.Peers == nil {
			v.Peers = []Peer{}
		}
		return withType("presence", v)
	case ErrorMsg:
		return withType("error", v)
	case PongMsg:
		return []byte(`{"type":"pong"}`), nil
	}
	return nil, fmt.Errorf("not a server message: %T", m)
}

// DecodeServerMsg parses one server frame.
func DecodeServerMsg(b []byte) (any, error) {
	t, err := peekType(b)
	if err != nil {
		return nil, err
	}
	switch t {
	case "welcome":
		var m WelcomeMsg
		return m, json.Unmarshal(b, &m)
	case "snapshot":
		var m SnapshotMsg
		return m, json.Unmarshal(b, &m)
	case "changes":
		var m Changes
		return m, json.Unmarshal(b, &m)
	case "result":
		var m ResultMsg
		return m, json.Unmarshal(b, &m)
	case "presence":
		var m PresenceMsg
		return m, json.Unmarshal(b, &m)
	case "error":
		var m ErrorMsg
		return m, json.Unmarshal(b, &m)
	case "pong":
		return PongMsg{}, nil
	}
	return nil, fmt.Errorf("unknown server message %q", t)
}

// HTTP bodies.
type LoginRequest struct {
	Username string `json:"username"`
	Password string `json:"password"`
	Client   string `json:"client"`
}

type LoginResponse struct {
	Token string `json:"token"`
	User  User   `json:"user"`
}

type HealthInfo struct {
	OK       bool   `json:"ok"`
	Impl     string `json:"impl"`
	Version  string `json:"version"`
	Protocol uint32 `json:"protocol"`
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
