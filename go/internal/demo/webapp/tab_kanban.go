//go:build js && wasm

package webapp

// Tab 15 — Kanban: kanban.cards in four columns. Keys move the selection
// and the cards; the mouse drags a card to another column (or position).
// Each change commits the whole list.

import (
	"fmt"
	"strconv"
	"syscall/js"

	"git.wharf.fluidgrid.org/jcz/tictactoe/go/internal/demo/core"
)

type kanbanTab struct {
	tabBase
	board js.Value
	col   int // selected column
	idx   int // selected card within the column
	sig   string

	// drag
	pressID  int64
	pressX   float64
	pressY   float64
	dragging bool
	ghost    js.Value
	overCol  string
}

func newKanbanTab(b tabBase) Tab {
	t := &kanbanTab{tabBase: b}
	t.build = t.mount
	return t
}

func (t *kanbanTab) mount(area js.Value) {
	t.board = E("div", "kanban")
	t.addCtl(t.board, "kanban", "kanban.cards")
	Add(area, t.board)
	t.scope.On(t.board, "pointerdown", func(ev js.Value) {
		if ev.Get("button").Int() != 0 {
			return
		}
		t.Focus(0)
		tgt := ev.Get("target")
		if c := Closest(tgt, "[data-card]"); c.Truthy() {
			ev.Call("preventDefault")
			id, _ := strconv.ParseInt(DataOf(c, "card"), 10, 64)
			t.selectID(id)
			t.pressID, t.pressX, t.pressY = id, ev.Get("clientX").Float(), ev.Get("clientY").Float()
			t.board.Call("setPointerCapture", ev.Get("pointerId"))
		} else if col := Closest(tgt, "[data-col]"); col.Truthy() {
			t.col = colIndex(DataOf(col, "col"))
			t.idx = 0
		}
		t.a.Invalidate()
	})
	t.scope.On(t.board, "pointermove", func(ev js.Value) {
		if t.pressID == 0 {
			return
		}
		x, y := ev.Get("clientX").Float(), ev.Get("clientY").Float()
		if !t.dragging && (x-t.pressX)*(x-t.pressX)+(y-t.pressY)*(y-t.pressY) > 25 {
			t.startDrag()
		}
		if t.dragging {
			Style(t.ghost, "left", Px(x+8))
			Style(t.ghost, "top", Px(y+6))
			over := ""
			if col := Closest(doc.Call("elementFromPoint", x, y), "[data-col]"); col.Truthy() {
				over = DataOf(col, "col")
			}
			if over != t.overCol {
				t.overCol = over
				t.sig = ""
				t.a.Invalidate()
			}
		}
	})
	end := func(ev js.Value) {
		if t.dragging {
			x, y := ev.Get("clientX").Float(), ev.Get("clientY").Float()
			t.drop(x, y)
		}
		t.pressID, t.dragging, t.overCol = 0, false, ""
		if t.ghost.Truthy() {
			t.ghost.Call("remove")
			t.ghost = js.Undefined()
		}
		t.sig = ""
		t.a.Invalidate()
	}
	t.scope.On(t.board, "pointerup", end)
	t.scope.On(t.board, "pointercancel", end)
}

func colIndex(id string) int {
	for i, c := range core.KanbanColumns {
		if c.ID == id {
			return i
		}
	}
	return 0
}

// inCol lists a column's cards in list order.
func (t *kanbanTab) inCol(cards []core.Card, col int) []core.Card {
	var out []core.Card
	for _, c := range cards {
		if c.Col == core.KanbanColumns[col].ID {
			out = append(out, c)
		}
	}
	return out
}

func (t *kanbanTab) selected() (core.Card, bool) {
	list := t.inCol(t.a.v.Cards(), t.col)
	if t.idx >= 0 && t.idx < len(list) {
		return list[t.idx], true
	}
	return core.Card{}, false
}

func (t *kanbanTab) selectID(id int64) {
	cards := t.a.v.Cards()
	for ci := range core.KanbanColumns {
		for i, c := range t.inCol(cards, ci) {
			if c.ID == id {
				t.col, t.idx = ci, i
			}
		}
	}
}

func (t *kanbanTab) startDrag() {
	t.dragging = true
	card, ok := t.selected()
	if !ok {
		return
	}
	t.ghost = ET("div", "kcard ghost", core.SafeText(card.Title))
	Add(doc.Get("body"), t.ghost)
	t.sig = ""
}

// drop moves the dragged card to the column under the pointer, before the
// card under it (after it when in that card's lower half).
func (t *kanbanTab) drop(x, y float64) {
	el := doc.Call("elementFromPoint", x, y)
	col := Closest(el, "[data-col]")
	if !col.Truthy() {
		return
	}
	var before int64
	if c := Closest(el, "[data-card]"); c.Truthy() {
		id, _ := strconv.ParseInt(DataOf(c, "card"), 10, 64)
		r := c.Call("getBoundingClientRect")
		if y < r.Get("top").Float()+r.Get("height").Float()/2 {
			before = id
		} else {
			before = t.nextInCol(id)
		}
	}
	t.move(t.pressID, DataOf(col, "col"), before)
}

// nextInCol is the id of the card after id in its column (0 = none).
func (t *kanbanTab) nextInCol(id int64) int64 {
	cards := t.a.v.Cards()
	for i, c := range cards {
		if c.ID == id {
			for _, d := range cards[i+1:] {
				if d.Col == c.Col {
					return d.ID
				}
			}
		}
	}
	return 0
}

// move puts card id into col before card `before` (0 = at the end).
func (t *kanbanTab) move(id int64, col string, before int64) {
	if id == before {
		return
	}
	cards := t.a.v.Cards()
	var moving core.Card
	var rest []core.Card
	for _, c := range cards {
		if c.ID == id {
			moving = c
		} else {
			rest = append(rest, c)
		}
	}
	if moving.ID == 0 {
		return
	}
	moving.Col = col
	out := make([]core.Card, 0, len(cards))
	placed := false
	for _, c := range rest {
		if c.ID == before && !placed {
			out = append(out, moving)
			placed = true
		}
		out = append(out, c)
	}
	if !placed {
		out = append(out, moving)
	}
	if t.a.Commit("kanban.cards", out) == nil {
		t.selectID(id)
	}
}

func (t *kanbanTab) Key(k Key) bool {
	cards := t.a.v.Cards()
	n := len(t.inCol(cards, t.col))
	switch {
	case k.Is("ArrowLeft"):
		t.col = (t.col + 3) % 4
	case k.Is("ArrowRight"):
		t.col = (t.col + 1) % 4
	case k.Is("ArrowUp"):
		t.idx = max(0, t.idx-1)
	case k.Is("ArrowDown"):
		t.idx = min(max(0, n-1), t.idx+1)
	case k.Is("S-ArrowLeft", "<"), k.Is("S-ArrowRight", ">"):
		d := 1
		if k.Is("S-ArrowLeft", "<") {
			d = -1
		}
		c, ok := t.selected()
		to := t.col + d
		if ok && to >= 0 && to < 4 {
			t.move(c.ID, core.KanbanColumns[to].ID, 0)
		}
	case k.Is("Insert"):
		t.newCard()
	case k.Is("Enter"):
		t.rename()
	case k.Is("F8", "C-Delete"):
		t.deleteCard()
	default:
		return false
	}
	t.a.Invalidate()
	return true
}

func (t *kanbanTab) newCard() {
	col := core.KanbanColumns[t.col]
	t.a.OpenDialog(newPrompt(t.a, "New card in "+col.Label, "Title", "", "Add", 60, func(v string) error {
		cards := append([]core.Card(nil), t.a.v.Cards()...)
		var ids []int64
		for _, c := range cards {
			ids = append(ids, c.ID)
		}
		id := core.NextID(ids)
		if err := t.a.Commit("kanban.cards", append(cards, core.Card{ID: id, Title: v, Col: col.ID})); err != nil {
			return err
		}
		t.selectID(id)
		return nil
	}))
}

func (t *kanbanTab) rename() {
	c, ok := t.selected()
	if !ok {
		return
	}
	t.a.OpenDialog(newPrompt(t.a, "Rename card", "Title", c.Title, "Rename", 60, func(v string) error {
		cards := append([]core.Card(nil), t.a.v.Cards()...)
		for i := range cards {
			if cards[i].ID == c.ID {
				cards[i].Title = v
			}
		}
		return t.a.Commit("kanban.cards", cards)
	}))
}

func (t *kanbanTab) deleteCard() {
	c, ok := t.selected()
	if !ok {
		return
	}
	t.a.OpenDialog(newConfirm(t.a, "Delete card", "Delete \""+c.Title+"\"?", "Delete", func() {
		out := []core.Card{}
		for _, x := range t.a.v.Cards() {
			if x.ID != c.ID {
				out = append(out, x)
			}
		}
		_ = t.a.Commit("kanban.cards", out)
	}))
}

func (t *kanbanTab) Render() {
	cards := t.a.v.Cards()
	n := len(t.inCol(cards, t.col))
	if t.idx >= n {
		t.idx = max(0, n-1)
	}
	sel, _ := t.selected()
	sig := fmt.Sprint(cards, t.col, t.idx, t.isCur(0), t.dragging, t.overCol)
	if sig == t.sig {
		return
	}
	t.sig = sig
	Clear(t.board)
	focused := t.isCur(0)
	for ci, col := range core.KanbanColumns {
		list := t.inCol(cards, ci)
		cls := "kcol"
		if ci == t.col {
			cls += " cur"
		}
		if t.dragging && col.ID == t.overCol {
			cls += " over"
		}
		ce := E("div", cls)
		Data(ce, "col", col.ID)
		h := E("div", "kcolh")
		Add(h, ET("span", "", col.Label), ET("span", "bcount", strconv.Itoa(len(list))))
		Add(ce, h)
		for _, c := range list {
			cc := "kcard"
			if c.ID == sel.ID && ci == t.col {
				cc += " sel"
				if !focused {
					cc += " dim"
				}
				if t.dragging {
					cc += " dragging"
				}
			}
			el := E("div", cc)
			Data(el, "card", strconv.FormatInt(c.ID, 10))
			Add(el, ET("span", "kid", "#"+strconv.FormatInt(c.ID, 10)), ET("span", "ktitle", core.SafeText(c.Title)))
			Add(ce, el)
		}
		if len(list) == 0 {
			Add(ce, ET("div", "kempty muted", "no cards — Ins adds one"))
		}
		Add(t.board, ce)
	}
}

func (t *kanbanTab) Hints() []hint {
	return []hint{{"←→↑↓", "Select", ""}, {"⇧←", "Move left", "S-ArrowLeft"}, {"⇧→", "Move right", "S-ArrowRight"},
		{"Ins", "New", "Insert"}, {"Enter", "Rename", "Enter"}, {"F8", "Delete", "F8"}}
}
