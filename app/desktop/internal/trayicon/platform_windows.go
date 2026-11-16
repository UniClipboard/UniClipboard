package trayicon

import (
	"context"
	"image/color"

	"golang.org/x/sys/windows"
)

var smallIcon = windows.NewLazySystemDLL("user32.dll").NewProc("GetSystemMetrics")

func platformSpec() spec {
	size := 16
	if px, _, _ := smallIcon.Call(49); px > 0 {
		size = int(px)
	} // SM_CXSMICON, as in v1.
	return spec{size: size, glyph: size, cat: size, foreground: color.NRGBA{R: 0x1b, G: 0x1b, B: 0x1b, A: 255}, template: true}
}

// Mygo updates a template when the Windows taskbar appearance changes.
func watchTheme(context.Context, func()) {}
