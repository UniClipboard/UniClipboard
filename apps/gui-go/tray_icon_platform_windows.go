package main

import (
	"image/color"

	"github.com/wailsapp/wails/v3/pkg/application"
	"github.com/wailsapp/wails/v3/pkg/events"
	"github.com/wailsapp/wails/v3/pkg/w32"
)

// Windows tray icons are not template images, so the image is drawn for the taskbar theme in use and drawn again when the system theme
// changes. Wails' own light/dark pair (SetIcon plus SetDarkModeIcon) is not used: on this Wails version a runtime SetIcon or SetDarkModeIcon
// makes both modes share the handle that was set last whenever they shared one before, so the two images never stay apart. The image is drawn
// at the small-icon size of the system DPI (16 px at 100%) so the shell does not rescale it; a different DPI on another monitor is not tracked.
var (
	winLight = iconPalette{fg: color.NRGBA{0x1B, 0x1B, 0x1B, 255}}
	winDark  = iconPalette{fg: color.NRGBA{0xFF, 0xFF, 0xFF, 255}}
)

func trayIconSpec() iconSpec { return trayIconSpecFor(designFromEnv()) }

func trayIconSpecFor(d trayDesign) iconSpec {
	size := 16
	if px := w32.GetSystemMetrics(w32.SM_CXSMICON); px > 0 {
		size = px
	}
	pal := winLight
	if w32.IsSystemCurrentlyDarkMode() {
		pal = winDark
	}
	return iconSpec{design: d, size: size, art: float64(size), pal: pal}
}

func applyTrayIcon(tray *application.SystemTray, data []byte) { tray.SetIcon(data) }

// watchSystemTheme asks for a repaint when the system theme changes (the same application event Wails' tray listens to).
func watchSystemTheme(app *application.App, changed func()) {
	app.Event.OnApplicationEvent(events.Windows.SystemThemeChanged, func(*application.ApplicationEvent) { changed() })
}
