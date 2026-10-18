package webapp

// Per-tab text (ui-spec §4): the one-sentence explanation under each tab's
// title and the Keys panel. The help screen's "This tab" and "All tabs"
// sections are built from the same table, so they can't disagree. Pure Go.

// tabText is one tab's explanation and its keys as [key, what] pairs.
type tabText struct {
	Explain string
	Keys    [][2]string
}

var tabTexts = map[string]tabText{
	"text": {"Single-line fields with live validation; the password is masked and never leaves the browser.", [][2]string{
		{"Enter", "save the field"}, {"Esc", "revert to the synced value (or take theirs)"},
		{"← → Home End", "move the caret"}, {"^← ^→", "word jump"}, {"^U", "clear the field"},
		{"^O, 👁", "show / hide the password"}, {"Tab ⇧Tab", "next / previous field"},
	}},
	"number": {"A decimal spinner, an integer spinner and a slider.", [][2]string{
		{"↑ ↓", "±1"}, {"⇧↑ ⇧↓", "±0.25 (Hours)"}, {"Enter", "save a spinner"}, {"Esc", "revert a spinner"},
		{"▲ ▼", "click to step and save"}, {"← →", "slider ±1"}, {"PgUp PgDn", "slider ±10"},
		{"Home End", "slider 0 / 100"}, {"click, drag", "set the slider"},
	}},
	"date": {"Natural-language dates with a live preview and a month calendar.", [][2]string{
		{"type", "fri, tom, +3d, -2w, eow, eom, 23 oct, 2026-11-02"}, {"↑ ↓", "calendar ±1 week"},
		{"^← ^→", "calendar ±1 day"}, {"PgUp PgDn", "calendar ±1 month"},
		{"Enter", "save the typed date, else the calendar day"}, {"t (button)", "today"},
		{"Del", "clear the date"}, {"Esc", "revert"}, {"click a day", "pick it"},
	}},
	"select": {"Closed dropdowns showing the value as a coloured chip.", [][2]string{
		{"Enter Space ↓", "open the list"}, {"↑ ↓", "move"}, {"type", "filter (first letter jumps)"},
		{"Enter", "pick and save"}, {"Esc", "close"}, {"click", "open / pick"},
	}},
	"combo": {"Type-ahead pickers with fuzzy matching over the people list.", [][2]string{
		{"type", "fuzzy search name, username, team"}, {"↓, click", "open the list"}, {"↑ ↓", "move"},
		{"Enter", "pick and save"}, {"Esc", "close / revert"}, {"Del", "clear to — none —"},
	}},
	"tags": {"Tag chips with ranked suggestions; every add or remove is saved.", [][2]string{
		{"type", "filter suggestions"}, {"↑ ↓", "choose a suggestion"},
		{"Enter", "add the suggestion or create the typed tag"}, {"Backspace", "remove the last chip (empty input)"},
		{"← →", "select a chip (empty input)"}, {"Del", "remove the selected chip"}, {"click ×", "remove a chip"},
	}},
	"textarea": {"A multi-line editor with word wrap, a position and word count, and explicit save.", [][2]string{
		{"Enter", "new line"}, {"^S", "save"}, {"Esc", "revert (asks first if edited)"},
		{"arrows PgUp PgDn", "move"}, {"^Home ^End", "start / end"}, {"Tab", "leave, keeping the draft"},
	}},
	"choice": {"A checkbox, a checkbox group, a radio group and a toggle switch.", [][2]string{
		{"Space", "toggle / select"}, {"← → ↑ ↓", "move within a group"}, {"Tab ⇧Tab", "next / previous group"},
		{"click", "toggle / select"},
	}},
	"cascade": {"Three dependent dropdowns: each level lists only the children of the one above.", [][2]string{
		{"Enter Space ↓", "open a list"}, {"type", "filter"}, {"Enter", "pick and save"},
		{"Del", "clear this level and below"}, {"Tab ⇧Tab", "next / previous level"}, {"Esc", "close"},
	}},
	"grid": {"An editable grid with per-column editors, a totals row and view-only sorting.", [][2]string{
		{"arrows Tab ⇧Tab", "move"}, {"Home End", "first / last column"}, {"Enter F2, type", "edit the cell"},
		{"Enter", "save and move down"}, {"Esc", "cancel the edit"}, {"Del", "clear Due"},
		{"Ins", "add a row"}, {"F8 ^Del", "delete the row"}, {"^O", "sort by column (▲ ▼ off)"},
		{"click, dbl-click", "select / edit"}, {"click header", "sort"},
	}},
	"menus": {"A menu bar with submenus, and a file list with a context menu.", [][2]string{
		{"Enter ↓", "open the menu bar"}, {"← →", "between menus"}, {"→", "open a submenu"},
		{"Enter", "pick an item"}, {"Esc", "close"}, {"↑ ↓", "move in the file list"},
		{"Enter ⇧F10 Menu", "context menu on a file"}, {"right-click", "context menu"},
	}},
	"palette": {"The command palette, shown inline: the same list ^K opens anywhere.", [][2]string{
		{"type", "fuzzy search commands"}, {"↑ ↓", "move"}, {"Enter", "run (and remember it)"},
		{"click", "run"}, {"^K ^P", "open the palette anywhere"},
	}},
	"dialogs": {"Buttons that open a confirm, a form, a message, a toast and a broadcast.", [][2]string{
		{"← → Tab", "choose a button"}, {"Enter Space", "open"}, {"y n", "confirm / cancel"},
		{"Tab", "between form fields"}, {"^S", "save the form"}, {"Esc", "close a dialog"},
	}},
	"charts": {"Twelve monthly values drawn as bars, a sparkline, a gauge and a heat strip.", [][2]string{
		{"← →", "select a month"}, {"↑ ↓", "±5"}, {"PgUp PgDn", "±20"},
		{"click a bar", "select it"}, {"drag a bar", "set its value"},
	}},
	"kanban": {"A board of cards in four columns; drag or use the keys to move them.", [][2]string{
		{"← →", "between columns"}, {"↑ ↓", "between cards"}, {"⇧← ⇧→, < >", "move the card"},
		{"Ins", "new card"}, {"Enter", "rename"}, {"F8 ^Del", "delete"}, {"drag", "move a card"},
	}},
	"gantt": {"A timeline of bars over a week-ticked axis with weekends and a today line.", [][2]string{
		{"↑ ↓", "select a bar"}, {"⇧← ⇧→, < >", "move ±1 day"}, {"[ ]", "shrink / extend"},
		{"+ -", "zoom day ▸ week ▸ month"}, {"t", "scroll to today"}, {"← →", "scroll the axis"},
		{"drag a bar", "move it"}, {"drag its right edge", "resize"},
	}},
}

// gettingAround and globalKeys are the help screen's fixed sections.
var gettingAround = [][2]string{
	{"↑ ↓ (sidebar)", "previous / next tab"}, {"Home End (sidebar)", "first / last tab"},
	{"Enter → Tab (sidebar)", "into the tab body"}, {"Tab ⇧Tab", "next / previous control"},
	{"⇧Tab (first control)", "back to the sidebar"}, {"Esc", "close a popup, else back to the sidebar"},
	{"F6", "toggle sidebar ⇄ tab body"},
}

var globalKeys = [][2]string{
	{"F1 ^G", "this help"}, {"F10", "menu bar"}, {"Alt+F Alt+V Alt+H", "open a menu"},
	{"^K ^P", "command palette"}, {"Alt+↓ Alt+↑", "next / previous tab"}, {"Alt+1…9 Alt+0", "go to tab 1–10"},
	{"^R", "reset this tab to defaults"}, {"View menu", "theme"}, {"File ▸ Leave", "back to the join page"},
	{"⇧F10 Menu right-click", "context menu"},
}

const howItWorks = "One server holds the current value of every control. Every connected client — " +
	"TUIs and browsers, Rust and Go — sees each change live. Each key has its own version: " +
	"a save carries the version you started from, and if someone else saved first you get a " +
	"conflict and the control shows their value."
