// Command unic-relay is the in-memory Direct-to-S3 reference server for local
// development and integration testing. Production and self-hosted deployments
// backed by real S3/COS/R2/MinIO object storage live in github.com/UniClipboard/relay.
package main

import (
	"context"
	"log/slog"
	"net/http"
	"os"
	"os/signal"
	"strconv"
	"strings"
	"syscall"
	"time"

	"github.com/uniclipboard/unic/internal/relayserver"
)

func main() {
	logger := slog.New(slog.NewTextHandler(os.Stderr, nil))
	addr := strings.TrimSpace(os.Getenv("UNIC_RELAY_ADDR"))
	if addr == "" {
		addr = ":8080"
	}
	cfg := relayserver.Config{
		Origin:    strings.TrimSpace(os.Getenv("UNIC_RELAY_ORIGIN")),
		AccessKey: os.Getenv("UNIC_RELAY_ACCESS_KEY"),
	}
	if raw := strings.TrimSpace(os.Getenv("UNIC_RELAY_MAX_BYTES")); raw != "" {
		if n, err := strconv.ParseInt(raw, 10, 64); err == nil && n > 0 {
			cfg.MaxTransferBytes = n
		}
	}
	if raw := strings.TrimSpace(os.Getenv("UNIC_RELAY_ALLOWED_PUBKEYS")); raw != "" {
		cfg.AllowedPubKeys = map[string]struct{}{}
		for _, part := range strings.Split(raw, ",") {
			if pk := strings.TrimSpace(part); pk != "" {
				cfg.AllowedPubKeys[pk] = struct{}{}
			}
		}
	}

	srv := relayserver.New(cfg)
	httpSrv := &http.Server{
		Addr:              addr,
		Handler:           srv,
		ReadHeaderTimeout: 10 * time.Second,
	}

	stopSweep := make(chan struct{})
	go func() {
		t := time.NewTicker(time.Minute)
		defer t.Stop()
		for {
			select {
			case <-t.C:
				srv.Sweep()
			case <-stopSweep:
				return
			}
		}
	}()

	ctx, stop := signal.NotifyContext(context.Background(), os.Interrupt, syscall.SIGTERM)
	defer stop()
	go func() {
		<-ctx.Done()
		close(stopSweep)
		srv.Close()
		shutCtx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
		defer cancel()
		_ = httpSrv.Shutdown(shutCtx)
	}()

	logger.Info("unic-relay listening", "addr", addr, "has_origin", cfg.Origin != "", "has_access_key", cfg.AccessKey != "", "allowed_pubkeys", len(cfg.AllowedPubKeys))
	if err := httpSrv.ListenAndServe(); err != nil && err != http.ErrServerClosed {
		logger.Error("unic-relay exited", "error", err)
		os.Exit(1)
	}
}
