package desktopui

import (
	"fmt"
	"runtime"
	"slices"
	"strconv"
	"strings"

	"github.com/egoist/mygo/ui"

	"github.com/uniclipboard/unic/app/desktop/internal/desktopkit"
	"github.com/uniclipboard/unic/internal/desktopkeys"
	config "github.com/uniclipboard/unic/internal/localconfig"
)

func (d *viewState) liveSettingsContent(c *ui.Context, k desktopkit.Kit) {
	l := d.live
	if !l.loaded {
		k.Text(c, d.tr("正在读取设置…"), secondary)
		return
	}
	cfg := l.config.Config
	cfg.History.ExcludedTags = slices.Clone(cfg.History.ExcludedTags)
	changed := false
	group := func(id, title string, body func()) {
		k.SettingsGroup(c.Key(id), desktopkit.SettingsGroupProps{Title: title}, body)
	}
	toggle := func(id, label, desc string, value *bool, dim, disabled bool) {
		k.SettingsRow(c.Key(id), desktopkit.SettingsRowProps{Label: label, Description: desc, Dim: dim}, nil, func() {
			if k.Switch(c, value, desktopkit.SwitchProps{Label: label, Disabled: l.busy || disabled}).Changed() {
				changed = true
			}
		})
	}
	number := func(id, label, desc string, value *int, values []int, unit string, dim, disabled bool) {
		k.SettingsRow(c.Key(id), desktopkit.SettingsRowProps{Label: label, Description: desc, Dim: dim}, nil, func() {
			opts := []desktopkit.SelectItem{}
			if !slices.Contains(values, *value) {
				values = append(slices.Clone(values), *value)
			}
			for _, n := range values {
				text := fmt.Sprintf("%d %s", n, unit)
				if n == 0 {
					text = d.tr("无限制")
				}
				opts = append(opts, desktopkit.SelectItem{Value: strconv.Itoa(n), Label: text})
			}
			v := strconv.Itoa(*value)
			if k.Select(c, desktopkit.SelectProps{Label: label, Value: &v, Items: opts, Disabled: l.busy || disabled}).Changed {
				*value, _ = strconv.Atoi(v)
				changed = true
			}
		})
	}

	switch d.settings.Value {
	case "general":
		group("language-device", d.tr("语言与设备"), func() {
			d.languageRow(c, k)
			k.SettingsRow(c.Key("device-name"), desktopkit.SettingsRowProps{
				Label:       d.tr("设备名称"),
				Description: d.tr("其他已配对设备将看到此名称，按回车保存。"),
			}, nil, func() {
				field := k.TextField(c, desktopkit.TextFieldProps{Label: d.tr("设备名称"), Value: &d.deviceName})
				if field.Submitted && !l.busy && strings.TrimSpace(d.deviceName) != "" {
					cfg.DeviceName = strings.TrimSpace(d.deviceName)
					changed = true
				}
			})
		})

		var loginEnabled bool
		var loginMsg string
		hasLoginStatus := l.loginStatus != nil
		if hasLoginStatus {
			loginEnabled, loginMsg = l.loginStatus()
		}
		var startupTrailing func()
		if hasLoginStatus {
			startupTrailing = func() {
				if loginEnabled {
					settingsPill(c.Key("login-pill"), k, d.tr("系统登录启动已开启"), desktopkit.StatusPositive, true, false)
				} else {
					settingsPill(c.Key("login-pill"), k, d.tr("系统登录启动已关闭"), desktopkit.StatusNeutral, true, false)
				}
			}
		}
		k.SettingsGroup(c.Key("startup"), desktopkit.SettingsGroupProps{
			Title:    d.tr("启动与运行模式"),
			Trailing: startupTrailing,
		}, func() {
			k.SettingsRow(c.Key("mode"), desktopkit.SettingsRowProps{
				Label:       d.tr("运行模式"),
				Description: d.tr("常规模式关闭主窗口后保留托盘与同步服务；轻量模式仅保留后台同步服务。"),
			}, nil, func() {
				if k.Select(c, desktopkit.SelectProps{
					Label:    d.tr("运行模式"),
					Value:    &cfg.Desktop.Mode,
					Disabled: l.busy,
					Items: []desktopkit.SelectItem{
						{Value: "normal", Label: d.tr("常规模式")},
						{Value: "lightweight", Label: d.tr("轻量模式")},
					},
				}).Changed {
					changed = true
				}
			})
			toggle("startup", d.tr("登录时启动"), d.tr("登录操作系统后自动启动 Unic。"), &cfg.Desktop.LaunchAtLogin, false, false)
			lightweight := cfg.Desktop.Mode == "lightweight"
			toggle("background", d.tr("后台启动"), d.tr("仅控制常规模式的登录启动；手动点击应用图标始终打开主窗口。"), &cfg.Desktop.StartInBackground, lightweight, lightweight)
			if hasLoginStatus && (loginEnabled != cfg.Desktop.LaunchAtLogin || loginMsg != "") {
				msg := loginMsg
				if msg == "" {
					msg = d.tr("系统登录注册状态与配置不一致，可重新应用。")
				}
				settingsFooterBar(c.Key("login-retry"), k, msg, true, func() {
					if k.Button(c.Key("retry-login"), d.tr("重新应用登录启动设置"), desktopkit.ButtonProps{
						Size:     desktopkit.SizeSmall,
						Disabled: l.busy,
					}).Clicked() && l.retryLogin != nil {
						l.retryLogin()
					}
				})
			}
		})

	case "appearance":
		changed = d.liveAppearanceContent(c, k, &cfg)

	case "diagnostics":
		d.diagnosticsSettings(c, k, &cfg, &changed)

	case "plugins":
		d.builtinPluginsContent(c, k)
		return

	case "sync":
		group("sync", d.tr("同步行为"), func() {
			if l.clipboardUnavailable != nil && l.clipboardUnavailable() {
				settingsNoteRow(c.Key("clipboard-unavailable"), k, d.tr("此桌面会话不支持后台剪贴板监听。仍可手动复制历史内容。"))
			}
			toggle("enabled", d.tr("启用同步"), d.tr("控制所有同步后端的发送与接收。"), &cfg.Sync.Enabled, false, false)
			dim := !cfg.Sync.Enabled
			toggle("automatic", d.tr("自动同步"), d.tr("复制内容后自动发送给已授权设备。"), &cfg.Sync.Automatic, dim, false)
			toggle("restore", d.tr("使用历史时同步"), d.tr("再次应用本机历史条目后发送一次。"), &cfg.Sync.OnRestore, dim, false)
			toggle("files", d.tr("同步文件"), d.tr("允许在设备之间发送和接收文件。"), &cfg.Sync.Files, dim, false)
			toggle("directoryAutomatic", d.tr("自动同步目录"), d.tr("默认关闭。开启后，每次复制目录都会扫描并发送。"), &cfg.Sync.Directories.Automatic, dim, false)
			if runtime.GOOS != "darwin" && runtime.GOOS != "windows" {
				settingsNoteRow(c.Key("dir-platform-note"), k, d.tr("此平台无法识别同一路径的重复复制。请使用主动发送。"))
			}
		})
		group("limits", d.tr("文件传输"), func() {
			n := int(cfg.Limits.MaxFileBytes / (1 << 20))
			number("limit", d.tr("单文件传输上限"), d.tr("双向限制单个文件的最大传输大小。"), &n, []int{10, 50, 100, 500}, "MB", false, false)
			cfg.Limits.MaxFileBytes = config.Size(n) * (1 << 20)
		})

	case "storage":
		d.storageCacheGroup(c, k, &cfg, &changed)

		group("history", d.tr("历史保留"), func() {
			toggle("cleanup", d.tr("自动清理历史"), d.tr("定期清理过期历史记录及应用管理的关联文件。"), &cfg.History.AutoCleanup, false, false)
			dim := !cfg.History.AutoCleanup
			number("days", d.tr("历史保留时间"), d.tr("按记录创建时间计算。"), &cfg.History.RetentionDays, []int{7, 30, 90, 180, 365, 0}, d.tr("天"), dim, dim)
			number("count", d.tr("最大历史条目数"), d.tr("排除标签命中的记录不计入上限。"), &cfg.History.MaxItems, []int{100, 500, 1000, 5000, 0}, d.tr("条"), dim, dim)
			k.SettingsRow(c.Key("excluded-tags"), desktopkit.SettingsRowProps{
				Label:       d.tr("清理时保留标签"),
				Description: d.tr("命中以下任一标签的历史将不会被自动清理。"),
				Dim:         dim,
			}, nil, func() {
				ui.Row(c.Key("tag-pills")).Wrap().Gap(6).Justify(ui.End).MaxWidth(440).MinWidth(0).Children(func() {
					for _, tag := range d.tags {
						tag := tag
						selected := slices.Contains(cfg.History.ExcludedTags, tag.id)
						label := "#" + d.tagLabel(tag.id)
						pill := ui.ButtonBase(c.Key("tag-"+tag.id)).
							Label(d.tr("保留 ")+label).
							Selected(selected).
							Disabled(l.busy || dim).
							Height(26).Padding(0, 9).Radius(13).Gap(5).
							AlignItems(ui.Center).
							FocusRingColor(k.Color("border-focus-ring")).FocusRingOffset(1)
						if selected {
							bg, fg := k.TagTint(tag.color)
							pill.Background(bg).TextColor(fg)
						} else {
							pill.Background(k.SubtleCardSurface()).
								Border(1, k.Color("border-button-default")).
								TextColor(k.Color("text-secondary"))
						}
						pill.Children(func() {
							ui.Box(c.Key("dot")).Size(7, 7).Radius(3.5).Shrink(0).Background(k.SwatchColor(tag.color)).AccessHidden(true)
							w := 500
							if selected {
								w = 600
							}
							ui.Text(c.Key("name"), label).FontSize(12).FontWeight(w).SingleLine()
							if selected {
								ui.Text(c.Key("chk"), "✓").FontSize(11).FontWeight(700).SingleLine().AccessHidden(true)
							}
						})
						if pill.Clicked() && !l.busy && !dim {
							if selected {
								cfg.History.ExcludedTags = slices.DeleteFunc(cfg.History.ExcludedTags, func(t string) bool { return t == tag.id })
							} else {
								cfg.History.ExcludedTags = append(cfg.History.ExcludedTags, tag.id)
							}
							changed = true
						}
					}
				})
			})
		})

		d.backupSettingsContent(c, k)
		d.storageLocalDataGroup(c, k)

	case "shortcuts":
		changed = d.editShortcuts(c, k, &cfg)

	case "quickPanel":
		group("shortcut", d.tr("全局快捷键"), func() {
			k.SettingsRow(c.Key("shortcut-select"), desktopkit.SettingsRowProps{
				Label:       d.tr("打开快捷面板"),
				Description: d.tr("在常规模式运行时生效。切换后由托盘进程原子替换注册。"),
			}, nil, func() {
				if _, custom := cfg.Desktop.Shortcuts[desktopkeys.TogglePanel]; custom {
					if k.Button(c.Key("shortcut-settings"), d.tr("编辑快捷键"), desktopkit.ButtonProps{Size: desktopkit.SizeSmall}).Clicked() {
						d.settings.Value = "shortcuts"
					}
					return
				}
				if k.Select(c, desktopkit.SelectProps{
					Label:    d.tr("全局快捷键"),
					Value:    &cfg.Desktop.QuickPanelShortcut,
					Disabled: l.busy,
					Items: []desktopkit.SelectItem{
						{Value: "Ctrl+Alt+V", Label: "Ctrl+Alt+V"},
						{Value: "Cmd+Ctrl+V", Label: "Command+Ctrl+V"},
						{Value: "Ctrl+`", Label: "Ctrl+`"},
						{Value: "CmdOrCtrl+Shift+Space", Label: "Command/Ctrl+Shift+Space"},
						{Value: "", Label: d.tr("关闭")},
					},
				}).Changed {
					changed = true
				}
			})
			k.SettingsRow(c.Key("double-tap"), desktopkit.SettingsRowProps{
				Label: d.tr("双击修饰键打开快捷面板"),
			}, nil, func() {
				if k.Select(c, desktopkit.SelectProps{
					Label: d.tr("双击修饰键打开快捷面板"), Value: &cfg.Desktop.QuickPanelDoubleTap, Disabled: l.busy,
					Items: []desktopkit.SelectItem{
						{Value: "", Label: d.tr("关闭")}, {Value: "alt", Label: "Option / Alt"},
						{Value: "control", Label: "Ctrl"}, {Value: "meta", Label: "Command / Win / Super"},
					},
				}).Changed {
					changed = true
				}
			})
			if d.quickShortcutStatus != nil {
				effective, message := d.quickShortcutStatus()
				desc := d.tr("显示系统全局快捷键实际注册结果。")
				if message != "" {
					desc = message
				}
				k.SettingsRow(c.Key("shortcut-status"), desktopkit.SettingsRowProps{
					Label:       d.tr("当前注册状态"),
					Description: desc,
				}, nil, func() {
					if message != "" {
						pillText := d.tr("注册冲突")
						if effective != "" {
							pillText = d.tr("注册冲突 · 仍为 ") + effective
						}
						settingsPill(c.Key("conflict-pill"), k, pillText, desktopkit.StatusWarning, true, false)
						if k.Button(c.Key("retry-shortcut"), d.tr("重试注册快捷键"), desktopkit.ButtonProps{
							Size:     desktopkit.SizeSmall,
							Disabled: l.busy,
						}).Clicked() && d.retryQuickShortcut != nil {
							d.retryQuickShortcut()
						}
					} else if effective != "" {
						settingsPill(c.Key("active-pill"), k, d.tr("已注册：")+effective, desktopkit.StatusPositive, true, false)
					} else {
						settingsPill(c.Key("off-pill"), k, d.tr("已关闭"), desktopkit.StatusNeutral, true, false)
					}
				})
			}
			k.SettingsRow(c.Key("preview-quick-panel"), desktopkit.SettingsRowProps{
				Label:       d.tr("预览快捷面板"),
				Description: d.tr("立即打开快捷面板查看当前外观与键盘交互。"),
			}, nil, func() {
				if k.Button(c.Key("open-quick-panel"), d.tr("打开快捷面板"), desktopkit.ButtonProps{
					Size:     desktopkit.SizeSmall,
					Disabled: d.openQuickPanel == nil,
				}).Clicked() && d.openQuickPanel != nil {
					d.openQuickPanel()
				}
			})
		})
		d.shortcutPermissionSettings(c, k)

	case "notifications":
		d.notificationSettingsContent(c, k, &cfg, &changed)

	case "about":
		d.aboutContent(c, k)

	default:
		k.ContentUnavailable(c, desktopkit.ContentUnavailableProps{Title: d.tr("此设置暂不可用"), Description: d.tr("当前版本尚不支持此设置。")}, nil)
	}

	if changed {
		d.saveLiveConfig(cfg)
	}
}

func (d *viewState) requestRestart(restartService bool) {
	if d.live == nil || d.live.restartApp == nil || d.live.restarting {
		return
	}
	l := d.live
	l.restarting = true
	l.message = ""
	d.toastDismiss(toastIDRestartError)
	l.restartApp(restartService, func(err error) {
		select {
		case <-l.ctx.Done():
		case l.results <- func() {
			l.restarting = false
			if err != nil {
				l.message = "无法重启应用。请手动退出并重新启动桌面端。"
				d.toastError(d.tr(l.message), desktopkit.ToastOptions{ID: toastIDRestartError})
			}
		}:
			l.wake()
		}
	})
}
