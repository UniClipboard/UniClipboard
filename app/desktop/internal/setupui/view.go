// Package setupui renders the first-install prototype with the shared UI Kit.
package setupui

import (
	"fmt"

	"github.com/egoist/mygo/ui"
	"github.com/uniclipboard/unic/app/desktop/internal/i18n"
	"github.com/uniclipboard/unic/app/desktop/internal/uikit"
	"github.com/uniclipboard/unic/internal/v1migration"
)

type Model struct {
	Stage, Source, Error, Language           string
	Detecting, Found, PendingStart, Entering bool
	Progress                                 v1migration.Progress
	Report                                   v1migration.Report
}
type Fields struct{ Passphrase string }
type Actions struct{ Begin, Redetect, Pick, Start, Unlock, Stop, Enter func() }

// Render is shared by the live flow and software-view acceptance.
func Render(c *ui.Context, m Model, fields *Fields, a Actions) {
	k := uikit.For(c.Theme().Dark)
	tr := func(s string) string { return i18n.Translate(m.Language, s) }
	k.UiRoot(c, func() {
		if k.Skin == uikit.MacOS {
			ui.Row(c.Key("titlebar")).Height(38).Center().Children(func() { k.Text(c, "UniClipboard", uikit.TextProps{Tone: uikit.ToneSecondary}) })
		}
		text := func(key, value string, p uikit.TextProps) { k.Text(c.Key(key), tr(value), p) }
		size := uikit.SizeMedium
		if m.Stage == "welcome" || m.Stage == "complete" {
			size = uikit.SizeLarge
		}
		button := func(key, label string, variant uikit.ButtonVariant, disabled bool, action func()) {
			if k.Button(c.Key(key), tr(label), uikit.ButtonProps{Size: size, Variant: variant, Disabled: disabled || action == nil || m.Entering}).Clicked() && action != nil {
				action()
			}
		}
		notice := func(key, title string, tone uikit.NoticeTone) {
			k.InlineNotice(c.Key(key), uikit.NoticeProps{Title: tr(title), Tone: tone})
		}
		secondary := uikit.TextProps{Tone: uikit.ToneSecondary, Role: uikit.RoleCaption}
		if m.Stage == "welcome" {
			ui.Column(c.Key("welcome")).Grow(1).FillWidth().Padding(24, 36).Gap(12).Center().Children(func() {
				ui.Box(c.Key("logo")).Size(76, 76).Radius(20).Background(c.Theme().Accent).Center().Children(func() { k.Icon(c, uikit.IconCopy, 40).TextColor(ui.RGB(255, 255, 255)) })
				k.Text(c.Key("heading"), tr("欢迎使用 UniClipboard"), uikit.TextProps{Role: uikit.RoleTitle}).FontSize(26).Margin(10, 0, 0)
				text("slogan", "让每一次复制更有用", uikit.TextProps{Tone: uikit.ToneSecondary})
				ui.Box(c).Margin(20, 0, 0).Children(func() { button("begin", "开始使用", uikit.Primary, m.PendingStart, a.Begin) })
				ui.Row(c.Key("detection")).Height(34).Gap(6).Center().Children(func() {
					switch {
					case m.Detecting:
						ui.Spinner(c)
						text("detecting", "正在检查这台电脑上的 v1 数据…", secondary)
					case m.Error != "":
						text("detection-error", m.Error, uikit.TextProps{Role: uikit.RoleCaption, Tone: uikit.ToneCritical})
						button("redetect", "重试", uikit.Link, false, a.Redetect)
					case m.Found:
						k.Icon(c, uikit.IconPackage, 14).TextColor(c.Theme().Success)
						text("detected", "发现 UniClipboard v1 的数据", secondary)
					}
				})
				if m.Error != "" {
					button("manual", "手动选择目录", uikit.Link, false, a.Pick)
				}
			})
			if m.Entering {
				notice("starting", "正在启动应用…", uikit.ToneInfo)
			}
			return
		}
		result := m.Stage == "complete" || m.Stage == "partial" || m.Stage == "cancelled" || m.Stage == "failed"
		k.Scroll(c.Key("body-scroll")).Grow(1).MinHeight(0).FillWidth().Children(func() {
			ui.Column(c.Key("body")).FillWidth().Padding(12, 36, 20).Gap(14).Children(func() {
				if result {
					renderResult(c, k, m, tr)
					return
				}
				k.EntityHeader(c.Key("header"), uikit.EntityHeaderProps{Icon: uikit.IconPackage, Title: tr("从 UniClipboard v1 导入历史"), Description: tr("导入后可在历史中直接使用。")}, nil)
				switch m.Stage {
				case "confirm", "invalid":
					panel(c.Key("source"), k, func() {
						ui.Row(c).Gap(10).AlignItems(ui.Center).Children(func() {
							k.Icon(c, uikit.IconFolder, 22)
							ui.Column(c).Grow(1).MinWidth(0).Gap(3).Children(func() {
								text("source-label", "v1 数据目录", secondary)
								k.Text(c.Key("path"), m.Source, uikit.TextProps{Monospace: true, Truncate: true}).FillWidth()
							})
							button("pick", "选择其他目录", uikit.Standard, false, a.Pick)
						})
					})
					if m.Error != "" {
						notice("invalid", m.Error, uikit.ToneError)
					}
					ui.Row(c.Key("scope")).Gap(12).Children(func() {
						scope(c.Key("included"), k, tr, "会导入", []string{"文本与 HTML", "图片", "文件", "标签及其关联"}, true)
						scope(c.Key("excluded"), k, tr, "不会导入", []string{"设置", "收藏", "设备身份", "设备信任关系"}, false)
					})
					notice("pairing", "其他设备需要升级到 v2，然后重新配对。", uikit.ToneInfo)
					notice("preserve", "迁移只读取 v1 数据，不修改也不删除旧数据。", uikit.ToneInfo)
					notice("quit-v1", "开始前，请先退出 UniClipboard v1。应用不会自动退出 v1。", uikit.ToneWarning)
				case "conflict":
					notice("conflict", "另一个 unic 服务正在使用数据目录。请先停止该服务，然后重试。应用不会自动停止它。", uikit.ToneError)
					text("no-credential", "没有读取凭据，也没有导入任何记录。", secondary)
				default:
					phase := 0
					if m.Stage == "import" {
						phase = 1
					}
					if m.Stage == "verify" {
						phase = 2
					}
					if m.Stage == "stopping" {
						phase = 1
						if m.Progress.Stage == "verify" {
							phase = 2
						}
					}
					steps(c.Key("steps"), k, tr, phase)
					panel(c.Key("progress-panel"), k, func() {
						switch m.Stage {
						case "password":
							text("password-heading", "输入 v1 口令", uikit.TextProps{Role: uikit.RoleHeadline})
							text("password-hint", "系统凭据无法解锁 v1 数据。请输入旧版口令。口令只用于本次解锁，不会保存。", secondary)
							field := k.FormField(c.Key("passphrase"), uikit.FormFieldProps{Label: tr("v1 口令"), Value: &fields.Passphrase, Password: true, RevealShow: tr("显示口令"), RevealHide: tr("隐藏口令"), Error: tr(m.Error), AutoFocus: true})
							field.Element.FillWidth()
							if field.Submitted && fields.Passphrase != "" && a.Unlock != nil {
								a.Unlock()
							}
						case "credentials":
							ui.Row(c).Gap(8).Children(func() {
								ui.Spinner(c)
								text("credentials", "正在读取系统凭据", uikit.TextProps{Role: uikit.RoleHeadline})
							})
							text("credential-hint", "系统可能请求访问 UniClipboard 凭据。请在系统窗口中允许访问。", secondary)
						default:
							title, hint := "正在导入记录", "按条写入历史。已写入的记录在中断后保留。"
							if m.Stage == "verify" {
								title, hint = "正在校验导入的内容", "重新打开历史库，逐条比对内容。"
							}
							if m.Stage == "stopping" {
								title, hint = "正在安全停止", "正在等待当前操作结束。已提交的记录会保留。"
							}
							ui.Row(c).Gap(8).Children(func() { ui.Spinner(c); text("phase", title, uikit.TextProps{Role: uikit.RoleHeadline}) })
							text("phase-hint", hint, secondary)
							var percent *float64
							if m.Progress.Total > 0 {
								value := float64(m.Progress.Processed) * 100 / float64(m.Progress.Total)
								percent = &value
							}
							k.ProgressBar(c.Key("progress"), uikit.ProgressBarProps{Label: tr(title), Value: percent}).FillWidth().Margin(10, 0, 2)
							count := "已处理 %d / %d 条"
							if m.Progress.Stage == "verify" {
								count = "已校验 %d / %d 条"
							}
							k.Text(c.Key("count"), fmt.Sprintf(tr(count), m.Progress.Processed, m.Progress.Total), secondary)
						}
					})
				}
			})
		})
		if m.Entering {
			notice("starting", "正在启动应用…", uikit.ToneInfo)
		}
		if m.Error != "" && result {
			notice("result-error", m.Error, uikit.ToneError)
		}
		ui.Divider(c)
		ui.Row(c.Key("actions")).Padding(14, 20).Gap(8).AlignItems(ui.Center).Children(func() {
			switch m.Stage {
			case "confirm", "invalid", "password", "conflict":
				button("skip", "暂不迁移", uikit.Subtle, false, a.Enter)
				ui.Spacer(c)
				switch m.Stage {
				case "password":
					button("unlock", "解锁并继续", uikit.Primary, fields.Passphrase == "", a.Unlock)
				case "conflict":
					button("retry", "重试", uikit.Primary, false, a.Start)
				default:
					button("start", "开始迁移", uikit.Primary, m.Stage == "invalid" || m.Detecting, a.Start)
				}
			case "credentials", "import", "verify", "stopping":
				text("offline", "迁移期间不会捕获剪贴板，也不会同步", secondary)
				ui.Spacer(c)
				label := "停止迁移"
				if m.Stage == "stopping" {
					label = "正在停止…"
				}
				button("stop", label, uikit.Standard, m.Stage == "stopping", a.Stop)
			case "complete":
				ui.Spacer(c)
				button("enter", "开始使用", uikit.Primary, false, a.Enter)
			case "partial":
				button("retry", "重试迁移", uikit.Standard, false, a.Start)
				ui.Spacer(c)
				button("enter", "进入应用", uikit.Primary, false, a.Enter)
			case "cancelled":
				button("enter", "进入应用", uikit.Standard, false, a.Enter)
				ui.Spacer(c)
				button("retry", "继续迁移", uikit.Primary, false, a.Start)
			case "failed":
				button("pick", "选择其他目录", uikit.Standard, false, a.Pick)
				button("enter", "不再迁移，进入应用", uikit.Standard, false, a.Enter)
				ui.Spacer(c)
				button("retry", "重试", uikit.Primary, false, a.Start)
			}
		})
		if m.Stage == "confirm" || m.Stage == "password" || m.Stage == "invalid" {
			k.Text(c.Key("later"), tr("之后可在设置 › 存储 › 从 v1 迁移中导入"), secondary).Padding(0, 20, 10)
		}
	})
}

func panel(c *ui.Context, k uikit.Kit, body func()) {
	ui.Column(c).FillWidth().Padding(16).Gap(10).Radius(12).Border(1, c.Theme().Border).Children(body)
}
func scope(c *ui.Context, k uikit.Kit, tr func(string) string, title string, items []string, included bool) {
	ui.Column(c).Grow(1).MinWidth(0).Padding(12).Gap(6).Radius(10).Border(1, c.Theme().Border).Children(func() {
		k.Text(c, tr(title), uikit.TextProps{Role: uikit.RoleCaption, Tone: uikit.ToneSecondary})
		for _, item := range items {
			ui.Row(c.Key(item)).Gap(7).AlignItems(ui.Center).Children(func() {
				icon := uikit.IconRemove
				color := c.Theme().TextMuted
				if included {
					icon = uikit.IconAdd
					color = c.Theme().Success
				}
				k.Icon(c, icon, 13).TextColor(color)
				k.Text(c, tr(item), uikit.TextProps{})
			})
		}
	})
}
func steps(c *ui.Context, k uikit.Kit, tr func(string) string, current int) {
	ui.Row(c).FillWidth().Gap(10).Children(func() {
		for i, label := range []string{"解锁旧数据", "导入记录", "校验内容"} {
			ui.Row(c.Key(label)).Gap(6).AlignItems(ui.Center).Children(func() {
				color, bg := c.Theme().TextMuted, c.Theme().Surface
				if i == current {
					color, bg = ui.RGB(255, 255, 255), c.Theme().Accent
				}
				if i < current {
					color = c.Theme().Success
				}
				ui.Box(c).Size(22, 22).Radius(11).Background(bg).Center().Children(func() {
					k.Text(c, fmt.Sprint(i+1), uikit.TextProps{Role: uikit.RoleCaption}).TextColor(color)
				})
				k.Text(c, tr(label), uikit.TextProps{Role: uikit.RoleCaption})
			})
			if i < 2 {
				ui.Box(c).Grow(1).Height(2).Background(c.Theme().Border).MarginY(10)
			}
		}
	})
}
func renderResult(c *ui.Context, k uikit.Kit, m Model, tr func(string) string) {
	title, lead, icon, color := "迁移未完成", "已提交的记录不会回滚。重试会跳过它们。", uikit.IconDismiss, c.Theme().Danger
	switch m.Stage {
	case "complete":
		title, lead, icon, color = "迁移完成", "所有记录都已导入并通过校验。", uikit.IconShield, c.Theme().Success
		if m.Report.Imported == 0 && m.Report.Skipped > 0 {
			lead = "没有新的记录需要导入。此前导入的记录仍在历史中。"
		}
	case "partial":
		title, lead, icon, color = "部分数据未能导入", "已导入的记录可以正常使用。问题记录可在重试时再次处理。", uikit.IconInfo, c.Theme().Warning
	case "cancelled":
		title, lead, icon, color = "迁移已停止", "已提交的记录已保留。继续迁移时会跳过它们。", uikit.IconRemove, c.Theme().TextMuted
	}
	ui.Column(c.Key("result")).FillWidth().Gap(12).Padding(18, 0).AlignItems(ui.Center).Children(func() {
		ui.Box(c).Size(64, 64).Radius(32).Background(color.Alpha(.12)).Center().Children(func() { k.Icon(c, icon, 30).TextColor(color) })
		k.Text(c.Key("title"), tr(title), uikit.TextProps{Role: uikit.RoleTitle}).FontSize(20)
		k.Text(c.Key("lead"), tr(lead), uikit.TextProps{Tone: uikit.ToneSecondary}).FillWidth()
		stats := []struct {
			label string
			count int
		}{{"本次新导入", m.Report.Imported}, {"此前已导入", m.Report.Skipped}, {"已校验", m.Report.Verified}}
		for _, v := range []struct {
			label string
			count int
		}{{"失败记录", m.Report.Failed}, {"缺失内容", m.Report.MissingParts}, {"校验失败", m.Report.VerificationFailed}} {
			if v.count > 0 {
				stats = append(stats, v)
			}
		}
		for offset := 0; offset < len(stats); offset += 3 {
			ui.Row(c.Key(fmt.Sprint(offset))).FillWidth().Gap(10).Children(func() {
				for _, stat := range stats[offset:min(offset+3, len(stats))] {
					ui.Column(c.Key(stat.label)).Grow(1).MinWidth(0).Padding(12, 10).Gap(4).Radius(10).Background(c.Theme().Surface).AlignItems(ui.Center).Children(func() {
						k.Text(c, fmt.Sprint(stat.count), uikit.TextProps{Role: uikit.RoleTitle}).FontSize(21)
						k.Text(c, tr(stat.label), uikit.TextProps{Role: uikit.RoleCaption, Tone: uikit.ToneSecondary})
					})
				}
			})
		}
		if m.Report.MissingParts > 0 {
			k.InlineNotice(c, uikit.NoticeProps{Tone: uikit.ToneInfo, Title: tr("缺失内容是记录中丢失的图片或文件附件。它与失败记录分开计数。")})
		}
	})
}
