//go:build js && wasm

package webapp

// Modal dialogs, adapted from pt's web client: confirm, message, a one-line
// prompt, the profile form (tab 13), About and the searchable help (F1).
// Each box carries data-popup="dialog" (help: "help", palette: "palette").

import (
	"fmt"
	"strings"
	"syscall/js"

	"git.wharf.fluidgrid.org/jcz/tictactoe/go/internal/demo/core"
)

type Dialog interface {
	Build() js.Value
	Key(k Key) bool
	Hints() []hint
	Destroy()
	AfterOpen()
}

var dialogBuilt Dialog
var dialogScope Scope

func (a *App) OpenDialog(d Dialog) {
	a.closeMenus()
	a.closePop()
	if a.dialog != nil {
		a.CloseDialog()
	}
	a.dialog = d
	// build now: a key pressed before the next frame must find the dialog ready
	a.renderDialog()
	a.Invalidate()
}

func (a *App) CloseDialog() {
	if a.dialog != nil {
		a.dialog.Destroy()
	}
	a.dialog = nil
	a.Invalidate()
	After(10, func() {
		if a.dialog == nil {
			a.focusCurrent()
		}
	})
}

func (a *App) renderDialog() {
	if a.dialog == dialogBuilt {
		return
	}
	dialogScope.Release()
	Clear(a.dialogLayer)
	dialogBuilt = a.dialog
	if a.dialog == nil {
		a.dialogLayer.Set("className", "dialog-layer")
		return
	}
	a.dialogLayer.Set("className", "dialog-layer open")
	back := E("div", "backdrop")
	dialogScope.On(back, "mousedown", func(ev js.Value) {
		if ev.Get("target").Equal(back) {
			a.CloseDialog()
		}
	})
	Add(back, a.dialog.Build())
	Add(a.dialogLayer, back)
	a.dialog.AfterOpen()
}

// dialogBox makes the box (data-popup=kind) and returns it and its body.
func dialogBox(title, cls, kind string) (js.Value, js.Value) {
	box := E("div", "dialog "+cls)
	Attr(box, "role", "dialog")
	Data(box, "popup", kind)
	h := E("div", "dhead")
	Add(h, ET("span", "", title), ET("span", "dclose muted", "Esc"))
	body := E("div", "dbody")
	Add(box, h, body)
	return box, body
}

// ---- confirm ----------------------------------------------------------------------------

type confirmDialog struct {
	app     *App
	title   string
	msg     string
	okLabel string
	ok      func()
	cancel  func()
	scope   Scope
	sel     int // 0 cancel (default), 1 ok
	btns    [2]js.Value
}

func newConfirm(a *App, title, msg, okLabel string, ok func()) *confirmDialog {
	return &confirmDialog{app: a, title: title, msg: msg, okLabel: okLabel, ok: ok}
}

func (d *confirmDialog) Build() js.Value {
	box, body := dialogBox(d.title, "confirm", "dialog")
	for i, line := range strings.Split(d.msg, "\n") {
		cls := "muted"
		if i == 0 {
			cls = "strong"
		}
		Add(body, ET("div", cls, line))
	}
	foot := E("div", "dfoot")
	cancel := ET("button", "btn", "Cancel (n)")
	ok := ET("button", "btn danger", d.okLabel+" (y)")
	Data(cancel, "control", "cancel")
	Data(ok, "control", "ok")
	d.btns = [2]js.Value{cancel, ok}
	Add(foot, E("span", "spacer"), cancel, ok)
	Add(box, foot)
	d.scope.On(cancel, "click", func(js.Value) { d.no() })
	d.scope.On(ok, "click", func(js.Value) { d.yes() })
	return box
}

func (d *confirmDialog) yes() {
	d.app.CloseDialog()
	d.ok()
}

func (d *confirmDialog) no() {
	d.app.CloseDialog()
	if d.cancel != nil {
		d.cancel()
	}
}

func (d *confirmDialog) AfterOpen() { d.btns[0].Call("focus") }

func (d *confirmDialog) Key(k Key) bool {
	switch {
	case k.Is("y"):
		d.yes()
	case k.Is("n", "Escape"):
		d.no()
	case k.Is("ArrowLeft", "ArrowRight", "Tab", "S-Tab"):
		d.sel = 1 - d.sel
		d.btns[d.sel].Call("focus")
	case k.Is("Enter", " "):
		if d.sel == 1 {
			d.yes()
		} else {
			d.no()
		}
	default:
		return false
	}
	return true
}

func (d *confirmDialog) Hints() []hint {
	return []hint{{"y", d.okLabel, "y"}, {"n", "Cancel", "n"}, {"←→", "Choose", ""}, {"Enter", "Confirm choice", "Enter"}}
}
func (d *confirmDialog) Destroy() { d.scope.Release() }

// ---- message (single OK) / about -------------------------------------------------------------

type infoDialog struct {
	app   *App
	title string
	build func(body js.Value)
	ok    js.Value
	scope Scope
}

func newMessage(a *App, title, text string) Dialog {
	return &infoDialog{app: a, title: title, build: func(body js.Value) {
		for _, line := range strings.Split(text, "\n") {
			Add(body, ET("p", "", line))
		}
	}}
}

func (d *infoDialog) Build() js.Value {
	box, body := dialogBox(d.title, "info", "dialog")
	d.build(body)
	foot := E("div", "dfoot")
	d.ok = ET("button", "btn primary", "OK")
	Data(d.ok, "control", "ok")
	Add(foot, E("span", "spacer"), d.ok)
	Add(box, foot)
	d.scope.On(d.ok, "click", func(js.Value) { d.app.CloseDialog() })
	return box
}
func (d *infoDialog) AfterOpen() { d.ok.Call("focus") }
func (d *infoDialog) Key(k Key) bool {
	if k.Is("Enter", " ") {
		d.app.CloseDialog()
		return true
	}
	return false
}
func (d *infoDialog) Hints() []hint {
	return []hint{{"Enter", "OK", "Enter"}, {"Esc", "Close", "Escape"}}
}
func (d *infoDialog) Destroy() { d.scope.Release() }

func newAboutDialog(a *App) Dialog {
	return &infoDialog{app: a, title: "About demo", build: func(body js.Value) {
		kv := E("dl", "kv")
		for _, p := range [][2]string{
			{"Server", a.v.Server.Impl + " " + a.v.Server.Version},
			{"Client", "web-go " + core.Version + " — Go " + goVersion() + " compiled to WebAssembly"},
			{"Protocol", fmt.Sprint(core.ProtocolVersion)},
			{"You", core.SafeText(a.v.You.Name) + fmt.Sprintf(" (connection %d)", a.v.You.Conn)},
		} {
			Add(kv, ET("dt", "", p[0]), ET("dd", "", p[1]))
		}
		Add(body, kv, ET("p", "muted", howItWorks))
	}}
}

// ---- one-line prompt (rename, new card, broadcast) -------------------------------------------

type promptDialog struct {
	app    *App
	title  string
	label  string
	init   string
	okLbl  string
	max    int
	ok     func(v string) error // a non-nil error keeps the dialog open
	in     js.Value
	msg    js.Value
	scope  Scope
	closed bool
}

func newPrompt(a *App, title, label, initial, okLabel string, max int, ok func(string) error) *promptDialog {
	return &promptDialog{app: a, title: title, label: label, init: initial, okLbl: okLabel, max: max, ok: ok}
}

func (d *promptDialog) Build() js.Value {
	box, body := dialogBox(d.title, "confirm", "dialog")
	row := E("label", "frow")
	d.in = E("input", "ed")
	Data(d.in, "control", "prompt")
	Attr(d.in, "spellcheck", "false")
	if d.max > 0 {
		Attr(d.in, "maxlength", fmt.Sprint(d.max))
	}
	d.in.Set("value", d.init)
	d.msg = E("span", "fmsg")
	in := E("div", "fin")
	Add(in, d.in, d.msg)
	Add(row, ET("span", "flabel", d.label), in)
	Add(body, row)
	foot := E("div", "dfoot")
	cancel := ET("button", "btn", "Cancel")
	okb := ET("button", "btn primary", d.okLbl)
	Add(foot, E("span", "spacer"), cancel, okb)
	Add(box, foot)
	d.scope.On(cancel, "click", func(js.Value) { d.app.CloseDialog() })
	d.scope.On(okb, "click", func(js.Value) { d.submit() })
	d.scope.On(d.in, "input", func(js.Value) { d.msg.Set("textContent", ""); d.in.Set("className", "ed") })
	return box
}

func (d *promptDialog) submit() {
	v := strings.TrimSpace(d.in.Get("value").String())
	if err := d.ok(v); err != nil {
		d.msg.Set("textContent", err.Error())
		d.in.Set("className", "ed bad")
		d.in.Call("focus")
		return
	}
	d.app.CloseDialog()
}

func (d *promptDialog) AfterOpen() { d.in.Call("focus"); d.in.Call("select") }
func (d *promptDialog) Key(k Key) bool {
	if k.Is("Enter") && k.InInput {
		d.submit()
		return true
	}
	return false
}
func (d *promptDialog) Hints() []hint {
	return []hint{{"Enter", d.okLbl, "Enter"}, {"Esc", "Cancel", "Escape"}}
}
func (d *promptDialog) Destroy() { d.scope.Release() }

// ---- profile form (tab 13) ---------------------------------------------------------------------

type profileDialog struct {
	app          *App
	base         int64
	name, email  js.Value
	nmsg, emsg   js.Value
	save, cancel js.Value
	scope        Scope
}

func newProfileDialog(a *App) *profileDialog {
	return &profileDialog{app: a, base: a.v.Version("dialog.profile")}
}

func (d *profileDialog) Build() js.Value {
	p := d.app.v.Profile()
	box, body := dialogBox("Edit profile", "form", "dialog")
	field := func(label, ctl, val string) (js.Value, js.Value) {
		row := E("label", "frow")
		in := E("input", "ed")
		Data(in, "control", ctl)
		Attr(in, "spellcheck", "false")
		in.Set("value", core.SafeText(val))
		msg := E("span", "fmsg")
		fin := E("div", "fin")
		Add(fin, in, msg)
		Add(row, ET("span", "flabel", label), fin)
		Add(body, row)
		d.scope.On(in, "input", func(js.Value) { d.validate() })
		return in, msg
	}
	d.name, d.nmsg = field("Name", "profile-name", p.Name)
	d.email, d.emsg = field("Email", "profile-email", p.Email)
	Add(body, ET("div", "muted", "Saved to dialog.profile and shared with everyone."))
	foot := E("div", "dfoot")
	d.cancel = ET("button", "btn", "Cancel")
	d.save = ET("button", "btn primary", "Save")
	Data(d.save, "control", "save")
	Add(foot, ET("span", "muted", "^S saves · Esc cancels"), E("span", "spacer"), d.cancel, d.save)
	Add(box, foot)
	d.scope.On(d.cancel, "click", func(js.Value) { d.app.CloseDialog() })
	d.scope.On(d.save, "click", func(js.Value) { d.submit() })
	return box
}

// validate marks each invalid field with its message; ok when both pass.
func (d *profileDialog) validate() bool {
	name := strings.TrimSpace(d.name.Get("value").String())
	email := strings.TrimSpace(d.email.Get("value").String())
	nerr, eerr := "", ""
	if _, err := core.NormalizeValue("text.name", name); err != nil {
		nerr = err.Error()
	}
	if email == "" {
		eerr = "email is required"
	} else if _, err := core.NormalizeValue("text.email", email); err != nil {
		eerr = err.Error()
	}
	mark := func(in, msg js.Value, err string) {
		msg.Set("textContent", err)
		if err != "" {
			in.Set("className", "ed bad")
		} else {
			in.Set("className", "ed")
		}
	}
	mark(d.name, d.nmsg, nerr)
	mark(d.email, d.emsg, eerr)
	return nerr == "" && eerr == ""
}

func (d *profileDialog) submit() {
	if !d.validate() {
		return
	}
	p := core.Profile{Name: d.name.Get("value").String(), Email: d.email.Get("value").String()}
	if err := d.app.CommitFrom("dialog.profile", p, d.base, nil); err != nil {
		d.emsg.Set("textContent", err.Error())
		return
	}
	d.app.CloseDialog()
}

func (d *profileDialog) AfterOpen() { d.name.Call("focus") }
func (d *profileDialog) Key(k Key) bool {
	switch {
	case k.Is("C-s"):
		d.submit()
	case k.Is("Enter") && k.InInput:
		// Enter in a field moves on; on the last field it saves
		if d.name.Equal(doc.Get("activeElement")) {
			d.email.Call("focus")
		} else {
			d.submit()
		}
	default:
		return false
	}
	return true
}
func (d *profileDialog) Hints() []hint {
	return []hint{{"Tab", "Next field", ""}, {"^S", "Save", "C-s"}, {"Esc", "Cancel", "Escape"}}
}
func (d *profileDialog) Destroy() { d.scope.Release() }

// ---- help (F1 / ^G) ------------------------------------------------------------------------------

type helpDialog struct {
	app   *App
	in    js.Value
	list  js.Value
	scope Scope
}

func newHelp(a *App) Dialog { return &helpDialog{app: a} }

// helpRow is one line: a section heading (key == "" && text == ""), a
// subheading (key == "", sub), a paragraph (key == "") or `key  what`.
type helpRow struct {
	section, sub, key, text string
}

func (d *helpDialog) rows() []helpRow {
	var out []helpRow
	cur := d.app.cur().Info()
	tt := tabTexts[cur.ID]
	sec := "This tab — " + cur.Title
	out = append(out, helpRow{section: sec, text: tt.Explain})
	for _, kv := range tt.Keys {
		out = append(out, helpRow{section: sec, key: kv[0], text: kv[1]})
	}
	for _, kv := range gettingAround {
		out = append(out, helpRow{section: "Getting around", key: kv[0], text: kv[1]})
	}
	for _, kv := range globalKeys {
		out = append(out, helpRow{section: "Global keys", key: kv[0], text: kv[1]})
	}
	for _, t := range core.Tabs {
		sub := fmt.Sprintf("%d %s", t.Num, t.Title)
		for _, kv := range tabTexts[t.ID].Keys {
			out = append(out, helpRow{section: "All tabs", sub: sub, key: kv[0], text: kv[1]})
		}
	}
	out = append(out, helpRow{section: "How demo works", text: howItWorks})
	return out
}

func (d *helpDialog) Build() js.Value {
	box, body := dialogBox("Keyboard help", "help", "help")
	d.in = E("input", "ed search")
	Data(d.in, "control", "help-search")
	Attr(d.in, "placeholder", "search keys and actions…")
	Attr(d.in, "spellcheck", "false")
	d.list = E("div", "helplist")
	Add(body, d.in, d.list)
	d.scope.On(d.in, "input", func(js.Value) { d.fill() })
	d.fill()
	return box
}

func (d *helpDialog) fill() {
	q := strings.ToLower(strings.TrimSpace(d.in.Get("value").String()))
	Clear(d.list)
	section, sub := "", ""
	n := 0
	for _, r := range d.rows() {
		if q != "" && !strings.Contains(strings.ToLower(r.section+" "+r.sub+" "+r.key+" "+r.text), q) {
			continue
		}
		n++
		if r.section != section {
			section, sub = r.section, ""
			Add(d.list, ET("h4", "", section))
		}
		if r.sub != "" && r.sub != sub {
			sub = r.sub
			Add(d.list, ET("h5", "", sub))
		}
		if r.key == "" {
			Add(d.list, ET("p", "hpara", r.text))
			continue
		}
		line := E("div", "hl-row")
		Add(line, ET("kbd", "", r.key), ET("span", "", r.text))
		Add(d.list, line)
	}
	if n == 0 {
		Add(d.list, ET("p", "muted", "Nothing matches."))
	}
}

func (d *helpDialog) AfterOpen() { d.in.Call("focus") }
func (d *helpDialog) Key(k Key) bool {
	step := 0.0
	page := d.list.Get("clientHeight").Float() - 40
	switch {
	case k.Is("ArrowDown"):
		step = 40
	case k.Is("ArrowUp"):
		step = -40
	case k.Is("PageDown"):
		step = page
	case k.Is("PageUp"):
		step = -page
	default:
		return false
	}
	d.list.Set("scrollTop", d.list.Get("scrollTop").Float()+step)
	return true
}
func (d *helpDialog) Hints() []hint {
	return []hint{{"type", "Search", ""}, {"↑↓ PgUp PgDn", "Scroll", ""}, {"Esc", "Close", "Escape"}}
}
func (d *helpDialog) Destroy() { d.scope.Release() }
