package desktopui

import (
	"strings"

	"github.com/egoist/mygo/ui"

	"github.com/uniclipboard/unic/app/desktop/internal/i18n"
	"github.com/uniclipboard/unic/app/desktop/internal/uikit"
)

func (d *viewState) currentLanguage() string {
	language := d.language
	if d.live != nil && d.live.loaded {
		language = d.live.config.Config.Desktop.Language
	}
	if language == "" {
		return "zh-CN"
	}
	return language
}
func (d *viewState) tr(source string) string { return i18n.Translate(d.currentLanguage(), source) }
func (p *quickPanelState) tr(source string) string {
	if p.live != nil {
		return p.live.d.tr(source)
	}
	return i18n.Translate(p.language, source)
}

func (d *viewState) languageRow(c *ui.Context, k uikit.Kit) {
	value := d.currentLanguage()
	if value == "" {
		value = "zh-CN"
	}
	k.SettingsRow(c.Key("language-choice"), uikit.SettingsRowProps{Label: d.tr("界面语言"), Description: d.tr("立即生效，并保存到本机。")}, nil, func() {
		items := []uikit.SelectItem{
			{Value: "zh-CN", Label: "简体中文"}, {Value: "en", Label: "English"},
			{Value: "ja", Label: "日本語"}, {Value: "pt-BR", Label: "Português (Brasil)"}, {Value: "ru", Label: "Русский"},
		}
		if k.Select(c, uikit.SelectProps{Label: d.tr("界面语言"), Value: &value, Items: items, Disabled: d.live != nil && d.live.busy}).Changed {
			if d.live == nil {
				d.language = value
			} else {
				cfg := d.live.config.Config
				cfg.Desktop.Language = value
				d.saveLiveConfig(cfg)
			}
		}
	})
}

func (d *viewState) searchCandidateLabel(c searchCandidate) string {
	if c.dimension == "tag" {
		return d.tagLabel(c.value)
	}
	if c.dimension == "type" || c.dimension == "time" {
		return d.tr(c.label)
	}
	return c.label
}
func (p *quickPanelState) filterLabel(f qpFilter) string {
	if f.dimension == "tag" && p.live != nil {
		return "#" + p.live.d.tagLabel(f.value)
	}
	if f.dimension == "type" {
		return "/" + p.tr(f.value)
	}
	if f.dimension == "time" {
		return p.tr(f.value)
	}
	return f.label
}
func (p *quickPanelState) actionLabel(a qpAction) string {
	if a.id == "send:windows" || a.id == "send:linux" || a.id == "send:phone" {
		return a.label
	}
	return p.tr(a.label)
}

func (d *viewState) displayDate(value string) string {
	for _, prefix := range []string{"今天", "昨天"} {
		if strings.HasPrefix(value, prefix) {
			return d.tr(prefix) + strings.TrimPrefix(value, prefix)
		}
	}
	return value
}
func (d *viewState) policyText(v device) string {
	if d.currentLanguage() == "" || d.currentLanguage() == "zh-CN" {
		return policySummary(v)
	}
	if v.direction == "pause" {
		return d.tr("同步已暂停。信任和路径保留，不传输任何内容。")
	}
	var kinds []string
	for _, item := range []struct {
		on   bool
		name string
	}{{v.text, "文字"}, {v.image, "图片"}, {v.file, "文件"}} {
		if item.on {
			kinds = append(kinds, d.tr(item.name))
		}
	}
	if len(kinds) == 0 {
		return d.tr("未选择任何内容类型，不会同步。")
	}
	way := map[string]string{"send": "仅发送到此设备", "recv": "仅从此设备接收"}[v.direction]
	if way == "" {
		way = "双向同步"
	}
	return strings.Join(kinds, ", ") + ": " + d.tr(way)
}

func (d *viewState) entryTitle(v entry) string {
	if v.generatedTitle {
		return d.tr(v.title)
	}
	return v.title
}
