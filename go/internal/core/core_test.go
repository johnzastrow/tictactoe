package core

import (
	"encoding/json"
	"errors"
	"testing"
)

func playAll(t *testing.T, cells ...int) *Game {
	t.Helper()
	g := NewGame()
	for _, c := range cells {
		if _, err := g.Play(c); err != nil {
			t.Fatalf("play %d: %v", c, err)
		}
	}
	return g
}

func TestTurnsAlternate(t *testing.T) {
	g := NewGame()
	if g.Turn() != X {
		t.Fatal("X must move first")
	}
	g.Play(4)
	if g.Turn() != O || g.Board()[4] != X {
		t.Fatalf("bad state after first move: %+v", g)
	}
}

func TestRejectsBadMoves(t *testing.T) {
	g := NewGame()
	var me *MoveError
	if _, err := g.Play(9); !errors.As(err, &me) || me.Error() != "cell 9 is out of range (0-8)" {
		t.Fatalf("got %v", err)
	}
	g.Play(0)
	if _, err := g.Play(0); err == nil || err.Error() != "cell 0 is already taken" {
		t.Fatalf("got %v", err)
	}
}

func TestWinDetectionAndGameOver(t *testing.T) {
	g := playAll(t, 0, 3, 1, 4, 2)
	s := g.Status()
	if s.Kind != Won || s.Winner != X || s.Line[0] != 0 || s.Line[2] != 2 {
		t.Fatalf("got %+v", s)
	}
	if _, err := g.Play(8); err == nil || err.Error() != "the game is already over" {
		t.Fatalf("got %v", err)
	}
	if len(g.LegalMoves()) != 0 {
		t.Fatal("no legal moves after a win")
	}
}

func TestDraw(t *testing.T) {
	if s := playAll(t, 0, 1, 2, 4, 3, 5, 7, 6, 8).Status(); s.Kind != Draw {
		t.Fatalf("got %+v", s)
	}
}

func TestAITakesWinAndBlocks(t *testing.T) {
	if c, _ := BestMove(playAll(t, 0, 3, 1, 4)); c != 2 {
		t.Fatalf("should win at 2, got %d", c)
	}
	if c, _ := BestMove(playAll(t, 0, 4, 1)); c != 2 {
		t.Fatalf("should block at 2, got %d", c)
	}
}

// TestAINeverLoses plays every possible opponent strategy against the AI,
// for both sides.
func TestAINeverLoses(t *testing.T) {
	games := 0
	var explore func(g *Game, ai Player)
	explore = func(g *Game, ai Player) {
		if s := g.Status(); s.Over() {
			games++
			if s.Kind == Won && s.Winner != ai {
				t.Fatalf("AI lost: %v", g.History())
			}
			return
		}
		if g.Turn() == ai {
			c, _ := BestMove(g)
			next := g.Clone()
			next.Play(c)
			explore(next, ai)
			return
		}
		for _, c := range g.LegalMoves() {
			next := g.Clone()
			next.Play(c)
			explore(next, ai)
		}
	}
	explore(NewGame(), X)
	explore(NewGame(), O)
	if games < 100 {
		t.Fatalf("only %d games explored", games)
	}
}

func TestJSONMatchesProtocol(t *testing.T) {
	s := Status{Kind: Won, Winner: O, Line: []int{2, 4, 6}}
	b, _ := json.Marshal(s)
	if string(b) != `{"kind":"won","winner":"O","line":[2,4,6]}` {
		t.Fatalf("got %s", b)
	}
	b, _ = json.Marshal(Status{Kind: InProgress})
	if string(b) != `{"kind":"in_progress"}` {
		t.Fatalf("got %s", b)
	}
	b, _ = json.Marshal(Board{X, None, O})
	if string(b) != `["X",null,"O",null,null,null,null,null,null]` {
		t.Fatalf("got %s", b)
	}
	var p Player
	if err := json.Unmarshal([]byte(`"Z"`), &p); err == nil {
		t.Fatal("accepted invalid player")
	}
}
