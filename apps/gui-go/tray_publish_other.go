//go:build !linux && !windows

package main

import "github.com/wailsapp/wails/v3/pkg/application"

// republishTrayMenu makes a changed menu structure visible; the native menu of macOS is rebuilt by Menu.Update.
func republishTrayMenu(_ *application.SystemTray, menu *application.Menu) {
	menu.Update()
}
