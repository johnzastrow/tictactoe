package tui

// Tabs 11 Menus (a demo menu bar with submenus, and a file list with a
// context menu), 12 Palette (the command palette inline) and 13 Dialogs.

import (
	"fmt"
	"strings"

	tea "charm.land/bubbletea/v2"

	"git.wharf.fluidgrid.org/jcz/tictactoe/go/internal/demo/core"
)

// ================= 11 Menus =================

type menusTab struct {
	ctlBase
	bar     int // highlighted demo menu
	sel     int // file list
	top     int
	titleX  []int
	barY    int
	listR   Rect
	ruler   bool
	snap    bool
	wrapTxt bool
}

func newMenusTab() *menusTab { return &menusTab{ruler: true, snap: false} }

var demoMenus = []string{"Document", "Format", "Arrange"}

func (t *menusTab) explain() string {
	return "A menu bar with submenus, keys, a disabled and a checkable item; and a file list whose context menu edits the shared list."
}

func (t *menusTab) controls() int { return 2 }

func (t *menusTab) keyHelp() [][2]string {
	return [][2]string{
		{"Enter ↓", "open menu"}, {"←→", "menus / submenu"}, {"Esc", "close"}, {"↑↓", "files"},
		{"Enter ⇧F10 Menu", "context menu"}, {"right-click", "context menu"}, {"Tab", "bar ⇄ files"},
	}
}

func (t *menusTab) hints(a *App) []hint {
	if t.cur == 0 {
		return []hint{{"←→", "Menu", ""}, h("enter", "Open"), {"↓", "Open", "down"}, h("tab", "Files")}
	}
	return []hint{{"↑↓", "Move", ""}, {"Enter", "Context menu", "enter"}, {"⇧F10", "Menu", "shift+f10"}, h("tab", "Next")}
}

// demoItems builds a demo menu; picking an item only reports its path.
func (t *menusTab) demoItems(i int) []menuItem {
	pick := func(path string) func(a *App) {
		return func(a *App) { a.info("Picked " + path) }
	}
	leafs := func(path string, labels ...string) []menuItem {
		var out []menuItem
		for _, l := range labels {
			out = append(out, menuItem{label: l, run: pick(path + " ▸ " + l)})
		}
		return out
	}
	switch i {
	case 0:
		return []menuItem{
			{label: "New", key: "^N", accel: 'n', run: pick("Document ▸ New")},
			{label: "Open…", key: "^O", accel: 'o', run: pick("Document ▸ Open…")},
			{label: "Save", key: "^S", accel: 's', run: pick("Document ▸ Save")},
			{sep: true},
			{label: "Export", accel: 'e', sub: leafs("Document ▸ Export", "PDF", "HTML", "Markdown")},
			{label: "Print…", key: "^P", off: true},
			{sep: true},
			{label: "Close", key: "^W", accel: 'c', run: pick("Document ▸ Close")},
		}
	case 1:
		text := []menuItem{
			{label: "Bold", key: "^B", accel: 'b', run: pick("Format ▸ Text ▸ Bold")},
			{label: "Italic", key: "^I", accel: 'i', run: pick("Format ▸ Text ▸ Italic")},
			{label: "Underline", key: "^U", accel: 'u', run: pick("Format ▸ Text ▸ Underline")},
			{label: "Strikethrough", accel: 's', run: pick("Format ▸ Text ▸ Strikethrough")},
		}
		return []menuItem{
			{label: "Text", accel: 't', sub: text},
			{label: "Align", accel: 'a', sub: leafs("Format ▸ Align", "Left", "Center", "Right", "Justify")},
			{sep: true},
			{label: "Show ruler", accel: 'r', checked: &t.ruler, run: func(a *App) { a.info(fmt.Sprintf("Picked Format ▸ Show ruler (%s)", onOff(t.ruler))) }},
			{label: "Wrap text", accel: 'w', checked: &t.wrapTxt, run: func(a *App) { a.info(fmt.Sprintf("Picked Format ▸ Wrap text (%s)", onOff(t.wrapTxt))) }},
			{label: "Clear formatting", off: true},
		}
	}
	return []menuItem{
		{label: "Bring to front", accel: 'f', run: pick("Arrange ▸ Bring to front")},
		{label: "Send to back", accel: 'b', run: pick("Arrange ▸ Send to back")},
		{sep: true},
		{label: "Group", key: "^G", accel: 'g', run: pick("Arrange ▸ Group")},
		{label: "Ungroup", off: true},
		{sep: true},
		{label: "Snap to grid", accel: 's', checked: &t.snap, run: func(a *App) { a.info(fmt.Sprintf("Picked Arrange ▸ Snap to grid (%s)", onOff(t.snap))) }},
	}
}

func onOff(b bool) string { return choose(b, "on", "off") }

func (t *menusTab) openDemo(a *App, i int) {
	i = (i + len(demoMenus)) % len(demoMenus)
	t.bar = i
	t.cur = 0
	x := 0
	if i < len(t.titleX) {
		x = t.titleX[i]
	}
	l := newList("", t.demoItems(i), x, t.barY+1)
	l.path = demoMenus[i]
	l.onLR = func(a *App, d int) { t.openDemo(a, t.bar+d) }
	a.push(l)
}

func (t *menusTab) files(a *App) []core.File { return a.vals.Files() }

func (t *menusTab) saveFiles(a *App, files []core.File) {
	if files == nil {
		files = []core.File{}
	}
	if err := a.commit("menus.files", files); err != nil && err != errOffline {
		a.setStatus("✗ "+err.Error(), kErr)
	}
}

// contextMenu opens the file's context menu at x,y.
func (t *menusTab) contextMenu(a *App, x, y int) {
	files := t.files(a)
	if t.sel < 0 || t.sel >= len(files) {
		return
	}
	f := files[t.sel]
	name := f.Name
	idx := func(a *App) int {
		for i, x := range a.vals.Files() {
			if x.Name == name {
				return i
			}
		}
		return -1
	}
	items := []menuItem{
		{label: "Open", accel: 'o', key: "Enter", run: func(a *App) { a.info("Opened " + core.SafeText(name)) }},
		{label: "Rename…", accel: 'r', key: "F2", run: func(a *App) {
			a.prompt("Rename", "Name", name, func(a *App, s string) error {
				i := idx(a)
				if i < 0 {
					return fmt.Errorf("the file is gone")
				}
				fs := a.vals.Files()
				fs[i].Name = s
				if _, err := a.vals.PrepareSet("menus.files", fs); err != nil {
					return err
				}
				t.saveFiles(a, fs)
				return nil
			})
		}},
		{label: "Duplicate", accel: 'd', run: func(a *App) {
			i := idx(a)
			if i < 0 {
				return
			}
			fs := a.vals.Files()
			fs = append(fs[:i+1], append([]core.File{{Name: copyName(fs, name)}}, fs[i+1:]...)...)
			t.saveFiles(a, fs)
			t.sel = i + 1
		}},
		{label: choose(f.Starred, "Unstar", "Star"), accel: 's', run: func(a *App) {
			if i := idx(a); i >= 0 {
				fs := a.vals.Files()
				fs[i].Starred = !fs[i].Starred
				t.saveFiles(a, fs)
			}
		}},
		{sep: true},
		{label: "Delete…", accel: 'e', key: "Del", run: func(a *App) {
			a.confirm("Delete file", "Delete "+core.SafeText(name)+"?", "Delete", func(a *App) {
				if i := idx(a); i >= 0 {
					fs := a.vals.Files()
					t.saveFiles(a, append(fs[:i], fs[i+1:]...))
					t.sel = max(0, min(t.sel, len(fs)-2))
				}
			})
		}},
	}
	l := newList(Ellipsis(core.SafeText(name), 24), items, x, y)
	a.push(l)
}

// copyName is "Name copy.ext" (then "copy 2", …), unique in the list.
func copyName(files []core.File, name string) string {
	base, ext := name, ""
	if i := strings.LastIndex(name, "."); i > 0 {
		base, ext = name[:i], name[i:]
	}
	taken := map[string]bool{}
	for _, f := range files {
		taken[f.Name] = true
	}
	for n := 1; ; n++ {
		c := base + " copy" + ext
		if n > 1 {
			c = fmt.Sprintf("%s copy %d%s", base, n, ext)
		}
		if !taken[c] && len([]rune(c)) <= 40 {
			return c
		}
		if n > 99 {
			return Ellipsis(base, 30) + fmt.Sprintf(" %d%s", n, ext)
		}
	}
}

func (t *menusTab) key(a *App, k tea.KeyPressMsg) bool {
	key := k.Keystroke()
	if t.cur == 0 {
		switch key {
		case "left":
			t.bar = (t.bar + len(demoMenus) - 1) % len(demoMenus)
		case "right":
			t.bar = (t.bar + 1) % len(demoMenus)
		case "enter", "down", "space":
			t.openDemo(a, t.bar)
		default:
			return false
		}
		return true
	}
	n := len(t.files(a))
	switch key {
	case "up":
		if t.sel == 0 {
			t.cur = 0
			return true
		}
		t.sel = max(0, t.sel-1)
	case "down":
		t.sel = min(n-1, t.sel+1)
	case "home":
		t.sel = 0
	case "end":
		t.sel = n - 1
	case "enter", "shift+f10", "menu", "space":
		t.contextMenu(a, t.listR.X+6, t.listR.Y+t.sel-t.top+1)
	case "f2":
		t.contextMenu(a, t.listR.X+6, t.listR.Y+t.sel-t.top+1)
		if l, ok := a.top().(*listOverlay); ok {
			l.run(a, 1)
		}
	case "delete":
		t.contextMenu(a, t.listR.X+6, t.listR.Y+t.sel-t.top+1)
		if l, ok := a.top().(*listOverlay); ok {
			l.run(a, len(l.items)-1)
		}
	default:
		return false
	}
	return true
}

func (t *menusTab) draw(a *App, b *Buf, r Rect, focused bool) []Hit {
	th := a.th()
	var hits []Hit
	// (a) the demo menu bar
	bar := Rect{r.X, r.Y, min(r.W, 60), 1}
	t.barY = r.Y
	bs := Sty{FG: th.FG, BG: th.Panel2}
	b.Fill(bar, ' ', bs)
	t.titleX = t.titleX[:0]
	x := bar.X + 1
	for i, m := range demoMenus {
		st := bs
		if focused && t.cur == 0 && i == t.bar {
			st = th.sel().With(Bold)
		}
		lbl := " " + m + " "
		b.Text(x, r.Y, lbl, st, -1)
		b.Set(x+1, r.Y, []rune(m)[0], st.With(Underline).Fg(choose(st.BG == th.CellBG, th.CellFG, th.Accent)))
		t.titleX = append(t.titleX, x)
		hits = append(hits, Hit{R: Rect{x, r.Y, StrW(lbl), 1}, Kind: HitCtl, S: "menu", A: i})
		x += StrW(lbl) + 1
	}
	b.Text(bar.Right()+2, r.Y, "← a demo menu bar", th.base().Fg(th.Muted), r.Right()-bar.Right()-2)
	// (b) the file list
	files := t.files(a)
	t.sel = max(0, min(t.sel, len(files)-1))
	lr := Rect{r.X, r.Y + 2, min(r.W, 46), max(3, min(len(files)+2, r.H-2))}
	t.listR = lr
	fcs := focused && t.cur == 1
	border := Sty{FG: choose(fcs, th.Accent, th.Border), BG: th.BG}
	if _, ok := a.flash["menus.files"]; ok {
		border = border.Fg(th.Warn)
	}
	b.Box(lr, border, th.base(), fmt.Sprintf("Files (%d)", len(files)), border.With(Bold))
	rows := lr.H - 2
	if t.sel < t.top {
		t.top = t.sel
	}
	if t.sel >= t.top+rows {
		t.top = t.sel - rows + 1
	}
	for i := 0; i < rows && t.top+i < len(files); i++ {
		fi := t.top + i
		f := files[fi]
		row := Rect{lr.X + 1, lr.Y + 1 + i, lr.W - 2, 1}
		st := th.base()
		if fi == t.sel {
			st = choose(fcs, th.sel(), Sty{FG: th.FG, BG: th.SelBG})
		}
		b.Fill(row, ' ', st)
		b.Text(row.X+1, row.Y, fileIcon(f.Name), st.Fg(choose(fi == t.sel && fcs, th.CellFG, th.Accent2)), 2)
		b.Text(row.X+3, row.Y, core.SafeText(f.Name), st, row.W-6)
		if f.Starred {
			b.Text(row.Right()-2, row.Y, "★", st.Fg(choose(fi == t.sel && fcs, th.CellFG, th.Warn)), 1)
		}
		hits = append(hits, Hit{R: row, Kind: HitCtl, S: "file", A: fi})
	}
	if lr.Right()+30 <= r.Right() {
		tip := []string{"Enter, ⇧F10, the Menu key or a", "right-click opens a file's", "context menu: Open · Rename… ·", "Duplicate · Star · Delete…"}
		for i, l := range tip {
			if lr.Y+1+i < r.Bottom() {
				b.Text(lr.Right()+2, lr.Y+1+i, l, th.base().Fg(th.Muted), r.Right()-lr.Right()-2)
			}
		}
	}
	return hits
}

func fileIcon(name string) string {
	switch {
	case strings.HasSuffix(name, ".pdf"):
		return "▤"
	case strings.HasSuffix(name, ".png"), strings.HasSuffix(name, ".svg"):
		return "▣"
	case strings.HasSuffix(name, ".zip"):
		return "▥"
	case strings.HasSuffix(name, ".xlsx"):
		return "▦"
	}
	return "▢"
}

func (t *menusTab) mouse(a *App, m hitMsg) {
	h := m.hit
	if d, ok := isWheel(m); ok && h.S == "file" {
		t.cur = 1
		t.sel = max(0, min(len(t.files(a))-1, t.sel+d))
		return
	}
	switch h.S {
	case "menu":
		if isClick(m) {
			t.cur = 0
			t.openDemo(a, h.A)
		}
	case "file":
		if isClick(m) || isRightClick(m) {
			t.cur, t.sel = 1, h.A
			if isRightClick(m) || a.doubleClick(h) {
				t.contextMenu(a, m.m.X, m.m.Y+1)
			}
		}
	}
}

// ================= 12 Palette =================

type paletteTab struct {
	ctlBase
	in      lineInput
	sel     int
	top     int
	items   []menuItem
	matches [][]int
}

func newPaletteTab() *paletteTab {
	t := &paletteTab{}
	t.in.placeholder = "type a command…"
	return t
}

func (t *paletteTab) explain() string {
	return "The command palette (^K), shown inline and always open: type to filter (fuzzy), Enter runs the command."
}

func (t *paletteTab) controls() int { return 1 }

func (t *paletteTab) keyHelp() [][2]string {
	return [][2]string{{"type", "filter (fuzzy)"}, {"↑↓ PgUp PgDn", "choose"}, {"Enter", "run"}, {"^U", "clear"}, {"^K", "the same as a popup"}}
}

func (t *paletteTab) hints(a *App) []hint {
	return []hint{{"type", "Filter", ""}, {"↑↓", "Choose", ""}, h("enter", "Run"), h("ctrl+u", "Clear")}
}

func (t *paletteTab) refilter(a *App) {
	all := a.commands()
	labels := make([]string, len(all))
	for i, it := range all {
		labels[i] = it.label
	}
	t.items, t.matches = nil, nil
	for _, rk := range core.FuzzyRank(t.in.Text(), labels) {
		t.items = append(t.items, all[rk.Index])
		t.matches = append(t.matches, rk.Match.Positions)
	}
	t.sel = max(0, min(t.sel, len(t.items)-1))
}

func (t *paletteTab) run(a *App) {
	t.refilter(a)
	if t.sel < len(t.items) {
		it := t.items[t.sel]
		t.in.SetText("")
		t.sel = 0
		a.runCommand(it)
	}
}

func (t *paletteTab) key(a *App, k tea.KeyPressMsg) bool {
	t.refilter(a)
	switch k.Keystroke() {
	case "up":
		t.sel = max(0, t.sel-1)
	case "down":
		t.sel = min(len(t.items)-1, t.sel+1)
	case "pgup":
		t.sel = max(0, t.sel-8)
	case "pgdown":
		t.sel = min(len(t.items)-1, t.sel+8)
	case "enter":
		t.run(a)
	case "esc":
		if t.in.Text() != "" {
			t.in.SetText("")
			t.sel = 0
			return true
		}
		return false
	default:
		handled, changed := t.in.Key(k)
		if changed {
			t.sel = 0
		}
		return handled
	}
	return true
}

func (t *paletteTab) paste(_ *App, s string) { t.in.Insert(s); t.sel = 0 }

func (t *paletteTab) draw(a *App, b *Buf, r Rect, focused bool) []Hit {
	th := a.th()
	t.refilter(a)
	var hits []Hit
	last := core.SafeText(a.vals.String("palette.last"))
	e := a.vals.Get("palette.last")
	ls := th.base().Fg(th.Muted)
	if _, ok := a.flash["palette.last"]; ok {
		ls = Sty{FG: th.BG, BG: th.Warn}
	}
	x := r.X + b.Text(r.X, r.Y, "Last command: ", ls, -1)
	if last == "" {
		b.Text(x, r.Y, "— none yet —", ls, r.W-14)
	} else {
		x += b.Text(x, r.Y, last, ls.Fg(choose(ls.BG == th.Warn, th.BG, th.Accent2)).With(Bold), r.W-30)
		if e.By != "" {
			b.Text(x, r.Y, " · by "+core.SafeText(e.By), ls, r.Right()-x)
		}
	}
	box := Rect{r.X, r.Y + 1, min(r.W, 64), max(4, r.H-1)}
	fill := Sty{FG: th.FG, BG: th.Panel}
	b.Box(box, Sty{FG: choose(focused, th.Accent, th.Border), BG: th.Panel}, fill, "Commands", Sty{FG: th.Accent, BG: th.Panel, A: Bold})
	b.Text(box.X+2, box.Y+1, "›", fill.Fg(th.Accent).With(Bold), 1)
	t.in.Draw(b, Rect{box.X + 4, box.Y + 1, box.W - 6, 1}, th, focused, fieldStyle(th, focused))
	hits = append(hits, Hit{R: Rect{box.X, box.Y + 1, box.W, 1}, Kind: HitCtl, S: "search"})
	b.Text(box.X+1, box.Y+2, strings.Repeat("─", box.W-2), fill.Fg(th.Border), box.W-2)
	rows := box.H - 4
	if t.sel < t.top {
		t.top = t.sel
	}
	if t.sel >= t.top+rows {
		t.top = t.sel - rows + 1
	}
	t.top = max(0, t.top)
	if len(t.items) == 0 {
		b.Text(box.X+2, box.Y+3, "no matches", fill.Fg(th.Muted), box.W-4)
	}
	for i := 0; i < rows && t.top+i < len(t.items); i++ {
		idx := t.top + i
		row := Rect{box.X + 1, box.Y + 3 + i, box.W - 2, 1}
		hh := drawMenuRow(b, th, row, t.items[idx], idx == t.sel && focused, t.matches[idx], idx)
		if idx == t.sel && !focused {
			b.Restyle(row, func(s Sty) Sty { return s.Bg(th.SelBG) })
		}
		hh.Kind, hh.S = HitCtl, "cmd"
		hits = append(hits, hh)
	}
	if len(t.items) > rows && rows > 0 {
		pos := box.Y + 3 + t.top*rows/len(t.items)
		b.Set(box.Right()-1, pos, '┃', fill.Fg(th.Accent))
	}
	return hits
}

func (t *paletteTab) mouse(a *App, m hitMsg) {
	if d, ok := isWheel(m); ok {
		t.sel = max(0, min(len(t.items)-1, t.sel+d))
		return
	}
	if isClick(m) && m.hit.S == "cmd" {
		t.sel = m.hit.A
		t.run(a)
	}
}

// ================= 13 Dialogs =================

type dialogsTab struct{ ctlBase }

func newDialogsTab() *dialogsTab { return &dialogsTab{} }

var dialogButtons = []string{"Confirm…", "Form…", "Message", "Toast", "Broadcast…"}

func (t *dialogsTab) explain() string {
	return "Buttons that open each kind of popup: a confirmation, a form that edits the shared profile, a message, a toast and a broadcast."
}

func (t *dialogsTab) controls() int { return len(dialogButtons) }

func (t *dialogsTab) keyHelp() [][2]string {
	return [][2]string{
		{"←→ Tab", "button"}, {"Enter Space", "open"}, {"y / n", "confirm"}, {"Tab", "form field"},
		{"^S", "save form"}, {"Esc", "cancel / close"},
	}
}

func (t *dialogsTab) hints(a *App) []hint {
	return []hint{{"←→", "Button", ""}, h("enter", "Open"), h("tab", "Next")}
}

func (t *dialogsTab) press(a *App, i int) {
	t.cur = i
	switch i {
	case 0:
		c := a.confirm("Confirm", "Delete 3 files? This can't be undone (in a real app).", "Delete",
			func(a *App) { a.info("Confirm: you chose Delete") })
		c.onNo = func(a *App) { a.info("Confirm: you chose Cancel") }
	case 1:
		t.openForm(a)
	case 2:
		a.message("Message", "This is an information box with a single OK button.", "Enter, Space, Esc or a click closes it.")
	case 3:
		a.toast("ℹ A local toast: only you see it; it goes away after 4 s.", kInfo)
	case 4:
		p := a.prompt("Broadcast", "Message", "", func(a *App, s string) error {
			if err := a.broadcast(s); err != nil {
				if err == errOffline {
					return fmt.Errorf("offline")
				}
				return err
			}
			a.setStatus("✓ Broadcast sent to everyone", kOK)
			return nil
		})
		p.help = "Enter sends a toast to everyone connected (1–120 characters) · Esc cancels"
		p.in.max = 120
	}
}

func (t *dialogsTab) openForm(a *App) {
	p := a.vals.Profile()
	f := &formDlg{title: "Edit profile (dialog.profile)", labels: []string{"Name", "Email"},
		ins: []*lineInput{{max: 80}, {max: 120}}, errs: make([]string, 2)}
	f.ins[0].SetText(core.SafeText(p.Name))
	f.ins[1].SetText(core.SafeText(p.Email))
	f.onSave = func(a *App, f *formDlg) bool {
		v := core.Profile{Name: f.ins[0].Text(), Email: f.ins[1].Text()}
		f.errs = make([]string, 2)
		bad := false
		if _, err := core.NormalizeValue("dialog.profile", core.Profile{Name: v.Name, Email: "x@example.com"}); err != nil {
			f.errs[0], bad = strings.TrimPrefix(err.Error(), "name: "), true
		}
		if _, err := core.NormalizeValue("dialog.profile", core.Profile{Name: "x", Email: v.Email}); err != nil {
			f.errs[1], bad = strings.TrimPrefix(err.Error(), "email: "), true
		}
		if bad {
			f.focus = choose(f.errs[0] != "", 0, 1)
			return false
		}
		if err := a.commit("dialog.profile", v); err != nil {
			f.msg = "✗ " + err.Error()
			return false
		}
		return true
	}
	a.push(f)
}

func (t *dialogsTab) key(a *App, k tea.KeyPressMsg) bool {
	switch k.Keystroke() {
	case "left":
		t.cur = max(0, t.cur-1)
	case "right":
		t.cur = min(len(dialogButtons)-1, t.cur+1)
	case "enter", "space":
		t.press(a, t.cur)
	default:
		return false
	}
	return true
}

func (t *dialogsTab) draw(a *App, b *Buf, r Rect, focused bool) []Hit {
	th := a.th()
	var hits []Hit
	x := r.X
	for i, l := range dialogButtons {
		br := button(b, th, x, r.Y, l, focused && t.cur == i, th.CellBG)
		hits = append(hits, Hit{R: br, Kind: HitCtl, S: "button", A: i})
		x = br.Right() + 2
	}
	desc := []string{
		"asks “Delete 3 files?” — Cancel is the default; y / n answer",
		"edits dialog.profile (Name, Email) with per-field validation",
		"an information box with a single OK",
		"a toast only you see (top right, 4 s)",
		"sends a toast to everyone connected",
	}
	b.Text(r.X, r.Y+1, "→ "+desc[t.cur], th.base().Fg(th.Muted), r.W)
	p := a.vals.Profile()
	y := r.Y + 3
	ls := th.base().Fg(th.Muted)
	if _, ok := a.flash["dialog.profile"]; ok {
		ls = ls.Fg(th.Warn)
	}
	b.Text(r.X, y, "Profile (shared)", th.base().Fg(th.Accent).With(Bold), r.W)
	b.Text(r.X+2, y+1, "Name   "+core.SafeText(p.Name), ls.Fg(choose(ls.FG == th.Warn, th.Warn, th.FG)), r.W-2)
	b.Text(r.X+2, y+2, "Email  "+core.SafeText(p.Email), ls.Fg(choose(ls.FG == th.Warn, th.Warn, th.FG)), r.W-2)
	return hits
}

func (t *dialogsTab) mouse(a *App, m hitMsg) {
	if isClick(m) && m.hit.S == "button" {
		t.press(a, m.hit.A)
	}
}
