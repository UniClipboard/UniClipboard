//go:build !darwin

package main

import (
	"context"
	"net/http"

	"github.com/uniclipboard/unic/app/service/internal/host"
	config "github.com/uniclipboard/unic/internal/localconfig"
	"github.com/uniclipboard/unic/internal/servicelifecycle"
)

func registerDesktopNotifications(_ context.Context, mux *http.ServeMux, _ *host.Node, _ *config.Manager, _ *servicelifecycle.Service) {
	mux.HandleFunc("POST /v1/desktop/notifications/disable", func(w http.ResponseWriter, r *http.Request) {
		w.WriteHeader(http.StatusNoContent)
	})
}
