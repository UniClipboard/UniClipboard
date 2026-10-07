package main

import (
	"image/color"
	"strconv"
	"syscall"
	"unsafe"

	"github.com/wailsapp/wails/v3/pkg/application"
	"github.com/wailsapp/wails/v3/pkg/events"
	"github.com/wailsapp/wails/v3/pkg/w32"
)

// Windows tray icons are not template images, so the image is drawn for the taskbar theme in use (SystemUsesLightTheme) and drawn again when
// the system theme changes. Wails' own light/dark pair (SetIcon plus SetDarkModeIcon) is not used: on this Wails version a runtime SetIcon or
// SetDarkModeIcon makes both modes share the handle that was set last whenever they shared one before, so the two images never stay apart.
// The design allows colour here: the system accent for transfer and new content, a warning red for attention. The image is drawn at the
// small-icon size of the current DPI (16 px at 100%) so the shell does not rescale it.
var (
	winLight = iconPalette{fg: color.NRGBA{0x1B, 0x1B, 0x1B, 255}, coloured: true, accent: color.NRGBA{0x00, 0x67, 0xC0, 255}, alert: color.NRGBA{0xC4, 0x2B, 0x1C, 255}}
	winDark  = iconPalette{fg: color.NRGBA{0xFF, 0xFF, 0xFF, 255}, coloured: true, accent: color.NRGBA{0x4C, 0xC2, 0xFF, 255}, alert: color.NRGBA{0xE0, 0x43, 0x4F, 255}}
)

func smallIconSize() int {
	if size := w32.GetSystemMetrics(w32.SM_CXSMICON); size > 0 {
		return size
	}
	return 16
}

func renderTrayFrame(v iconView) ([]byte, error) {
	size := smallIconSize()
	pal := winLight
	if w32.IsSystemCurrentlyDarkMode() {
		pal = winDark
	}
	return encodeIcon(iconSpec{base: v.base, dot: v.dot, earL: v.pose.left, earR: v.pose.right, size: size, art: float64(size), pal: pal})
}

// trayFrameVariant is the theme and icon size a cached image was drawn for.
func trayFrameVariant() string {
	if w32.IsSystemCurrentlyDarkMode() {
		return "dark/" + strconv.Itoa(smallIconSize())
	}
	return "light/" + strconv.Itoa(smallIconSize())
}

func applyTrayIcon(tray *application.SystemTray, data []byte) { tray.SetIcon(data) }

// watchSystemTheme asks for a repaint when the system theme changes (the same application event Wails' tray listens to).
func watchSystemTheme(app *application.App, changed func()) {
	app.Event.OnApplicationEvent(events.Windows.SystemThemeChanged, func(*application.ApplicationEvent) { changed() })
}

// systemReducesMotion reports "Show animations in Windows" being off (SPI_GETCLIENTAREAANIMATION).
func systemReducesMotion() bool {
	const spiGetClientAreaAnimation = 0x1042
	var enabled int32
	proc := syscall.NewLazyDLL("user32.dll").NewProc("SystemParametersInfoW")
	if ret, _, _ := proc.Call(spiGetClientAreaAnimation, 0, uintptr(unsafe.Pointer(&enabled)), 0); ret == 0 {
		return false
	}
	return enabled == 0
}
