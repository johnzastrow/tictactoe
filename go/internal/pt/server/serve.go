package server

import (
	"context"
	"errors"
	"fmt"
	"log/slog"
	"net"
	"net/http"
	"time"

	"git.wharf.fluidgrid.org/jcz/tictactoe/go/internal/pt/core"
	"git.wharf.fluidgrid.org/jcz/tictactoe/go/internal/pt/store"
	"git.wharf.fluidgrid.org/jcz/tictactoe/go/internal/pt/web"
)

type Config struct {
	DB             string
	Bind           string
	TLSCert        string
	TLSKey         string
	AllowPlaintext bool
}

func Info() core.ServerInfo { return core.ServerInfo{Impl: "go", Version: core.Version} }

// App starts the hub for an open store and returns its HTTP handler; the
// hub stops when ctx is cancelled.
func App(ctx context.Context, st *store.Store, tls bool, log *slog.Logger) http.Handler {
	store.Logf = func(f string, a ...any) { log.Error(fmt.Sprintf(f, a...)) }
	h := StartHub(ctx, st, Info(), log)
	return NewHandler(h, web.Dist(), tls, log)
}

// Serve runs until ctx is cancelled.
func Serve(ctx context.Context, cfg Config, log *slog.Logger) error {
	tls := cfg.TLSCert != "" && cfg.TLSKey != ""
	host, _, err := net.SplitHostPort(cfg.Bind)
	if err != nil {
		return fmt.Errorf("bad --bind %q: %w", cfg.Bind, err)
	}
	ip := net.ParseIP(host)
	loopback := host == "localhost" || (ip != nil && ip.IsLoopback())
	if !loopback && !tls && !cfg.AllowPlaintext {
		return fmt.Errorf("refusing to serve %s without TLS: pass --tls-cert/--tls-key, or --allow-plaintext on a trusted network (e.g. Tailscale)", cfg.Bind)
	}
	st, err := store.Open(cfg.DB)
	if err != nil {
		return fmt.Errorf("opening %s: %w", cfg.DB, err)
	}
	defer st.Close()
	if empty, _ := st.IsEmpty(); empty {
		log.Warn("the database has no users: run `pt seed` or `pt user add <name> --role admin`")
	}
	// Bind first so "address in use" is a clean error.
	ln, err := net.Listen("tcp", cfg.Bind)
	if err != nil {
		return fmt.Errorf("binding %s: %w", cfg.Bind, err)
	}
	hctx, stop := context.WithCancel(ctx)
	defer stop()
	srv := &http.Server{Handler: App(hctx, st, tls, log), ReadHeaderTimeout: 10 * time.Second}
	go func() {
		<-ctx.Done()
		sctx, c := context.WithTimeout(context.Background(), 3*time.Second)
		defer c()
		_ = srv.Shutdown(sctx)
	}()
	if tls {
		log.Info("pt (go) listening on https://" + cfg.Bind)
		err = srv.ServeTLS(ln, cfg.TLSCert, cfg.TLSKey)
	} else {
		log.Info("pt (go) listening on http://" + cfg.Bind)
		err = srv.Serve(ln)
	}
	if errors.Is(err, http.ErrServerClosed) {
		return nil
	}
	return err
}
