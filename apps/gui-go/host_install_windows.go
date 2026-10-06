//go:build windows

package main

import (
	"fmt"
	"log"
	"os"

	"github.com/UniClipboard/UniClipboard/apps/gui-go/internal/update"
	"github.com/UniClipboard/UniClipboard/packages/desktop-host-go/apppaths"
)

// installPayload runs the NSIS installer in place (Tauri contract, see update.NSISArgs). A portable build never
// updates itself: it has no installer to replace (`detect_install_kind` -> WindowsPortable). The live daemon image
// blocks the installer's file replacement, so unlike on macOS a daemon that cannot be stopped aborts the update.
func (h *HostService) installPayload(data []byte, version string) error {
	if apppaths.IsPortable() {
		return fmt.Errorf("the portable build does not update itself; download the new portable package instead")
	}
	if err := stopDaemon(); err != nil {
		log.Printf("pre-update: the daemon is still running, aborting: %v", err)
		return fmt.Errorf("pre-update: failed to stop the daemon: %w", err)
	}
	return update.InstallWindows(data, version, os.Args[1:])
}

// relaunchAfterInstall only quits: the installer (`/R`) starts the new version itself, so starting a copy here
// would race it for the single-instance lock. The daemon is already stopped.
func (h *HostService) relaunchAfterInstall() error {
	h.quit(true)
	return nil
}
