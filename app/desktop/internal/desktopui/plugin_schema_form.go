package desktopui

import (
	"context"
	"encoding/json"
	"fmt"
	"slices"
	"strconv"
	"strings"

	"github.com/egoist/mygo/ui"

	"github.com/uniclipboard/unic/app/desktop/internal/uikit"
	api "github.com/uniclipboard/unic/internal/localapi"
	config "github.com/uniclipboard/unic/internal/localconfig"
	"github.com/uniclipboard/unic/plugin/settings"
)

type pluginSettingsForm struct {
	state   api.PluginSettings
	schema  settings.Schema
	values  map[string]any
	message string
	ready   bool
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
	f.state, f.schema, f.values, f.ready = state, schema, values, true
	f.message = ""
	return nil
}

func (d *viewState) externalPluginDetail(c *ui.Context, k uikit.Kit, item api.PluginInfo, cfg config.Config) {
	l := d.live
	p := &d.pluginSettings
	k.EntityHeader(c.Key("external-plugin-header"), uikit.EntityHeaderProps{Icon: uikit.IconEdit, Title: item.ID, Description: item.Version}, nil)
	on := item.Configured
	k.SettingsRow(c.Key("external-plugin-enabled"), uikit.SettingsRowProps{Label: d.tr("启用")}, nil, func() {
		if !k.Switch(c, &on, uikit.SwitchProps{Label: d.tr("启用"), Disabled: l.busy || l.loading}).Changed() {
			return
		}
		cfg.Plugins.Lua = slices.DeleteFunc(slices.Clone(cfg.Plugins.Lua), func(id string) bool { return id == item.ID })
		cfg.Plugins.Order = slices.DeleteFunc(slices.Clone(cfg.Plugins.Order), func(id string) bool { return id == item.ID })
		if in, known := cfg.Plugins.Instances[item.ID]; known {
			in.Enabled = nil
			if !on {
				in.Enabled = &on
			}
			cfg.Plugins.Instances[item.ID] = in
		}
		if on {
			cfg.Plugins.Order = append(cfg.Plugins.Order, item.ID)
		}
		d.saveLiveConfig(cfg)
	})
	if item.Restart || p.form != nil && p.form.state.Restart {
		k.InlineNotice(c, uikit.NoticeProps{Tone: uikit.ToneWarning, Title: d.tr("需要重启")})
	}
	if !item.HasSettings {
		return
	}
	if p.form == nil {
		p.form = &pluginSettingsForm{}
		d.refreshPluginForm(item.ID, p.form, nil)
	}
	f := p.form
	if f.message != "" {
		k.InlineNotice(c, uikit.NoticeProps{Tone: uikit.ToneError, Title: d.tr(f.message)})
	}
	if !f.ready {
		if k.Button(c.Key("load-plugin-settings"), d.tr("重试"), uikit.ButtonProps{Disabled: l.busy}).Clicked() {
			d.refreshPluginForm(item.ID, f, nil)
		}
		return
	}
	valid := true
	k.SettingsGroup(c.Key("schema-settings"), uikit.SettingsGroupProps{Title: d.tr("插件设置")}, func() {
		valid = d.pluginFormObject(c, k, f, &f.schema, f.values, "")
	})
	ui.Row(c).Gap(8).Children(func() {
		if k.Button(c.Key("reload-plugin-settings"), d.tr("重新加载"), uikit.ButtonProps{Disabled: l.busy}).Clicked() {
			d.refreshPluginForm(item.ID, f, nil)
		}
		if k.Button(c.Key("save-plugin-settings"), d.tr("保存"), uikit.ButtonProps{Variant: uikit.Primary, Disabled: !valid || l.busy}).Clicked() {
			raw, err := json.Marshal(f.values)
			if err != nil || len(raw) > settings.MaxConfigBytes {
				f.message = "插件设置超过大小限制。"
				return
			}
			d.refreshPluginForm(item.ID, f, &api.PluginSettingsEdit{Revision: f.state.Revision, Values: raw})
		}
	})
}

func (d *viewState) refreshPluginForm(id string, f *pluginSettingsForm, edit *api.PluginSettingsEdit) {
	l := d.live
	if l.busy {
		return
	}
	l.busy = true
	if !l.enqueue(func(ctx context.Context) func() {
		var state api.PluginSettings
		var err error
		if edit == nil {
			state, err = l.client.PluginSettings(ctx, id)
		} else {
			state, err = l.client.SavePluginSettings(ctx, id, *edit)
		}
		return func() {
			l.busy = false
			if err == nil {
				err = f.load(state)
			}
			if err != nil {
				f.message = "无法应用插件设置。请检查字段后重试。"
				if api.ReasonOf(err) == "conflict" {
					f.message = "数据已变化。请刷新后重试。"
				}
			}
		}
	}) {
		l.busy = false
	}
}

func schemaOrder(field *settings.Schema) int {
	if field.UI != nil {
		return field.UI.Order
	}
	return 0
}

func (d *viewState) pluginFormObject(c *ui.Context, k uikit.Kit, f *pluginSettingsForm, schema *settings.Schema, values map[string]any, parent string) bool {
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
	groups := make(map[string][]string)
	var names []string
	for _, key := range keys {
		var group string
		if hints := schema.Properties[key].UI; hints != nil {
			group = hints.Group
		}
		if _, exists := groups[group]; !exists {
			names = append(names, group)
		}
		groups[group] = append(groups[group], key)
	}
	valid := true
	for _, group := range names {
		if group != "" {
			k.Text(c.Key("group-"+group), group, uikit.TextProps{Emphasized: true})
		}
		for _, key := range groups[group] {
			field := schema.Properties[key]
			path := parent + "/" + strings.ReplaceAll(strings.ReplaceAll(key, "~", "~0"), "/", "~1")
			label := field.Title
			if label == "" {
				label = key
			}
			required := slices.Contains(schema.Required, key)
			value, present := values[key]
			ui.Column(c.Key(path)).Gap(4).Children(func() {
				k.Text(c, label, uikit.TextProps{Emphasized: true})
				if field.Description != "" {
					k.Text(c, field.Description, footnote)
				}
				if field.WriteOnly {
					ok := d.pluginSecretField(c, k, f, field, values, key, path, label, required)
					valid = valid && ok
					return
				}
				if !required {
					if k.Switch(c.Key("present"), &present, uikit.SwitchProps{Label: label}).Changed() {
						if present {
							value = initialFieldValue(field)
						} else {
							delete(values, key)
						}
					}
					if !present {
						return
					}
				} else if !present {
					value = initialFieldValue(field)
				}
				updated, ok := d.pluginFormField(c, k, f, field, value, path, label)
				values[key] = updated
				valid = valid && ok
			})
		}
	}
	return valid
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

func (d *viewState) pluginSecretField(c *ui.Context, k uikit.Kit, f *pluginSettingsForm, field *settings.Schema, values map[string]any, key, path, label string, required bool) bool {
	saved := f.state.Secrets[path]
	value, edited := values[key]
	text, _ := value.(string)
	placeholder := d.tr("未设置")
	if saved && !edited {
		placeholder = d.tr("已设置")
	}
	if k.TextField(c.Key("password"), uikit.TextFieldProps{Label: label, Value: &text, Placeholder: placeholder, Password: true}).Edited {
		values[key], edited, value = text, true, text
	}
	if (saved || edited) && !required {
		if k.Button(c.Key("clear"), d.tr("清除"), uikit.ButtonProps{Variant: uikit.Subtle}).Clicked() {
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

func (d *viewState) pluginFormField(c *ui.Context, k uikit.Kit, f *pluginSettingsForm, field *settings.Schema, value any, path, label string) (any, bool) {
	switch field.Type {
	case "object":
		child, _ := value.(map[string]any)
		if child == nil {
			child = make(map[string]any)
		}
		return child, d.pluginFormObject(c, k, f, field, child, path)
	case "array":
		list, _ := value.([]any)
		valid := true
		for i, v := range list {
			index := strconv.Itoa(i)
			remove := false
			ui.Row(c.Key(index)).Gap(4).Children(func() {
				updated, ok := d.pluginFormField(c.Key("value"), k, f, field.Items, v, path+"/"+index, label+" "+index)
				list[i] = updated
				valid = valid && ok
				remove = k.Button(c.Key("remove"), d.tr("删除"), uikit.ButtonProps{Variant: uikit.Subtle}).Clicked()
			})
			if remove {
				list = slices.Delete(list, i, i+1)
				break
			}
		}
		if k.Button(c.Key("add"), d.tr("添加"), uikit.ButtonProps{Disabled: field.MaxItems != nil && len(list) >= *field.MaxItems}).Clicked() {
			list = append(list, initialFieldValue(field.Items))
		}
		valid = valid && settings.ValidateField(field, list) == nil
		return list, valid
	}
	if len(field.Enum) > 0 {
		data, _ := json.Marshal(value)
		selected := string(data)
		options := make([]uikit.SelectItem, 0, len(field.Enum))
		for i, option := range field.Enum {
			name := string(option)
			if field.UI != nil && len(field.UI.OptionLabels) != 0 {
				name = field.UI.OptionLabels[i]
			} else if field.Type == "string" {
				_ = json.Unmarshal(option, &name)
			}
			options = append(options, uikit.SelectItem{Value: string(option), Label: name})
		}
		k.Select(c.Key("enum"), uikit.SelectProps{Label: label, Value: &selected, Items: options})
		if parsed, err := settings.DecodeValues([]byte(fmt.Sprintf("{\"value\":%s}", selected))); err == nil {
			value = parsed["value"]
		}
	} else if field.Type == "boolean" {
		checked, _ := value.(bool)
		k.Switch(c.Key("boolean"), &checked, uikit.SwitchProps{Label: label})
		value = checked
	} else if field.Type == "integer" || field.Type == "number" {
		value = d.pluginNumberField(c, k, field, fmt.Sprint(value), label)
	} else {
		text := fmt.Sprint(value)
		if field.UI != nil && field.UI.Widget == "textarea" {
			ui.TextArea(c.Key("textarea"), &text).Label(label).Disabled(d.live.busy).Height(88).FillWidth()
		} else {
			k.TextField(c.Key("input"), uikit.TextFieldProps{Label: label, Value: &text, Disabled: d.live.busy})
		}
		value = text
	}
	return value, settings.ValidateField(field, value) == nil
}

func (d *viewState) pluginNumberField(c *ui.Context, k uikit.Kit, field *settings.Schema, text, label string) json.Number {
	minimum, maximum := -float64(settings.MaxSafeNumber), float64(settings.MaxSafeNumber)
	if field.Minimum != nil {
		minimum = *field.Minimum
	}
	if field.Maximum != nil {
		maximum = *field.Maximum
	}
	ui.Row(c).AlignItems(ui.Start).Gap(8).Children(func() {
		hint := ""
		if settings.ValidateField(field, json.Number(text)) != nil {
			hint = d.tr("请输入有效数值。")
		}
		k.TextField(c.Key("number"), uikit.TextFieldProps{Label: label, Value: &text, Error: hint, Disabled: d.live.busy})
		number, err := strconv.ParseFloat(text, 64)
		if ui.Stepper(c.Key("stepper"), &number, minimum, maximum, 1).Label(label).Disabled(d.live.busy || err != nil || hint != "").Changed() {
			text = strconv.FormatFloat(number, 'f', -1, 64)
		}
	})
	return json.Number(text)
}
