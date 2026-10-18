package tui

// Key and mouse routing, global actions, the menu bar and the command
// palette (ui-spec §2–3).

import (
	"fmt"
	"time"

	tea "charm.land/bubbletea/v2"

	"git.wharf.fluidgrid.org/jcz/tictactoe/go/internal/demo/core"
)

func (a *App) quit() tea.Cmd {
	a.quitting = true
	a.cancel()
	return tea.Quit
}

func (a *App) onKey(k tea.KeyPressMsg) tea.Cmd {
	key := k.Keystroke()
	if key == "ctrl+c" || key == "ctrl+q" && a.ph != phMain {
		return a.quit()
	}
	switch a.ph {
	case phJoin:
		return a.join.key(a, k)
	case phConnecting:
		if key == "esc" {
			if a.opts.Name != "" {
				return a.quit()
			}
			// back to the form to fix the server or name
			a.cancelSession()
			a.ph = phJoin
		}
		return nil
	}
	if key == "ctrl+q" {
		return a.quit()
	}
	if o := a.top(); o != nil {
		// the global help / palette keys still work over a menu
		if ac := lookupGlobal(key); (ac == AHelp || ac == APalette) && !isHelp(o) {
			a.overlays = nil
			return a.do(ac)
		}
		o.key(a, k)
		return nil
	}
	if a.drag != nil {
		return nil
	}
	ac := lookupGlobal(key)
	if ac != ANone && !tabFirst[key] {
		return a.do(ac)
	}
	// Alt+letter opens a menu
	if len(key) == 5 && key[:4] == "alt+" {
		for i, m := range menuBar {
			if rune(key[4]) == m.accel {
				a.openMenu(i)
				return nil
			}
		}
	}
	if a.inBody {
		t := a.curTab()
		if t.key(a, k) {
			return nil
		}
		if ac != ANone {
			return a.do(ac)
		}
		switch key {
		case "tab":
			if t.focused()+1 < t.controls() {
				t.focusCtl(a, t.focused()+1)
			} else {
				a.inBody = false // past the last control: back to the sidebar
			}
		case "shift+tab":
			if t.focused() > 0 {
				t.focusCtl(a, t.focused()-1)
			} else {
				a.inBody = false
			}
		case "esc":
			a.inBody = false
		}
		return nil
	}
	if ac != ANone {
		return a.do(ac)
	}
	// sidebar
	switch key {
	case "up":
		a.setTab(a.tab - 1)
	case "down":
		a.setTab(a.tab + 1)
	case "home", "pgup":
		a.setTab(0)
	case "end", "pgdown":
		a.setTab(len(a.tabs) - 1)
	case "enter", "right", "tab", "space":
		a.enterBody(0)
	case "shift+tab":
		a.enterBody(-1)
	default:
		// a digit jumps to that tab (1–9)
		if k.Text >= "1" && k.Text <= "9" && len(k.Text) == 1 {
			a.setTab(int(k.Text[0] - '1'))
		}
	}
	return nil
}

func isHelp(o overlay) bool { _, ok := o.(*helpDlg); return ok }

func (a *App) cancelSession() {
	a.cancel()
	a.sess = nil
	a.ctx, a.cancel = contextWithCancel()
}

// do runs an action (keys, menus, palette and hint clicks all land here).
func (a *App) do(ac Action) tea.Cmd {
	switch {
	case ac >= ATab1 && ac < ATab1+10:
		a.setTab(int(ac - ATab1))
		return nil
	}
	switch ac {
	case AHelp:
		a.openHelp()
	case AMenu:
		a.openMenu(0)
	case APalette:
		a.openPalette()
	case ANextTab:
		a.setTab(a.tab + 1)
	case APrevTab:
		a.setTab(a.tab - 1)
	case AReset:
		ti := a.curTabInfo()
		a.confirm("Reset tab", fmt.Sprintf("Reset %s to its defaults for everyone? (%s)", ti.Title, joinKeys(ti.Keys)), "Reset",
			func(a *App) { a.resetTab() })
	case AResetAll:
		a.confirm("Reset all tabs", "Reset every control on every tab to its default, for everyone?", "Reset all",
			func(a *App) { a.resetKeys("all tabs") })
	case ATheme:
		a.theme = (a.theme + 1) % len(themes)
		a.info("Theme: " + themes[a.theme].Name)
	case ARedraw:
		return tea.ClearScreen
	case AQuit:
		return a.quit()
	case AFocus:
		if a.inBody {
			a.inBody = false
		} else {
			a.enterBody(a.curTab().focused())
		}
	case AAbout:
		a.about()
	case AToggleKeys:
		a.hideKeys = !a.hideKeys
	case AToggleValue:
		a.hideVal = !a.hideVal
	}
	return nil
}

func (a *App) resetTab() {
	ti := a.curTabInfo()
	a.resetKeys(ti.Title, ti.Keys...)
}

func joinKeys(keys []string) string {
	s := ""
	for i, k := range keys {
		if i > 0 {
			s += ", "
		}
		s += k
	}
	return s
}

func (a *App) about() {
	srv := "not connected"
	if a.vals.Server.Impl != "" {
		srv = core.SafeText(a.vals.Server.Impl + " " + a.vals.Server.Version)
	}
	a.message("About demo",
		"demo — a control gallery: every control alone on its own tab, synced live.",
		"",
		"Server:   "+srv+" at "+a.opts.Server,
		"Client:   go "+core.Version+" (Bubble Tea TUI, tui-go)",
		fmt.Sprintf("Protocol: %d", core.ProtocolVersion),
		"You:      "+core.SafeText(a.myName()))
}

// ================= menu bar =================

type menuDef struct {
	title string
	accel rune
	items func(a *App) []menuItem
}

var menuBar []menuDef

func init() { menuBar = menuDefs }

var menuDefs = []menuDef{
	{"File", 'f', func(a *App) []menuItem {
		return []menuItem{
			{label: "Reset this tab", key: "^R", accel: 'r', run: func(a *App) { a.do(AReset) }},
			{label: "Reset all tabs…", accel: 'a', run: func(a *App) { a.do(AResetAll) }},
			{sep: true},
			{label: "Quit", key: "^Q", accel: 'q', run: func(a *App) { a.queue(a.quit()) }},
		}
	}},
	{"View", 'v', func(a *App) []menuItem {
		var items []menuItem
		for i, tb := range core.Tabs {
			key := ""
			if i < 10 {
				key = fmt.Sprintf("Alt+%d", (i+1)%10)
			}
			items = append(items, menuItem{label: fmt.Sprintf("%d %s", tb.Num, tb.Title), key: key, run: func(a *App) { a.setTab(i) }})
		}
		var themesSub []menuItem
		for i, th := range themes {
			cur := i == a.theme
			themesSub = append(themesSub, menuItem{label: th.Name, checked: &cur, run: func(a *App) { a.theme = i }})
		}
		keys, val := !a.hideKeys, !a.hideVal
		items = append(items,
			menuItem{sep: true},
			menuItem{label: "Next tab", key: "^PgDn", accel: 'n', run: func(a *App) { a.do(ANextTab) }},
			menuItem{label: "Previous tab", key: "^PgUp", accel: 'p', run: func(a *App) { a.do(APrevTab) }},
			menuItem{label: "Theme", accel: 't', sub: themesSub},
			menuItem{label: "Keys panel", accel: 'k', checked: &keys, run: func(a *App) { a.do(AToggleKeys) }},
			menuItem{label: "Value panel", accel: 'v', checked: &val, run: func(a *App) { a.do(AToggleValue) }},
		)
		return items
	}},
	{"Help", 'h', func(a *App) []menuItem {
		return []menuItem{
			{label: "Keyboard help", key: "F1", accel: 'k', run: func(a *App) { a.openHelp() }},
			{label: "About", accel: 'a', run: func(a *App) { a.about() }},
		}
	}},
}

// menuX is the x of each menu title (as drawMenuBar lays them out).
func menuX(i int) int {
	x := 1
	for j := 0; j < i; j++ {
		x += StrW(menuBar[j].title) + 3
	}
	return x
}

func (a *App) openMenu(i int) {
	i = (i + len(menuBar)) % len(menuBar)
	l := newList("", menuBar[i].items(a), menuX(i), 1)
	l.menuBar = i
	if i == 1 {
		l.maxRows = max(6, a.h-4)
	}
	l.onLR = func(a *App, d int) { a.openMenu(i + d) }
	a.push(l)
}

// ================= command palette =================

// commands lists every action with its key (ui-spec §3).
func (a *App) commands() []menuItem {
	var out []menuItem
	for i, tb := range core.Tabs {
		key := ""
		if i < 10 {
			key = fmt.Sprintf("Alt+%d", (i+1)%10)
		}
		out = append(out, menuItem{label: "Go to: " + tb.Title, key: key, run: func(a *App) { a.setTab(i) }})
	}
	out = append(out,
		menuItem{label: "Reset this tab", key: "^R", run: func(a *App) { a.do(AReset) }},
		menuItem{label: "Reset all tabs", run: func(a *App) { a.do(AResetAll) }},
	)
	for i, th := range themes {
		out = append(out, menuItem{label: "Theme: " + th.Name, run: func(a *App) { a.theme = i; a.info("Theme: " + themes[i].Name) }})
	}
	out = append(out,
		menuItem{label: "Help", key: "F1", run: func(a *App) { a.openHelp() }},
		menuItem{label: "About", run: func(a *App) { a.about() }},
		menuItem{label: "Open menu bar", key: "F10", run: func(a *App) { a.openMenu(0) }},
		menuItem{label: "Toggle Keys panel", run: func(a *App) { a.do(AToggleKeys) }},
		menuItem{label: "Toggle Value panel", run: func(a *App) { a.do(AToggleValue) }},
		menuItem{label: "Quit", key: "^Q", run: func(a *App) { a.queue(a.quit()) }},
	)
	return out
}

// runCommand runs a palette command and records it in palette.last
// (from ^K and from the Palette tab alike).
func (a *App) runCommand(it menuItem) {
	if a.online() {
		_ = a.commit("palette.last", it.label)
	}
	if it.run != nil {
		it.run(a)
	}
}

func (a *App) openPalette() {
	l := newList("Commands", a.commands(), -1, 0)
	l.filter = &lineInput{placeholder: "type a command…"}
	l.width = min(64, a.w-4)
	l.maxRows = max(5, min(14, a.h-10))
	l.refilter()
	l.onRun = func(a *App, it menuItem) {
		if a.online() {
			_ = a.commit("palette.last", it.label)
		}
	}
	a.push(l)
}

// ================= mouse =================

func (a *App) onMouse(m hitMsg) tea.Cmd {
	if a.ph == phJoin {
		a.join.mouse(a, m)
		return nil
	}
	if a.ph != phMain {
		return nil
	}
	// a drag in progress gets motion and the release
	if d := a.drag; d != nil {
		switch ev := m.ev.(type) {
		case tea.MouseMotionMsg:
			d.x, d.y = ev.X, ev.Y
			if ev.X != d.x0 || ev.Y != d.y0 {
				d.moved = true
			}
			if d.move != nil {
				d.move(a, ev.X, ev.Y)
			}
			return nil
		case tea.MouseReleaseMsg:
			a.drag = nil
			d.x, d.y = ev.X, ev.Y
			if d.drop != nil {
				d.drop(a, ev.X, ev.Y, m.hits)
			}
			return nil
		}
	}
	if o := a.top(); o != nil {
		o.mouse(a, m)
		return nil
	}
	if !m.ok {
		return nil
	}
	h := m.hit
	_, click := m.ev.(tea.MouseClickMsg)
	if isClick(m) {
		switch h.Kind {
		case HitMenuTitle:
			a.openMenu(h.A)
			return nil
		case HitSideTab:
			a.setTab(h.A)
			a.inBody = false
			return nil
		case HitHint:
			return a.onKey(synthKey(h.S))
		case HitToast:
			if h.A < len(a.toasts) {
				a.toasts = append(a.toasts[:h.A], a.toasts[h.A+1:]...)
			}
			return nil
		}
	}
	if d, ok := isWheel(m); ok && h.Kind == HitSideTab {
		a.setTab(a.tab + d)
		return nil
	}
	if h.Kind == HitCtl {
		if click {
			a.inBody = true
		}
		a.curTab().mouse(a, m)
		if click {
			a.lastClick, a.lastHit = time.Now(), h
		}
	}
	return nil
}

// doubleClick reports whether this click repeats the last one on the same
// target within 400 ms.
func (a *App) doubleClick(h Hit) bool {
	return time.Since(a.lastClick) < 400*time.Millisecond && a.lastHit.S == h.S && a.lastHit.A == h.A && a.lastHit.B == h.B
}

func (a *App) onPaste(s string) {
	switch {
	case a.ph == phJoin:
		a.join.paste(s)
	case a.top() != nil:
		a.top().paste(a, s)
	case a.inBody:
		if p, ok := a.curTab().(pasteTarget); ok {
			p.paste(a, s)
		}
	}
}
