//go:build js && wasm

package webapp

// HTTP (fetch) and WebSocket plumbing. Everything is callback based: Go
// code runs inside JS callbacks on the one browser thread, so no goroutines
// and no locks are needed.

import (
	"syscall/js"
)

// fetchText runs fetch(path) and calls done(status, body); status 0 = network error.
func fetchText(method, path, body string, done func(status int, text string)) {
	opts := js.Global().Get("Object").New()
	opts.Set("method", method)
	opts.Set("credentials", "same-origin")
	if body != "" {
		h := js.Global().Get("Object").New()
		h.Set("Content-Type", "application/json")
		opts.Set("headers", h)
		opts.Set("body", body)
	}
	var onResp, onText, onErr js.Func
	status := 0
	release := func() {
		onResp.Release()
		onText.Release()
		onErr.Release()
	}
	onText = js.FuncOf(func(_ js.Value, args []js.Value) any {
		t := ""
		if len(args) > 0 {
			t = args[0].String()
		}
		release()
		safe(func() { done(status, t) })
		return nil
	})
	onResp = js.FuncOf(func(_ js.Value, args []js.Value) any {
		resp := args[0]
		status = resp.Get("status").Int()
		resp.Call("text").Call("then", onText).Call("catch", onErr)
		return nil
	})
	onErr = js.FuncOf(func(_ js.Value, args []js.Value) any {
		msg := "network error"
		if len(args) > 0 && args[0].Truthy() {
			msg = args[0].Call("toString").String()
		}
		release()
		safe(func() { done(0, msg) })
		return nil
	})
	global.Call("fetch", path, opts).Call("then", onResp).Call("catch", onErr)
}

// Socket is the app's WebSocket with automatic reconnect.
type Socket struct {
	ws        js.Value
	scope     Scope
	backoffMs int
	stopped   bool
	OnOpen    func()
	OnMessage func(text string)
	OnClose   func(code int, reason string)
}

func wsURL() string {
	loc := global.Get("location")
	proto := "ws://"
	if loc.Get("protocol").String() == "https:" {
		proto = "wss://"
	}
	return proto + loc.Get("host").String() + "/ws?client=web-go"
}

func (s *Socket) Connect() {
	s.stopped = false
	s.scope.Release()
	ws := global.Get("WebSocket").New(wsURL())
	s.ws = ws
	s.scope.On(ws, "open", func(js.Value) {
		s.backoffMs = 0
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

func (s *Socket) Stop() {
	s.stopped = true
	if s.ws.Truthy() {
		w := s.ws
		s.ws = js.Null()
		w.Call("close")
	}
	s.scope.Release()
}
