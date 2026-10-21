package main

import (
	"bytes"
	"image/png"
	"log"
	"sync"

	"github.com/wailsapp/wails/v3/pkg/application"
)

// trayIconStyle is the user's tray icon preference (`general.trayIcon`, docs/architecture/gui-go-tray-icon.md).
type trayIconStyle string

const (
	trayIconGlyph trayIconStyle = "glyph" // the default: the "one glyph" design
	trayIconCat   trayIconStyle = "cat"   // the image shipped in v1.1.2 (icons/TRAY-CAT-SOURCE.md)
)

// trayIcon puts the tray's image on the system tray, in the platform's colours and size. The style comes from the daemon's settings; the
// image otherwise only changes when the platform's own look does (the taskbar theme on Windows), and then only the icon call is made, never
// the menu's publish path, so an open menu is not closed (t-0188).
type trayIcon struct {
	render func(trayIconStyle) ([]byte, error) // the image for a style in the platform's current look
	apply  func([]byte)                        // the native icon call
	mu     sync.Mutex                          // render and apply happen together, so a theme change and a style change cannot leave an older image
	shown  trayIconStyle                       // the style the tray last received; "" until the first image is applied
}

func newTrayIcon(tray *application.SystemTray) *trayIcon {
	return &trayIcon{render: trayIconImage, apply: func(data []byte) { applyTrayIcon(tray, data) }}
}

// show renders the style the tray shows (the default before any settings were read) for the platform's current look and hands it to the
// tray. Wails runs the icon call on the main thread and waits for it, so a caller that is itself on the main thread (an application event
// handler) must call this from its own goroutine.
func (i *trayIcon) show() {
	i.mu.Lock()
	defer i.mu.Unlock()
	style := i.shown
	if style == "" {
		style = trayIconGlyph
	}
	i.put(style)
}

// setStyle shows the daemon's tray icon preference. It runs after every settings read, so an unchanged style makes no native call. A
// failure leaves the shown style as it was, and the next read tries again; the saved preference is never touched.
func (i *trayIcon) setStyle(value string) {
	style := trayIconStyle(value)
	if style != trayIconGlyph && style != trayIconCat {
		log.Printf("tray icon: unknown style in settings, showing the default")
		style = trayIconGlyph
	}
	i.mu.Lock()
	defer i.mu.Unlock()
	if style == i.shown {
		return
	}
	i.put(style)
}

// put renders and applies one style; it runs with i.mu held.
func (i *trayIcon) put(style trayIconStyle) {
	data, err := i.render(style)
	if err != nil {
		log.Printf("tray icon: rendering the %s style failed: %v", style, err)
		return
	}
	i.apply(data)
	i.shown = style
}

// trayIconImage is the PNG for a style in the platform's current look.
func trayIconImage(style trayIconStyle) ([]byte, error) {
	if style == trayIconCat {
		return catTrayImage()
	}
	return encodeIcon(trayIconSpec())
}

// encodeIcon renders and PNG-encodes one image.
func encodeIcon(spec iconSpec) ([]byte, error) {
	var buf bytes.Buffer
	if err := png.Encode(&buf, renderIcon(spec)); err != nil {
		return nil, err
	}
	return buf.Bytes(), nil
}
