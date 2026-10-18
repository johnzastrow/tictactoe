// Command demo: a control gallery. One binary: the server, the reset tool
// and the TUI client (docs/demo/PLAN.md). Same CLI as the Rust demo.
package main

import (
	"context"
	"errors"
	"flag"
	"fmt"
	"log/slog"
	"os"
	"os/signal"

	"git.wharf.fluidgrid.org/jcz/tictactoe/go/internal/demo/core"
	"git.wharf.fluidgrid.org/jcz/tictactoe/go/internal/demo/server"
	"git.wharf.fluidgrid.org/jcz/tictactoe/go/internal/demo/store"
	"git.wharf.fluidgrid.org/jcz/tictactoe/go/internal/demo/tui"
)

const usage = `demo — a control gallery: every TUI/web control on its own tab, synced live

usage:
  demo serve [--db demo.db] [--bind 127.0.0.1:8790] [--tls-cert F --tls-key F] [--allow-plaintext]
  demo reset [--db demo.db]          restore every control to its default
  demo tui   [--server http://127.0.0.1:8790] [--name NAME] [--ca-cert F] [--theme NAME] [--colors 256|truecolor]
`

func main() {
	if err := run(os.Args[1:]); err != nil {
		fmt.Fprintln(os.Stderr, "error:", err)
		os.Exit(1)
	}
}

// parse handles flags and positionals in any order (like clap); demo's
// commands take no positionals, so any is an error.
func parse(fs *flag.FlagSet, args []string) error {
	if err := fs.Parse(args); err != nil {
		return err
	}
	if fs.NArg() > 0 {
		return fmt.Errorf("unexpected argument %q", fs.Arg(0))
	}
	return nil
}

func run(args []string) error {
	if len(args) == 0 || args[0] == "-h" || args[0] == "--help" || args[0] == "help" {
		fmt.Print(usage)
		return nil
	}
	if args[0] == "-V" || args[0] == "--version" {
		fmt.Println("demo", core.Version)
		return nil
	}
	cmd, args := args[0], args[1:]
	fs := flag.NewFlagSet("demo "+cmd, flag.ContinueOnError)
	fs.Usage = func() { fmt.Fprint(fs.Output(), usage) }
	switch cmd {
	case "serve":
		db := fs.String("db", "demo.db", "SQLite database file")
		bind := fs.String("bind", core.DefaultBind, "address to listen on")
		cert := fs.String("tls-cert", "", "TLS certificate (PEM)")
		key := fs.String("tls-key", "", "TLS key (PEM)")
		plain := fs.Bool("allow-plaintext", false, "allow a non-loopback bind without TLS (trusted networks such as Tailscale)")
		if err := parse(fs, args); err != nil {
			return err
		}
		if (*cert == "") != (*key == "") {
			return errors.New("--tls-cert and --tls-key go together")
		}
		log := slog.New(slog.NewTextHandler(os.Stderr, nil))
		ctx, stop := signal.NotifyContext(context.Background(), os.Interrupt)
		defer stop()
		return server.Serve(ctx, server.Config{DB: *db, Bind: *bind, TLSCert: *cert, TLSKey: *key, AllowPlaintext: *plain}, log)
	case "reset":
		db := fs.String("db", "demo.db", "SQLite database file")
		if err := parse(fs, args); err != nil {
			return err
		}
		st, err := store.Open(*db)
		if err != nil {
			return fmt.Errorf("opening %s: %w", *db, err)
		}
		defer st.Close()
		entries, err := st.Reset(nil, "", core.NowTS())
		if err != nil {
			return err
		}
		fmt.Printf("Reset %d controls in %s to their defaults.\n", len(entries), *db)
		fmt.Println("A running server keeps serving the old values to connected clients until they reconnect.")
		return nil
	case "tui":
		srv := fs.String("server", core.DefaultServer, "server URL")
		name := fs.String("name", "", "display name (without it, a join form is shown)")
		ca := fs.String("ca-cert", "", "extra CA certificate (PEM) to trust for https servers")
		theme := fs.String("theme", "", "colour theme: midnight, daylight, solarized-dark, gruvbox")
		colors := fs.String("colors", "", "force colours: 256 or truecolor (default: detect)")
		if err := parse(fs, args); err != nil {
			return err
		}
		if *colors != "" && *colors != "256" && *colors != "truecolor" {
			return errors.New("--colors must be 256 or truecolor")
		}
		return tui.Run(tui.Options{Server: *srv, Name: *name, CACert: *ca, Theme: *theme, Colors: *colors})
	}
	fmt.Print(usage)
	return fmt.Errorf("unknown command %q", cmd)
}
