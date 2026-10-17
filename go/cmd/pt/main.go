// Command pt: projects & tasks. One binary: the server, the seeder, user
// admin, and the TUI client (docs/pt/PLAN.md). Same CLI as the Rust pt.
package main

import (
	"context"
	"errors"
	"flag"
	"fmt"
	"log/slog"
	"os"
	"os/signal"
	"strings"

	"golang.org/x/term"

	"git.wharf.fluidgrid.org/jcz/tictactoe/go/internal/pt/auth"
	"git.wharf.fluidgrid.org/jcz/tictactoe/go/internal/pt/core"
	"git.wharf.fluidgrid.org/jcz/tictactoe/go/internal/pt/seed"
	"git.wharf.fluidgrid.org/jcz/tictactoe/go/internal/pt/server"
	"git.wharf.fluidgrid.org/jcz/tictactoe/go/internal/pt/store"
	"git.wharf.fluidgrid.org/jcz/tictactoe/go/internal/pt/tui"
)

const usage = `pt — projects & tasks: a TUI + web demo backed by one SQLite server

usage:
  pt serve  [--db pt.db] [--bind 127.0.0.1:8788] [--tls-cert F --tls-key F] [--allow-plaintext]
  pt seed   [--db pt.db] [--password PW] [--force]
  pt user   add <username> [--name "Full Name"] [--role admin|member|viewer] [--db pt.db]
  pt user   passwd <username> [--db pt.db]
  pt user   list [--db pt.db]
  pt tui    [--server http://127.0.0.1:8788] [--user NAME] [--ca-cert F] [--theme NAME]
  pt logout [--server URL]
`

func main() {
	if err := run(os.Args[1:]); err != nil {
		fmt.Fprintln(os.Stderr, "error:", err)
		os.Exit(1)
	}
}

// parse handles flags and positionals in any order (like clap).
func parse(fs *flag.FlagSet, args []string) ([]string, error) {
	var pos []string
	for {
		if err := fs.Parse(args); err != nil {
			return nil, err
		}
		if fs.NArg() == 0 {
			return pos, nil
		}
		pos = append(pos, fs.Arg(0))
		args = fs.Args()[1:]
	}
}

func readPassword(confirm bool) (string, error) {
	if p, ok := os.LookupEnv("PT_PASSWORD"); ok {
		return p, nil
	}
	prompt := func(label string) (string, error) {
		fmt.Fprint(os.Stderr, label)
		b, err := term.ReadPassword(int(os.Stdin.Fd()))
		fmt.Fprintln(os.Stderr)
		return string(b), err
	}
	p, err := prompt("Password: ")
	if err != nil {
		return "", err
	}
	if confirm {
		again, err := prompt("Again: ")
		if err != nil {
			return "", err
		}
		if again != p {
			return "", errors.New("passwords don't match")
		}
	}
	return p, nil
}

func checkPassword(p string) error {
	if n := len([]rune(p)); n < 8 || n > 256 {
		return errors.New("password must be 8–256 characters")
	}
	return nil
}

func run(args []string) error {
	if len(args) == 0 || args[0] == "-h" || args[0] == "--help" || args[0] == "help" {
		fmt.Print(usage)
		return nil
	}
	if args[0] == "-V" || args[0] == "--version" {
		fmt.Println("pt", core.Version)
		return nil
	}
	cmd, args := args[0], args[1:]
	fs := flag.NewFlagSet("pt "+cmd, flag.ContinueOnError)
	fs.Usage = func() { fmt.Fprint(fs.Output(), usage) }
	switch cmd {
	case "serve":
		db := fs.String("db", "pt.db", "SQLite database file")
		bind := fs.String("bind", "127.0.0.1:8788", "address to listen on")
		cert := fs.String("tls-cert", "", "TLS certificate (PEM)")
		key := fs.String("tls-key", "", "TLS key (PEM)")
		plain := fs.Bool("allow-plaintext", false, "allow a non-loopback bind without TLS (trusted networks such as Tailscale)")
		if _, err := parse(fs, args); err != nil {
			return err
		}
		if (*cert == "") != (*key == "") {
			return errors.New("--tls-cert and --tls-key go together")
		}
		log := slog.New(slog.NewTextHandler(os.Stderr, nil))
		ctx, stop := signal.NotifyContext(context.Background(), os.Interrupt)
		defer stop()
		return server.Serve(ctx, server.Config{DB: *db, Bind: *bind, TLSCert: *cert, TLSKey: *key, AllowPlaintext: *plain}, log)
	case "seed":
		db := fs.String("db", "pt.db", "SQLite database file")
		password := fs.String("password", "", "password for every demo user (default: random, printed once)")
		force := fs.Bool("force", false, "wipe existing data first")
		if _, err := parse(fs, args); err != nil {
			return err
		}
		st, err := store.Open(*db)
		if err != nil {
			return fmt.Errorf("opening %s: %w", *db, err)
		}
		defer st.Close()
		if *force {
			if err := seed.Wipe(st); err != nil {
				return err
			}
		}
		pw, generated := *password, false
		if pw == "" {
			pw = os.Getenv("PT_SEED_PASSWORD")
		}
		if pw == "" {
			pw, generated = auth.RandomPassword(12), true
		}
		if err := checkPassword(pw); err != nil {
			return err
		}
		r, err := seed.Seed(st, pw)
		if err != nil {
			return err
		}
		fmt.Printf("Seeded %s: %d users, %d projects, %d tasks.\n", *db, r.Users, r.Projects, r.Tasks)
		fmt.Println("Users: admin (admin), alice … heidi (members), victor (viewer).")
		if generated {
			fmt.Println("Password for every user:", pw)
		}
		return nil
	case "user":
		if len(args) == 0 {
			return errors.New("usage: pt user add|passwd|list")
		}
		sub, args := args[0], args[1:]
		db := fs.String("db", "pt.db", "SQLite database file")
		name := fs.String("name", "", "display name")
		role := fs.String("role", "member", "admin, member or viewer")
		pos, err := parse(fs, args)
		if err != nil {
			return err
		}
		st, err := store.Open(*db)
		if err != nil {
			return err
		}
		defer st.Close()
		switch sub {
		case "add":
			if len(pos) != 1 {
				return errors.New("usage: pt user add <username>")
			}
			r, ok := core.ParseRole(*role)
			if !ok {
				return errors.New("role must be admin, member or viewer")
			}
			username := strings.ToLower(strings.TrimSpace(pos[0]))
			if !core.ValidUsername(username) {
				return errors.New("username must be 2–32 of a–z, 0–9, '_', '.', '-' starting with a letter")
			}
			if u, err := st.UserByName(username); err != nil {
				return err
			} else if u != nil {
				return fmt.Errorf("user %s already exists", username)
			}
			pw, err := readPassword(true)
			if err != nil {
				return err
			}
			if err := checkPassword(pw); err != nil {
				return err
			}
			h, err := auth.HashPassword(pw)
			if err != nil {
				return err
			}
			display := *name
			if display == "" {
				display = username
			}
			id, err := st.AddUser(username, display, r, h)
			if err != nil {
				return err
			}
			fmt.Printf("Added %s (id %d, %s).\n", username, id, r)
			return nil
		case "passwd":
			if len(pos) != 1 {
				return errors.New("usage: pt user passwd <username>")
			}
			u, err := st.UserByName(pos[0])
			if err != nil {
				return err
			}
			if u == nil {
				return errors.New("no such user")
			}
			pw, err := readPassword(true)
			if err != nil {
				return err
			}
			if err := checkPassword(pw); err != nil {
				return err
			}
			h, err := auth.HashPassword(pw)
			if err != nil {
				return err
			}
			if err := st.SetPasswordHash(u.ID, h); err != nil {
				return err
			}
			if err := st.DeleteUserSessions(u.ID, ""); err != nil {
				return err
			}
			fmt.Printf("Password changed for %s; their sessions were ended.\n", u.Username)
			return nil
		case "list":
			us, err := st.ListUsers()
			if err != nil {
				return err
			}
			fmt.Printf("%-4s %-12s %-22s %-7s active\n", "id", "username", "name", "role")
			for _, u := range us {
				fmt.Printf("%-4d %-12s %-22s %-7s %t\n", u.ID, u.Username, u.DisplayName, u.Role, u.Active)
			}
			return nil
		}
		return fmt.Errorf("unknown user command %q (add, passwd, list)", sub)
	case "tui":
		srv := fs.String("server", "http://127.0.0.1:8788", "server URL")
		user := fs.String("user", "", "username")
		ca := fs.String("ca-cert", "", "extra CA certificate (PEM) to trust for https servers")
		theme := fs.String("theme", "", "colour theme: midnight, daylight, solarized-dark, gruvbox")
		colors := fs.String("colors", "", "force colours: 256 or truecolor (default: detect)")
		if _, err := parse(fs, args); err != nil {
			return err
		}
		return tui.Run(tui.Options{Server: *srv, User: *user, CACert: *ca, Theme: *theme, ForceColors: *colors})
	case "logout":
		srv := fs.String("server", "http://127.0.0.1:8788", "server URL")
		if _, err := parse(fs, args); err != nil {
			return err
		}
		return tui.ForgetSession(*srv)
	}
	fmt.Print(usage)
	return fmt.Errorf("unknown command %q", cmd)
}
