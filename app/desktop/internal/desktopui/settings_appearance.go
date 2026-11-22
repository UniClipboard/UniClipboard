package desktopui

import (
	"github.com/egoist/mygo/ui"

	"github.com/uniclipboard/unic/app/desktop/internal/desktopkit"
	config "github.com/uniclipboard/unic/internal/localconfig"
)

// liveAppearanceContent edits only the caller's configuration snapshot. The
// existing revision-checked save path owns persistence; the Desktop host owns
// application to every native window through mygo's shared theme source.
func (d *viewState) liveAppearanceContent(c *ui.Context, k desktopkit.Kit, cfg *config.Config) bool {
	changed := false
	busy := d.live.busy

	// 1. Visual Color Mode cards (matching prototype/settings.html).
	k.SettingsGroup(c.Key("appearance"), desktopkit.SettingsGroupProps{
		Title: d.tr("颜色模式"),
		Trailing: func() {
			settingsPill(c.Key("live-pill"), k, d.tr("实时生效 · 同步全角色"), desktopkit.StatusInfo, false, false)
		},
		Footer: d.tr("同步应用到主窗口、首次引导与快捷面板，无需重启。"),
	}, func() {
		modes := []struct{ id, name, sub string }{
			{"system", "跟随系统", "自动匹配操作系统外观"},
			{"light", "浅色", "始终使用明亮界面"},
			{"dark", "深色", "始终使用低眩光深色"},
		}
		ui.Row(c.Key("theme-grid")).Wrap().Padding(14, 16).Gap(12).MinWidth(0).Children(func() {
			for _, m := range modes {
				selected := cfg.Desktop.ColorMode == m.id
				card := ui.ButtonBase(c.Key("mode-" + m.id)).
					Label(d.tr(m.name)).
					Selected(selected).
					Disabled(busy).
					Basis(170).Grow(1).MinWidth(150).
					Padding(10).Radius(11).
					FocusRingColor(k.Color("border-focus-ring")).FocusRingOffset(2)
				bg := k.SubtleCardSurface()
				borderCol := k.Color("border-button-default")
				var borderW float32 = 1.5
				if selected {
					bg = k.Color("accent-50")
					borderCol = k.Color("accent-500")
				} else if card.Hovered() && !busy {
					borderCol = k.Color("border-button-hover")
				}
				card.Background(bg).Border(borderW, borderCol)
				card.Children(func() {
					ui.Column(c.Key("inner")).FillWidth().Gap(9).MinWidth(0).Children(func() {
						themeMiniPreview(c.Key("preview"), m.id)
						ui.Row(c.Key("meta")).FillWidth().AlignItems(ui.Center).Justify(ui.SpaceBetween).Gap(6).Padding(0, 2).MinWidth(0).Children(func() {
							ui.Column(c.Key("labels")).Grow(1).MinWidth(0).Gap(1).Children(func() {
								k.Text(c.Key("name"), d.tr(m.name), desktopkit.TextProps{Emphasized: true, Truncate: true})
								k.Text(c.Key("sub"), d.tr(m.sub), desktopkit.TextProps{Role: desktopkit.RoleCaption, Tone: desktopkit.ToneSecondary, Truncate: true})
							})
							check := ui.Row(c.Key("check")).Size(18, 18).Radius(9).Center().Shrink(0).AccessHidden(true)
							if selected {
								check.Background(k.Color("accent-500"))
								check.Children(func() {
									ui.Text(c.Key("mark"), "✓").FontSize(11).FontWeight(700).TextColor(k.Color("text-white"))
								})
							} else {
								check.Border(1.5, k.Color("border-button-default"))
							}
						})
					})
				})
				if card.Clicked() && !busy && !selected {
					cfg.Desktop.ColorMode = m.id
					changed = true
				}
			}
		})
	})

	// 2. Tray icon & Window frame (relocated from General per prototype/settings.html).
	k.SettingsGroup(c.Key("tray-window"), desktopkit.SettingsGroupProps{Title: d.tr("托盘与窗口")}, func() {
		k.SettingsRow(c.Key("tray-icon"), desktopkit.SettingsRowProps{
			Label:       d.tr("托盘图标"),
			Description: d.tr("更改后立即生效，无需重启。"),
		}, nil, func() {
			seg := k.SegmentedControl(c.Key("tray-seg"), desktopkit.SegmentedControlProps{
				Label: d.tr("托盘图标"),
				Value: &cfg.Desktop.TrayIcon,
				Items: []desktopkit.SegmentedItem{
					{Value: "glyph", Label: d.tr("默认")},
					{Value: "cat", Label: d.tr("经典猫头")},
				},
			})
			if seg.Changed && !busy {
				changed = true
			}
		})
		framePending := d.windowFrame != nil && *d.windowFrame != cfg.Desktop.SystemWindowFrame
		var extra func()
		if framePending {
			extra = func() {
				settingsPill(c.Key("restart-pill"), k, d.tr("需重开窗口"), desktopkit.StatusWarning, false, false)
			}
		}
		k.SettingsRow(c.Key("system-window-frame"), desktopkit.SettingsRowProps{
			Label:       d.tr("使用系统边框"),
			Description: d.tr("关闭并重新打开主窗口后生效。"),
		}, extra, func() {
			if k.Switch(c, &cfg.Desktop.SystemWindowFrame, desktopkit.SwitchProps{
				Label:    d.tr("使用系统边框"),
				Disabled: busy,
			}).Changed() {
				changed = true
			}
		})
	})

	return changed
}

// themeMiniPreview draws a crisp miniature BoardUI split-window preview inside each theme card.
func themeMiniPreview(c *ui.Context, kind string) {
	lightSide, lightBorder, lightMain, lightBar, lightAccent, lightCard, lightCardBorder :=
		ui.RGB(242, 242, 245), ui.RGB(226, 226, 231), ui.RGB(255, 255, 255), ui.RGB(212, 212, 220), ui.RGB(43, 127, 255), ui.RGB(247, 247, 250), ui.RGB(229, 229, 234)
	darkSide, darkBorder, darkMain, darkBar, darkAccent, darkCard, darkCardBorder :=
		ui.RGB(20, 20, 23), ui.RGB(43, 43, 50), ui.RGB(30, 30, 34), ui.RGB(59, 59, 68), ui.RGB(77, 160, 255), ui.RGB(39, 39, 45), ui.RGB(53, 53, 61)

	dots := func(key string, dotCol ui.Color, count int) {
		ui.Row(c.Key(key)).Gap(3).Margin(0, 0, 2, 0).Children(func() {
			for i := 0; i < count; i++ {
				ui.Box(c.Key(i)).Size(4, 4).Radius(2).Background(dotCol)
			}
		})
	}
	pane := func(key string, dark, withSide bool) {
		sideBg, sideBdr, mainBg, barCol, accCol, cardBg, cardBdr := lightSide, lightBorder, lightMain, lightBar, lightAccent, lightCard, lightCardBorder
		dotCol := ui.RGBA(136, 136, 136, .45)
		if dark {
			sideBg, sideBdr, mainBg, barCol, accCol, cardBg, cardBdr = darkSide, darkBorder, darkMain, darkBar, darkAccent, darkCard, darkCardBorder
			dotCol = ui.RGBA(120, 120, 128, .45)
		}
		ui.Row(c.Key(key)).Grow(1).FillHeight().MinWidth(0).Children(func() {
			if withSide {
				ui.Column(c.Key("side")).Width(42).FillHeight().Padding(6, 5).Gap(4).Background(sideBg).
					Border(0, sideBdr).Children(func() {
					dots("dots", dotCol, 3)
					ui.Box(c.Key("b1")).Height(5).FillWidth().Radius(2).Background(accCol)
					ui.Box(c.Key("b2")).Height(5).Width(24).Radius(2).Background(barCol)
					ui.Box(c.Key("b3")).Height(5).Width(18).Radius(2).Background(barCol)
				})
				ui.Box(c.Key("sep")).Width(1).FillHeight().Background(sideBdr)
			}
			ui.Column(c.Key("main")).Grow(1).FillHeight().Padding(7, 6).Background(mainBg).MinWidth(0).Children(func() {
				ui.Column(c.Key("card")).Grow(1).FillWidth().Padding(5).Gap(4).Radius(4).Background(cardBg).Border(1, cardBdr).Children(func() {
					if withSide {
						ui.Box(c.Key("c1")).Height(5).Width(42).Radius(2).Background(barCol)
						ui.Box(c.Key("c2")).Height(5).FillWidth().Radius(2).Background(barCol)
						ui.Box(c.Key("c3")).Height(5).Width(30).Radius(2).Background(barCol)
					} else {
						ui.Box(c.Key("c1")).Height(5).Width(36).Radius(2).Background(accCol)
						ui.Box(c.Key("c2")).Height(5).FillWidth().Radius(2).Background(barCol)
					}
				})
			})
		})
	}

	ui.Row(c).FillWidth().Height(74).Radius(7).Clip().Border(1, ui.RGBA(128, 128, 140, .24)).AccessHidden(true).Children(func() {
		switch kind {
		case "system":
			pane("left", false, true)
			pane("right", true, false)
		case "dark":
			pane("full", true, true)
		default:
			pane("full", false, true)
		}
	})
}
