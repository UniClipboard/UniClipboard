//go:build darwin

package main

import (
	"log"
	"os"

	"github.com/UniClipboard/UniClipboard/apps/gui-go/internal/update"
)

// installPayload swaps the app bundle in place. The daemon is stopped first so the new app replaces it with its
// own bundled version; a failed stop only warns, since the kernel lets a running binary be overwritten.
func (h *HostService) installPayload(data []byte, _ string) error {
	exe, err := os.Executable()
	if err != nil {
		return err
	}
	bundle, err := update.BundleOf(exe)
	if err != nil {
		return err
	}
	if err := stopDaemon(); err != nil {
		log.Printf("pre-update: daemon stop failed, proceeding: %v", err)
	}
	return update.Install(data, bundle)
}

// relaunchAfterInstall starts the new bundle; the daemon stays down for the new GUI to replace.
func (h *HostService) relaunchAfterInstall() error { return h.restartGUI() }
