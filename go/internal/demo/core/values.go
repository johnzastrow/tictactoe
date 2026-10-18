package core

// The client-side value store shared by the TUI and the web client: every
// key's synced Entry, kept current from welcome / changed / conflict acks,
// with typed getters and a helper that validates and builds a set.

import (
	"encoding/json"
	"sort"
	"strconv"
	"strings"
	"time"
	"unicode"
)

// Values is one client's copy of the server's values. It is not safe for
// concurrent use (each front end touches it from its UI loop only).
type Values struct {
	entries map[string]Entry
	loaded  bool

	// From the last welcome.
	Server ServerInfo
	You    You
	Peers  []Peer // last presence, sorted by conn
}

// NewValues starts with every key at its default, version 0 (not yet synced).
func NewValues() *Values {
	v := &Values{entries: make(map[string]Entry, len(Keys))}
	for _, k := range Keys {
		v.entries[k] = Entry{Value: Default(k)}
	}
	return v
}

// Loaded reports whether a welcome has arrived.
func (v *Values) Loaded() bool { return v.loaded }

// ApplyWelcome replaces every value (also on reconnect). Unknown keys are
// ignored; keys the server didn't send keep their current entries.
func (v *Values) ApplyWelcome(m WelcomeMsg) {
	v.Server, v.You, v.loaded = m.Server, m.You, true
	for k, e := range m.Values {
		if IsKey(k) {
			v.entries[k] = e
		}
	}
}

// ApplyChanged applies a broadcast change. It returns false (and changes
// nothing) for an unknown key or a version older than the one held, so a
// late message can't roll a value back.
func (v *Values) ApplyChanged(m ChangedMsg) bool {
	return v.ApplyEntry(m.Key, m.Entry())
}

// ApplyEntry stores an entry for key if it is not older than the current
// one (use it for an ack's conflict Current). It returns whether it did.
func (v *Values) ApplyEntry(key string, e Entry) bool {
	cur, ok := v.entries[key]
	if !ok || e.Version < cur.Version {
		return false
	}
	v.entries[key] = e
	return true
}

// ApplyPresence stores the peer list (sorted by conn).
func (v *Values) ApplyPresence(m PresenceMsg) {
	peers := append([]Peer(nil), m.Peers...)
	sort.Slice(peers, func(i, j int) bool { return peers[i].Conn < peers[j].Conn })
	v.Peers = peers
}

// Apply feeds any server message to the store: welcome, changed and
// presence update it; others are ignored. It returns true when a value or
// the peer list changed.
func (v *Values) Apply(msg any) bool {
	switch m := msg.(type) {
	case WelcomeMsg:
		v.ApplyWelcome(m)
		return true
	case ChangedMsg:
		return v.ApplyChanged(m)
	case PresenceMsg:
		v.ApplyPresence(m)
		return true
	}
	return false
}

// OtherPeers is Peers without this connection.
func (v *Values) OtherPeers() []Peer {
	var out []Peer
	for _, p := range v.Peers {
		if p.Conn != v.You.Conn {
			out = append(out, p)
		}
	}
	return out
}

// Get returns a key's entry (zero Entry for an unknown key).
func (v *Values) Get(key string) Entry { return v.entries[key] }

// Raw returns a key's value as canonical JSON.
func (v *Values) Raw(key string) json.RawMessage { return v.entries[key].Value }

// Version returns a key's version (0 before the welcome).
func (v *Values) Version(key string) int64 { return v.entries[key].Version }

// Describe is the Value panel's second line: "v4 · by bob · 2m ago"
// ("v1 · default" for an unchanged default).
func (v *Values) Describe(key string, now time.Time) string {
	e := v.entries[key]
	s := "v" + itoa(e.Version)
	if e.By == "" {
		return s + " · default"
	}
	return s + " · by " + e.By + " · " + RelativeTime(e.At, now)
}

func itoa(n int64) string { return strconv.FormatInt(n, 10) }

// SafeText replaces control characters (other than newline and tab) with
// U+FFFD, for showing text that came from another peer: a server that is
// laxer than this core must not be able to send terminal escapes to a TUI.
func SafeText(s string) string {
	if !hasControl(s, true) {
		return s
	}
	return strings.Map(func(r rune) rune {
		if unicode.IsControl(r) && r != '\n' && r != '\t' {
			return '\uFFFD'
		}
		return r
	}, s)
}

// PrepareSet validates value for key locally (the same rules the server
// applies, including the date cross-key rule) and builds the SetMsg with
// base = the version held now. The caller fills Req. On error nothing
// should be sent; the error text is what the control shows.
func (v *Values) PrepareSet(key string, value any) (SetMsg, error) {
	norm, err := NormalizeValue(key, value)
	if err != nil {
		return SetMsg{}, err
	}
	if err := CheckCross(key, norm, v.Raw); err != nil {
		return SetMsg{}, err
	}
	return SetMsg{Key: key, Base: v.Version(key), Value: norm}, nil
}

// get decodes a key's value into out, falling back to the default (which
// always decodes) if the held value doesn't fit the type.
func (v *Values) get(key string, out any) {
	if json.Unmarshal(v.entries[key].Value, out) != nil {
		_ = json.Unmarshal(Default(key), out)
	}
}

// ---- typed getters (key names as in protocol §5) ----

// String: text.*, select.*, choice.size, palette.last, textarea.notes.
func (v *Values) String(key string) string {
	var s string
	v.get(key, &s)
	return s
}

// OptString: date.*, combo.* (nil = null).
func (v *Values) OptString(key string) *string {
	var s *string
	v.get(key, &s)
	return s
}

// Float: number.hours.
func (v *Values) Float(key string) float64 {
	var f float64
	v.get(key, &f)
	return f
}

// Int: number.qty, number.percent.
func (v *Values) Int(key string) int64 {
	var n float64 // tolerate 3.0 from another implementation
	v.get(key, &n)
	return int64(n)
}

// Bool: choice.notify, choice.dark.
func (v *Values) Bool(key string) bool {
	var b bool
	v.get(key, &b)
	return b
}

// Strings: tags.tags, choice.channels.
func (v *Values) Strings(key string) []string {
	var s []string
	v.get(key, &s)
	return s
}

// Date: date.start / date.due as a Date (ok false for null).
func (v *Values) Date(key string) (Date, bool) {
	s := v.OptString(key)
	if s == nil {
		return 0, false
	}
	return ParseISO(*s)
}

func (v *Values) Cascade() Cascade {
	var c Cascade
	v.get("cascade.pick", &c)
	return c
}

func (v *Values) GridRows() []GridRow {
	var r []GridRow
	v.get("grid.rows", &r)
	return r
}

func (v *Values) Files() []File {
	var f []File
	v.get("menus.files", &f)
	return f
}

func (v *Values) Profile() Profile {
	var p Profile
	v.get("dialog.profile", &p)
	return p
}

func (v *Values) Series() []int64 {
	var s []int64
	v.get("charts.series", &s)
	return s
}

func (v *Values) Cards() []Card {
	var c []Card
	v.get("kanban.cards", &c)
	return c
}

func (v *Values) Bars() []Bar {
	var b []Bar
	v.get("gantt.bars", &b)
	return b
}

// NextID is max(id)+1 over a list's ids (1 for an empty list), as clients
// assign new grid rows, cards and bars.
func NextID(ids []int64) int64 {
	var m int64
	for _, id := range ids {
		m = max(m, id)
	}
	return m + 1
}
