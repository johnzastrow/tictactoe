package server

import (
	"context"
	"errors"
	"fmt"
	"log/slog"
	"net"
	"net/http"
	"time"

	"git.wharf.fluidgrid.org/jcz/tictactoe/go/internal/demo/core"
	"git.wharf.fluidgrid.org/jcz/tictactoe/go/internal/demo/store"
	"git.wharf.fluidgrid.org/jcz/tictactoe/go/internal/demo/web"
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
	h := StartHub(ctx, st, Info(), log)
	return NewHandler(h, web.Dist(), tls, log)
}

// CheckBind enforces §1: a non-loopback address needs TLS or an explicit
// --allow-plaintext.
func CheckBind(bind string, tls, allowPlaintext bool) error {
	host, _, err := net.SplitHostPort(bind)
	if err != nil {
		return fmt.Errorf("bad --bind %q: %w", bind, err)
	}
	ip := net.ParseIP(host)
	loopback := host == "localhost" || (ip != nil && ip.IsLoopback())
	if !loopback && !tls && !allowPlaintext {
		return fmt.Errorf("refusing to serve %s without TLS: pass --tls-cert/--tls-key, or --allow-plaintext on a trusted network (e.g. Tailscale)", bind)
	}
	return nil
}

// Serve runs until ctx is cancelled.
func Serve(ctx context.Context, cfg Config, log *slog.Logger) error {
	tls := cfg.TLSCert != "" && cfg.TLSKey != ""
	if err := CheckBind(cfg.Bind, tls, cfg.AllowPlaintext); err != nil {
		return err
	}
	st, err := store.Open(cfg.DB)
	if err != nil {
		return fmt.Errorf("opening %s: %w", cfg.DB, err)
	}
	defer st.Close()
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
		log.Info("demo (go) listening on https://" + cfg.Bind)
		err = srv.ServeTLS(ln, cfg.TLSCert, cfg.TLSKey)
	} else {
		log.Info("demo (go) listening on http://" + cfg.Bind)
		err = srv.Serve(ln)
	}
	if errors.Is(err, http.ErrServerClosed) {
		return nil
	}
	return err
}
