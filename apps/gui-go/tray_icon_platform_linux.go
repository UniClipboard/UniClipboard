package main

import (
	"context"
	"os"
	"os/exec"
	"strings"
	"time"

	"github.com/wailsapp/wails/v3/pkg/application"
	"github.com/wailsapp/wails/v3/pkg/events"
)

// StatusNotifierItem hosts draw explicit colours at the design's 22 px size.
// KDE uses the Plasma style's panel colour, falling back to application colours;
// other desktops and unresolved KDE colours use the existing gsettings rule.
// System theme events and KDE KConfig polling repaint the currently shown style.
// Boundaries: non-KDE bars with independent colours (XFCE/waybar), third-party
// Plasma SVGs that ignore the colors file, and translucent panels over wallpaper.
const linuxTrayPx = 22

func trayIconSpec() iconSpec {
	env := linuxDesktopEnv{getenv: os.Getenv, readFile: readLinuxThemeFile}
	return iconSpec{size: linuxTrayPx, art: linuxTrayPx, pal: iconPalette{fg: linuxTrayPalette(env, gsetting)}}
}

func applyTrayIcon(tray *application.SystemTray, data []byte) { tray.SetIcon(data) }

// catTrayImage tints the cat's alpha with the same size and colour the glyph gets.
func catTrayImage() ([]byte, error) { return tintedCat(trayIconSpec()) }

// watchSystemTheme asks for a repaint when the portal colour scheme changes (Wails' SystemThemeChanged) and, on KDE, when plasmarc or
// kdeglobals changes; changed only calls the icon API, never the menu publish path, so an open menu stays open.
func watchSystemTheme(app *application.App, changed func()) {
	app.Event.OnApplicationEvent(events.Linux.SystemThemeChanged, func(*application.ApplicationEvent) { changed() })
	env := linuxDesktopEnv{getenv: os.Getenv, readFile: readLinuxThemeFile}
	if !env.isKDE() {
		return
	}
	poll := newKDEThemePoll(env, changed)
	// This goroutine lives for the GUI process lifetime, like the tray itself.
	go func() {
		ticker := time.NewTicker(time.Second)
		defer ticker.Stop()
		for range ticker.C {
			poll()
		}
	}()
}

func gsetting(schema, key string) string {
	ctx, cancel := context.WithTimeout(context.Background(), time.Second)
	defer cancel()
	out, err := exec.CommandContext(ctx, "gsettings", "get", schema, key).Output()
	if err != nil {
		return ""
	}
	return strings.TrimSpace(string(out))
}
