package tui

// Hit regions: View records what it drew where, and View.OnMouse turns a
// mouse event into a hitMsg against exactly that frame. Update never has to
// recompute geometry, and View never mutates the model.

import tea "charm.land/bubbletea/v2"

type HitKind int

const (
	HitNone HitKind = iota
	HitHeader
	HitColBorder
	HitCell
	HitGroup
	HitGutter
	HitGridBody
	HitPopupItem
	HitCalPrev
	HitCalNext
	HitCalDay
	HitTab
	HitMenuTitle
	HitMenuItem
	HitHint
	HitFilter
	HitField
	HitButton
	HitListItem
	HitCard
	HitBoardCol
	HitScope
	HitTLRow
	HitTLBar
	HitTLBarEnd
	HitTLArea
	HitTile
	HitToast
	HitPane
	HitDialog
	HitBackdrop
	HitDetailPane
)

type Hit struct {
	R    Rect
	Kind HitKind
	A, B int
	S    string
	// Pane identifies which grid/pane the hit belongs to (0 main, 1 detail).
	Pane int
}

// hitMsg is a mouse event resolved against the last frame.
type hitMsg struct {
	m    tea.Mouse
	ev   tea.MouseMsg
	hit  Hit
	ok   bool
	hits []Hit // all hits (drags need to find drop targets)
}

func topHit(hits []Hit, x, y int) (Hit, bool) {
	for i := len(hits) - 1; i >= 0; i-- {
		if hits[i].R.Contains(x, y) {
			return hits[i], true
		}
	}
	return Hit{}, false
}

// hitOfKind finds the topmost hit of a kind at x,y.
func hitOfKind(hits []Hit, x, y int, kinds ...HitKind) (Hit, bool) {
	for i := len(hits) - 1; i >= 0; i-- {
		if !hits[i].R.Contains(x, y) {
			continue
		}
		for _, k := range kinds {
			if hits[i].Kind == k {
				return hits[i], true
			}
		}
	}
	return Hit{}, false
}
