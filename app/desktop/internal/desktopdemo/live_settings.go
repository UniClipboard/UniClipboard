package desktopdemo

import (
	"fmt"
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
		k.Text(c, "正在读取设置…", secondary)
		return
	}
	if l.message != "" {
		k.InlineNotice(c, uikit.NoticeProps{Tone: uikit.ToneWarning, Title: l.message})
	}
	cfg := l.config.Config
	cfg.History.ExcludedTags = slices.Clone(cfg.History.ExcludedTags)
	changed := false
	group := func(id, title string, body func()) {
		k.SettingsGroup(c.Key(id), uikit.SettingsGroupProps{Title: title}, body)
	}
	toggle := func(id, label, desc string, value *bool) {
		k.SettingsRow(c.Key(id), uikit.SettingsRowProps{Label: label, Description: desc}, nil, func() {
			if k.Switch(c, value, uikit.SwitchProps{Label: label, LabelHidden: true, Disabled: l.busy}).Changed() {
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
					text = "无限制"
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
		group("device", "设备", func() {
			k.SettingsRow(c, uikit.SettingsRowProps{Label: "设备名称", Description: "输入后按回车保存。"}, nil, func() {
				field := k.TextField(c, uikit.TextFieldProps{Label: "设备名称", Value: &d.deviceName})
				if field.Submitted && !l.busy && strings.TrimSpace(d.deviceName) != "" {
					cfg.DeviceName = strings.TrimSpace(d.deviceName)
					changed = true
				}
			})
		})
		group("startup", "启动与运行模式", func() {
			k.SettingsRow(c, uikit.SettingsRowProps{Label: "运行模式", Description: "下次启动时生效。"}, nil, func() {
				if k.Select(c, uikit.SelectProps{Label: "运行模式", Value: &cfg.Desktop.Mode, Disabled: l.busy, Items: []uikit.SelectItem{{Value: "normal", Label: "常规模式"}, {Value: "lightweight", Label: "轻量模式"}}}).Changed {
					changed = true
				}
			})
			toggle("startup", "登录时启动", "登录系统后自动启动。", &cfg.Desktop.LaunchAtLogin)
			toggle("background", "后台启动", "下次常规启动时隐藏主窗口。", &cfg.Desktop.StartInBackground)
			if l.loginStatus != nil {
				enabled, message := l.loginStatus()
				label := "系统登录启动已关闭"
				if enabled {
					label = "系统登录启动已开启"
				}
				k.Text(c, label, footnote)
				if message != "" {
					k.Text(c, message, uikit.TextProps{Tone: uikit.ToneCritical})
				}
				if enabled != cfg.Desktop.LaunchAtLogin || message != "" {
					if k.Button(c, "重新应用登录启动设置", uikit.ButtonProps{Disabled: l.busy}).Clicked() && l.retryLogin != nil {
						l.retryLogin()
					}
				}
			}
		})
		group("privacy", "隐私与诊断", func() {
			toggle("telemetry", "发送使用数据与诊断", "关闭后停止遥测上传。", &cfg.Telemetry.Enabled)
		})
	case "sync":
		group("sync", "同步行为", func() {
			toggle("enabled", "启用同步", "控制所有同步后端。", &cfg.Sync.Enabled)
			toggle("automatic", "自动同步", "复制内容后自动发送。", &cfg.Sync.Automatic)
			toggle("restore", "使用历史时同步", "再次使用历史后发送。", &cfg.Sync.OnRestore)
			toggle("files", "同步文件", "允许发送和接收文件。", &cfg.Sync.Files)
		})
		group("limits", "文件传输", func() {
			n := int(cfg.Limits.MaxFileBytes / (1 << 20))
			number("limit", "单文件传输上限", "限制单个文件的传输大小。", &n, []int{10, 50, 100, 500}, "MB")
			cfg.Limits.MaxFileBytes = config.Size(n) * (1 << 20)
		})
	case "storage":
		group("history", "历史保留", func() {
			toggle("cleanup", "自动清理历史", "清理历史记录及应用管理的文件。", &cfg.History.AutoCleanup)
			number("days", "历史保留时间", "按创建时间计算。", &cfg.History.RetentionDays, []int{7, 30, 90, 180, 365, 0}, "天")
			number("count", "最大历史条目数", "排除标签命中的记录不计入上限。", &cfg.History.MaxItems, []int{100, 500, 1000, 5000, 0}, "条")
			for _, tag := range d.tags {
				selected := slices.Contains(cfg.History.ExcludedTags, tag.id)
				if k.Checkbox(c.Key(tag.id), &selected, uikit.CheckboxProps{Label: "保留 #" + tag.name, Disabled: l.busy}).Changed() {
					if selected {
						cfg.History.ExcludedTags = append(cfg.History.ExcludedTags, tag.id)
					} else {
						cfg.History.ExcludedTags = slices.DeleteFunc(cfg.History.ExcludedTags, func(t string) bool { return t == tag.id })
					}
					changed = true
				}
			}
		})
		group("cache", "文件存储", func() {
			n := int(cfg.FileCache.MaxBytes / (1 << 20))
			number("quota", "单设备缓存配额", "仅限制应用管理的文件缓存。", &n, []int{100, 500, 1024, 2048, 4096}, "MB")
			cfg.FileCache.MaxBytes = config.Size(n) * (1 << 20)
			inbox := cfg.Inbox
			if inbox == "" {
				inbox = "应用管理的缓存"
			}
			k.SettingsRow(c, uikit.SettingsRowProps{Label: "接收目录", Description: inbox}, nil, nil)
		})
	case "quickPanel":
		if k.Button(c, "打开快捷面板", uikit.ButtonProps{}).Clicked() && d.openQuickPanel != nil {
			d.openQuickPanel()
		}
	case "about":
		ui.Image(c, appIcon()).Size(64, 64)
		k.EntityHeader(c, uikit.EntityHeaderProps{Icon: uikit.IconCopy, Title: "UniClipboard", Description: "跨设备剪贴板同步"}, nil)
		group("runtime", "桌面", func() {
			k.SettingsRow(c, uikit.SettingsRowProps{Label: "界面组件", Description: "mygo + UI Kit"}, nil, nil)
		})
	default:
		k.ContentUnavailable(c, uikit.ContentUnavailableProps{Title: "此设置暂不可用", Description: "当前版本尚不支持此设置。"}, nil)
	}
	if len(l.config.RestartRequired) > 0 {
		k.Text(c, "部分设置将在下次启动时生效。", footnote)
	}
	if changed {
		d.saveLiveConfig(cfg)
	}
}
