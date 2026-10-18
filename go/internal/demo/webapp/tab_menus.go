//go:build js && wasm

package webapp

// Tab 11 — Menus: (a) a demo menu bar with submenus, keys, separators, a
// disabled and checkable items (picking only reports what was picked);
// (b) a file list (menus.files) with a context menu that edits it.

import (
	"fmt"
	"path"
	"strconv"
	"strings"
	"syscall/js"

	"git.wharf.fluidgrid.org/jcz/tictactoe/go/internal/demo/core"
)

type mitem struct {
	label, key string
	sub        []mitem
	disabled   bool
	check      bool // checkable
	sep        bool
}

var sepItem = mitem{sep: true}

var demoMenus = []struct {
	title string
	items []mitem
}{
	{"Document", []mitem{{label: "New", key: "^N"}, {label: "Open…", key: "^O"}, {label: "Save", key: "^S"}, sepItem,
		{label: "Export", sub: []mitem{{label: "PDF"}, {label: "PNG"}, {label: "Markdown"}}}, sepItem,
		{label: "Print…", key: "^P", disabled: true}}},
	{"Format", []mitem{
		{label: "Text", sub: []mitem{{label: "Bold", key: "^B"}, {label: "Italic", key: "^I"}, {label: "Underline", key: "^U"}, {label: "Strikethrough"}}},
		{label: "Align", sub: []mitem{{label: "Left"}, {label: "Center"}, {label: "Right"}, {label: "Justify"}}}, sepItem,
		{label: "Show ruler", check: true}, {label: "Clear formatting", disabled: true}}},
	{"Arrange", []mitem{{label: "Bring to front", key: "^]"}, {label: "Send to back", key: "^["}, sepItem,
		{label: "Group"}, {label: "Ungroup", disabled: true}, sepItem, {label: "Snap to grid", check: true}}},
}

type menusTab struct {
	tabBase
	bar     js.Value
	mcur    int   // highlighted menu title
	open    bool  // a menu is open
	path    []int // selected item per open level
	checked map[string]bool
	barSig  string

	files   js.Value
	fcur    int
	ctxOpen bool
	ctxSel  int
	ctxX    float64
	ctxY    float64
	fileSig string
	ctxSig  string
	ps      Scope
}

func newMenusTab(b tabBase) Tab {
	t := &menusTab{tabBase: b, checked: map[string]bool{"Format ▸ Show ruler": true}}
	t.build = t.mount
	return t
}

func (t *menusTab) mount(area js.Value) {
	Add(area, ET("div", "sublabel", "a) Menu bar"))
	t.bar = E("div", "dmenubar")
	Attr(t.bar, "role", "menubar")
	t.addCtl(t.bar, "demo-menubar", "")
	Add(area, t.bar, ET("div", "sublabel", "b) Files (menus.files) — Enter or right-click for the context menu"))
	t.files = E("div", "filelist")
	Attr(t.files, "role", "listbox")
	t.addCtl(t.files, "files", "menus.files")
	Add(area, t.files)

	t.scope.On(t.bar, "mousedown", func(ev js.Value) {
		ev.Call("preventDefault")
		t.Focus(0)
		tgt := ev.Get("target")
		if it := Closest(tgt, "[data-path]"); it.Truthy() {
			t.path = parsePath(DataOf(it, "path"))
			t.activate()
		} else if m := Closest(tgt, "[data-m]"); m.Truthy() {
			i, _ := strconv.Atoi(DataOf(m, "m"))
			if t.open && t.mcur == i {
				t.open = false
			} else {
				t.mcur, t.open, t.path = i, true, []int{t.firstItem(demoMenus[i].items)}
			}
		}
		t.a.Invalidate()
	})
	t.scope.On(t.bar, "mouseover", func(ev js.Value) {
		if !t.open {
			return
		}
		tgt := ev.Get("target")
		if it := Closest(tgt, "[data-path]"); it.Truthy() {
			p := parsePath(DataOf(it, "path"))
			if fmt.Sprint(p) != fmt.Sprint(t.path) {
				t.path = p
				t.a.Invalidate()
			}
		} else if m := Closest(tgt, "[data-m]"); m.Truthy() {
			if i, _ := strconv.Atoi(DataOf(m, "m")); i != t.mcur {
				t.mcur, t.path = i, []int{t.firstItem(demoMenus[i].items)}
				t.a.Invalidate()
			}
		}
	})
	t.scope.On(t.files, "mousedown", func(ev js.Value) {
		if it := Closest(ev.Get("target"), "[data-f]"); it.Truthy() {
			t.fcur, _ = strconv.Atoi(DataOf(it, "f"))
			if ev.Get("button").Int() == 2 {
				ev.Call("preventDefault")
				t.Focus(1)
				t.openCtx(ev.Get("clientX").Float(), ev.Get("clientY").Float())
			}
			t.a.Invalidate()
		}
	})
	t.scope.On(t.files, "contextmenu", func(ev js.Value) { ev.Call("preventDefault") })
	t.scope.On(t.files, "dblclick", func(ev js.Value) {
		if Closest(ev.Get("target"), "[data-f]").Truthy() {
			t.openCtx(-1, -1)
		}
	})
}

func parsePath(s string) []int {
	var out []int
	for _, p := range strings.Split(s, ".") {
		n, _ := strconv.Atoi(p)
		out = append(out, n)
	}
	return out
}

func (t *menusTab) firstItem(items []mitem) int {
	for i, it := range items {
		if !it.sep {
			return i
		}
	}
	return 0
}

// levelItems returns the items at level l of the open path.
func (t *menusTab) levelItems(l int) []mitem {
	items := demoMenus[t.mcur].items
	for i := 0; i < l; i++ {
		items = items[t.path[i]].sub
	}
	return items
}

func (t *menusTab) pathLabel() string {
	parts := []string{demoMenus[t.mcur].title}
	items := demoMenus[t.mcur].items
	for _, i := range t.path {
		parts = append(parts, items[i].label)
		items = items[i].sub
	}
	return strings.Join(parts, " ▸ ")
}

// activate runs the selected item: open its submenu, toggle, or report it.
func (t *menusTab) activate() {
	l := len(t.path) - 1
	it := t.levelItems(l)[t.path[l]]
	label := t.pathLabel()
	switch {
	case it.sep:
	case it.sub != nil:
		t.path = append(t.path, t.firstItem(it.sub))
	case it.disabled:
		t.a.info(label + " is disabled")
	case it.check:
		t.checked[label] = !t.checked[label]
		state := "off"
		if t.checked[label] {
			state = "on"
		}
		t.a.info("Picked " + label + " (" + state + ")")
		t.open = false
	default:
		t.a.info("Picked " + label)
		t.open = false
	}
}

func (t *menusTab) barKey(k Key) bool {
	nm := len(demoMenus)
	if !t.open {
		switch {
		case k.Is("ArrowLeft"):
			t.mcur = (t.mcur + nm - 1) % nm
		case k.Is("ArrowRight"):
			t.mcur = (t.mcur + 1) % nm
		case k.Is("Enter", "ArrowDown", " "):
			t.open, t.path = true, []int{t.firstItem(demoMenus[t.mcur].items)}
		default:
			return false
		}
		return true
	}
	l := len(t.path) - 1
	items := t.levelItems(l)
	move := func(d int) {
		for range items {
			t.path[l] = (t.path[l] + d + len(items)) % len(items)
			if !items[t.path[l]].sep {
				return
			}
		}
	}
	switch {
	case k.Is("ArrowDown"):
		move(1)
	case k.Is("ArrowUp"):
		move(-1)
	case k.Is("ArrowRight"):
		if items[t.path[l]].sub != nil {
			t.path = append(t.path, t.firstItem(items[t.path[l]].sub))
		} else {
			t.mcur = (t.mcur + 1) % nm
			t.path = []int{t.firstItem(demoMenus[t.mcur].items)}
		}
	case k.Is("ArrowLeft"):
		if l > 0 {
			t.path = t.path[:l]
		} else {
			t.mcur = (t.mcur + nm - 1) % nm
			t.path = []int{t.firstItem(demoMenus[t.mcur].items)}
		}
	case k.Is("Enter", " "):
		t.activate()
	case k.Is("Escape"):
		if l > 0 {
			t.path = t.path[:l]
		} else {
			t.open = false
		}
	case k.Is("Tab", "S-Tab"):
		t.open = false
		return false
	default:
		return true
	}
	return true
}

func (t *menusTab) renderBar() {
	sig := fmt.Sprint(t.mcur, t.open, t.path, t.checked, t.isCur(0))
	if sig == t.barSig {
		return
	}
	t.barSig = sig
	Clear(t.bar)
	for i, m := range demoMenus {
		cls := "menu"
		if i == t.mcur && (t.open || t.isCur(0)) {
			cls += " open"
		}
		el := E("span", cls)
		Data(el, "m", strconv.Itoa(i))
		Add(el, ET("u", "", m.title[:1]), Txt(m.title[1:]))
		if t.open && i == t.mcur {
			Add(el, t.drop(m.title, m.items, 0, ""))
		}
		Add(t.bar, el)
	}
}

// drop builds the dropdown at level l (and its open submenu, recursively).
func (t *menusTab) drop(prefix string, items []mitem, l int, pfx string) js.Value {
	d := E("div", "drop")
	if l > 0 {
		d.Set("className", "drop sub")
	}
	Data(d, "popup", "menu")
	for j, it := range items {
		if it.sep {
			Add(d, E("div", "sep"))
			continue
		}
		cls := "item"
		if l < len(t.path) && t.path[l] == j {
			cls += " sel"
		}
		if it.disabled {
			cls += " disabled"
		}
		el := E("div", cls)
		p := pfx + strconv.Itoa(j)
		Data(el, "path", p)
		chk := ""
		if it.check && t.checked[prefix+" ▸ "+it.label] {
			chk = "✓"
		}
		key := it.key
		if it.sub != nil {
			key = "▸"
		}
		Add(el, ET("span", "chk", chk), ET("span", "il", it.label), ET("span", "ik", key))
		if it.sub != nil && l+1 < len(t.path) && t.path[l] == j {
			Add(el, t.drop(prefix+" ▸ "+it.label, it.sub, l+1, p+"."))
		}
		Add(d, el)
	}
	return d
}

// ---- file list + context menu ----------------------------------------------------------------

var ctxItems = []string{"Open", "Rename…", "Duplicate", "Star", "Delete…"}

func (t *menusTab) openCtx(x, y float64) {
	if len(t.a.v.Files()) == 0 {
		return
	}
	t.ctxOpen, t.ctxSel, t.ctxX, t.ctxY = true, 0, x, y
	t.ctxSig = ""
	t.a.Invalidate()
}

func (t *menusTab) closeCtx() {
	t.ctxOpen = false
	t.a.ClearPop(t)
}

func (t *menusTab) ctxRun(i int) {
	files := append([]core.File(nil), t.a.v.Files()...)
	if t.fcur >= len(files) {
		return
	}
	f := files[t.fcur]
	idx := t.fcur
	t.closeCtx()
	switch i {
	case 0:
		t.a.info("Opened " + core.SafeText(f.Name))
	case 1:
		t.a.OpenDialog(newPrompt(t.a, "Rename", "Name", f.Name, "Rename", 40, func(v string) error {
			if v == f.Name {
				return nil
			}
			files := append([]core.File(nil), t.a.v.Files()...)
			for j := range files {
				if files[j].Name == f.Name {
					files[j].Name = v
				}
			}
			return t.a.Commit("menus.files", files)
		}))
	case 2:
		name := copyName(f.Name, files)
		out := append(append(append([]core.File{}, files[:idx+1]...), core.File{Name: name}), files[idx+1:]...)
		if t.a.Commit("menus.files", out) == nil {
			t.fcur = idx + 1
		}
	case 3:
		files[idx].Starred = !files[idx].Starred
		_ = t.a.Commit("menus.files", files)
	case 4:
		t.a.OpenDialog(newConfirm(t.a, "Delete file", "Delete "+f.Name+"?", "Delete", func() {
			var out []core.File
			for _, x := range t.a.v.Files() {
				if x.Name != f.Name {
					out = append(out, x)
				}
			}
			if out == nil {
				out = []core.File{}
			}
			_ = t.a.Commit("menus.files", out)
		}))
	}
}

// copyName: "Roadmap.pdf" → "Roadmap copy.pdf", then "Roadmap copy 2.pdf", …
func copyName(name string, files []core.File) string {
	ext := path.Ext(name)
	stem := strings.TrimSuffix(name, ext)
	taken := map[string]bool{}
	for _, f := range files {
		taken[f.Name] = true
	}
	for n := 1; ; n++ {
		c := stem + " copy" + ext
		if n > 1 {
			c = fmt.Sprintf("%s copy %d%s", stem, n, ext)
		}
		if !taken[c] {
			return c
		}
	}
}

func (t *menusTab) filesKey(k Key) bool {
	files := t.a.v.Files()
	n := len(files)
	if t.ctxOpen {
		switch {
		case k.Is("ArrowDown"):
			t.ctxSel = (t.ctxSel + 1) % len(ctxItems)
		case k.Is("ArrowUp"):
			t.ctxSel = (t.ctxSel + len(ctxItems) - 1) % len(ctxItems)
		case k.Is("Enter", " "):
			t.ctxRun(t.ctxSel)
		case k.Is("Escape"):
			t.closeCtx()
		case k.Is("Tab", "S-Tab"):
			t.closeCtx()
			return false
		}
		return true
	}
	switch {
	case k.Is("ArrowDown"):
		t.fcur = min(n-1, t.fcur+1)
	case k.Is("ArrowUp"):
		t.fcur = max(0, t.fcur-1)
	case k.Is("Home"):
		t.fcur = 0
	case k.Is("End"):
		t.fcur = n - 1
	case k.Is("Enter", "S-F10", "ContextMenu"):
		t.openCtx(-1, -1)
	default:
		return false
	}
	return true
}

func (t *menusTab) Key(k Key) bool {
	var ok bool
	if t.cur == 0 {
		ok = t.barKey(k)
	} else {
		ok = t.filesKey(k)
	}
	if ok {
		t.a.Invalidate()
	}
	return ok
}

func (t *menusTab) Render() {
	if t.cur != 0 && t.open {
		t.open = false
	}
	t.renderBar()
	files := t.a.v.Files()
	if t.fcur >= len(files) {
		t.fcur = max(0, len(files)-1)
	}
	sig := fmt.Sprint(files, t.fcur, t.isCur(1))
	if sig != t.fileSig {
		t.fileSig = sig
		Clear(t.files)
		for i, f := range files {
			cls := "frowf"
			if i == t.fcur {
				cls += " sel"
				if !t.isCur(1) {
					cls += " dim"
				}
			}
			r := E("div", cls)
			Data(r, "f", strconv.Itoa(i))
			star := " "
			if f.Starred {
				star = "★"
			}
			Add(r, ET("span", "star", star), ET("span", "fname", core.SafeText(f.Name)), ET("span", "muted fext", strings.TrimPrefix(path.Ext(f.Name), ".")))
			Add(t.files, r)
		}
		if len(files) == 0 {
			Add(t.files, ET("div", "muted empty", "no files"))
		}
	}
	if !t.ctxOpen {
		t.a.ClearPop(t)
		return
	}
	csig := fmt.Sprint(t.ctxSel, t.fcur, files)
	if csig == t.ctxSig && t.a.PopOwner(t) {
		return
	}
	t.ctxSig = csig
	el := E("div", "drop ctxmenu")
	Add(el, ET("div", "ctxhead", core.SafeText(files[t.fcur].Name)))
	for i, label := range ctxItems {
		if i == 3 && files[t.fcur].Starred {
			label = "Unstar"
		}
		if i == 4 {
			Add(el, E("div", "sep"))
		}
		cls := "item"
		if i == t.ctxSel {
			cls += " sel"
		}
		it := E("div", cls)
		Data(it, "i", strconv.Itoa(i))
		Add(it, ET("span", "chk", ""), ET("span", "il", label))
		Add(el, it)
	}
	t.ps.Release()
	t.ps.On(el, "mousedown", func(ev js.Value) {
		ev.Call("preventDefault")
		if it := Closest(ev.Get("target"), "[data-i]"); it.Truthy() {
			i, _ := strconv.Atoi(DataOf(it, "i"))
			t.ctxRun(i)
			t.a.Invalidate()
		}
	})
	t.ps.On(el, "mouseover", func(ev js.Value) {
		if it := Closest(ev.Get("target"), "[data-i]"); it.Truthy() {
			if i, _ := strconv.Atoi(DataOf(it, "i")); i != t.ctxSel {
				t.ctxSel = i
				t.a.Invalidate()
			}
		}
	})
	if t.ctxX >= 0 {
		t.a.SetPopAt(t, "context-menu", t.ctxX, t.ctxY, el, func() { t.ctxOpen = false })
	} else {
		anchor := t.files.Call("querySelector", ".frowf.sel")
		t.a.SetPop(t, "context-menu", anchor, el, func() { t.ctxOpen = false })
	}
}

func (t *menusTab) Hints() []hint {
	if t.cur == 0 {
		if t.open {
			return []hint{{"↑↓", "Items", ""}, {"←→", "Menus / submenu", ""}, {"Enter", "Pick", "Enter"}, {"Esc", "Close", "Escape"}}
		}
		return []hint{{"Enter", "Open menu", "Enter"}, {"←→", "Menus", ""}, {"Tab", "Files", "Tab"}}
	}
	if t.ctxOpen {
		return []hint{{"↑↓", "Items", ""}, {"Enter", "Run", "Enter"}, {"Esc", "Close", "Escape"}}
	}
	return []hint{{"↑↓", "Move", ""}, {"Enter", "Context menu", "Enter"}, {"⇧F10", "Context menu", "S-F10"}}
}
