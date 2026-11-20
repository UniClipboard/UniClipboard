package desktopui

import (
	"context"
	"fmt"
	"slices"
	"strings"
	"time"

	"github.com/uniclipboard/unic/engine"
	api "github.com/uniclipboard/unic/internal/localapi"

	"github.com/egoist/mygo/ui"
	"github.com/egoist/mygo/ui/richtext"

	"github.com/uniclipboard/unic/app/desktop/internal/desktopkit"
)

func (d *viewState) sidebar(c *ui.Context, k desktopkit.Kit) {
	pinned := 0
	for _, v := range d.entries {
		if v.pinned {
			pinned++
		}
	}
	tags := []desktopkit.SidebarNavItem{}
	for _, tag := range d.tags {
		if !tag.sidebar {
			continue
		}
		tags = append(tags, desktopkit.SidebarNavItem{ID: "tag:" + tag.id, Label: "#" + d.tagLabel(tag.id), Swatch: tag.color, Count: fmt.Sprint(d.tagCount(tag.id))})
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
	k.SidebarNav(c, &d.nav, desktopkit.SidebarNavProps{Label: d.tr("资料库"), Sections: []desktopkit.SidebarNavSection{
		{ID: "library", Items: []desktopkit.SidebarNavItem{{ID: "all", Label: d.tr("全部项目"), Icon: desktopkit.IconHistory, Count: d.historyCountLabel()}, {ID: "pinned", Label: d.tr("已置顶"), Icon: desktopkit.IconPin, Count: fmt.Sprint(pinned)}}},
		{ID: "tags", Title: d.tr("标签"), Items: tags, Action: &desktopkit.NavAction{Label: d.tr("管理标签"), Icon: desktopkit.IconAdd}, More: &desktopkit.NavAction{Label: fmt.Sprintf(d.tr("全部标签 · %d"), len(d.tags))}},
		{ID: "devices", Title: d.tr("设备"), Items: devices},
	}, Footer: []desktopkit.SidebarNavItem{{ID: "manage", Label: d.tr("管理设备…"), Icon: desktopkit.IconDevices}, {ID: "settings", Label: d.tr("设置…"), Icon: desktopkit.IconSettings, ShortcutHint: "⌘,"}}}).Grow(1)
	if cmd, _ := d.nav.Command(); cmd == desktopkit.NavSectionAction || cmd == desktopkit.NavMore {
		d.tagsOpen = true
	}
	if d.nav.Changed() {
		d.notice = ""
		switch {
		case d.nav.Value == "manage":
			d.devicesOpen = true
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
	k.SplitView(c, desktopkit.SplitViewProps{
		Title: d.tr("全部"), Subtitle: fmt.Sprint(len(items)),
		Sidebar: func() { d.sidebar(c, k) }, SidebarHidden: &d.sidebarHidden, SidebarOverlay: d.compact,
		SidebarToggleLabel: d.tr("切换侧栏"), DetailOpen: &d.detailOpen, BackLabel: d.tr("返回历史"),
		Search: func() {
			d.historySearch(c.Key("search"), k)
		},
		ToolbarLeading: func() {
			if d.layout == "grid" && !d.compact {
				k.Slider(c.Key("card-size"), &d.gridSize, desktopkit.SliderProps{Label: d.tr("卡片大小"), Min: 0, Max: 1, Step: .05}).Width(80)
			}
			r := k.SegmentedControl(c.Key("layout"), desktopkit.SegmentedControlProps{Label: d.tr("显示方式"), Value: &d.layout, Toolbar: true, Items: []desktopkit.SegmentedItem{{Value: "list", Label: d.tr("列表"), Icon: desktopkit.IconList, IconOnly: true}, {Value: "grid", Label: d.tr("网格"), Icon: desktopkit.IconCopy, IconOnly: true}}})
			if r.Changed {
				d.inspector, d.detailOpen = false, false
			}
		},
		Toolbar: func() { k.ToolbarGroup(c, func() { d.historyCommands(c, k) }) },
	}, func() {
		pane := desktopkit.PaneList
		if d.layout == "grid" {
			pane = desktopkit.PaneMain
		}
		k.Pane(c.Key("browse"), desktopkit.PaneProps{Kind: pane, Label: d.tr("历史列表"), DetailOpen: &d.detailOpen}, func() {
			d.filters(c, k)
			if d.live != nil {
				if d.live.readError != "" {
					k.InlineNotice(c, desktopkit.NoticeProps{Tone: desktopkit.ToneWarning, Title: d.tr(d.live.readError)})
				}
				if d.live.message != "" {
					k.InlineNotice(c, desktopkit.NoticeProps{Tone: desktopkit.ToneWarning, Title: d.tr(d.live.message)})
				}
				if d.live.foregroundLoading {
					k.Text(c, d.tr("正在读取历史…"), secondary)
				}
				if d.unsupportedSearch() {
					k.InlineNotice(c, desktopkit.NoticeProps{Tone: desktopkit.ToneWarning, Title: d.tr("此搜索条件暂不可用"), Text: d.tr("请选择类型、设备、标签或时间筛选。")})
				}
			}
			if len(items) == 0 {
				k.ContentUnavailable(c, desktopkit.ContentUnavailableProps{Icon: desktopkit.IconSearch, Title: d.tr("没有匹配的历史"), Description: d.tr("修改搜索词或清除筛选。")}, func() {
					if k.Button(c, d.tr("清除筛选"), desktopkit.ButtonProps{}).Clicked() {
						d.query, d.kind, d.source, d.tag, d.timeRange, d.nav.Value = "", "", "", "", "", "all"
					}
				})
			} else {
				d.historyList(c, k, items)
			}
		})
		if d.layout == "list" || d.inspector || (d.compact && d.detailOpen) {
			k.Pane(c.Key("detail"), desktopkit.PaneProps{Kind: desktopkit.PaneDetail, Label: d.tr("历史详情"), DetailOpen: &d.detailOpen}, func() { d.detail(c, k) })
		}
	})
	if d.quickLook && d.current() != nil {
		copyLabel := "复制图片"
		if d.current().kind == desktopkit.KindFile {
			copyLabel = "复制文件"
		}
		r := k.QuickLook(c.Key("quick-look"), desktopkit.QuickLookProps{Open: &d.quickLook, Title: d.entryTitle(*d.current()), CloseLabel: d.tr("关闭预览"), Src: d.entryImage(c, *d.current()), Alt: d.current().title, PrimaryLabel: d.tr(copyLabel)}, nil)
		if r.Primary {
			d.useEntry()
		}
	}
}

func (d *viewState) filters(c *ui.Context, k desktopkit.Kit) {
	ui.Row(c.Key("filters")).Padding(5, 16, 5).Gap(6).Wrap().Shrink(0).Children(func() {
		k.Select(c.Key("kind"), desktopkit.SelectProps{Label: d.tr("内容类型"), Placeholder: d.tr("类型"), Filter: true, Value: &d.kind, Items: []desktopkit.SelectItem{{Value: "all", Label: d.tr("所有类型")}, {Value: "文本", Label: d.tr("文本")}, {Value: "代码", Label: d.tr("代码")}, {Value: "链接", Label: d.tr("链接")}, {Value: "图片", Label: d.tr("图片")}, {Value: "文件", Label: d.tr("文件")}, {Value: "文件夹", Label: d.tr("文件夹")}}})
		sources := []desktopkit.SelectItem{{Value: "all", Label: d.tr("所有设备")}}
		for _, v := range d.devices {
			sources = append(sources, desktopkit.SelectItem{Value: v.id, Label: v.name})
		}
		k.Select(c.Key("source"), desktopkit.SelectProps{Label: d.tr("来源设备"), Placeholder: d.tr("设备"), Filter: true, Value: &d.source, Items: sources})
		tagLabel := d.tr("标签")
		if n := len(selectedSearchTags(d.tag)); n > 1 {
			tagLabel = fmt.Sprintf(d.tr("标签 · %d"), n)
		}
		tagItems := []desktopkit.SelectItem{{Value: "all", Label: d.tr("所有标签")}}
		for _, tag := range d.tags {
			tagItems = append(tagItems, desktopkit.SelectItem{Value: tag.id, Label: "#" + d.tagLabel(tag.id)})
		}
		k.Select(c.Key("tag"), desktopkit.SelectProps{Label: d.tr("标签筛选"), Placeholder: tagLabel, Filter: true, Value: &d.tag, Items: tagItems})
		k.Select(c.Key("time"), desktopkit.SelectProps{Label: d.tr("时间范围"), Placeholder: d.tr("时间"), Filter: true, Value: &d.timeRange, Items: []desktopkit.SelectItem{{Value: "all", Label: d.tr("所有时间")}, {Value: "today", Label: d.tr("今天")}, {Value: "yesterday", Label: d.tr("昨天")}, {Value: "last_7d", Label: d.tr("最近 7 天")}, {Value: "last_30d", Label: d.tr("最近 30 天")}, {Value: "this_week", Label: d.tr("本周")}, {Value: "this_month", Label: d.tr("本月")}}})
	})
}

func (d *viewState) historyCommands(c *ui.Context, k desktopkit.Kit) {
	fit := func(e ui.Element) ui.Element {

		return e
	}
	v := d.current()
	b, command := k.Menu(c.Key("send"), desktopkit.MenuProps{Label: d.tr("发送到设备"), Icon: desktopkit.IconSend, IconOnly: true, Variant: desktopkit.Subtle, Items: d.sendMenuItems()})
	fit(b)
	if command != "" {
		d.sendTo(command)
	}
	if fit(k.Button(c.Key("pin"), "", desktopkit.ButtonProps{Icon: desktopkit.IconPin, Label: d.tr("切换置顶"), Variant: desktopkit.Subtle, Disabled: v == nil || d.live != nil})).Clicked() {
		d.togglePin()
	}
	if d.live != nil && v != nil && !d.live.busy {
		kind := d.live.items[v.id].Kind
		var actions []desktopkit.MenuEntry
		for _, action := range d.live.actions {
			if slices.Contains(action.Accepts, kind) {
				actions = append(actions, desktopkit.MenuEntry{ID: action.Key, Label: d.tr(action.Title)})
			}
		}
		if len(actions) > 0 {
			button, key := k.Menu(c.Key("history-actions"), desktopkit.MenuProps{Label: d.tr("动作"), Icon: desktopkit.IconEdit, IconOnly: true, Variant: desktopkit.Subtle, Items: actions})
			fit(button)
			if key != "" {
				d.runHistoryAction(v.id, key)
			}
		}
	}
	b, command = k.Menu(c.Key("more"), desktopkit.MenuProps{Label: d.tr("更多操作"), Icon: desktopkit.IconMore, IconOnly: true, Variant: desktopkit.Subtle, Items: []desktopkit.MenuEntry{{ID: "tags", Label: d.tr("管理标签"), Icon: desktopkit.IconTag}, {ID: "delete", Label: d.tr("删除"), Icon: desktopkit.IconDelete, Destructive: true, Disabled: v == nil}, desktopkit.MenuSeparator("sep"), {ID: "quick-panel", Label: d.tr("查看快捷面板")}}})
	fit(b)
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
			card := desktopkit.GridItem{ID: v.id, Title: d.entryTitle(v), Subtitle: d.sourceName(v.source) + " · " + d.displayDate(v.date), Kind: v.kind, Text: v.text}
			if v.kind == desktopkit.KindImage {
				card.Media = func() { ui.Image(c, d.entryImage(c, v)).FillWidth().Height(110) }
			}
			cards = append(cards, card)
		}
		k.GroupedGrid(c.Key("grid"), &d.list, desktopkit.GroupedGridProps{Label: d.tr("历史网格"), Groups: groups, Items: cards, Size: float32(d.gridSize)}).Grow(1).MinHeight(0)
	} else {
		rows := make([]desktopkit.ItemListItem, 0, len(items))
		for _, v := range items {
			rows = append(rows, desktopkit.ItemListItem{ID: v.id, Title: d.entryTitle(v), Highlight: d.historyHighlightTerms(v), Subtitle: d.sourceName(v.source) + " · " + strings.TrimPrefix(strings.TrimPrefix(v.date, "今天 "), "昨天 "), Leading: func() {
				labels := []string{"TXT", "CODE", "URL", "图片", "FILE", "DIR"}
				p := desktopkit.KindBadgeProps{Kind: v.kind, Label: d.tr(labels[v.kind])}
				if v.kind == desktopkit.KindImage {
					p.Image = d.entryImage(c, v)
				}
				k.KindBadge(c.Key(v.id+"-icon"), p)
			}, Trailing: func() {
				if v.pinned {
					k.Icon(c.Key(v.id+"-pin"), desktopkit.IconPin, 12)
				}
				for _, name := range v.tags {
					k.Tag(c.Key(v.id+name), "#"+d.tagLabel(name), desktopkit.TagProps{Color: d.tagColor(name)})
				}
			}})
		}
		k.GroupedList(c.Key("list"), &d.list, desktopkit.GroupedListProps{Label: d.tr("历史条目"), Groups: groups, Items: rows}).Grow(1).MinHeight(0).Margin(0, 0, 0, 8)
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
	}
	if id, ok := d.list.Activated(); ok {
		d.selected = id
		d.useEntry()
		d.detailOpen, d.inspector = true, true
	}
	if d.live != nil {
		d.live.loadDetail(d)
	}
	if ids, ok := d.list.DeleteRequested(); ok && len(ids) > 0 {
		d.requestDelete(ids)
	}
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
	k.Scroll(c.Key(v.id)).Grow(1).MinHeight(0).Children(func() {
		ui.Column(c).Padding(1, 24, 112).Gap(16).Children(func() {
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
			if v.kind == desktopkit.KindImage {
				kind = desktopkit.PreviewImage
			}
			if v.kind == desktopkit.KindFile || v.kind == desktopkit.KindFolder {
				kind = desktopkit.PreviewFile
			}
			if v.image != nil {
				kind = desktopkit.PreviewImage
			}
			p := desktopkit.PreviewFrameProps{Kind: kind}
			if kind == desktopkit.PreviewImage {
				if v.kind == desktopkit.KindFile {
					p.Caption = v.text
				}
				p.BottomStart = func() {
					if k.Button(c, d.tr("快速查看"), desktopkit.ButtonProps{Size: desktopkit.SizeSmall}).Clicked() {
						d.quickLook = true
					}
				}
			}
			k.PreviewFrame(c, p, func() {
				if d.live != nil && (d.live.detailBusy || v.contentError != "") {
					label := d.tr(v.contentError)
					if d.live.detailBusy {
						label = d.tr("正在读取内容…")
					}
					k.Text(c, label, secondary)
					return
				}

				if v.rich != nil {
					richtext.EditorBase(c, v.rich, richtext.EditorOptions{Label: d.tr("富文本预览"), ReadOnly: true}).FillWidth().Height(240)
					return
				}
				if kind == desktopkit.PreviewImage {
					ui.Image(c, d.entryImage(c, *v)).FillWidth().Height(220)
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
				e := k.Text(c, v.text, desktopkit.TextProps{}).Padding(24).FillWidth()
				if v.kind == desktopkit.KindCode {
					e.Font("monospace").TextColor(ui.RGBA(235, 235, 240, 1))
				}
			})
			if kind == desktopkit.PreviewImage {
				facts := []desktopkit.Fact{{ID: "size", Label: d.tr("尺寸"), Value: "1600 × 1000"}, {ID: "format", Label: d.tr("格式"), Value: "PNG · 286 KB"}}
				if d.live != nil {
					facts = nil
					if v.image != nil {
						w, h := imageDimensions(*v)
						facts = []desktopkit.Fact{{ID: "size", Label: d.tr("尺寸"), Value: fmt.Sprintf("%.0f × %.0f", w, h)}, {ID: "bytes", Label: d.tr("大小"), Value: v.size}}
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
	actions := func() {
		k.ActionBar(c, desktopkit.ActionBarProps{Hint: d.tr("↩ 复制 · T 标签 · …")}, func() {
			copy := d.tr("复制")
			if d.notice != "" {
				copy = d.tr(d.notice)
			}
			if k.Button(c, copy, desktopkit.ButtonProps{Variant: desktopkit.Primary, Icon: desktopkit.IconCopy, Size: desktopkit.SizeXLarge}).Clicked() {
				d.useEntry()
			}
			_, target := k.Menu(c.Key("send-target"), desktopkit.MenuProps{Label: d.tr("发送到…"), Variant: desktopkit.Standard, Icon: desktopkit.IconSend, Size: desktopkit.SizeLarge, Items: d.sendMenuItems()})
			if target != "" {
				d.sendTo(target)
			}
		}).FillWidth()
	}

	{
		actions()
	}
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

func (d *viewState) entryImage(c *ui.Context, v entry) ui.ImageSource {
	if d.live != nil {
		if v.picture != nil {
			return v.picture.source(c)
		}
		return v.image
	}
	return d.previewImage
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
	if d.live != nil && d.live.cursor != "" {
		return fmt.Sprintf("%d+", len(d.entries))
	}
	return fmt.Sprint(len(d.entries))
}

func (d *viewState) historyHighlightTerms(v entry) []string {
	if d.live == nil {
		return nil
	}
	r := []rune(v.title)
	out := []string{}
	for _, span := range v.highlights {
		start, end := max(0, span.Start), min(len(r), span.End)
		if start < end {
			out = append(out, string(r[start:end]))
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
			} else if out.SyncDisposition != engine.SyncQueued {
				l.message = "内容已保存，但未发送。请检查同步设置和设备连接。"
			} else {
				d.notice = "发送完成"
			}
		}:
			l.wake()
		case <-l.ctx.Done():
		}
	}()
}
