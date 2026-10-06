//go:build linux

package main

import (
	"errors"
	"fmt"
	"log"
	"os"
	"os/exec"
	"strconv"

	"github.com/UniClipboard/UniClipboard/apps/gui-go/internal/update"
)

// installPayload replaces the AppImage the process runs from. Only an AppImage updates itself in place (the Tauri
// contract): deb and rpm installs belong to the package manager, and a portable or source-tree binary has nothing
// to replace, so those report the install as unsupported instead of touching files they do not own. The daemon is
// stopped first, as on macOS, so the new application brings its own bundled daemon; a failed stop only warns.
func (h *HostService) installPayload(data []byte, _ string) error {
	appImage := os.Getenv("APPIMAGE")
	if appImage == "" {
		return fmt.Errorf("%w: only an AppImage installs updates itself (install kind %q)", update.ErrInstallUnsupported, installKind())
	}
	if err := stopDaemon(); err != nil {
		log.Printf("pre-update: daemon stop failed, proceeding: %v", err)
	}
	return update.InstallAppImage(data, appImage)
}

// relaunchAfterInstall starts the replaced AppImage: the mounted image of the old process is gone once it exits, so
// os.Executable() (inside that mount) cannot be used; the AppImage file itself is.
func (h *HostService) relaunchAfterInstall() error {
	appImage := os.Getenv("APPIMAGE")
	if appImage == "" {
		return errors.New("APPIMAGE is not set")
	}
	cmd := exec.Command(appImage)
	cmd.Env = append(os.Environ(), restartParentEnv+"="+strconv.Itoa(os.Getpid()))
	if err := cmd.Start(); err != nil {
		return err
	}
	go func() { _ = cmd.Wait() }()
	h.quit(true)
	return nil
}
