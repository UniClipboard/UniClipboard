package desktopui

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"math"
	"slices"
	"strconv"
	"strings"

	"github.com/egoist/mygo/ui"

	"github.com/uniclipboard/unic/app/desktop/internal/desktopkit"
	api "github.com/uniclipboard/unic/internal/localapi"
	config "github.com/uniclipboard/unic/internal/localconfig"
	"github.com/uniclipboard/unic/plugin"
	"github.com/uniclipboard/unic/plugin/settings"
)

type pluginSettingsForm struct {
	state       api.PluginSettings
	schema      settings.Schema
	values      map[string]any
	savedValues []byte
	message     string
	ready       bool
}

func (f *pluginSettingsForm) load(state api.PluginSettings) error {
	if _, err := settings.Parse(state.Schema); err != nil {
		return err
	}
	var schema settings.Schema
	if err := json.Unmarshal(state.Schema, &schema); err != nil {
		return err
	}
	values, err := settings.DecodeValues(state.Values)
	if err != nil {
		return err
	}
	saved, _ := json.Marshal(values)
	f.state, f.schema, f.values, f.savedValues, f.ready = state, schema, values, saved, true
	f.message = ""
	return nil
}

func (f *pluginSettingsForm) isDirty() bool {
	if !f.ready {
		return false
	}
	cur, err := json.Marshal(f.values)
	if err != nil {
		return true
	}
	return !bytes.Equal(cur, f.savedValues)
}

func (d *viewState) externalPluginStatus(item api.PluginInfo) (string, desktopkit.StatusTone) {
	if item.NeedsSettings {
		return d.tr("需要配置"), desktopkit.StatusWarning
	}
	l := d.live
	if l != nil && slices.Contains(item.Capabilities, "sync_backend") {
		for _, backend := range l.syncBackends {
			if backend.ID == item.ID {
				switch backend.State {
				case "running":
					return d.tr("运行中"), desktopkit.StatusPositive
				case "restarting":
					return d.tr("正在重启"), desktopkit.StatusWarning
				case "unavailable":
					return d.tr("不可用"), desktopkit.StatusCritical
				case "suspended":
					return d.tr("已暂停"), desktopkit.StatusWarning
				}
			}
		}
	}
	if item.RunningVersion != "" {
		if item.Restart {
			return d.tr("需要重启"), desktopkit.StatusWarning
		}
		return d.tr("运行中"), desktopkit.StatusPositive
	}
	if item.Restart && item.Configured {
		return d.tr("需要重启"), desktopkit.StatusWarning
	}
	return d.tr("已停用"), desktopkit.StatusNeutral
}

// pluginDisplay resolves the name and summary shown for an installed external
// plugin: the manifest text for the current language, then the marketplace
// entry, then the bare plugin ID. The summary may be empty.
func (d *viewState) pluginDisplay(item api.PluginInfo) (name, summary string) {
	name, summary = item.Name, item.Summary
	if text, ok := settings.Pick(item.I18N, d.currentLanguage()); ok {
		if text.Name != "" {
			name = text.Name
		}
		if text.Summary != "" {
			summary = text.Summary
		}
	}
	if cat, ok := d.marketplaceEntry(item.ID); ok {
		if name == "" {
			name = cat.Name
		}
		if summary == "" {
			summary = cat.Summary
		}
	}
	if name == "" {
		name = item.ID
	}
	return name, summary
}

// pluginSubtitle is the line under a plugin name. It never repeats the plugin
// ID when the ID is already the title.
func pluginSubtitle(item api.PluginInfo, summary string) string {
	if summary != "" {
		return summary
	}
	if item.ImplementationID != "" && item.ImplementationID != item.ID {
		return item.ImplementationID + " · v" + item.Version
	}
	return "v" + item.Version
}

func (d *viewState) externalPluginDetail(c *ui.Context, k desktopkit.Kit, item api.PluginInfo, cfg config.Config) {
	l := d.live
	p := &d.pluginSettings
	cat, hasCat := d.marketplaceEntry(item.ID)
	title, summary := d.pluginDisplay(item)
	desc := pluginSubtitle(item, summary)
	badge := d.tr("已安装")
	if hasCat && cat.Official {
		badge = d.tr("官方")
	}
	stateText, stateTone := d.externalPluginStatus(item)
	statusColor := desktopkit.TagColor("gray")
	switch stateTone {
	case desktopkit.StatusPositive:
		statusColor = "green"
	case desktopkit.StatusWarning, desktopkit.StatusCritical:
		statusColor = "orange"
	}
	isProcessor := slices.Contains(item.Capabilities, plugin.CapProcessor)
	isGuard := isProcessor && item.ProcessorStage == plugin.ProcessorStageGuard
	icon, tone := pluginPresentationIcon(cat.Category, item.Capabilities, item.Icon, cat.Icon)
	ui.Row(c.Key("external-plugin-header")).Gap(12).AlignItems(ui.Start).Children(func() {
		pluginIconTile(c.Key("ico"), k, icon, tone, 46)
		ui.Column(c.Key("meta")).Grow(1).MinWidth(0).Gap(4).Children(func() {
			ui.Row(c.Key("top")).Gap(8).AlignItems(ui.Center).Wrap().Children(func() {
				k.Text(c.Key("title"), title, desktopkit.TextProps{Role: desktopkit.RoleTitle, Emphasized: true})
				if badge != "" {
					k.Tag(c.Key("badge"), badge, desktopkit.TagProps{Color: "blue"})
				}
				if isGuard {
					k.Tag(c.Key("stage"), d.tr("前置拦截"), desktopkit.TagProps{Color: "purple"})
				}
				k.Tag(c.Key("status"), stateText, desktopkit.TagProps{Color: statusColor})
			})
			k.Text(c.Key("desc"), desc, secondary)
		})
	})

	controlFooter := ""
	if isGuard {
		controlFooter = d.tr("此插件运行在前置拦截阶段，始终先于常规处理器与所有同步后端执行。")
	}
	k.SettingsGroup(c.Key("external-plugin-control"), desktopkit.SettingsGroupProps{Footer: controlFooter}, func() {
		if item.LiveStart && item.RunningVersion == "" && !item.NeedsSettings {
			k.SettingsRow(c.Key("external-plugin-enabled"), desktopkit.SettingsRowProps{Label: d.tr("启动")}, nil, func() {
				if k.Button(c.Key("start"), d.tr("启动"), desktopkit.ButtonProps{Variant: desktopkit.Primary, Disabled: l.showBusy()}).Clicked() {
					d.startExternalPlugin(item.ID)
				}
			})
		} else {
			on := item.Configured
			description := ""
			if !item.LiveStart {
				description = d.tr("退出并重新启动桌面端后生效。")
			}
			k.SettingsRow(c.Key("external-plugin-enabled"), desktopkit.SettingsRowProps{
				Label:       d.tr("启用"),
				Description: description,
			}, nil, func() {
				if k.Switch(c, &on, desktopkit.SwitchProps{Label: d.tr("启用"), Disabled: l.showBusy()}).Changed() && !l.busy {
					d.setExternalPluginEnabled(cfg, item.ID, on)
				}
			})
		}
		if isProcessor {
			instance := cfg.Plugins.Instances[item.ID]
			global := d.tr("继续处理")
			if cfg.Plugins.OnError == "block_sync" {
				global = d.tr("禁止同步此条目")
			}
			policy := instance.OnError
			if policy == "" {
				policy = "inherit"
			}
			errDesc := d.tr("当此插件运行出错或超时时，决定是否允许该条剪贴板继续同步。重启后生效。")
			k.SettingsRow(c.Key("external-plugin-on-error"), desktopkit.SettingsRowProps{
				Label:       d.tr("处理失败时"),
				Description: errDesc,
			}, nil, func() {
				items := []desktopkit.SelectItem{
					{Value: "inherit", Label: fmt.Sprintf(d.tr("跟随全局（%s）"), global)},
					{Value: "continue", Label: d.tr("继续处理")},
					{Value: "block_sync", Label: d.tr("禁止同步此条目")},
				}
				ui.Box(c.Key("on-error-box")).Width(220).Children(func() {
					if k.Select(c, desktopkit.SelectProps{Label: d.tr("处理失败时"), Value: &policy, Items: items, Disabled: l.showBusy()}).Changed && !l.busy {
						instance.OnError = policy
						if policy == "inherit" {
							instance.OnError = ""
						}
						cfg.Plugins.Instances[item.ID] = instance
						d.saveLiveConfig(cfg)
					}
				})
			})
		}
	})

	isSyncBackend := slices.Contains(item.Capabilities, "sync_backend")
	if len(item.Actions) > 0 {
		k.SettingsGroup(c.Key("external-plugin-actions"), desktopkit.SettingsGroupProps{Title: d.tr("动作")}, func() {
			for _, a := range item.Actions {
				accepts := make([]string, 0, len(a.Accepts))
				for _, t := range a.Accepts {
					switch t {
					case "text":
						accepts = append(accepts, d.tr("文本"))
					case "image":
						accepts = append(accepts, d.tr("图片"))
					case "files":
						accepts = append(accepts, d.tr("文件"))
					default:
						accepts = append(accepts, t)
					}
				}
				k.SettingsRow(c.Key("act-"+a.ID), desktopkit.SettingsRowProps{
					Label:       a.Title,
					Description: d.tr("接受：") + strings.Join(accepts, " · "),
				}, nil, nil)
			}
		})
	}

	if hasCat && !item.HasSettings && !isSyncBackend {
		if len(cat.HowTo) > 0 {
			ui.Column(c.Key("external-howto")).Gap(6).Children(func() {
				k.Text(c.Key("h"), d.tr("如何使用"), desktopkit.TextProps{Role: desktopkit.RoleCaption, Tone: desktopkit.ToneSecondary, Emphasized: true})
				pluginHowToCard(c.Key("card"), k, cat.HowTo)
			})
		}
		if cat.Privacy != "" {
			subtleInfoCard(c.Key("privacy"), k, d.tr("隐私与安全"), cat.Privacy)
		}
	}

	weType := isWeType(item)
	canPair := item.Configured && !item.Restart && stateTone == desktopkit.StatusPositive
	if weType {
		d.weTypePairingContent(c.Key("wetype"), k, item, stateText, canPair)
	} else if item.Kind == "go" && isSyncBackend {
		d.createPluginInstance(c.Key("create-instance"), k, item)
	}
	if isSyncBackend {
		d.syncPluginPolicy(c, k, item.ID, cfg)
		if item.ExternalChannels && !weType {
			k.Text(c, d.tr("此后端使用外部渠道。请在插件设置中配置授权。无需设备指纹或设备配对码。"), footnote)
		} else if !item.ExternalChannels && item.TransportKind == "object_store" {
			k.Text(c, d.tr("对象存储设备协议尚未实现。此类型暂不可用。"), footnote)
		}
	}

	if item.HasSettings {
		if p.form == nil {
			p.form = &pluginSettingsForm{}
			d.refreshPluginForm(item.ID, p.form, nil)
		}
		f := p.form
		if !f.ready {
			k.SettingsGroup(c.Key("schema-loading"), desktopkit.SettingsGroupProps{Title: d.tr("插件设置")}, func() {
				k.SettingsRow(c.Key("retry-row"), desktopkit.SettingsRowProps{
					Label:       d.tr("未能加载插件设置"),
					Description: d.tr("点击重试以重新读取当前插件配置。"),
				}, nil, func() {
					if k.Button(c.Key("load-plugin-settings"), d.tr("重试"), desktopkit.ButtonProps{Disabled: l.showBusy()}).Clicked() {
						d.refreshPluginForm(item.ID, f, nil)
					}
				})
			})
		} else {
			valid := d.pluginFormTopLevel(c, k, f, &f.schema, f.values)
			dirty := f.isDirty()
			ui.Row(c.Key("schema-actions")).AlignItems(ui.Center).Justify(ui.SpaceBetween).Gap(12).MinWidth(0).Children(func() {
				ui.Box(c.Key("status")).Grow(1).MinWidth(0).Children(func() {
					switch {
					case !valid:
						k.Text(c.Key("invalid"), d.tr("请修正无效配置项后再保存。"), desktopkit.TextProps{Role: desktopkit.RoleCaption, Tone: desktopkit.ToneCritical})
					case f.state.Restart:
						k.Text(c.Key("restart"), d.tr("配置已保存，退出并重新启动桌面端后生效。"), desktopkit.TextProps{Role: desktopkit.RoleCaption, Tone: desktopkit.ToneSecondary})
					case dirty:
						k.Text(c.Key("dirty"), d.tr("有未保存的更改。"), desktopkit.TextProps{Role: desktopkit.RoleCaption, Tone: desktopkit.ToneSecondary})
					}
				})
				ui.Row(c.Key("buttons")).Gap(8).AlignItems(ui.Center).Shrink(0).Children(func() {
					if k.Button(c.Key("reload-plugin-settings"), d.tr("重新加载"), desktopkit.ButtonProps{Disabled: l.showBusy()}).Clicked() {
						d.refreshPluginForm(item.ID, f, nil)
					}
					if k.Button(c.Key("save-plugin-settings"), d.tr("保存"), desktopkit.ButtonProps{Variant: desktopkit.Primary, Disabled: !valid || l.showBusy()}).Clicked() && !l.busy {
						raw, err := json.Marshal(f.values)
						if err != nil || len(raw) > settings.MaxConfigBytes {
							f.message = "插件设置超过大小限制。"
							d.toastError(d.tr(f.message), desktopkit.ToastOptions{ID: toastIDPluginFeedback})
							return
						}
						d.refreshPluginForm(item.ID, f, &api.PluginSettingsEdit{Revision: f.state.Revision, Values: raw})
					}
				})
			})
		}
	}

	if weType {
		d.weTypeConnectionOptions(c.Key("wetype-options"), k, item, canPair)
	}

	// Collapsible source & technical info (kept out of sight until expanded).
	techIcon := desktopkit.IconChevronRight
	if p.detailTechOpen {
		techIcon = desktopkit.IconChevronDown
	}
	ui.Row(c.Key("detail-tech-row")).AlignItems(ui.Center).Children(func() {
		if k.Button(c.Key("detail-toggle-tech"), d.tr("查看来源与技术信息"), desktopkit.ButtonProps{Variant: desktopkit.Link, Icon: techIcon}).Clicked() {
			p.detailTechOpen = !p.detailTechOpen
		}
	})
	if p.detailTechOpen {
		runningVer := d.tr("未运行")
		if item.RunningVersion != "" {
			runningVer = "v" + item.RunningVersion
		}
		facts := []desktopkit.Fact{
			{ID: "id", Label: d.tr("插件标识"), Value: item.ID, Monospace: true},
			{ID: "ver", Label: d.tr("安装版本"), Value: "v" + item.Version, Monospace: true},
			{ID: "run", Label: d.tr("运行版本"), Value: runningVer, Monospace: true},
		}
		if isProcessor {
			stageLabel := d.tr("常规处理（annotate）")
			if isGuard {
				stageLabel = d.tr("前置拦截（guard）")
			}
			facts = append(facts, desktopkit.Fact{ID: "stage", Label: d.tr("处理阶段"), Value: stageLabel})
		}
		if item.Repository != "" {
			facts = append(facts, desktopkit.Fact{ID: "repo", Label: d.tr("开源仓库"), Value: item.Repository, Monospace: true})
		}
		if len(item.Commit) >= 12 {
			facts = append(facts, desktopkit.Fact{ID: "commit", Label: "Commit", Value: item.Commit[:12], Monospace: true})
		}
		k.FactList(c.Key("external-plugin-facts"), desktopkit.FactListProps{Label: d.tr("来源信息"), Facts: facts})
	}

	if item.HasPrevious || item.Source != "local" {
		updating := d.pluginInstallTask(item.ID) != nil
		k.SettingsGroup(c.Key("external-plugin-manage"), desktopkit.SettingsGroupProps{Title: d.tr("管理")}, func() {
			if item.HasPrevious {
				k.SettingsRow(c.Key("rollback-row"), desktopkit.SettingsRowProps{
					Label:       d.tr("回滚至上一版本"),
					Description: "v" + item.PreviousVersion,
				}, nil, func() {
					if k.Button(c.Key("rollback-btn"), d.tr("回滚"), desktopkit.ButtonProps{Disabled: l.showBusy() || updating}).Clicked() && !l.busy && !updating {
						p.rollbackOpen = true
						p.rollbackID = item.ID
						p.rollbackName = title
						p.rollbackVer = item.PreviousVersion
					}
				})
			}
			if item.Source != "local" {
				k.SettingsRow(c.Key("uninstall-row"), desktopkit.SettingsRowProps{
					Label:       d.tr("卸载插件"),
					Description: d.tr("重启时移除程序文件，保留剪贴板历史与本地配置。"),
				}, nil, func() {
					if k.Button(c.Key("uninstall-btn"), d.tr("卸载"), desktopkit.ButtonProps{Variant: desktopkit.Destructive, Icon: desktopkit.IconDelete, Disabled: l.showBusy() || updating}).Clicked() && !l.busy && !updating {
						p.uninstallOpen = true
						p.uninstallID = item.ID
						p.uninstallName = title
					}
				})
			}
		})
	}
}

func (d *viewState) createPluginInstance(c *ui.Context, k desktopkit.Kit, item api.PluginInfo) {
	l := d.live
	p := &d.pluginSettings
	k.SettingsGroup(c, desktopkit.SettingsGroupProps{Title: d.tr("新增同步实例")}, func() {
		k.SettingsRow(c.Key("create-instance-row"), desktopkit.SettingsRowProps{
			Label:       d.tr("实例 ID"),
			Description: d.tr("新实例使用独立设置和私有数据。配置并启用后重启生效。"),
		}, nil, func() {
			ui.Row(c.Key("create-instance-controls")).Gap(8).AlignItems(ui.Center).Shrink(0).Children(func() {
				ui.Box(c.Key("instance-id")).Width(200).Children(func() {
					k.TextField(c, desktopkit.TextFieldProps{Label: d.tr("实例 ID"), Value: &p.instanceName, Placeholder: "example.work", Disabled: l.showBusy()})
				})
				if k.Button(c.Key("create-instance-btn"), d.tr("创建实例"), desktopkit.ButtonProps{Disabled: l.showBusy() || strings.TrimSpace(p.instanceName) == ""}).Clicked() {
					in := api.PluginInstanceCreate{ID: strings.TrimSpace(p.instanceName), ImplementationID: item.ImplementationID, Revision: l.config.Revision}
					d.mutate(func(ctx context.Context) error { _, err := l.client.CreatePluginInstance(ctx, in); return err }, "实例已创建。请配置并启用。")
				}
			})
		})
	})
}

func (d *viewState) refreshPluginForm(id string, f *pluginSettingsForm, edit *api.PluginSettingsEdit) {
	l := d.live
	if l.busy {
		return
	}
	l.startBusy("")
	if !l.enqueue(func(ctx context.Context) func() {
		var state api.PluginSettings
		var err error
		if edit == nil {
			state, err = l.client.PluginSettings(ctx, id)
		} else {
			state, err = l.client.SavePluginSettings(ctx, id, *edit)
		}
		return func() {
			l.stopBusy()
			if err == nil {
				err = f.load(state)
			}
			if err != nil {
				f.message = "无法应用插件设置。请检查字段后重试。"
				if api.ReasonOf(err) == "conflict" {
					f.message = "数据已变化。请刷新后重试。"
				}
				d.toastError(d.tr(f.message), desktopkit.ToastOptions{ID: toastIDPluginFeedback})
			}
		}
	}) {
		l.stopBusy()
	}
}

func schemaOrder(field *settings.Schema) int {
	if field.UI != nil {
		return field.UI.Order
	}
	return 0
}

func orderedSchemaGroups(schema *settings.Schema, lang string) ([]string, map[string][]string, map[string]settings.Text) {
	keys := make([]string, 0, len(schema.Properties))
	for key := range schema.Properties {
		keys = append(keys, key)
	}
	slices.SortFunc(keys, func(a, b string) int {
		x, y := schemaOrder(schema.Properties[a]), schemaOrder(schema.Properties[b])
		if x < y {
			return -1
		}
		if x > y {
			return 1
		}
		return strings.Compare(a, b)
	})
	texts := make(map[string]settings.Text, len(keys))
	groups := make(map[string][]string)
	var names []string
	for _, key := range keys {
		text := schema.Properties[key].Localized(lang)
		texts[key] = text
		if _, exists := groups[text.Group]; !exists {
			names = append(names, text.Group)
		}
		groups[text.Group] = append(groups[text.Group], key)
	}
	return names, groups, texts
}

func isBlockSchemaField(field *settings.Schema) bool {
	if field.WriteOnly {
		return false
	}
	if field.Type == "array" || field.Type == "object" {
		return true
	}
	return field.Type == "string" && field.UI != nil && field.UI.Widget == "textarea"
}

// pluginFormTopLevel renders each top-level x-unic-ui.group as its own
// BoardUI SettingsGroup card so multi-group schemas read cleanly.
func (d *viewState) pluginFormTopLevel(c *ui.Context, k desktopkit.Kit, f *pluginSettingsForm, schema *settings.Schema, values map[string]any) bool {
	names, groups, texts := orderedSchemaGroups(schema, d.currentLanguage())
	valid := true
	for gi, group := range names {
		title := group
		if title == "" {
			if len(names) == 1 {
				title = d.tr("插件设置")
			} else {
				title = d.tr("常规")
			}
		}
		k.SettingsGroup(c.Key("schema-group-"+strconv.Itoa(gi)), desktopkit.SettingsGroupProps{Title: title}, func() {
			for _, key := range groups[group] {
				ok := d.pluginFormProperty(c, k, f, schema, values, key, texts[key], "")
				valid = valid && ok
			}
		})
	}
	return valid
}

// pluginFormNestedObject renders a nested object's properties inside an inset
// card, keeping group headers subtle without nesting outer SettingsGroups.
func (d *viewState) pluginFormNestedObject(c *ui.Context, k desktopkit.Kit, f *pluginSettingsForm, schema *settings.Schema, values map[string]any, parent string) bool {
	names, groups, texts := orderedSchemaGroups(schema, d.currentLanguage())
	valid := true
	ui.Column(c.Key("nested-card")).MinWidth(0).Radius(8).Background(deviceInsetBg(k, false)).Border(1, deviceCardBorder(k)).Dividers(1, deviceCardBorder(k)).Clip().Children(func() {
		for _, group := range names {
			if group != "" {
				ui.Box(c.Key("ng-"+group)).Padding(8, 14).Children(func() {
					k.Text(c, group, desktopkit.TextProps{Role: desktopkit.RoleCaption, Tone: desktopkit.ToneSecondary, Emphasized: true})
				})
			}
			for _, key := range groups[group] {
				ok := d.pluginFormProperty(c, k, f, schema, values, key, texts[key], parent)
				valid = valid && ok
			}
		}
	})
	return valid
}

func (d *viewState) pluginFormProperty(c *ui.Context, k desktopkit.Kit, f *pluginSettingsForm, parentSchema *settings.Schema, values map[string]any, key string, text settings.Text, parent string) bool {
	field := parentSchema.Properties[key]
	path := parent + "/" + strings.ReplaceAll(strings.ReplaceAll(key, "~", "~0"), "/", "~1")
	label := text.Title
	if label == "" {
		label = key
	}
	required := slices.Contains(parentSchema.Required, key)
	value, present := values[key]

	// Optional unset scalar/block fields render as one clean SettingsRow.
	if !field.WriteOnly && !required && !present {
		k.SettingsRow(c.Key(path), desktopkit.SettingsRowProps{Label: label, Description: text.Description}, nil, func() {
			k.Text(c.Key("unset"), d.tr("未设置"), footnote)
			if k.Button(c.Key("set"), d.tr("设置"), desktopkit.ButtonProps{Variant: desktopkit.Subtle, Size: desktopkit.SizeSmall, Disabled: d.live.showBusy()}).Clicked() {
				values[key] = initialFieldValue(field)
			}
		})
		return true
	}

	if field.WriteOnly {
		ok := true
		k.SettingsRow(c.Key(path), desktopkit.SettingsRowProps{Label: label, Description: text.Description}, nil, func() {
			ok = d.pluginSecretField(c, k, f, field, values, key, path, label, required)
		})
		return ok
	}

	if !present {
		value = initialFieldValue(field)
	}

	if !isBlockSchemaField(field) {
		ok := true
		k.SettingsRow(c.Key(path), desktopkit.SettingsRowProps{Label: label, Description: text.Description}, nil, func() {
			updated, fieldOK := d.pluginInlineControl(c, k, field, value, label, text.OptionLabels)
			values[key] = updated
			ok = fieldOK
			if !required {
				if k.Button(c.Key("unset-btn"), d.tr("移除"), desktopkit.ButtonProps{Variant: desktopkit.Subtle, Size: desktopkit.SizeSmall, Disabled: d.live.showBusy()}).Clicked() {
					delete(values, key)
					ok = true
				}
			}
		})
		return ok
	}

	// Block fields: textarea, array, nested object.
	ok := true
	ui.Column(c.Key(path)).Padding(12, 16).Gap(8).MinWidth(0).Children(func() {
		switch field.Type {
		case "array":
			list, _ := value.([]any)
			addClicked := false
			removedOptional := false
			ui.Row(c.Key("head")).Justify(ui.SpaceBetween).AlignItems(ui.Center).Gap(12).MinWidth(0).Children(func() {
				ui.Column(c.Key("text")).Grow(1).MinWidth(0).Gap(2).Children(func() {
					ui.Row(c.Key("label")).Wrap().Gap(6).AlignItems(ui.Center).Children(func() {
						k.Text(c.Key("l"), label, desktopkit.TextProps{Emphasized: true})
						if len(list) > 0 {
							k.Tag(c.Key("count"), fmt.Sprintf(d.tr("%d 项"), len(list)), desktopkit.TagProps{Color: "gray"})
						}
					})
					if text.Description != "" {
						k.Text(c.Key("d"), text.Description, desktopkit.TextProps{Role: desktopkit.RoleCaption, Tone: desktopkit.ToneSecondary})
					}
				})
				ui.Row(c.Key("actions")).Gap(6).AlignItems(ui.Center).Shrink(0).Children(func() {
					atMax := field.MaxItems != nil && len(list) >= *field.MaxItems
					if k.Button(c.Key("add"), d.tr("添加"), desktopkit.ButtonProps{Icon: desktopkit.IconAdd, Size: desktopkit.SizeSmall, Disabled: d.live.showBusy() || atMax}).Clicked() {
						addClicked = true
					}
					if !required {
						if k.Button(c.Key("unset-btn"), d.tr("移除"), desktopkit.ButtonProps{Variant: desktopkit.Subtle, Size: desktopkit.SizeSmall, Disabled: d.live.showBusy()}).Clicked() {
							removedOptional = true
						}
					}
				})
			})
			if removedOptional {
				delete(values, key)
				ok = true
				return
			}
			if addClicked {
				list = append(list, initialFieldValue(field.Items))
			}
			itemsOK := true
			if len(list) == 0 {
				k.Text(c.Key("empty"), d.tr("暂无配置项"), footnote)
			} else {
				ui.Column(c.Key("items")).Gap(6).MinWidth(0).Children(func() {
					for i, v := range list {
						index := strconv.Itoa(i)
						remove := false
						ui.Row(c.Key(index)).Gap(8).AlignItems(ui.Center).FillWidth().MinWidth(0).Children(func() {
							ui.Box(c.Key("ctrl")).Grow(1).MinWidth(0).Children(func() {
								updated, itemOK := d.pluginArrayItemControl(c.Key("value"), k, field.Items, v, label+" "+index)
								list[i] = updated
								itemsOK = itemsOK && itemOK
							})
							remove = k.Button(c.Key("remove"), "", desktopkit.ButtonProps{
								Variant:  desktopkit.Subtle,
								Icon:     desktopkit.IconDelete,
								Size:     desktopkit.SizeSmall,
								Label:    d.tr("删除"),
								Tooltip:  d.tr("删除"),
								Disabled: d.live.showBusy(),
							}).Clicked()
						})
						if remove {
							list = slices.Delete(list, i, i+1)
							break
						}
					}
				})
			}
			values[key] = list
			arrErr := d.pluginFieldError(field, list)
			if arrErr != "" {
				k.Text(c.Key("err"), arrErr, desktopkit.TextProps{Role: desktopkit.RoleCaption, Tone: desktopkit.ToneCritical})
			}
			ok = itemsOK && arrErr == ""

		case "object":
			removedOptional := false
			ui.Row(c.Key("head")).Justify(ui.SpaceBetween).AlignItems(ui.Center).Gap(12).MinWidth(0).Children(func() {
				ui.Column(c.Key("text")).Grow(1).MinWidth(0).Gap(2).Children(func() {
					k.Text(c.Key("l"), label, desktopkit.TextProps{Emphasized: true})
					if text.Description != "" {
						k.Text(c.Key("d"), text.Description, desktopkit.TextProps{Role: desktopkit.RoleCaption, Tone: desktopkit.ToneSecondary})
					}
				})
				if !required {
					if k.Button(c.Key("unset-btn"), d.tr("移除"), desktopkit.ButtonProps{Variant: desktopkit.Subtle, Size: desktopkit.SizeSmall, Disabled: d.live.showBusy()}).Clicked() {
						removedOptional = true
					}
				}
			})
			if removedOptional {
				delete(values, key)
				ok = true
				return
			}
			child, _ := value.(map[string]any)
			if child == nil {
				child = make(map[string]any)
			}
			ok = d.pluginFormNestedObject(c, k, f, field, child, path)
			values[key] = child

		default: // textarea
			removedOptional := false
			ui.Row(c.Key("head")).Justify(ui.SpaceBetween).AlignItems(ui.Center).Gap(12).MinWidth(0).Children(func() {
				ui.Column(c.Key("text")).Grow(1).MinWidth(0).Gap(2).Children(func() {
					k.Text(c.Key("l"), label, desktopkit.TextProps{Emphasized: true})
					if text.Description != "" {
						k.Text(c.Key("d"), text.Description, desktopkit.TextProps{Role: desktopkit.RoleCaption, Tone: desktopkit.ToneSecondary})
					}
				})
				if !required {
					if k.Button(c.Key("unset-btn"), d.tr("移除"), desktopkit.ButtonProps{Variant: desktopkit.Subtle, Size: desktopkit.SizeSmall, Disabled: d.live.showBusy()}).Clicked() {
						removedOptional = true
					}
				}
			})
			if removedOptional {
				delete(values, key)
				ok = true
				return
			}
			strVal := fmt.Sprint(value)
			ui.TextArea(c.Key("textarea"), &strVal).Label(label).Disabled(d.live.showBusy()).Height(88).FillWidth()
			values[key] = strVal
			fieldErr := d.pluginFieldError(field, strVal)
			if fieldErr != "" {
				k.Text(c.Key("err"), fieldErr, desktopkit.TextProps{Role: desktopkit.RoleCaption, Tone: desktopkit.ToneCritical})
			}
			ok = fieldErr == ""
		}
	})
	return ok
}

func initialFieldValue(field *settings.Schema) any {
	raw := field.Default
	if len(raw) == 0 && len(field.Enum) != 0 {
		raw = field.Enum[0]
	}
	if len(raw) != 0 {
		if values, err := settings.DecodeValues(append(append([]byte("{\"value\":"), raw...), '}')); err == nil {
			return values["value"]
		}
	}
	switch field.Type {
	case "object":
		return map[string]any{}
	case "array":
		return []any{}
	case "boolean":
		return false
	case "integer", "number":
		return json.Number("0")
	}
	return ""
}

func (d *viewState) pluginSecretField(c *ui.Context, k desktopkit.Kit, f *pluginSettingsForm, field *settings.Schema, values map[string]any, key, path, label string, required bool) bool {
	saved := f.state.Secrets[path]
	value, edited := values[key]
	text, _ := value.(string)
	placeholder := d.tr("未设置")
	if saved && !edited {
		placeholder = d.tr("已加密保存")
	}
	errMsg := ""
	if edited && value != nil {
		errMsg = d.pluginFieldError(field, value)
	}
	ui.Box(c.Key("pw-box")).Width(220).Children(func() {
		if k.TextField(c.Key("password"), desktopkit.TextFieldProps{Label: label, Value: &text, Placeholder: placeholder, Password: true, Error: errMsg, Disabled: d.live.showBusy()}).Edited {
			values[key], edited, value = text, true, text
		}
	})
	if (saved || edited) && !required {
		if k.Button(c.Key("clear"), d.tr("清除"), desktopkit.ButtonProps{Variant: desktopkit.Subtle, Size: desktopkit.SizeSmall, Disabled: d.live.showBusy()}).Clicked() {
			values[key], edited, value = nil, true, nil
		}
	}
	if !edited {
		return saved || !required
	}
	if value == nil {
		return !required
	}
	return settings.ValidateField(field, value) == nil
}

func (d *viewState) pluginInlineControl(c *ui.Context, k desktopkit.Kit, field *settings.Schema, value any, label string, optionLabels []string) (any, bool) {
	if len(field.Enum) > 0 {
		data, _ := json.Marshal(value)
		selected := string(data)
		options := make([]desktopkit.SelectItem, 0, len(field.Enum))
		for i, option := range field.Enum {
			name := string(option)
			if len(optionLabels) == len(field.Enum) && len(optionLabels) != 0 {
				name = optionLabels[i]
			} else if field.Type == "string" {
				_ = json.Unmarshal(option, &name)
			}
			options = append(options, desktopkit.SelectItem{Value: string(option), Label: name})
		}
		ui.Box(c.Key("enum-box")).Width(240).Children(func() {
			k.Select(c.Key("enum"), desktopkit.SelectProps{Label: label, Value: &selected, Items: options, Disabled: d.live.showBusy()})
		})
		if parsed, err := settings.DecodeValues([]byte(fmt.Sprintf("{\"value\":%s}", selected))); err == nil {
			value = parsed["value"]
		}
		return value, settings.ValidateField(field, value) == nil
	}
	switch field.Type {
	case "boolean":
		checked, _ := value.(bool)
		k.Switch(c.Key("boolean"), &checked, desktopkit.SwitchProps{Label: label, Disabled: d.live.showBusy()})
		return checked, true
	case "integer", "number":
		num := d.pluginNumberControl(c, k, field, fmt.Sprint(value), label)
		return num, settings.ValidateField(field, num) == nil
	default:
		text := fmt.Sprint(value)
		errMsg := d.pluginFieldError(field, text)
		ui.Box(c.Key("input-box")).Width(220).Children(func() {
			k.TextField(c.Key("input"), desktopkit.TextFieldProps{Label: label, Value: &text, Error: errMsg, Disabled: d.live.showBusy()})
		})
		return text, settings.ValidateField(field, text) == nil
	}
}

func (d *viewState) pluginArrayItemControl(c *ui.Context, k desktopkit.Kit, field *settings.Schema, value any, label string) (any, bool) {
	if len(field.Enum) > 0 {
		data, _ := json.Marshal(value)
		selected := string(data)
		options := make([]desktopkit.SelectItem, 0, len(field.Enum))
		for _, option := range field.Enum {
			name := string(option)
			if field.Type == "string" {
				_ = json.Unmarshal(option, &name)
			}
			options = append(options, desktopkit.SelectItem{Value: string(option), Label: name})
		}
		k.Select(c.Key("enum"), desktopkit.SelectProps{Label: label, Value: &selected, Items: options, Disabled: d.live.showBusy()})
		if parsed, err := settings.DecodeValues([]byte(fmt.Sprintf("{\"value\":%s}", selected))); err == nil {
			value = parsed["value"]
		}
		return value, settings.ValidateField(field, value) == nil
	}
	switch field.Type {
	case "boolean":
		checked, _ := value.(bool)
		k.Switch(c.Key("boolean"), &checked, desktopkit.SwitchProps{Label: label, Disabled: d.live.showBusy()})
		return checked, true
	case "integer", "number":
		num := d.pluginNumberControl(c, k, field, fmt.Sprint(value), label)
		return num, settings.ValidateField(field, num) == nil
	default:
		text := fmt.Sprint(value)
		errMsg := d.pluginFieldError(field, text)
		k.TextField(c.Key("input"), desktopkit.TextFieldProps{Label: label, Value: &text, Error: errMsg, Disabled: d.live.showBusy()})
		return text, settings.ValidateField(field, text) == nil
	}
}

func (d *viewState) pluginNumberControl(c *ui.Context, k desktopkit.Kit, field *settings.Schema, text, label string) json.Number {
	minimum, maximum := -float64(settings.MaxSafeNumber), float64(settings.MaxSafeNumber)
	if field.Minimum != nil {
		minimum = *field.Minimum
	}
	if field.Maximum != nil {
		maximum = *field.Maximum
	}
	errMsg := d.pluginFieldError(field, json.Number(text))
	number, parseErr := strconv.ParseFloat(text, 64)
	validNum := parseErr == nil && errMsg == "" && !math.IsNaN(number) && !math.IsInf(number, 0)
	formatStep := func(v float64) string {
		if field.Type == "integer" {
			return strconv.FormatInt(int64(math.Round(v)), 10)
		}
		return strconv.FormatFloat(v, 'f', -1, 64)
	}
	ui.Row(c).AlignItems(ui.Center).Gap(6).Children(func() {
		ui.Box(c.Key("num-box")).Width(132).Children(func() {
			k.TextField(c.Key("number"), desktopkit.TextFieldProps{Label: label, Value: &text, Error: errMsg, Disabled: d.live.showBusy()})
		})
		if k.Button(c.Key("dec"), "", desktopkit.ButtonProps{
			Variant:  desktopkit.Subtle,
			Icon:     desktopkit.IconRemove,
			Size:     desktopkit.SizeSmall,
			Label:    d.tr("减少") + label,
			Disabled: d.live.showBusy() || !validNum || number-1 < minimum,
		}).Clicked() && validNum && number-1 >= minimum {
			text = formatStep(number - 1)
		}
		if k.Button(c.Key("inc"), "", desktopkit.ButtonProps{
			Variant:  desktopkit.Subtle,
			Icon:     desktopkit.IconAdd,
			Size:     desktopkit.SizeSmall,
			Label:    d.tr("增加") + label,
			Disabled: d.live.showBusy() || !validNum || number+1 > maximum,
		}).Clicked() && validNum && number+1 <= maximum {
			text = formatStep(number + 1)
		}
	})
	return json.Number(text)
}

func (d *viewState) pluginFieldError(field *settings.Schema, value any) string {
	err := settings.ValidateField(field, value)
	if err == nil {
		return ""
	}
	var se *settings.Error
	if !errors.As(err, &se) {
		return d.tr("配置项格式无效。")
	}
	switch se.Code {
	case "range":
		if field.Minimum != nil && field.Maximum != nil {
			return fmt.Sprintf(d.tr("请输入 %g 到 %g 之间的数值。"), *field.Minimum, *field.Maximum)
		}
		if field.Minimum != nil {
			return fmt.Sprintf(d.tr("数值不能小于 %g。"), *field.Minimum)
		}
		if field.Maximum != nil {
			return fmt.Sprintf(d.tr("数值不能大于 %g。"), *field.Maximum)
		}
		return d.tr("请输入有效数值。")
	case "length":
		if field.MinLength != nil && field.MaxLength != nil {
			return fmt.Sprintf(d.tr("长度需在 %d 到 %d 个字符之间。"), *field.MinLength, *field.MaxLength)
		}
		if field.MinLength != nil {
			return fmt.Sprintf(d.tr("长度不能少于 %d 个字符。"), *field.MinLength)
		}
		if field.MaxLength != nil {
			return fmt.Sprintf(d.tr("长度不能超过 %d 个字符。"), *field.MaxLength)
		}
		return d.tr("内容长度不符合要求。")
	case "items":
		if field.MinItems != nil && field.MaxItems != nil {
			return fmt.Sprintf(d.tr("数量需在 %d 到 %d 项之间。"), *field.MinItems, *field.MaxItems)
		}
		if field.MinItems != nil {
			return fmt.Sprintf(d.tr("至少需要 %d 项。"), *field.MinItems)
		}
		if field.MaxItems != nil {
			return fmt.Sprintf(d.tr("最多允许 %d 项。"), *field.MaxItems)
		}
		return d.tr("列表数量不符合要求。")
	case "type":
		if field.Type == "integer" {
			return d.tr("请输入有效整数。")
		}
		if field.Type == "number" {
			return d.tr("请输入有效数值。")
		}
		return d.tr("配置项格式无效。")
	default:
		return d.tr("配置项格式无效。")
	}
}
