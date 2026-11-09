package desktopdemo

import (
	"github.com/egoist/mygo/ui"
	"github.com/uniclipboard/unic/app/desktop/internal/uikit"
)

var categories = []uikit.SidebarNavItem{
	{ID: "general", Label: "通用", Icon: uikit.IconSettings},
	{ID: "appearance", Label: "外观", Icon: uikit.IconSun},
	{ID: "shortcuts", Label: "快捷键", Icon: uikit.IconEdit},
	{ID: "quickPanel", Label: "快捷面板", Icon: uikit.IconList},
	{ID: "sync", Label: "同步", Icon: uikit.IconRetry},
	{ID: "security", Label: "安全", Icon: uikit.IconShield},
	{ID: "network", Label: "网络", Icon: uikit.IconGlobe},
	{ID: "storage", Label: "存储", Icon: uikit.IconPackage},
	{ID: "about", Label: "关于", Icon: uikit.IconInfo},
}

func (d *demo) settingsPage(c *ui.Context, k uikit.Kit) {
	title := "通用"
	for _, v := range categories {
		if v.ID == d.settings.Value {
			title = v.Label
		}
	}
	k.SplitView(c, uikit.SplitViewProps{Title: title, SidebarHidden: &d.sidebarHidden, SidebarOverlay: d.compact, SidebarToggleLabel: "切换设置侧栏", Sidebar: func() {
		k.SidebarNav(c, &d.settings, uikit.SidebarNavProps{Label: "设置", Sections: []uikit.SidebarNavSection{{ID: "categories", Items: categories}}, Footer: []uikit.SidebarNavItem{{ID: "back", Label: "返回历史", Icon: uikit.IconBack, ShortcutHint: "Esc"}}}).Grow(1)
		if d.settings.Changed() {
			if d.settings.Value == "back" {
				d.page, d.settings.Value = "history", "general"
			}
			if d.compact {
				d.sidebarHidden = true
			}
		}
	}}, func() {
		k.Pane(c, uikit.PaneProps{Kind: uikit.PaneMain, Label: title}, func() {
			k.SettingsContent(c.Key(d.settings.Value), uikit.SettingsContentProps{Label: title}, func() { d.settingsContent(c, k) })
		})
	})
}

func (d *demo) switchRow(c *ui.Context, k uikit.Kit, id, label, description string) {
	k.SettingsRow(c.Key(id), uikit.SettingsRowProps{Label: label, Description: description}, nil, func() {
		on := d.options[id]
		k.Switch(c, &on, uikit.SwitchProps{Label: label})
		d.options[id] = on
	})
}

func (d *demo) selectRow(c *ui.Context, k uikit.Kit, id, label, description string, values ...string) bool {
	changed := false
	k.SettingsRow(c.Key(id), uikit.SettingsRowProps{Label: label, Description: description}, nil, func() {
		value := d.choices[id]
		items := make([]uikit.SelectItem, len(values))
		for i, v := range values {
			items[i] = uikit.SelectItem{Value: v, Label: v}
		}
		r := k.Select(c, uikit.SelectProps{Label: label, Value: &value, Items: items})
		d.choices[id], changed = value, r.Changed
	})
	return changed
}

func (d *demo) settingsContent(c *ui.Context, k uikit.Kit) {
	group := func(id, title string, body func()) {
		k.SettingsGroup(c.Key(id), uikit.SettingsGroupProps{Title: title}, body)
	}
	switch d.settings.Value {
	case "general":
		group("device", "设备", func() {
			k.SettingsRow(c, uikit.SettingsRowProps{Label: "设备名称", Description: "其他设备将显示此名称"}, nil, func() {
				if k.TextField(c, uikit.TextFieldProps{Label: "设备名称", Value: &d.deviceName}).Edited {
					d.devices[0].name = d.deviceName
				}
			})
		})
		group("startup", "启动与托盘", func() {
			d.switchRow(c, k, "startup", "登录时启动", "登录系统后自动启动 UniClipboard")
			d.switchRow(c, k, "tray", "显示托盘图标", "在托盘或菜单栏中显示同步状态")
		})
		group("language", "语言", func() {
			d.selectRow(c, k, "language", "界面语言", "演示控件仅保存本次选择", "简体中文", "English")
		})
		group("sound", "声音", func() { d.switchRow(c, k, "sound", "交互声音", "完成操作时播放提示音") })
		group("privacy", "隐私与诊断", func() {
			d.switchRow(c, k, "telemetry", "发送匿名使用数据", "帮助改进产品体验")
			k.SettingsRow(c, uikit.SettingsRowProps{Label: "诊断信息", Description: "本次演示没有采集诊断日志"}, nil, func() { k.Text(c, "未采集", uikit.TextProps{Tone: uikit.ToneSecondary}) })
		})
	case "appearance":
		group("theme", "外观", func() {
			if d.selectRow(c, k, "appearance", "颜色模式", "跟随系统或选择固定模式", "system", "light", "dark") {
				d.theme = d.choices["appearance"]
			}
			d.selectRow(c, k, "font", "界面字号", "调整文字显示大小", "小", "标准", "大")
			d.switchRow(c, k, "reduceMotion", "减少动态效果", "减少界面切换动画")
		})
	case "shortcuts":
		group("shortcuts", "全局快捷键", func() {
			d.selectRow(c, k, "shortcut", "打开快捷面板", "演示快捷键不会注册到系统", "⌘⇧V", "⌃⌥V", "Ctrl+Shift+V")
		})
		group("window", "窗口快捷键", func() {
			for _, v := range []struct{ name, value string }{{"复制所选内容", "Enter"}, {"置顶或取消置顶", "⌘P"}, {"打开设置", "⌘,"}, {"切换侧栏", "⌃⌘S"}} {
				k.SettingsRow(c.Key(v.name), uikit.SettingsRowProps{Label: v.name}, nil, func() { k.Text(c, v.value, uikit.TextProps{Role: uikit.RoleCallout}) })
			}
		})
	case "quickPanel":
		group("quick", "快捷面板", func() {
			d.switchRow(c, k, "quickPanel", "启用快捷面板", "用全局快捷键打开剪贴板历史")
			d.switchRow(c, k, "preview", "显示内容预览", "在所选条目旁显示详情")
			d.switchRow(c, k, "hover", "显示快捷键提示", "悬停时显示数字键提示")
			k.SettingsRow(c, uikit.SettingsRowProps{Label: "预览快捷面板"}, nil, func() {
				if k.Button(c, "打开预览", uikit.ButtonProps{}).Clicked() {
					d.page = "quick-panel"
				}
			})
		})
	case "sync":
		group("sync", "同步行为", func() {
			d.switchRow(c, k, "sync", "启用同步", "在关联设备之间同步剪贴板")
			d.switchRow(c, k, "autoSync", "自动同步", "复制内容后立即发送")
			d.switchRow(c, k, "restore", "使用历史时同步", "将再次使用的内容发送到其他设备")
		})
		group("file", "文件同步", func() {
			d.selectRow(c, k, "fileLimit", "自动同步文件上限", "超过上限的文件需要手动发送", "10 MB", "50 MB", "100 MB", "无限制")
			d.switchRow(c, k, "saveFiles", "自动保存接收的文件", "将收到的文件保存到下载目录")
		})
	case "security":
		group("security", "空间安全", func() {
			d.switchRow(c, k, "encryption", "端到端加密", "演示空间使用本地假数据")
			k.SettingsRow(c, uikit.SettingsRowProps{Label: "空间口令", Description: "使用口令解锁本机数据"}, nil, func() {
				if k.Button(c, "查看解锁界面", uikit.ButtonProps{}).Clicked() {
					d.page = "unlock"
				}
			})
		})
		group("lock", "自动锁定", func() {
			d.switchRow(c, k, "autoLock", "系统锁定时锁定", "下次打开时需要输入空间口令")
		})
	case "network":
		group("network", "连接", func() {
			d.switchRow(c, k, "relay", "允许中继回退", "无法直连时通过中继连接")
			d.selectRow(c, k, "proxy", "代理模式", "控制出站连接使用的代理", "系统代理", "直接连接", "手动代理")
		})
		group("status", "网络状态", func() {
			k.FactList(c, uikit.FactListProps{Label: "网络状态", Facts: []uikit.Fact{{ID: "lan", Label: "局域网", Value: "可用"}, {ID: "relay", Label: "中继", Value: "已就绪"}}})
		})
	case "storage":
		group("usage", "存储空间", func() {
			k.SettingsRow(c, uikit.SettingsRowProps{Label: "本机历史", Description: "22 条历史 · 13.1 MB"}, nil, func() { k.Text(c, "演示数据", uikit.TextProps{Tone: uikit.ToneSecondary}) })
			d.selectRow(c, k, "retention", "历史保留时间", "到期后清理本机历史", "7 天", "30 天", "90 天", "永久")
			d.switchRow(c, k, "history", "保存本机历史", "在本机保留复制过的内容")
		})
	case "about":
		k.EntityHeader(c, uikit.EntityHeaderProps{Icon: uikit.IconCopy, Title: "UniClipboard", Description: "跨设备剪贴板同步 · 原生界面演示"}, nil)
		group("version", "版本", func() {
			k.SettingsRow(c, uikit.SettingsRowProps{Label: "界面参考", Description: "main · " + Reference[:9]}, nil, nil)
			k.SettingsRow(c, uikit.SettingsRowProps{Label: "界面组件", Description: "mygo + UI Kit · Adwaita / macOS glass / Fluent"}, nil, nil)
		})
		group("preview", "演示入口", func() {
			for _, v := range []struct{ id, name string }{{"setup", "首次引导"}, {"startup", "启动状态"}, {"recovery", "数据恢复"}} {
				k.SettingsRow(c.Key(v.id), uikit.SettingsRowProps{Label: v.name}, nil, func() {
					if k.Button(c, "查看", uikit.ButtonProps{}).Clicked() {
						d.page = v.id
					}
				})
			}
		})
	}
}
