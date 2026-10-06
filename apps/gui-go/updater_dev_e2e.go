//go:build e2e

package main

import (
	"os"

	"github.com/UniClipboard/UniClipboard/apps/gui-go/internal/update"
)

type devUpdate struct {
	endpoints func(update.Channel) []string
	publicKey string
}

// devUpdateOverrides lets the e2e build point the updater at a local feed signed
// with a throwaway key, mirroring the Tauri shell's debug-only UC_UPDATE_* overrides.
func devUpdateOverrides() (devUpdate, bool) {
	endpoint, key := os.Getenv("UC_UPDATE_ENDPOINT"), os.Getenv("UC_UPDATE_PUBKEY")
	if endpoint == "" || key == "" {
		return devUpdate{}, false
	}
	return devUpdate{endpoints: func(update.Channel) []string { return []string{endpoint} }, publicKey: key}, true
}
