// Command ttt: tic-tac-toe in a rich TUI and a local web page, sharing one game.
//
//	ttt                 TUI + web server (default)
//	ttt -no-web         TUI only
//	ttt serve           web server only (headless)
package main

import (
	"context"
	"errors"
	"flag"
	"fmt"
	"log/slog"
	"net"
	"os"
	"os/signal"
	"strconv"
	"time"

	tea "charm.land/bubbletea/v2"

	"git.wharf.fluidgrid.org/johnzastrow/tictactoe/go/internal/core"
	"git.wharf.fluidgrid.org/johnzastrow/tictactoe/go/internal/hub"
	"git.wharf.fluidgrid.org/johnzastrow/tictactoe/go/internal/tui"
	"git.wharf.fluidgrid.org/johnzastrow/tictactoe/go/internal/web"
)

const aiDelay = 350 * time.Millisecond

func main() {
	if err := run(); err != nil {
		fmt.Fprintln(os.Stderr, "error:", err)
		os.Exit(1)
	}
}

func run() error {
	// Allow "ttt serve [flags]" as well as "ttt [flags]".
	args := os.Args[1:]
	serve := len(args) > 0 && args[0] == "serve"
	if serve {
		args = args[1:]
	}
	fs := flag.NewFlagSet("ttt", flag.ExitOnError)
	port := fs.Int("port", 8787, "port for the web interface (always bound to 127.0.0.1)")
	aiFlag := fs.String("ai", "o", "who the AI plays at startup: off, x, o")
	noWeb := fs.Bool("no-web", false, "run the TUI without starting the web interface")
	fs.Usage = func() {
		fmt.Fprintln(fs.Output(), "usage: ttt [serve] [flags]\n\n  serve    run only the web interface (Ctrl-C to stop)\n\nflags:")
		fs.PrintDefaults()
	}
	fs.Parse(args)

	ai, err := parseAI(*aiFlag)
	if err != nil {
		return err
	}
	if *port < 0 || *port > 65535 {
		return fmt.Errorf("invalid port %d", *port)
	}

	ctx, stop := signal.NotifyContext(context.Background(), os.Interrupt)
	defer stop()
	h := hub.Start(ctx, ai, aiDelay)

	if serve {
		ln, err := listen(*port)
		if err != nil {
			return err
		}
		slog.Info("serving tic-tac-toe at http://"+ln.Addr().String(), "stop", "Ctrl-C")
		return web.Serve(ctx, ln, h)
	}
	return runTUI(ctx, h, *port, !*noWeb)
}

func parseAI(s string) (core.Player, error) {
	switch s {
	case "off":
		return core.None, nil
	case "x", "X":
		return core.X, nil
	case "o", "O":
		return core.O, nil
	}
	return core.None, fmt.Errorf("invalid -ai %q (want off, x or o)", s)
}

// listen binds before anything else so "port in use" is a clean error.
func listen(port int) (net.Listener, error) {
	addr := net.JoinHostPort("127.0.0.1", strconv.Itoa(port))
	ln, err := net.Listen("tcp", addr)
	if err != nil {
		return nil, fmt.Errorf("could not listen on %s (in use? try -port <N>): %w", addr, err)
	}
	return ln, nil
}

func runTUI(ctx context.Context, h *hub.Hub, port int, withWeb bool) error {
	ctx, cancel := context.WithCancel(ctx)
	defer cancel()

	url := ""
	webDone := make(chan error, 1)
	if withWeb {
		ln, err := listen(port)
		if err != nil {
			return err
		}
		url = "http://" + ln.Addr().String()
		// No logging in TUI mode: log lines would corrupt the screen.
		slog.SetDefault(slog.New(slog.DiscardHandler))
		go func() { webDone <- web.Serve(ctx, ln, h) }()
	} else {
		close(webDone)
	}

	model, unsubscribe := tui.New(h, url)
	defer unsubscribe()
	_, err := tea.NewProgram(model, tea.WithContext(ctx)).Run()
	if errors.Is(err, tea.ErrProgramKilled) && ctx.Err() != nil {
		err = nil // Ctrl-C via signal, not a failure
	}

	cancel()
	select { // don't hang on exit if a browser keeps a socket open
	case <-webDone:
	case <-time.After(2 * time.Second):
	}
	return err
}
