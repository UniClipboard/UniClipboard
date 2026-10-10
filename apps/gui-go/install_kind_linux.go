//go:build linux

package main

import (
	"os"
	"os/exec"
	"strings"
	"sync"
)

var (
	installKindOnce  sync.Once
	installKindValue InstallKind
)

// platformInstallKind classifies the running installation like the Tauri shell's `get_install_kind`
// (crates/uc-tauri/src/commands/updater.rs): `appimage` when the AppImage runtime exported $APPIMAGE; otherwise
// an executable under /usr, /opt, /bin or /sbin is asked of the package databases (`dpkg-query -S`, `rpm -qf`) and
// is `deb`, `rpm`, or `unknown`; anything else (a portable or source tree) is `unknown`. The frontend sends deb
// and rpm to a "update with your package manager" dialog, so only AppImage installs itself in place. Cached.
func platformInstallKind() InstallKind {
	installKindOnce.Do(func() { installKindValue = detectLinuxInstallKind() })
	return installKindValue
}

func detectLinuxInstallKind() InstallKind {
	if os.Getenv("APPIMAGE") != "" {
		return InstallKindAppImage
	}
	exe, err := os.Executable()
	if err != nil {
		return InstallKindUnknown
	}
	managed := false
	for _, prefix := range []string{"/usr/", "/opt/", "/bin/", "/sbin/"} {
		if strings.HasPrefix(exe, prefix) {
			managed = true
		}
	}
	if !managed {
		return InstallKindUnknown
	}
	if exec.Command("dpkg-query", "-S", exe).Run() == nil {
		return InstallKindDeb
	}
	if exec.Command("rpm", "-qf", exe).Run() == nil {
		return InstallKindRPM
	}
	return InstallKindUnknown
}
