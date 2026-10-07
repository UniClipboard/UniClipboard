package main

import (
	"context"
	"image/color"
	"os/exec"
	"strings"
	"sync"
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

// linuxIconPalette is chosen once per process: asking the desktop starts a process, which must not happen for every animation frame.
var linuxIconPalette = sync.OnceValue(func() iconPalette {
	if gsetting("org.gnome.desktop.interface", "color-scheme") == "'prefer-light'" {
		return linuxDarkIcon
	}
	return linuxLightIcon
})

func renderTrayFrame(v iconView) ([]byte, error) {
	return encodeIcon(iconSpec{base: v.base, dot: v.dot, earL: v.pose.left, earR: v.pose.right, size: linuxTrayPx, art: linuxTrayPx, pal: linuxIconPalette()})
}

func trayFrameVariant() string { return "" }

func applyTrayIcon(tray *application.SystemTray, data []byte) { tray.SetIcon(data) }

// watchSystemTheme: the colour scheme is read once at the first frame; a later change needs a restart (a boundary, see the document).
func watchSystemTheme(*application.App, func()) {}

// systemReducesMotion reads GNOME's enable-animations; other desktops have no common setting, so motion stays on there.
func systemReducesMotion() bool {
	motionMu.Lock()
	defer motionMu.Unlock()
	if time.Since(motionRead) > motionCacheFor {
		motionOff, motionRead = gsetting("org.gnome.desktop.interface", "enable-animations") == "false", time.Now()
	}
	return motionOff
}

// The setting is cached for a few seconds so a burst of events does not start a gsettings process each.
const motionCacheFor = 5 * time.Second

var (
	motionMu   sync.Mutex
	motionOff  bool
	motionRead time.Time
)

func gsetting(schema, key string) string {
	ctx, cancel := context.WithTimeout(context.Background(), time.Second)
	defer cancel()
	out, err := exec.CommandContext(ctx, "gsettings", "get", schema, key).Output()
	if err != nil {
		return ""
	}
	return strings.TrimSpace(string(out))
}
