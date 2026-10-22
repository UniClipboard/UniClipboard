package main

/*
#cgo CFLAGS: -x objective-c
#cgo LDFLAGS: -framework Cocoa
#import <Cocoa/Cocoa.h>

static void setRegularActivationPolicy(int regular) {
	[NSApp setActivationPolicy:regular ? NSApplicationActivationPolicyRegular : NSApplicationActivationPolicyAccessory];
}
*/
import "C"

import (
	"log"
	"os"
	"path/filepath"
	"sync"

	"github.com/wailsapp/wails/v3/pkg/application"
	"github.com/wailsapp/wails/v3/pkg/events"
)

// The app is a menu bar app with a Dock icon only while the main window is shown, as in the Tauri shell:
// the activation policy is Regular (Dock icon, app menu, Cmd+Tab) while the window is open and Accessory
// otherwise. Wails' dock service (HideAppIcon/ShowAppIcon) is not used: it always dispatch_syncs to the main
// queue, which traps when the caller is already on the main thread, and the tray icon's click handler runs
// there (systrayClickCallback -> showMainWindow). application.InvokeSync runs inline on the main thread and
// dispatches otherwise, so the switch is safe from every caller.

// setDockIconVisible switches between the Regular and the Accessory activation policy. The e2e build records
// the requested state and, in quiet mode, does not apply it (quiet mode keeps the app an accessory).
func (h *HostService) setDockIconVisible(visible bool) {
	if !applyDockIconVisible(visible) {
		return
	}
	regular := C.int(0)
	if visible {
		regular = 1
	}
	application.InvokeSync(func() { C.setRegularActivationPolicy(regular) })
	if !visible {
		return
	}
	// macOS re-adds the app to the Dock without re-reading the bundle icon after Accessory -> Regular, which
	// leaves the running indicator over a blank tile; setting the bundle's own icns again repaints it.
	if icon := bundleDockIcon(); len(icon) > 0 {
		h.app.SetIcon(icon)
	}
}

// bundleDockIcon is Contents/Resources/icon.icns of the running bundle (CFBundleIconFile, see
// packaging/macos/package.py), or nil outside a bundle. Wails hands the bytes to the main thread
// asynchronously, so they are read once and kept for the life of the process.
var bundleDockIcon = sync.OnceValue(func() []byte {
	exe, err := os.Executable()
	if err != nil {
		return nil
	}
	icon, err := os.ReadFile(filepath.Join(filepath.Dir(exe), "..", "Resources", "icon.icns"))
	if err != nil {
		log.Printf("dock icon refresh unavailable: %v", err)
		return nil
	}
	return icon
})

// routeDockReopen sends the reopen event (a Dock click, or launching the running app again from Finder or
// Spotlight) with no window visible to showMainWindow instead of Wails' default, which only shows windows
// that already exist and so neither builds the main window after a Silent start nor restores the Dock icon.
func (h *HostService) routeDockReopen() {
	h.app.Event.RegisterApplicationEventHook(events.Mac.ApplicationShouldHandleReopen, func(e *application.ApplicationEvent) {
		if w, ok := h.app.Window.GetByName("main"); ok && w.IsVisible() {
			return
		}
		e.Cancel()
		if !h.deferShowUntilReady() {
			h.showMainWindow()
		}
	})
}
