package tui

// The tab contract. Each of the 16 tabs (ui-spec §4) is a tabCtl: it draws
// its controls into the control area, handles keys while the tab body has
// focus, and describes its keys for the Keys panel, the hint bar and help.
// The shell (render.go, actions.go) does the rest: title, explanation,
// Keys / Value panels, focus movement between controls, globals.

import (
	tea "charm.land/bubbletea/v2"

	"git.wharf.fluidgrid.org/jcz/tictactoe/go/internal/demo/core"
)

type tabCtl interface {
	// explain is the one-sentence explanation under the title.
	explain() string
	// draw renders the controls in r and returns their click targets.
	draw(a *App, b *Buf, r Rect, focused bool) []Hit
	// key handles a key while the body has focus; false passes it on
	// (Tab / Shift+Tab / Esc then move focus, globals run).
	key(a *App, k tea.KeyPressMsg) bool
	// mouse handles a click / wheel / drag start on one of its hits.
	mouse(a *App, m hitMsg)
	// controls is the number of Tab stops; focusCtl moves to one.
	controls() int
	focusCtl(a *App, i int)
	focused() int
	// hints is the hint bar while the body has focus.
	hints(a *App) []hint
	// keyHelp lists the tab's keys for the Keys panel and help.
	keyHelp() [][2]string
	// note is shown at the right of the title line ("" for none).
	note(a *App) string
}

// ctlBase holds the focused control index (embedded by every tab).
type ctlBase struct{ cur int }

func (c *ctlBase) focused() int           { return c.cur }
func (c *ctlBase) focusCtl(_ *App, i int) { c.cur = i }
func (c *ctlBase) note(*App) string       { return "" }
func (c *ctlBase) mouse(*App, hitMsg)     {}

func newTabs() []tabCtl {
	return []tabCtl{
		newTextTab(),
		newNumberTab(),
		newDateTab(),
		newSelectTab(),
		newComboTab(),
		newTagsTab(),
		newAreaTab(),
		newChoiceTab(),
		newCascadeTab(),
		newGridTab(),
		newMenusTab(),
		newPaletteTab(),
		newDialogsTab(),
		newChartsTab(),
		newKanbanTab(),
		newGanttTab(),
	}
}

// tabIndex returns the index of a tab id (−1 if unknown).
func tabIndex(id string) int {
	for i, t := range core.Tabs {
		if t.ID == id {
			return i
		}
	}
	return -1
}

// pasteTarget is implemented by tabs with text inputs.
type pasteTarget interface {
	paste(a *App, s string)
}
