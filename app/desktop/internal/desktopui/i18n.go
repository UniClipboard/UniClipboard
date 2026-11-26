package desktopui

import (
	"strings"

	"github.com/egoist/mygo/ui"

	"github.com/uniclipboard/unic/app/desktop/internal/desktopkit"
	"github.com/uniclipboard/unic/app/desktop/internal/i18n"
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

func (d *viewState) languageRow(c *ui.Context, k desktopkit.Kit) {
	value := d.currentLanguage()
	if value == "" {
		value = "zh-CN"
	}
	k.SettingsRow(c.Key("language-choice"), desktopkit.SettingsRowProps{Label: d.tr("界面语言"), Description: d.tr("立即生效，并保存到本机。")}, nil, func() {
		items := []desktopkit.SelectItem{
			{Value: "zh-CN", Label: "简体中文"}, {Value: "en", Label: "English"},
			{Value: "ja", Label: "日本語"}, {Value: "pt-BR", Label: "Português (Brasil)"}, {Value: "ru", Label: "Русский"},
		}
		if k.Select(c, desktopkit.SelectProps{Label: d.tr("界面语言"), Value: &value, Items: items, Disabled: d.live != nil && d.live.showBusy()}).Changed {
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

func (d *viewState) displayRelativeTime(value string) string {
	value = strings.TrimSpace(value)
	if value == "" {
		return ""
	}
	if value == "刚刚" {
		return d.tr("刚刚")
	}
	for _, prefix := range []string{"今天 ", "昨天 "} {
		if strings.HasPrefix(value, prefix) {
			return d.tr(strings.TrimSpace(prefix)) + " " + strings.TrimPrefix(value, prefix)
		}
	}
	if strings.HasSuffix(value, " 分钟前") {
		n := strings.TrimSuffix(value, " 分钟前")
		return strings.Replace(d.tr("%s 分钟前"), "%s", n, 1)
	}
	if strings.HasSuffix(value, " 小时前") {
		n := strings.TrimSuffix(value, " 小时前")
		return strings.Replace(d.tr("%s 小时前"), "%s", n, 1)
	}
	if strings.HasSuffix(value, " 天前") {
		n := strings.TrimSuffix(value, " 天前")
		return strings.Replace(d.tr("%s 天前"), "%s", n, 1)
	}
	return value
}

func (d *viewState) entryTitle(v entry) string {
	if v.generatedTitle {
		return d.tr(v.title)
	}
	return v.title
}
