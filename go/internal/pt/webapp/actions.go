//go:build js && wasm

package webapp

// One table of actions drives global key dispatch, the menu bar, the
// command palette and the help screen, so the four can't disagree.

import (
	"fmt"
	"strings"
)

type action struct {
	ID    string
	Label string
	Keys  []string // web key specs; first is shown
	Group string   // help section
	Run   func(*App)
	When  func(*App) bool
}

func gridOnly(a *App) bool { return a.curView().ActiveGrid() != nil }

func withGrid(f func(*Grid)) func(*App) {
	return func(a *App) {
		if g := a.curView().ActiveGrid(); g != nil {
			f(g)
		}
	}
}

var actions []*action

func init() {
	actions = []*action{
		{ID: "help", Label: "Keyboard help", Keys: []string{"F1", "C-g"}, Group: "Global", Run: func(a *App) { a.OpenDialog(newHelp(a)) }},
		{ID: "menu", Label: "Menu bar", Keys: []string{"F10"}, Group: "Global", Run: func(a *App) { a.menuOpen, a.menuSel = 0, 0 }},
		{ID: "palette", Label: "Command palette", Keys: []string{"C-k", "C-p"}, Group: "Global", Run: func(a *App) { a.OpenDialog(newPalette(a)) }},
		{ID: "filter", Label: "Focus filter bar", Keys: []string{"C-f", "F3"}, Group: "Global", Run: func(a *App) { a.focusQuery() }},
		{ID: "clear_filter", Label: "Clear filter", Group: "Data", Run: func(a *App) { a.curView().SetQuery(""); a.syncQueryInput() }},
		{ID: "next_view", Label: "Next view", Keys: []string{"A-ArrowRight"}, Group: "Global", Run: func(a *App) { a.SwitchView((a.view + 1) % len(a.views)) }},
		{ID: "prev_view", Label: "Previous view", Keys: []string{"A-ArrowLeft"}, Group: "Global", Run: func(a *App) { a.SwitchView((a.view + len(a.views) - 1) % len(a.views)) }},
		{ID: "next_pane", Label: "Next pane", Keys: []string{"F6"}, Group: "Global", Run: func(a *App) { a.curView().NextPane(1) }},
		{ID: "prev_pane", Label: "Previous pane", Keys: []string{"S-F6"}, Group: "Global", Run: func(a *App) { a.curView().NextPane(-1) }},
		{ID: "undo", Label: "Undo (your last change)", Keys: []string{"C-z"}, Group: "Edit", Run: func(a *App) { a.Undo() }},
		{ID: "redo", Label: "Redo", Keys: []string{"C-y", "C-S-z"}, Group: "Edit", Run: func(a *App) { a.Redo() }},
		{ID: "new", Label: "New row / record", Keys: []string{"Insert", "A-n"}, Group: "Edit", Run: func(a *App) { a.curView().New() }},
		{ID: "new_project", Label: "New project…", Group: "File", Run: func(a *App) { a.OpenDialog(newProjectDialog(a)) }},
		{ID: "new_task", Label: "New task…", Group: "File", Run: func(a *App) { a.OpenDialog(newTaskDialog(a, a.contextProject(), "")) }},
		{ID: "edit_record", Label: "Edit record (form)", Keys: []string{"C-e"}, Group: "Edit", Run: func(a *App) { a.editRecord() }},
		{ID: "delete", Label: "Delete row(s)", Keys: []string{"F8", "C-Delete"}, Group: "Edit", Run: withGrid(func(g *Grid) { g.DeleteRows() }), When: gridOnly},
		{ID: "copy", Label: "Copy", Keys: []string{"C-c"}, Group: "Edit", Run: withGrid(func(g *Grid) { g.Copy() }), When: gridOnly},
		{ID: "paste", Label: "Paste (use ^V)", Group: "Edit", Run: withGrid(func(g *Grid) { g.Paste(g.app.clipboard) }), When: gridOnly},
		{ID: "fill_down", Label: "Fill down", Keys: []string{"C-d"}, Group: "Edit", Run: withGrid(func(g *Grid) { g.FillDown() }), When: gridOnly},
		{ID: "clear_cell", Label: "Clear cell", Group: "Edit", Run: withGrid(func(g *Grid) { g.ClearCell() }), When: gridOnly},
		{ID: "mark_all", Label: "Mark all rows", Keys: []string{"C-a"}, Group: "Grid", Run: withGrid(func(g *Grid) { g.MarkAll() }), When: gridOnly},
		{ID: "toggle_detail", Label: "Toggle detail pane", Keys: []string{"C-b"}, Group: "View", Run: func(a *App) { a.curView().ToggleDetail() }},
		{ID: "columns", Label: "Columns…", Keys: []string{"F4"}, Group: "View", Run: withGrid(func(g *Grid) { g.app.OpenDialog(newColumnsDialog(g)) }), When: gridOnly},
		{ID: "sort_col", Label: "Sort by column (▲ ▼ off)", Keys: []string{"C-o"}, Group: "Data", Run: withGrid(func(g *Grid) { g.CycleSort(false) }), When: gridOnly},
		{ID: "sort_dialog", Label: "Sort…", Group: "Data", Run: withGrid(func(g *Grid) { g.app.OpenDialog(newSortDialog(g)) }), When: gridOnly},
		{ID: "group_col", Label: "Group by column", Keys: []string{"C-r"}, Group: "Data", Run: withGrid(func(g *Grid) { g.ToggleGroup() }), When: gridOnly},
		{ID: "clear_group", Label: "Clear grouping", Group: "Data", Run: withGrid(func(g *Grid) { g.SetGroup("") }), When: gridOnly},
		{ID: "save_filter", Label: "Save filter…", Group: "Data", Run: func(a *App) { a.OpenDialog(newPromptDialog(a, "Save filter", "Name", a.curView().Query(), a.saveFilter)) }},
		{ID: "theme", Label: "Next theme", Keys: []string{"C-t"}, Group: "View", Run: func(a *App) { a.cycleTheme() }},
		{ID: "density", Label: "Toggle density", Group: "View", Run: func(a *App) {
			if a.density == "compact" {
				a.density = ""
			} else {
				a.density = "compact"
			}
			storageSet("pt.density", a.density)
			a.applyTheme()
			for _, v := range a.views {
				v.DataChanged(false)
			}
		}},
		{ID: "context", Label: "Context menu", Keys: []string{"S-F10", "ContextMenu"}, Group: "Grid", Run: withGrid(func(g *Grid) { g.OpenContext(-1, -1) }), When: gridOnly},
		{ID: "logout", Label: "Log out", Group: "File", Run: func(a *App) { a.Logout() }},
		{ID: "about", Label: "About pt", Group: "Help", Run: func(a *App) { a.OpenDialog(newAboutDialog(a)) }},
	}
	for i, v := range viewNames {
		i, v := i, v
		actions = append(actions, &action{
			ID: "view" + fmt.Sprint(i+1), Label: "Go to " + v, Keys: []string{fmt.Sprintf("A-%d", i+1)}, Group: "Views",
			Run: func(a *App) { a.SwitchView(i) },
		})
	}
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
	if act := actionByID(id); act != nil && act.Run != nil && (act.When == nil || act.When(a)) {
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

var themes = []string{"midnight", "daylight", "solarized", "gruvbox"}
var themeNames = map[string]string{"midnight": "Midnight", "daylight": "Daylight", "solarized": "Solarized Dark", "gruvbox": "Gruvbox"}

func (a *App) setTheme(t string) {
	a.theme = t
	storageSet("pt.theme", t)
	a.applyTheme()
	a.setStatus("info", "Theme: "+themeNames[t])
}

func (a *App) cycleTheme() {
	for i, t := range themes {
		if t == a.theme {
			a.setTheme(themes[(i+1)%len(themes)])
			return
		}
	}
	a.setTheme(themes[0])
}

// ---- menus ---------------------------------------------------------------------------------

type menuDef struct {
	Title string
	Accel string // Alt+letter
	Items []string
}

func (a *App) menus() []menuDef {
	views := []string{}
	for i := range viewNames {
		views = append(views, fmt.Sprintf("view%d", i+1))
	}
	data := []string{"filter", "clear_filter", "-", "sort_col", "sort_dialog", "group_col", "clear_group", "-", "save_filter"}
	for i := range a.savedQ {
		data = append(data, fmt.Sprintf("saved:%d", i))
	}
	view := append(views, "-", "toggle_detail", "columns", "-", "theme:midnight", "theme:daylight", "theme:solarized", "theme:gruvbox", "density")
	return []menuDef{
		{"File", "f", []string{"new_project", "new_task", "-", "logout"}},
		{"Edit", "e", []string{"undo", "redo", "-", "copy", "paste", "fill_down", "clear_cell", "-", "edit_record", "new", "delete"}},
		{"View", "v", view},
		{"Data", "d", data},
		{"Help", "h", []string{"help", "palette", "about"}},
	}
}

// menuItem resolves a menu entry id to (label, key, checked, run).
func (a *App) menuItem(id string) (string, string, bool, func()) {
	switch {
	case strings.HasPrefix(id, "theme:"):
		t := strings.TrimPrefix(id, "theme:")
		return "Theme: " + themeNames[t], "", a.theme == t, func() { a.setTheme(t) }
	case strings.HasPrefix(id, "saved:"):
		var i int
		fmt.Sscanf(strings.TrimPrefix(id, "saved:"), "%d", &i)
		if i < len(a.savedQ) {
			f := a.savedQ[i]
			return "★ " + f.Name, viewNames[f.View], false, func() { a.applySaved(f) }
		}
		return "?", "", false, nil
	}
	act := actionByID(id)
	if act == nil {
		return id, "", false, nil
	}
	checked := false
	if strings.HasPrefix(id, "view") {
		var i int
		fmt.Sscanf(strings.TrimPrefix(id, "view"), "%d", &i)
		checked = a.view == i-1
	}
	if id == "density" {
		checked = a.density == "compact"
	}
	enabled := act.When == nil || act.When(a)
	if !enabled {
		return act.Label, keyLabel(act), checked, nil
	}
	return act.Label, keyLabel(act), checked, func() { a.closeMenus(); act.Run(a) }
}

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
	case k.Is("Enter", " "):
		if _, _, _, run := a.menuItem(items[a.menuSel]); run != nil {
			a.menuOpen = -1
			run()
		}
	default:
		// Alt+letter switches menus while one is open
		for i, m := range ms {
			if k.Is("A-" + m.Accel) {
				a.menuOpen, a.menuSel = i, 0
				return true
			}
		}
		return false
	}
	return true
}
