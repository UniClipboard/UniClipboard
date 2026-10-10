//go:build linux || windows

package main

import "github.com/wailsapp/wails/v3/pkg/application"

// republishTrayMenu makes a changed menu structure visible. On Windows the tray popup is a Win32Menu that SystemTray.SetMenu builds
// from the Menu; Menu.Update refreshes a different native menu (the one of Menu.impl) that the tray never shows, so labels and
// structure changed after startup stayed stale in the popup. On Linux the tray is a StatusNotifierItem whose
// dbusmenu layout is rebuilt only by SystemTray.SetMenu, which also runs on the main thread. Menu.Update would
// instead build and rebuild a separate GTK menu that the tray never shows, off the main thread, and free the
// submenu widget it then reuses (Gtk-CRITICAL on every refresh after the first).
func republishTrayMenu(tray *application.SystemTray, menu *application.Menu) {
	tray.SetMenu(menu)
}
