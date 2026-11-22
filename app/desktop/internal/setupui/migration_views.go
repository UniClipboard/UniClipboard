package setupui

import (
	"fmt"

	"github.com/egoist/mygo/ui"

	"github.com/uniclipboard/unic/app/desktop/internal/desktopkit"
)

type journeyStepState int

const (
	stepPending journeyStepState = iota
	stepCurrent
	stepDone
	stepWarning
	stepError
)

type journeyStep struct {
	name  string
	meta  string
	state journeyStepState
}

func renderMigration(c *ui.Context, k desktopkit.Kit, m Model, fields *Fields, a Actions, tr func(string) string, compact bool, topBand float32) {
	p := paletteFor(k)
	steps := journeyStepsFor(m, tr)
	if compact {
		ui.Column(c.Key("mig-compact")).Fill().Background(p.workspace).Children(func() {
			renderCompactRail(c, k, p, steps, topBand)
			ui.Box(c).FillWidth().Height(1).Background(p.sep).Shrink(0)
			renderMigrationWorkspace(c, k, p, m, fields, a, tr, true, 0)
		})
		return
	}
	ui.Row(c.Key("mig-split")).Fill().AlignItems(ui.Stretch).Background(p.workspace).Children(func() {
		renderJourneyRail(c, k, p, m, steps, tr, topBand)
		ui.Box(c).Width(1).FillHeight().Background(p.sep).Shrink(0)
		renderMigrationWorkspace(c, k, p, m, fields, a, tr, false, topBand)
	})
}

func journeyStepsFor(m Model, tr func(string) string) []journeyStep {
	totalMeta := tr("逐条写入历史")
	if m.Progress.Total > 0 {
		totalMeta = fmt.Sprintf(tr("共 %d 条"), m.Progress.Total)
	}
	d := []journeyStep{
		{name: tr("确认目录与范围"), meta: tr("等待确认")},
		{name: tr("解锁旧数据"), meta: tr("系统凭据或口令")},
		{name: tr("导入历史"), meta: totalMeta},
		{name: tr("重开校验"), meta: tr("重开库校验")},
	}
	switch m.Stage {
	case "confirm":
		d[0].state, d[0].meta = stepCurrent, tr("已就绪")
	case "invalid":
		d[0].state, d[0].meta = stepError, tr("目录不受支持")
	case "conflict":
		d[0].state, d[0].meta = stepError, tr("目标目录被占用")
	case "credentials":
		d[0].state, d[0].meta = stepDone, tr("已确认")
		d[1].state, d[1].meta = stepCurrent, tr("等待系统授权…")
	case "password":
		d[0].state, d[0].meta = stepDone, tr("已确认")
		d[1].state, d[1].meta = stepWarning, tr("等待输入 v1 口令")
	case "import":
		d[0].state, d[0].meta = stepDone, tr("已确认")
		d[1].state, d[1].meta = stepDone, tr("已解锁")
		d[2].state = stepCurrent
		if m.Progress.Total > 0 {
			d[2].meta = fmt.Sprintf(tr("%d / %d 条"), m.Progress.Processed, m.Progress.Total)
		} else {
			d[2].meta = tr("正在导入…")
		}
	case "verify":
		d[0].state, d[0].meta = stepDone, tr("已确认")
		d[1].state, d[1].meta = stepDone, tr("已解锁")
		d[2].state, d[2].meta = stepDone, tr("已全部处理")
		d[3].state = stepCurrent
		if m.Progress.Total > 0 {
			d[3].meta = fmt.Sprintf(tr("%d / %d 条"), m.Progress.Processed, m.Progress.Total)
		} else {
			d[3].meta = tr("正在校验…")
		}
	case "stopping":
		d[0].state, d[0].meta = stepDone, tr("已确认")
		d[1].state, d[1].meta = stepDone, tr("已解锁")
		if m.Progress.Stage == "verify" {
			d[2].state, d[2].meta = stepDone, tr("已处理")
			d[3].state, d[3].meta = stepWarning, tr("正在安全停止…")
		} else {
			d[2].state, d[2].meta = stepWarning, tr("正在安全停止…")
		}
	case "complete":
		d[0].state, d[0].meta = stepDone, tr("已确认")
		d[1].state, d[1].meta = stepDone, tr("已解锁")
		d[2].state, d[2].meta = stepDone, fmt.Sprintf(tr("已保留 %d 条"), m.Report.Imported+m.Report.Skipped)
		d[3].state, d[3].meta = stepDone, tr("校验通过")
	case "partial":
		d[0].state, d[0].meta = stepDone, tr("已确认")
		d[1].state, d[1].meta = stepDone, tr("已解锁")
		d[2].state, d[2].meta = stepWarning, tr("部分记录失败")
		d[3].state, d[3].meta = stepWarning, tr("存在异常项")
	case "cancelled":
		d[0].state, d[0].meta = stepDone, tr("已确认")
		d[1].state, d[1].meta = stepDone, tr("已解锁")
		d[2].state, d[2].meta = stepWarning, fmt.Sprintf(tr("已停止 · 留存 %d 条"), m.Report.Imported+m.Report.Skipped)
		d[3].meta = tr("尚未校验")
	case "failed":
		d[0].state, d[0].meta = stepDone, tr("已确认")
		d[1].state, d[1].meta = stepDone, tr("已解锁")
		d[2].state, d[2].meta = stepError, tr("读取异常中断")
		d[3].meta = tr("未完成")
	}
	return d
}

func renderJourneyRail(c *ui.Context, k desktopkit.Kit, p setupColors, m Model, steps []journeyStep, tr func(string) string, topBand float32) {
	entryTag := "首次安装引导"
	if m.MigrationOnly {
		entryTag = "设置 › 存储补迁移"
	}
	rail := ui.Column(c.Key("j-rail")).Width(220).FillHeight().Shrink(0).Background(p.sidebar).Gap(0)
	rail.Children(func() {
		// Top header band matches right m-head height (88px with macOS traffic lights, 74px otherwise)
		// so the horizontal divider aligns cleanly across the split seam.
		headBandH := float32(74)
		captionH := float32(0)
		if topBand > 0 {
			headBandH = 88
			captionH = 42
		}
		ui.Column(c.Key("j-top")).FillWidth().Height(headBandH).Shrink(0).Padding(0, 14, 0).Gap(0).Children(func() {
			if captionH > 0 {
				ui.Row(c.Key("j-caption")).FillWidth().Height(captionH).Shrink(0).DragWindow()
			}
			// Product header with official pixel-cat logo.
			headRow := ui.Row(c.Key("j-head")).FillWidth().Grow(1).Gap(9).AlignItems(ui.Center).Padding(0, 4, 2)
			if captionH > 0 {
				headRow.DragWindow()
			}
			headRow.Children(func() {
				brandLogo(c.Key("j-logo"), k, 28, 7, false)
				ui.Column(c).Grow(1).MinWidth(0).Gap(1).Children(func() {
					k.Text(c, tr("从 v1 迁移历史"), desktopkit.TextProps{Role: desktopkit.RoleCallout, Emphasized: true}).FontSize(13).SingleLine()
					k.Text(c, tr(entryTag), desktopkit.TextProps{Role: desktopkit.RoleCaption}).FontSize(11).TextColor(p.textMuted).SingleLine()
				})
			})
		})
		ui.Box(c).FillWidth().Height(1).Background(p.sep).Shrink(0)

		// Vertical 4-stage live stepper + persistent trust footer.
		ui.Column(c.Key("j-body")).Grow(1).FillWidth().MinHeight(0).Padding(12, 14, 14).Gap(10).Children(func() {
			ui.Column(c.Key("j-steps")).Grow(1).FillWidth().Gap(4).Children(func() {
				for i, st := range steps {
					row := ui.Row(c.Key(i)).FillWidth().Padding(8, 9).Radius(9).Gap(9).AlignItems(ui.Start)
					if st.state == stepCurrent {
						row.Background(p.activeStepBg).Border(1, p.activeStepBorder)
					}
					row.Children(func() {
						stepBadge(c.Key("num"), k, p, i+1, st.state, 20)
						ui.Column(c).Grow(1).MinWidth(0).Gap(2).Children(func() {
							nameCol := p.textSub
							emph := false
							if st.state == stepCurrent || st.state == stepDone {
								nameCol = k.Color("text-primary")
								emph = st.state == stepCurrent
							}
							metaCol := p.textMuted
							if st.state == stepCurrent {
								metaCol = p.textSub
							}
							k.Text(c, st.name, desktopkit.TextProps{Role: desktopkit.RoleCaption, Emphasized: emph}).FontSize(12.5).TextColor(nameCol).SingleLine()
							k.Text(c, st.meta, desktopkit.TextProps{Role: desktopkit.RoleCaption}).FontSize(11).TextColor(metaCol).SingleLine()
						})
					})
				}
			})

			ui.Box(c).FillWidth().Height(1).Background(p.sep).Shrink(0)

			// Persistent safety & entry guarantees in left rail.
			ui.Column(c.Key("j-trust")).FillWidth().Gap(7).Padding(2, 2, 0).Children(func() {
				trustRow(c.Key("t-ro"), k, p, desktopkit.IconShield, tr("只读 v1 源目录，不修改也不删除旧数据。"))
				trustRow(c.Key("t-off"), k, p, desktopkit.IconLock, tr("迁移期间离线独占，不捕获剪贴板与同步。"))
				trustRow(c.Key("t-later"), k, p, desktopkit.IconInfo, tr("跳过后随时可在设置 › 存储中再次迁移。"))
			})
		})
	})
}

func renderCompactRail(c *ui.Context, k desktopkit.Kit, p setupColors, steps []journeyStep, topBand float32) {
	ui.Column(c.Key("j-rail-compact-wrap")).FillWidth().Background(p.sidebar).Children(func() {
		if topBand > 0 {
			ui.Row(c.Key("j-caption-compact")).FillWidth().Height(topBand).Shrink(0).DragWindow()
		}
		ui.Row(c.Key("j-rail-compact")).FillWidth().Padding(10, 18).Gap(8).AlignItems(ui.Center).Children(func() {
			for i, st := range steps {
				item := ui.Row(c.Key(i)).Grow(1).MinWidth(0).Padding(5, 7).Radius(7).Gap(6).AlignItems(ui.Center)
				if st.state == stepCurrent {
					item.Background(p.activeStepBg).Border(1, p.activeStepBorder)
				}
				item.Children(func() {
					stepBadge(c.Key("num"), k, p, i+1, st.state, 18)
					nameCol := p.textSub
					emph := st.state == stepCurrent
					if st.state == stepCurrent || st.state == stepDone {
						nameCol = k.Color("text-primary")
					}
					k.Text(c, st.name, desktopkit.TextProps{Role: desktopkit.RoleCaption, Emphasized: emph}).FontSize(11.5).TextColor(nameCol).SingleLine()
				})
			}
		})
	})
}

func stepBadge(c *ui.Context, k desktopkit.Kit, p setupColors, num int, state journeyStepState, size float32) {
	bg, fg := k.Color("background-tertiary-default"), p.textMuted
	if k.Dark {
		bg = ui.RGBA(255, 255, 255, .10)
	}
	switch state {
	case stepCurrent:
		bg, fg = k.Color("accent-500"), k.Color("text-white")
	case stepDone:
		bg, fg = p.okBg, p.okFg
	case stepWarning:
		bg, fg = p.warnPillBg, p.warnFg
	case stepError:
		bg, fg = p.errBg, p.errFg
	}
	ui.Box(c).Size(size, size).Radius(size / 2).Background(bg).Center().Shrink(0).Children(func() {
		label := fmt.Sprint(num)
		if state == stepDone {
			label = "✓"
		}
		k.Text(c, label, desktopkit.TextProps{Role: desktopkit.RoleCaption, Emphasized: true}).FontSize(10.5).TextColor(fg)
	})
}

func trustRow(c *ui.Context, k desktopkit.Kit, p setupColors, icon desktopkit.IconName, text string) {
	ui.Row(c).FillWidth().Gap(7).AlignItems(ui.Start).Children(func() {
		k.Icon(c, icon, 13).TextColor(p.textMuted).Margin(1, 0, 0)
		k.Text(c, text, desktopkit.TextProps{Role: desktopkit.RoleCaption}).FontSize(11).TextColor(p.textMuted).Grow(1).MinWidth(0)
	})
}

func renderMigrationWorkspace(c *ui.Context, k desktopkit.Kit, p setupColors, m Model, fields *Fields, a Actions, tr func(string) string, compact bool, topBand float32) {
	title, sub, pillLabel, pillKind := workspaceHeaderFor(m, tr)
	padX := float32(26)
	if compact {
		padX = 20
	}

	pane := ui.Column(c.Key("m-pane")).Grow(1).MinWidth(0).MinHeight(0).Background(p.workspace)
	if !compact {
		pane.FillHeight()
	} else {
		pane.FillWidth()
	}
	pane.Children(func() {
		// Top workspace header band (draggable on macOS when setup owns the window top).
		headBandH := float32(74)
		if topBand > 0 && !compact {
			headBandH = 88
		}
		head := ui.Row(c.Key("m-head")).FillWidth().Shrink(0).Gap(12).AlignItems(ui.Center)
		if !compact {
			head.Height(headBandH).Padding(0, padX, 0)
			if topBand > 0 {
				head.Padding(12, padX, 0).DragWindow()
			}
		} else {
			head.Padding(14, padX, 12)
		}
		head.Children(func() {
			ui.Column(c).Grow(1).MinWidth(0).Gap(3).Children(func() {
				k.Text(c.Key("head-title"), title, desktopkit.TextProps{Role: desktopkit.RoleHeadline, Emphasized: true}).FontSize(17).LetterSpacing(-0.2)
				k.Text(c.Key("head-sub"), sub, desktopkit.TextProps{Role: desktopkit.RoleCaption}).FontSize(12.5).TextColor(p.textSub)
			})
			if pillLabel != "" {
				statusPill(c.Key("head-pill"), k, p, pillKind, pillLabel, true)
			}
		})
		ui.Box(c).FillWidth().Height(1).Background(p.sep).Shrink(0)

		// Scrollable main content area.
		k.Scroll(c.Key("body-scroll")).Grow(1).MinHeight(0).FillWidth().Children(func() {
			ui.Column(c.Key("body")).FillWidth().Padding(16, padX, 18).Gap(14).Children(func() {
				switch m.Stage {
				case "confirm", "invalid":
					renderConfirmBody(c, k, p, m, a, tr)
				case "conflict":
					renderConflictBody(c, k, p, tr)
				case "credentials":
					renderCredentialsBody(c, k, p, m, tr)
				case "password":
					renderPasswordBody(c, k, p, m, fields, a, tr)
				case "import", "verify", "stopping":
					renderProgressBody(c, k, p, m, tr)
				case "complete", "partial", "cancelled", "failed":
					renderResultBody(c, k, p, m, tr)
				}
				if m.Entering {
					semanticBanner(c.Key("starting"), k, p, desktopkit.ToneInfo, desktopkit.IconInfo, tr("正在启动应用…"), "")
				}
			})
		})

		ui.Box(c).FillWidth().Height(1).Background(p.sep).Shrink(0)

		// Docked footer action bar.
		renderMigrationFooter(c, k, p, m, fields, a, tr, compact)
	})
}

func workspaceHeaderFor(m Model, tr func(string) string) (title, sub, pillLabel string, tone pillTone) {
	switch m.Stage {
	case "confirm", "invalid":
		subText := "导入完成后即可在 v2 历史中直接使用旧记录。"
		if m.MigrationOnly {
			subText = "桌面托管的后台服务已安全暂停，确认源目录即可独占导入。"
		}
		return tr("确认导入范围与目录"), tr(subText), "", pillNeutral
	case "conflict":
		return tr("无法获取目录独占权"), tr("需要释放目标数据目录后才能继续。"), tr("目录被占用"), pillError
	case "credentials":
		return tr("解锁 v1 加密数据"), tr("只读获取解密密钥，不会修改系统凭据。"), tr("等待授权"), pillInfo
	case "password":
		return tr("解锁 v1 加密数据"), tr("在当前流程内完成口令兜底解锁。"), tr("需口令"), pillWarning
	case "import":
		return tr("正在迁移 v1 历史"), tr("导入与校验分阶段进行，完成后才会呈现结果。"), tr("导入阶段"), pillInfo
	case "verify":
		return tr("正在迁移 v1 历史"), tr("导入与校验分阶段进行，完成后才会呈现结果。"), tr("校验阶段"), pillPositive
	case "stopping":
		return tr("正在安全停止迁移"), tr("等待当前操作落盘并释放存储锁。"), tr("正在停止"), pillNeutral
	case "complete":
		subText := "导入与校验均已结束，你可以进入应用开始使用。"
		if m.Report.Imported == 0 && m.Report.Skipped > 0 {
			subText = "重复迁移复用持久映射，不会产生重复记录。"
		}
		return tr("迁移结果"), tr(subText), tr("已校验完成"), pillPositive
	case "partial":
		return tr("迁移结果"), tr("部分记录或附件未能完整恢复，已导入部分不受影响。"), tr("部分完成"), pillWarning
	case "cancelled":
		return tr("迁移结果"), tr("迁移已安全停止并释放存储锁。"), tr("已停止"), pillNeutral
	default:
		return tr("迁移结果"), tr("导入过程因源数据异常中止，已提交记录未回滚。"), tr("未完成"), pillError
	}
}

func renderConfirmBody(c *ui.Context, k desktopkit.Kit, p setupColors, m Model, a Actions, tr func(string) string) {
	invalid := m.Stage == "invalid"

	// Block 1: Source directory card with inline validation footer strip.
	srcBorder := p.cardBorder
	if invalid {
		srcBorder = p.errEdge
	}
	ui.Column(c.Key("source")).FillWidth().Radius(12).Clip().Background(p.card).Border(1, srcBorder).Children(func() {
		ui.Row(c.Key("source-row")).FillWidth().Padding(12, 14).Gap(12).AlignItems(ui.Center).Children(func() {
			icBg, icFg := p.inset, p.textSub
			if invalid {
				icBg, icFg = p.errBg, p.errFg
			}
			iconTile(c.Key("folder-ic"), k, 34, 8, icBg, icFg, desktopkit.IconFolder, 16)
			ui.Column(c).Grow(1).MinWidth(0).Gap(3).Children(func() {
				ui.Row(c).Gap(7).AlignItems(ui.Center).Children(func() {
					k.Text(c.Key("source-label"), tr("v1 数据目录"), desktopkit.TextProps{Role: desktopkit.RoleCaption, Emphasized: true}).FontSize(13)
					if invalid {
						statusPill(c.Key("src-bad"), k, p, pillError, tr("不受支持"), true)
					} else {
						statusPill(c.Key("src-ok"), k, p, pillPositive, tr("结构有效"), true)
					}
				})
				k.Text(c.Key("path"), m.Source, desktopkit.TextProps{Role: desktopkit.RoleCaption, Monospace: true, Truncate: true}).FontSize(11.8).TextColor(p.textSub).FillWidth()
			})
			compactSecondaryButton(c, k, p, "pick", tr("选择其他目录"), m.Entering, a.Pick)
		})
		if m.Error != "" {
			ui.Row(c.Key("invalid")).FillWidth().Padding(9, 14).Gap(7).AlignItems(ui.Center).Background(p.errBg).Border(1, p.errEdge).Children(func() {
				k.Icon(c, desktopkit.IconInfo, 14).TextColor(p.errFg)
				k.Text(c, tr(m.Error), desktopkit.TextProps{Role: desktopkit.RoleCaption}).FontSize(11.8).TextColor(p.errFg).Grow(1).MinWidth(0)
			})
		}
	})

	// Block 2: Unified 2-column scope comparison card (includes favorites & compatible settings).
	ui.Row(c.Key("scope")).FillWidth().AlignItems(ui.Stretch).Radius(12).Clip().Background(p.card).Border(1, p.cardBorder).Children(func() {
		// Left: Included items (6 items in balanced 2x3 layout).
		ui.Column(c.Key("included")).Grow(1).MinWidth(0).Padding(13, 15).Gap(10).Children(func() {
			ui.Row(c).FillWidth().AlignItems(ui.Center).Children(func() {
				k.Text(c, tr("将会导入"), desktopkit.TextProps{Role: desktopkit.RoleCaption, Emphasized: true}).FontSize(12.5).TextColor(p.textSub)
				ui.Spacer(c)
				statusPill(c.Key("inc-pill"), k, p, pillPositive, tr("历史与偏好"), true)
			})
			scopeGrid(c.Key("inc-grid"), k, p, tr, []string{"文本与 HTML", "图片", "文件", "标签及关联", "收藏", "兼容的设置"}, true)
			ui.Box(c).FillWidth().Height(1).Background(p.sep).Shrink(0)
			ui.Row(c).Gap(6).AlignItems(ui.Center).Children(func() {
				k.Icon(c, desktopkit.IconShield, 13).TextColor(p.textMuted)
				k.Text(c, tr("只读 v1 数据，不修改也不删除旧文件"), desktopkit.TextProps{Role: desktopkit.RoleCaption}).FontSize(11.2).TextColor(p.textMuted).Grow(1).MinWidth(0)
			})
		})

		ui.Box(c).Width(1).FillHeight().Background(p.sep).Shrink(0)

		// Right: Excluded items (recessed subtle background).
		ui.Column(c.Key("excluded")).Grow(1).MinWidth(0).Padding(13, 15).Gap(10).Background(p.cardSubtle).Children(func() {
			ui.Row(c).FillWidth().AlignItems(ui.Center).Children(func() {
				k.Text(c, tr("不会导入"), desktopkit.TextProps{Role: desktopkit.RoleCaption, Emphasized: true}).FontSize(12.5).TextColor(p.textSub)
				ui.Spacer(c)
				statusPill(c.Key("exc-pill"), k, p, pillNeutral, tr("需重新建立"), false)
			})
			scopeGrid(c.Key("exc-grid"), k, p, tr, []string{"设备身份", "设备信任", "不兼容设置", "旧版同步配置"}, false)
			ui.Spacer(c)
			ui.Box(c).FillWidth().Height(1).Background(p.sep).Shrink(0)
			ui.Row(c).Gap(6).AlignItems(ui.Center).Children(func() {
				k.Icon(c, desktopkit.IconDevices, 13).TextColor(p.textMuted)
				k.Text(c, tr("其他设备需升级到 v2 后重新配对"), desktopkit.TextProps{Role: desktopkit.RoleCaption}).FontSize(11.2).TextColor(p.textMuted).Grow(1).MinWidth(0)
			})
		})
	})

	// Block 3: Single high-priority amber pre-flight reminder.
	semanticBanner(c.Key("quit-v1"), k, p, desktopkit.ToneWarning, desktopkit.IconInfo,
		tr("开始前，请先退出 UniClipboard v1。"),
		tr("应用不会自动退出旧版进程；若 v1 仍在运行并回收文件，可能导致个别附件缺失。"))
}

func scopeGrid(c *ui.Context, k desktopkit.Kit, p setupColors, tr func(string) string, items []string, included bool) {
	markBg, markFg, markGlyph := p.inset, p.textMuted, "–"
	textCol := p.textSub
	if included {
		markBg, markFg, markGlyph = p.okBg, p.okFg, "✓"
		textCol = k.Color("text-primary")
	}
	ui.Column(c).FillWidth().Gap(7).Children(func() {
		for i := 0; i < len(items); i += 2 {
			ui.Row(c.Key(i)).FillWidth().Gap(10).Children(func() {
				for j := i; j < min(i+2, len(items)); j++ {
					ui.Row(c.Key(items[j])).Grow(1).MinWidth(0).Gap(6).AlignItems(ui.Center).Children(func() {
						ui.Box(c).Size(15, 15).Radius(7.5).Background(markBg).Center().Shrink(0).AccessHidden(true).Children(func() {
							k.Text(c, markGlyph, desktopkit.TextProps{Role: desktopkit.RoleCaption, Emphasized: true}).FontSize(9.5).TextColor(markFg)
						})
						k.Text(c, tr(items[j]), desktopkit.TextProps{Role: desktopkit.RoleCaption}).FontSize(12.2).TextColor(textCol).SingleLine()
					})
				}
			})
		}
	})
}

func renderConflictBody(c *ui.Context, k desktopkit.Kit, p setupColors, tr func(string) string) {
	ui.Column(c.Key("conflict")).FillWidth().Padding(18).Radius(12).Gap(14).Background(p.card).Border(1, p.cardBorder).Children(func() {
		ui.Row(c).FillWidth().Gap(12).AlignItems(ui.Start).Children(func() {
			iconTile(c, k, 40, 10, p.errBg, p.errFg, desktopkit.IconLock, 18)
			ui.Column(c).Grow(1).MinWidth(0).Gap(4).Children(func() {
				k.Text(c, tr("另一个 unic 服务正在使用目标数据目录"), desktopkit.TextProps{Role: desktopkit.RoleHeadline, Emphasized: true}).FontSize(14.5)
				k.Text(c, tr("离线迁移需要独占目标目录锁。请先手动停止正在运行的独立 unic 服务，然后重试。应用不会自动终止独立服务。"), desktopkit.TextProps{Role: desktopkit.RoleCaption, Tone: desktopkit.ToneSecondary}).FontSize(12.2)
			})
		})
		semanticBanner(c.Key("no-credential"), k, p, desktopkit.ToneInfo, desktopkit.IconShield,
			tr("安全边界"),
			tr("本次预检在读取系统凭据前已中止，没有读取凭据，也没有导入任何记录。"))
	})
}

func renderCredentialsBody(c *ui.Context, k desktopkit.Kit, p setupColors, m Model, tr func(string) string) {
	ui.Column(c.Key("progress-panel")).FillWidth().Padding(18).Radius(12).Gap(14).Background(p.card).Border(1, p.cardBorder).Children(func() {
		ui.Row(c).FillWidth().Gap(12).AlignItems(ui.Start).Children(func() {
			ui.Box(c).Size(40, 40).Radius(10).Background(p.accentSoft).Border(1, p.accentEdge).Center().Shrink(0).Children(func() {
				ui.Spinner(c)
			})
			ui.Column(c).Grow(1).MinWidth(0).Gap(4).Children(func() {
				k.Text(c.Key("credentials"), tr("正在读取系统凭据"), desktopkit.TextProps{Role: desktopkit.RoleHeadline, Emphasized: true}).FontSize(14.5)
				k.Text(c.Key("credential-hint"), tr("系统可能请求访问 UniClipboard 凭据。请在系统窗口中允许访问。"), desktopkit.TextProps{Role: desktopkit.RoleCaption, Tone: desktopkit.ToneSecondary}).FontSize(12.2)
			})
		})
		if m.Source != "" {
			ui.Column(c.Key("src-tile")).FillWidth().Padding(10, 12).Radius(9).Gap(2).Background(p.cardSubtle).Border(1, p.sep).Children(func() {
				k.Text(c, tr("正在解锁源目录"), desktopkit.TextProps{Role: desktopkit.RoleCaption, Tone: desktopkit.ToneTertiary}).FontSize(11)
				k.Text(c, m.Source, desktopkit.TextProps{Role: desktopkit.RoleCaption, Monospace: true, Truncate: true}).FontSize(11.8).FillWidth()
			})
		}
	})
}

func renderPasswordBody(c *ui.Context, k desktopkit.Kit, p setupColors, m Model, fields *Fields, a Actions, tr func(string) string) {
	ui.Column(c.Key("progress-panel")).FillWidth().Padding(18).Radius(12).Gap(14).Background(p.card).Border(1, p.cardBorder).Children(func() {
		ui.Row(c).FillWidth().Gap(12).AlignItems(ui.Start).Children(func() {
			iconTile(c, k, 40, 10, p.warnPillBg, p.warnFg, desktopkit.IconLock, 18)
			ui.Column(c).Grow(1).MinWidth(0).Gap(3).Children(func() {
				ui.Row(c).FillWidth().AlignItems(ui.Center).Gap(8).Children(func() {
					k.Text(c.Key("password-heading"), tr("输入 v1 口令以继续"), desktopkit.TextProps{Role: desktopkit.RoleHeadline, Emphasized: true}).FontSize(14.5)
					ui.Spacer(c)
					actionButton(c, k, "retry-cred", tr("重试系统凭据"), desktopkit.Link, desktopkit.SizeSmall, "", m.Entering, false, a.Start)
				})
				k.Text(c.Key("password-hint"), tr("系统凭据无法解锁 v1 数据。请输入旧版口令。口令只用于本次解锁，不会保存。"), desktopkit.TextProps{Role: desktopkit.RoleCaption, Tone: desktopkit.ToneSecondary}).FontSize(12)
			})
		})

		field := k.FormField(c.Key("passphrase"), desktopkit.FormFieldProps{
			Label:      tr("v1 口令"),
			Value:      &fields.Passphrase,
			Password:   true,
			RevealShow: tr("显示口令"),
			RevealHide: tr("隐藏口令"),
			Hint:       tr("口令仅在内存中用于本次解密，绝不进入命令参数、环境变量、磁盘或日志。"),
			Error:      tr(m.Error),
			AutoFocus:  true,
		})
		field.Element.FillWidth()
		if field.Submitted && fields.Passphrase != "" && a.Unlock != nil {
			a.Unlock()
		}
	})
}

func renderProgressBody(c *ui.Context, k desktopkit.Kit, p setupColors, m Model, tr func(string) string) {
	isVerify := m.Stage == "verify" || (m.Stage == "stopping" && m.Progress.Stage == "verify")
	isStop := m.Stage == "stopping"

	title, hint := "正在导入记录", "按条写入本地历史库，不估算剩余时间；导入 100% 后将自动进入校验阶段。"
	if m.Stage == "verify" {
		title, hint = "正在校验导入的内容", "关闭并重新打开目标库，通过公开接口逐条比对内容摘要、标签、收藏与兼容设置。"
	}
	if isStop {
		title, hint = "正在安全停止", "正在等待当前记录落盘并释放存储锁。所有已提交的记录都会完整保留。"
	}

	ui.Column(c.Key("progress-panel")).FillWidth().Padding(18).Radius(12).Gap(13).Background(p.card).Border(1, p.cardBorder).Children(func() {
		// 2-phase explicit visual strip: 1. 导入记录 -> 2. 重开校验.
		ui.Row(c.Key("phase-track")).FillWidth().Padding(8, 12).Radius(9).Gap(8).AlignItems(ui.Center).
			Background(p.cardSubtle).Border(1, p.sep).Children(func() {
			p1Col := k.Color("text-primary")
			p1Prefix := "1."
			if isVerify {
				p1Col = p.okFg
				p1Prefix = "✓"
			}
			k.Text(c, p1Prefix+" "+tr("导入记录"), desktopkit.TextProps{Role: desktopkit.RoleCaption, Emphasized: !isVerify}).FontSize(11.8).TextColor(p1Col)
			k.Text(c, "→", desktopkit.TextProps{Role: desktopkit.RoleCaption, Tone: desktopkit.ToneTertiary}).FontSize(11)
			p2Col := k.Color("text-tertiary")
			if isVerify && !isStop {
				p2Col = k.Color("text-primary")
			}
			k.Text(c, "2. "+tr("重开校验"), desktopkit.TextProps{Role: desktopkit.RoleCaption, Emphasized: isVerify && !isStop}).FontSize(11.8).TextColor(p2Col)
		})

		// Phase icon + title + honest description.
		ui.Row(c).FillWidth().Gap(12).AlignItems(ui.Start).Children(func() {
			icBg, icFg, ic := p.accentSoft, p.accentInk, desktopkit.IconDownload
			if isVerify {
				icBg, icFg, ic = p.okBg, p.okFg, desktopkit.IconShield
			}
			if isStop {
				icBg, icFg, ic = k.Color("background-tertiary-default"), k.Color("text-secondary"), desktopkit.IconRemove
			}
			iconTile(c, k, 40, 10, icBg, icFg, ic, 18)
			ui.Column(c).Grow(1).MinWidth(0).Gap(3).Children(func() {
				ui.Row(c).Gap(7).AlignItems(ui.Center).Children(func() {
					ui.Spinner(c)
					k.Text(c.Key("phase"), tr(title), desktopkit.TextProps{Role: desktopkit.RoleHeadline, Emphasized: true}).FontSize(14.5)
				})
				k.Text(c.Key("phase-hint"), tr(hint), desktopkit.TextProps{Role: desktopkit.RoleCaption, Tone: desktopkit.ToneSecondary}).FontSize(12)
			})
		})

		// Colored rounded progress bar + exact processed/total count.
		val := -1.0
		pctLabel := ""
		if m.Progress.Total > 0 {
			ratio := min(max(float64(m.Progress.Processed)/float64(m.Progress.Total), 0), 1)
			val = ratio
			pctLabel = fmt.Sprintf("%d%%", int(ratio*100+0.5))
		}
		fillCol := k.Color("accent-500")
		if isVerify {
			fillCol = p.okDot
		}
		if isStop {
			fillCol = k.Color("text-tertiary")
		}
		ui.Column(c.Key("bar-wrap")).FillWidth().Gap(7).Children(func() {
			ui.Progress(c.Key("progress"), val).Label(tr(title)).FillWidth().Height(7).Radius(3.5).
				Background(k.Color("background-tertiary-default")).TextColor(fillCol)
			countFmt := "已处理 %d / %d 条"
			if isVerify {
				countFmt = "已校验 %d / %d 条"
			}
			ui.Row(c).FillWidth().AlignItems(ui.Center).Children(func() {
				k.Text(c.Key("count"), fmt.Sprintf(tr(countFmt), m.Progress.Processed, m.Progress.Total), desktopkit.TextProps{Role: desktopkit.RoleCaption, Tone: desktopkit.ToneSecondary, Monospace: true}).FontSize(12)
				ui.Spacer(c)
				if pctLabel != "" {
					k.Text(c.Key("pct"), pctLabel, desktopkit.TextProps{Role: desktopkit.RoleCaption, Tone: desktopkit.ToneSecondary, Monospace: true, Emphasized: true}).FontSize(12)
				}
			})
		})

		// Live safety summary tiles.
		committed := m.Progress.Processed
		if isVerify && m.Progress.Total > 0 {
			committed = m.Progress.Total
		}
		ui.Row(c.Key("safety-tiles")).FillWidth().Gap(10).Children(func() {
			safetyTile(c.Key("tile-commit"), k, p, tr("已安全提交（中断不丢失）"), fmt.Sprintf(tr("%d 条"), committed), true)
			safetyTile(c.Key("tile-bound"), k, p, tr("运行边界"), tr("离线独占 · 不写剪贴板"), false)
		})
	})
}

func safetyTile(c *ui.Context, k desktopkit.Kit, p setupColors, label, value string, mono bool) {
	ui.Column(c).Grow(1).MinWidth(0).Padding(9, 11).Radius(9).Gap(2).Background(p.cardSubtle).Border(1, p.sep).Children(func() {
		k.Text(c, label, desktopkit.TextProps{Role: desktopkit.RoleCaption}).FontSize(11).TextColor(p.textMuted).SingleLine()
		k.Text(c, value, desktopkit.TextProps{Role: desktopkit.RoleCaption, Emphasized: true, Monospace: mono}).FontSize(12.5).SingleLine()
	})
}

func renderResultBody(c *ui.Context, k desktopkit.Kit, p setupColors, m Model, tr func(string) string) {
	title, lead := "迁移未完成", "已提交的记录不会回滚，仍可在历史中使用。重试会跳过它们。"
	icon := desktopkit.IconDismiss
	heroBg, heroBorder, icBg, icFg := p.errBg.MixOklab(p.card, .45), p.errEdge, p.errBg, p.errFg

	switch m.Stage {
	case "complete":
		title, lead = "迁移完成", "所有支持的历史记录、标签、收藏与兼容设置都已导入并通过校验。"
		if m.Report.Imported == 0 && m.Report.Skipped > 0 {
			lead = "没有新的记录需要导入。此前导入的记录均安然保留在历史中。"
		}
		icon = desktopkit.IconShield
		heroBg, heroBorder, icBg, icFg = p.okBg.MixOklab(p.card, .68), p.okEdge, p.okBg, p.okFg
	case "partial":
		title, lead = "部分数据未能导入", "已导入的记录可以正常使用。问题记录可在重试时再次处理。"
		icon = desktopkit.IconInfo
		heroBg, heroBorder, icBg, icFg = p.warnBg.MixOklab(p.card, .35), p.warnEdge, p.warnPillBg, p.warnFg
	case "cancelled":
		title, lead = "迁移已停止", "已提交的记录已保留并可在历史中使用。继续迁移时会自动跳过它们。"
		icon = desktopkit.IconRemove
		heroBg, heroBorder, icBg, icFg = p.card, p.cardBorder, k.Color("background-tertiary-default"), k.Color("text-secondary")
	}

	// Result hero summary banner.
	ui.Row(c.Key("result")).FillWidth().Padding(15, 16).Radius(12).Gap(13).AlignItems(ui.Start).
		Background(heroBg).Border(1, heroBorder).Children(func() {
		iconTile(c, k, 42, 11, icBg, icFg, icon, 20)
		ui.Column(c).Grow(1).MinWidth(0).Gap(3).Children(func() {
			k.Text(c.Key("title"), tr(title), desktopkit.TextProps{Role: desktopkit.RoleHeadline, Emphasized: true}).FontSize(16)
			k.Text(c.Key("lead"), tr(lead), desktopkit.TextProps{Role: desktopkit.RoleCaption, Tone: desktopkit.ToneSecondary}).FontSize(12.2)
		})
	})

	// Primary positive metrics row.
	type statItem struct {
		label string
		count int
		ok    bool
	}
	primary := []statItem{
		{label: "本次新导入", count: m.Report.Imported, ok: m.Stage == "complete" && m.Report.Imported > 0},
		{label: "此前已导入", count: m.Report.Skipped},
		{label: "已校验", count: m.Report.Verified, ok: m.Stage == "complete" && m.Report.Imported == 0 && m.Report.Verified > 0},
	}
	ui.Row(c.Key("primary-stats")).FillWidth().Gap(10).Children(func() {
		for _, s := range primary {
			border := p.cardBorder
			numCol := k.Color("text-primary")
			if s.ok {
				border = p.okEdge
				numCol = p.okFg
			}
			ui.Column(c.Key(s.label)).Grow(1).MinWidth(0).Padding(12, 14).Radius(10).Gap(3).
				Background(p.card).Border(1, border).Children(func() {
				k.Text(c, fmt.Sprint(s.count), desktopkit.TextProps{Role: desktopkit.RoleTitle, Emphasized: true, Monospace: true}).FontSize(21).TextColor(numCol)
				k.Text(c, tr(s.label), desktopkit.TextProps{Role: desktopkit.RoleCaption, Tone: desktopkit.ToneSecondary}).FontSize(11.5).SingleLine()
			})
		}
	})

	// Nonzero anomaly breakdown chips (separate from positive counts).
	type issueItem struct {
		label    string
		count    int
		critical bool
	}
	issues := make([]issueItem, 0, 5)
	for _, candidate := range []issueItem{
		{label: "失败记录", count: m.Report.Failed, critical: true},
		{label: "缺失内容", count: m.Report.MissingParts, critical: false},
		{label: "校验失败", count: m.Report.VerificationFailed, critical: true},
		{label: "颜色导入失败", count: m.Report.ColorFailures, critical: true},
		{label: "待补回颜色", count: m.Report.ColorsMissing, critical: false},
	} {
		if candidate.count > 0 {
			issues = append(issues, candidate)
		}
	}
	if len(issues) > 0 {
		ui.Row(c.Key("issue-strip")).FillWidth().Wrap().Gap(8).Children(func() {
			for _, it := range issues {
				bg, border, fg := p.warnBg, p.warnEdge, p.warnFg
				if it.critical {
					bg, border, fg = p.errBg, p.errEdge, p.errFg
				}
				ui.Row(c.Key(it.label)).Padding(6, 11).Radius(9).Gap(6).AlignItems(ui.Center).
					Background(bg).Border(1, border).Children(func() {
					k.Text(c, fmt.Sprint(it.count), desktopkit.TextProps{Role: desktopkit.RoleCaption, Emphasized: true, Monospace: true}).FontSize(13).TextColor(fg)
					k.Text(c, tr(it.label), desktopkit.TextProps{Role: desktopkit.RoleCaption, Emphasized: true}).FontSize(11.8).TextColor(fg)
				})
			}
		})
	}

	if m.Report.MissingParts > 0 {
		semanticBanner(c.Key("missing-note"), k, p, desktopkit.ToneInfo, desktopkit.IconInfo, "",
			tr("缺失内容是记录中丢失的图片或文件附件。它与失败记录分开计数。"))
	}
	if m.Stage == "failed" && m.Error != "" {
		semanticBanner(c.Key("result-error"), k, p, desktopkit.ToneError, desktopkit.IconDismiss, "", tr(m.Error))
	}
}

func renderMigrationFooter(c *ui.Context, k desktopkit.Kit, p setupColors, m Model, fields *Fields, a Actions, tr func(string) string, compact bool) {
	footBg := p.sidebar.MixOklab(p.workspace, .55)
	ui.Column(c.Key("m-foot-wrap")).FillWidth().Background(footBg).Children(func() {
		ui.Row(c.Key("actions")).FillWidth().Padding(13, 26).Gap(8).AlignItems(ui.Center).Children(func() {
			switch m.Stage {
			case "confirm", "invalid", "password", "conflict":
				actionButton(c, k, "skip", tr("暂不迁移"), desktopkit.Standard, desktopkit.SizeMedium, "", m.Entering, false, a.Enter)
				ui.Spacer(c)
				switch m.Stage {
				case "password":
					actionButton(c, k, "unlock", tr("解锁并继续"), desktopkit.Primary, desktopkit.SizeMedium, "", fields.Passphrase == "" || m.Entering, false, a.Unlock)
				case "conflict":
					actionButton(c, k, "retry", tr("重试"), desktopkit.Primary, desktopkit.SizeMedium, "", m.Entering, false, a.Start)
				default:
					actionButton(c, k, "start", tr("开始迁移"), desktopkit.Primary, desktopkit.SizeMedium, "", m.Stage == "invalid" || m.Detecting || m.Entering, false, a.Start)
				}
			case "credentials", "import", "verify", "stopping":
				k.Text(c.Key("offline"), tr("迁移期间不会捕获剪贴板，也不会同步"), desktopkit.TextProps{Role: desktopkit.RoleCaption}).FontSize(11.8).TextColor(p.textSub).SingleLine()
				ui.Spacer(c)
				label := "停止迁移"
				if m.Stage == "stopping" {
					label = "正在停止…"
				}
				actionButton(c, k, "stop", tr(label), desktopkit.Standard, desktopkit.SizeMedium, "", m.Stage == "stopping" || m.Entering, m.Stage == "stopping", a.Stop)
			case "complete":
				k.Text(c.Key("no-clip"), tr("不会自动应用导入内容到系统剪贴板"), desktopkit.TextProps{Role: desktopkit.RoleCaption}).FontSize(11.5).TextColor(p.textMuted).SingleLine()
				ui.Spacer(c)
				actionButton(c, k, "enter", tr("开始使用"), desktopkit.Primary, desktopkit.SizeMedium, "", m.Entering, m.Entering, a.Enter)
			case "partial":
				actionButton(c, k, "retry", tr("重试迁移"), desktopkit.Standard, desktopkit.SizeMedium, "", m.Entering, false, a.Start)
				ui.Spacer(c)
				actionButton(c, k, "enter", tr("进入应用"), desktopkit.Primary, desktopkit.SizeMedium, "", m.Entering, m.Entering, a.Enter)
			case "cancelled":
				actionButton(c, k, "enter", tr("进入应用"), desktopkit.Standard, desktopkit.SizeMedium, "", m.Entering, m.Entering, a.Enter)
				ui.Spacer(c)
				actionButton(c, k, "retry", tr("继续迁移"), desktopkit.Primary, desktopkit.SizeMedium, "", m.Entering, false, a.Start)
			case "failed":
				actionButton(c, k, "pick", tr("选择其他目录"), desktopkit.Standard, desktopkit.SizeMedium, "", m.Entering, false, a.Pick)
				actionButton(c, k, "enter", tr("不再迁移，进入应用"), desktopkit.Standard, desktopkit.SizeMedium, "", m.Entering, m.Entering, a.Enter)
				ui.Spacer(c)
				actionButton(c, k, "retry", tr("重试"), desktopkit.Primary, desktopkit.SizeMedium, "", m.Entering, false, a.Start)
			}
		})
		if compact && (m.Stage == "confirm" || m.Stage == "password" || m.Stage == "invalid") {
			k.Text(c.Key("later"), tr("之后可在设置 › 存储 › 从 v1 迁移中导入"), desktopkit.TextProps{Role: desktopkit.RoleCaption}).FontSize(11.2).TextColor(p.textMuted).Padding(0, 22, 10)
		}
	})
}
