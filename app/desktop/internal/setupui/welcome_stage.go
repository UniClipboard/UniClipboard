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

func renderWelcome(c *ui.Context, k desktopkit.Kit, m Model, a Actions, tr func(string) string, compact bool) {
	p := paletteFor(k)
	if compact {
		ui.Column(c.Key("welcome-compact")).Fill().Background(p.workspace).Children(func() {
			renderWelcomeStage(c, k, p, tr, true)
			ui.Divider(c)
			renderWelcomeAction(c, k, p, m, a, tr, true)
		})
		return
	}
	ui.Row(c.Key("welcome-split")).Fill().AlignItems(ui.Stretch).Background(p.workspace).Children(func() {
		renderWelcomeStage(c, k, p, tr, false)
		ui.Box(c).Width(1).FillHeight().Background(p.sep).Shrink(0)
		renderWelcomeAction(c, k, p, m, a, tr, false)
	})
}

func renderWelcomeStage(c *ui.Context, k desktopkit.Kit, p setupColors, tr func(string) string, compact bool) {
	panel := ui.Column(c.Key("welcome-visual")).MinWidth(0).MinHeight(0).Clip()
	if compact {
		panel.FillWidth().Height(248).Padding(12, 22, 10).Gap(8)
	} else {
		panel.Width(392).FillHeight().Shrink(0).Padding(14, 24, 18).Gap(12)
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
		from, to := p.sidebar, p.inset
		if k.Dark {
			from, to = ui.RGB(23, 24, 30), ui.RGB(17, 18, 22)
		}
		pt.FillGradient(rc, ui.LinearGradient{From: from, To: to, Angle: 155, Oklab: true}, 0)
		glow := k.Color("accent-500").Alpha(.11)
		if k.Dark {
			glow = k.Color("accent-500").Alpha(.15)
		}
		pt.FillRadialGradient(rc, ui.RadialGradient{
			Center: ui.Point{X: .5, Y: .38},
			Radius: max(rc.W, rc.H) * .62,
			Stops:  []ui.GradientStop{{Color: glow, At: 0}, {Color: ui.Transparent, At: 1}},
		}, 0)
		dotCol := k.Color("border-button-hover").Alpha(.42)
		if k.Dark {
			dotCol = ui.RGBA(255, 255, 255, .08)
		}
		for y := rc.Y + 12; y < rc.Y+rc.H; y += 22 {
			for x := rc.X + 12; x < rc.X+rc.W; x += 22 {
				pt.Fill(ui.Rect{X: x, Y: y, W: 1.5, H: 1.5}, dotCol, 1)
			}
		}
	})

	panel.Children(func() {
		// Top right subtle brand badge.
		ui.Row(c.Key("stage-top")).FillWidth().Justify(ui.End).AlignItems(ui.Center).Children(func() {
			ui.Row(c.Key("brand-pill")).Height(24).Padding(0, 9, 0, 4).Radius(12).Gap(6).AlignItems(ui.Center).
				Background(p.card.Alpha(.88)).Border(1, p.cardBorder).Children(func() {
				brandLogo(c.Key("mini-logo"), k, 16, 4, false)
				k.Text(c, "UniClipboard v2", desktopkit.TextProps{Role: desktopkit.RoleCaption, Tone: desktopkit.ToneSecondary, Emphasized: true}).FontSize(11)
			})
		})

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

		// Bottom BoardUI segmented scene switcher + 1-line scene caption.
		ui.Column(c.Key("stage-controls")).FillWidth().AlignItems(ui.Center).Gap(7).Children(func() {
			seg := k.SegmentedControl(c.Key("scene-seg"), desktopkit.SegmentedControlProps{
				Label: tr("核心能力预览"),
				Value: &st.scene,
				Items: []desktopkit.SegmentedItem{
					{Value: "pipeline", Label: tr("流水线标签")},
					{Value: "actions", Label: tr("插件与动作")},
					{Value: "sync", Label: tr("跨端直连")},
				},
			})
			if seg.Changed {
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
				k.Text(c.Key("scene-caption"), tr(caption), desktopkit.TextProps{Role: desktopkit.RoleCaption, Tone: desktopkit.ToneSecondary}).FontSize(11).TextAlign(ui.Center)
			}
		})
	})

	st.hovered = panel.Hovered()
}

func renderScenePipeline(c *ui.Context, k desktopkit.Kit, p setupColors, tr func(string) string, compact bool) {
	ui.Column(c.Key("scene-pipeline")).FillWidth().Gap(9).Children(func() {
		if !compact {
			// Miniature Unified Omnibar showcasing #/@/ token search & Quick Panel shortcut.
			ui.Row(c.Key("omni-mock")).FillWidth().Height(32).PaddingX(9).Radius(9).Gap(5).AlignItems(ui.Center).
				Background(p.card).Border(1, p.cardBorder).Children(func() {
				k.Icon(c, desktopkit.IconSearch, 13).TextColor(k.Color("text-tertiary"))
				omniToken(c.Key("tok-code"), k, "purple", tr("#代码"))
				omniToken(c.Key("tok-color"), k, "blue", tr("#颜色"))
				omniToken(c.Key("tok-dev"), k, "gray", "@MacBook")
				ui.Spacer(c)
				ui.Box(c.Key("kbd")).Padding(2, 5).Radius(4).Background(p.inset).Border(1, p.cardBorder).Children(func() {
					k.Text(c, "⌘⇧V", desktopkit.TextProps{Role: desktopkit.RoleCaption, Tone: desktopkit.ToneSecondary, Monospace: true, Emphasized: true}).FontSize(10)
				})
			})
		}

		// Front elevated semantic capture card.
		front := ui.Column(c.Key("pipe-front")).FillWidth().Padding(11, 13).Radius(12).Gap(8).
			Background(p.card).Border(1, p.accentEdge)
		front.Draw(func(pt *ui.Painter, rc ui.Rect) {
			pt.Shadow(rc, 12, 0, 8, 22, 0, p.shadowFloat)
		})
		front.Children(func() {
			ui.Row(c.Key("pf-head")).FillWidth().Gap(8).AlignItems(ui.Center).Children(func() {
				iconTile(c.Key("src-ic"), k, 24, 6, p.accentSoft, p.accentInk, desktopkit.IconCode, 13)
				ui.Column(c).Grow(1).MinWidth(0).Gap(1).Children(func() {
					k.Text(c, tr("VS Code · 刚刚复制"), desktopkit.TextProps{Role: desktopkit.RoleCaption, Emphasized: true}).FontSize(11.5).SingleLine()
					k.Text(c, tr("Capture → Processing 流水线"), desktopkit.TextProps{Role: desktopkit.RoleCaption, Tone: desktopkit.ToneTertiary}).FontSize(10).SingleLine()
				})
				statusPill(c.Key("classified"), k, p, pillPositive, tr("已归类"), true)
			})

			ui.Row(c.Key("code-swatch")).FillWidth().Padding(7, 9).Radius(8).Gap(8).AlignItems(ui.Center).
				Background(p.cardSubtle).Border(1, p.sep).Children(func() {
				k.Text(c, `const accent = "#2B7FFF";`, desktopkit.TextProps{Role: desktopkit.RoleCaption, Monospace: true}).FontSize(11).Grow(1).MinWidth(0).SingleLine()
				ui.Row(c.Key("swatch-chip")).Padding(3, 6, 3, 4).Radius(6).Gap(5).AlignItems(ui.Center).Shrink(0).
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
			// Peeking second card underneath for realistic BoardUI stack depth (symmetric inset).
			ui.Box(c.Key("pipe-back-wrap")).FillWidth().Padding(0, 10, 0).Children(func() {
				ui.Row(c.Key("pipe-back")).FillWidth().Padding(7, 11).Radius(10).Gap(8).AlignItems(ui.Center).
					Background(p.card.Alpha(.84)).Border(1, p.cardBorder).Children(func() {
					k.Icon(c, desktopkit.IconImage, 13).TextColor(k.SwatchColor("lime"))
					k.Text(c, "boardui-preview@2x.png · 1440×900", desktopkit.TextProps{Role: desktopkit.RoleCaption, Tone: desktopkit.ToneSecondary}).FontSize(10.5).Grow(1).MinWidth(0).SingleLine()
					tagChip(c.Key("t-img"), k, "lime", tr("图片"))
				})
			})
		}
	})
}

func renderSceneActions(c *ui.Context, k desktopkit.Kit, p setupColors, tr func(string) string, compact bool) {
	ui.Column(c.Key("scene-actions")).FillWidth().Gap(9).Children(func() {
		// Selected clipboard snapshot entry.
		ui.Row(c.Key("act-entry")).FillWidth().Padding(9, 11).Radius(11).Gap(9).AlignItems(ui.Center).
			Background(p.card).Border(1, p.cardBorder).Children(func() {
			bg, ink := k.TagTint("orange")
			iconTile(c.Key("file-ic"), k, 28, 7, bg, ink, desktopkit.IconFile, 14)
			ui.Column(c).Grow(1).MinWidth(0).Gap(1).Children(func() {
				k.Text(c, "release-assets-v2.zip", desktopkit.TextProps{Role: desktopkit.RoleCaption, Emphasized: true}).FontSize(11.8).SingleLine()
				k.Text(c, tr("12.4 MB · 目录快照"), desktopkit.TextProps{Role: desktopkit.RoleCaption, Tone: desktopkit.ToneTertiary}).FontSize(10.5).SingleLine()
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
				ui.Row(c.Key("act-h")).FillWidth().Padding(2, 8, 3).AlignItems(ui.Center).Children(func() {
					k.Text(c, tr("对当前条目执行动作 · Actions"), desktopkit.TextProps{Role: desktopkit.RoleCaption, Tone: desktopkit.ToneTertiary, Emphasized: true}).FontSize(10.5)
					ui.Spacer(c)
					k.Text(c, "⌘K", desktopkit.TextProps{Role: desktopkit.RoleCaption, Tone: desktopkit.ToneTertiary, Monospace: true}).FontSize(10)
				})
			}

			// Highlighted active Action: 临时分享 (generic, no third-party brand).
			ui.Row(c.Key("act-share")).FillWidth().Padding(7, 8).Radius(8).Gap(8).AlignItems(ui.Center).
				Background(p.accentSoft).Border(1, p.accentEdge).Children(func() {
				iconTile(c, k, 22, 6, k.Color("accent-500"), k.Color("text-white"), desktopkit.IconSend, 12)
				ui.Column(c).Grow(1).MinWidth(0).Gap(1).Children(func() {
					k.Text(c, tr("临时分享"), desktopkit.TextProps{Role: desktopkit.RoleCaption, Emphasized: true}).FontSize(11.5).SingleLine()
					k.Text(c, tr("生成取件链接 · 阅后过期"), desktopkit.TextProps{Role: desktopkit.RoleCaption, Tone: desktopkit.ToneTertiary}).FontSize(10).SingleLine()
				})
				statusPill(c.Key("shared"), k, p, pillPositive, tr("链接已生成"), true)
			})

			// Secondary Action: builtin.text.
			ui.Row(c.Key("act-text")).FillWidth().Padding(6, 8).Radius(8).Gap(8).AlignItems(ui.Center).Children(func() {
				iconTile(c, k, 22, 6, p.inset, k.Color("text-secondary"), desktopkit.IconEdit, 12)
				ui.Column(c).Grow(1).MinWidth(0).Gap(1).Children(func() {
					k.Text(c, tr("文本清洗 / JSON 格式化"), desktopkit.TextProps{Role: desktopkit.RoleCaption, Emphasized: true}).FontSize(11.5).SingleLine()
					k.Text(c, "builtin.text", desktopkit.TextProps{Role: desktopkit.RoleCaption, Tone: desktopkit.ToneTertiary, Monospace: true}).FontSize(10).SingleLine()
				})
			})

			if !compact {
				// Tertiary Action: Lua custom plugin.
				ui.Row(c.Key("act-lua")).FillWidth().Padding(6, 8).Radius(8).Gap(8).AlignItems(ui.Center).Children(func() {
					iconTile(c, k, 22, 6, p.inset, k.Color("text-secondary"), desktopkit.IconCode, 12)
					ui.Column(c).Grow(1).MinWidth(0).Gap(1).Children(func() {
						k.Text(c, tr("自定义 Lua 处理脚本"), desktopkit.TextProps{Role: desktopkit.RoleCaption, Emphasized: true}).FontSize(11.5).SingleLine()
						k.Text(c, tr("插件市场 · 沙箱运行"), desktopkit.TextProps{Role: desktopkit.RoleCaption, Tone: desktopkit.ToneTertiary}).FontSize(10).SingleLine()
					})
				})
			}
		})
	})
}

func renderSceneSync(c *ui.Context, k desktopkit.Kit, p setupColors, tr func(string) string, compact bool) {
	ui.Column(c.Key("scene-sync")).FillWidth().Gap(0).Children(func() {
		// Top local hub card using the official pixel-cat logo.
		hub := ui.Row(c.Key("sync-hub")).FillWidth().Padding(9, 11).Radius(12).Gap(10).AlignItems(ui.Center).
			Background(p.card).Border(1, p.cardBorder)
		hub.Draw(func(pt *ui.Painter, rc ui.Rect) {
			pt.Shadow(rc, 12, 0, 6, 18, 0, p.shadowFloat)
		})
		hub.Children(func() {
			brandLogo(c.Key("hub-logo"), k, 30, 8, false)
			ui.Column(c).Grow(1).MinWidth(0).Gap(1).Children(func() {
				k.Text(c, tr("MacBook Pro（本机）"), desktopkit.TextProps{Role: desktopkit.RoleCaption, Emphasized: true}).FontSize(11.8).SingleLine()
				k.Text(c, tr("设备身份已就绪 · 仅本地存储"), desktopkit.TextProps{Role: desktopkit.RoleCaption, Tone: desktopkit.ToneTertiary}).FontSize(10.5).SingleLine()
			})
			statusPill(c.Key("direct"), k, p, pillPositive, tr("直连"), true)
		})

		// Painted dashed P2P branch bridge with centered Tailcat encrypted pill.
		bridgeH := float32(42)
		if compact {
			bridgeH = 30
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
		})
		bridge.Children(func() {
			ui.Row(c.Key("tailcat-pill")).Height(22).PaddingX(9).Radius(11).Gap(5).AlignItems(ui.Center).
				Background(p.card).Border(1, p.accentEdge).Children(func() {
				ui.Box(c).Size(6, 6).Radius(3).Background(k.Color("accent-500")).Shrink(0).AccessHidden(true)
				k.Text(c, tr("Tailcat 端到端加密 · 免云账号"), desktopkit.TextProps{Role: desktopkit.RoleCaption, Emphasized: true}).FontSize(10.5).TextColor(p.accentInk).NoWrap()
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
	ui.Column(c).Grow(1).MinWidth(0).Padding(9, 10).Radius(10).Gap(4).
		Background(p.card).Border(1, p.cardBorder).Children(func() {
		ui.Row(c).FillWidth().AlignItems(ui.Center).Gap(6).Children(func() {
			k.Text(c, name, desktopkit.TextProps{Role: desktopkit.RoleCaption, Emphasized: true}).FontSize(11.5).Grow(1).MinWidth(0).SingleLine()
			ui.Box(c).Size(6, 6).Radius(3).Background(p.okDot).Shrink(0).AccessHidden(true)
		})
		ui.Row(c).FillWidth().AlignItems(ui.Center).Gap(4).Children(func() {
			k.Text(c, leftMeta, desktopkit.TextProps{Role: desktopkit.RoleCaption, Tone: desktopkit.ToneTertiary}).FontSize(10.5).Grow(1).MinWidth(0).SingleLine()
			k.Text(c, rightMeta, desktopkit.TextProps{Role: desktopkit.RoleCaption, Tone: desktopkit.ToneTertiary, Monospace: true}).FontSize(10.5).NoWrap()
		})
	})
}

func omniToken(c *ui.Context, k desktopkit.Kit, color desktopkit.TagColor, text string) {
	bg, ink := k.TagTint(color)
	ui.Box(c).Padding(2, 6).Radius(5).Background(bg).Shrink(0).Children(func() {
		k.Text(c, text, desktopkit.TextProps{Role: desktopkit.RoleCaption, Emphasized: true}).FontSize(10.5).TextColor(ink).NoWrap()
	})
}

func renderWelcomeAction(c *ui.Context, k desktopkit.Kit, p setupColors, m Model, a Actions, tr func(string) string, compact bool) {
	col := ui.Column(c.Key("welcome-action")).Grow(1).MinWidth(0).MinHeight(0).Justify(ui.Center)
	if compact {
		col.FillWidth().Padding(16, 36, 20).AlignItems(ui.Center).Gap(0)
	} else {
		col.FillHeight().Padding(32, 36, 28).AlignItems(ui.Start).Gap(0)
	}

	col.Children(func() {
		// Brand logo + UniClipboard v2 eyebrow.
		logoSize, logoRadius := float32(44), float32(11)
		if compact {
			logoSize, logoRadius = 34, 9
		}
		ui.Row(c.Key("brand-row")).Gap(11).AlignItems(ui.Center).Margin(0, 0, 12).Children(func() {
			brandLogo(c.Key("logo"), k, logoSize, logoRadius, true)
			k.Text(c.Key("eyebrow"), "UniClipboard v2", desktopkit.TextProps{Role: desktopkit.RoleCaption, Tone: desktopkit.ToneTertiary, Emphasized: true}).FontSize(11.5)
		})

		// Exact 0026 product heading and slogan.
		heading := k.Text(c.Key("heading"), tr("欢迎使用 UniClipboard"), desktopkit.TextProps{Role: desktopkit.RoleTitle, Emphasized: true}).FontSize(23)
		slogan := k.Text(c.Key("slogan"), tr("让每一次复制更有用"), desktopkit.TextProps{Tone: desktopkit.ToneSecondary}).FontSize(14).Margin(4, 0, 18)
		if compact {
			heading.TextAlign(ui.Center)
			slogan.TextAlign(ui.Center).Margin(4, 0, 14)
		}

		// Contextual non-intrusive v1 detection card.
		renderWelcomeDetectCard(c, k, p, m, a, tr, compact)

		// Exact 0026 primary CTA.
		ui.Row(c.Key("cta-row")).Gap(12).AlignItems(ui.Center).Children(func() {
			actionButton(c, k, "begin", tr("开始使用"), desktopkit.Primary, desktopkit.SizeLarge, "", m.PendingStart || m.Entering, m.PendingStart || m.Entering, a.Begin)
			if m.Entering {
				k.Text(c.Key("starting"), tr("正在启动应用…"), desktopkit.TextProps{Role: desktopkit.RoleCaption, Tone: desktopkit.ToneSecondary})
			}
		})
	})
}

func renderWelcomeDetectCard(c *ui.Context, k desktopkit.Kit, p setupColors, m Model, a Actions, tr func(string) string, compact bool) {
	if !m.Detecting && !m.Found && m.Error == "" {
		return
	}
	card := ui.Row(c.Key("detection")).FillWidth().Padding(10, 12).Radius(10).Gap(10).AlignItems(ui.Start).Margin(0, 0, 18)
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
				k.Text(c.Key("detecting"), tr("正在检查这台电脑上的 v1 数据…"), desktopkit.TextProps{Role: desktopkit.RoleCaption, Emphasized: true}).FontSize(12.2)
				k.Text(c, tr("仅检查默认目录结构，不会读取剪贴板或请求系统凭据。"), desktopkit.TextProps{Role: desktopkit.RoleCaption, Tone: desktopkit.ToneSecondary}).FontSize(11.3)
			})
		})
	case m.Error != "":
		card.Background(p.errBg).Border(1, p.errEdge).Children(func() {
			iconTile(c, k, 28, 7, p.card, p.errFg, desktopkit.IconInfo, 15).Border(1, p.errEdge)
			ui.Column(c).Grow(1).MinWidth(0).Gap(4).Children(func() {
				k.Text(c.Key("detection-error"), tr(m.Error), desktopkit.TextProps{Role: desktopkit.RoleCaption, Emphasized: true}).FontSize(12).TextColor(p.errFg)
				ui.Row(c.Key("err-acts")).Gap(10).AlignItems(ui.Center).Children(func() {
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
					k.Text(c.Key("detected"), tr("发现 UniClipboard v1 的数据"), desktopkit.TextProps{Role: desktopkit.RoleCaption, Emphasized: true}).FontSize(12.2)
					statusPill(c.Key("importable"), k, p, pillPositive, tr("可导入"), true)
				})
				k.Text(c, tr("点击「开始使用」后可确认导入旧历史，或选择暂不迁移。"), desktopkit.TextProps{Role: desktopkit.RoleCaption, Tone: desktopkit.ToneSecondary}).FontSize(11.3)
			})
		})
	}
}
