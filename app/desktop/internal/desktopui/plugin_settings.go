package desktopui

import (
	"fmt"
	"maps"
	"slices"
	"strconv"
	"strings"

	"github.com/egoist/mygo/ui"

	"github.com/uniclipboard/unic/app/desktop/internal/desktopkit"
	api "github.com/uniclipboard/unic/internal/localapi"
	config "github.com/uniclipboard/unic/internal/localconfig"
)

// Built-in settings are host policy, not plugin-specific Schema fields.
// Keep draft text separate from the authoritative service snapshot.
type builtinPluginSettings struct {
	id, revision, threshold, thresholdError string
	instanceName                            string
	channelPair                             weTypePairing
	tab, query                              string
	view                                    string // "" (list) | "marketplace" | "advanced"
	marketQuery                             string
	marketCategory                          string
	marketSelected                          string
	marketTechOpen                          bool
	detailTechOpen                          bool
	installingID                            string
	installNotice                           string
	installError                            string
	manualOpen                              bool
	manualRepo, manualRef, manualPath       string
	manualBuild, manualEnable               bool
	uninstallOpen                           bool
	uninstallID, uninstallName              string
	rollbackOpen                            bool
	rollbackID, rollbackName, rollbackVer   string
	form                                    *pluginSettingsForm
	classification                          classificationSettings
}

type builtinPluginEntry struct {
	id, name, description string
	icon                  desktopkit.IconName
	tone                  string
	capabilities, events  []string
	actions               []string
}

var builtinPluginEntries = []builtinPluginEntry{
	{id: "builtin.tailcat", name: "Tailcat 同步", description: "在已配对设备间点对点同步剪贴板，支持文本、图片、文件与文件夹。", icon: desktopkit.IconRetry, tone: "teal", capabilities: []string{"同步后端"}},
	{id: "builtin.text", name: "文本工具", description: "提供转大写、转小写、去除首尾空白和纯文本四个常用动作。", icon: desktopkit.IconEdit, tone: "purple",
		capabilities: []string{"自动标注", "动作"}, events: []string{"clipboard.captured", "clipboard.received"}, actions: []string{"转大写", "转小写", "去除首尾空白", "纯文本"}},
	{id: "builtin.classify", name: "内容分类", description: "自动为文本、图片、文件、文件夹和链接添加基础类型标签。", icon: desktopkit.IconTag, tone: "accent",
		capabilities: []string{"自动标注"}, events: []string{"clipboard.captured", "clipboard.received"}},
}

func (d *viewState) pluginsToolbarActions(c *ui.Context, k desktopkit.Kit) {
	l := d.live
	p := &d.pluginSettings
	if k.ToolbarQuietButton(c.Key("tb-refresh-marketplace"), d.tr("检查更新"), desktopkit.ToolbarQuietButtonProps{
		Icon:     desktopkit.IconRetry,
		Busy:     l.marketplaceLoading,
		Disabled: l.marketplaceLoading,
	}).Clicked() {
		d.ensureMarketplaceLoaded(true)
	}
	_, cmd := k.ToolbarIconMenu(c.Key("tb-plugin-more"), desktopkit.ToolbarIconMenuProps{
		Label: d.tr("更多"),
		Icon:  desktopkit.IconMore,
		Items: []desktopkit.MenuEntry{
			{ID: "advanced", Label: d.tr("执行顺序与高级设置"), Icon: desktopkit.IconList},
			{ID: "manual", Label: d.tr("从链接手动安装"), Icon: desktopkit.IconLink},
		},
	})
	switch cmd {
	case "advanced":
		p.view = "advanced"
	case "manual":
		p.manualOpen = true
		p.installError = ""
	}
}

func pluginIconTile(c *ui.Context, k desktopkit.Kit, icon desktopkit.IconName, tone string, size float32) {
	radius := float32(10)
	iconSize := float32(18)
	if size >= 44 {
		radius = 12
		iconSize = 20
	}
	bg := deviceInsetBg(k, false)
	border := deviceCardBorder(k)
	fg := k.Color("text-primary")
	switch tone {
	case "accent":
		bg, border = deviceAccentSurface(k)
		fg = k.Color("accent-600")
		if k.Dark {
			fg = k.Color("accent-400")
		}
	case "purple":
		bg = k.Color("status-purple-background")
		fg = k.Color("status-purple-text")
		border = bg
	case "teal":
		bg = k.Color("status-cyan-background")
		fg = k.Color("status-cyan-text")
		border = bg
	}
	ui.Box(c).Size(size, size).Shrink(0).Radius(radius).Background(bg).Border(1, border).Center().Children(func() {
		k.Icon(c.Key("ico"), icon, iconSize).TextColor(fg)
	})
}

func pluginIconForCategory(category string, caps []string) (desktopkit.IconName, string) {
	if slices.Contains(caps, "sync_backend") || category == "sync" {
		return desktopkit.IconGlobe, "teal"
	}
	switch category {
	case "share":
		return desktopkit.IconSend, "purple"
	case "text":
		return desktopkit.IconEdit, "accent"
	case "organize":
		return desktopkit.IconTag, "accent"
	}
	if slices.Contains(caps, "action") {
		return desktopkit.IconSend, "purple"
	}
	return desktopkit.IconPackage, "accent"
}

func (d *viewState) builtinPluginsContent(c *ui.Context, k desktopkit.Kit) {
	l := d.live
	d.ensureMarketplaceLoaded(false)
	cfg := l.config.Config
	// Instances is a map in an otherwise value-copied Config. Never mutate the
	// service snapshot or the pointer fields while an asynchronous save is pending.
	cfg.Plugins.Instances = maps.Clone(cfg.Plugins.Instances)
	if cfg.Plugins.Instances == nil {
		cfg.Plugins.Instances = map[string]config.Instance{}
	}
	if l.config.ApplyError != "" {
		d.restartNotice(c.Key("apply-error-notice"), k, desktopkit.ToneError, d.tr("设置已保存，但未能生效。请重试或重新启动桌面端。"), true)
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
	p := &d.pluginSettings
	external, externalSelected := d.installedExternalPlugin(p.id)
	externalPending := externalSelected && (external.Restart || p.form != nil && p.form.state.Restart)
	if pending || externalPending {
		d.restartNotice(c.Key("plugin-restart-notice"), k, desktopkit.ToneWarning, d.tr("插件设置已保存。请退出并重新启动桌面端以生效。"), true)
	}
	if p.installError != "" {
		k.InlineNotice(c, desktopkit.NoticeProps{Tone: desktopkit.ToneError, Title: p.installError})
	} else if p.installNotice != "" {
		k.InlineNotice(c, desktopkit.NoticeProps{Tone: desktopkit.ToneInfo, Title: p.installNotice})
	}
	status := func(id string) (string, desktopkit.StatusTone) {
		switch {
		case pending:
			return d.tr("需要重启"), desktopkit.StatusWarning
		case cfg.PluginEnabled(id):
			return d.tr("已启用"), desktopkit.StatusPositive
		}
		return d.tr("已停用"), desktopkit.StatusNeutral
	}
	defer func() {
		d.pluginManualInstallDialog(c, k)
		d.pluginConfirmDialogs(c, k)
	}()

	if p.id == "" {
		switch p.view {
		case "advanced":
			d.pluginsAdvancedView(c, k, cfg)
			return
		default:
			d.builtinPluginList(c, k, &cfg)
			return
		}
	}
	var entry builtinPluginEntry
	for _, e := range builtinPluginEntries {
		if e.id == p.id {
			entry = e
		}
	}
	if entry.id == "" {
		if externalSelected {
			d.externalPluginDetail(c, k, external, cfg)
			return
		}
		p.id = ""
		p.form = nil
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
		d.syncPluginPolicy(c, k, p.id, cfg)
		return
	}
	k.SettingsGroup(c.Key("plugin-detail-"+p.id), desktopkit.SettingsGroupProps{Title: name, Footer: d.tr("以下策略保存后立即生效，仍受全局同步设置限制。")}, func() {
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
	if changed {
		cfg.Plugins.Instances[p.id] = instance
		d.saveLiveConfig(cfg)
	}
	if p.id == "builtin.classify" {
		d.classificationRules(c, k, cfg, instance)
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

	// Obsidian-style Marketplace Hero Card (single focused primary CTA; secondary actions live in toolbar).
	ui.Row(c.Key("plugin-marketplace-hero")).Padding(16, 18).Radius(14).Background(deviceCardBg(k)).Border(1, deviceCardBorder(k)).Gap(14).AlignItems(ui.Center).Children(func() {
		pluginIconTile(c.Key("hero-ico"), k, desktopkit.IconPackage, "accent", 44)
		ui.Column(c.Key("hero-body")).Grow(1).MinWidth(0).Gap(3).Children(func() {
			ui.Row(c.Key("hero-title")).Gap(8).AlignItems(ui.Center).Wrap().Children(func() {
				k.Text(c.Key("t"), d.tr("插件市场"), desktopkit.TextProps{Role: desktopkit.RoleTitle, Emphasized: true})
				if len(l.marketplace.Plugins) > 0 {
					k.Tag(c.Key("cnt"), fmt.Sprintf(d.tr("%d 款扩展"), len(l.marketplace.Plugins)), desktopkit.TagProps{Color: "blue"})
				}
			})
			k.Text(c.Key("s"), d.tr("浏览官方与社区扩展，一键在线安装剪贴板动作、整理工具与同步方式。"), secondary)
		})
		if k.Button(c.Key("open-marketplace"), d.tr("浏览插件市场"), desktopkit.ButtonProps{Variant: desktopkit.Primary, Icon: desktopkit.IconPackage, Disabled: l.busy}).Clicked() {
			p.view = "marketplace"
			p.installError = ""
			p.installNotice = ""
			p.marketTechOpen = false
			if p.marketCategory == "" {
				p.marketCategory = "all"
			}
			if p.marketSelected == "" && len(l.marketplace.Plugins) > 0 {
				p.marketSelected = l.marketplace.Plugins[0].ID
			}
		}
	})

	// Installed third-party plugins (or highlighted official recommendation card when none installed).
	ui.Column(c.Key("installed-section")).Gap(8).MinWidth(0).Children(func() {
		ui.Row(c.Key("installed-head")).Justify(ui.SpaceBetween).AlignItems(ui.Center).Padding(0, 2).Children(func() {
			title := fmt.Sprintf(d.tr("已安装插件（%d）"), len(l.externalPlugins))
			if len(l.externalPlugins) == 0 {
				title = d.tr("官方推荐扩展")
			}
			k.Text(c.Key("ht"), title, desktopkit.TextProps{Role: desktopkit.RoleCaption, Tone: desktopkit.ToneSecondary, Emphasized: true})
			if len(l.externalPlugins) >= 2 {
				ui.Box(c.Key("mini-search")).Width(220).Children(func() {
					search := k.SearchField(c.Key("plugin-search"), desktopkit.SearchFieldProps{Label: d.tr("搜索插件"), Placeholder: d.tr("搜索插件名称或 ID"), Value: &p.query})
					if search.Escaped {
						p.query = ""
					}
				})
			} else {
				p.query = ""
			}
		})

		query := strings.ToLower(strings.TrimSpace(p.query))
		if len(l.externalPlugins) == 0 {
			if len(l.marketplace.Plugins) > 0 {
				pick := l.marketplace.Plugins[0]
				accentBg, accentEdge := deviceAccentSurface(k)
				icon, tone := pluginIconForCategory(pick.Category, pick.Capabilities)
				ui.Column(c.Key("pick-card-"+pick.ID)).Padding(14, 16).Radius(13).Background(accentBg).Border(1, accentEdge).Gap(10).Children(func() {
					ui.Row(c.Key("top")).Gap(12).AlignItems(ui.Center).Children(func() {
						pluginIconTile(c.Key("ico"), k, icon, tone, 36)
						ui.Column(c.Key("meta")).Grow(1).MinWidth(0).Gap(2).Children(func() {
							ui.Row(c.Key("name")).Gap(6).AlignItems(ui.Center).Wrap().Children(func() {
								k.Text(c.Key("n"), pick.Name, desktopkit.TextProps{Emphasized: true})
								if pick.Official {
									k.Tag(c.Key("off"), d.tr("官方"), desktopkit.TagProps{Color: "blue"})
								}
								if pick.ZeroConfig {
									k.Tag(c.Key("zero"), d.tr("免配置"), desktopkit.TagProps{Color: "gray"})
								}
							})
							k.Text(c.Key("auth"), pick.Author+" · v"+pick.Version, footnote)
						})
					})
					k.Text(c.Key("desc"), pick.Summary, secondary)
					ui.Box(c.Key("div")).Height(1).FillWidth().Background(deviceCardBorder(k))
					ui.Row(c.Key("foot")).Justify(ui.SpaceBetween).AlignItems(ui.Center).Children(func() {
						k.Text(c.Key("cat"), d.categoryLabel(pick.Category), footnote)
						ui.Row(c.Key("btns")).Gap(8).AlignItems(ui.Center).Children(func() {
							if k.Button(c.Key("pick-view-"+pick.ID), d.tr("查看详情"), desktopkit.ButtonProps{Variant: desktopkit.Subtle}).Clicked() {
								p.view = "marketplace"
								p.marketSelected = pick.ID
								p.marketTechOpen = false
							}
							installing := p.installingID == pick.ID
							btnLabel := d.tr("立即安装")
							if installing {
								btnLabel = d.tr("正在安装…")
							}
							if k.Button(c.Key("quick-install-"+pick.ID), btnLabel, desktopkit.ButtonProps{Variant: desktopkit.Primary, Icon: desktopkit.IconDownload, Busy: installing, Disabled: l.busy || p.installingID != ""}).Clicked() {
								d.installMarketplacePlugin(api.PluginInstallRequest{
									Repository: pick.Repository,
									Ref:        pick.Ref,
									Path:       pick.Path,
									Build:      pick.BuildFallback,
								}, pick.Name, pick.ID)
							}
						})
					})
				})
			} else {
				k.SettingsGroup(c.Key("external-empty-group"), desktopkit.SettingsGroupProps{}, func() {
					k.SettingsRow(c.Key("external-empty"), desktopkit.SettingsRowProps{
						Label:       d.tr("尚未安装第三方插件"),
						Description: d.tr("打开插件市场即可一键在线安装扩展。"),
					}, nil, nil)
				})
			}
		} else {
			k.SettingsGroup(c.Key("external-plugins"), desktopkit.SettingsGroupProps{}, func() {
				shown := 0
				for _, item := range l.externalPlugins {
					cat, hasCat := d.marketplaceEntry(item.ID)
					displayName, catSummary := d.pluginDisplay(item)
					summary := pluginSubtitle(item, catSummary)
					meta := "v" + item.Version
					icon, tone := pluginIconForCategory("", item.Capabilities)
					if hasCat {
						meta = "v" + item.Version + " · " + cat.Author
						if cat.ZeroConfig {
							meta += " · " + d.tr("免配置")
						}
						icon, tone = pluginIconForCategory(cat.Category, item.Capabilities)
					}
					if query != "" && !strings.Contains(strings.ToLower(displayName+" "+item.ID+" "+summary), query) {
						continue
					}
					shown++
					canUpdate := hasCat && cat.Version != "" && cat.Version != item.Version && item.Source != "local"
					isConfigurable := item.HasSettings || slices.Contains(item.Capabilities, "sync_backend")
					ui.Row(c.Key(item.ID)).Padding(12, 14).Gap(12).AlignItems(ui.Center).Children(func() {
						pluginIconTile(c.Key("ico"), k, icon, tone, 36)
						ui.Column(c.Key("meta")).Grow(1).MinWidth(0).Gap(2).Children(func() {
							ui.Row(c.Key("top")).Gap(6).AlignItems(ui.Center).Wrap().Children(func() {
								k.Text(c.Key("n"), displayName, desktopkit.TextProps{Emphasized: true})
								if hasCat && cat.Official {
									k.Tag(c.Key("off"), d.tr("官方"), desktopkit.TagProps{Color: "blue"})
								}
								if canUpdate {
									k.Tag(c.Key("upd"), d.tr("可更新"), desktopkit.TagProps{Color: "green"})
								}
							})
							k.Text(c.Key("s"), summary, secondary)
							k.Text(c.Key("m"), meta, footnote)
						})
						ui.Row(c.Key("ctrl")).Gap(6).AlignItems(ui.Center).Shrink(0).Children(func() {
							if canUpdate {
								updating := p.installingID == item.ID
								if k.Button(c.Key("update-"+item.ID), d.tr("更新"), desktopkit.ButtonProps{Variant: desktopkit.Primary, Busy: updating, Disabled: l.busy || p.installingID != ""}).Clicked() {
									d.installMarketplacePlugin(api.PluginInstallRequest{
										Repository: cat.Repository,
										Ref:        cat.Ref,
										Path:       cat.Path,
										Build:      cat.BuildFallback,
										Replace:    true,
									}, displayName, item.ID)
								}
							}
							detailIcon := desktopkit.IconChevronRight
							if isConfigurable {
								detailIcon = desktopkit.IconSettings
							}
							if k.Button(c.Key("open-"+item.ID), "", desktopkit.ButtonProps{Variant: desktopkit.Subtle, Icon: detailIcon, Label: item.ID, Tooltip: displayName, Disabled: l.busy}).Clicked() {
								p.id = item.ID
								p.view = ""
								p.detailTechOpen = false
								p.form = nil
								p.channelPair = weTypePairing{}
							}
							if item.Source != "local" {
								if k.Button(c.Key("uninstall-"+item.ID), "", desktopkit.ButtonProps{Variant: desktopkit.Subtle, Icon: desktopkit.IconDelete, Label: d.tr("卸载") + displayName, Tooltip: d.tr("卸载"), Disabled: l.busy}).Clicked() {
									p.uninstallOpen = true
									p.uninstallID = item.ID
									p.uninstallName = displayName
								}
							}
							on := item.Configured
							if k.Switch(c.Key("enable-"+item.ID), &on, desktopkit.SwitchProps{Label: d.tr("启用") + displayName, Disabled: l.busy || l.loading}).Changed() {
								d.setExternalPluginEnabled(*cfg, item.ID, on)
							}
						})
					})
				}
				if shown == 0 {
					ui.Box(c.Key("empty")).Padding(16).Children(func() {
						k.Text(c, d.tr("无匹配"), footnote)
					})
				}
			})
		}
	})

	// Built-in features (with colored icon tiles matching prototype/plugins.html).
	k.SettingsGroup(c.Key("builtin-plugin-list"), desktopkit.SettingsGroupProps{Title: d.tr("内置功能")}, func() {
		for _, entry := range builtinPluginEntries {
			ui.Row(c.Key(entry.id)).Padding(12, 14).Gap(12).AlignItems(ui.Center).Children(func() {
				pluginIconTile(c.Key("ico"), k, entry.icon, entry.tone, 36)
				ui.Column(c.Key("meta")).Grow(1).MinWidth(0).Gap(2).Children(func() {
					ui.Row(c.Key("top")).Gap(6).AlignItems(ui.Center).Wrap().Children(func() {
						k.Text(c.Key("n"), d.tr(entry.name), desktopkit.TextProps{Emphasized: true})
						k.Tag(c.Key("b"), d.tr("内置"), desktopkit.TagProps{Color: "gray"})
					})
					k.Text(c.Key("s"), d.tr(entry.description), secondary)
				})
				ui.Row(c.Key("ctrl")).Gap(6).AlignItems(ui.Center).Shrink(0).Children(func() {
					icon := desktopkit.IconChevronRight
					if entry.id == "builtin.tailcat" {
						icon = desktopkit.IconSettings
					}
					if k.Button(c.Key("open-"+entry.id), "", desktopkit.ButtonProps{Variant: desktopkit.Subtle, Icon: icon, Label: d.tr(entry.name), Tooltip: d.tr(entry.name), Disabled: l.busy || l.loading}).Clicked() {
						p.id = entry.id
						p.view = ""
						p.form = nil
					}
					on := cfg.PluginEnabled(entry.id)
					if k.Switch(c.Key("enable-"+entry.id), &on, desktopkit.SwitchProps{Label: d.tr("启用") + d.tr(entry.name), Disabled: l.busy || l.loading}).Changed() {
						instance := cfg.Plugins.Instances[entry.id]
						instance.Enabled = &on
						cfg.Plugins.Instances[entry.id] = instance
						d.saveLiveConfig(*cfg)
					}
				})
			})
		}
	})
}

func (d *viewState) syncPluginPolicy(c *ui.Context, k desktopkit.Kit, id string, cfg config.Config) {
	l := d.live
	p := &d.pluginSettings
	instance := cfg.Plugins.Instances[id]
	changed := false
	toggle := func(key, label, desc string, on *bool) {
		k.SettingsRow(c.Key(key), desktopkit.SettingsRowProps{Label: d.tr(label), Description: d.tr(desc)}, nil, func() {
			if k.Switch(c, on, desktopkit.SwitchProps{Label: d.tr(label), Disabled: l.busy || l.loading}).Changed() {
				changed = true
			}
		})
	}
	name := d.tr("同步策略")

	if p.revision != l.config.Revision {
		p.revision = l.config.Revision
		p.threshold = strconv.FormatFloat(float64(instance.Sync.AutoFileMaxBytes)/(1<<20), 'f', -1, 64)
		p.thresholdError = ""
	}
	k.SettingsGroup(c.Key("plugin-detail-"+p.id), desktopkit.SettingsGroupProps{Title: name}, func() {
		if id == "builtin.tailcat" {
			enabled := cfg.PluginEnabled(id)
			toggle("enabled", "启用", "退出并重新启动桌面端后生效。", &enabled)
			if changed {
				instance.Enabled = &enabled
			}
		}
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
		k.SettingsRow(c.Key("threshold"), desktopkit.SettingsRowProps{Label: d.tr("自动文件传输阈值（MB）"), Description: d.tr("0 表示不增加限制。逐个文件检查。任一文件超限时跳过整次自动发送。主动发送不受此阈值限制。") + " " + d.tr("输入后按回车保存。")}, nil, func() {
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
	if changed {
		cfg.Plugins.Instances[id] = instance
		d.saveLiveConfig(cfg)
	}
}
