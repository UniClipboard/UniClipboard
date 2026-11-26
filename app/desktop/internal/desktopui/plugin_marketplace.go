package desktopui

import (
	"context"
	"fmt"
	"maps"
	"slices"
	"strconv"
	"strings"
	"time"

	"github.com/egoist/mygo/ui"

	"github.com/uniclipboard/unic/app/desktop/internal/desktopkit"
	api "github.com/uniclipboard/unic/internal/localapi"
	config "github.com/uniclipboard/unic/internal/localconfig"
)

const (
	pluginInstallTimeout      = 3 * time.Minute
	pluginConfigToastDuration = 8 * time.Second
)

type pluginInstallTask struct {
	trackID     string
	displayName string
	replace     bool
	cancel      context.CancelFunc
	cancelling  bool
	cancelled   bool
}

func pluginConfigurable(info api.PluginInfo) bool {
	return info.HasSettings || info.NeedsSettings || slices.Contains(info.Capabilities, "sync_backend")
}

func manualInstallDisplayName(repo string) string {
	trimmed := strings.TrimSuffix(strings.TrimSpace(repo), "/")
	trimmed = strings.TrimSuffix(trimmed, ".git")
	if idx := strings.LastIndex(trimmed, "/"); idx >= 0 && idx+1 < len(trimmed) {
		return trimmed[idx+1:]
	}
	return trimmed
}

func (d *viewState) pluginInstallTask(trackID string) *pluginInstallTask {
	if d.live == nil || trackID == "" || d.live.pluginInstalls == nil {
		return nil
	}
	return d.live.pluginInstalls[trackID]
}

func (l *liveState) finishPluginInstall(trackID string) *pluginInstallTask {
	if l == nil || trackID == "" {
		return nil
	}
	task := l.pluginInstalls[trackID]
	delete(l.pluginInstalls, trackID)
	l.pluginInstallOrder = slices.DeleteFunc(l.pluginInstallOrder, func(id string) bool { return id == trackID })
	return task
}

func (d *viewState) cancelPluginInstall(trackID string) {
	task := d.pluginInstallTask(trackID)
	if task == nil || task.cancelling {
		return
	}
	task.cancelling = true
	task.cancelled = true
	if task.cancel != nil {
		task.cancel()
	}
}

func (d *viewState) syncPluginInstallToasts() {
	l := d.live
	if l == nil || len(l.pluginInstallOrder) == 0 {
		return
	}
	for _, trackID := range l.pluginInstallOrder {
		task := l.pluginInstalls[trackID]
		if task == nil {
			continue
		}
		toastID := toastIDPluginInstallPrefix + trackID
		title := fmt.Sprintf(d.tr("正在安装「%s」…"), task.displayName)
		cancelLabel := d.tr("取消安装")
		if task.replace {
			title = fmt.Sprintf(d.tr("正在更新「%s」…"), task.displayName)
			cancelLabel = d.tr("取消更新")
		}
		if task.cancelling {
			cancelLabel = d.tr("正在取消…")
		}
		id := trackID
		d.toasts.SyncPersistent(toastID, title, desktopkit.ToastOptions{
			Status:         desktopkit.ToastInformation,
			Loading:        true,
			NonDismissible: true,
			Actions: []desktopkit.ToastAction{{
				Label:    cancelLabel,
				Variant:  "secondary",
				Busy:     task.cancelling,
				Disabled: task.cancelling,
				KeepOpen: true,
				OnClick:  func() { d.cancelPluginInstall(id) },
			}},
		})
	}
}

func (d *viewState) ensureMarketplaceLoaded(refresh bool) {
	l := d.live
	if l == nil || l.marketplaceLoading || (l.marketplaceLoaded && !refresh) {
		return
	}
	l.marketplaceLoading = true
	l.marketplaceError = ""
	client := l.client
	go func() {
		ctx, cancel := context.WithTimeout(l.ctx, 10*time.Second)
		defer cancel()
		cat, err := client.PluginMarketplace(ctx, refresh)
		apply := func() {
			l.marketplaceLoading = false
			if err != nil {
				l.marketplaceError = "未能加载插件市场目录。"
				d.toastWarning(d.tr(l.marketplaceError), desktopkit.ToastOptions{ID: "marketplace-load"})
				return
			}
			l.marketplaceLoaded = true
			l.marketplace = cat
			if cat.Warning != "" {
				d.toastWarning(cat.Warning, desktopkit.ToastOptions{ID: "marketplace-warning"})
			}
			if d.pluginSettings.marketSelected == "" && len(cat.Plugins) > 0 {
				d.pluginSettings.marketSelected = cat.Plugins[0].ID
			}
		}
		select {
		case <-l.ctx.Done():
		case l.results <- apply:
			l.wake()
		}
	}()
}

func (d *viewState) categoryLabel(cat string) string {
	switch cat {
	case "official":
		return d.tr("官方推荐")
	case "share":
		return d.tr("分享与导出")
	case "text":
		return d.tr("文本效率")
	case "organize":
		return d.tr("智能整理")
	case "sync":
		return d.tr("同步扩展")
	default:
		return d.tr("全部")
	}
}

func (d *viewState) marketplaceEntry(id string) (api.MarketplaceEntry, bool) {
	if d.live == nil {
		return api.MarketplaceEntry{}, false
	}
	for _, entry := range d.live.marketplace.Plugins {
		if entry.ID == id {
			return entry, true
		}
	}
	return api.MarketplaceEntry{}, false
}

func (d *viewState) installedExternalPlugin(id string) (api.PluginInfo, bool) {
	if d.live == nil {
		return api.PluginInfo{}, false
	}
	for _, item := range d.live.externalPlugins {
		if item.ID == id {
			return item, true
		}
	}
	return api.PluginInfo{}, false
}

func (d *viewState) setExternalPluginEnabled(cfg config.Config, id string, on bool) {
	if d.live.busy {
		return
	}
	if item, ok := d.installedExternalPlugin(id); on && ok && item.LiveStart && item.RunningVersion == "" {
		d.startExternalPlugin(id)
		return
	}
	cfg.Plugins.Lua = slices.DeleteFunc(slices.Clone(cfg.Plugins.Lua), func(x string) bool { return x == id })
	cfg.Plugins.Order = slices.DeleteFunc(slices.Clone(cfg.Plugins.Order), func(x string) bool { return x == id })
	if cfg.Plugins.Instances != nil {
		cfg.Plugins.Instances = maps.Clone(cfg.Plugins.Instances)
		if in, known := cfg.Plugins.Instances[id]; known {
			in.Enabled = nil
			if !on {
				in.Enabled = &on
			}
			cfg.Plugins.Instances[id] = in
		}
	}
	if on {
		cfg.Plugins.Order = append(cfg.Plugins.Order, id)
	}
	if idx := slices.IndexFunc(d.live.externalPlugins, func(item api.PluginInfo) bool { return item.ID == id }); idx >= 0 {
		d.live.externalPlugins[idx].Configured = on
	}
	d.saveLiveConfig(cfg)
}

func (d *viewState) startExternalPlugin(id string) {
	l := d.live
	p := &d.pluginSettings
	if l.busy {
		return
	}
	p.installError = ""
	p.installNotice = ""
	l.startBusy("")
	if !l.enqueueWithin(func(ctx context.Context) func() {
		info, err := l.client.StartPlugin(ctx, id)
		return func() {
			l.stopBusy()
			l.nextRefresh = time.Time{}
			if err != nil {
				p.installError = d.tr("启动失败") + " · " + liveErrorMessage(err)
				d.toastError(p.installError, desktopkit.ToastOptions{ID: toastIDPluginFeedback})
				return
			}
			if i := slices.IndexFunc(l.externalPlugins, func(item api.PluginInfo) bool { return item.ID == id }); i >= 0 {
				l.externalPlugins[i] = info
			}
			if info.NeedsSettings {
				p.id, p.view, p.form = id, "", nil
			}
		}
	}, 15*time.Second) {
		l.stopBusy()
	}
}

func (d *viewState) installMarketplacePlugin(req api.PluginInstallRequest, displayName, trackID string) {
	l := d.live
	p := &d.pluginSettings
	if l == nil {
		return
	}
	if trackID == "" {
		trackID = req.Repository
	}
	if d.pluginInstallTask(trackID) != nil {
		return
	}
	labelName := displayName
	if labelName == "" {
		labelName = manualInstallDisplayName(req.Repository)
	}
	if labelName == "" {
		labelName = trackID
	}
	if l.pluginInstalls == nil {
		l.pluginInstalls = map[string]*pluginInstallTask{}
	}
	installCtx, installCancel := context.WithTimeout(l.ctx, pluginInstallTimeout)
	task := &pluginInstallTask{
		trackID:     trackID,
		displayName: labelName,
		replace:     req.Replace,
		cancel:      installCancel,
	}
	l.pluginInstalls[trackID] = task
	l.pluginInstallOrder = append(l.pluginInstallOrder, trackID)
	p.installError = ""
	p.installNotice = ""
	p.manualOpen = false

	client := l.client
	go func() {
		defer installCancel()
		info, err := client.InstallPlugin(installCtx, req)
		var plugins []api.PluginInfo
		var saved config.State
		if err == nil {
			refreshCtx, refreshCancel := context.WithTimeout(l.ctx, 5*time.Second)
			plugins, _ = client.Plugins(refreshCtx)
			saved, _ = client.Config(refreshCtx)
			refreshCancel()
		}
		apply := func() {
			finished := l.finishPluginInstall(trackID)
			cancelledByUser := finished != nil && finished.cancelled && l.ctx.Err() == nil
			toastID := toastIDPluginInstallPrefix + trackID
			if err != nil {
				if cancelledByUser {
					msg := fmt.Sprintf(d.tr("已取消安装「%s」"), labelName)
					if req.Replace {
						msg = fmt.Sprintf(d.tr("已取消更新「%s」"), labelName)
					}
					d.toasts.Show(msg, desktopkit.ToastOptions{
						ID:       toastID,
						Status:   desktopkit.ToastWarning,
						Duration: 3 * time.Second,
					})
					return
				}
				p.installError = d.tr("安装失败：") + err.Error()
				titlePrefix := d.tr("安装失败：")
				if req.Replace {
					titlePrefix = d.tr("更新失败：")
				}
				d.toasts.Show(titlePrefix+labelName, desktopkit.ToastOptions{
					ID:          toastID,
					Status:      desktopkit.ToastError,
					Description: err.Error(),
					Duration:    6 * time.Second,
				})
				return
			}
			if plugins != nil {
				l.externalPlugins = plugins
			} else if info.ID != "" {
				if idx := slices.IndexFunc(l.externalPlugins, func(item api.PluginInfo) bool { return item.ID == info.ID }); idx >= 0 {
					l.externalPlugins[idx] = info
				} else {
					l.externalPlugins = append(l.externalPlugins, info)
				}
			}
			if saved.Revision != "" {
				l.config = saved
			}
			l.nextRefresh = time.Time{}

			if strings.TrimSpace(p.manualRepo) == req.Repository {
				p.manualRepo, p.manualRef, p.manualPath = "", "", ""
			}

			name := displayName
			if name == "" {
				name, _ = d.pluginDisplay(info)
			}
			if name == "" {
				name = labelName
			}

			status := desktopkit.ToastSuccess
			var notice string
			switch {
			case info.NeedsSettings:
				status = desktopkit.ToastInformation
				notice = name + " · " + d.tr("需要配置")
			case req.Replace && info.Restart:
				notice = fmt.Sprintf(d.tr("已更新「%s」，请重启桌面端生效。"), name)
			case req.Replace:
				notice = fmt.Sprintf(d.tr("已更新「%s」。"), name)
			case info.LiveStart && !req.Enable:
				notice = fmt.Sprintf(d.tr("已安装「%s」。"), name)
			case req.Enable:
				notice = fmt.Sprintf(d.tr("已安装并启用「%s」，请重启桌面端生效。"), name)
			default:
				notice = fmt.Sprintf(d.tr("已安装「%s」，点击「启用」后重启生效。"), name)
			}
			p.installNotice = notice
			opts := desktopkit.ToastOptions{
				ID:     toastID,
				Status: status,
			}
			if pluginConfigurable(info) && info.ID != "" {
				pluginID := info.ID
				opts.Duration = pluginConfigToastDuration
				opts.Actions = []desktopkit.ToastAction{{
					Label:   d.tr("前往配置"),
					Variant: "primary",
					OnClick: func() {
						d.openPluginSettings(pluginID)
					},
				}}
			}
			d.toasts.Show(notice, opts)
		}
		select {
		case <-l.ctx.Done():
		case l.results <- apply:
			l.wake()
		}
	}()
}

func (d *viewState) uninstallExternalPlugin(id, displayName string) {
	l := d.live
	p := &d.pluginSettings
	if l == nil || l.busy {
		return
	}
	p.installError = ""
	p.installNotice = ""
	l.startBusy("")
	client := l.client
	if !l.enqueue(func(ctx context.Context) func() {
		err := client.UninstallPlugin(ctx, id)
		var plugins []api.PluginInfo
		var saved config.State
		if err == nil {
			plugins, _ = client.Plugins(ctx)
			saved, _ = client.Config(ctx)
		}
		return func() {
			l.stopBusy()
			p.uninstallOpen = false
			if err != nil {
				p.installError = d.tr("卸载失败：") + err.Error()
				d.toastError(p.installError, desktopkit.ToastOptions{ID: toastIDPluginFeedback})
				return
			}
			if plugins != nil {
				l.externalPlugins = plugins
			}
			if saved.Revision != "" {
				l.config = saved
			}
			if p.id == id {
				p.id = ""
				p.form = nil
			}
			p.installNotice = fmt.Sprintf(d.tr("已标记卸载「%s」，重启后清理程序文件。"), displayName)
			d.toastInfo(p.installNotice, desktopkit.ToastOptions{ID: toastIDPluginFeedback})
		}
	}) {
		l.stopBusy()
	}
}

func (d *viewState) rollbackExternalPlugin(id, displayName string) {
	l := d.live
	p := &d.pluginSettings
	if l == nil || l.busy {
		return
	}
	p.installError = ""
	p.installNotice = ""
	l.startBusy("")
	client := l.client
	if !l.enqueue(func(ctx context.Context) func() {
		_, err := client.RollbackPlugin(ctx, id)
		var plugins []api.PluginInfo
		if err == nil {
			plugins, _ = client.Plugins(ctx)
		}
		return func() {
			l.stopBusy()
			p.rollbackOpen = false
			if err != nil {
				p.installError = d.tr("回滚失败：") + err.Error()
				d.toastError(p.installError, desktopkit.ToastOptions{ID: toastIDPluginFeedback})
				return
			}
			if plugins != nil {
				l.externalPlugins = plugins
			}
			p.installNotice = fmt.Sprintf(d.tr("已回滚「%s」，请重启桌面端生效。"), displayName)
			d.toastSuccess(p.installNotice, desktopkit.ToastOptions{ID: toastIDPluginFeedback})
		}
	}) {
		l.stopBusy()
	}
}

func pluginHowToCard(c *ui.Context, k desktopkit.Kit, steps []string) {
	accentBg, accentEdge := deviceAccentSurface(k)
	accentFg := k.Color("accent-600")
	if k.Dark {
		accentFg = k.Color("accent-400")
	}
	ui.Column(c).Padding(12, 14).Radius(12).Background(deviceInspectorCardBg(k)).Border(1, deviceCardBorder(k)).Gap(9).Children(func() {
		for i, step := range steps {
			ui.Row(c.Key(strconv.Itoa(i))).Gap(9).AlignItems(ui.Start).Children(func() {
				ui.Box(c.Key("num")).Size(18, 18).Shrink(0).Radius(9).Background(accentBg).Border(1, accentEdge).Center().Children(func() {
					k.Text(c.Key("n"), strconv.Itoa(i+1), desktopkit.TextProps{Role: desktopkit.RoleCaption, Emphasized: true}).TextColor(accentFg)
				})
				k.Text(c.Key("txt"), step, desktopkit.TextProps{}).Grow(1).MinWidth(0)
			})
		}
	})
}

// pluginsMarketplaceWorkspace renders the Obsidian-style full-bleed 2-column
// online plugin marketplace (matching prototype/plugins.html).
func (d *viewState) pluginsMarketplaceWorkspace(c *ui.Context, k desktopkit.Kit) {
	l := d.live
	d.ensureMarketplaceLoaded(false)
	p := &d.pluginSettings
	cfg := l.config.Config
	if p.marketCategory == "" {
		p.marketCategory = "all"
	}

	defer func() {
		d.pluginManualInstallDialog(c, k)
		d.pluginConfirmDialogs(c, k)
	}()

	q := strings.ToLower(strings.TrimSpace(p.marketQuery))
	var filtered []api.MarketplaceEntry
	for _, entry := range l.marketplace.Plugins {
		if p.marketCategory == "official" && !entry.Official {
			continue
		} else if p.marketCategory != "all" && p.marketCategory != "official" && entry.Category != p.marketCategory {
			continue
		}
		if q != "" {
			hay := strings.ToLower(entry.Name + " " + entry.Summary + " " + entry.Description + " " + entry.Author + " " + entry.ID)
			if !strings.Contains(hay, q) {
				continue
			}
		}
		filtered = append(filtered, entry)
	}

	var selected *api.MarketplaceEntry
	for i := range filtered {
		if filtered[i].ID == p.marketSelected {
			selected = &filtered[i]
			break
		}
	}
	if selected == nil && len(filtered) > 0 {
		selected = &filtered[0]
		p.marketSelected = selected.ID
	}

	cats := []struct{ id, label string }{
		{"all", "全部"},
		{"official", "官方推荐"},
		{"share", "分享与导出"},
		{"text", "文本效率"},
		{"organize", "智能整理"},
		{"sync", "同步扩展"},
	}

	ui.Column(c.Key("marketplace-workspace")).Grow(1).MinHeight(0).MinWidth(0).Children(func() {
		// Top search + category chip bar (white surface + 1px bottom border).
		ui.Column(c.Key("market-header")).Padding(12, 18, 12, 18).Gap(10).Background(deviceToolbarBg(k)).InsetShadow(0, -1, 0, 0, k.Color("separator-border")).Shrink(0).Children(func() {
			search := k.SearchField(c.Key("market-search"), desktopkit.SearchFieldProps{
				Label:       d.tr("搜索插件市场"),
				Placeholder: d.tr("搜索插件名称、功能或作者"),
				Value:       &p.marketQuery,
			})
			if search.Escaped {
				p.marketQuery = ""
			}
			ui.Row(c.Key("market-cat-chips")).Wrap().Gap(6).AlignItems(ui.Center).Children(func() {
				accentBg, accentEdge := deviceAccentSurface(k)
				accentFg := k.Color("accent-600")
				if k.Dark {
					accentFg = k.Color("accent-400")
				}
				for _, cat := range cats {
					active := p.marketCategory == cat.id
					bg := deviceCardBg(k)
					edge := deviceCardBorder(k)
					fg := k.Color("text-secondary")
					if active {
						bg, edge, fg = accentBg, accentEdge, accentFg
					}
					chip := ui.ButtonBase(c.Key("chip-"+cat.id)).Label(d.tr(cat.label)).Height(26).Padding(0, 11).Radius(999).Background(bg).Border(1, edge).Center().Children(func() {
						k.Text(c.Key("l"), d.tr(cat.label), desktopkit.TextProps{Role: desktopkit.RoleCaption, Emphasized: active}).TextColor(fg)
					})
					if chip.Clicked() {
						p.marketCategory = cat.id
					}
				}
			})
		})

		// Master-Detail 2-column body: left L1 card canvas + right 380px white inspector.
		ui.Row(c.Key("market-split")).Grow(1).MinHeight(0).MinWidth(0).AlignItems(ui.Stretch).Children(func() {
			// Left Column: L1 neutral canvas with plugin cards + sticky bottom developer footer.
			ui.Column(c.Key("market-left")).Grow(1).MinWidth(0).MinHeight(0).Background(deviceCanvasBg(k)).Children(func() {
				k.Scroll(c.Key("market-cards-scroll")).Role(ui.RoleRegion).Label(d.tr("插件市场")).Grow(1).MinHeight(0).Children(func() {
					ui.Column(c.Key("market-cards-pad")).Padding(16, 18, 20, 18).Gap(10).MinWidth(0).Children(func() {
						if len(filtered) == 0 {
							ui.Column(c.Key("market-empty")).Padding(32, 20).Radius(13).Background(deviceCardBg(k)).Border(1, deviceCardBorder(k)).AlignItems(ui.Center).Gap(6).Children(func() {
								k.Text(c.Key("t"), d.tr("无匹配插件"), desktopkit.TextProps{Emphasized: true})
								k.Text(c.Key("d"), d.tr("尝试更换关键词或切换分类。"), secondary)
							})
							return
						}
						// Single-column rectangular card list (one plugin per row).
						for _, entry := range filtered {
							d.marketplaceCard(c.Key("card-"+entry.ID), k, entry, selected != nil && selected.ID == entry.ID)
						}
					})
				})

				// Sticky subtle footer on left pane (developer manual install tucked cleanly here).
				ui.Row(c.Key("market-footer")).Padding(8, 16).Background(deviceToolbarBg(k)).InsetShadow(0, 1, 0, 0, k.Color("separator-border")).Justify(ui.SpaceBetween).AlignItems(ui.Center).Shrink(0).Children(func() {
					k.Text(c.Key("note"), d.tr("从市场安装的插件都会在本地进行完整性校验。"), footnote)
					if k.Button(c.Key("market-manual-link"), d.tr("开发者：从链接手动安装"), desktopkit.ButtonProps{Variant: desktopkit.Link, Disabled: l.showBusy()}).Clicked() && !l.busy {
						p.manualOpen = true
						p.installError = ""
					}
				})
			})

			// Right Column: 380 DIP pure-white Detail Inspector with 1px left divider.
			ui.Column(c.Key("market-right")).Width(380).Shrink(0).MinHeight(0).Background(deviceInspectorBg(k)).InsetShadow(1, 0, 0, 0, k.Color("separator-border")).Children(func() {
				if selected == nil {
					ui.Column(c.Key("no-sel")).Grow(1).Center().Padding(24).Children(func() {
						k.Text(c, d.tr("无匹配插件"), secondary)
					})
					return
				}
				entry := *selected
				d.marketplaceDetailInspector(c.Key("detail-"+entry.ID), k, entry, cfg)
			})
		})
	})
}

func (d *viewState) marketplaceCard(c *ui.Context, k desktopkit.Kit, entry api.MarketplaceEntry, isSelected bool) {
	inst, isInstalled := d.installedExternalPlugin(entry.ID)
	canUpdate := isInstalled && entry.Version != "" && entry.Version != inst.Version
	installing := d.pluginInstallTask(entry.ID) != nil
	icon, tone := pluginPresentationIcon(entry.Category, entry.Capabilities, entry.Icon)

	card := ui.ButtonBase(c).Label(entry.Name).FillWidth().MinWidth(0).Radius(12).Padding(13, 15).
		FocusRingColor(k.Color("border-focus-ring")).FocusRingOffset(2)
	card.AccessSelected(isSelected)

	bg := deviceCardBg(k)
	border := deviceCardBorder(k)
	borderW := float32(1)
	if isSelected {
		bg, _ = deviceAccentSurface(k)
		border = k.Color("accent-500")
		borderW = 1.5
	} else if card.Hovered() {
		border = deviceCardBorderHover(k)
	}
	card.Background(bg).Border(borderW, border)

	card.Children(func() {
		ui.Row(c.Key("row")).FillWidth().Gap(12).AlignItems(ui.Start).MinWidth(0).Children(func() {
			pluginIconTile(c.Key("ico"), k, icon, tone, 40)
			ui.Column(c.Key("body")).Grow(1).MinWidth(0).Gap(5).Children(func() {
				ui.Row(c.Key("top")).FillWidth().Gap(8).AlignItems(ui.Center).MinWidth(0).Children(func() {
					ui.Row(c.Key("title-row")).Grow(1).MinWidth(0).Gap(6).AlignItems(ui.Center).Wrap().Children(func() {
						k.Text(c.Key("name"), entry.Name, desktopkit.TextProps{Emphasized: true})
						if entry.Official {
							k.Tag(c.Key("off"), d.tr("官方"), desktopkit.TagProps{Color: "blue"})
						}
					})
					if installing {
						lbl := d.tr("正在安装")
						if isInstalled {
							lbl = d.tr("正在更新")
						}
						k.Tag(c.Key("st"), lbl, desktopkit.TagProps{Color: "blue"}).Shrink(0)
					} else if canUpdate {
						k.Tag(c.Key("st"), d.tr("可更新"), desktopkit.TagProps{Color: "green"}).Shrink(0)
					} else if isInstalled {
						lbl := d.tr("已安装")
						if inst.Configured {
							lbl = d.tr("已启用")
						}
						k.Tag(c.Key("st"), lbl, desktopkit.TagProps{Color: "green"}).Shrink(0)
					} else if entry.ZeroConfig {
						k.Tag(c.Key("zc"), d.tr("免配置"), desktopkit.TagProps{Color: "gray"}).Shrink(0)
					}
				})
				k.Text(c.Key("summary"), entry.Summary, secondary)
				ui.Row(c.Key("foot")).FillWidth().Gap(8).Justify(ui.SpaceBetween).AlignItems(ui.Center).MinWidth(0).Children(func() {
					meta := entry.Author
					if entry.Version != "" {
						if meta != "" {
							meta += " · "
						}
						meta += "v" + entry.Version
					}
					k.Text(c.Key("meta"), meta, footnote).Grow(1).MinWidth(0)
					if catLabel := d.categoryLabel(entry.Category); catLabel != "" {
						k.Text(c.Key("cat"), catLabel, footnote).Shrink(0)
					}
				})
			})
		})
	})
	if card.Clicked() {
		d.pluginSettings.marketSelected = entry.ID
		d.pluginSettings.marketTechOpen = false
	}
}

func (d *viewState) marketplaceDetailInspector(c *ui.Context, k desktopkit.Kit, entry api.MarketplaceEntry, cfg config.Config) {
	l := d.live
	p := &d.pluginSettings
	inst, isInstalled := d.installedExternalPlugin(entry.ID)
	canUpdate := isInstalled && entry.Version != "" && entry.Version != inst.Version
	installTask := d.pluginInstallTask(entry.ID)
	installing := installTask != nil
	icon, tone := pluginPresentationIcon(entry.Category, entry.Capabilities, entry.Icon)

	k.Scroll(c.Key("scroll")).Role(ui.RoleRegion).Label(entry.Name).Grow(1).MinHeight(0).Children(func() {
		ui.Column(c.Key("body")).Padding(18, 20, 24, 20).Gap(15).MinWidth(0).Children(func() {
			// Header: 46x46 icon tile + Name + Official badge + Author/Version.
			ui.Row(c.Key("head")).Gap(12).AlignItems(ui.Start).Children(func() {
				pluginIconTile(c.Key("ico"), k, icon, tone, 46)
				ui.Column(c.Key("meta")).Grow(1).MinWidth(0).Gap(3).Children(func() {
					ui.Row(c.Key("nr")).Gap(6).AlignItems(ui.Center).Wrap().Children(func() {
						k.Text(c.Key("name"), entry.Name, desktopkit.TextProps{Role: desktopkit.RoleTitle, Emphasized: true})
						if entry.Official {
							k.Tag(c.Key("off"), d.tr("官方"), desktopkit.TagProps{Color: "blue"})
						}
					})
					sub := entry.Author + " · v" + entry.Version
					if isInstalled {
						stText, _ := d.externalPluginStatus(inst)
						sub += " · " + stText
					}
					k.Text(c.Key("sub"), sub, footnote)
				})
			})

			// Obsidian-style single-click CTA bar.
			ui.Row(c.Key("cta")).Wrap().Gap(8).AlignItems(ui.Center).Children(func() {
				if !isInstalled {
					if installing {
						k.Button(c.Key("m-install"), d.tr("正在安装…"), desktopkit.ButtonProps{Variant: desktopkit.Primary, Icon: desktopkit.IconDownload, Busy: true, Disabled: true})
						cancelLabel := d.tr("取消安装")
						if installTask.cancelling {
							cancelLabel = d.tr("正在取消…")
						}
						if k.Button(c.Key("m-install-cancel"), cancelLabel, desktopkit.ButtonProps{Busy: installTask.cancelling, Disabled: installTask.cancelling}).Clicked() {
							d.cancelPluginInstall(entry.ID)
						}
					} else if k.Button(c.Key("m-install"), d.tr("安装"), desktopkit.ButtonProps{Variant: desktopkit.Primary, Icon: desktopkit.IconDownload}).Clicked() {
						d.installMarketplacePlugin(api.PluginInstallRequest{
							Repository: entry.Repository,
							Ref:        entry.Ref,
							Path:       entry.Path,
							Build:      entry.BuildFallback,
						}, entry.Name, entry.ID)
					}
				} else {
					if canUpdate {
						if installing {
							k.Button(c.Key("m-update"), d.tr("正在更新…"), desktopkit.ButtonProps{Variant: desktopkit.Primary, Icon: desktopkit.IconUpdate, Busy: true, Disabled: true})
							cancelLabel := d.tr("取消更新")
							if installTask.cancelling {
								cancelLabel = d.tr("正在取消…")
							}
							if k.Button(c.Key("m-update-cancel"), cancelLabel, desktopkit.ButtonProps{Busy: installTask.cancelling, Disabled: installTask.cancelling}).Clicked() {
								d.cancelPluginInstall(entry.ID)
							}
						} else if k.Button(c.Key("m-update"), fmt.Sprintf(d.tr("更新至 v%s"), entry.Version), desktopkit.ButtonProps{Variant: desktopkit.Primary, Icon: desktopkit.IconUpdate}).Clicked() {
							d.installMarketplacePlugin(api.PluginInstallRequest{
								Repository: entry.Repository,
								Ref:        entry.Ref,
								Path:       entry.Path,
								Build:      entry.BuildFallback,
								Replace:    true,
							}, entry.Name, entry.ID)
						}
					}
					if !inst.Configured || inst.LiveStart && inst.RunningVersion == "" && !inst.NeedsSettings {
						label := "启用"
						if inst.LiveStart && inst.RunningVersion == "" {
							label = "启动"
						}
						if k.Button(c.Key("m-enable"), d.tr(label), desktopkit.ButtonProps{Variant: desktopkit.Primary, Disabled: l.showBusy() || installing}).Clicked() && !installing {
							d.setExternalPluginEnabled(cfg, inst.ID, true)
						}
					} else {
						if k.Button(c.Key("m-disable"), d.tr("停用"), desktopkit.ButtonProps{Disabled: l.showBusy() || installing}).Clicked() && !installing {
							d.setExternalPluginEnabled(cfg, inst.ID, false)
						}
					}
					if pluginConfigurable(inst) {
						label, icon := "选项", desktopkit.IconSettings
						if isWeType(inst) && inst.Configured && d.channelForPlugin(inst.ID) == nil {
							label, icon = "加入共享空间", desktopkit.IconLink
						}
						if k.Button(c.Key("m-options"), d.tr(label), desktopkit.ButtonProps{Icon: icon, Disabled: l.showBusy()}).Clicked() && !l.busy {
							p.id = inst.ID
							p.view = ""
							p.form = nil
							p.channelPair = weTypePairing{}
						}
					}
					if inst.Source != "local" {
						if k.Button(c.Key("m-uninstall"), d.tr("卸载"), desktopkit.ButtonProps{Variant: desktopkit.Destructive, Icon: desktopkit.IconDelete, Disabled: l.showBusy() || installing}).Clicked() && !l.busy && !installing {
							p.uninstallOpen = true
							p.uninstallID = inst.ID
							p.uninstallName = entry.Name
						}
					}
				}
			})

			// Highlights pills.
			if len(entry.Highlights) > 0 {
				ui.Row(c.Key("highlights")).Wrap().Gap(6).Children(func() {
					for i, h := range entry.Highlights {
						k.Tag(c.Key(fmt.Sprintf("hl-%d", i)), h, desktopkit.TagProps{Color: "gray"})
					}
				})
			}

			// Description section.
			if entry.Description != "" {
				ui.Column(c.Key("desc-sec")).Gap(5).Children(func() {
					k.Text(c.Key("h"), d.tr("简介"), desktopkit.TextProps{Role: desktopkit.RoleCaption, Tone: desktopkit.ToneSecondary, Emphasized: true})
					k.Text(c.Key("p"), entry.Description, desktopkit.TextProps{})
				})
			}

			// Numbered How-To card.
			if len(entry.HowTo) > 0 {
				ui.Column(c.Key("howto-sec")).Gap(6).Children(func() {
					k.Text(c.Key("h"), d.tr("如何使用"), desktopkit.TextProps{Role: desktopkit.RoleCaption, Tone: desktopkit.ToneSecondary, Emphasized: true})
					pluginHowToCard(c.Key("card"), k, entry.HowTo)
				})
			}

			// Privacy note.
			if entry.Privacy != "" {
				subtleInfoCard(c.Key("privacy"), k, d.tr("隐私与安全"), entry.Privacy)
			}

			// Collapsible technical/source metadata (default collapsed so ordinary users aren't distracted).
			ui.Box(c.Key("tech-div")).Height(1).FillWidth().Background(k.Color("separator-border"))
			techIcon := desktopkit.IconChevronRight
			if p.marketTechOpen {
				techIcon = desktopkit.IconChevronDown
			}
			ui.Row(c.Key("tech-toggle-row")).AlignItems(ui.Center).Children(func() {
				if k.Button(c.Key("toggle-tech"), d.tr("查看来源与技术信息"), desktopkit.ButtonProps{Variant: desktopkit.Link, Icon: techIcon}).Clicked() {
					p.marketTechOpen = !p.marketTechOpen
				}
			})
			if p.marketTechOpen {
				k.FactList(c.Key("market-source"), desktopkit.FactListProps{
					Label: d.tr("来源信息"),
					Facts: []desktopkit.Fact{
						{ID: "id", Label: d.tr("插件标识"), Value: entry.ID, Monospace: true},
						{ID: "repo", Label: d.tr("开源仓库"), Value: strings.TrimPrefix(entry.Repository, "https://"), Monospace: true},
					},
				})
			}
		})
	})
}

func (d *viewState) pluginsAdvancedView(c *ui.Context, k desktopkit.Kit, cfg config.Config) {
	l := d.live
	subtleInfoCard(c.Key("order-note"), k, "", d.tr("顺序仅影响同一阶段多个插件的先后；同步始终在本地保存之后执行。"))

	// Assemble visible order list from configured order + built-ins.
	order := slices.Clone(cfg.Plugins.Order)
	for _, b := range builtinPluginEntries {
		if cfg.PluginEnabled(b.id) && !slices.Contains(order, b.id) {
			order = append(order, b.id)
		}
	}
	nameOf := func(id string) string {
		for _, b := range builtinPluginEntries {
			if b.id == id {
				return d.tr(b.name)
			}
		}
		if item, ok := d.installedExternalPlugin(id); ok {
			name, _ := d.pluginDisplay(item)
			return name
		}
		if cat, ok := d.marketplaceEntry(id); ok {
			return cat.Name
		}
		return id
	}

	k.SettingsGroup(c.Key("plugin-order-group"), desktopkit.SettingsGroupProps{Title: d.tr("处理先后顺序")}, func() {
		if len(order) == 0 {
			k.SettingsRow(c.Key("order-empty"), desktopkit.SettingsRowProps{Label: d.tr("当前没有已启用插件")}, nil, nil)
			return
		}
		for i, id := range order {
			idx := i
			k.SettingsRow(c.Key("ord-"+id), desktopkit.SettingsRowProps{
				Label:       fmt.Sprintf("%d. %s", idx+1, nameOf(id)),
				Description: id,
			}, nil, func() {
				if k.Button(c.Key("up-"+id), d.tr("上移"), desktopkit.ButtonProps{Variant: desktopkit.Subtle, Disabled: idx == 0 || l.showBusy()}).Clicked() && !l.busy {
					next := slices.Clone(order)
					next[idx-1], next[idx] = next[idx], next[idx-1]
					cfg.Plugins.Order = next
					d.saveLiveConfig(cfg)
				}
				if k.Button(c.Key("down-"+id), d.tr("下移"), desktopkit.ButtonProps{Variant: desktopkit.Subtle, Disabled: idx == len(order)-1 || l.showBusy()}).Clicked() && !l.busy {
					next := slices.Clone(order)
					next[idx+1], next[idx] = next[idx], next[idx+1]
					cfg.Plugins.Order = next
					d.saveLiveConfig(cfg)
				}
			})
		}
	})

	k.SettingsGroup(c.Key("plugin-global-error"), desktopkit.SettingsGroupProps{Title: d.tr("异常处理")}, func() {
		policy := cfg.Plugins.OnError
		if policy == "" {
			policy = "continue"
		}
		k.SettingsRow(c.Key("global-on-error"), desktopkit.SettingsRowProps{
			Label:       d.tr("当插件运行出错时"),
			Description: d.tr("决定是否允许该条剪贴板继续同步到其他设备。重启后生效。"),
		}, nil, func() {
			items := []desktopkit.SelectItem{
				{Value: "continue", Label: d.tr("继续处理")},
				{Value: "block_sync", Label: d.tr("禁止同步此条目")},
			}
			if k.Select(c, desktopkit.SelectProps{Label: d.tr("当插件运行出错时"), Value: &policy, Items: items, Disabled: l.showBusy()}).Changed && !l.busy {
				cfg.Plugins.OnError = policy
				d.saveLiveConfig(cfg)
			}
		})
	})
}

func (d *viewState) pluginManualInstallDialog(c *ui.Context, k desktopkit.Kit) {
	p := &d.pluginSettings
	repo := strings.TrimSpace(p.manualRepo)
	installTask := d.pluginInstallTask(repo)
	installing := installTask != nil
	k.Dialog(c.Key("manual-install-dialog"), desktopkit.DialogProps{
		Open:        &p.manualOpen,
		Title:       d.tr("从链接手动安装"),
		Description: d.tr("推荐优先使用插件市场。手动安装适用于测试未上架插件，请确保来源可信。"),
		Width:       480,
	}, func() {
		k.FormField(c.Key("manual-repo"), desktopkit.FormFieldProps{
			Label:       d.tr("仓库链接"),
			Value:       &p.manualRepo,
			Placeholder: "https://github.com/owner/unic-plugin-slug",
			Disabled:    installing,
		})
		k.FormField(c.Key("manual-ref"), desktopkit.FormFieldProps{
			Label:       d.tr("版本 Ref（可选）"),
			Value:       &p.manualRef,
			Placeholder: d.tr("留空使用默认分支"),
			Disabled:    installing,
		})
		k.FormField(c.Key("manual-path"), desktopkit.FormFieldProps{
			Label:       d.tr("子目录（可选）"),
			Value:       &p.manualPath,
			Placeholder: "plugins/example",
			Disabled:    installing,
		})
		k.Checkbox(c.Key("manual-build"), &p.manualBuild, desktopkit.CheckboxProps{
			Label:    d.tr("无预编译产物时从源码构建"),
			Disabled: installing,
		})
		k.Checkbox(c.Key("manual-enable"), &p.manualEnable, desktopkit.CheckboxProps{
			Label:    d.tr("安装后自动启用"),
			Disabled: installing,
		})
		ui.Row(c.Key("manual-actions")).Justify(ui.End).Gap(8).Children(func() {
			dismissLabel := d.tr("取消")
			if installing {
				dismissLabel = d.tr("关闭")
			}
			if k.Button(c.Key("manual-cancel"), dismissLabel, desktopkit.ButtonProps{}).Clicked() {
				p.manualOpen = false
			}
			if installing {
				cancelLabel := d.tr("取消安装")
				if installTask.cancelling {
					cancelLabel = d.tr("正在取消…")
				}
				if k.Button(c.Key("manual-abort"), cancelLabel, desktopkit.ButtonProps{Busy: installTask.cancelling, Disabled: installTask.cancelling}).Clicked() {
					d.cancelPluginInstall(repo)
				}
				k.Button(c.Key("manual-submit"), d.tr("正在安装…"), desktopkit.ButtonProps{Variant: desktopkit.Primary, Busy: true, Disabled: true})
				return
			}
			canSubmit := strings.HasPrefix(repo, "https://github.com/")
			if k.Button(c.Key("manual-submit"), d.tr("安装"), desktopkit.ButtonProps{Variant: desktopkit.Primary, Disabled: !canSubmit}).Clicked() && canSubmit {
				d.installMarketplacePlugin(api.PluginInstallRequest{
					Repository: repo,
					Ref:        strings.TrimSpace(p.manualRef),
					Path:       strings.TrimSpace(p.manualPath),
					Build:      p.manualBuild,
					Enable:     p.manualEnable,
				}, "", repo)
			}
		})
	})
}

func (d *viewState) pluginConfirmDialogs(c *ui.Context, k desktopkit.Kit) {
	l := d.live
	p := &d.pluginSettings
	if res := k.AlertDialog(c.Key("uninstall-plugin-alert"), desktopkit.AlertDialogProps{
		Open:              &p.uninstallOpen,
		Title:             fmt.Sprintf(d.tr("卸载「%s」？"), p.uninstallName),
		Description:       d.tr("重启时将停止并移除该插件程序。已保存的剪贴板历史不会被删除，随时可以在插件市场重新安装。"),
		ConfirmLabel:      d.tr("卸载"),
		CancelLabel:       d.tr("取消"),
		Destructive:       true,
		KeepOpenOnConfirm: true,
		ConfirmDisabled:   l.showBusy(),
	}, nil); res.Confirmed {
		d.uninstallExternalPlugin(p.uninstallID, p.uninstallName)
	}
	if res := k.AlertDialog(c.Key("rollback-plugin-alert"), desktopkit.AlertDialogProps{
		Open:              &p.rollbackOpen,
		Title:             fmt.Sprintf(d.tr("回滚「%s」？"), p.rollbackName),
		Description:       fmt.Sprintf(d.tr("将切换回上一安装版本 v%s，重启桌面端后生效。"), p.rollbackVer),
		ConfirmLabel:      d.tr("确认回滚"),
		CancelLabel:       d.tr("取消"),
		KeepOpenOnConfirm: true,
		ConfirmDisabled:   l.showBusy(),
	}, nil); res.Confirmed {
		d.rollbackExternalPlugin(p.rollbackID, p.rollbackName)
	}
}
