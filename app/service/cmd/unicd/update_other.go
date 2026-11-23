//go:build !darwin

package main

import (
	"net/http"

	config "github.com/uniclipboard/unic/internal/localconfig"
	"github.com/uniclipboard/unic/internal/servicelifecycle"
)

func registerDesktopUpdate(*http.ServeMux, *config.Manager, *servicelifecycle.Service) {}
