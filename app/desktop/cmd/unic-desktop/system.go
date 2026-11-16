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
	xdraw "golang.org/x/image/draw"

	"github.com/uniclipboard/unic/app/desktop/internal/appicons"
	"github.com/uniclipboard/unic/app/desktop/internal/i18n"
	api "github.com/uniclipboard/unic/internal/localapi"
)

var checkDesktopUpdates func()

var showDesktop = func() {}
var showQuickPanel = func() {}

func setupTray(language string) func(string) {
	menu := func(language string) *mygo.Menu {
		tr := func(text string) string { return i18n.Translate(language, text) }
		items := []*mygo.MenuItem{
			{Label: tr("打开主窗口"), Click: func(*mygo.MenuItem, *mygo.Window) { showDesktop() }},
			{Label: tr("快捷面板"), Click: func(*mygo.MenuItem, *mygo.Window) { showQuickPanel() }},
			mygo.Separator(), {Label: tr("退出"), Role: mygo.RoleQuit},
		}
		if checkDesktopUpdates != nil {
			items = append(items[:2], append([]*mygo.MenuItem{{Label: tr("检查更新"), Click: func(*mygo.MenuItem, *mygo.Window) { checkDesktopUpdates() }}}, items[2:]...)...)
		}
		return mygo.NewMenu(items)
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

// trayIcon preserves the v1 cat artwork and macOS menu-bar padding.
func trayIcon() []byte {
	src, err := png.Decode(bytes.NewReader(appicons.TrayPNG))
	if err != nil {
		panic(err)
	}
	size, art := 32, 32
	ink := color.NRGBA{R: 79, G: 124, B: 255, A: 255}
	if runtime.GOOS == "darwin" {
		size, art = 44, 34
		ink = color.NRGBA{A: 255}
	} else if runtime.GOOS == "windows" {
		ink = color.NRGBA{A: 255}
	}
	mask := image.NewAlpha(image.Rect(0, 0, size, size))
	offset := (size - art) / 2
	xdraw.CatmullRom.Scale(mask, image.Rect(offset, offset, offset+art, offset+art), src, src.Bounds(), xdraw.Src, nil)
	img := image.NewNRGBA(mask.Bounds())
	draw.DrawMask(img, img.Bounds(), image.NewUniform(ink), image.Point{}, mask, image.Point{}, draw.Src)
	var b bytes.Buffer
	if err := png.Encode(&b, img); err != nil {
		panic(err)
	}
	return b.Bytes()
}

var daemonLogger *slog.Logger

func logHostFailure() {
	if daemonLogger != nil {
		daemonLogger.Error("host.failed", "code", "unavailable")
	}
}
