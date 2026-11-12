package desktopdemo

import (
	_ "embed"
	"encoding/json"
	"strings"

	"github.com/egoist/mygo/ui"
	"github.com/uniclipboard/unic/app/desktop/internal/uikit"
)

// Chinese source strings are catalogue IDs. User content never passes through
// this lookup. The empty language preserves the existing Chinese default.
//
//go:embed locales.json
var catalogue []byte

var translations = func() map[string]map[string]string {
	var values map[string]map[string]string
	if err := json.Unmarshal(catalogue, &values); err != nil {
		panic(err)
	}
	return values
}()

// Translate resolves application-owned UI text for the desktop host.
func Translate(language, source string) string {
	if text := translations[source][language]; text != "" {
		return text
	}
	return source
}
func (d *demo) currentLanguage() string {
	language := d.language
	if d.live != nil && d.live.loaded {
		language = d.live.config.Config.Desktop.Language
	}
	if language == "" {
		return "zh-CN"
	}
	return language
}
func (d *demo) tr(source string) string { return Translate(d.currentLanguage(), source) }
func (p *quickDemo) tr(source string) string {
	if p.live != nil {
		return p.live.d.tr(source)
	}
	return Translate(p.language, source)
}

func (d *demo) languageRow(c *ui.Context, k uikit.Kit) {
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

func (d *demo) searchCandidateLabel(c searchCandidate) string {
	if c.dimension == "type" || c.dimension == "time" {
		return d.tr(c.label)
	}
	return c.label
}
func (p *quickDemo) filterLabel(f qpFilter) string {
	if f.dimension == "type" {
		return "/" + p.tr(f.value)
	}
	if f.dimension == "time" {
		return p.tr(f.value)
	}
	return f.label
}
func (p *quickDemo) actionLabel(a qpAction) string {
	if a.id == "send:windows" || a.id == "send:linux" || a.id == "send:phone" {
		return a.label
	}
	return p.tr(a.label)
}

func (d *demo) displayDate(value string) string {
	for _, prefix := range []string{"今天", "昨天"} {
		if strings.HasPrefix(value, prefix) {
			return d.tr(prefix) + strings.TrimPrefix(value, prefix)
		}
	}
	return value
}
func (d *demo) policyText(v device) string {
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

func (d *demo) entryTitle(v entry) string {
	if v.generatedTitle {
		return d.tr(v.title)
	}
	return v.title
}
