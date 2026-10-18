//go:build js && wasm

package webapp

// WebSocket plumbing, adapted from pt's web client. Everything is callback
// based: Go code runs inside JS callbacks on the one browser thread, so no
// goroutines and no locks are needed.

import (
	"syscall/js"
)

// Socket is the app's WebSocket with automatic reconnect.
type Socket struct {
	Name      string // display name, sent as ?name=
	ws        js.Value
	scope     Scope
	backoffMs int
	Failures  int // consecutive closes without a successful open
	stopped   bool
	OnOpen    func()
	OnMessage func(text string)
	OnClose   func(code int, reason string)
}

// wsURL is /ws on the page's own origin (protocol §1: same-origin only).
func (s *Socket) wsURL() string {
	loc := global.Get("location")
	proto := "ws://"
	if loc.Get("protocol").String() == "https:" {
		proto = "wss://"
	}
	name := global.Call("encodeURIComponent", s.Name).String()
	return proto + loc.Get("host").String() + "/ws?client=web-go&name=" + name
}

func (s *Socket) Connect() {
	s.stopped = false
	s.scope.Release()
	ws := global.Get("WebSocket").New(s.wsURL())
	s.ws = ws
	s.scope.On(ws, "open", func(js.Value) {
		s.backoffMs = 0
		s.Failures = 0
		if s.OnOpen != nil {
			s.OnOpen()
		}
	})
	s.scope.On(ws, "message", func(ev js.Value) {
		if s.OnMessage != nil {
			s.OnMessage(ev.Get("data").String())
		}
	})
	s.scope.On(ws, "close", func(ev js.Value) {
		if !ws.Equal(s.ws) {
			return
		}
		s.Failures++
		code := ev.Get("code").Int()
		reason := ev.Get("reason").String()
		s.ws = js.Null()
		if s.OnClose != nil {
			s.OnClose(code, reason)
		}
	})
}

// Retry schedules a reconnect with exponential backoff (0.5 s → 5 s).
func (s *Socket) Retry() {
	if s.stopped {
		return
	}
	if s.backoffMs == 0 {
		s.backoffMs = 500
	} else {
		s.backoffMs = min(s.backoffMs*2, 5000)
	}
	After(s.backoffMs, func() {
		if !s.stopped && (s.ws.IsNull() || s.ws.IsUndefined()) {
			s.Connect()
		}
	})
}

func (s *Socket) Open() bool {
	return s.ws.Truthy() && s.ws.Get("readyState").Int() == 1
}

func (s *Socket) Send(b []byte) bool {
	if !s.Open() {
		return false
	}
	s.ws.Call("send", string(b))
	return true
}

// Stop closes the socket for good (File ▸ Leave).
func (s *Socket) Stop() {
	s.stopped = true
	if s.ws.Truthy() {
		w := s.ws
		s.ws = js.Null()
		w.Call("close")
	}
	s.scope.Release()
}
