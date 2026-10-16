// Package hub owns the game state.
//
// A single goroutine applies every command in order and pushes a fresh
// StateView to subscribers. Because nothing else touches the game, the TUI
// and any number of browsers can't race each other, and no mutex is needed.
package hub

import (
	"context"
	"errors"
	"math/rand/v2"
	"slices"
	"time"

	"git.wharf.fluidgrid.org/jcz/tictactoe/go/internal/core"
)

type Score struct {
	X     int `json:"x"`
	O     int `json:"o"`
	Draws int `json:"draws"`
}

// StateView is everything a front end needs to render; it is also the wire format.
type StateView struct {
	Board      core.Board      `json:"board"`
	Turn       core.Player     `json:"turn"`
	Status     core.Status     `json:"status"`
	History    []int           `json:"history"`
	Score      Score           `json:"score"`
	AI         core.Player     `json:"ai"`
	Difficulty core.Difficulty `json:"difficulty"`
	WebClients int             `json:"web_clients"`
	GameNo     int             `json:"game_no"`
}

var (
	ErrAITurn = errors.New("it's the AI's turn")
	ErrClosed = errors.New("game server has stopped")
)

type command func(h *state)

// Hub is a handle to the running game goroutine. Safe for concurrent use.
type Hub struct {
	cmds chan command
	done chan struct{}
}

type state struct {
	game    *core.Game
	score   Score
	ai      core.Player
	level   core.Difficulty
	web     int
	gameNo  int
	aiDelay time.Duration
	subs    map[int]chan StateView
	nextSub int
	last    StateView
	hub     *Hub
}

// Start launches the hub goroutine; it stops when ctx is cancelled.
// aiDelay is a cosmetic pause before the AI moves.
func Start(ctx context.Context, ai core.Player, level core.Difficulty, aiDelay time.Duration) *Hub {
	h := &Hub{cmds: make(chan command, 256), done: make(chan struct{})}
	s := &state{
		game: core.NewGame(), ai: ai, level: level, gameNo: 1, aiDelay: aiDelay,
		subs: map[int]chan StateView{}, hub: h,
	}
	s.last = s.view()
	go func() {
		defer close(h.done)
		s.scheduleAI()
		for {
			select {
			case <-ctx.Done():
				for _, ch := range s.subs {
					close(ch)
				}
				return
			case cmd := <-h.cmds:
				cmd(s)
				s.publish()
			}
		}
	}()
	return h
}

// do runs fn on the hub goroutine and waits for it to finish.
func (h *Hub) do(fn func(s *state)) error {
	finished := make(chan struct{})
	select {
	case h.cmds <- func(s *state) { fn(s); close(finished) }:
	case <-h.done:
		return ErrClosed
	}
	select {
	case <-finished:
		return nil
	case <-h.done:
		return ErrClosed
	}
}

// Play places the current player's mark in cell.
func (h *Hub) Play(cell int) error {
	var err error
	if cerr := h.do(func(s *state) { err = s.humanMove(cell) }); cerr != nil {
		return cerr
	}
	return err
}

func (h *Hub) NewGame() error {
	return h.do(func(s *state) {
		s.game = core.NewGame()
		s.gameNo++
		s.scheduleAI()
	})
}

// SetAI chooses who the AI plays; core.None means two humans.
func (h *Hub) SetAI(p core.Player) error {
	return h.do(func(s *state) {
		s.ai = p
		s.scheduleAI()
	})
}

// SetDifficulty changes how hard the AI plays from its next move on.
func (h *Hub) SetDifficulty(d core.Difficulty) error {
	return h.do(func(s *state) { s.level = d })
}

func (h *Hub) Snapshot() StateView {
	var v StateView
	h.do(func(s *state) { v = s.view() })
	return v
}

// Subscribe returns a channel that always holds the latest state (slow
// readers skip intermediate states rather than blocking the hub), plus a
// cancel func. web=true counts the subscriber as a connected browser.
func (h *Hub) Subscribe(web bool) (<-chan StateView, func()) {
	ch := make(chan StateView, 1)
	id := -1
	h.do(func(s *state) {
		id = s.nextSub
		s.nextSub++
		s.subs[id] = ch
		if web {
			s.web++
		}
		ch <- s.view()
	})
	cancel := func() {
		h.do(func(s *state) {
			if _, ok := s.subs[id]; ok {
				delete(s.subs, id)
				if web {
					s.web--
				}
			}
		})
	}
	return ch, cancel
}

func (s *state) humanMove(cell int) error {
	if !s.game.Status().Over() && s.ai == s.game.Turn() {
		return ErrAITurn
	}
	if err := s.apply(cell); err != nil {
		return err
	}
	s.scheduleAI()
	return nil
}

func (s *state) apply(cell int) error {
	st, err := s.game.Play(cell)
	if err != nil {
		return err
	}
	switch {
	case st.Kind == core.Won && st.Winner == core.X:
		s.score.X++
	case st.Kind == core.Won && st.Winner == core.O:
		s.score.O++
	case st.Kind == core.Draw:
		s.score.Draws++
	}
	return nil
}

func (s *state) scheduleAI() {
	if s.game.Status().Over() || s.ai == core.None || s.ai != s.game.Turn() {
		return
	}
	gameNo, ply := s.gameNo, s.game.Plies()
	h := s.hub
	time.AfterFunc(s.aiDelay, func() {
		select {
		case h.cmds <- func(s *state) {
			// Stale if a new game started or settings changed meanwhile.
			if gameNo != s.gameNo || ply != s.game.Plies() || s.ai != s.game.Turn() {
				return
			}
			if c, ok := core.ChooseMove(s.game, s.level, rand.IntN); ok {
				s.apply(c)
			}
		}:
		case <-h.done:
		}
	})
}

func (s *state) view() StateView {
	return StateView{
		Board: s.game.Board(), Turn: s.game.Turn(), Status: s.game.Status(),
		History: s.game.History(), Score: s.score, AI: s.ai, Difficulty: s.level,
		WebClients: s.web, GameNo: s.gameNo,
	}
}

// publish pushes the new state to every subscriber if anything changed.
func (s *state) publish() {
	v := s.view()
	if equal(v, s.last) {
		return
	}
	s.last = v
	for _, ch := range s.subs {
		// Replace any unread state with the newest one; never block.
		select {
		case <-ch:
		default:
		}
		ch <- v
	}
}

func equal(a, b StateView) bool {
	return a.Board == b.Board && a.Turn == b.Turn && a.Status.Kind == b.Status.Kind &&
		slices.Equal(a.History, b.History) && a.Score == b.Score && a.AI == b.AI && a.Difficulty == b.Difficulty &&
		a.WebClients == b.WebClients && a.GameNo == b.GameNo
}
