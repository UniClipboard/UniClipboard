//go:build unic_e2e

package main

import (
	"context"
	"net/http"
	"os"
	"strconv"
	"time"

	"github.com/uniclipboard/unic/app/service/internal/host"
	"github.com/uniclipboard/unic/internal/clipboardintegration"
	api "github.com/uniclipboard/unic/internal/localapi"
	"github.com/uniclipboard/unic/internal/testboard"
)

// retireHold keeps the listener open after shutdown starts so a test can meet a retiring generation.
func retireHold() {
	if ms, err := strconv.Atoi(os.Getenv("UNIC_E2E_RETIRE_HOLD_MS")); err == nil && ms > 0 {
		time.Sleep(time.Duration(ms) * time.Millisecond)
	}
}

func configureDaemon(o *host.Options) error {
	// A slow startup lets a test kill the launching CLI before readiness.
	if ms, err := strconv.Atoi(os.Getenv("UNIC_E2E_START_DELAY_MS")); err == nil && ms > 0 {
		time.Sleep(time.Duration(ms) * time.Millisecond)
	}
	if os.Getenv("UNIC_E2E_POSTHOG") == "true" {
		o.Observability.Token = func(context.Context) (string, error) { return "phc_unic_e2e_only", nil }
		o.Observability.AnalyticsInterval = 100 * time.Millisecond
	}
	if os.Getenv("UNIC_E2E_FAST_DIAGNOSTICS") == "1" {
		o.Observability.MetricsInterval = 500 * time.Millisecond
		o.Observability.LogInterval = 100 * time.Millisecond
	}
	if os.Getenv("UNIC_E2E_DIAGNOSTICS") == "1" {
		tr, e := newE2ETransport()
		if e != nil {
			return e
		}
		o.Transport = tr
		syncFixtureTransport = tr
		o.Observability.Now = syncFixtureNow
	}
	return nil
}
func daemonTestRoutes(ctx context.Context, mux *http.ServeMux, n *host.Node, sock string) {
	if os.Getenv("UNIC_E2E_DIAGNOSTICS") == "1" {
		testDiagnosticRoutes(mux, n)
		syncFixtureRoutes(mux)
	}
	if d := os.Getenv("UNIC_MOCK_CLIPBOARD_DIR"); d != "" {
		go func() { _ = clipboardintegration.RunRemote(ctx, api.NewClient(sock), testboard.Dir(d), nil) }()
	}
}
