package main

import (
	"bytes"
	"image/png"
	"log"
	"sync"

	"github.com/wailsapp/wails/v3/pkg/application"
)

// trayIcon puts the tray's one image on the system tray: the "B solid cat" in the platform's colours and size
// (docs/architecture/gui-go-tray-icon.md). The image does not depend on any daemon state. It only changes when the platform's own look does
// (the taskbar theme on Windows), and then only the icon call is made, never the menu's publish path, so an open menu is not closed (t-0188).
type trayIcon struct {
	tray *application.SystemTray
	mu   sync.Mutex // render and apply happen together, so two theme changes cannot leave the older image on the tray
}

func newTrayIcon(tray *application.SystemTray) *trayIcon { return &trayIcon{tray: tray} }

// show renders the image for the platform's current look and hands it to the tray. Wails runs the icon call on the main thread and waits for
// it, so a caller that is itself on the main thread (an application event handler) must call this from its own goroutine.
func (i *trayIcon) show() {
	i.mu.Lock()
	defer i.mu.Unlock()
	data, err := encodeIcon(trayIconSpec())
	if err != nil {
		log.Printf("tray icon: %v", err)
		return
	}
	applyTrayIcon(i.tray, data)
}

// encodeIcon renders and PNG-encodes one image.
func encodeIcon(spec iconSpec) ([]byte, error) {
	var buf bytes.Buffer
	if err := png.Encode(&buf, renderIcon(spec)); err != nil {
		return nil, err
	}
	return buf.Bytes(), nil
}
