// Package desktopdemo launches isolated previews using shared production views.
package desktopdemo

import (
	"errors"
	"flag"
	"fmt"
	"slices"

	"github.com/egoist/mygo"
	"github.com/egoist/mygo/ui"

	"github.com/uniclipboard/unic/app/desktop/internal/desktopdemo/fixtures"
	"github.com/uniclipboard/unic/app/desktop/internal/desktopui"
	"github.com/uniclipboard/unic/app/desktop/internal/i18n"
	"github.com/uniclipboard/unic/app/desktop/internal/windowchrome"
)

// Run parses all preview options before starting mygo. It never opens a service.
func Run(args []string) error {
	flags := flag.NewFlagSet("unic-desktop-demo", flag.ContinueOnError)
	page := flags.String("page", "history", "history, devices, settings, quick-panel")
	theme := flags.String("theme", "system", "system, light, dark")
	quickState := flags.String("quick-state", "history", "history, disconnected, empty")
	if err := flags.Parse(args); err != nil {
		if errors.Is(err, flag.ErrHelp) {
			return nil
		}
		return err
	}
	if flags.NArg() != 0 {
		return fmt.Errorf("unexpected demo argument: %s", flags.Arg(0))
	}
	if !slices.Contains([]string{"history", "devices", "settings", "quick-panel"}, *page) {
		return fmt.Errorf("unknown demo page: %s", *page)
	}
	if !slices.Contains([]string{"system", "light", "dark"}, *theme) {
		return fmt.Errorf("unknown demo theme: %s", *theme)
	}
	if !slices.Contains([]string{"history", "disconnected", "empty"}, *quickState) {
		return fmt.Errorf("unknown quick panel state: %s", *quickState)
	}
	if *page == "quick-panel" {
		quick := desktopui.NewQuickPreview(fixtures.Quick(*theme, *quickState))
		mygo.App.SetName("unic Quick Panel Demo")
		mygo.App.WhenReady(func() { quick.Open(mygo.App.Quit) })
		mygo.App.OnWindowAllClosed(func() {})
		return mygo.App.Run()
	}
	screen := desktopui.NewPreview(fixtures.Desktop(*page, *theme))
	mygo.App.SetName("unic UI Demo")
	mygo.App.WhenReady(func() {
		var win *mygo.Window
		view := func(c *ui.Context) {
			windowchrome.View(c, windowchrome.Actions{
				Minimize: func() { win.Minimize() },
				ToggleMaximize: func() {
					if win.IsMaximized() {
						win.Unmaximize()
					} else {
						win.Maximize()
					}
					win.Invalidate()
				},
				IsMaximized: func() bool { return win != nil && win.IsMaximized() },
				Close:       func() { win.Close() },
			}, "zh-CN", func() { screen.View(c) })
		}
		opts := mygo.WindowOptions{Title: "unic · " + i18n.Translate("zh-CN", "界面演示"), Width: 1100, Height: 720, MinWidth: 600, MinHeight: 480, Content: ui.View(view)}
		windowchrome.Configure(&opts, "", false)
		win = mygo.NewWindow(opts)
		var quick *desktopui.QuickPreview
		screen.SetQuickPanelOpener(func(theme string) {
			if quick == nil {
				quick = desktopui.NewQuickPreview(fixtures.Quick(theme, "history"))
				quick.Open(func() { quick = nil })
			} else {
				quick.Show()
			}
		})
	})
	return mygo.App.Run()
}
