package main

import (
	"bytes"
	"context"
	"fmt"
	"image"
	"image/color"
	"image/draw"
	"image/png"
	"log/slog"
	"runtime"
	"time"

	"github.com/egoist/mygo"

	"github.com/uniclipboard/unic/app/desktop/internal/i18n"
	api "github.com/uniclipboard/unic/internal/localapi"
)

var showDesktop = func() {}
var showQuickPanel = func() {}

func setupTray(language string) func(string) {
	menu := func(language string) *mygo.Menu {
		tr := func(text string) string { return i18n.Translate(language, text) }
		return mygo.NewMenu([]*mygo.MenuItem{
			{Label: tr("打开主窗口"), Click: func(*mygo.MenuItem, *mygo.Window) { showDesktop() }},
			{Label: tr("快捷面板"), Click: func(*mygo.MenuItem, *mygo.Window) { showQuickPanel() }},
			mygo.Separator(), {Label: tr("退出"), Role: mygo.RoleQuit},
		})
	}

	tray, err := mygo.NewTray(mygo.TrayOptions{
		ID:             "unic-desktop",
		Icon:           trayIcon(),
		IconIsTemplate: true,
		ToolTip:        i18n.Translate(language, "跨设备剪贴板同步"),
		Menu:           menu(language),
	})
	if err != nil {
		// Some desktop sessions do not provide a tray host.
		logHostFailure()
		return func(string) {}
	}
	mygo.App.OnWillQuit(func(*mygo.QuitEvent) { tray.Destroy() })
	return func(language string) {
		tray.SetMenu(menu(language))
		tray.SetToolTip(i18n.Translate(language, "跨设备剪贴板同步"))
	}
}

// notifyPending raises a system notification when a device asks to join, so
// that the request is not missed while the window is closed. It is best
// effort: some systems refuse notifications from unsigned programs.
func notifyPending(ctx context.Context, sock string, language func() string) {
	if !mygo.NotificationsSupported() {
		return
	}
	c := api.NewClient(sock)
	for ctx.Err() == nil {
		_ = c.Events(ctx, func(ev api.Event) {
			if ev.Kind != "pending" {
				return
			}
			tr := func(text string) string { return i18n.Translate(language(), text) }
			body := fmt.Sprintf(tr("%s 请求同步剪贴板。打开 unic 批准或拒绝。"), ev.Peer)
			if ev.Info == "code" {
				body = fmt.Sprintf(tr("%s 输入了本机配对码。打开 unic，核对指纹后批准。"), ev.Peer)
			} else if ev.Info != "" {
				body = fmt.Sprintf(tr("%s 由 %s 介绍，请求加入。打开 unic 批准或拒绝。"), ev.Peer, ev.Info)
			}
			n := mygo.NewNotification(mygo.NotificationOptions{Title: tr("新设备"), Body: body})
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
