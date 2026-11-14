package desktopui

import (
	"maps"
	"strconv"
	"strings"

	"github.com/egoist/mygo/ui"

	"github.com/uniclipboard/unic/app/desktop/internal/uikit"
	config "github.com/uniclipboard/unic/internal/localconfig"
)

// Built-in settings are host policy, not plugin-specific Schema fields.
// Keep draft text separate from the authoritative service snapshot.
type builtinPluginSettings struct {
	id, revision, threshold, thresholdError string
}

var builtinPluginEntries = []struct{ id, name, description string }{
	{"builtin.tailcat", "Tailcat 同步", "设备间同步。支持文本、图片、文件与文件夹。"},
	{"builtin.text", "文本工具", "标注邮箱和链接，提供大写、小写、去除首尾空白和纯文本动作。"},
	{"builtin.classify", "内容分类", "按文本、图片、文件、文件夹和链接添加类型标签。"},
}

func (d *viewState) builtinPluginsContent(c *ui.Context, k uikit.Kit) {
	l := d.live
	cfg := l.config.Config
	// Instances is a map in an otherwise value-copied Config. Never mutate the
	// service snapshot or the pointer fields while an asynchronous save is pending.
	cfg.Plugins.Instances = maps.Clone(cfg.Plugins.Instances)
	if cfg.Plugins.Instances == nil {
		cfg.Plugins.Instances = map[string]config.Instance{}
	}
	if l.config.ApplyError != "" {
		k.InlineNotice(c, uikit.NoticeProps{Tone: uikit.ToneError, Title: d.tr("设置已保存，但未能生效。请重试或重新启动桌面端。")})
	}
	if l.config.Error != "" {
		k.InlineNotice(c, uikit.NoticeProps{Tone: uikit.ToneWarning, Title: d.tr("配置文件无效。当前保留上一份有效设置。")})
	}
	pending := false
	for _, key := range l.config.RestartRequired {
		if key == "plugins" || strings.HasPrefix(key, "plugins.") {
			pending = true
		}
	}
	if pending {
		k.InlineNotice(c, uikit.NoticeProps{Tone: uikit.ToneWarning, Title: d.tr("插件设置已保存。请退出并重新启动桌面端以生效。")})
	}
	p := &d.pluginSettings
	if p.id == "" {
		k.SettingsGroup(c.Key("builtin-plugin-list"), uikit.SettingsGroupProps{Title: d.tr("内置插件")}, func() {
			for _, entry := range builtinPluginEntries {
				k.SettingsRow(c.Key(entry.id), uikit.SettingsRowProps{Label: d.tr(entry.name), Description: d.tr(entry.description)}, nil, func() {
					status := d.tr("已启用")
					if !cfg.PluginEnabled(entry.id) {
						status = d.tr("已停用")
					}
					if pending && entry.id != "builtin.tailcat" {
						status = d.tr("需要重启")
					}
					k.Text(c, status, footnote)
					if k.Button(c.Key("open-"+entry.id), d.tr(entry.name), uikit.ButtonProps{Disabled: l.busy || l.loading}).Clicked() {
						*p = builtinPluginSettings{id: entry.id}
					}
				})
			}
		})
		return
	}
	if k.Button(c.Key("back-to-plugins"), d.tr("返回插件列表"), uikit.ButtonProps{Icon: uikit.IconBack, Disabled: l.busy || l.loading}).Clicked() {
		*p = builtinPluginSettings{}
		return
	}
	name := ""
	for _, entry := range builtinPluginEntries {
		if entry.id == p.id {
			name = d.tr(entry.name)
		}
	}
	if name == "" {
		*p = builtinPluginSettings{}
		return
	}
	changed := false
	instance := cfg.Plugins.Instances[p.id]
	toggle := func(id, label, desc string, on *bool) {
		k.SettingsRow(c.Key(id), uikit.SettingsRowProps{Label: d.tr(label), Description: d.tr(desc)}, nil, func() {
			if k.Switch(c, on, uikit.SwitchProps{Label: d.tr(label), Disabled: l.busy || l.loading}).Changed() {
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
		k.SettingsGroup(c.Key("plugin-detail-"+p.id), uikit.SettingsGroupProps{Title: name}, func() {
			k.SettingsRow(c, uikit.SettingsRowProps{Label: d.tr("内置同步后端"), Description: d.tr("随应用升级，不能单独停用或卸载。")}, nil, nil)
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
			k.SettingsRow(c.Key("threshold"), uikit.SettingsRowProps{Label: d.tr("自动文件传输阈值（MB）"), Description: d.tr("0 表示不增加限制。按整次选择的文件总大小计算。主动发送不受此阈值限制。") + " " + d.tr("输入后按回车保存。")}, nil, func() {
				field := k.TextField(c, uikit.TextFieldProps{Label: d.tr("自动文件传输阈值（MB）"), Value: &p.threshold, Disabled: l.busy || l.loading, Error: d.tr(p.thresholdError)})
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
		k.SettingsGroup(c.Key("plugin-detail-"+p.id), uikit.SettingsGroupProps{Title: name}, func() {
			enabled := cfg.PluginEnabled(p.id)
			label := "启用文本工具"
			if p.id == "builtin.classify" {
				label = "启用内容分类"
			}
			toggle("enabled", label, "退出并重新启动桌面端后生效。", &enabled)
			if changed {
				instance.Enabled = &enabled
			}
			policy, _ := cfg.ProcessorSettings(p.id)
			block := policy == "block_sync"
			k.SettingsRow(c.Key("failure"), uikit.SettingsRowProps{Label: d.tr("处理失败时阻止同步"), Description: d.tr("开启后，处理失败将阻止该条内容同步。关闭后继续同步。重启后生效。")}, nil, func() {
				if k.Switch(c, &block, uikit.SwitchProps{Label: d.tr("处理失败时阻止同步"), Disabled: l.busy || l.loading}).Changed() {
					instance.OnError = "continue"
					if block {
						instance.OnError = "block_sync"
					}
					changed = true
				}
			})
			if instance.OnError == "" {
				k.Text(c, d.tr("当前使用全局失败策略。"), footnote)
			} else {
				if k.Button(c.Key("inherit-failure-policy"), d.tr("使用全局失败策略"), uikit.ButtonProps{Disabled: l.busy || l.loading}).Clicked() {
					instance.OnError = ""
					changed = true
				}
			}
		})
	}
	if changed {
		cfg.Plugins.Instances[p.id] = instance
		d.saveLiveConfig(cfg)
	}
}
