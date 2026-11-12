package desktopdemo

import (
	"fmt"
	"runtime"
	"slices"
	"strconv"
	"strings"

	"github.com/egoist/mygo/ui"

	"github.com/uniclipboard/unic/app/desktop/internal/uikit"
	config "github.com/uniclipboard/unic/internal/localconfig"
)

func (d *demo) liveSettingsContent(c *ui.Context, k uikit.Kit) {
	l := d.live
	if !l.loaded {
		k.Text(c, d.tr("正在读取设置…"), secondary)
		return
	}
	if l.message != "" {
		k.InlineNotice(c, uikit.NoticeProps{Tone: uikit.ToneWarning, Title: d.tr(l.message)})
	}
	cfg := l.config.Config
	cfg.History.ExcludedTags = slices.Clone(cfg.History.ExcludedTags)
	changed := false
	group := func(id, title string, body func()) {
		k.SettingsGroup(c.Key(id), uikit.SettingsGroupProps{Title: title}, body)
	}
	toggle := func(id, label, desc string, value *bool) {
		k.SettingsRow(c.Key(id), uikit.SettingsRowProps{Label: label, Description: desc}, nil, func() {
			if k.Switch(c, value, uikit.SwitchProps{Label: label, Disabled: l.busy}).Changed() {
				changed = true
			}
		})
	}
	number := func(id, label, desc string, value *int, values []int, unit string) {
		k.SettingsRow(c.Key(id), uikit.SettingsRowProps{Label: label, Description: desc}, nil, func() {
			opts := []uikit.SelectItem{}
			if !slices.Contains(values, *value) {
				values = append(slices.Clone(values), *value)
			}
			for _, n := range values {
				text := fmt.Sprintf("%d %s", n, unit)
				if n == 0 {
					text = d.tr("无限制")
				}
				opts = append(opts, uikit.SelectItem{Value: strconv.Itoa(n), Label: text})
			}
			v := strconv.Itoa(*value)
			if k.Select(c, uikit.SelectProps{Label: label, Value: &v, Items: opts, Disabled: l.busy}).Changed {
				*value, _ = strconv.Atoi(v)
				changed = true
			}
		})
	}
	switch d.settings.Value {
	case "general":
		group("language", d.tr("语言"), func() { d.languageRow(c, k) })
		group("device", d.tr("设备"), func() {
			k.SettingsRow(c, uikit.SettingsRowProps{Label: d.tr("设备名称"), Description: d.tr("输入后按回车保存。")}, nil, func() {
				field := k.TextField(c, uikit.TextFieldProps{Label: d.tr("设备名称"), Value: &d.deviceName})
				if field.Submitted && !l.busy && strings.TrimSpace(d.deviceName) != "" {
					cfg.DeviceName = strings.TrimSpace(d.deviceName)
					changed = true
				}
			})
		})
		group("startup", d.tr("启动与运行模式"), func() {
			k.SettingsRow(c, uikit.SettingsRowProps{Label: d.tr("运行模式"), Description: d.tr("下次启动时生效。")}, nil, func() {
				if k.Select(c, uikit.SelectProps{Label: d.tr("运行模式"), Value: &cfg.Desktop.Mode, Disabled: l.busy, Items: []uikit.SelectItem{{Value: "normal", Label: d.tr("常规模式")}, {Value: "lightweight", Label: d.tr("轻量模式")}}}).Changed {
					changed = true
				}
			})
			toggle("startup", d.tr("登录时启动"), d.tr("登录系统后自动启动。"), &cfg.Desktop.LaunchAtLogin)
			toggle("background", d.tr("后台启动"), d.tr("下次常规启动时隐藏主窗口。"), &cfg.Desktop.StartInBackground)
			if l.loginStatus != nil {
				enabled, message := l.loginStatus()
				label := d.tr("系统登录启动已关闭")
				if enabled {
					label = d.tr("系统登录启动已开启")
				}
				k.Text(c, label, footnote)
				if message != "" {
					k.Text(c, message, uikit.TextProps{Tone: uikit.ToneCritical})
				}
				if enabled != cfg.Desktop.LaunchAtLogin || message != "" {
					if k.Button(c, d.tr("重新应用登录启动设置"), uikit.ButtonProps{Disabled: l.busy}).Clicked() && l.retryLogin != nil {
						l.retryLogin()
					}
				}
			}
		})
		group("privacy", d.tr("隐私与诊断"), func() {
			toggle("telemetry", d.tr("发送使用数据与诊断"), d.tr("关闭后停止遥测上传。"), &cfg.Telemetry.Enabled)
		})
	case "sync":
		group("sync", d.tr("同步行为"), func() {
			toggle("enabled", d.tr("启用同步"), d.tr("控制所有同步后端。"), &cfg.Sync.Enabled)
			toggle("directoryAutomatic", d.tr("自动同步目录"), d.tr("默认关闭。开启后，每次复制目录都会扫描并发送。"), &cfg.Sync.Directories.Automatic)
			if runtime.GOOS != "darwin" && runtime.GOOS != "windows" {
				k.Text(c, d.tr("此平台无法识别同一路径的重复复制。请使用主动发送。"), uikit.TextProps{Tone: uikit.ToneSecondary})
			}
			toggle("automatic", d.tr("自动同步"), d.tr("复制内容后自动发送。"), &cfg.Sync.Automatic)
			toggle("restore", d.tr("使用历史时同步"), d.tr("再次使用历史后发送。"), &cfg.Sync.OnRestore)
			toggle("files", d.tr("同步文件"), d.tr("允许发送和接收文件。"), &cfg.Sync.Files)
		})
		group("limits", d.tr("文件传输"), func() {
			n := int(cfg.Limits.MaxFileBytes / (1 << 20))
			number("limit", d.tr("单文件传输上限"), d.tr("限制单个文件的传输大小。"), &n, []int{10, 50, 100, 500}, "MB")
			cfg.Limits.MaxFileBytes = config.Size(n) * (1 << 20)
		})
	case "storage":
		group("history", d.tr("历史保留"), func() {
			toggle("cleanup", d.tr("自动清理历史"), d.tr("清理历史记录及应用管理的文件。"), &cfg.History.AutoCleanup)
			number("days", d.tr("历史保留时间"), d.tr("按创建时间计算。"), &cfg.History.RetentionDays, []int{7, 30, 90, 180, 365, 0}, d.tr("天"))
			number("count", d.tr("最大历史条目数"), d.tr("排除标签命中的记录不计入上限。"), &cfg.History.MaxItems, []int{100, 500, 1000, 5000, 0}, d.tr("条"))
			for _, tag := range d.tags {
				selected := slices.Contains(cfg.History.ExcludedTags, tag.id)
				if k.Checkbox(c.Key(tag.id), &selected, uikit.CheckboxProps{Label: d.tr("保留 #") + tag.name, Disabled: l.busy}).Changed() {
					if selected {
						cfg.History.ExcludedTags = append(cfg.History.ExcludedTags, tag.id)
					} else {
						cfg.History.ExcludedTags = slices.DeleteFunc(cfg.History.ExcludedTags, func(t string) bool { return t == tag.id })
					}
					changed = true
				}
			}
		})
		group("cache", d.tr("文件存储"), func() {
			n := int(cfg.FileCache.MaxBytes / (1 << 20))
			number("quota", d.tr("单设备缓存配额"), d.tr("仅限制应用管理的文件缓存。"), &n, []int{100, 500, 1024, 2048, 4096}, "MB")
			cfg.FileCache.MaxBytes = config.Size(n) * (1 << 20)
			inbox := cfg.Inbox
			if inbox == "" {
				inbox = d.tr("应用管理的缓存")
			}
			k.SettingsRow(c, uikit.SettingsRowProps{Label: d.tr("接收目录"), Description: inbox}, nil, nil)
		})
	case "quickPanel":
		group("shortcut", d.tr("全局快捷键"), func() {
			k.SettingsRow(c, uikit.SettingsRowProps{Label: d.tr("打开快捷面板"), Description: d.tr("在常规模式运行时生效。")}, nil, func() {
				if k.Select(c, uikit.SelectProps{Label: d.tr("全局快捷键"), Value: &cfg.Desktop.QuickPanelShortcut, Disabled: l.busy, Items: []uikit.SelectItem{{Value: "Ctrl+Alt+V", Label: "Ctrl+Alt+V"}, {Value: "Cmd+Ctrl+V", Label: "Command+Ctrl+V"}, {Value: "Ctrl+`", Label: "Ctrl+`"}, {Value: "CmdOrCtrl+Shift+Space", Label: "Command/Ctrl+Shift+Space"}, {Value: "", Label: d.tr("关闭")}}}).Changed {
					changed = true
				}
			})
			if d.quickShortcutStatus != nil {
				effective, message := d.quickShortcutStatus()
				if effective == "" {
					effective = d.tr("关闭")
				}
				k.Text(c, d.tr("当前注册：")+effective, footnote)
				if message != "" {
					k.InlineNotice(c, uikit.NoticeProps{Tone: uikit.ToneWarning, Title: message})
					if k.Button(c, d.tr("重试注册快捷键"), uikit.ButtonProps{Disabled: l.busy}).Clicked() && d.retryQuickShortcut != nil {
						d.retryQuickShortcut()
					}
				}
			}
		})
		if k.Button(c, d.tr("打开快捷面板"), uikit.ButtonProps{}).Clicked() && d.openQuickPanel != nil {
			d.openQuickPanel()
		}
	case "about":
		ui.Image(c, appIcon()).Size(64, 64)
		k.EntityHeader(c, uikit.EntityHeaderProps{Icon: uikit.IconCopy, Title: "UniClipboard", Description: d.tr("跨设备剪贴板同步")}, nil)
		group("runtime", d.tr("桌面"), func() {
			k.SettingsRow(c, uikit.SettingsRowProps{Label: d.tr("界面组件"), Description: "mygo + UI Kit"}, nil, nil)
		})
	default:
		k.ContentUnavailable(c, uikit.ContentUnavailableProps{Title: d.tr("此设置暂不可用"), Description: d.tr("当前版本尚不支持此设置。")}, nil)
	}
	if len(l.config.RestartRequired) > 0 {
		k.Text(c, d.tr("部分设置将在下次启动时生效。"), footnote)
	}
	if changed {
		d.saveLiveConfig(cfg)
	}
}
