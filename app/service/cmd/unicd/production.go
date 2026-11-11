//go:build !unic_e2e

package main

import (
	"context"
	"net/http"

	"github.com/uniclipboard/unic/app/service/internal/host"
)

func configureDaemon(*host.Options) error                                  { return nil }
func daemonTestRoutes(context.Context, *http.ServeMux, *host.Node, string) {}

// retireHold lets an e2e build keep a retiring generation observable. Production returns at once.
func retireHold() {}
