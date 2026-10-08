package main

import (
	"context"
	"image/color"
	"os/exec"
	"strings"
	"time"

	"github.com/wailsapp/wails/v3/pkg/application"
)

// A StatusNotifierItem host draws the pixmap as it is: there is no template image and no way to ask the bar for its colour. The design's
// Linux environment is a dark bar with a light icon (Catppuccin #CDD6F4); a dark icon is used when the desktop reports a light colour
// scheme. A bar whose colour differs from the desktop scheme is a boundary this does not detect. 22 px is the design's Linux size.
const linuxTrayPx = 22

var (
	linuxLightIcon = iconPalette{fg: color.NRGBA{0xCD, 0xD6, 0xF4, 255}}
	linuxDarkIcon  = iconPalette{fg: color.NRGBA{0x1B, 0x1B, 0x1B, 255}}
)

func trayIconSpec() iconSpec { return trayIconSpecFor(designFromEnv()) }

func trayIconSpecFor(d trayDesign) iconSpec {
	pal := linuxLightIcon
	if gsetting("org.gnome.desktop.interface", "color-scheme") == "'prefer-light'" {
		pal = linuxDarkIcon
	}
	return iconSpec{design: d, size: linuxTrayPx, art: linuxTrayPx, pal: pal}
}

func applyTrayIcon(tray *application.SystemTray, data []byte) { tray.SetIcon(data) }

// watchSystemTheme: the colour scheme is read when the icon is shown; a later change needs a restart (a boundary, see the document).
func watchSystemTheme(*application.App, func()) {}

func gsetting(schema, key string) string {
	ctx, cancel := context.WithTimeout(context.Background(), time.Second)
	defer cancel()
	out, err := exec.CommandContext(ctx, "gsettings", "get", schema, key).Output()
	if err != nil {
		return ""
	}
	return strings.TrimSpace(string(out))
}
