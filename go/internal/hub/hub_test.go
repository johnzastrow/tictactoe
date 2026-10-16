package hub

import (
	"context"
	"encoding/json"
	"errors"
	"strings"
	"testing"
	"time"

	"git.wharf.fluidgrid.org/jcz/tictactoe/go/internal/core"
)

func TestMovesUpdateStateAndScore(t *testing.T) {
	h := Start(t.Context(), core.None, core.Hard, 0)
	for _, c := range []int{0, 3, 1, 4, 2} {
		if err := h.Play(c); err != nil {
			t.Fatal(err)
		}
	}
	s := h.Snapshot()
	if s.Status.Kind != core.Won || s.Score.X != 1 {
		t.Fatalf("got %+v", s)
	}
	if err := h.Play(8); err == nil {
		t.Fatal("move after game over accepted")
	}
	h.NewGame()
	if s := h.Snapshot(); s.GameNo != 2 || s.Score.X != 1 || len(s.History) != 0 {
		t.Fatalf("got %+v", s)
	}
}

func TestAIRepliesAndBlocksHuman(t *testing.T) {
	h := Start(t.Context(), core.O, core.Hard, 30*time.Millisecond)
	updates, cancel := h.Subscribe(false)
	defer cancel()
	h.Play(0)
	if err := h.Play(1); !errors.Is(err, ErrAITurn) {
		t.Fatalf("got %v", err)
	}
	timeout := time.After(2 * time.Second)
	for {
		select {
		case s := <-updates:
			if len(s.History) == 2 {
				if s.Board[4] != core.O {
					t.Fatalf("perfect O answers a corner with the centre, got %v", s.Board)
				}
				return
			}
		case <-timeout:
			t.Fatal("AI never moved")
		}
	}
}

func TestWebClientCountAndShutdown(t *testing.T) {
	ctx, stop := context.WithCancel(context.Background())
	h := Start(ctx, core.None, core.Hard, 0)
	_, cancel := h.Subscribe(true)
	if n := h.Snapshot().WebClients; n != 1 {
		t.Fatalf("web clients = %d", n)
	}
	cancel()
	cancel() // idempotent
	if n := h.Snapshot().WebClients; n != 0 {
		t.Fatalf("web clients = %d", n)
	}
	stop()
	<-h.done
	if err := h.Play(0); !errors.Is(err, ErrClosed) {
		t.Fatalf("got %v", err)
	}
}

func TestStateJSONMatchesProtocol(t *testing.T) {
	h := Start(t.Context(), core.O, core.Hard, time.Hour)
	b, _ := json.Marshal(h.Snapshot())
	want := `{"board":[null,null,null,null,null,null,null,null,null],"turn":"X","status":{"kind":"in_progress"},"history":[],"score":{"x":0,"o":0,"draws":0},"ai":"O","difficulty":"hard","web_clients":0,"game_no":1}`
	if string(b) != want {
		t.Fatalf("got  %s\nwant %s", b, want)
	}
	h.SetAI(core.None)
	if b, _ := json.Marshal(h.Snapshot()); !strings.Contains(string(b), `"ai":null`) {
		t.Fatalf("got %s", b)
	}
}
