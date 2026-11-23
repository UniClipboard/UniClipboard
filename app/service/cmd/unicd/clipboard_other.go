//go:build !darwin

package main

import (
	"context"
	"net/http"

	"github.com/uniclipboard/unic/internal/servicelifecycle"
)

func registerDesktopClipboard(context.Context, *http.ServeMux, string, *servicelifecycle.Service) func() {
	return func() {}
}
