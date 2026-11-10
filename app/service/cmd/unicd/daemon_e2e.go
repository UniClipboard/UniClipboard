//go:build unic_e2e

package main

import (
	"context"
	"net/http"
	"os"
	"time"

	"github.com/uniclipboard/unic/app/service/internal/host"
	"github.com/uniclipboard/unic/internal/clipboardintegration"
	api "github.com/uniclipboard/unic/internal/localapi"
	"github.com/uniclipboard/unic/internal/testboard"
)

func configureDaemon(o *host.Options) error {
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
	}
	return nil
}
func daemonTestRoutes(ctx context.Context, mux *http.ServeMux, n *host.Node, sock string) {
	if os.Getenv("UNIC_E2E_DIAGNOSTICS") == "1" {
		testDiagnosticRoutes(mux, n)
	}
	if d := os.Getenv("UNIC_MOCK_CLIPBOARD_DIR"); d != "" {
		go func() { _ = clipboardintegration.RunRemote(ctx, api.NewClient(sock), testboard.Dir(d), nil) }()
	}
}
