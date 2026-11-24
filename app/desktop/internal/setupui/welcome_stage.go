package setupui

import (
	"time"

	"github.com/egoist/mygo/ui"

	"github.com/uniclipboard/unic/app/desktop/internal/desktopkit"
)

type showcaseState struct {
	scene   string
	manual  bool
	hovered bool
	last    time.Time
}

var showcaseOrder = []string{"pipeline", "actions", "sync"}

func renderWelcome(c *ui.Context, k desktopkit.Kit, m Model, a Actions, tr func(string) string, compact bool, topBand float32) {
	p := paletteFor(k)
	if compact {
		ui.Column(c.Key("welcome-compact")).Fill().Background(p.workspace).Children(func() {
			renderWelcomeStage(c, k, p, tr, true, topBand)
			ui.Box(c).FillWidth().Height(1).Background(p.sep).Shrink(0)
			renderWelcomeAction(c, k, p, m, a, tr, true, 0)
		})
		return
	}
	ui.Row(c.Key("welcome-split")).Fill().AlignItems(ui.Stretch).Background(p.workspace).Children(func() {
		renderWelcomeStage(c, k, p, tr, false, topBand)
		ui.Box(c).Width(1).FillHeight().Background(p.sep).Shrink(0)
		renderWelcomeAction(c, k, p, m, a, tr, false, topBand)
	})
}

func renderWelcomeStage(c *ui.Context, k desktopkit.Kit, p setupColors, tr func(string) string, compact bool, topBand float32) {
	panel := ui.Column(c.Key("welcome-visual")).MinWidth(0).MinHeight(0).Clip()
	if compact {
		h := float32(252)
		if topBand > 0 {
			h = 240 + topBand
		}
		panel.FillWidth().Height(h).Padding(0, 24, 14).Gap(8)
	} else {
		panel.Width(392).FillHeight().Shrink(0).Padding(0, 26, 22).Gap(10)
	}

	st := ui.Local(panel, "setupui.showcase", func() showcaseState {
		return showcaseState{scene: "pipeline", last: time.Now()}
	})
	if st.scene == "" {
		st.scene = "pipeline"
	}
	if !st.manual && !st.hovered && !c.Preferences().ReduceMotion {
		now := time.Now()
		if now.Sub(st.last) >= 5200*time.Millisecond {
			st.last = now
			for i, id := range showcaseOrder {
				if id == st.scene {
					st.scene = showcaseOrder[(i+1)%len(showcaseOrder)]
					break
				}
			}
		}
	}

	// Paint BoardUI layered backdrop: diagonal subtle gradient + radial blue halo + dot grid.
	panel.Draw(func(pt *ui.Painter, rc ui.Rect) {
		from, to := ui.RGB(246, 247, 250), ui.RGB(239, 241, 246)
		if k.Dark {
			from, to = ui.RGB(23, 24, 30), ui.RGB(16, 17, 22)
		}
		pt.FillGradient(rc, ui.LinearGradient{From: from, To: to, Angle: 155, Oklab: true}, 0)
		glow := k.Color("accent-500").Alpha(.10)
		if k.Dark {
			glow = k.Color("accent-500").Alpha(.14)
		}
		pt.FillRadialGradient(rc, ui.RadialGradient{
			Center: ui.Point{X: .5, Y: .40},
			Radius: max(rc.W, rc.H) * .60,
			Stops:  []ui.GradientStop{{Color: glow, At: 0}, {Color: ui.Transparent, At: 1}},
		}, 0)
		dotCol := ui.RGBA(140, 146, 165, .28)
		if k.Dark {
			dotCol = ui.RGBA(255, 255, 255, .095)
		}
		for y := rc.Y + 14; y < rc.Y+rc.H; y += 22 {
			for x := rc.X + 14; x < rc.X+rc.W; x += 22 {
				pt.Fill(ui.Rect{X: x, Y: y, W: 1.5, H: 1.5}, dotCol, 1)
			}
		}
	})

	panel.Children(func() {
		// Top draggable band: reserves macOS traffic-light clearance when integrated.
		dragH := topBand
		if dragH <= 0 {
			if compact {
				dragH = 14
			} else {
				dragH = 26
			}
		}
		drag := ui.Row(c.Key("stage-drag")).FillWidth().Height(dragH).Shrink(0)
		if topBand > 0 {
			drag.DragWindow()
		}

		// Central interactive scene viewport.
		ui.Column(c.Key("scene-viewport")).Grow(1).FillWidth().MinHeight(0).Justify(ui.Center).Children(func() {
			switch st.scene {
			case "actions":
				renderSceneActions(c, k, p, tr, compact)
			case "sync":
				renderSceneSync(c, k, p, tr, compact)
			default:
				renderScenePipeline(c, k, p, tr, compact)
			}
		})

		// Bottom compact BoardUI segmented scene switcher + 1-line scene caption.
		ui.Column(c.Key("stage-controls")).FillWidth().AlignItems(ui.Center).Gap(9).Children(func() {
			if renderShowcaseSwitcher(c.Key("scene-seg"), k, p, tr("核心能力预览"), &st.scene, []showcaseTab{
				{value: "pipeline", label: tr("流水线标签")},
				{value: "actions", label: tr("插件与动作")},
				{value: "sync", label: tr("跨端直连")},
			}) {
				st.manual = true
				st.last = time.Now()
			}
			if !compact {
				caption := "自动识别代码、色值、图片与文件，支持 #标签 @设备 令牌检索"
				switch st.scene {
				case "actions":
					caption = "通过内置动作与 Go / Lua 插件，把一次复制直接变成工作流"
				case "sync":
					caption = "Tailcat 点对点端到端加密同步，无需云账号即可跨设备流转"
				}
				k.Text(c.Key("scene-caption"), tr(caption), desktopkit.TextProps{Role: desktopkit.RoleCaption}).
					FontSize(11.5).TextColor(p.textSub).TextAlign(ui.Center)
			}
		})
	})

	st.hovered = panel.Hovered()
}

type showcaseTab struct {
	value, label string
}

func renderShowcaseSwitcher(c *ui.Context, k desktopkit.Kit, p setupColors, groupLabel string, value *string, items []showcaseTab) bool {
	before := *value
	trackBg := ui.RGBA(17, 17, 22, .055)
	trackBorder := ui.RGBA(17, 17, 22, .06)
	activeBg := p.card
	activeBorder := ui.RGBA(0, 0, 0, .06)
	activeShadow := ui.RGBA(0, 0, 0, .07)
	if k.Dark {
		trackBg = ui.RGBA(255, 255, 255, .06)
		trackBorder = ui.RGBA(255, 255, 255, .08)
		activeBg = ui.RGB(44, 45, 53)
		activeBorder = ui.RGBA(255, 255, 255, .12)
		activeShadow = ui.RGBA(0, 0, 0, .34)
	}
	ui.Row(c).Label(groupLabel).Role(ui.RoleGroup).FocusGroup(ui.Horizontal).
		Padding(3).Gap(2).Radius(9).AlignItems(ui.Center).
		Background(trackBg).Border(1, trackBorder).Children(func() {
		for _, it := range items {
			selected := *value == it.value
			btn := ui.ButtonBase(c.Key(it.value)).Role(ui.RoleToggleButton).Label(it.label).
				Checked(selected).TabStop(selected).
				Height(26).Padding(0, 11).Radius(6.5).AlignItems(ui.Center).Justify(ui.Center)
			if btn.Clicked() {
				*value = it.value
				selected = true
			}
			ink := p.textSub
			if selected {
				ink = k.Color("text-primary")
				btn.Background(activeBg).Border(1, activeBorder)
				btn.Draw(func(pt *ui.Painter, rc ui.Rect) {
					pt.Shadow(rc, 6.5, 0, 1, 3, 0, activeShadow)
				})
			} else if btn.Hovered() {
				ink = k.Color("text-primary")
				if k.Dark {
					btn.Background(ui.RGBA(255, 255, 255, .05))
				} else {
					btn.Background(ui.RGBA(17, 17, 22, .04))
				}
			}
			btn.FocusRingColor(k.Color("border-focus-ring")).FocusRingOffset(1).Children(func() {
				k.Text(c.Key("t"), it.label, desktopkit.TextProps{Role: desktopkit.RoleCaption, Emphasized: selected}).
					FontSize(11.5).TextColor(ink).SingleLine()
			})
		}
	})
	return *value != before
}

func renderScenePipeline(c *ui.Context, k desktopkit.Kit, p setupColors, tr func(string) string, compact bool) {
	ui.Column(c.Key("scene-pipeline")).FillWidth().Gap(9).Children(func() {
		if !compact {
			// Miniature Unified Omnibar showcasing #/@/ token search & Quick Panel shortcut.
			omni := ui.Row(c.Key("omni-mock")).FillWidth().Height(33).PaddingX(10).Radius(9).Gap(5).AlignItems(ui.Center).
				Background(p.card).Border(1, p.cardBorder)
			omni.Draw(func(pt *ui.Painter, rc ui.Rect) {
				pt.Shadow(rc, 9, 0, 1, 3, 0, p.shadowCard)
			})
			omni.Children(func() {
				k.Icon(c, desktopkit.IconSearch, 13).TextColor(p.textMuted)
				omniToken(c.Key("tok-code"), k, "purple", tr("#代码"))
				omniToken(c.Key("tok-color"), k, "blue", tr("#颜色"))
				omniToken(c.Key("tok-dev"), k, "gray", "@MacBook")
				ui.Spacer(c)
				ui.Box(c.Key("kbd")).Padding(2, 5).Radius(4.5).Background(p.inset).Border(1, p.cardBorder).Children(func() {
					k.Text(c, "⌘⇧V", desktopkit.TextProps{Role: desktopkit.RoleCaption, Monospace: true, Emphasized: true}).FontSize(10).TextColor(p.textSub)
				})
			})
		}

		// Front elevated semantic capture card.
		front := ui.Column(c.Key("pipe-front")).FillWidth().Padding(12, 14).Radius(12).Gap(9).
			Background(p.card).Border(1, p.accentEdge)
		front.Draw(func(pt *ui.Painter, rc ui.Rect) {
			pt.Shadow(rc, 12, 0, 8, 22, 0, p.shadowFloat)
		})
		front.Children(func() {
			ui.Row(c.Key("pf-head")).FillWidth().Gap(8).AlignItems(ui.Center).Children(func() {
				iconTile(c.Key("src-ic"), k, 26, 7, p.accentSoft, p.accentInk, desktopkit.IconCode, 13)
				ui.Column(c).Grow(1).MinWidth(0).Gap(1).Children(func() {
					k.Text(c, tr("VS Code · 刚刚复制"), desktopkit.TextProps{Role: desktopkit.RoleCaption, Emphasized: true}).FontSize(12).SingleLine()
					k.Text(c, tr("Capture → Processing 流水线"), desktopkit.TextProps{Role: desktopkit.RoleCaption}).FontSize(10.5).TextColor(p.textMuted).SingleLine()
				})
				statusPill(c.Key("classified"), k, p, pillPositive, tr("已归类"), true)
			})

			// Syntax-highlighted code preview with inline color swatch.
			kwCol := ui.RGB(126, 34, 206)
			if k.Dark {
				kwCol = ui.RGB(216, 180, 254)
			}
			ui.Row(c.Key("code-swatch")).FillWidth().Padding(8, 10).Radius(8).Gap(8).AlignItems(ui.Center).
				Background(p.cardSubtle).Border(1, p.sep).Children(func() {
				ui.Row(c.Key("code-tokens")).Grow(1).MinWidth(0).Gap(5).AlignItems(ui.Center).Clip().Children(func() {
					k.Text(c, "const", desktopkit.TextProps{Role: desktopkit.RoleCaption, Monospace: true, Emphasized: true}).FontSize(11.2).TextColor(kwCol).NoWrap()
					k.Text(c, "accent", desktopkit.TextProps{Role: desktopkit.RoleCaption, Monospace: true}).FontSize(11.2).NoWrap()
					k.Text(c, "=", desktopkit.TextProps{Role: desktopkit.RoleCaption, Monospace: true}).FontSize(11.2).TextColor(p.textMuted).NoWrap()
					ui.Row(c.Key("code-val")).Gap(0).AlignItems(ui.Center).Children(func() {
						k.Text(c, `"#2B7FFF"`, desktopkit.TextProps{Role: desktopkit.RoleCaption, Monospace: true, Emphasized: true}).FontSize(11.2).TextColor(p.accentInk).NoWrap()
						k.Text(c, ";", desktopkit.TextProps{Role: desktopkit.RoleCaption, Monospace: true}).FontSize(11.2).TextColor(p.textMuted).NoWrap()
					})
				})
				ui.Row(c.Key("swatch-chip")).Padding(3, 7, 3, 4).Radius(6).Gap(5).AlignItems(ui.Center).Shrink(0).
					Background(p.card).Border(1, p.cardBorder).Children(func() {
					ui.Box(c).Size(11, 11).Radius(3).Background(ui.RGB(43, 127, 255)).Shrink(0).AccessHidden(true)
					k.Text(c, "#2B7FFF", desktopkit.TextProps{Role: desktopkit.RoleCaption, Monospace: true, Emphasized: true}).FontSize(10.5).NoWrap()
				})
			})

			ui.Row(c.Key("pf-tags")).Wrap().Gap(5).AlignItems(ui.Center).Children(func() {
				tagChip(c.Key("t-code"), k, "purple", tr("代码"))
				tagChip(c.Key("t-color"), k, "blue", tr("颜色"))
				tagChip(c.Key("t-text"), k, "gray", tr("文本"))
			})
		})

		if !compact {
			// Peeking second card underneath for realistic BoardUI stack depth.
			ui.Box(c.Key("pipe-back-wrap")).FillWidth().Padding(0, 10, 0).Children(func() {
				ui.Row(c.Key("pipe-back")).FillWidth().Padding(8, 11).Radius(10).Gap(8).AlignItems(ui.Center).
					Background(p.card.Alpha(.88)).Border(1, p.cardBorder).Children(func() {
					k.Icon(c, desktopkit.IconImage, 13).TextColor(p.okDot)
					k.Text(c, "boardui-preview@2x.png · 1440×900", desktopkit.TextProps{Role: desktopkit.RoleCaption}).FontSize(10.8).TextColor(p.textSub).Grow(1).MinWidth(0).SingleLine()
					tagChip(c.Key("t-img"), k, "lime", tr("图片"))
				})
			})
		}
	})
}

func renderSceneActions(c *ui.Context, k desktopkit.Kit, p setupColors, tr func(string) string, compact bool) {
	ui.Column(c.Key("scene-actions")).FillWidth().Gap(9).Children(func() {
		// Selected clipboard snapshot entry.
		ui.Row(c.Key("act-entry")).FillWidth().Padding(9, 12).Radius(11).Gap(9).AlignItems(ui.Center).
			Background(p.card).Border(1, p.cardBorder).Children(func() {
			bg, ink := k.TagTint("orange")
			iconTile(c.Key("file-ic"), k, 30, 8, bg, ink, desktopkit.IconFile, 14)
			ui.Column(c).Grow(1).MinWidth(0).Gap(1).Children(func() {
				k.Text(c, "release-assets-v2.zip", desktopkit.TextProps{Role: desktopkit.RoleCaption, Emphasized: true}).FontSize(12).SingleLine()
				k.Text(c, tr("12.4 MB · 目录快照"), desktopkit.TextProps{Role: desktopkit.RoleCaption}).FontSize(10.8).TextColor(p.textMuted).SingleLine()
			})
			tagChip(c.Key("t-file"), k, "orange", tr("文件"))
		})

		// Floating BoardUI Action menu popover.
		menu := ui.Column(c.Key("act-menu")).FillWidth().Padding(6).Radius(12).Gap(3).
			Background(p.card).Border(1, p.cardBorder)
		menu.Draw(func(pt *ui.Painter, rc ui.Rect) {
			pt.Shadow(rc, 12, 0, 8, 22, 0, p.shadowFloat)
		})
		menu.Children(func() {
			if !compact {
				ui.Row(c.Key("act-h")).FillWidth().Padding(3, 8, 4).AlignItems(ui.Center).Children(func() {
					k.Text(c, tr("对当前条目执行动作 · Actions"), desktopkit.TextProps{Role: desktopkit.RoleCaption, Emphasized: true}).FontSize(10.5).TextColor(p.textMuted)
					ui.Spacer(c)
					k.Text(c, "⌘K", desktopkit.TextProps{Role: desktopkit.RoleCaption, Monospace: true}).FontSize(10).TextColor(p.textMuted)
				})
			}

			// Highlighted active Action: 临时分享.
			ui.Row(c.Key("act-share")).FillWidth().Padding(7, 9).Radius(8).Gap(8).AlignItems(ui.Center).
				Background(p.accentSoft).Border(1, p.accentEdge).Children(func() {
				iconTile(c, k, 24, 6, k.Color("accent-500"), k.Color("text-white"), desktopkit.IconSend, 12)
				ui.Column(c).Grow(1).MinWidth(0).Gap(1).Children(func() {
					k.Text(c, tr("临时分享"), desktopkit.TextProps{Role: desktopkit.RoleCaption, Emphasized: true}).FontSize(11.8).SingleLine()
					k.Text(c, tr("生成取件链接 · 阅后过期"), desktopkit.TextProps{Role: desktopkit.RoleCaption}).FontSize(10.3).TextColor(p.textSub).SingleLine()
				})
				statusPill(c.Key("shared"), k, p, pillPositive, tr("链接已生成"), true)
			})

			// Secondary Action: builtin.text.
			ui.Row(c.Key("act-text")).FillWidth().Padding(6, 9).Radius(8).Gap(8).AlignItems(ui.Center).Children(func() {
				iconTile(c, k, 24, 6, p.inset, p.textSub, desktopkit.IconEdit, 12)
				ui.Column(c).Grow(1).MinWidth(0).Gap(1).Children(func() {
					k.Text(c, tr("文本清洗 / JSON 格式化"), desktopkit.TextProps{Role: desktopkit.RoleCaption, Emphasized: true}).FontSize(11.8).SingleLine()
					k.Text(c, "builtin.text", desktopkit.TextProps{Role: desktopkit.RoleCaption, Monospace: true}).FontSize(10.3).TextColor(p.textMuted).SingleLine()
				})
			})

			if !compact {
				// Tertiary Action: Lua custom plugin.
				ui.Row(c.Key("act-lua")).FillWidth().Padding(6, 9).Radius(8).Gap(8).AlignItems(ui.Center).Children(func() {
					iconTile(c, k, 24, 6, p.inset, p.textSub, desktopkit.IconCode, 12)
					ui.Column(c).Grow(1).MinWidth(0).Gap(1).Children(func() {
						k.Text(c, tr("自定义 Lua 处理脚本"), desktopkit.TextProps{Role: desktopkit.RoleCaption, Emphasized: true}).FontSize(11.8).SingleLine()
						k.Text(c, tr("插件市场 · 沙箱运行"), desktopkit.TextProps{Role: desktopkit.RoleCaption}).FontSize(10.3).TextColor(p.textMuted).SingleLine()
					})
				})
			}
		})
	})
}

func renderSceneSync(c *ui.Context, k desktopkit.Kit, p setupColors, tr func(string) string, compact bool) {
	ui.Column(c.Key("scene-sync")).FillWidth().Gap(0).Children(func() {
		// Top local hub card using the official pixel-cat logo.
		hub := ui.Row(c.Key("sync-hub")).FillWidth().Padding(10, 12).Radius(12).Gap(10).AlignItems(ui.Center).
			Background(p.card).Border(1, p.cardBorder)
		hub.Draw(func(pt *ui.Painter, rc ui.Rect) {
			pt.Shadow(rc, 12, 0, 6, 18, 0, p.shadowFloat)
		})
		hub.Children(func() {
			brandLogo(c.Key("hub-logo"), k, 32, 8, false)
			ui.Column(c).Grow(1).MinWidth(0).Gap(1).Children(func() {
				k.Text(c, tr("MacBook Pro（本机）"), desktopkit.TextProps{Role: desktopkit.RoleCaption, Emphasized: true}).FontSize(12).SingleLine()
				k.Text(c, tr("设备身份已就绪 · 仅本地存储"), desktopkit.TextProps{Role: desktopkit.RoleCaption}).FontSize(10.8).TextColor(p.textMuted).SingleLine()
			})
			statusPill(c.Key("direct"), k, p, pillPositive, tr("直连"), true)
		})

		// Painted P2P branch bridge with centered Tailcat encrypted pill.
		bridgeH := float32(46)
		if compact {
			bridgeH = 32
		}
		bridge := ui.Row(c.Key("sync-bridge")).FillWidth().Height(bridgeH).Center()
		bridge.Draw(func(pt *ui.Painter, rc ui.Rect) {
			midX := rc.X + rc.W/2
			midY := rc.Y + rc.H/2
			leftX := rc.X + rc.W*.25
			rightX := rc.X + rc.W*.75
			lineCol := p.accentEdge
			pt.Line(midX, rc.Y, midX, midY, 1.5, lineCol)
			pt.Line(leftX, midY, rightX, midY, 1.5, lineCol)
			pt.Line(leftX, midY, leftX, rc.Y+rc.H, 1.5, lineCol)
			pt.Line(rightX, midY, rightX, rc.Y+rc.H, 1.5, lineCol)
			pt.Fill(ui.Rect{X: leftX - 2.5, Y: rc.Y + rc.H - 2.5, W: 5, H: 5}, k.Color("accent-500"), 2.5)
			pt.Fill(ui.Rect{X: rightX - 2.5, Y: rc.Y + rc.H - 2.5, W: 5, H: 5}, k.Color("accent-500"), 2.5)
		})
		bridge.Children(func() {
			ui.Row(c.Key("tailcat-pill")).Height(23).PaddingX(10).Radius(11.5).Gap(5).AlignItems(ui.Center).
				Background(p.card).Border(1, p.accentEdge).Children(func() {
				ui.Box(c).Size(6, 6).Radius(3).Background(k.Color("accent-500")).Shrink(0).AccessHidden(true)
				k.Text(c, tr("Tailcat 端到端加密 · 免云账号"), desktopkit.TextProps{Role: desktopkit.RoleCaption, Emphasized: true}).FontSize(10.8).TextColor(p.accentInk).NoWrap()
			})
		})

		// Bottom 2-column peer device cards.
		ui.Row(c.Key("peer-grid")).FillWidth().Gap(8).Children(func() {
			peerCard(c.Key("peer-win"), k, p, tr("Windows 台式机"), tr("指纹已信任"), "4 ms")
			peerCard(c.Key("peer-ios"), k, p, "iPhone", tr("已同步最新条目"), tr("刚刚"))
		})
	})
}

func peerCard(c *ui.Context, k desktopkit.Kit, p setupColors, name, leftMeta, rightMeta string) {
	ui.Column(c).Grow(1).MinWidth(0).Padding(10, 11).Radius(10).Gap(4).
		Background(p.card).Border(1, p.cardBorder).Children(func() {
		ui.Row(c).FillWidth().AlignItems(ui.Center).Gap(6).Children(func() {
			k.Text(c, name, desktopkit.TextProps{Role: desktopkit.RoleCaption, Emphasized: true}).FontSize(11.8).Grow(1).MinWidth(0).SingleLine()
			ui.Box(c).Size(6, 6).Radius(3).Background(p.okDot).Shrink(0).AccessHidden(true)
		})
		ui.Row(c).FillWidth().AlignItems(ui.Center).Gap(4).Children(func() {
			k.Text(c, leftMeta, desktopkit.TextProps{Role: desktopkit.RoleCaption}).FontSize(10.5).TextColor(p.textMuted).Grow(1).MinWidth(0).SingleLine()
			k.Text(c, rightMeta, desktopkit.TextProps{Role: desktopkit.RoleCaption, Monospace: true}).FontSize(10.5).TextColor(p.textSub).NoWrap()
		})
	})
}

func omniToken(c *ui.Context, k desktopkit.Kit, color desktopkit.TagColor, text string) {
	bg, ink := k.TagTint(color)
	if k.Dark && (color == "" || color == "gray") {
		bg = ui.RGBA(255, 255, 255, .10)
		ink = ui.RGB(190, 192, 202)
	}
	ui.Box(c).Padding(2, 6).Radius(5).Background(bg).Shrink(0).Children(func() {
		k.Text(c, text, desktopkit.TextProps{Role: desktopkit.RoleCaption, Emphasized: true}).FontSize(10.8).TextColor(ink).NoWrap()
	})
}

func renderWelcomeAction(c *ui.Context, k desktopkit.Kit, p setupColors, m Model, a Actions, tr func(string) string, compact bool, topBand float32) {
	col := ui.Column(c.Key("welcome-action")).Grow(1).MinWidth(0).MinHeight(0).Justify(ui.Center)
	if compact {
		col.FillWidth().Padding(18, 36, 22).AlignItems(ui.Center).Gap(0)
	} else {
		col.FillHeight().Padding(32, 40, 32).AlignItems(ui.Start).Gap(0)
	}

	col.Children(func() {
		if !compact && topBand > 0 {
			ui.Box(c.Key("action-drag")).Absolute().Top(0).Left(0).Right(0).Height(topBand).DragWindow()
		}

		// Brand logo + crisp version eyebrow.
		logoSize, logoRadius := float32(48), float32(12)
		if compact {
			logoSize, logoRadius = 36, 9
		}
		ui.Row(c.Key("brand-row")).Gap(12).AlignItems(ui.Center).Margin(0, 0, 16).Children(func() {
			brandLogo(c.Key("logo"), k, logoSize, logoRadius, true)
			ui.Row(c.Key("ver-pill")).Height(22).PaddingX(8).Radius(11).AlignItems(ui.Center).
				Background(p.cardSubtle).Border(1, p.cardBorder).Children(func() {
				k.Text(c.Key("eyebrow"), "UniClipboard v2", desktopkit.TextProps{Role: desktopkit.RoleCaption, Emphasized: true}).
					FontSize(11).TextColor(p.textSub).NoWrap()
			})
		})

		// Exact 0026 product heading and slogan.
		heading := k.Text(c.Key("heading"), tr("欢迎使用 UniClipboard"), desktopkit.TextProps{Role: desktopkit.RoleTitle, Emphasized: true}).
			FontSize(24).LetterSpacing(-0.3)
		slogan := k.Text(c.Key("slogan"), tr("让每一次复制更有用"), desktopkit.TextProps{}).
			FontSize(14.5).TextColor(p.textSub).Margin(6, 0, 22)
		if compact {
			heading.TextAlign(ui.Center)
			slogan.TextAlign(ui.Center).Margin(5, 0, 16)
		}

		// Contextual non-intrusive v1 detection card.
		renderWelcomeDetectCard(c, k, p, m, a, tr, compact)

		// Exact 0026 primary CTA.
		ui.Row(c.Key("cta-row")).Gap(12).AlignItems(ui.Center).Children(func() {
			actionButton(c, k, "begin", tr("开始使用"), desktopkit.Primary, desktopkit.SizeLarge, "", m.PendingStart || m.Entering, m.PendingStart || m.Entering, a.Begin)
			if m.Entering {
				k.Text(c.Key("starting"), tr("正在启动应用…"), desktopkit.TextProps{Role: desktopkit.RoleCaption}).FontSize(12).TextColor(p.textSub)
			}
		})
	})
}

func renderWelcomeDetectCard(c *ui.Context, k desktopkit.Kit, p setupColors, m Model, a Actions, tr func(string) string, compact bool) {
	if !m.Detecting && !m.Found && m.Error == "" {
		return
	}
	card := ui.Row(c.Key("detection")).FillWidth().Padding(11, 13).Radius(10).Gap(10).AlignItems(ui.Start).Margin(0, 0, 20)
	if compact {
		card.MaxWidth(460).Margin(0, 0, 14)
	}
	switch {
	case m.Detecting:
		card.Background(p.cardSubtle).Border(1, p.cardBorder).Children(func() {
			ui.Box(c).Size(28, 28).Radius(7).Background(p.card).Border(1, p.cardBorder).Center().Shrink(0).Children(func() {
				ui.Spinner(c)
			})
			ui.Column(c).Grow(1).MinWidth(0).Gap(2).Children(func() {
				k.Text(c.Key("detecting"), tr("正在检查这台电脑上的 v1 数据…"), desktopkit.TextProps{Role: desktopkit.RoleCaption, Emphasized: true}).FontSize(12.5)
				k.Text(c, tr("仅检查默认目录结构，不会读取剪贴板或请求系统凭据。"), desktopkit.TextProps{Role: desktopkit.RoleCaption}).FontSize(11.5).TextColor(p.textSub)
			})
		})
	case m.Error != "":
		card.Background(p.errBg).Border(1, p.errEdge).Children(func() {
			iconTile(c, k, 28, 7, p.card, p.errFg, desktopkit.IconInfo, 15).Border(1, p.errEdge)
			ui.Column(c).Grow(1).MinWidth(0).Gap(5).Children(func() {
				k.Text(c.Key("detection-error"), tr(m.Error), desktopkit.TextProps{Role: desktopkit.RoleCaption, Emphasized: true}).FontSize(12.2).TextColor(p.errFg)
				ui.Row(c.Key("err-acts")).Gap(12).AlignItems(ui.Center).Children(func() {
					actionButton(c, k, "redetect", tr("重试"), desktopkit.Link, desktopkit.SizeSmall, "", m.Entering, false, a.Redetect)
					actionButton(c, k, "manual", tr("手动选择目录"), desktopkit.Link, desktopkit.SizeSmall, "", m.Entering, false, a.Pick)
				})
			})
		})
	case m.Found:
		card.Background(p.cardSubtle).Border(1, p.okEdge).Children(func() {
			iconTile(c, k, 28, 7, p.okBg, p.okFg, desktopkit.IconPackage, 14)
			ui.Column(c).Grow(1).MinWidth(0).Gap(3).Children(func() {
				ui.Row(c).Wrap().Gap(6).AlignItems(ui.Center).Children(func() {
					k.Text(c.Key("detected"), tr("发现 UniClipboard v1 的数据"), desktopkit.TextProps{Role: desktopkit.RoleCaption, Emphasized: true}).FontSize(12.5)
					statusPill(c.Key("importable"), k, p, pillPositive, tr("可导入"), true)
				})
				k.Text(c, tr("点击「开始使用」后可确认导入旧历史，或选择暂不迁移。"), desktopkit.TextProps{Role: desktopkit.RoleCaption}).FontSize(11.5).TextColor(p.textSub)
			})
		})
	}
}
