//go:build unic_e2e

package main

import (
	"context"
	"net/http"
	"os"
	"os/signal"
	"syscall"
	"time"

	"github.com/uniclipboard/unic/app/desktop/internal/api"
	"github.com/uniclipboard/unic/app/desktop/internal/board"
	"github.com/uniclipboard/unic/app/desktop/internal/config"
	"github.com/uniclipboard/unic/app/desktop/internal/host"
	"github.com/uniclipboard/unic/app/desktop/internal/paths"
	"github.com/uniclipboard/unic/internal/observability"
)

// This entry is excluded from release builds. It never starts mygo.App.
func runDaemon() error {
	dir, err := paths.Dir()
	if err != nil {
		return err
	}
	sock, err := paths.Socket()
	if err != nil {
		return err
	}
	cfgPath, err := paths.Config()
	if err != nil {
		return err
	}
	settings, err := config.Open(cfgPath, func(string, ...any) {})
	if err != nil {
		return err
	}
	luaDir, err := paths.LuaDir()
	if err != nil {
		return err
	}
	ctx, cancel := signal.NotifyContext(context.Background(), os.Interrupt, syscall.SIGTERM)
	defer cancel()
	obs := observability.Options{}
	if os.Getenv("UNIC_E2E_FAST_DIAGNOSTICS") == "1" {
		obs.MetricsInterval = 500 * time.Millisecond
		obs.LogInterval = 100 * time.Millisecond
	}
	options := host.Options{Observability: obs, Dir: dir, Settings: settings, Board: board.Dir(os.Getenv("UNIC_MOCK_CLIPBOARD_DIR")), LuaDir: luaDir}
	if os.Getenv("UNIC_E2E_DIAGNOSTICS") == "1" {
		tr, err := newE2ETransport()
		if err != nil {
			return err
		}
		options.Transport = tr
	}
	n, err := host.Open(ctx, options)
	if err != nil {
		return err
	}
	defer func() {
		closeCtx, done := context.WithTimeout(context.Background(), 10*time.Second)
		defer done()
		_ = n.Close(closeCtx)
	}()
	settings.OnChange = func(_, _ config.Config, err error) { n.ConfigChanged(err) }
	ln, err := api.Listen(sock)
	if err != nil {
		return err
	}
	defer func() { _ = ln.Close(); _ = os.Remove(sock) }()
	if err := n.Start(ctx); err != nil {
		return err
	}
	go func() { _ = settings.Watch(ctx) }()
	mux := http.NewServeMux()
	mux.Handle("/", api.Handler(n, settings))
	if os.Getenv("UNIC_E2E_DIAGNOSTICS") == "1" {
		testDiagnosticRoutes(mux, n)
	}
	server := &http.Server{Handler: mux, ReadHeaderTimeout: 5 * time.Second}
	go func() { <-ctx.Done(); _ = server.Close() }()
	err = server.Serve(ln)
	if err == http.ErrServerClosed {
		return nil
	}
	return err
}
