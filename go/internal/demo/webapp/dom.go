//go:build js && wasm

package webapp

// A tiny DOM kit over syscall/js. There is no virtual DOM: regions are
// rebuilt when their inputs change, and the grid updates pooled elements in
// place, caching what it last wrote so unchanged cells cost no JS calls.
// Text always goes through textContent; nothing ever sets innerHTML.

import (
	"fmt"
	"runtime/debug"
	"strconv"
	"syscall/js"
)

var (
	global = js.Global()
	doc    = global.Get("document")
)

const svgNS = "http://www.w3.org/2000/svg"

// E creates an element with an optional class.
func E(tag, class string) js.Value {
	e := doc.Call("createElement", tag)
	if class != "" {
		e.Set("className", class)
	}
	return e
}

// ET creates an element with a class and text.
func ET(tag, class, text string) js.Value {
	e := E(tag, class)
	if text != "" {
		e.Set("textContent", text)
	}
	return e
}

// S creates an SVG element.
func S(tag string, attrs ...string) js.Value {
	e := doc.Call("createElementNS", svgNS, tag)
	for i := 0; i+1 < len(attrs); i += 2 {
		e.Call("setAttribute", attrs[i], attrs[i+1])
	}
	return e
}

func Add(parent js.Value, kids ...js.Value) js.Value {
	for _, k := range kids {
		if k.Truthy() {
			parent.Call("appendChild", k)
		}
	}
	return parent
}

func Txt(s string) js.Value { return doc.Call("createTextNode", s) }

func Attr(e js.Value, k, v string) js.Value {
	e.Call("setAttribute", k, v)
	return e
}

func Data(e js.Value, k, v string) js.Value {
	e.Get("dataset").Set(k, v)
	return e
}

func DataOf(e js.Value, k string) string {
	if !e.Truthy() {
		return ""
	}
	d := e.Get("dataset")
	if !d.Truthy() {
		return ""
	}
	v := d.Get(k)
	if v.IsUndefined() {
		return ""
	}
	return v.String()
}

func Style(e js.Value, prop, v string) js.Value {
	e.Get("style").Call("setProperty", prop, v)
	return e
}

func Px(n float64) string { return strconv.FormatFloat(n, 'f', 1, 64) + "px" }

func Clear(e js.Value) {
	e.Set("textContent", "")
}

func Replace(parent js.Value, kids ...js.Value) {
	Clear(parent)
	Add(parent, kids...)
}

// Closest walks up from a node to the nearest ancestor (or self) with a selector.
func Closest(e js.Value, sel string) js.Value {
	if !e.Truthy() || e.Get("closest").IsUndefined() {
		return js.Null()
	}
	return e.Call("closest", sel)
}

// Scope owns event listeners so a rebuilt region can release them.
type Scope struct {
	fns []js.Func
}

func (s *Scope) Func(fn func(js.Value)) js.Func {
	f := js.FuncOf(func(this js.Value, args []js.Value) any {
		var ev js.Value
		if len(args) > 0 {
			ev = args[0]
		}
		safe(func() { fn(ev) })
		return nil
	})
	s.fns = append(s.fns, f)
	return f
}

func (s *Scope) On(e js.Value, event string, fn func(js.Value)) {
	e.Call("addEventListener", event, s.Func(fn))
}

// Release frees the listeners on the next tick: removing a focused element
// fires blur synchronously, and that handler must still be callable.
func (s *Scope) Release() {
	fns := s.fns
	s.fns = nil
	if len(fns) == 0 {
		return
	}
	After(0, func() {
		for _, f := range fns {
			f.Release()
		}
	})
}

// Global (never released) listener helper.
var permanent Scope

func On(e js.Value, event string, fn func(js.Value)) { permanent.On(e, event, fn) }

// Timer helpers: setTimeout with a self-releasing callback.
func After(ms int, fn func()) {
	var f js.Func
	f = js.FuncOf(func(js.Value, []js.Value) any {
		f.Release()
		safe(fn)
		return nil
	})
	global.Call("setTimeout", f, ms)
}

// safe runs a callback and turns a panic into a logged error and a toast:
// a panic escaping a js.Func would end the Go program (and the app).
func safe(fn func()) {
	defer func() {
		if r := recover(); r != nil {
			stack := string(debug.Stack())
			global.Get("console").Call("error", fmt.Sprintf("demo: internal error: %v\n%s", r, stack))
			if app != nil && app.v != nil {
				app.toast("err", "Internal error", fmt.Sprint(r))
			}
		}
	}()
	fn()
}

// cached element writers: skip the JS call when nothing changed.
type cachedEl struct {
	el    js.Value
	text  string
	class string
	title string
	style map[string]string
}

func (c *cachedEl) SetText(s string) {
	if c.text != s {
		c.text = s
		c.el.Set("textContent", s)
	}
}

func (c *cachedEl) SetClass(s string) {
	if c.class != s {
		c.class = s
		c.el.Set("className", s)
	}
}

func (c *cachedEl) SetTitle(s string) {
	if c.title != s {
		c.title = s
		c.el.Set("title", s)
	}
}

func (c *cachedEl) SetStyle(prop, v string) {
	if c.style == nil {
		c.style = map[string]string{}
	}
	if old, ok := c.style[prop]; ok && old == v {
		return
	}
	c.style[prop] = v
	if v == "" {
		c.el.Get("style").Call("removeProperty", prop)
	} else {
		c.el.Get("style").Call("setProperty", prop, v)
	}
}

// storage wraps localStorage; it can throw (private mode), so every access is guarded.
func storageGet(k string) (v string) {
	defer func() {
		if recover() != nil {
			v = ""
		}
	}()
	ls := global.Get("localStorage")
	if !ls.Truthy() {
		return ""
	}
	r := ls.Call("getItem", k)
	if r.IsNull() || r.IsUndefined() {
		return ""
	}
	return r.String()
}

func storageSet(k, v string) {
	defer func() { _ = recover() }()
	ls := global.Get("localStorage")
	if ls.Truthy() {
		ls.Call("setItem", k, v)
	}
}

func nowMs() float64 { return global.Get("Date").Call("now").Float() }
