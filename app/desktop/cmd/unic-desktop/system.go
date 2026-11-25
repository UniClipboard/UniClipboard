package main

import (
	"context"
	"fmt"
	"log/slog"

	"github.com/egoist/mygo"

	"github.com/uniclipboard/unic/app/desktop/internal/i18n"
	"github.com/uniclipboard/unic/app/desktop/internal/notifications"
	"github.com/uniclipboard/unic/app/desktop/internal/trayicon"
	config "github.com/uniclipboard/unic/internal/localconfig"
)

var checkDesktopUpdates func()

var showDesktop = func() {}
var openDesktopRoute = func(string) {}
var showQuickPanel = func() {}

type trayController struct {
	updateConfig  func(config.Desktop)
	updatePending func(notifications.PendingSummary)
}

func setupTray(ctx context.Context, cfg config.Desktop) *trayController {
	language := cfg.Language
	var pending notifications.PendingSummary

	buildTooltip := func(lang string, p notifications.PendingSummary) string {
		base := i18n.Translate(lang, "跨设备剪贴板同步")
		if p.Count > 0 {
			return fmt.Sprintf("%s · %s", base, fmt.Sprintf(i18n.Translate(lang, "待批准设备请求 (%d)"), p.Count))
		}
		return base
	}
	menu := func(lang string, p notifications.PendingSummary) *mygo.Menu {
		tr := func(text string) string { return i18n.Translate(lang, text) }
		var items []*mygo.MenuItem
		if p.Count > 0 {
			route := "devices"
			if p.FirstID != "" {
				route = "devices:" + p.FirstID
			}
			items = append(items,
				&mygo.MenuItem{
					Label: fmt.Sprintf(tr("待批准设备请求 (%d)…"), p.Count),
					Click: func(*mygo.MenuItem, *mygo.Window) { openDesktopRoute(route) },
				},
				mygo.Separator(),
			)
		}
		items = append(items,
			&mygo.MenuItem{Label: tr("打开主窗口"), Click: func(*mygo.MenuItem, *mygo.Window) { showDesktop() }},
			&mygo.MenuItem{Label: tr("快捷面板"), Click: func(*mygo.MenuItem, *mygo.Window) { showQuickPanel() }},
		)
		if checkDesktopUpdates != nil {
			items = append(items, &mygo.MenuItem{Label: tr("检查更新"), Click: func(*mygo.MenuItem, *mygo.Window) { checkDesktopUpdates() }})
		}
		items = append(items, mygo.Separator(), &mygo.MenuItem{Label: tr("退出整个应用"), Role: mygo.RoleQuit})
		return mygo.NewMenu(items)
	}

	noop := &trayController{
		updateConfig:  func(config.Desktop) {},
		updatePending: func(notifications.PendingSummary) {},
	}
	raw, template, err := trayicon.Render(cfg.TrayIcon)
	if err != nil {
		logHostFailure()
		return noop
	}
	tray, err := mygo.NewTray(mygo.TrayOptions{
		ID:             "unic-desktop",
		Icon:           raw,
		IconIsTemplate: template,
		ToolTip:        buildTooltip(language, pending),
		Menu:           menu(language, pending),
	})
	if err != nil {
		// Some desktop sessions do not provide a tray host.
		logHostFailure()
		return noop
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
	return &trayController{
		updateConfig: func(current config.Desktop) {
			if current.Language != language {
				language = current.Language
				tray.SetMenu(menu(language, pending))
				tray.SetToolTip(buildTooltip(language, pending))
			}
			if err := icon.SetStyle(current.TrayIcon); err != nil {
				logHostFailure()
			}
		},
		updatePending: func(current notifications.PendingSummary) {
			if current == pending {
				return
			}
			pending = current
			tray.SetMenu(menu(language, pending))
			tray.SetToolTip(buildTooltip(language, pending))
		},
	}
}

var daemonLogger *slog.Logger

func logHostFailure() {
	if daemonLogger != nil {
		daemonLogger.Error("host.failed", "code", "unavailable")
	}
}
