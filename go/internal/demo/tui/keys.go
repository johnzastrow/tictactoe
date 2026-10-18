package tui

// Global keys (ui-spec §3) in one table that drives dispatch, the hint bar,
// the menus and the help screen, so they can never disagree. Keys are Bubble
// Tea keystroke strings ("ctrl+k", "alt+1", "f10").

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
	ANextTab
	APrevTab
	AReset
	AResetAll
	ATheme
	ARedraw
	AQuit
	AFocus // F6: sidebar ⇄ tab body
	AAbout
	AToggleKeys
	AToggleValue
	ATab1 // ATab1+n: go to tab n+1 (Alt+1…9, Alt+0 = 10)
)

var actionLabels = map[Action]string{
	AHelp: "Help", AMenu: "Menu", APalette: "Commands", ANextTab: "Next tab", APrevTab: "Previous tab",
	AReset: "Reset this tab", AResetAll: "Reset all tabs…", ATheme: "Theme", ARedraw: "Redraw", AQuit: "Quit",
	AFocus: "Sidebar ⇄ tab", AAbout: "About", AToggleKeys: "Toggle Keys panel", AToggleValue: "Toggle Value panel",
}

type Bind struct {
	Keys   []string
	Action Action
}

// globalBinds are checked before the tab body sees a key, except the ones
// in tabFirst (a focused control may own them: ^T is "today" in a date field).
var globalBinds = []Bind{
	{[]string{"f1", "ctrl+g"}, AHelp},
	{[]string{"f10"}, AMenu},
	{[]string{"ctrl+k", "ctrl+p"}, APalette},
	{[]string{"ctrl+pgdown"}, ANextTab},
	{[]string{"ctrl+pgup"}, APrevTab},
	{[]string{"ctrl+r"}, AReset},
	{[]string{"ctrl+t"}, ATheme},
	{[]string{"ctrl+l"}, ARedraw},
	{[]string{"ctrl+q"}, AQuit},
	{[]string{"f6"}, AFocus},
	{[]string{"alt+1"}, ATab1},
	{[]string{"alt+2"}, ATab1 + 1},
	{[]string{"alt+3"}, ATab1 + 2},
	{[]string{"alt+4"}, ATab1 + 3},
	{[]string{"alt+5"}, ATab1 + 4},
	{[]string{"alt+6"}, ATab1 + 5},
	{[]string{"alt+7"}, ATab1 + 6},
	{[]string{"alt+8"}, ATab1 + 7},
	{[]string{"alt+9"}, ATab1 + 8},
	{[]string{"alt+0"}, ATab1 + 9},
}

var tabFirst = map[string]bool{"ctrl+t": true}

func (a Action) Label() string { return actionLabels[a] }

// keyName normalises a key press: printable characters match as themselves
// ("<", "S", "+"), everything else by keystroke.
func keyName(k tea.KeyPressMsg) string {
	if k.Text != "" && k.Mod&(tea.ModCtrl|tea.ModAlt) == 0 {
		if k.Text == " " {
			return "space"
		}
		return k.Text
	}
	return k.Keystroke()
}

func lookupGlobal(key string) Action {
	for _, b := range globalBinds {
		for _, k := range b.Keys {
			if k == key {
				return b.Action
			}
		}
	}
	return ANone
}

// keysFor is the first key bound to an action, as a label ("^K").
func keysFor(a Action) string {
	for _, b := range globalBinds {
		if b.Action == a {
			return keyLabel(b.Keys[0])
		}
	}
	return ""
}

// keyLabel renders a key nano-style: ctrl+k → ^K, f2 → F2, shift+left → ⇧←.
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
	case "home":
		base = "Home"
	case "end":
		base = "End"
	case "backspace":
		base = "⌫"
	case "menu":
		base = "Menu"
	default:
		base = strings.ToUpper(base)
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

// hint is one "KEY Label" pair in the hint bar. Press is the keystroke a
// click on it synthesises ("" = not clickable).
type hint struct {
	Key, Label, Press string
}

// h builds a hint whose key label comes from the keystroke it presses.
func h(press, label string) hint { return hint{keyLabel(press), label, press} }

var namedKeys = map[string]rune{
	"enter": tea.KeyEnter, "esc": tea.KeyEscape, "tab": tea.KeyTab, "backspace": tea.KeyBackspace,
	"delete": tea.KeyDelete, "insert": tea.KeyInsert, "up": tea.KeyUp, "down": tea.KeyDown,
	"left": tea.KeyLeft, "right": tea.KeyRight, "home": tea.KeyHome, "end": tea.KeyEnd,
	"pgup": tea.KeyPgUp, "pgdown": tea.KeyPgDown, "space": tea.KeySpace, "menu": tea.KeyMenu,
	"f1": tea.KeyF1, "f2": tea.KeyF2, "f3": tea.KeyF3, "f4": tea.KeyF4, "f5": tea.KeyF5, "f6": tea.KeyF6,
	"f7": tea.KeyF7, "f8": tea.KeyF8, "f9": tea.KeyF9, "f10": tea.KeyF10,
}

// synthKey builds the key press for a keystroke string (hint clicks, tests):
// "ctrl+s", "shift+left", "f10", "x".
func synthKey(s string) tea.KeyPressMsg {
	var mod tea.KeyMod
	for {
		switch {
		case strings.HasPrefix(s, "ctrl+") && len(s) > 5:
			mod |= tea.ModCtrl
			s = s[5:]
			continue
		case strings.HasPrefix(s, "alt+") && len(s) > 4:
			mod |= tea.ModAlt
			s = s[4:]
			continue
		case strings.HasPrefix(s, "shift+") && len(s) > 6:
			mod |= tea.ModShift
			s = s[6:]
			continue
		}
		break
	}
	if code, ok := namedKeys[s]; ok {
		k := tea.KeyPressMsg{Code: code, Mod: mod}
		if code == tea.KeySpace && mod == 0 {
			k.Text = " "
		}
		return k
	}
	r := []rune(s)[0]
	k := tea.KeyPressMsg{Code: r, Mod: mod}
	if mod&(tea.ModCtrl|tea.ModAlt) == 0 {
		k.Text = s
	}
	return k
}
