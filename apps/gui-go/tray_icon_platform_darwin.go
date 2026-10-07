package main

/*
#cgo CFLAGS: -x objective-c
#cgo LDFLAGS: -framework Cocoa
#import <Cocoa/Cocoa.h>

static int reduceMotionEnabled(void) {
	return [[NSWorkspace sharedWorkspace] accessibilityDisplayShouldReduceMotion] ? 1 : 0;
}
*/
import "C"

import (
	"image/color"

	"github.com/wailsapp/wails/v3/pkg/application"
)

// macOS gives the status item a square of the menu bar's thickness (22 pt; Wails sets it) and draws a template image in the menu bar's own
// colour from its alpha alone. The design's 18 pt artwork sits centred in that square. 44 px is the 2x pixel size, which every current Mac
// shows at most.
const (
	macTrayImagePx = 44
	macTrayArtPx   = 36 // 18 pt at 2x
)

func renderTrayFrame(v iconView) ([]byte, error) {
	return encodeIcon(iconSpec{base: v.base, dot: v.dot, earL: v.pose.left, earR: v.pose.right, size: macTrayImagePx, art: macTrayArtPx,
		pal: iconPalette{fg: color.NRGBA{A: 255}}}) // only alpha matters for a template image
}

// trayFrameVariant: a template image looks right on a light and a dark menu bar alike, so there is one variant.
func trayFrameVariant() string { return "" }

func applyTrayIcon(tray *application.SystemTray, data []byte) { tray.SetTemplateIcon(data) }

// watchSystemTheme: macOS recolours a template image itself, so nothing to watch.
func watchSystemTheme(*application.App, func()) {}

// systemReducesMotion reports System Settings > Accessibility > Display > Reduce motion.
func systemReducesMotion() bool { return C.reduceMotionEnabled() != 0 }
