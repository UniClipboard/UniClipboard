package desktopui

import (
	"fmt"
	"slices"
	"strconv"
	"strings"

	"github.com/egoist/mygo/ui"

	"github.com/uniclipboard/unic/app/desktop/internal/desktopkit"
	config "github.com/uniclipboard/unic/internal/localconfig"
	"github.com/uniclipboard/unic/internal/plugins/builtinclassify"
)

type classificationSettings struct {
	saved   []builtinclassify.Rule
	draft   builtinclassify.Rule
	open    bool
	index   int // len(saved) when adding a rule
	invalid bool
}

func (d *viewState) classificationRules(c *ui.Context, k desktopkit.Kit, cfg config.Config, instance config.Instance) {
	form := &d.pluginSettings.classification
	if !slices.Equal(form.saved, instance.Rules) {
		*form = classificationSettings{saved: instance.Rules}
	}
	busy := d.live.busy || d.live.loading
	save := func(rules []builtinclassify.Rule) {
		instance.Rules = rules
		cfg.Plugins.Instances[builtinclassify.ID] = instance
		d.saveLiveConfigThen(cfg, func() {
			*form = classificationSettings{saved: rules}
		})
	}
	group := c.Key("classification-rules")
	k.SettingsGroup(group, desktopkit.SettingsGroupProps{
		Title:  d.tr("正则规则"),
		Footer: d.tr("每条匹配的规则都会给新文本添加标签。重启服务后生效。"),
	}, func() {
		for i, rule := range instance.Rules {
			row := group.Key(strconv.Itoa(i))
			label := d.tagLabel(rule.Tag)
			k.SettingsRow(row, desktopkit.SettingsRowProps{Label: label, Description: rule.Pattern}, nil, func() {
				ui.Row(row).Gap(6).Children(func() {
					edit := fmt.Sprintf(d.tr("编辑规则 %s"), label)
					if k.Button(row.Key("edit"), "", desktopkit.ButtonProps{Label: edit, Icon: desktopkit.IconEdit, Variant: desktopkit.Subtle, Disabled: busy || form.open}).Clicked() {
						form.open, form.index, form.draft = true, i, rule
					}
					remove := fmt.Sprintf(d.tr("删除规则 %s"), label)
					if k.Button(row.Key("delete"), "", desktopkit.ButtonProps{Label: remove, Icon: desktopkit.IconDelete, Variant: desktopkit.Subtle, Disabled: busy || form.open}).Clicked() {
						save(slices.Delete(slices.Clone(instance.Rules), i, i+1))
					}
				})
			})
		}
		if !form.open {
			k.SettingsRow(group.Key("add-row"), desktopkit.SettingsRowProps{}, nil, func() {
				if k.Button(group.Key("add"), d.tr("添加规则"), desktopkit.ButtonProps{Icon: desktopkit.IconAdd, Variant: desktopkit.Subtle, Disabled: busy || len(instance.Rules) >= builtinclassify.MaxRules}).Clicked() {
					form.open, form.index = true, len(instance.Rules)
				}
			})
			return
		}
		editor := group.Key("editor")
		ui.Column(editor).Padding(12, 16).Gap(10).Children(func() {
			k.TextField(editor.Key("pattern"), desktopkit.TextFieldProps{Label: d.tr("正则表达式"), Value: &form.draft.Pattern, Placeholder: `\bUNI-[0-9]+\b`, Disabled: busy})
			k.TextField(editor.Key("tag"), desktopkit.TextFieldProps{Label: d.tr("目标标签"), Value: &form.draft.Tag, Disabled: busy})
			if form.invalid {
				k.InlineNotice(editor.Key("invalid"), desktopkit.NoticeProps{Tone: desktopkit.ToneError, Title: d.tr("请输入有效的 Go 正则表达式和标签。表达式最多 1024 字节，标签最多 64 字节。")})
			}
			ui.Row(editor.Key("actions")).Gap(8).Children(func() {
				if k.Button(editor.Key("cancel"), d.tr("取消"), desktopkit.ButtonProps{Variant: desktopkit.Subtle, Disabled: busy}).Clicked() {
					*form = classificationSettings{saved: instance.Rules}
				}
				if k.Button(editor.Key("save"), d.tr("保存规则"), desktopkit.ButtonProps{Variant: desktopkit.Primary, Disabled: busy}).Clicked() {
					rule := form.draft
					rule.Tag = strings.TrimSpace(rule.Tag)
					rules := slices.Clone(instance.Rules)
					if form.index == len(rules) {
						rules = append(rules, rule)
					} else {
						rules[form.index] = rule
					}
					_, err := builtinclassify.New(rules)
					form.invalid = err != nil
					if !form.invalid {
						save(rules)
					}
				}
			})
		})
	})
	k.Text(c, d.tr("Lua 规则使用本地插件。脚本通过 ctx:add_tag 添加标签。"), footnote)
}
