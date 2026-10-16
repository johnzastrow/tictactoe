// Package core holds pure tic-tac-toe rules and an unbeatable AI.
//
// It does no I/O. Every front end drives the game through [Game.Play],
// which is the single place moves are validated.
package core

import (
	"encoding/json"
	"fmt"
	"slices"
)

// Cells are indexed 0..8, row-major:
//
//	0 | 1 | 2
//	3 | 4 | 5
//	6 | 7 | 8
const Cells = 9

// Lines lists every winning line.
var Lines = [8][3]int{
	{0, 1, 2}, {3, 4, 5}, {6, 7, 8},
	{0, 3, 6}, {1, 4, 7}, {2, 5, 8},
	{0, 4, 8}, {2, 4, 6},
}

// Player is a mark on the board. The zero value None means "empty" / "nobody".
type Player uint8

const (
	None Player = iota
	X
	O
)

func (p Player) Other() Player {
	switch p {
	case X:
		return O
	case O:
		return X
	}
	return None
}

func (p Player) String() string {
	switch p {
	case X:
		return "X"
	case O:
		return "O"
	}
	return ""
}

// MarshalJSON encodes X/O as strings and None as null, matching the protocol.
func (p Player) MarshalJSON() ([]byte, error) {
	if p == None {
		return []byte("null"), nil
	}
	return json.Marshal(p.String())
}

// UnmarshalJSON rejects anything except "X", "O" or null.
func (p *Player) UnmarshalJSON(b []byte) error {
	var s *string
	if err := json.Unmarshal(b, &s); err != nil {
		return err
	}
	switch {
	case s == nil:
		*p = None
	case *s == "X":
		*p = X
	case *s == "O":
		*p = O
	default:
		return fmt.Errorf("invalid player %q", *s)
	}
	return nil
}

type Board [Cells]Player

type StatusKind string

const (
	InProgress StatusKind = "in_progress"
	Won        StatusKind = "won"
	Draw       StatusKind = "draw"
)

// Status serializes as {"kind":"won","winner":"X","line":[0,4,8]} etc.
type Status struct {
	Kind   StatusKind `json:"kind"`
	Winner Player     `json:"winner,omitempty"`
	Line   []int      `json:"line,omitempty"`
}

func (s Status) Over() bool { return s.Kind != InProgress }

// MoveError explains why a move was rejected.
type MoveError struct {
	Reason string
	Cell   int
}

func (e *MoveError) Error() string {
	switch e.Reason {
	case "range":
		return fmt.Sprintf("cell %d is out of range (0-8)", e.Cell)
	case "occupied":
		return fmt.Sprintf("cell %d is already taken", e.Cell)
	}
	return "the game is already over"
}

type Game struct {
	board   Board
	turn    Player
	status  Status
	history []int
}

// NewGame returns a fresh game; X always moves first.
func NewGame() *Game {
	return &Game{turn: X, status: Status{Kind: InProgress}, history: make([]int, 0, Cells)}
}

func (g *Game) Board() Board   { return g.board }
func (g *Game) Turn() Player   { return g.turn }
func (g *Game) Status() Status { return g.status }
func (g *Game) History() []int { return slices.Clone(g.history) } // non-nil, so JSON is [] not null
func (g *Game) Plies() int     { return len(g.history) }

func (g *Game) Clone() *Game {
	c := *g
	c.history = g.History()
	return &c
}

func (g *Game) LegalMoves() []int {
	if g.status.Over() {
		return nil
	}
	var moves []int
	for i, p := range g.board {
		if p == None {
			moves = append(moves, i)
		}
	}
	return moves
}

// Play places the current player's mark. This is the only mutation path,
// so all validation lives here rather than in any UI.
func (g *Game) Play(cell int) (Status, error) {
	if g.status.Over() {
		return g.status, &MoveError{Reason: "over"}
	}
	if cell < 0 || cell >= Cells {
		return g.status, &MoveError{Reason: "range", Cell: cell}
	}
	if g.board[cell] != None {
		return g.status, &MoveError{Reason: "occupied", Cell: cell}
	}
	g.board[cell] = g.turn
	g.history = append(g.history, cell)
	g.status = Evaluate(&g.board)
	if !g.status.Over() {
		g.turn = g.turn.Other()
	}
	return g.status, nil
}

// Evaluate works out the status of an arbitrary board.
func Evaluate(b *Board) Status {
	for _, l := range Lines {
		if p := b[l[0]]; p != None && b[l[1]] == p && b[l[2]] == p {
			return Status{Kind: Won, Winner: p, Line: []int{l[0], l[1], l[2]}}
		}
	}
	for _, p := range b {
		if p == None {
			return Status{Kind: InProgress}
		}
	}
	return Status{Kind: Draw}
}

// BestMove returns the minimax-optimal cell for the player to move, or
// ok=false if the game is over. Faster wins and slower losses score higher;
// ties go to the lowest cell index so results are deterministic.
func BestMove(g *Game) (cell int, ok bool) {
	me := g.turn
	board := g.board
	best := 0
	for _, c := range g.LegalMoves() {
		board[c] = me
		score := -negamax(&board, me.Other(), 1)
		board[c] = None
		if !ok || score > best {
			best, cell, ok = score, c, true
		}
	}
	return cell, ok
}

// negamax scores the board from the perspective of toMove. The game tree is
// tiny (~550k nodes from empty), so exhaustive search needs no pruning.
func negamax(b *Board, toMove Player, depth int) int {
	switch Evaluate(b).Kind {
	case Won:
		return depth - 10 // the previous mover won: bad for toMove
	case Draw:
		return 0
	}
	best := -1 << 31
	for c := range Cells {
		if b[c] == None {
			b[c] = toMove
			best = max(best, -negamax(b, toMove.Other(), depth+1))
			b[c] = None
		}
	}
	return best
}

// Difficulty controls how hard the AI tries. It marshals as "easy" etc.
type Difficulty string

const (
	Easy   Difficulty = "easy"   // uniformly random legal moves
	Medium Difficulty = "medium" // perfect play, but a random move MediumMistakePercent% of the time
	Hard   Difficulty = "hard"   // perfect play (minimax); cannot be beaten
)

const MediumMistakePercent = 35

// ParseDifficulty accepts exactly "easy", "medium" or "hard".
func ParseDifficulty(s string) (Difficulty, error) {
	switch d := Difficulty(s); d {
	case Easy, Medium, Hard:
		return d, nil
	}
	return "", fmt.Errorf("invalid difficulty %q", s)
}

// UnmarshalJSON rejects unknown levels instead of silently accepting them.
func (d *Difficulty) UnmarshalJSON(b []byte) error {
	var s string
	if err := json.Unmarshal(b, &s); err != nil {
		return err
	}
	v, err := ParseDifficulty(s)
	if err == nil {
		*d = v
	}
	return err
}

func (d Difficulty) Next() Difficulty {
	switch d {
	case Easy:
		return Medium
	case Medium:
		return Hard
	}
	return Easy
}

// ChooseMove picks the AI's move at the given difficulty. pick(n) must
// return a uniformly random int in [0, n); passing it in keeps this package
// free of global state and lets tests use a deterministic source.
func ChooseMove(g *Game, d Difficulty, pick func(n int) int) (int, bool) {
	random := func() (int, bool) {
		moves := g.LegalMoves()
		if len(moves) == 0 {
			return 0, false
		}
		return moves[pick(len(moves))], true
	}
	switch {
	case d == Easy:
		return random()
	case d == Medium && pick(100) < MediumMistakePercent:
		return random()
	}
	return BestMove(g)
}
