package tui

// One table drives key dispatch, the nano-style hint bar, menus and the
// help screen (ui-spec §2), so they can never disagree. Keys are Bubble Tea
// keystroke strings ("ctrl+k", "shift+f6", "alt+1").

import (
	"strings"

	tea "charm.land/bubbletea/v2"
)

type Action int

const (
	ANone Action = iota
	AHelp
	AMenu
	APalette
	AFilter
	AClearFilter
	ASaveFilter
	ANextView
	APrevView
	AView1
	AView2
	AView3
	AView4
	AView5
	AView6
	AView7
	AView8
	AView9
	ANextPane
	APrevPane
	AUndo
	ARedo
	ANew
	ANewProject
	ANewTask
	AEditRecord
	ADelete
	AToggleDetail
	AColumns
	ASortColumn
	ASortDialog
	AGroupColumn
	AClearGroup
	ATheme
	ARedraw
	AQuit
	ALogout
	AContextMenu
	ACopy
	APaste
	AFillDown
	AMarkAll
	AMark
	AClear
	AEdit
	ASetPassword
	AAbout
	ACardLeft
	ACardRight
	ASwimlanes
	AScope
	AZoomIn
	AZoomOut
	AToday
	ATimeBack
	ATimeFwd
	AShrinkDue
	AExtendDue
	ABarLeft
	ABarRight
	ADrill
)

var viewNames = []string{"Projects", "Tasks", "Board", "Timeline", "Dashboard", "Activity", "Templates", "Fields", "Users"}

var actionLabels = map[Action]string{
	AHelp: "Help", AMenu: "Menu", APalette: "Commands", AFilter: "Filter", AClearFilter: "Clear filter",
	ASaveFilter: "Save filter…", ANextView: "Next view", APrevView: "Previous view",
	ANextPane: "Next pane", APrevPane: "Previous pane", AUndo: "Undo", ARedo: "Redo", ANew: "New",
	ANewProject: "New project…", ANewTask: "New task…", AEditRecord: "Edit record", ADelete: "Delete",
	AToggleDetail: "Detail pane", AColumns: "Columns…", ASortColumn: "Sort by column", ASortDialog: "Sort…",
	AGroupColumn: "Group by column", AClearGroup: "Clear grouping", ATheme: "Theme", ARedraw: "Redraw",
	AQuit: "Quit", ALogout: "Log out", AContextMenu: "Context menu", ACopy: "Copy", APaste: "Paste",
	AFillDown: "Fill down", AMarkAll: "Mark all", AMark: "Mark row", AClear: "Clear cell", AEdit: "Edit cell",
	ASetPassword: "Set password…", AAbout: "About", ACardLeft: "Move card ◂", ACardRight: "Move card ▸",
	ASwimlanes: "Swimlanes", AScope: "Scope", AZoomIn: "Zoom in", AZoomOut: "Zoom out", AToday: "Today",
	ATimeBack: "Scroll ◂", ATimeFwd: "Scroll ▸", AShrinkDue: "Due −", AExtendDue: "Due +",
	ABarLeft: "Move ◂", ABarRight: "Move ▸", ADrill: "Drill down",
}

func (a Action) Label() string {
	if a >= AView1 && a <= AView9 {
		return viewNames[a-AView1]
	}
	return actionLabels[a]
}

type Ctx int

const (
	CtxGlobal Ctx = iota
	CtxGrid
	CtxBoard
	CtxTimeline
	CtxDashboard
)

type Bind struct {
	Keys   []string
	Action Action
	Ctx    Ctx
	Hint   int // order in the hint bar; 0 = not shown
}

var bindings = []Bind{
	// grid
	{[]string{"f2"}, AEdit, CtxGrid, 3},
	{[]string{"insert", "ctrl+n"}, ANew, CtxGrid, 4},
	{[]string{"f8", "ctrl+delete"}, ADelete, CtxGrid, 6},
	{[]string{"ctrl+e"}, AEditRecord, CtxGrid, 5},
	{[]string{"ctrl+o"}, ASortColumn, CtxGrid, 9},
	{[]string{"ctrl+r"}, AGroupColumn, CtxGrid, 10},
	{[]string{"ctrl+b"}, AToggleDetail, CtxGrid, 11},
	{[]string{"f4"}, AColumns, CtxGrid, 12},
	{[]string{"ctrl+c"}, ACopy, CtxGrid, 0},
	{[]string{"ctrl+v"}, APaste, CtxGrid, 0},
	{[]string{"ctrl+d"}, AFillDown, CtxGrid, 13},
	{[]string{"ctrl+a"}, AMarkAll, CtxGrid, 0},
	{[]string{"space"}, AMark, CtxGrid, 0},
	{[]string{"delete"}, AClear, CtxGrid, 0},
	// board
	{[]string{"shift+left", "<", ","}, ACardLeft, CtxBoard, 3},
	{[]string{"shift+right", ">", "."}, ACardRight, CtxBoard, 4},
	{[]string{"enter", "ctrl+e"}, AEditRecord, CtxBoard, 5},
	{[]string{"insert", "ctrl+n"}, ANew, CtxBoard, 6},
	{[]string{"ctrl+r"}, ASwimlanes, CtxBoard, 7},
	{[]string{"s", "S"}, AScope, CtxBoard, 8},
	{[]string{"f8", "ctrl+delete"}, ADelete, CtxBoard, 0},
	// timeline
	{[]string{"+", "="}, AZoomIn, CtxTimeline, 3},
	{[]string{"-"}, AZoomOut, CtxTimeline, 4},
	{[]string{"t", "T"}, AToday, CtxTimeline, 5},
	{[]string{"shift+left"}, ABarLeft, CtxTimeline, 6},
	{[]string{"shift+right"}, ABarRight, CtxTimeline, 7},
	{[]string{"["}, AShrinkDue, CtxTimeline, 8},
	{[]string{"]"}, AExtendDue, CtxTimeline, 9},
	{[]string{"<", ","}, ATimeBack, CtxTimeline, 10},
	{[]string{">", "."}, ATimeFwd, CtxTimeline, 11},
	{[]string{"ctrl+e"}, AEditRecord, CtxTimeline, 12},
	// dashboard
	{[]string{"enter"}, ADrill, CtxDashboard, 3},
	// global
	{[]string{"ctrl+k", "ctrl+p"}, APalette, CtxGlobal, 1},
	{[]string{"ctrl+f", "f3"}, AFilter, CtxGlobal, 2},
	{[]string{"ctrl+z"}, AUndo, CtxGlobal, 7},
	{[]string{"ctrl+y"}, ARedo, CtxGlobal, 0},
	{[]string{"f1", "ctrl+g"}, AHelp, CtxGlobal, 20},
	{[]string{"f10"}, AMenu, CtxGlobal, 21},
	{[]string{"ctrl+q"}, AQuit, CtxGlobal, 22},
	{[]string{"ctrl+pgdown"}, ANextView, CtxGlobal, 0},
	{[]string{"ctrl+pgup"}, APrevView, CtxGlobal, 0},
	{[]string{"f6"}, ANextPane, CtxGlobal, 0},
	{[]string{"shift+f6"}, APrevPane, CtxGlobal, 0},
	{[]string{"ctrl+t"}, ATheme, CtxGlobal, 0},
	{[]string{"ctrl+l"}, ARedraw, CtxGlobal, 0},
	{[]string{"shift+f10", "menu"}, AContextMenu, CtxGlobal, 0},
	{[]string{"alt+1"}, AView1, CtxGlobal, 0},
	{[]string{"alt+2"}, AView2, CtxGlobal, 0},
	{[]string{"alt+3"}, AView3, CtxGlobal, 0},
	{[]string{"alt+4"}, AView4, CtxGlobal, 0},
	{[]string{"alt+5"}, AView5, CtxGlobal, 0},
	{[]string{"alt+6"}, AView6, CtxGlobal, 0},
	{[]string{"alt+7"}, AView7, CtxGlobal, 0},
	{[]string{"alt+8"}, AView8, CtxGlobal, 0},
	{[]string{"alt+9"}, AView9, CtxGlobal, 0},
}

// keyName normalises a key press to the strings used in bindings.
func keyName(k tea.KeyPressMsg) string {
	s := k.Keystroke()
	// Printable characters: match the character itself ("<", "S", "+").
	if k.Text != "" && k.Mod&(tea.ModCtrl|tea.ModAlt) == 0 {
		if k.Text == " " {
			return "space"
		}
		return k.Text
	}
	return s
}

// lookup finds the action for a key: context bindings win over global ones.
func lookup(ctx Ctx, key string) Action {
	for pass := 0; pass < 2; pass++ {
		for _, b := range bindings {
			if (pass == 0 && b.Ctx != ctx) || (pass == 1 && b.Ctx != CtxGlobal) {
				continue
			}
			for _, k := range b.Keys {
				if k == key {
					return b.Action
				}
			}
		}
	}
	return ANone
}

// keysFor returns the first key bound to an action (for menus and hints).
func keysFor(a Action, ctx Ctx) string {
	for _, b := range bindings {
		if b.Action == a && (b.Ctx == ctx || b.Ctx == CtxGlobal) {
			return keyLabel(b.Keys[0])
		}
	}
	for _, b := range bindings {
		if b.Action == a {
			return keyLabel(b.Keys[0])
		}
	}
	return ""
}

// keyLabel renders a key nano-style: ctrl+k → ^K, f2 → F2.
func keyLabel(k string) string {
	if k == "+" || strings.HasSuffix(k, "++") {
		return strings.TrimSuffix(k, "+")
	}
	parts := strings.Split(k, "+")
	base := parts[len(parts)-1]
	mods := parts[:len(parts)-1]
	switch base {
	case "space":
		base = "Space"
	case "insert":
		base = "Ins"
	case "delete":
		base = "Del"
	case "pgdown":
		base = "PgDn"
	case "pgup":
		base = "PgUp"
	case "left":
		base = "←"
	case "right":
		base = "→"
	case "up":
		base = "↑"
	case "down":
		base = "↓"
	case "enter":
		base = "Enter"
	case "esc":
		base = "Esc"
	case "tab":
		base = "Tab"
	case "menu":
		base = "Menu"
	default:
		if len(base) > 1 && base[0] == 'f' {
			base = strings.ToUpper(base)
		} else {
			base = strings.ToUpper(base)
		}
	}
	out := ""
	for _, m := range mods {
		switch m {
		case "ctrl":
			out += "^"
		case "alt":
			out += "Alt+"
		case "shift":
			out += "⇧"
		}
	}
	return out + base
}

type hint struct {
	Key, Label string
	Action     Action
}

// hintsFor builds the hint bar for a context: context hints, then globals.
func hintsFor(ctx Ctx) []hint {
	var ctxH, globH []Bind
	for _, b := range bindings {
		if b.Hint == 0 {
			continue
		}
		if b.Ctx == ctx {
			ctxH = append(ctxH, b)
		} else if b.Ctx == CtxGlobal {
			globH = append(globH, b)
		}
	}
	sortBinds(ctxH)
	sortBinds(globH)
	var out []hint
	for _, b := range append(ctxH, globH...) {
		out = append(out, hint{keyLabel(b.Keys[0]), b.Action.Label(), b.Action})
	}
	return out
}

func sortBinds(bs []Bind) {
	for i := 1; i < len(bs); i++ {
		for j := i; j > 0 && bs[j].Hint < bs[j-1].Hint; j-- {
			bs[j], bs[j-1] = bs[j-1], bs[j]
		}
	}
}
