package desktopui

import (
	"strings"

	"github.com/egoist/mygo/ui"

	"github.com/uniclipboard/unic/app/desktop/internal/desktopkit"
	config "github.com/uniclipboard/unic/internal/localconfig"
	"github.com/uniclipboard/unic/internal/plugins/builtinpreview"
)

type previewSettingsState struct {
	revision      string
	excludedDraft string
	excludedError string
}

func (d *viewState) smartPreviewSettings(c *ui.Context, k desktopkit.Kit, cfg config.Config, instance config.Instance) {
	l := d.live
	ps := &d.pluginSettings.preview
	var s builtinpreview.Settings
	if instance.Preview != nil {
		s = *instance.Preview
	}
	if ps.revision != l.config.Revision {
		ps.revision = l.config.Revision
		ps.excludedDraft = strings.Join(s.ExcludedDomains, ", ")
		ps.excludedError = ""
	}
	busy := l.busy || l.loading
	enabled := cfg.PluginEnabled(builtinpreview.ID)
	linkOn := s.LinkPreviewEnabled()
	favOn := s.FetchFaviconEnabled()
	thumbOn := s.FetchThumbnailEnabled()

	save := func(nextEnabled bool, next builtinpreview.Settings) {
		en := nextEnabled
		instance.Enabled = &en
		copySettings := next
		instance.Preview = &copySettings
		cfg.Plugins.Instances[builtinpreview.ID] = instance
		d.saveLiveConfig(cfg)
	}

	k.SettingsGroup(c.Key("plugin-detail-builtin.preview"), desktopkit.SettingsGroupProps{
		Title: d.tr("智能预览"),
	}, func() {
		k.SettingsRow(c.Key("enabled"), desktopkit.SettingsRowProps{
			Label:       d.tr("启用智能预览"),
			Description: d.tr("退出并重新启动桌面端后生效。"),
		}, nil, func() {
			if k.Switch(c, &enabled, desktopkit.SwitchProps{
				Label:    d.tr("启用智能预览"),
				Disabled: busy,
			}).Changed() {
				save(enabled, s)
			}
		})
	})

	k.SettingsGroup(c.Key("smart-preview-link"), desktopkit.SettingsGroupProps{
		Title:  d.tr("网页链接预览"),
		Footer: d.tr("仅处理整段为单个 HTTP/HTTPS 链接的条目，自动拦截本机与内网地址。重启桌面端后生效。"),
	}, func() {
		k.SettingsRow(c.Key("link-preview"), desktopkit.SettingsRowProps{
			Label:       d.tr("网页链接预览"),
			Description: d.tr("自动抓取网页标题与摘要，并支持按标题和摘要检索。"),
		}, nil, func() {
			if k.Switch(c, &linkOn, desktopkit.SwitchProps{
				Label:    d.tr("网页链接预览"),
				Disabled: busy || !enabled,
			}).Changed() {
				v := linkOn
				s.LinkPreview = &v
				save(enabled, s)
			}
		})

		k.SettingsRow(c.Key("fetch-favicon"), desktopkit.SettingsRowProps{
			Label:       d.tr("抓取网站图标"),
			Description: d.tr("抓取并缓存站点图标用于列表和详情页展示。"),
		}, nil, func() {
			if k.Switch(c, &favOn, desktopkit.SwitchProps{
				Label:    d.tr("抓取网站图标"),
				Disabled: busy || !enabled || !linkOn,
			}).Changed() {
				v := favOn
				s.FetchFavicon = &v
				save(enabled, s)
			}
		})

		k.SettingsRow(c.Key("fetch-thumbnail"), desktopkit.SettingsRowProps{
			Label:       d.tr("抓取网页封面图"),
			Description: d.tr("默认抓取网页 og:image 封面图用于链接预览卡片。"),
		}, nil, func() {
			if k.Switch(c, &thumbOn, desktopkit.SwitchProps{
				Label:    d.tr("抓取网页封面图"),
				Disabled: busy || !enabled || !linkOn,
			}).Changed() {
				v := thumbOn
				s.FetchThumbnail = &v
				save(enabled, s)
			}
		})

		k.SettingsRow(c.Key("excluded-domains"), desktopkit.SettingsRowProps{
			Label:       d.tr("排除域名"),
			Description: d.tr("不抓取匹配的域名（支持子域名，多个域名用逗号分隔，回车保存）。"),
		}, nil, func() {
			field := k.TextField(c, desktopkit.TextFieldProps{
				Label:       d.tr("排除域名"),
				Placeholder: "intranet.example, corp.local",
				Value:       &ps.excludedDraft,
				Disabled:    busy || !enabled || !linkOn,
				Error:       d.tr(ps.excludedError),
			})
			if field.Submitted && !busy {
				domains := parseExcludedDomainsDraft(ps.excludedDraft)
				candidate := s
				candidate.ExcludedDomains = domains
				if err := candidate.Validate(); err != nil {
					ps.excludedError = "域名格式无效，请以逗号分隔合法主机名（最多 64 个）。"
				} else {
					ps.excludedError = ""
					s.ExcludedDomains = domains
					save(enabled, s)
				}
			}
		})
	})

	k.Text(c.Key("preview-ext-note"), d.tr("智能预览的异步富化与预览注解能力同时开放给第三方插件；你也可以安装外部预览插件并在高级设置中调整优先级。"), footnote)
}

func parseExcludedDomainsDraft(raw string) []string {
	normalized := strings.ReplaceAll(raw, "，", ",")
	parts := strings.Split(normalized, ",")
	var out []string
	for _, p := range parts {
		d := strings.TrimSpace(p)
		if d != "" {
			out = append(out, d)
		}
	}
	return out
}
