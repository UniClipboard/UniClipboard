package main

import (
	"context"
	"errors"

	"github.com/UniClipboard/UniClipboard/packages/desktop-host-go/settings"
)

var errSyncRejected = errors.New("sync settings update was rejected")

func (h *HostService) readSyncEnabled(ctx context.Context) (bool, error) {
	view, err := settings.Read(ctx, h.daemon())
	return view.Sync.SyncEnabled, err
}
