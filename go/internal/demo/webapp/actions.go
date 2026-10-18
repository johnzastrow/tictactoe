//go:build js && wasm

package webapp

// One table of actions drives global key dispatch, the menu bar, the
// command palette and the help screen, so the four can't disagree.

import (
	"fmt"
	"strings"

	"git.wharf.fluidgrid.org/jcz/tictactoe/go/internal/demo/core"
)

type action struct {
	ID      string
	Label   string   // menu / help label
	Palette string   // command palette label ("" = not in the palette)
	Keys    []string // web key specs; the first is shown
	Run     func(*App)
}

var actions []*action

func init() {
	actions = []*action{
		{ID: "help", Label: "Keyboard help", Palette: "Help", Keys: []string{"F1", "C-g"}, Run: func(a *App) { a.OpenDialog(newHelp(a)) }},
		{ID: "menu", Label: "Menu bar", Palette: "Open menu bar", Keys: []string{"F10"}, Run: func(a *App) {
			a.closePop()
			a.menuOpen, a.menuSel = 0, 0
		}},
		{ID: "palette", Label: "Command palette", Keys: []string{"C-k", "C-p"}, Run: func(a *App) { a.OpenDialog(newPaletteDialog(a)) }},
		{ID: "next_tab", Label: "Next tab", Keys: []string{"A-ArrowDown"}, Run: func(a *App) { a.SwitchTab((a.tab + 1) % len(a.tabs)) }},
		{ID: "prev_tab", Label: "Previous tab", Keys: []string{"A-ArrowUp"}, Run: func(a *App) { a.SwitchTab((a.tab + len(a.tabs) - 1) % len(a.tabs)) }},
		{ID: "pane", Label: "Toggle sidebar ⇄ tab body", Keys: []string{"F6"}, Run: func(a *App) {
			if a.focusSide {
				a.focusSide = false
				a.cur().FocusCur()
			} else {
				a.focusSidebar()
			}
		}},
		{ID: "reset_tab", Label: "Reset this tab", Palette: "Reset this tab", Keys: []string{"C-r"}, Run: func(a *App) { a.confirmResetTab() }},
		{ID: "reset_all", Label: "Reset all tabs…", Palette: "Reset all tabs", Run: func(a *App) {
			a.OpenDialog(newConfirm(a, "Reset all tabs", "Reset every control on all 16 tabs to its default?\nEveryone connected sees the change.", "Reset all", func() {
				a.Reset(nil, "all tabs")
			}))
		}},
		{ID: "toggle_keys", Label: "Toggle Keys panel", Run: func(a *App) {
			a.showKeys = !a.showKeys
			storageSet("demo.hideKeys", boolFlag(!a.showKeys))
		}},
		{ID: "toggle_value", Label: "Toggle Value panel", Run: func(a *App) {
			a.showValue = !a.showValue
			storageSet("demo.hideValue", boolFlag(!a.showValue))
		}},
		{ID: "leave", Label: "Leave", Palette: "Quit", Run: func(a *App) { a.Leave() }},
		{ID: "about", Label: "About", Palette: "About", Run: func(a *App) { a.OpenDialog(newAboutDialog(a)) }},
	}
	// Alt+1…9, Alt+0 go to tabs 1–10; 11–16 are reached from the sidebar, menu or palette.
	for i, t := range core.Tabs {
		i, t := i, t
		var keys []string
		if i < 10 {
			keys = []string{fmt.Sprintf("A-%d", (i+1)%10)}
		}
		actions = append(actions, &action{
			ID: "tab:" + t.ID, Label: fmt.Sprintf("%d %s", t.Num, t.Title), Palette: "Go to: " + t.Title, Keys: keys,
			Run: func(a *App) { a.SwitchTab(i) },
		})
	}
	for _, th := range themes {
		th := th
		actions = append(actions, &action{ID: "theme:" + th, Label: "Theme: " + themeNames[th], Palette: "Theme: " + themeNames[th],
			Run: func(a *App) { a.setTheme(th) }})
	}
}

func boolFlag(b bool) string {
	if b {
		return "1"
	}
	return ""
}

func actionByID(id string) *action {
	for _, a := range actions {
		if a.ID == id {
			return a
		}
	}
	return nil
}

func (a *App) run(id string) {
	if act := actionByID(id); act != nil {
		a.closeMenus()
		act.Run(a)
		a.Invalidate()
	}
}

func keyLabel(act *action) string {
	if len(act.Keys) == 0 {
		return ""
	}
	return Label(act.Keys[0])
}

// confirmResetTab: ^R — reset the current tab's keys after a confirm.
func (a *App) confirmResetTab() {
	t := a.cur().Info()
	a.OpenDialog(newConfirm(a, "Reset tab", fmt.Sprintf("Reset %s to its defaults?\n%s", t.Title, strings.Join(t.Keys, ", ")), "Reset", func() {
		a.Reset(t.Keys, t.Title)
	}))
}

var themes = []string{"midnight", "daylight", "solarized", "gruvbox"}
var themeNames = map[string]string{"midnight": "Midnight", "daylight": "Daylight", "solarized": "Solarized Dark", "gruvbox": "Gruvbox"}

func (a *App) setTheme(t string) {
	a.theme = t
	storageSet("demo.theme", t)
	a.applyTheme()
	a.info("Theme: " + themeNames[t])
}

// ---- menus ---------------------------------------------------------------------------------

type menuDef struct {
	Title string
	Accel string // Alt+letter
	Items []string
}

func (a *App) menus() []menuDef {
	view := []string{}
	for _, t := range core.Tabs {
		view = append(view, "tab:"+t.ID)
	}
	view = append(view, "-", "next_tab", "prev_tab", "-")
	for _, th := range themes {
		view = append(view, "theme:"+th)
	}
	view = append(view, "-", "toggle_keys", "toggle_value")
	return []menuDef{
		{"File", "f", []string{"reset_tab", "reset_all", "-", "leave"}},
		{"View", "v", view},
		{"Help", "h", []string{"help", "palette", "about"}},
	}
}

// menuItem resolves a menu entry id to (label, key, checked, run).
func (a *App) menuItem(id string) (string, string, bool, func()) {
	act := actionByID(id)
	if act == nil {
		return id, "", false, nil
	}
	checked := false
	switch {
	case strings.HasPrefix(id, "theme:"):
		checked = a.theme == strings.TrimPrefix(id, "theme:")
	case strings.HasPrefix(id, "tab:"):
		checked = a.cur().Info().ID == strings.TrimPrefix(id, "tab:")
	case id == "toggle_keys":
		checked = a.showKeys
	case id == "toggle_value":
		checked = a.showValue
	}
	return act.Label, keyLabel(act), checked, func() { a.closeMenus(); act.Run(a) }
}

func (a *App) closeMenus() { a.menuOpen = -1 }

func (a *App) menuKey(k Key) bool {
	ms := a.menus()
	items := ms[a.menuOpen].Items
	move := func(d int) {
		for range items {
			a.menuSel = (a.menuSel + d + len(items)) % len(items)
			if items[a.menuSel] != "-" {
				return
			}
		}
	}
	switch {
	case k.Is("Escape", "F10"):
		a.menuOpen = -1
		After(0, a.focusCurrent)
	case k.Is("ArrowLeft"):
		a.menuOpen = (a.menuOpen + len(ms) - 1) % len(ms)
		a.menuSel = 0
	case k.Is("ArrowRight"):
		a.menuOpen = (a.menuOpen + 1) % len(ms)
		a.menuSel = 0
	case k.Is("ArrowDown"):
		move(1)
	case k.Is("ArrowUp"):
		move(-1)
	case k.Is("Home"):
		a.menuSel = 0
	case k.Is("End"):
		a.menuSel = len(items) - 1
	case k.Is("Enter", " "):
		if _, _, _, run := a.menuItem(items[a.menuSel]); run != nil {
			a.menuOpen = -1
			run()
			After(0, a.focusCurrent)
		}
	default:
		// Alt+letter (or the plain letter) switches menus while one is open
		for i, m := range ms {
			if k.Is("A-"+m.Accel, m.Accel) {
				a.menuOpen, a.menuSel = i, 0
				return true
			}
		}
		return true // swallow everything else while the menu is open
	}
	return true
}
