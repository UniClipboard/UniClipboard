package desktopui

import (
	"fmt"
	"maps"
	"strconv"
	"strings"

	"github.com/egoist/mygo/ui"

	"github.com/uniclipboard/unic/app/desktop/internal/desktopkit"
	config "github.com/uniclipboard/unic/internal/localconfig"
)

// Built-in settings are host policy, not plugin-specific Schema fields.
// Keep draft text separate from the authoritative service snapshot.
type builtinPluginSettings struct {
	id, revision, threshold, thresholdError string
	tab, query                              string
	form                                    *pluginSettingsForm
}

type builtinPluginEntry struct {
	id, name, description string
	icon                  desktopkit.IconName
	capabilities, events  []string
	actions               []string
}

var builtinPluginEntries = []builtinPluginEntry{
	{id: "builtin.tailcat", name: "Tailcat 同步", description: "设备间同步。支持文本、图片、文件与文件夹。", icon: desktopkit.IconRetry, capabilities: []string{"同步后端"}},
	{id: "builtin.text", name: "文本工具", description: "标注邮箱和链接，提供大写、小写、去除首尾空白和纯文本动作。", icon: desktopkit.IconEdit,
		capabilities: []string{"自动标注", "动作"}, events: []string{"clipboard.captured", "clipboard.received"}, actions: []string{"转大写", "转小写", "去除首尾空白", "纯文本"}},
	{id: "builtin.classify", name: "内容分类", description: "按文本、图片、文件、文件夹和链接添加类型标签。", icon: desktopkit.IconList,
		capabilities: []string{"自动标注"}, events: []string{"clipboard.captured", "clipboard.received"}},
}

func (d *viewState) builtinPluginsContent(c *ui.Context, k desktopkit.Kit) {
	l := d.live
	cfg := l.config.Config
	// Instances is a map in an otherwise value-copied Config. Never mutate the
	// service snapshot or the pointer fields while an asynchronous save is pending.
	cfg.Plugins.Instances = maps.Clone(cfg.Plugins.Instances)
	if cfg.Plugins.Instances == nil {
		cfg.Plugins.Instances = map[string]config.Instance{}
	}
	if l.config.ApplyError != "" {
		k.InlineNotice(c, desktopkit.NoticeProps{Tone: desktopkit.ToneError, Title: d.tr("设置已保存，但未能生效。请重试或重新启动桌面端。")})
	}
	if l.config.Error != "" {
		k.InlineNotice(c, desktopkit.NoticeProps{Tone: desktopkit.ToneWarning, Title: d.tr("配置文件无效。当前保留上一份有效设置。")})
	}
	pending := false
	for _, key := range l.config.RestartRequired {
		if key == "plugins" || strings.HasPrefix(key, "plugins.") {
			pending = true
		}
	}
	if pending {
		k.InlineNotice(c, desktopkit.NoticeProps{Tone: desktopkit.ToneWarning, Title: d.tr("插件设置已保存。请退出并重新启动桌面端以生效。")})
	}
	p := &d.pluginSettings
	status := func(id string) (string, desktopkit.StatusTone) {
		switch {
		case pending && id != "builtin.tailcat":
			return d.tr("需要重启"), desktopkit.StatusWarning
		case cfg.PluginEnabled(id):
			return d.tr("已启用"), desktopkit.StatusPositive
		}
		return d.tr("已停用"), desktopkit.StatusNeutral
	}
	if p.id == "" {
		d.builtinPluginList(c, k, &cfg)
		return
	}
	var entry builtinPluginEntry
	for _, e := range builtinPluginEntries {
		if e.id == p.id {
			entry = e
		}
	}
	if entry.id == "" {
		for _, item := range l.externalPlugins {
			if item.ID == p.id {
				d.externalPluginDetail(c, k, item, cfg)
				return
			}
		}
		*p = builtinPluginSettings{}
		return
	}
	name := d.tr(entry.name)
	text, tone := status(entry.id)
	k.EntityHeader(c.Key("plugin-header"), desktopkit.EntityHeaderProps{Icon: entry.icon, Title: name, Badge: d.tr("内置"), Status: desktopkit.EntityStatus{Text: text, Tone: tone}, Description: d.tr(entry.description)}, nil)
	if len(entry.actions) > 0 {
		if p.tab == "" {
			p.tab = "overview"
		}
		k.SegmentedControl(c.Key("plugin-tabs"), desktopkit.SegmentedControlProps{Label: d.tr("插件详情"), Value: &p.tab, Items: []desktopkit.SegmentedItem{{Value: "overview", Label: d.tr("概览")}, {Value: "actions", Label: d.tr("动作")}}})
	} else {
		p.tab = "overview"
	}
	if p.tab == "actions" {
		k.SettingsGroup(c.Key("plugin-actions"), desktopkit.SettingsGroupProps{Title: d.tr("动作")}, func() {
			for _, a := range entry.actions {
				k.SettingsRow(c.Key(a), desktopkit.SettingsRowProps{Label: d.tr(a), Description: d.tr("接受：文本")}, nil, nil)
			}
		})
		k.Text(c, d.tr("动作显示在历史条目的“动作”菜单中。结果是新的历史条目。"), footnote)
		return
	}
	facts := []desktopkit.Fact{{ID: "caps", Label: d.tr("能力"), Value: d.join(entry.capabilities)}}
	events := "—"
	if len(entry.events) > 0 {
		events = strings.Join(entry.events, ", ")
	}
	facts = append(facts, desktopkit.Fact{ID: "events", Label: d.tr("订阅事件"), Value: events, Monospace: true}, desktopkit.Fact{ID: "state", Label: d.tr("运行状态"), Value: text})
	k.FactList(c.Key("plugin-facts"), desktopkit.FactListProps{Label: d.tr("插件信息"), Facts: facts})
	changed := false
	instance := cfg.Plugins.Instances[p.id]
	toggle := func(id, label, desc string, on *bool) {
		k.SettingsRow(c.Key(id), desktopkit.SettingsRowProps{Label: d.tr(label), Description: d.tr(desc)}, nil, func() {
			if k.Switch(c, on, desktopkit.SwitchProps{Label: d.tr(label), Disabled: l.busy || l.loading}).Changed() {
				changed = true
			}
		})
	}
	if p.id == "builtin.tailcat" {
		if p.revision != l.config.Revision {
			p.revision = l.config.Revision
			p.threshold = strconv.FormatFloat(float64(instance.Sync.AutoFileMaxBytes)/(1<<20), 'f', -1, 64)
			p.thresholdError = ""
		}
		k.SettingsGroup(c.Key("plugin-detail-"+p.id), desktopkit.SettingsGroupProps{Title: name}, func() {
			k.SettingsRow(c, desktopkit.SettingsRowProps{Label: d.tr("内置同步后端"), Description: d.tr("随应用升级，不能单独停用或卸载。")}, nil, nil)
			k.Text(c, d.tr("以下策略保存后立即生效，仍受全局同步设置限制。"), footnote)
			on := func(v *bool) bool { return v == nil || *v }
			text, images, files := on(instance.Sync.Text), on(instance.Sync.Images), on(instance.Sync.Files)
			toggle("text", "同步文本", "允许此后端发送和接收文本。", &text)
			toggle("images", "同步图片", "允许此后端发送和接收图片。", &images)
			toggle("files", "同步文件与文件夹", "文件夹作为完整内容传输。自动同步目录仍由全局同步设置控制。", &files)
			if changed {
				instance.Sync.Text = &text
				instance.Sync.Images = &images
				instance.Sync.Files = &files
			}
			k.SettingsRow(c.Key("threshold"), desktopkit.SettingsRowProps{Label: d.tr("自动文件传输阈值（MB）"), Description: d.tr("0 表示不增加限制。按整次选择的文件总大小计算。主动发送不受此阈值限制。") + " " + d.tr("输入后按回车保存。")}, nil, func() {
				field := k.TextField(c, desktopkit.TextFieldProps{Label: d.tr("自动文件传输阈值（MB）"), Value: &p.threshold, Disabled: l.busy || l.loading, Error: d.tr(p.thresholdError)})
				if field.Submitted && !l.busy {
					value, err := strconv.ParseInt(strings.TrimSpace(p.threshold), 10, 64)
					if err != nil || value < 0 || value > 4096 {
						p.thresholdError = "请输入 0 到 4096 之间的整数。"
					} else {
						p.thresholdError = ""
						instance.Sync.AutoFileMaxBytes = config.Size(value * (1 << 20))
						changed = true
					}
				}
			})
		})
	} else {
		k.SettingsGroup(c.Key("plugin-detail-"+p.id), desktopkit.SettingsGroupProps{Title: name}, func() {
			enabled := cfg.PluginEnabled(p.id)
			label := "启用文本工具"
			if p.id == "builtin.classify" {
				label = "启用内容分类"
			}
			toggle("enabled", label, "退出并重新启动桌面端后生效。", &enabled)
			if changed {
				instance.Enabled = &enabled
			}
			// One choice for inheritance and both overrides, as in the prototype.
			global := d.tr("继续处理")
			if cfg.Plugins.OnError == "block_sync" {
				global = d.tr("禁止同步此条目")
			}
			policy := instance.OnError
			if policy == "" {
				policy = "inherit"
			}
			k.SettingsRow(c.Key("failure"), desktopkit.SettingsRowProps{Label: d.tr("处理失败时"), Description: d.tr("重启后生效。")}, nil, func() {
				items := []desktopkit.SelectItem{
					{Value: "inherit", Label: fmt.Sprintf(d.tr("跟随全局（%s）"), global)},
					{Value: "continue", Label: d.tr("继续处理")},
					{Value: "block_sync", Label: d.tr("禁止同步此条目")},
				}
				if k.Select(c, desktopkit.SelectProps{Label: d.tr("处理失败时"), Value: &policy, Items: items, Disabled: l.busy || l.loading}).Changed {
					instance.OnError = policy
					if policy == "inherit" {
						instance.OnError = ""
					}
					changed = true
				}
			})
		})
	}
	if changed {
		cfg.Plugins.Instances[p.id] = instance
		d.saveLiveConfig(cfg)
	}
}

func (d *viewState) join(items []string) string {
	out := make([]string, len(items))
	for i, v := range items {
		out[i] = d.tr(v)
	}
	return strings.Join(out, " · ")
}

func (d *viewState) builtinPluginList(c *ui.Context, k desktopkit.Kit, cfg *config.Config) {
	l := d.live
	p := &d.pluginSettings
	search := k.SearchField(c.Key("plugin-search"), desktopkit.SearchFieldProps{Label: d.tr("搜索插件"), Placeholder: d.tr("搜索插件名称或 ID"), Value: &p.query})
	if search.Escaped {
		p.query = ""
	}
	query := strings.ToLower(strings.TrimSpace(p.query))
	k.SettingsGroup(c.Key("builtin-plugin-list"), desktopkit.SettingsGroupProps{Title: d.tr("内置插件")}, func() {
		shown := 0
		for _, entry := range builtinPluginEntries {
			if query != "" && !strings.Contains(strings.ToLower(d.tr(entry.name)+" "+entry.id), query) {
				continue
			}
			shown++
			labelExtra := func() {
				for _, cap := range entry.capabilities {
					k.Tag(c.Key("cap-"+entry.id+cap), d.tr(cap), desktopkit.TagProps{Color: "blue"})
				}
			}
			k.SettingsRow(c.Key(entry.id), desktopkit.SettingsRowProps{Label: d.tr(entry.name), Description: entry.id + " · " + d.tr(entry.description)}, labelExtra, func() {
				on := cfg.PluginEnabled(entry.id)
				if k.Switch(c.Key("enable-"+entry.id), &on, desktopkit.SwitchProps{Label: d.tr("启用") + d.tr(entry.name), Disabled: entry.id == "builtin.tailcat" || l.busy || l.loading}).Changed() {
					instance := cfg.Plugins.Instances[entry.id]
					instance.Enabled = &on
					cfg.Plugins.Instances[entry.id] = instance
					d.saveLiveConfig(*cfg)
				}
				if k.Button(c.Key("open-"+entry.id), "", desktopkit.ButtonProps{Variant: desktopkit.Subtle, Icon: desktopkit.IconChevronRight, Label: d.tr(entry.name), Tooltip: d.tr(entry.name), Disabled: l.busy || l.loading}).Clicked() {
					*p = builtinPluginSettings{id: entry.id}
				}
			})
		}
		if shown == 0 {
			k.Text(c, d.tr("无匹配"), footnote)
		}
	})
	if len(l.externalPlugins) != 0 {
		k.SettingsGroup(c.Key("external-plugins"), desktopkit.SettingsGroupProps{Title: d.tr("外部插件")}, func() {
			for _, item := range l.externalPlugins {
				if query != "" && !strings.Contains(strings.ToLower(item.ID), query) {
					continue
				}
				k.SettingsRow(c.Key(item.ID), desktopkit.SettingsRowProps{Label: item.ID, Description: item.Version}, nil, func() {
					if k.Button(c.Key(item.ID), d.tr("打开"), desktopkit.ButtonProps{Variant: desktopkit.Subtle, Disabled: l.busy}).Clicked() {
						*p = builtinPluginSettings{id: item.ID}
					}
				})
			}
		})
	}

}
