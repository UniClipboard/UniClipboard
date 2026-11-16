package main

import (
	"context"
	"fmt"
	"log/slog"
	"time"

	"github.com/egoist/mygo"

	"github.com/uniclipboard/unic/app/desktop/internal/i18n"
	"github.com/uniclipboard/unic/app/desktop/internal/trayicon"
	api "github.com/uniclipboard/unic/internal/localapi"
	config "github.com/uniclipboard/unic/internal/localconfig"
)

var checkDesktopUpdates func()

var showDesktop = func() {}
var showQuickPanel = func() {}

func setupTray(ctx context.Context, cfg config.Desktop) func(config.Desktop) {
	language := cfg.Language
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

	raw, template, err := trayicon.Render(cfg.TrayIcon)
	if err != nil {
		logHostFailure()
		return func(config.Desktop) {}
	}
	tray, err := mygo.NewTray(mygo.TrayOptions{
		ID:             "unic-desktop",
		Icon:           raw,
		IconIsTemplate: template,
		ToolTip:        i18n.Translate(language, "跨设备剪贴板同步"),
		Menu:           menu(language),
	})
	if err != nil {
		// Some desktop sessions do not provide a tray host.
		logHostFailure()
		return func(config.Desktop) {}
	}
	icon := trayicon.New(tray.SetIcon)
	if err := icon.SetStyle(cfg.TrayIcon); err != nil {
		logHostFailure()
	}
	themeCtx, cancelTheme := context.WithCancel(ctx)
	refresh := func() {
		mygo.RunOnMain(func() {
			if themeCtx.Err() == nil {
				if err := icon.Refresh(); err != nil {
					logHostFailure()
				}
			}
		})
	}
	stopTheme := mygo.Theme.OnUpdated(refresh)
	trayicon.WatchTheme(themeCtx, refresh)
	mygo.App.OnWillQuit(func(*mygo.QuitEvent) { cancelTheme(); stopTheme(); tray.Destroy() })
	return func(current config.Desktop) {
		if current.Language != language {
			tray.SetMenu(menu(current.Language))
			tray.SetToolTip(i18n.Translate(current.Language, "跨设备剪贴板同步"))
			language = current.Language
		}
		if err := icon.SetStyle(current.TrayIcon); err != nil {
			logHostFailure()
		}
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

var daemonLogger *slog.Logger

func logHostFailure() {
	if daemonLogger != nil {
		daemonLogger.Error("host.failed", "code", "unavailable")
	}
}
