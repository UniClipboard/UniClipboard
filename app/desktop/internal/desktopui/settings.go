package desktopui

import (
	"github.com/egoist/mygo/ui"

	"github.com/uniclipboard/unic/app/desktop/internal/desktopkit"
)

var categories = []desktopkit.SidebarNavItem{
	{ID: "general", Label: "通用", Icon: desktopkit.IconSettings},
	{ID: "appearance", Label: "外观", Icon: desktopkit.IconSun},
	{ID: "shortcuts", Label: "快捷键", Icon: desktopkit.IconEdit},
	{ID: "quickPanel", Label: "快捷面板", Icon: desktopkit.IconList},
	{ID: "sync", Label: "同步", Icon: desktopkit.IconRetry},
	{ID: "plugins", Label: "插件", Icon: desktopkit.IconPackage},
	{ID: "security", Label: "安全", Icon: desktopkit.IconShield},
	{ID: "network", Label: "网络", Icon: desktopkit.IconGlobe},
	{ID: "storage", Label: "存储", Icon: desktopkit.IconPackage},
	{ID: "about", Label: "关于", Icon: desktopkit.IconInfo},
}

func (d *viewState) openPluginSettings(instanceID string) {
	if d.deviceTask != "" {
		d.setDeviceTask("")
	}
	d.page = "settings"
	d.settings.Value = "plugins"
	d.renaming = false
	d.deviceDetailOpen = false
	d.pluginSettings = builtinPluginSettings{id: instanceID}
}

func (d *viewState) settingsPage(c *ui.Context, k desktopkit.Kit) {
	items := append([]desktopkit.SidebarNavItem(nil), categories...)
	for i := range items {
		items[i].Label = d.tr(items[i].Label)
	}
	title := d.tr("通用")
	for _, v := range items {
		if v.ID == d.settings.Value {
			title = v.Label
		}
	}
	var titleLeading func()
	var toolbar func()
	if d.live != nil && d.settings.Value == "plugins" {
		if d.pluginSettings.id != "" || d.pluginSettings.view != "" {
			if d.pluginSettings.view == "marketplace" && d.pluginSettings.id == "" {
				title = d.tr("插件市场")
				toolbar = func() {
					if k.Button(c.Key("market-refresh-tb"), d.tr("刷新目录"), desktopkit.ButtonProps{Variant: desktopkit.Subtle, Icon: desktopkit.IconRetry, Busy: d.live.marketplaceLoading, Disabled: d.live.marketplaceLoading}).Clicked() {
						d.ensureMarketplaceLoaded(true)
					}
				}
			} else if d.pluginSettings.view == "advanced" && d.pluginSettings.id == "" {
				title = d.tr("执行顺序与高级设置")
			}
			titleLeading = func() {
				label := d.tr("返回插件列表")
				if k.Button(c.Key("back-to-plugins"), "", desktopkit.ButtonProps{Variant: desktopkit.Subtle, Icon: desktopkit.IconBack, Label: label, Tooltip: label, Disabled: d.live.busy || d.live.loading}).Clicked() {
					d.pluginSettings.id = ""
					d.pluginSettings.view = ""
					d.pluginSettings.form = nil
				}
			}
		} else {
			toolbar = func() {
				d.pluginsToolbarActions(c, k)
			}
		}
	}
	k.SplitView(c, desktopkit.SplitViewProps{Toolbar: toolbar, ToolbarTrailing: d.trailingToolbar(c), Title: title, TitleLeading: titleLeading, SidebarHidden: &d.sidebarHidden, SidebarOverlay: d.compact, SidebarToggleLabel: d.tr("切换设置侧栏"), Sidebar: func() {
		k.SidebarNav(c, &d.settings, desktopkit.SidebarNavProps{Label: d.tr("设置"), Sections: []desktopkit.SidebarNavSection{{ID: "categories", Items: items}}, Footer: []desktopkit.SidebarNavItem{{ID: "back", Label: d.tr("返回历史"), Icon: desktopkit.IconBack, ShortcutHint: "Esc"}}}).Grow(1)
		if d.settings.Changed() {
			if d.settings.Value == "back" {
				d.page, d.settings.Value = "history", "general"
			}
			if d.compact {
				d.sidebarHidden = true
			}
		}
	}}, func() {
		k.Pane(c, desktopkit.PaneProps{Kind: desktopkit.PaneMain, Label: title}, func() {
			if d.live != nil && d.settings.Value == "plugins" && d.pluginSettings.view == "marketplace" && d.pluginSettings.id == "" {
				d.pluginsMarketplaceWorkspace(c, k)
				return
			}
			k.SettingsContent(c.Key(d.settings.Value), desktopkit.SettingsContentProps{Label: title}, func() { d.settingsContent(c, k) })
		})
	})
}

func (d *viewState) switchRow(c *ui.Context, k desktopkit.Kit, id, label, description string) {
	k.SettingsRow(c.Key(id), desktopkit.SettingsRowProps{Label: label, Description: description}, nil, func() {
		on := d.options[id]
		k.Switch(c, &on, desktopkit.SwitchProps{Label: label})
		d.options[id] = on
	})
}

func (d *viewState) selectRow(c *ui.Context, k desktopkit.Kit, id, label, description string, values ...string) bool {
	changed := false
	k.SettingsRow(c.Key(id), desktopkit.SettingsRowProps{Label: label, Description: description}, nil, func() {
		value := d.choices[id]
		items := make([]desktopkit.SelectItem, len(values))
		for i, v := range values {
			items[i] = desktopkit.SelectItem{Value: v, Label: d.tr(v)}
		}
		r := k.Select(c, desktopkit.SelectProps{Label: label, Value: &value, Items: items})
		d.choices[id], changed = value, r.Changed
	})
	return changed
}

func (d *viewState) settingsContent(c *ui.Context, k desktopkit.Kit) {
	if d.live != nil {
		d.liveSettingsContent(c, k)
		return
	}
	group := func(id, title string, body func()) {
		k.SettingsGroup(c.Key(id), desktopkit.SettingsGroupProps{Title: title}, body)
	}
	switch d.settings.Value {
	case "general":
		group("device", d.tr("设备"), func() {
			k.SettingsRow(c, desktopkit.SettingsRowProps{Label: d.tr("设备名称"), Description: d.tr("其他设备将显示此名称")}, nil, func() {
				if k.TextField(c, desktopkit.TextFieldProps{Label: d.tr("设备名称"), Value: &d.deviceName}).Edited {
					d.devices[0].name = d.deviceName
				}
			})
		})
		group("startup", d.tr("启动与托盘"), func() {
			d.switchRow(c, k, "startup", d.tr("登录时启动"), d.tr("登录系统后自动启动 UniClipboard"))
			d.switchRow(c, k, "tray", d.tr("显示托盘图标"), d.tr("在托盘或菜单栏中显示同步状态"))
			k.SettingsRow(c, desktopkit.SettingsRowProps{Label: d.tr("托盘图标"), Description: d.tr("更改后立即生效。")}, nil, func() {
				value := d.choices["trayIcon"]
				if value == "" {
					value = "glyph"
				}
				k.Select(c, desktopkit.SelectProps{Label: d.tr("托盘图标"), Value: &value, Items: []desktopkit.SelectItem{{Value: "glyph", Label: d.tr("默认")}, {Value: "cat", Label: d.tr("经典猫头")}}})
				d.choices["trayIcon"] = value
			})
		})
		group("language", d.tr("语言"), func() {
			d.languageRow(c, k)
		})
		group("sound", d.tr("声音"), func() { d.switchRow(c, k, "sound", d.tr("交互声音"), d.tr("完成操作时播放提示音")) })
		group("privacy", d.tr("隐私与诊断"), func() {
			d.switchRow(c, k, "telemetry", d.tr("发送匿名使用数据"), d.tr("帮助改进产品体验"))
			k.SettingsRow(c, desktopkit.SettingsRowProps{Label: d.tr("诊断信息"), Description: d.tr("本次演示没有采集诊断日志")}, nil, func() { k.Text(c, d.tr("未采集"), desktopkit.TextProps{Tone: desktopkit.ToneSecondary}) })
		})
	case "appearance":
		group("theme", d.tr("外观"), func() {
			if d.selectRow(c, k, "appearance", d.tr("颜色模式"), d.tr("跟随系统或选择固定模式"), "system", "light", "dark") {
				d.theme = d.choices["appearance"]
			}
			d.selectRow(c, k, "font", d.tr("界面字号"), d.tr("调整文字显示大小"), "小", "标准", "大")
			d.switchRow(c, k, "reduceMotion", d.tr("减少动态效果"), d.tr("减少界面切换动画"))
		})
	case "shortcuts":
		group("shortcuts", d.tr("全局快捷键"), func() {
			d.selectRow(c, k, "shortcut", d.tr("打开快捷面板"), d.tr("演示快捷键不会注册到系统"), "⌘⇧V", "⌃⌥V", "Ctrl+Shift+V")
		})
		group("window", d.tr("窗口快捷键"), func() {
			for _, v := range []struct{ name, value string }{{d.tr("复制所选内容"), "Enter"}, {d.tr("置顶或取消置顶"), "⌘P"}, {d.tr("打开设置"), "⌘,"}, {d.tr("切换侧栏"), "⌃⌘S"}} {
				k.SettingsRow(c.Key(v.name), desktopkit.SettingsRowProps{Label: v.name}, nil, func() { k.Text(c, v.value, desktopkit.TextProps{Role: desktopkit.RoleCallout}) })
			}
		})
	case "quickPanel":
		group("quick", d.tr("快捷面板"), func() {
			d.switchRow(c, k, "quickPanel", d.tr("启用快捷面板"), d.tr("用全局快捷键打开剪贴板历史"))
			d.switchRow(c, k, "preview", d.tr("显示内容预览"), d.tr("在所选条目旁显示详情"))
			d.switchRow(c, k, "hover", d.tr("显示快捷键提示"), d.tr("悬停时显示数字键提示"))
			k.SettingsRow(c, desktopkit.SettingsRowProps{Label: d.tr("预览快捷面板")}, nil, func() {
				if k.Button(c, d.tr("打开预览"), desktopkit.ButtonProps{}).Clicked() {
					if d.openQuickPanel != nil {
						d.openQuickPanel()
					} else {
						d.page = "quick-panel"
					}
				}
			})
		})
	case "sync":
		group("sync", d.tr("同步行为"), func() {
			d.switchRow(c, k, "sync", d.tr("启用同步"), d.tr("在关联设备之间同步剪贴板"))
			d.switchRow(c, k, "autoSync", d.tr("自动同步"), d.tr("复制内容后立即发送"))
			d.switchRow(c, k, "restore", d.tr("使用历史时同步"), d.tr("将再次使用的内容发送到其他设备"))
		})
		group("file", d.tr("文件同步"), func() {
			d.selectRow(c, k, "fileLimit", d.tr("自动同步文件上限"), d.tr("超过上限的文件需要手动发送"), "10 MB", "50 MB", "100 MB", "无限制")
			d.switchRow(c, k, "saveFiles", d.tr("自动保存接收的文件"), d.tr("将收到的文件保存到下载目录"))
		})
	case "plugins":
		group("sync-channels", d.tr("同步渠道"), func() {
			for i := range d.syncChannels {
				ch := &d.syncChannels[i]
				desc := ch.implementationID
				if ch.policy != "" {
					desc += " · " + ch.policy
				}
				k.SettingsRow(c.Key("demo-ch-"+ch.id), desktopkit.SettingsRowProps{Label: d.channelDisplayName(ch.id), Description: desc}, nil, func() {
					on := ch.state == "running"
					if k.Switch(c.Key("en-"+ch.id), &on, desktopkit.SwitchProps{Label: d.tr("启用") + d.channelDisplayName(ch.id)}).Changed() {
						if on {
							ch.state = "running"
						} else {
							ch.state = "disabled"
						}
					}
				})
			}
		})
	case "security":
		k.Text(c, d.tr("设备配对需要双方核对指纹。"), secondary)

	case "network":
		group("network", d.tr("连接"), func() {
			d.switchRow(c, k, "relay", d.tr("允许中继回退"), d.tr("无法直连时通过中继连接"))
			d.selectRow(c, k, "proxy", d.tr("代理模式"), d.tr("控制出站连接使用的代理"), "系统代理", "直接连接", "手动代理")
		})
		group("status", d.tr("网络状态"), func() {
			k.FactList(c, desktopkit.FactListProps{Label: d.tr("网络状态"), Facts: []desktopkit.Fact{{ID: "lan", Label: d.tr("局域网"), Value: d.tr("可用")}, {ID: "relay", Label: d.tr("中继"), Value: d.tr("已就绪")}}})
		})
	case "storage":
		group("usage", d.tr("存储空间"), func() {
			k.SettingsRow(c, desktopkit.SettingsRowProps{Label: d.tr("本机历史"), Description: d.tr("22 条历史 · 13.1 MB")}, nil, func() { k.Text(c, d.tr("演示数据"), desktopkit.TextProps{Tone: desktopkit.ToneSecondary}) })
			d.selectRow(c, k, "retention", d.tr("历史保留时间"), d.tr("到期后清理本机历史"), "7 天", "30 天", "90 天", "永久")
			d.switchRow(c, k, "history", d.tr("保存本机历史"), d.tr("在本机保留复制过的内容"))
		})
	case "about":
		d.aboutContent(c, k)
	}
}
