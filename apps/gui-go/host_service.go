package main

import (
	"context"
	"time"

	"github.com/UniClipboard/UniClipboard/apps/gui-go/internal/hostapi"
	"github.com/wailsapp/wails/v3/pkg/application"
)

// The exported methods of HostService are the host command contract: Wails binds each one and
// `wails3 generate bindings` derives the TypeScript calls and DTOs from the signatures (see
// scripts/gen-host-bindings.mjs). Anything that is not a command must therefore stay unexported.

// hostService wraps the host in a Wails service whose rejections keep their typed wire shape.
func hostService(h *HostService) application.Service {
	return application.NewServiceWithOptions(h, application.ServiceOptions{MarshalError: hostapi.Marshal})
}

// commandTimeout gives long-running update commands room; others fail fast.
func commandTimeout(name string) time.Duration {
	switch name {
	case "download_update", "install_update":
		return 30 * time.Minute
	}
	return 30 * time.Second
}

// commandContext bounds a command by its timeout. The Wails-supplied context already ends when the frontend cancels
// the call; the timeout keeps a hung daemon from holding the call forever. The name is the command's wire name
// (snake_case), kept for the timeout table, the e2e probe and the audit document.
func commandContext(ctx context.Context, name string) (context.Context, context.CancelFunc) {
	return context.WithTimeout(ctx, commandTimeout(name))
}
