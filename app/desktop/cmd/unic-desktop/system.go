package main

import (
	"bytes"
	"context"
	"image"
	"image/color"
	"image/draw"
	"image/png"
	"log/slog"
	"runtime"
	"time"

	"github.com/egoist/mygo"

	api "github.com/uniclipboard/unic/internal/localapi"
)

var showDesktop = func() {}

func setupTray() {
	tray, err := mygo.NewTray(mygo.TrayOptions{
		ID:             "unic-desktop",
		Icon:           trayIcon(),
		IconIsTemplate: true,
		ToolTip:        "unic — clipboard sync",
		Menu: mygo.NewMenu([]*mygo.MenuItem{
			{Label: "Open unic…", Click: func(*mygo.MenuItem, *mygo.Window) { showDesktop() }},
			mygo.Separator(),
			{Role: mygo.RoleQuit},
		}),
	})
	if err != nil {
		// Some desktop sessions do not provide a tray host.
		logHostFailure()
		return
	}
	mygo.App.OnWillQuit(func(*mygo.QuitEvent) { tray.Destroy() })
}

// notifyPending raises a system notification when a device asks to join, so
// that the request is not missed while the window is closed. It is best
// effort: some systems refuse notifications from unsigned programs.
func notifyPending(ctx context.Context, sock string) {
	if !mygo.NotificationsSupported() {
		return
	}
	c := api.NewClient(sock)
	for ctx.Err() == nil {
		_ = c.Events(ctx, func(ev api.Event) {
			if ev.Kind != "pending" {
				return
			}
			body := ev.Peer + " asks to sync clipboards with this device. Open unic to approve or reject."
			if ev.Info == "code" {
				body = ev.Peer + " entered this device's pairing code. Open unic, compare the fingerprints and approve."
			} else if ev.Info != "" {
				body = ev.Peer + ", introduced by " + ev.Info + ", wants to join. Open unic to approve or reject."
			}
			n := mygo.NewNotification(mygo.NotificationOptions{Title: "New device", Body: body})
			if err := n.Show(); err != nil {
				logHostFailure()
			}
		})
		select {
		case <-ctx.Done():
		case <-time.After(2 * time.Second):
		}
	}
}

// trayIcon draws two overlapping sheets, black on transparent so macOS tints
// it for the menu bar; elsewhere it is blue to show on any taskbar.
func trayIcon() []byte {
	img := image.NewNRGBA(image.Rect(0, 0, 32, 32))
	ink := color.NRGBA{R: 79, G: 124, B: 255, A: 255}
	if runtime.GOOS == "darwin" {
		ink = color.NRGBA{A: 255}
	}
	for _, r := range []image.Rectangle{
		// back sheet
		image.Rect(4, 3, 20, 5), image.Rect(4, 5, 6, 21), image.Rect(18, 3, 20, 9),
		// front sheet
		image.Rect(11, 11, 28, 13), image.Rect(11, 28, 28, 30),
		image.Rect(11, 13, 13, 28), image.Rect(26, 13, 28, 28),
		image.Rect(15, 17, 24, 19), image.Rect(15, 22, 22, 24),
	} {
		draw.Draw(img, r, image.NewUniform(ink), image.Point{}, draw.Src)
	}
	var b bytes.Buffer
	_ = png.Encode(&b, img)
	return b.Bytes()
}

var daemonLogger *slog.Logger

func logHostFailure() {
	if daemonLogger != nil {
		daemonLogger.Error("host.failed", "code", "unavailable")
	}
}
