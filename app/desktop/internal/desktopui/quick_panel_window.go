package desktopui

import (
	"strings"
	"time"

	"github.com/egoist/mygo"
	"github.com/egoist/mygo/ui"
)

func openQuickWindow(p *quickPanelState, closed func()) *mygo.Window {
	p.quit = mygo.App.Quit

	title := "unic · 快捷面板演示"
	if p.live != nil {
		title = "unic · 快捷面板"
	}
	history := mygo.NewWindow(mygo.WindowOptions{
		Title: title, Width: 380, Height: 366, UseContentSize: true,
		Frameless: true, DisableResize: true, AlwaysOnTop: true,
		Content: ui.View(p.panel),
	})
	satellite := mygo.NewFlyout(mygo.FlyoutOptions{
		Parent: history, Width: 400, Height: 240, Hidden: true, Focusable: false,
		Placement: mygo.PlacementRight, Gap: 8, Shadow: true, CornerRadius: 12,
		Anchor: mygo.Rectangle{X: 0, Y: 54, Width: 380, Height: 30}, Content: ui.View(p.satellite),
	})
	syncWindow := func() {
		if !p.visible {
			satellite.Hide()
			history.Hide()
			return
		}
		if p.menu == "" && p.previewID == "" {
			satellite.Hide()
			return
		}
		width, height := 400, 240
		if p.menu != "" {
			width = 380
			height = 44 + len(p.actions())*32 + 34
		} else if e := p.active(); e != nil {
			height = 80 + (strings.Count(e.text, "\n")+1)*22
			if e.kind == "图片" {
				height = 310
			}
		}
		height = max(96, min(480, height))
		anchor := p.anchor
		if anchor.H <= 0 {
			anchor = ui.Rect{Y: 54, W: 380, H: 30}
		}
		// Use the full parent width so the satellite follows a row vertically
		// but never overlaps the history panel horizontally.
		satellite.SetAnchor(mygo.Rectangle{X: 0, Y: int(anchor.Y), Width: 380, Height: int(anchor.H)})
		satellite.SetSize(width, height)
		satellite.Invalidate()
		satellite.ShowInactive()
	}
	p.onUpdate = syncWindow
	history.OnMove(syncWindow)
	p.nativeFocused = history.IsFocused
	ignoreBlurUntil := time.Now().Add(300 * time.Millisecond)
	history.OnBlur(func() {
		p.blurDue = time.Now().Add(100 * time.Millisecond)
		if p.blurDue.Before(ignoreBlurUntil) {
			p.blurDue = ignoreBlurUntil
		}
		history.Invalidate()
	})
	offActivate := func() {}
	if p.live == nil {
		offActivate = mygo.App.OnActivate(func(hasVisibleWindows bool) {
			if !hasVisibleWindows {
				p.visible = true
				p.expanded = false
				p.menu = ""
				p.applySearch()
				history.Show()
				history.Invalidate()
			}
		})
	}

	history.OnClosed(func() {
		offActivate()
		p.onUpdate = nil
		p.panelInvalidate = nil
		p.satelliteInvalidate = nil
		p.nativeFocused = nil
		if closed != nil {
			closed()
		}
	})
	return history
}
