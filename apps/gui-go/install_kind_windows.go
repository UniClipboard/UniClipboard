//go:build windows

package main

import "github.com/UniClipboard/UniClipboard/packages/desktop-host-go/apppaths"

// platformInstallKind separates the installed copy (the NSIS installer updates it in place) from the portable
// zip, whose folder the installer would not update: the page routes `windowsportable` to a "download the new
// portable zip" dialog instead of self-installing. Portable mode is the one apppaths decides everywhere else.
func platformInstallKind() InstallKind {
	if apppaths.IsPortable() {
		return InstallKindWindowsPortable
	}
	return InstallKindWindows
}
