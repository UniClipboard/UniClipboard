package desktopui

import (
	"context"
	"fmt"
	"mime"
	"net/url"
	"path/filepath"
	"slices"
	"strings"
	"time"

	"github.com/uniclipboard/unic/engine"
	api "github.com/uniclipboard/unic/internal/localapi"
	"github.com/uniclipboard/unic/plugin"

	"github.com/egoist/mygo/ui"
	"github.com/egoist/mygo/ui/richtext"

	"github.com/uniclipboard/unic/app/desktop/internal/desktopkit"
	"github.com/uniclipboard/unic/internal/desktopkeys"
)

func (d *viewState) sidebar(c *ui.Context, k desktopkit.Kit) {
	favoriteCount := ""
	if d.live == nil {
		count := 0
		for _, v := range d.entries {
			if v.pinned {
				count++
			}
		}
		favoriteCount = fmt.Sprint(count)
	} else if count, ok := d.live.countFor(favoriteHistoryQuery()); ok {
		favoriteCount = fmt.Sprint(count)
	}
	tags := []desktopkit.SidebarNavItem{}
	for _, tag := range d.tags {
		if !tag.sidebar {
			continue
		}
		tagCountLabel := ""
		if d.live == nil {
			tagCountLabel = fmt.Sprint(d.tagCount(tag.id))
		} else if count, ok := d.live.countFor(tagHistoryQuery(tag.id)); ok {
			tagCountLabel = fmt.Sprint(count)
		}
		tags = append(tags, desktopkit.SidebarNavItem{ID: "tag:" + tag.id, Label: d.tagLabel(tag.id), Swatch: tag.color, Count: tagCountLabel})
	}
	devices := []desktopkit.SidebarNavItem{}
	for _, v := range d.devices {
		if v.id == "local" {
			continue
		}
		tone := desktopkit.StatusNeutral
		if v.online {
			tone = desktopkit.StatusPositive
		}
		state := ""
		if !v.online {
			state = d.tr("离线")
		}
		devices = append(devices, desktopkit.SidebarNavItem{ID: "device:" + v.id, Label: v.name, Status: &tone, Count: state})
	}
	var overrides map[string][]string
	if d.live != nil {
		overrides = d.live.config.Config.Desktop.Shortcuts
	}
	k.SidebarNav(c, &d.nav, desktopkit.SidebarNavProps{Label: d.tr("资料库"), Sections: []desktopkit.SidebarNavSection{
		{ID: "library", Items: []desktopkit.SidebarNavItem{{ID: "all", Label: d.tr("全部项目"), Icon: desktopkit.IconHistory, Count: d.historyCountLabel()}, {ID: "pinned", Label: d.tr("已收藏"), Icon: desktopkit.IconPin, Count: favoriteCount}}},
		{ID: "tags", Title: d.tr("标签"), Items: tags, Action: &desktopkit.NavAction{Label: d.tr("管理标签"), Icon: desktopkit.IconAdd}, More: &desktopkit.NavAction{Label: fmt.Sprintf(d.tr("全部标签 · %d"), len(d.tags))}},
		{ID: "devices", Title: d.tr("设备"), Items: devices},
	}, Footer: []desktopkit.SidebarNavItem{{ID: "manage", Label: d.tr("管理设备…"), Icon: desktopkit.IconDevices}, {ID: "settings", Label: d.tr("设置…"), Icon: desktopkit.IconSettings, ShortcutHint: shortcutHint(overrides, desktopkeys.Settings)}}}).Grow(1)
	if cmd, _ := d.nav.Command(); cmd == desktopkit.NavSectionAction || cmd == desktopkit.NavMore {
		d.tagsOpen = true
	}
	if d.nav.Changed() {
		d.notice = ""
		switch {
		case d.nav.Value == "manage":
			d.page = "devices"
			d.nav.Value = "all"
		case d.nav.Value == "settings":
			d.page = "settings"
			d.nav.Value = "all"
		case strings.HasPrefix(d.nav.Value, "tag:"):
			d.tag, d.source = strings.TrimPrefix(d.nav.Value, "tag:"), ""
		case strings.HasPrefix(d.nav.Value, "device:"):
			d.source, d.tag = strings.TrimPrefix(d.nav.Value, "device:"), ""
		default:
			d.tag, d.source = "", ""
		}
		if d.compact {
			d.sidebarHidden = true
		}
	}
}

func (d *viewState) history(c *ui.Context, k desktopkit.Kit) {
	if d.live != nil && d.live.sendCancel != nil {
		labels := map[string]string{"scanning": "正在扫描目录…", "preparing": "正在准备目录…", "processing": "正在提交…", "sending": "正在发送…"}
		label := labels[d.live.sendStage]
		if label == "" {
			label = "正在准备发送…"
		}
		k.Text(c, d.tr(label), desktopkit.TextProps{})
		if k.Button(c.Key("cancel-send"), d.tr("取消发送"), desktopkit.ButtonProps{}).Clicked() {
			d.live.sendCancel()
		}
	}

	items := d.filtered()
	if d.current() == nil || (d.live == nil && !slices.ContainsFunc(items, func(v entry) bool { return v.id == d.selected })) {
		d.selectEntry("")
		if len(items) > 0 {
			d.selectEntry(items[0].id)
		}
	}
	gridDrawerOpen := d.layout == "grid" && !d.compact && d.inspector && d.current() != nil
	if gridDrawerOpen {
		c.OnWindowShortcut(0, ui.KeyEscape, func() {
			d.inspector = false
		})
	} else if len(items) == 0 && (d.hasActiveSearchOrFilter() || d.isSearchUnavailableState()) && !d.tagsOpen && !d.tagEditor.open && !d.actionResult.open && d.deviceTask == "" && d.confirm == "" && !d.filterOpen {
		c.OnWindowShortcut(0, ui.KeyEscape, d.clearAllHistoryFilters)
	}
	coldLoading := len(items) == 0 && d.live != nil && d.live.foregroundLoading && !d.live.loaded
	subtitle := fmt.Sprint(len(items))
	if d.isSearchUnavailableState() {
		subtitle = "—"
	} else if coldLoading {
		subtitle = ""
	}
	subtitleMuted := !coldLoading && d.live != nil && d.live.showForegroundLoading() && d.live.displayedKey != d.liveQueryKey()
	k.SplitView(c, desktopkit.SplitViewProps{ContentScale: zoomScale(d.zoomStep), ToolbarTrailing: d.trailingToolbar(c),
		Title: d.historyHeadingTitle(), Subtitle: subtitle, SubtitleMuted: subtitleMuted,
		ToolbarLoading: d.live != nil && d.live.showForegroundLoading(),
		Sidebar:        func() { d.sidebar(c, k) }, SidebarHidden: &d.sidebarHidden, SidebarOverlay: d.compact,
		SidebarToggleLabel: d.tr("切换侧栏"), DetailOpen: &d.detailOpen, BackLabel: d.tr("返回历史"),
		DrawerOpen:     gridDrawerOpen,
		DrawerLabel:    d.tr("历史详情"),
		DrawerWidth:    436,
		DrawerInset:    8,
		DrawerRadius:   12,
		SearchCentered: true,
		Drawer: func() {
			d.gridDetailDrawer(c.Key("grid-drawer"), k, items)
		},
		Search: func() {
			d.historySearch(c.Key("search"), k)
		},
		ToolbarLeading: func() {
			ui.Row(c.Key("layout-controls")).AlignItems(ui.Center).Shrink(0).Children(func() {
				showSlider := d.layout == "grid" && !d.compact && len(items) > 0
				sliderHost := ui.Row(c.Key("card-size-reveal")).Height(30).AlignItems(ui.Center).Clip().Shrink(0).AccessHidden(!showSlider)
				sliderDur := 220 * time.Millisecond
				if c.Preferences().ReduceMotion {
					sliderDur = 0
				}
				targetP := float32(0)
				if showSlider {
					targetP = 1
				}
				sliderP := sliderHost.AnimateTo("slider-p", targetP, ui.Animation{
					Duration: sliderDur,
					Ease:     ui.CubicBezier(.32, .72, 0, 1),
				})
				if sliderP > 0.01 {
					sliderHost.Width(124*sliderP).Margin(0, 6*sliderP, 0, 0).Opacity(sliderP)
					sliderHost.Children(func() {
						ui.Row(c.Key("card-size-group")).Width(124).Gap(7).PaddingX(4).Height(30).AlignItems(ui.Center).Shrink(0).Children(func() {
							k.Icon(c.Key("small"), desktopkit.IconGrid, 13).TextColor(k.Color("text-tertiary"))
							k.Slider(c.Key("card-size"), &d.gridSize, desktopkit.SliderProps{Label: d.tr("卡片大小"), Min: 0, Max: 1, Step: .05, Compact: true}).Width(74)
							ui.Box(c.Key("large")).Size(13, 13).Radius(2.5).Border(1.4, k.Color("text-tertiary")).Shrink(0).AccessHidden(true)
						})
					})
				} else {
					sliderHost.Width(0)
				}
				r := k.SegmentedControl(c.Key("layout"), desktopkit.SegmentedControlProps{Label: d.tr("显示方式"), Value: &d.layout, Toolbar: true, Items: []desktopkit.SegmentedItem{{Value: "list", Label: d.tr("列表"), Icon: desktopkit.IconList, IconOnly: true}, {Value: "grid", Label: d.tr("网格"), Icon: desktopkit.IconGrid, IconOnly: true}}})
				if r.Changed {
					d.inspector, d.detailOpen = false, false
				}
			})
		},
		Toolbar: func() { k.ToolbarGroup(c, func() { d.historyCommands(c, k) }) },
	}, func() {
		// V3 Segmented Directional Slide between List and Grid views (±28 DIP).
		stage := ui.Row(c.Key("v3-layout-stage")).Grow(1).MinWidth(0).MinHeight(0).AlignItems(ui.Stretch).Clip()
		st := ui.Local(stage, "v3-switch", func() layoutSwitchState { return layoutSwitchState{} })
		if !st.initialized {
			stage.Jump("v3-p", 1)
			st.initialized = true
			st.lastLayout = d.layout
		} else if d.layout != st.lastLayout {
			if !c.Preferences().ReduceMotion {
				if d.layout == "grid" {
					st.dir = 1
				} else {
					st.dir = -1
				}
				stage.Jump("v3-p", 0)
			} else {
				stage.Jump("v3-p", 1)
			}
			st.lastLayout = d.layout
		}
		v3Dur := 230 * time.Millisecond
		if c.Preferences().ReduceMotion {
			v3Dur = 0
			stage.Jump("v3-p", 1)
		}
		v3P := stage.AnimateTo("v3-p", 1, ui.Animation{Duration: v3Dur, Ease: ui.CubicBezier(.22, 1, .36, 1)})
		if v3P < 0.999 {
			stage.Translate(28*st.dir*(1-v3P), 0).Opacity(0.55 + 0.45*v3P)
		}
		stage.Children(func() {
			empty := len(items) == 0
			coldLoading := empty && d.live != nil && d.live.foregroundLoading && !d.live.loaded
			initialLoading := empty && d.live != nil && d.live.foregroundLoading
			showColdSplit := coldLoading && d.layout == "list" && !d.compact
			pane := desktopkit.PaneList
			if d.layout == "grid" || (empty && !showColdSplit) {
				pane = desktopkit.PaneMain
			}
			k.Pane(c.Key("browse"), desktopkit.PaneProps{Kind: pane, Label: d.tr("历史列表"), DetailOpen: &d.detailOpen}, func() {
				resultsStage := ui.Column(c.Key("results-stage")).Grow(1).MinWidth(0).MinHeight(0).AlignItems(ui.Stretch)
				rsw := ui.Local(resultsStage, "results-transition", func() resultsTransitionState { return resultsTransitionState{} })
				resultKey := d.resultsTransitionKey(empty)
				if !rsw.initialized {
					resultsStage.Jump("res-p", 1)
					resultsStage.Jump("stale-a", 1)
					rsw.initialized = true
					rsw.lastKey = resultKey
					rsw.wasEmpty = empty
					rsw.lastScope = d.liveScopeKey()
				} else if resultKey != rsw.lastKey {
					scopeShift := rsw.wasEmpty != empty || d.isScopeResultTransition(rsw.lastScope)
					rsw.scopeShift = scopeShift
					rsw.wasEmpty = empty
					rsw.lastScope = d.liveScopeKey()
					if !c.Preferences().ReduceMotion {
						resultsStage.Jump("res-p", 0)
					} else {
						resultsStage.Jump("res-p", 1)
					}
					rsw.lastKey = resultKey
				}
				resAnim := ui.Animation{Duration: 115 * time.Millisecond, Ease: ui.EaseOut, NonMotion: true}
				if rsw.scopeShift {
					resAnim = ui.Animation{Duration: 165 * time.Millisecond, Ease: ui.CubicBezier(.22, 1, .36, 1)}
				}
				if c.Preferences().ReduceMotion {
					resAnim.Duration = 0
					resultsStage.Jump("res-p", 1)
				}
				resP := resultsStage.AnimateTo("res-p", 1, resAnim)
				staleTarget := float32(1)
				if d.live != nil && d.live.showForegroundLoading() && !empty {
					staleTarget = 0.78
				}
				staleA := resultsStage.AnimateTo("stale-a", staleTarget, ui.Animation{
					Duration:  140 * time.Millisecond,
					Ease:      ui.EaseOut,
					NonMotion: true,
				})
				baseOpacity := float32(0.88) + 0.12*resP
				if rsw.scopeShift {
					baseOpacity = 0.68 + 0.32*resP
				}
				opacity := baseOpacity * staleA
				if rsw.scopeShift && resP < 0.999 {
					resultsStage.Translate(0, 4*(1-resP))
				}
				if opacity < 0.999 {
					resultsStage.Opacity(opacity)
				}
				resultsStage.Children(func() {
					if coldLoading {
						if d.live.showForegroundLoading() {
							if d.layout == "grid" {
								d.coldStartGridSkeleton(c.Key("cold-grid"), k)
							} else {
								d.coldStartListSkeleton(c.Key("cold-list"), k)
							}
						}
					} else if initialLoading {
						if d.live.showForegroundLoading() {
							ui.Column(c.Key("history-loading")).Grow(1).FillWidth().AlignItems(ui.Center).Justify(ui.Center).Gap(10).Children(func() {
								ui.Spinner(c.Key("spin")).Size(18, 18).TextColor(k.Color("text-tertiary"))
								k.Text(c.Key("lbl"), d.tr("正在读取历史…"), secondary)
							})
						}
					} else if empty {
						d.historyEmptyState(c, k)
					} else {
						d.historyList(c, k, items)
					}
				})
			})
			if showColdSplit {
				k.Pane(c.Key("detail"), desktopkit.PaneProps{Kind: desktopkit.PaneDetail, Label: d.tr("历史详情"), DetailOpen: &d.detailOpen}, func() {
					if d.live.showForegroundLoading() {
						d.coldStartDetailSkeleton(c.Key("cold-detail"), k)
					}
				})
			} else if !empty && (d.layout == "list" || (d.compact && d.detailOpen)) {
				k.Pane(c.Key("detail"), desktopkit.PaneProps{Kind: desktopkit.PaneDetail, Label: d.tr("历史详情"), DetailOpen: &d.detailOpen}, func() { d.detail(c, k) })
			}
		})
	})
	if d.quickLook && d.current() != nil {
		copyLabel := "复制图片"
		if d.current().kind == desktopkit.KindFile {
			copyLabel = "复制文件"
		}
		image := d.entryImage(*d.current())
		r := k.QuickLook(c.Key("quick-look"), desktopkit.QuickLookProps{
			Open: &d.quickLook, Title: d.entryTitle(*d.current()), CloseLabel: d.tr("关闭预览"),
			Src: image.source, Alt: d.current().title, ImageDrawOver: image.drawOver(), PrimaryLabel: d.tr(copyLabel),
		}, nil)
		if r.Primary {
			d.useEntry()
		}
	}
}

type layoutSwitchState struct {
	initialized bool
	lastLayout  string
	dir         float32
}

type resultsTransitionState struct {
	initialized bool
	lastKey     string
	lastScope   string
	wasEmpty    bool
	scopeShift  bool
}

type detailImageFadeState struct {
	initialized bool
	lastID      string
	hadImage    bool
}

func (d *viewState) resultsTransitionKey(empty bool) string {
	if d.live != nil {
		return fmt.Sprintf("live:%d:%v", d.live.resultEpoch, empty)
	}
	return fmt.Sprintf("demo:%s:%v", d.liveQueryKey(), empty)
}

func (d *viewState) isScopeResultTransition(prevScope string) bool {
	if d.live != nil {
		return d.live.lastResultKind == "scope" || d.live.lastResultKind == "cold"
	}
	return prevScope != d.liveScopeKey()
}

func isImageFilename(name string) bool {
	ext := strings.ToLower(filepath.Ext(strings.TrimSpace(name)))
	if ext == "" {
		return false
	}
	if strings.HasPrefix(mime.TypeByExtension(ext), "image/") {
		return true
	}
	switch ext {
	case ".png", ".jpg", ".jpeg", ".gif", ".webp", ".bmp", ".tif", ".tiff":
		return true
	default:
		return false
	}
}

func (d *viewState) detailCached(id string) bool {
	if d.live == nil {
		return false
	}
	_, ok := d.live.details[id]
	return ok
}

func (d *viewState) expectImagePreview(v entry) bool {
	if v.kind == desktopkit.KindImage || v.image != nil {
		return true
	}
	if v.kind != desktopkit.KindFile {
		return false
	}
	if d.live != nil {
		if it, ok := d.live.items[v.id]; ok {
			fileCount := 0
			var size int64
			for _, p := range it.Parts {
				if p.Kind == "file" {
					fileCount++
					size = p.Size
				}
			}
			if fileCount != 1 || size > 32<<20 {
				return false
			}
		}
	}
	name := strings.TrimSpace(entryBodyText(v))
	if name == "" {
		name = strings.TrimSpace(v.title)
	}
	if name == "" || strings.Contains(name, "\n") {
		return false
	}
	return isImageFilename(name)
}

func (d *viewState) previewTextWhileLoading(v entry) (string, bool) {
	if d.live == nil || !d.live.detailBusy || v.contentError != "" || v.preview == "" {
		return "", false
	}
	if v.kind != desktopkit.KindText && v.kind != desktopkit.KindCode && v.kind != desktopkit.KindLink {
		return "", false
	}
	if it, ok := d.live.items[v.id]; ok {
		for _, p := range it.Parts {
			if p.MIME == "text/html" {
				return "", false
			}
		}
	}
	return v.preview, true
}

func (d *viewState) renderDetailImagePreview(c *ui.Context, k desktopkit.Kit, v entry, height float32) {
	img := d.entryImage(v)
	upgrading := d.live != nil && v.picture != nil && !v.picture.matches(0)
	stage := ui.Box(c.Key("img-stage")).FillWidth().Height(height).Clip().Center()
	st := ui.Local(stage, "detail-img-fade", func() detailImageFadeState { return detailImageFadeState{} })
	if !st.initialized || st.lastID != v.id {
		st.initialized = true
		st.lastID = v.id
		st.hadImage = img.source != nil
		stage.Jump("img-a", 1)
		if upgrading {
			stage.Jump("img-blur", 1)
		} else {
			stage.Jump("img-blur", 0)
		}
	} else if !st.hadImage && img.source != nil {
		st.hadImage = true
		stage.Jump("img-a", 0)
	}
	alpha := stage.AnimateTo("img-a", 1, ui.Animation{
		Duration:  150 * time.Millisecond,
		Ease:      ui.EaseOut,
		NonMotion: true,
	})
	blurTarget := float32(0)
	if upgrading {
		blurTarget = 1
	}
	blurP := stage.AnimateTo("img-blur", blurTarget, ui.Animation{
		Duration:  210 * time.Millisecond,
		Ease:      ui.EaseOut,
		NonMotion: true,
	})
	stage.Children(func() {
		if img.source == nil {
			k.ShimmerBox(c.Key("img-skel"), 8).Fill()
			return
		}
		im := img.element(c.Key("img")).FillWidth().Height(height)
		if alpha < 0.999 {
			im.Opacity(alpha)
		}
		if blurP > 0.01 {
			im.Blur(6 * blurP)
			if !c.Preferences().ReduceMotion {
				sc := float32(1) + 0.03*blurP
				im.Scale(sc, sc)
			}
		}
		if upgrading && d.live != nil && d.live.showDetailLoading() {
			ui.Row(c.Key("hd-badge")).Absolute().Right(10).Bottom(10).Padding(4, 9).Radius(999).Gap(5).
				AlignItems(ui.Center).Background(ui.RGBA(17, 17, 20, .64)).ZIndex(2).Children(func() {
				ui.Spinner(c.Key("spin")).Size(11, 11).TextColor(ui.RGB(255, 255, 255)).Shrink(0).AccessHidden(true)
				ui.Text(c.Key("lbl"), d.tr("正在加载原图…")).FontSize(11).TextColor(ui.RGB(255, 255, 255)).SingleLine()
			})
		}
	})
}

func (d *viewState) renderStreamingTailSkeleton(c *ui.Context, k desktopkit.Kit, padX float32, onDark bool) {
	sep := k.Color("separator-border")
	if onDark {
		sep = ui.RGBA(255, 255, 255, .14)
	}
	ui.Column(c.Key("streaming-tail")).Margin(0, padX, 16, padX).Padding(10, 0, 0, 0).Gap(7).
		BorderWidth(1, 0, 0, 0).BorderColor(sep).Children(func() {
		k.ShimmerBox(c.Key("t1"), 5).WidthPercent(88).Height(11)
		k.ShimmerBox(c.Key("t2"), 5).WidthPercent(62).Height(11)
	})
}

func (d *viewState) renderDetailLoadingSkeleton(c *ui.Context, k desktopkit.Kit, v entry, padY, padX float32) {
	if v.kind == desktopkit.KindFolder {
		ui.Column(c.Key("folder-skel")).Padding(padY, padX).Gap(10).FillWidth().Children(func() {
			widths := []float32{78, 64, 86, 52}
			for i, w := range widths {
				ui.Row(c.Key(i)).Gap(10).AlignItems(ui.Center).FillWidth().Children(func() {
					k.ShimmerBox(c.Key("ic"), 4).Size(16, 16).Shrink(0)
					k.ShimmerBox(c.Key("ln"), 5).WidthPercent(w).Height(12)
				})
			}
		})
		return
	}
	ui.Column(c.Key("body-skel")).Padding(padY, padX).Gap(10).FillWidth().Children(func() {
		k.ShimmerBox(c.Key("l1"), 5).WidthPercent(84).Height(13)
		k.ShimmerBox(c.Key("l2"), 5).WidthPercent(94).Height(13)
		k.ShimmerBox(c.Key("l3"), 5).WidthPercent(62).Height(13)
	})
}

func (d *viewState) coldStartListSkeleton(c *ui.Context, k desktopkit.Kit) {
	border := k.Color("separator-border")
	ui.Column(c).Grow(1).FillWidth().Padding(10, 10, 24, 10).Gap(6).Children(func() {
		ui.Row(c.Key("gh")).Padding(6, 6, 4, 6).AlignItems(ui.Center).Children(func() {
			k.ShimmerBox(c.Key("t"), 5).Size(44, 11)
			ui.Spacer(c)
			k.ShimmerBox(c.Key("cnt"), 5).Size(28, 11)
		})
		widths := [][2]float32{{72, 54}, {64, 48}, {82, 66}, {58, 70}, {76, 44}}
		for i, w := range widths {
			ui.Row(c.Key(i)).Padding(9, 11).Radius(10).Gap(10).AlignItems(ui.Start).
				Background(k.CardSurface()).Border(1, border).FillWidth().Children(func() {
				if i == 1 {
					k.ShimmerBox(c.Key("th"), 7).Size(34, 34).Shrink(0)
				} else {
					k.ShimmerBox(c.Key("ic"), 7).Size(28, 28).Margin(2, 0, 0, 0).Shrink(0)
				}
				ui.Column(c.Key("col")).Grow(1).MinWidth(0).Gap(6).Children(func() {
					k.ShimmerBox(c.Key("r1"), 5).WidthPercent(w[0]).Height(13)
					k.ShimmerBox(c.Key("r2"), 5).WidthPercent(w[1]).Height(11)
					k.ShimmerBox(c.Key("r3"), 5).Size(108, 10)
				})
			})
		}
	})
}

func (d *viewState) coldStartDetailSkeleton(c *ui.Context, k desktopkit.Kit) {
	border := k.Color("separator-border")
	ui.Column(c).Grow(1).FillWidth().Padding(18, 24, 36, 24).Gap(16).Children(func() {
		ui.Row(c.Key("hdr")).Gap(10).AlignItems(ui.Center).Children(func() {
			k.ShimmerBox(c.Key("badge"), 6).Size(48, 20)
			k.ShimmerBox(c.Key("prov"), 5).Size(168, 13)
		})
		ui.Column(c.Key("frame")).Padding(20).Gap(10).Radius(12).
			Background(k.SubtleCardSurface()).Border(1, border).FillWidth().Children(func() {
			k.ShimmerBox(c.Key("p1"), 5).WidthPercent(82).Height(14)
			k.ShimmerBox(c.Key("p2"), 5).WidthPercent(94).Height(14)
			k.ShimmerBox(c.Key("p3"), 5).WidthPercent(76).Height(14)
			k.ShimmerBox(c.Key("p4"), 5).WidthPercent(46).Height(14)
		})
		ui.Row(c.Key("facts")).Gap(8).FillWidth().Children(func() {
			for i := 0; i < 3; i++ {
				ui.Column(c.Key(i)).Grow(1).Basis(0).Padding(9, 11).Radius(9).Gap(6).
					Background(k.SubtleCardSurface()).Border(1, border).Children(func() {
					k.ShimmerBox(c.Key("k"), 4).Size(44, 10)
					k.ShimmerBox(c.Key("v"), 5).Size(74, 13)
				})
			}
		})
	})
}

func (d *viewState) coldStartGridSkeleton(c *ui.Context, k desktopkit.Kit) {
	border := k.Color("separator-border")
	ui.Column(c).Grow(1).FillWidth().Padding(14, 16, 28, 16).Gap(12).Children(func() {
		k.ShimmerBox(c.Key("gh"), 5).Size(48, 12)
		for row := 0; row < 2; row++ {
			ui.Row(c.Key(row)).Gap(12).FillWidth().Children(func() {
				for col := 0; col < 3; col++ {
					ui.Column(c.Key(col)).Grow(1).Basis(0).Height(176).Padding(11).Radius(12).Gap(8).
						Background(k.CardSurface()).Border(1, border).Children(func() {
						ui.Row(c.Key("h")).Gap(6).AlignItems(ui.Center).Children(func() {
							k.ShimmerBox(c.Key("ic"), 5).Size(18, 18).Shrink(0)
							k.ShimmerBox(c.Key("t"), 5).WidthPercent(58).Height(12)
						})
						k.ShimmerBox(c.Key("m"), 8).Grow(1).FillWidth()
						k.ShimmerBox(c.Key("f"), 5).WidthPercent(52).Height(10)
					})
				}
			})
		}
	})
}

func entryBodyText(v entry) string {
	if v.text != "" {
		return v.text
	}
	return v.preview
}

func listRowExcerpt(v entry, title string) string {
	body := entryBodyText(v)
	switch v.kind {
	case desktopkit.KindImage:
		return ""
	case desktopkit.KindLink:
		raw := strings.TrimSpace(body)
		if raw == "" || raw == strings.TrimSpace(title) {
			return ""
		}
		return raw
	case desktopkit.KindFile, desktopkit.KindFolder:
		var parts []string
		for _, line := range strings.Split(body, "\n") {
			s := strings.TrimSpace(line)
			if s != "" && s != strings.TrimSpace(title) {
				parts = append(parts, s)
			}
		}
		if len(parts) == 0 {
			return ""
		}
		return strings.Join(parts, ", ")
	case desktopkit.KindCode:
		var parts []string
		for _, line := range strings.Split(body, "\n") {
			s := strings.TrimSpace(line)
			if s != "" {
				parts = append(parts, s)
			}
		}
		joined := strings.Join(parts, " ")
		if joined == "" || joined == strings.TrimSpace(title) {
			return ""
		}
		return joined
	default:
		text := strings.TrimSpace(body)
		tt := strings.TrimSpace(title)
		if text == "" || text == tt {
			return ""
		}
		if strings.HasPrefix(text, tt) {
			rest := strings.TrimSpace(strings.TrimPrefix(text, tt))
			if rest == "" {
				return ""
			}
			text = rest
		}
		return strings.Join(strings.Fields(text), " ")
	}
}

func (d *viewState) historyCommands(c *ui.Context, k desktopkit.Kit) {
	v := d.current()
	_, command := k.ToolbarIconMenu(c.Key("send"), desktopkit.ToolbarIconMenuProps{
		Label:    d.tr("发送到设备"),
		Open:     &d.historySendMenu,
		Icon:     desktopkit.IconSend,
		Disabled: v == nil,
		Items:    d.sendMenuItems(),
	})
	if command != "" {
		d.sendTo(command)
	}
	if k.ToolbarIconButton(c.Key("pin"), desktopkit.ToolbarIconButtonProps{
		Icon:        desktopkit.IconPin,
		Label:       d.tr("切换收藏"),
		Active:      v != nil && v.pinned,
		ActiveColor: k.Color("status-orange-text"),
		Disabled:    v == nil || d.live != nil && d.live.showBusy(),
	}).Clicked() {
		d.togglePin()
	}
	if d.live != nil && v != nil {
		if actions := d.historyActionsFor(v.id, plugin.ActionPlacementHistoryToolbar, ""); len(actions) > 0 {
			if d.live.busy {
				d.historyActionsMenu = false
			}
			_, key := k.ToolbarIconMenu(c.Key("history-actions"), desktopkit.ToolbarIconMenuProps{
				Label:    d.tr("动作"),
				Open:     &d.historyActionsMenu,
				Icon:     desktopkit.IconEdit,
				Disabled: d.live.showBusy(),
				Items:    actions,
			})
			if key != "" && !d.live.busy {
				d.runHistoryAction(v.id, key)
			}
		} else {
			d.historyActionsMenu = false
		}
	} else {
		d.historyActionsMenu = false
	}
	_, command = k.ToolbarIconMenu(c.Key("more"), desktopkit.ToolbarIconMenuProps{
		Label: d.tr("更多操作"),
		Icon:  desktopkit.IconMore,
		Items: []desktopkit.MenuEntry{
			{ID: "tags", Label: d.tr("管理标签"), Icon: desktopkit.IconTag},
			{ID: "delete", Label: d.tr("删除"), Icon: desktopkit.IconDelete, Destructive: true, Disabled: v == nil},
			desktopkit.MenuSeparator("sep"),
			{ID: "quick-panel", Label: d.tr("查看快捷面板")},
		},
	})
	switch command {
	case "tags":
		d.tagsOpen = true
	case "delete":
		if v != nil {
			d.requestDelete(d.list.Selection())
		}
	case "quick-panel":
		if d.openQuickPanel != nil {
			d.openQuickPanel()
		} else {
			d.page = command
		}
	}
}

func (d *viewState) historyList(c *ui.Context, k desktopkit.Kit, items []entry) {
	groups := []desktopkit.ListGroup{}
	for _, v := range items {
		day := strings.Fields(v.date)[0]
		if len(groups) == 0 || groups[len(groups)-1].Title != day {
			groups = append(groups, desktopkit.ListGroup{ID: day, Title: day})
		}
		groups[len(groups)-1].Count++
	}
	for i := range groups {
		if d.live != nil {
			groups[i].Trailing = fmt.Sprintf(d.tr("%d 项"), groups[i].Count)
			continue
		}
		groups[i].Trailing = fmt.Sprintf(d.tr("%d 项"), groups[i].Count)
		if groups[i].Title == "今天" {
			groups[i].Detail = "11月9日 周一"
		} else {
			groups[i].Detail = "11月8日 周日"
		}
	}
	for i := range groups {
		groups[i].Title = d.tr(groups[i].Title)
	}
	if d.layout == "grid" {
		cards := make([]desktopkit.GridItem, 0, len(items))
		for _, v := range items {
			entryItem := v
			shortTime := strings.TrimPrefix(strings.TrimPrefix(v.date, "今天 "), "昨天 ")
			meta := d.sourceName(v.source)
			if v.size != "" && v.kind != desktopkit.KindImage {
				meta += " · " + v.size
			}
			var cardTags []desktopkit.GridTag
			for _, name := range v.tags {
				if tag := d.tagByID(name); tag != nil && tag.builtin {
					continue
				}
				cardTags = append(cardTags, desktopkit.GridTag{Label: d.tagLabel(name), Color: d.tagColor(name)})
			}
			card := desktopkit.GridItem{
				ID:            v.id,
				Title:         d.entryTitle(v),
				Subtitle:      d.sourceName(v.source) + " · " + d.displayDate(v.date),
				Kind:          v.kind,
				KindLabel:     d.tr(kindName(v.kind)),
				Text:          entryBodyText(v),
				Time:          shortTime,
				Meta:          meta,
				SourceAppName: v.sourceAppName,
				SourceAppIcon: v.sourceAppIcon,
				Pinned:        v.pinned,
				Tags:          cardTags,
				Highlight:     d.historyHighlightTerms(v),
				CopyLabel:     d.tr("复制"),
				PinLabel:      d.tr("切换收藏"),
				OnCopy: func() {
					d.selected = entryItem.id
					d.list.Select(entryItem.id)
					d.useEntry()
				},
				OnPin: func() {
					d.toggleFavorites([]string{entryItem.id})
				},
			}
			if v.kind == desktopkit.KindImage || (v.kind == desktopkit.KindFile && d.expectImagePreview(entryItem)) {
				if img := d.entryImage(entryItem); img.source != nil {
					card.Media = func() { img.element(c).FillWidth().Grow(1).Fit(ui.Cover) }
				} else if d.live != nil && (v.kind == desktopkit.KindImage || !d.detailCached(entryItem.id)) {
					card.ImageLoading = true
				}
			}
			cards = append(cards, card)
		}
		k.GroupedGrid(c.Key("grid"), &d.list, desktopkit.GroupedGridProps{Label: d.tr("历史网格"), Groups: groups, Items: cards, Size: float32(d.gridSize), Menu: d.historyItemMenu}).Grow(1).MinHeight(0)
	} else {
		rows := make([]desktopkit.ItemListItem, 0, len(items))
		for _, v := range items {
			entryItem := v
			title := d.entryTitle(v)
			shortTime := strings.TrimPrefix(strings.TrimPrefix(v.date, "今天 "), "昨天 ")
			meta := d.sourceName(v.source)
			if v.sourceAppName != "" {
				meta = v.sourceAppName + " · " + meta
			} else if v.size != "" {
				meta += " · " + v.size
			}
			var rowTags []desktopkit.GridTag
			for _, name := range v.tags {
				if tag := d.tagByID(name); tag != nil && tag.builtin {
					continue
				}
				rowTags = append(rowTags, desktopkit.GridTag{Label: d.tagLabel(name), Color: d.tagColor(name)})
			}
			row := desktopkit.ItemListItem{
				ID:            v.id,
				Title:         title,
				Subtitle:      d.sourceName(v.source) + " · " + shortTime,
				Highlight:     d.historyHighlightTerms(v),
				CardRow:       true,
				Excerpt:       listRowExcerpt(v, title),
				ExcerptKind:   v.kind,
				Time:          shortTime,
				Meta:          meta,
				SourceAppName: v.sourceAppName,
				SourceAppIcon: v.sourceAppIcon,
				Kind:          v.kind,
				KindLabel:     d.tr(kindName(v.kind)),
				Pinned:        v.pinned,
				Tags:          rowTags,
				CopyLabel:     d.tr("复制"),
				OnCopy: func() {
					d.selected = entryItem.id
					d.list.Select(entryItem.id)
					d.useEntry()
				},
			}
			if v.kind == desktopkit.KindImage || (v.kind == desktopkit.KindFile && d.expectImagePreview(entryItem)) {
				img := d.entryImage(entryItem)
				row.Image, row.ImageDrawOver = img.source, img.drawOver()
				row.ImageLoading = row.Image == nil && d.live != nil && (v.kind == desktopkit.KindImage || !d.detailCached(entryItem.id))
			}
			rows = append(rows, row)
		}
		k.GroupedList(c.Key("list"), &d.list, desktopkit.GroupedListProps{Label: d.tr("历史条目"), Groups: groups, Items: rows, Menu: d.historyItemMenu}).Grow(1).MinHeight(0)
	}
	if d.live != nil {
		first, last := d.list.VisibleRange()
		d.live.prefetchDetails(d, items, first, last)
		d.live.browseRange(d, first, last, len(items))
	}
	if d.list.Changed() {
		if ids := d.list.Selection(); len(ids) > 0 {
			d.selected, d.notice, d.detailOpen, d.inspector = ids[0], "", true, true
		}
	} else if id, ok := d.list.Clicked(); ok && d.layout == "grid" {
		if ids := d.list.Selection(); len(ids) == 1 {
			d.selected, d.notice, d.detailOpen, d.inspector = id, "", true, true
		}
	}
	if id, ok := d.list.Activated(); ok {
		d.selected = id
		d.useEntry()
		d.detailOpen, d.inspector = true, true
	}
	if cmd, ids, ok := d.list.MenuChosen(); ok && len(ids) > 0 {
		d.selected = ids[0]
		switch {
		case cmd == "copy":
			d.useEntry()
		case cmd == "pin":
			d.toggleFavorites(ids)
		case strings.HasPrefix(cmd, "send:"):
			d.sendTo(strings.TrimPrefix(cmd, "send:"))
		case strings.HasPrefix(cmd, "action:"):
			if len(ids) == 1 {
				d.runHistoryAction(ids[0], strings.TrimPrefix(cmd, "action:"))
			}
		case cmd == "delete":
			d.requestDelete(ids)
		}
	}
	if d.live != nil {
		d.live.loadDetail(d)
	}
	if ids, ok := d.list.DeleteRequested(); ok && len(ids) > 0 {
		d.requestDelete(ids)
	}
}

func (d *viewState) actionPluginLabel(pluginID string) string {
	for _, entry := range builtinPluginEntries {
		if entry.id == pluginID {
			return d.tr(entry.name)
		}
	}
	return pluginID
}

func (d *viewState) historyActionsFor(id, placement, idPrefix string) []desktopkit.MenuEntry {
	if d.live == nil {
		return nil
	}
	item, ok := d.live.items[id]
	if !ok {
		return nil
	}
	var actions []desktopkit.MenuEntry
	for _, action := range d.live.actions {
		if slices.Contains(action.Accepts, item.Kind) && slices.Contains(action.Placements, placement) {
			actions = append(actions, desktopkit.MenuEntry{
				ID:       idPrefix + action.Key,
				Label:    d.tr(action.Title),
				Subtitle: d.actionPluginLabel(action.PluginID),
				Disabled: d.live.busy,
			})
		}
	}
	return actions
}

func (d *viewState) historyItemMenu(ids []string) []desktopkit.MenuEntry {
	pinLabel := d.tr("收藏")
	if d.allFavorited(ids) {
		pinLabel = d.tr("取消收藏")
	}
	rawSend := d.sendMenuItems()
	sendChildren := make([]desktopkit.MenuEntry, len(rawSend))
	for i, it := range rawSend {
		if !it.Separator {
			it.ID = "send:" + it.ID
		}
		sendChildren[i] = it
	}
	sendDisabled := len(ids) != 1 || len(sendChildren) == 0 || sendChildren[0].Disabled
	entries := []desktopkit.MenuEntry{
		{ID: "copy", Label: d.tr("复制"), Icon: desktopkit.IconCopy, Disabled: len(ids) != 1},
		{ID: "pin", Label: pinLabel, Icon: desktopkit.IconPin, Disabled: len(ids) == 0 || d.live != nil && d.live.busy},
		{ID: "send", Label: d.tr("发送到"), Icon: desktopkit.IconSend, Disabled: sendDisabled, Children: sendChildren},
	}
	if len(ids) == 1 {
		if actionChildren := d.historyActionsFor(ids[0], plugin.ActionPlacementHistoryItemMenu, "action:"); len(actionChildren) > 0 {
			entries = append(entries, desktopkit.MenuEntry{
				ID:       "actions",
				Label:    d.tr("动作"),
				Icon:     desktopkit.IconEdit,
				Disabled: d.live != nil && d.live.busy,
				Children: actionChildren,
			})
		}
	}
	entries = append(entries,
		desktopkit.MenuSeparator("sep"),
		desktopkit.MenuEntry{ID: "delete", Label: d.tr("删除"), Icon: desktopkit.IconDelete, Destructive: true, Disabled: len(ids) == 0},
	)
	return entries
}

// sourceApplication shows the stored origin application's identity, independent
// of the source device and the clipboard content preview.
func (d *viewState) sourceApplication(c *ui.Context, k desktopkit.Kit, v entry) {
	if v.sourceAppName == "" {
		return
	}
	tooltip := v.sourceAppName
	if v.sourceAppID != "" && v.sourceAppID != v.sourceAppName {
		tooltip += "\n" + v.sourceAppID
	}
	ui.Row(c).Gap(5).AlignItems(ui.Center).MinWidth(0).MaxWidth(180).Shrink(1).
		Role(ui.RoleGroup).Label(d.tr("来源应用")).Tooltip(tooltip).Children(func() {
		if v.sourceAppIcon != nil {
			ui.Image(c.Key("icon"), v.sourceAppIcon).Size(16, 16).Shrink(0).Fit(ui.Contain).Role(ui.RoleImage).Label(v.sourceAppName)
		} else {
			ui.Box(c.Key("icon")).Size(16, 16).Shrink(0).Role(ui.RoleImage).Label(v.sourceAppName).
				TextColor(k.Color("text-secondary")).Children(func() { k.Icon(c, desktopkit.IconPackage, 16) })
		}
		k.Text(c.Key("name"), v.sourceAppName, desktopkit.TextProps{Role: desktopkit.RoleCallout, Tone: desktopkit.ToneSecondary, Truncate: true}).MinWidth(0).Shrink(1)
	})
}

func (d *viewState) detail(c *ui.Context, k desktopkit.Kit) {
	if ids := d.list.Selection(); len(ids) > 1 {
		k.ContentUnavailable(c, desktopkit.ContentUnavailableProps{Icon: desktopkit.IconCopy, Title: fmt.Sprintf(d.tr("已选择 %d 项"), len(ids)), Description: d.tr("通过工具栏置顶或删除所选历史。")}, nil)
		return
	}
	v := d.current()
	if v == nil {
		k.ContentUnavailable(c, desktopkit.ContentUnavailableProps{Title: d.tr("未选择历史"), Description: d.tr("选择一条历史以查看内容。")}, nil)
		return
	}
	border := k.Color("separator-border")
	detailStage := ui.Column(c.Key("detail-s1-stage")).Grow(1).MinHeight(0).Clip()
	dsw := ui.Local(detailStage, "detail-s1-state", func() layoutSwitchState { return layoutSwitchState{} })
	if !dsw.initialized {
		detailStage.Jump("ds1-p", 1)
		dsw.initialized = true
		dsw.lastLayout = v.id
	} else if v.id != dsw.lastLayout {
		if dsw.lastLayout != "" && !c.Preferences().ReduceMotion {
			detailStage.Jump("ds1-p", 0)
		} else {
			detailStage.Jump("ds1-p", 1)
		}
		dsw.lastLayout = v.id
	}
	ds1Dur := 180 * time.Millisecond
	if c.Preferences().ReduceMotion {
		ds1Dur = 0
		detailStage.Jump("ds1-p", 1)
	}
	ds1P := detailStage.AnimateTo("ds1-p", 1, ui.Animation{Duration: ds1Dur, Ease: ui.CubicBezier(.22, 1, .36, 1)})
	detailStaleTarget := float32(1)
	if d.live != nil && d.live.showForegroundLoading() {
		detailStaleTarget = 0.78
	}
	detailStaleA := detailStage.AnimateTo("detail-stale-a", detailStaleTarget, ui.Animation{
		Duration:  140 * time.Millisecond,
		Ease:      ui.EaseOut,
		NonMotion: true,
	})
	if detailStaleA < 0.999 {
		detailStage.Opacity(detailStaleA)
	}
	highlightTerms := d.historyHighlightTerms(*v)
	detailStage.Children(func() {
		detailScroll := k.Scroll(c.Key(v.id)).Grow(1).MinHeight(0)
		if ds1P < 0.999 {
			detailScroll.Translate(14*(1-ds1P), 0).Opacity(0.45 + 0.55*ds1P)
		}
		detailScroll.Children(func() {
			ui.Column(c).Padding(4, 24, 88).Gap(16).Children(func() {
				// Main-toolbar actions own pin/delete on every platform.
				k.DetailHeader(c, desktopkit.DetailHeaderProps{}, func() {
					k.KindBadge(c, desktopkit.KindBadgeProps{Kind: v.kind, Label: d.tr(kindName(v.kind)), Variant: desktopkit.KindPill})
					ui.Row(c.Key("provenance")).Grow(1).MinWidth(0).Gap(8).AlignItems(ui.Center).Children(func() {
						stamp := k.Text(c.Key("device-time"), d.sourceName(v.source)+" · "+d.displayDate(v.date), desktopkit.TextProps{Role: desktopkit.RoleCallout, Tone: desktopkit.ToneSecondary, Truncate: true}).MinWidth(0)
						if v.sourceAppName == "" {
							stamp.Grow(1)
						} else {
							stamp.Shrink(0).MaxWidthPercent(65)
						}
						d.sourceApplication(c.Key("source-application"), k, *v)
					})
				})
				kind := desktopkit.PreviewPlain
				if v.kind == desktopkit.KindCode {
					kind = desktopkit.PreviewCode
				}
				if v.kind == desktopkit.KindFile || v.kind == desktopkit.KindFolder {
					kind = desktopkit.PreviewFile
				}
				if d.expectImagePreview(*v) && (v.image != nil || d.live == nil || d.live.detailBusy) {
					kind = desktopkit.PreviewImage
				}
				loadingPreviewText, useLoadingPreviewText := d.previewTextWhileLoading(*v)
				linkBody := v.text
				if linkBody == "" && useLoadingPreviewText {
					linkBody = loadingPreviewText
				}
				if v.kind == desktopkit.KindLink && v.rich == nil && v.image == nil && (d.live == nil || (v.contentError == "" && (!d.live.detailBusy || useLoadingPreviewText))) {
					host, _ := urlHostAndPath(linkBody)
					ui.Column(c.Key("link-preview")).Padding(14, 16).Radius(12).Gap(10).
						Background(k.SubtleCardSurface()).Border(1, border).Shrink(0).Children(func() {
						if host != "" {
							bg, fg := k.TagTint("blue")
							ui.Row(c.Key("host")).Gap(6).AlignItems(ui.Center).Children(func() {
								ui.Box(c.Key("ico")).Size(18, 18).Radius(5).Background(bg).Center().Shrink(0).Children(func() {
									k.Icon(c, desktopkit.IconLink, 11).TextColor(fg)
								})
								k.HighlightedText(c.Key("h"), host, highlightTerms, desktopkit.HighlightTextProps{
									FontSize:   12,
									FontWeight: 600,
									TextColor:  fg,
									SingleLine: true,
								})
							})
						}
						if title := d.entryTitle(*v); title != "" && title != strings.TrimSpace(linkBody) {
							k.HighlightedText(c.Key("t"), title, highlightTerms, desktopkit.HighlightTextProps{
								FontSize:   15,
								FontWeight: 600,
								TextColor:  k.Color("text-primary"),
							})
						}
						ui.Box(c.Key("url-box")).Padding(9, 11).Radius(8).
							Background(k.DrawerInnerSurface()).Border(1, border).Children(func() {
							k.HighlightedText(c, strings.TrimSpace(linkBody), highlightTerms, desktopkit.HighlightTextProps{
								FontSize:  11.5,
								TextColor: k.Color("accent-500"),
								Monospace: true,
							})
						})
					})
				} else {
					p := desktopkit.PreviewFrameProps{Kind: kind}
					if kind == desktopkit.PreviewImage {
						if v.kind == desktopkit.KindFile {
							capText := v.text
							if capText == "" {
								capText = strings.TrimSpace(v.preview)
							}
							p.Caption = capText
						}
						if v.image != nil || d.live == nil {
							p.BottomStart = func() {
								if k.Button(c, d.tr("快速查看"), desktopkit.ButtonProps{Size: desktopkit.SizeSmall}).Clicked() {
									d.quickLook = true
								}
							}
						}
					}
					k.PreviewFrame(c, p, func() {
						if d.live != nil && v.contentError != "" && !d.live.detailBusy {
							k.Text(c, d.tr(v.contentError), secondary).Padding(20, 24)
							return
						}
						if kind == desktopkit.PreviewImage {
							d.renderDetailImagePreview(c, k, *v, 220)
							return
						}
						if d.live != nil && d.live.detailBusy {
							if useLoadingPreviewText {
								isCode := v.kind == desktopkit.KindCode
								textCol := k.Color("text-primary")
								if isCode {
									textCol = ui.RGBA(235, 235, 240, 1)
								}
								ui.Column(c.Key("loading-preview")).FillWidth().Children(func() {
									k.HighlightedText(c.Key("txt"), loadingPreviewText, highlightTerms, desktopkit.HighlightTextProps{
										FontSize:  13,
										TextColor: textCol,
										Monospace: isCode,
										OnDark:    isCode,
									}).Padding(24).FillWidth()
									if d.live.showDetailLoading() && len([]rune(loadingPreviewText)) >= 80 {
										d.renderStreamingTailSkeleton(c, k, 24, isCode)
									}
								})
								return
							}
							if v.kind == desktopkit.KindFile && strings.TrimSpace(v.preview) != "" {
								for i, name := range strings.Split(strings.TrimSpace(v.preview), "\n") {
									k.SettingsRow(c.Key(i), desktopkit.SettingsRowProps{Label: name, Description: d.tr("已接收")}, nil, func() { k.Icon(c, desktopkit.IconFile, 18) })
								}
								return
							}
							d.renderDetailLoadingSkeleton(c, k, *v, 20, 24)
							return
						}

						if v.rich != nil {
							richtext.EditorBase(c, v.rich, richtext.EditorOptions{Label: d.tr("富文本预览"), ReadOnly: true}).FillWidth().Height(240)
							return
						}
						if v.kind == desktopkit.KindFolder {
							d.folderPreview(c, k, v, secondary)
							return
						}
						if v.kind == desktopkit.KindFile {
							for i, name := range strings.Split(v.text, "\n") {
								k.SettingsRow(c.Key(i), desktopkit.SettingsRowProps{Label: name, Description: d.tr("已接收")}, nil, func() { k.Icon(c, desktopkit.IconFile, 18) })
							}
							return
						}
						isCode := v.kind == desktopkit.KindCode
						textCol := k.Color("text-primary")
						if isCode {
							textCol = ui.RGBA(235, 235, 240, 1)
						}
						k.HighlightedText(c, v.text, highlightTerms, desktopkit.HighlightTextProps{
							FontSize:  13,
							TextColor: textCol,
							Monospace: isCode,
							OnDark:    isCode,
						}).Padding(24).FillWidth()
					})
				}
				// 3-column compact facts strip matching the D1-b drawer.
				sizeVal := v.size
				if sizeVal == "" {
					sizeVal = fmt.Sprintf("%d", len([]rune(v.text)))
				}
				ui.Row(c.Key("facts")).Gap(8).AlignItems(ui.Stretch).Children(func() {
					factCard := func(key, label, value string) {
						ui.Column(c.Key(key)).Grow(1).Basis(0).MinWidth(0).Padding(8, 10).Radius(9).Gap(2).
							Background(k.SubtleCardSurface()).Border(1, border).Children(func() {
							ui.Text(c.Key("k"), label).FontSize(10.5).TextColor(k.Color("text-tertiary")).SingleLine()
							ui.Text(c.Key("v"), value).FontSize(12).FontWeight(600).TextColor(k.Color("text-primary")).SingleLine()
						})
					}
					factCard("kind", d.tr("内容类型"), d.tr(kindName(v.kind)))
					factCard("spec", d.tr("规格"), sizeVal)
					factCard("dev", d.tr("来源设备"), d.sourceName(v.source))
				})
				if kind == desktopkit.PreviewImage {
					facts := []desktopkit.Fact{{ID: "size", Label: d.tr("尺寸"), Value: "1600 × 1000"}, {ID: "format", Label: d.tr("格式"), Value: "PNG · 286 KB"}}
					if d.live != nil {
						facts = nil
						if v.image != nil {
							w, h := imageDimensions(*v)
							facts = []desktopkit.Fact{{ID: "size", Label: d.tr("尺寸"), Value: fmt.Sprintf("%.0f × %.0f", w, h)}, {ID: "bytes", Label: d.tr("大小"), Value: v.size}}
						} else if d.live.detailBusy {
							facts = []desktopkit.Fact{{ID: "size", Label: d.tr("尺寸"), Value: "—"}, {ID: "bytes", Label: d.tr("大小"), Value: v.size}}
						}
					}
					if len(facts) > 0 {
						k.FactList(c, desktopkit.FactListProps{Label: d.tr("图片信息"), Facts: facts})
					}
				}
				d.detailTags(c.Key("detail-tags"), k, v)
				ui.Column(c).Gap(6).Children(func() {
					if d.live != nil {
						k.Text(c, d.tr("同步状态：")+d.tr(syncStateLabel(v.syncState)), secondary)
						for _, backend := range v.backendSync {
							text := backend.BackendID + " · " + d.tr(syncStateLabel(backend.State)) + " · " + d.tr(backend.Disposition)
							if backend.ErrorCode != "" {
								text += " · " + backend.ErrorCode
							}
							k.Text(c, text, secondary)
						}
						return
					}
					rows := []desktopkit.InsetListRow{}
					for _, peer := range d.devices {
						if peer.id == "local" {
							continue
						}
						rows = append(rows, desktopkit.InsetListRow{ID: peer.id, Title: peer.name, Status: func() { k.Icon(c, desktopkit.IconInfo, 16).TextColor(k.Color("text-secondary")) }})
					}
					ui.Row(c).Gap(8).AlignItems(ui.Center).Children(func() {
						k.Text(c, d.tr("送达"), desktopkit.TextProps{Emphasized: true})
						k.Text(c, fmt.Sprintf(d.tr("0/%d 台设备"), len(rows)), desktopkit.TextProps{Role: desktopkit.RoleCallout, Tone: desktopkit.ToneSecondary})
					})
					k.InsetList(c, desktopkit.InsetListProps{Label: d.tr("送达情况"), Rows: rows})
					k.Text(c, d.tr("本地加密存储 · 设备之间端到端加密"), desktopkit.TextProps{Role: desktopkit.RoleCaption, Tone: desktopkit.ToneTertiary})
				})
			})
		})
	})
	k.ActionBar(c, desktopkit.ActionBarProps{
		Status:   d.actionBarStatus(),
		Floating: true,
	}, func() {
		if k.ActionDockPrimary(c.Key("copy"), d.tr("复制"), desktopkit.IconCopy, "↩").Clicked() {
			d.useEntry()
		}
		k.ActionDockSeparator(c.Key("sep"))
		_, target := k.ActionDockMenu(c.Key("send-target"), desktopkit.MenuProps{
			Label: d.tr("发送到…"),
			Icon:  desktopkit.IconSend,
			Items: d.sendMenuItems(),
		})
		if target != "" {
			d.sendTo(target)
		}
	})
}

type drawerSwitchState struct {
	initialized bool
	lastID      string
	lastIdx     int
	lastEntry   entry
	prevEntry   *entry
	prevIdx     int
	dir         float32
}

// gridDetailDrawer renders the D1-b floating inset full-height inspector card
// (8-DIP window margin, 12-DIP radius) with M5 enter/exit depth motion owned by
// SplitView and S1 directional micro-slide transitions when switching cards.
func (d *viewState) gridDetailDrawer(c *ui.Context, k desktopkit.Kit, items []entry) {
	v := d.current()
	if v == nil {
		return
	}
	curIdx := slices.IndexFunc(items, func(e entry) bool { return e.id == v.id })
	step := func(delta int) {
		if len(items) == 0 || curIdx < 0 {
			return
		}
		if delta >= 0 {
			d.drawerSwitchDir = 1
		} else {
			d.drawerSwitchDir = -1
		}
		next := (curIdx + delta + len(items)) % len(items)
		d.selectEntry(items[next].id)
		d.notice = ""
		d.list.ScrollTo(items[next].id)
	}

	root := ui.Column(c.Key("drawer-root")).Grow(1).MinHeight(0).AlignItems(ui.Stretch)
	sw := ui.Local(root, "drawer-switch-s1", func() drawerSwitchState { return drawerSwitchState{} })
	if !sw.initialized {
		root.Jump("s1-p", 1)
		sw.initialized = true
		sw.lastID = v.id
		sw.lastIdx = curIdx
		sw.lastEntry = *v
	} else if v.id != sw.lastID {
		if sw.lastID != "" && d.inspector && !c.Preferences().ReduceMotion {
			prev := sw.lastEntry
			sw.prevEntry = &prev
			sw.prevIdx = sw.lastIdx
			switch {
			case d.drawerSwitchDir != 0:
				sw.dir = float32(d.drawerSwitchDir)
			case curIdx >= 0 && sw.lastIdx >= 0 && curIdx < sw.lastIdx:
				sw.dir = -1
			default:
				sw.dir = 1
			}
			root.Jump("s1-p", 0)
		} else {
			sw.prevEntry = nil
			root.Jump("s1-p", 1)
		}
		d.drawerSwitchDir = 0
		sw.lastID = v.id
		sw.lastIdx = curIdx
		sw.lastEntry = *v
	} else {
		sw.lastEntry = *v
		sw.lastIdx = curIdx
	}

	swDur := 210 * time.Millisecond
	if c.Preferences().ReduceMotion {
		swDur = 0
		root.Jump("s1-p", 1)
	}
	swP := root.AnimateTo("s1-p", 1, ui.Animation{Duration: swDur, Ease: ui.CubicBezier(.22, 1, .36, 1)})
	if swP >= 0.999 && sw.prevEntry != nil {
		sw.prevEntry = nil
	}
	outP := min(float32(1), swP/0.67)

	const headerH = float32(48)
	border := k.Color("separator-border")

	renderTitle := func(key string, item *entry) ui.Element {
		row := ui.Row(c.Key(key)).Absolute().Top(0).Left(0).Right(0).Bottom(0).Gap(8).AlignItems(ui.Center)
		row.Children(func() {
			if item.sourceAppName != "" {
				if item.sourceAppIcon != nil {
					ui.Image(c.Key("app-icon"), item.sourceAppIcon).Size(22, 22).Radius(5.5).Fit(ui.Contain).Shrink(0).Role(ui.RoleImage).Label(item.sourceAppName)
				} else {
					ui.Box(c.Key("app-fallback")).Size(22, 22).Radius(5.5).Background(k.Color("background-secondary-default")).
						Border(1, border).Center().Shrink(0).Children(func() {
						k.Icon(c, desktopkit.IconPackage, 13).TextColor(k.Color("text-secondary"))
					})
				}
				ui.Text(c.Key("app-name"), item.sourceAppName).FontSize(13.5).FontWeight(600).
					TextColor(k.Color("text-primary")).SingleLine().MinWidth(0).Shrink(1)
			} else {
				ui.Text(c.Key("entry-title"), d.entryTitle(*item)).FontSize(13.5).FontWeight(600).
					TextColor(k.Color("text-primary")).SingleLine().MinWidth(0).Shrink(1)
			}
			k.KindBadge(c.Key("kind"), desktopkit.KindBadgeProps{Kind: item.kind, Label: d.tr(kindName(item.kind)), Variant: desktopkit.KindPill})
		})
		return row
	}

	renderBody := func(key string, item *entry, idx int, interactive bool) ui.Element {
		layer := k.Scroll(c.Key(key)).Grow(1).MinHeight(0)
		drawerStaleTarget := float32(1)
		if d.live != nil && d.live.showForegroundLoading() {
			drawerStaleTarget = 0.78
		}
		drawerStaleA := layer.AnimateTo("drawer-stale-a", drawerStaleTarget, ui.Animation{
			Duration:  140 * time.Millisecond,
			Ease:      ui.EaseOut,
			NonMotion: true,
		})
		if drawerStaleA < 0.999 {
			layer.Opacity(drawerStaleA)
		}
		drawerHighlights := d.historyHighlightTerms(*item)
		layer.Children(func() {
			ui.Column(c).Padding(14, 18, 88).Gap(14).Children(func() {
				// Subtle sub-row: source device · timestamp on left, index counter on right.
				ui.Row(c.Key("sub-meta")).AlignItems(ui.Center).Gap(8).Shrink(0).Children(func() {
					ui.Text(c.Key("sub"), d.sourceName(item.source)+" · "+d.displayDate(item.date)).
						FontSize(12).TextColor(k.Color("text-tertiary")).SingleLine().Grow(1).MinWidth(0)
					if idx >= 0 && len(items) > 0 {
						ui.Text(c.Key("counter"), fmt.Sprintf("%d / %d", idx+1, len(items))).
							FontSize(11.5).TextColor(k.Color("text-tertiary")).SingleLine().Shrink(0)
					}
				})

				// Semantic content preview.
				drawerLoadingText, useDrawerLoadingText := d.previewTextWhileLoading(*item)
				drawerLinkBody := item.text
				if drawerLinkBody == "" && useDrawerLoadingText {
					drawerLinkBody = drawerLoadingText
				}
				if item.kind == desktopkit.KindLink && item.rich == nil && item.image == nil && (d.live == nil || (item.contentError == "" && (!d.live.detailBusy || useDrawerLoadingText))) {
					host, _ := urlHostAndPath(drawerLinkBody)
					ui.Column(c.Key("link-preview")).Padding(14, 16).Radius(12).Gap(10).
						Background(k.DrawerInnerSurface()).Border(1, border).Shrink(0).Children(func() {
						if host != "" {
							bg, fg := k.TagTint("blue")
							ui.Row(c.Key("host")).Gap(6).AlignItems(ui.Center).Children(func() {
								ui.Box(c.Key("ico")).Size(18, 18).Radius(5).Background(bg).Center().Shrink(0).Children(func() {
									k.Icon(c, desktopkit.IconLink, 11).TextColor(fg)
								})
								k.HighlightedText(c.Key("h"), host, drawerHighlights, desktopkit.HighlightTextProps{
									FontSize:   12,
									FontWeight: 600,
									TextColor:  fg,
									SingleLine: true,
								})
							})
						}
						if title := d.entryTitle(*item); title != "" && title != strings.TrimSpace(drawerLinkBody) {
							k.HighlightedText(c.Key("t"), title, drawerHighlights, desktopkit.HighlightTextProps{
								FontSize:   15,
								FontWeight: 600,
								TextColor:  k.Color("text-primary"),
							})
						}
						ui.Box(c.Key("url-box")).Padding(9, 11).Radius(8).
							Background(k.Color("background-primary-default")).Border(1, border).Children(func() {
							k.HighlightedText(c, strings.TrimSpace(drawerLinkBody), drawerHighlights, desktopkit.HighlightTextProps{
								FontSize:  11.5,
								TextColor: k.Color("accent-500"),
								Monospace: true,
							})
						})
					})
				} else {
					kind := desktopkit.PreviewPlain
					if item.kind == desktopkit.KindCode {
						kind = desktopkit.PreviewCode
					}
					if item.kind == desktopkit.KindFile || item.kind == desktopkit.KindFolder {
						kind = desktopkit.PreviewFile
					}
					if d.expectImagePreview(*item) && (item.image != nil || d.live == nil || d.live.detailBusy) {
						kind = desktopkit.PreviewImage
					}
					p := desktopkit.PreviewFrameProps{Kind: kind}
					if kind == desktopkit.PreviewImage {
						if item.kind == desktopkit.KindFile {
							capText := item.text
							if capText == "" {
								capText = strings.TrimSpace(item.preview)
							}
							p.Caption = capText
						}
						if interactive && (item.image != nil || d.live == nil) {
							p.BottomStart = func() {
								if k.Button(c, d.tr("快速查看"), desktopkit.ButtonProps{Size: desktopkit.SizeSmall}).Clicked() {
									d.quickLook = true
								}
							}
						}
					}
					k.PreviewFrame(c, p, func() {
						if d.live != nil && item.contentError != "" && !d.live.detailBusy {
							k.Text(c, d.tr(item.contentError), secondary).Padding(18, 20)
							return
						}
						if kind == desktopkit.PreviewImage {
							d.renderDetailImagePreview(c, k, *item, 210)
							return
						}
						if d.live != nil && d.live.detailBusy {
							if useDrawerLoadingText {
								isCode := item.kind == desktopkit.KindCode
								textCol := k.Color("text-primary")
								if isCode {
									textCol = ui.RGBA(235, 235, 240, 1)
								}
								ui.Column(c.Key("loading-preview")).FillWidth().Children(func() {
									k.HighlightedText(c.Key("txt"), drawerLoadingText, drawerHighlights, desktopkit.HighlightTextProps{
										FontSize:  13,
										TextColor: textCol,
										Monospace: isCode,
										OnDark:    isCode,
									}).Padding(18).FillWidth()
									if d.live.showDetailLoading() && len([]rune(drawerLoadingText)) >= 80 {
										d.renderStreamingTailSkeleton(c, k, 18, isCode)
									}
								})
								return
							}
							if item.kind == desktopkit.KindFile && strings.TrimSpace(item.preview) != "" {
								for i, name := range strings.Split(strings.TrimSpace(item.preview), "\n") {
									k.SettingsRow(c.Key(i), desktopkit.SettingsRowProps{Label: name, Description: d.tr("已接收")}, nil, func() { k.Icon(c, desktopkit.IconFile, 18) })
								}
								return
							}
							d.renderDetailLoadingSkeleton(c, k, *item, 18, 20)
							return
						}
						if item.rich != nil {
							richtext.EditorBase(c, item.rich, richtext.EditorOptions{Label: d.tr("富文本预览"), ReadOnly: true}).FillWidth().Height(220)
							return
						}
						if item.kind == desktopkit.KindFolder {
							d.folderPreview(c, k, item, secondary)
							return
						}
						if item.kind == desktopkit.KindFile {
							for i, name := range strings.Split(item.text, "\n") {
								k.SettingsRow(c.Key(i), desktopkit.SettingsRowProps{Label: name, Description: d.tr("已接收")}, nil, func() { k.Icon(c, desktopkit.IconFile, 18) })
							}
							return
						}
						isCode := item.kind == desktopkit.KindCode
						textCol := k.Color("text-primary")
						if isCode {
							textCol = ui.RGBA(235, 235, 240, 1)
						}
						k.HighlightedText(c, item.text, drawerHighlights, desktopkit.HighlightTextProps{
							FontSize:  13,
							TextColor: textCol,
							Monospace: isCode,
							OnDark:    isCode,
						}).Padding(18).FillWidth()
					})
				}

				// 3-column compact facts strip.
				sizeVal := item.size
				if sizeVal == "" {
					sizeVal = fmt.Sprintf("%d", len([]rune(item.text)))
				}
				ui.Row(c.Key("facts")).Gap(8).AlignItems(ui.Stretch).Children(func() {
					factCard := func(key, label, value string) {
						ui.Column(c.Key(key)).Grow(1).Basis(0).MinWidth(0).Padding(8, 10).Radius(9).Gap(2).
							Background(k.DrawerInnerSurface()).Border(1, border).Children(func() {
							ui.Text(c.Key("k"), label).FontSize(10.5).TextColor(k.Color("text-tertiary")).SingleLine()
							ui.Text(c.Key("v"), value).FontSize(12).FontWeight(600).TextColor(k.Color("text-primary")).SingleLine()
						})
					}
					factCard("kind", d.tr("内容类型"), d.tr(kindName(item.kind)))
					factCard("spec", d.tr("规格"), sizeVal)
					factCard("dev", d.tr("来源设备"), d.sourceName(item.source))
				})

				// Interactive tags section.
				if interactive {
					d.detailTags(c.Key("drawer-tags"), k, item)
				}

				// Delivery status section.
				ui.Column(c.Key("delivery")).Gap(6).Children(func() {
					if d.live != nil {
						k.Text(c, d.tr("同步状态：")+d.tr(syncStateLabel(item.syncState)), secondary)
						return
					}
					rows := []desktopkit.InsetListRow{}
					for _, peer := range d.devices {
						if peer.id == "local" {
							continue
						}
						rows = append(rows, desktopkit.InsetListRow{ID: peer.id, Title: peer.name, Status: func() { k.Icon(c, desktopkit.IconInfo, 16).TextColor(k.Color("text-secondary")) }})
					}
					ui.Row(c).Gap(8).AlignItems(ui.Center).Children(func() {
						k.Text(c, d.tr("送达"), desktopkit.TextProps{Emphasized: true})
						k.Text(c, fmt.Sprintf(d.tr("0/%d 台设备"), len(rows)), desktopkit.TextProps{Role: desktopkit.RoleCallout, Tone: desktopkit.ToneSecondary})
					})
					k.InsetList(c, desktopkit.InsetListProps{Label: d.tr("送达情况"), Rows: rows})
					k.Text(c, d.tr("本地加密存储 · 设备之间端到端加密"), desktopkit.TextProps{Role: desktopkit.RoleCaption, Tone: desktopkit.ToneTertiary})
				})
			})
		})
		return layer
	}

	root.Children(func() {
		// 48-DIP integrated D1-b top bar: [App icon + Name + KindPill] ... [‹ ›] [Pin] [Del] [×].
		ui.Row(c.Key("drawer-bar")).Height(headerH).Padding(0, 12, 0, 16).Gap(8).AlignItems(ui.Center).Shrink(0).
			Background(k.Color("background-primary-default")).InsetShadow(0, -1, 0, 0, border).Children(func() {
			ui.Box(c.Key("drawer-title-stage")).Grow(1).MinWidth(0).Height(28).Clip().DragWindow().Children(func() {
				if sw.prevEntry != nil && swP < 0.999 {
					renderTitle("title-out-"+sw.prevEntry.id, sw.prevEntry).
						Translate(0, -4*outP).Opacity(1 - outP).PassThrough().AccessHidden(true)
				}
				inTitle := renderTitle("title-in-"+v.id, v)
				if swP < 0.999 {
					inTitle.Translate(0, 4*(1-swP)).Opacity(swP)
				}
			})

			// Trailing controls: compact ‹ › pager pill + Pin + Delete + Close.
			ui.Row(c.Key("drawer-actions")).Gap(3).AlignItems(ui.Center).Shrink(0).Children(func() {
				ui.Row(c.Key("pager")).Padding(2).Gap(1).Radius(7).Margin(0, 3, 0, 0).Background(k.Color("background-tertiary-default")).
					AlignItems(ui.Center).Shrink(0).Children(func() {
					prev := ui.ButtonBase(c.Key("prev")).Size(24, 22).Radius(5).Center().
						Label(d.tr("上一条")).Tooltip(d.tr("上一条")).Disabled(len(items) <= 1)
					prevCol := k.Color("text-secondary")
					if prev.Hovered() && len(items) > 1 {
						prev.Background(k.Color("background-primary-default"))
						prevCol = k.Color("text-primary")
					}
					prev.Children(func() { k.Icon(c, desktopkit.IconBack, 13).TextColor(prevCol) })
					if prev.Clicked() {
						step(-1)
					}

					next := ui.ButtonBase(c.Key("next")).Size(24, 22).Radius(5).Center().
						Label(d.tr("下一条")).Tooltip(d.tr("下一条")).Disabled(len(items) <= 1)
					nextCol := k.Color("text-secondary")
					if next.Hovered() && len(items) > 1 {
						next.Background(k.Color("background-primary-default"))
						nextCol = k.Color("text-primary")
					}
					next.Children(func() { k.Icon(c, desktopkit.IconChevronRight, 13).TextColor(nextCol) })
					if next.Clicked() {
						step(1)
					}
				})

				busy := d.live != nil && d.live.showBusy()
				pinBtn := ui.ButtonBase(c.Key("drawer-pin")).Size(28, 28).Radius(7).Center().
					Label(d.tr("切换收藏")).Tooltip(d.tr("切换收藏")).Disabled(busy)
				pinCol := k.Color("text-secondary")
				if v.pinned {
					pinCol = k.Color("status-orange-text")
				}
				if pinBtn.Hovered() && !busy {
					pinBtn.Background(k.Color("background-secondary-hover"))
					if !v.pinned {
						pinCol = k.Color("text-primary")
					}
				}
				pinBtn.Children(func() { k.Icon(c, desktopkit.IconPin, 13).TextColor(pinCol) })
				if pinBtn.Clicked() {
					d.toggleFavorites([]string{v.id})
				}

				delBtn := ui.ButtonBase(c.Key("drawer-del")).Size(28, 28).Radius(7).Center().
					Label(d.tr("删除")).Tooltip(d.tr("删除"))
				delCol := k.Color("text-secondary")
				if delBtn.Hovered() {
					delBtn.Background(k.Color("text-error-primary").Alpha(.1))
					delCol = k.Color("text-error-primary")
				}
				delBtn.Children(func() { k.Icon(c, desktopkit.IconDelete, 13).TextColor(delCol) })
				if delBtn.Clicked() {
					d.requestDelete([]string{v.id})
				}

				closeBtn := ui.ButtonBase(c.Key("drawer-close")).Size(28, 28).Radius(7).Center().
					Label(d.tr("关闭详情")).Tooltip(d.tr("关闭详情"))
				closeCol := k.Color("text-secondary")
				if closeBtn.Hovered() {
					closeBtn.Background(k.Color("background-secondary-hover"))
					closeCol = k.Color("text-primary")
				}
				closeBtn.Children(func() { k.Icon(c, desktopkit.IconDismiss, 14).TextColor(closeCol) })
				if closeBtn.Clicked() {
					d.inspector = false
				}
			})
		})

		// S1 Directional Micro-Slide body stage: outgoing shifts -16px*dir while incoming enters from +24px*dir.
		ui.Box(c.Key("drawer-body-stage")).Grow(1).MinHeight(0).Clip().Children(func() {
			if sw.prevEntry != nil && swP < 0.999 {
				out := ui.Column(c.Key("body-out-"+sw.prevEntry.id)).Absolute().Top(0).Left(0).Right(0).Bottom(0).
					Translate(-16*sw.dir*outP, 0).Opacity(1 - outP).PassThrough().AccessHidden(true)
				out.Children(func() {
					renderBody("drawer-scroll-out-"+sw.prevEntry.id, sw.prevEntry, sw.prevIdx, false)
				})
			}
			in := ui.Column(c.Key("body-in-" + v.id)).Absolute().Top(0).Left(0).Right(0).Bottom(0)
			if swP < 0.999 {
				in.Translate(24*sw.dir*(1-swP), 0).Opacity(swP)
			}
			in.Children(func() {
				renderBody("drawer-scroll-"+v.id, v, curIdx, true)
			})
		})

		// Anchored floating pill dock (never jumps during card switches).
		k.ActionBar(c.Key("drawer-dock"), desktopkit.ActionBarProps{
			Status:   d.actionBarStatus(),
			Floating: true,
		}, func() {
			if k.ActionDockPrimary(c.Key("copy"), d.tr("复制"), desktopkit.IconCopy, "↩").Clicked() {
				d.useEntry()
			}
			k.ActionDockSeparator(c.Key("sep"))
			_, target := k.ActionDockMenu(c.Key("send-target"), desktopkit.MenuProps{
				Label: d.tr("发送到…"),
				Icon:  desktopkit.IconSend,
				Items: d.sendMenuItems(),
			})
			if target != "" {
				d.sendTo(target)
			}
		})
	})
}

func urlHostAndPath(raw string) (host, path string) {
	u, err := url.Parse(strings.TrimSpace(raw))
	if err != nil || u.Host == "" {
		return "", strings.TrimSpace(raw)
	}
	host = strings.TrimPrefix(u.Host, "www.")
	path = u.RequestURI()
	if path == "/" {
		path = ""
	}
	return host, path
}

// Send menus share the current remote-device registry, including offline peers.
func (d *viewState) sendMenuItems() []desktopkit.MenuEntry {
	items := []desktopkit.MenuEntry{{ID: "all", Label: d.tr("所有设备"), Icon: desktopkit.IconDevices, Disabled: d.live != nil && d.live.sendCancel != nil}}
	for _, peer := range d.devices {
		if peer.id == "local" {
			continue
		}
		if len(items) == 1 {
			items = append(items, desktopkit.MenuSeparator("devices"))
		}
		items = append(items, desktopkit.MenuEntry{ID: peer.id, Label: peer.name, Disabled: d.live != nil && d.live.sendCancel != nil})
	}
	items[0].Disabled = len(items) == 1 || (d.live != nil && d.live.sendCancel != nil)
	return items
}

func (d *viewState) sendTo(target string) {
	if d.live != nil {
		d.startHistorySend(target)
		return
	}
	name := d.sourceName(target)
	if target == "all" {
		name = "所有设备"
	}
	d.notice = "演示：已发送到" + name
}

func (d *viewState) entryImage(v entry) bitmapPreview {
	if d.live != nil {
		return v.bitmapPreview()
	}
	return bitmapPreview{source: d.previewImage}
}
func syncStateLabel(s string) string {
	switch s {
	case "sent":
		return "已发送"
	case "partial":
		return "部分发送"
	case "failed":
		return "发送失败"
	case "pending":
		return "正在发送"
	case "interrupted":
		return "已中断"
	default:
		return "未请求同步"
	}
}

func (d *viewState) historyCountLabel() string {
	if d.live != nil {
		if count, ok := d.live.countFor(allHistoryQuery()); ok {
			return fmt.Sprint(count)
		}
		if d.live.cursor != "" {
			return fmt.Sprintf("%d+", len(d.entries))
		}
		if !d.live.loaded {
			return ""
		}
	}
	return fmt.Sprint(len(d.entries))
}

func (d *viewState) historyHighlightTerms(v entry) []string {
	out := []string{}
	seen := map[string]bool{}
	add := func(term string) {
		t := strings.TrimSpace(term)
		if t == "" {
			return
		}
		low := strings.ToLower(t)
		if seen[low] {
			return
		}
		seen[low] = true
		out = append(out, t)
	}
	if d.live != nil && len(v.highlights) > 0 {
		src := v.preview
		if src == "" {
			src = v.title
		}
		r := []rune(src)
		for _, span := range v.highlights {
			start, end := max(0, span.Start), min(len(r), span.End)
			if start < end {
				add(string(r[start:end]))
			}
		}
	}
	if len(out) == 0 && strings.TrimSpace(d.query) != "" {
		for _, tok := range strings.Fields(d.query) {
			if strings.HasPrefix(tok, "#") || strings.HasPrefix(tok, "@") || strings.HasPrefix(tok, "/") || strings.Contains(tok, ":") {
				continue
			}
			add(strings.Trim(tok, `"'`))
		}
	}
	return out
}

func (d *viewState) startHistorySend(target string) {
	l := d.live
	v := d.current()
	if v == nil || l.sendCancel != nil {
		return
	}
	id := v.id
	request := newRequestID()
	ctx, cancel := context.WithCancel(l.ctx)
	l.sendCancel = cancel
	l.sendStage = "scanning"
	var devices []string
	if target != "all" {
		devices = []string{target}
	}
	go func() {
		defer cancel()
		progress := func(stage string) {
			select {
			case l.results <- func() { l.sendStage = stage }:
				l.wake()
			case <-l.ctx.Done():
			}
		}
		// Await admission independently of user cancellation, so cancellation
		// cannot reach the service before it has registered this request.
		// Cancellation does not guarantee that blocking work has stopped.
		out, err := l.client.Submit(l.ctx, api.SubmitInput{RequestID: request, HistoryID: id, DeviceIDs: devices, Async: true})
		if err == nil && out.Phase == engine.SubmissionProcessing {
			out, err = l.client.WaitSubmission(ctx, request, progress)
		}
		if err == nil && out.OperationID != "" {
			progress("sending")
			for {
				op, e := l.client.Operation(ctx, string(out.OperationID))
				if e != nil {
					err = e
					break
				}
				if op.Done() {
					if op.Outcome != engine.OutcomeSuccess {
						err = fmt.Errorf("send incomplete")
					}
					break
				}
				select {
				case <-ctx.Done():
					err = ctx.Err()
				case <-time.After(150 * time.Millisecond):
				}
				if err != nil {
					break
				}
			}
		}
		if ctx.Err() != nil {
			stop, end := context.WithTimeout(context.Background(), 3*time.Second)
			_ = l.client.CancelSubmission(stop, request)
			end()
		}
		resultErr := err
		select {
		case l.results <- func() {
			l.sendCancel = nil
			l.sendStage = ""
			l.nextRefresh = time.Time{}
			if resultErr != nil {
				l.message = "发送未完成。请检查设备连接和源目录。"
				d.toastError(d.tr(l.message), desktopkit.ToastOptions{ID: toastIDLiveMessage})
			} else if out.SyncDisposition != engine.SyncQueued {
				l.message = "内容已保存，但未发送。请检查同步设置和设备连接。"
				d.toastWarning(d.tr(l.message), desktopkit.ToastOptions{ID: toastIDLiveMessage})
			} else {
				d.notice = "发送完成"
			}
		}:
			l.wake()
		case <-l.ctx.Done():
		}
	}()
}
