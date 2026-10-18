package tui

// Hit regions: frame records what it drew where, and a mouse event is
// resolved against exactly that frame (frame is pure, so Update can call it
// again). Update never has to recompute geometry, and drawing never mutates
// the model.

import tea "charm.land/bubbletea/v2"

type HitKind int

const (
	HitNone      HitKind = iota
	HitMenuTitle         // A = menu index
	HitSideTab           // A = tab index
	HitHint              // S = keystroke to synthesise
	HitToast             // A = toast index
	HitListItem          // overlay list row, A = item index
	HitButton            // dialog button, A = index
	HitField             // dialog / join form field, A = index
	HitDialog            // the dialog box itself
	HitBackdrop          // everything behind a modal
	HitCtl               // a tab control: S names the part, A/B are tab-defined
)

type Hit struct {
	R    Rect
	Kind HitKind
	A, B int
	S    string
}

// hitMsg is a mouse event resolved against the last frame.
type hitMsg struct {
	m    tea.Mouse
	ev   tea.MouseMsg
	hit  Hit
	ok   bool
	hits []Hit // all hits (drags look for drop targets)
}

func topHit(hits []Hit, x, y int) (Hit, bool) {
	for i := len(hits) - 1; i >= 0; i-- {
		if hits[i].R.Contains(x, y) {
			return hits[i], true
		}
	}
	return Hit{}, false
}

// ctlHit finds the topmost tab-control hit with part name s at x,y.
func ctlHit(hits []Hit, x, y int, s string) (Hit, bool) {
	for i := len(hits) - 1; i >= 0; i-- {
		h := hits[i]
		if h.Kind == HitCtl && h.S == s && h.R.Contains(x, y) {
			return h, true
		}
	}
	return Hit{}, false
}

func isClick(m hitMsg) bool {
	c, ok := m.ev.(tea.MouseClickMsg)
	return ok && c.Button == tea.MouseLeft
}

func isRightClick(m hitMsg) bool {
	c, ok := m.ev.(tea.MouseClickMsg)
	return ok && c.Button == tea.MouseRight
}

func isWheel(m hitMsg) (int, bool) {
	w, ok := m.ev.(tea.MouseWheelMsg)
	if !ok {
		return 0, false
	}
	switch w.Button {
	case tea.MouseWheelUp:
		return -1, true
	case tea.MouseWheelDown:
		return 1, true
	}
	return 0, false
}
