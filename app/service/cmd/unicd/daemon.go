package main

import (
	"context"
	"net/http"
	"os"
	"os/signal"
	"syscall"
	"time"

	"github.com/uniclipboard/unic/app/service/internal/server"

	"github.com/uniclipboard/unic/app/service/internal/host"
	config "github.com/uniclipboard/unic/internal/localconfig"
	paths "github.com/uniclipboard/unic/internal/localpaths"
)

// Production service entry. No graphical runtime is loaded.
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
	options := host.Options{Dir: dir, Settings: settings, LuaDir: luaDir}
	if err := configureDaemon(&options); err != nil {
		return err
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
	ln, err := server.Listen(sock)
	if err != nil {
		return err
	}
	defer func() { _ = ln.Close(); _ = os.Remove(sock) }()
	if err := n.Start(ctx); err != nil {
		return err
	}
	go func() { _ = settings.Watch(ctx) }()
	mux := http.NewServeMux()
	mux.Handle("/", server.Handler(n, settings))
	daemonTestRoutes(ctx, mux, n, sock)
	server := &http.Server{Handler: mux, ReadHeaderTimeout: 5 * time.Second, ReadTimeout: 15 * time.Second}
	go func() { <-ctx.Done(); _ = server.Close() }()
	err = server.Serve(ln)
	if err == http.ErrServerClosed {
		return nil
	}
	return err
}
