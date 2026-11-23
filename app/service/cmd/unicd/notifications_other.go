//go:build !darwin

package main

import (
	"context"
	"net/http"

	"github.com/uniclipboard/unic/app/service/internal/host"
	config "github.com/uniclipboard/unic/internal/localconfig"
	"github.com/uniclipboard/unic/internal/servicelifecycle"
)

func registerDesktopNotifications(context.Context, *http.ServeMux, *host.Node, *config.Manager, *servicelifecycle.Service) {
}
