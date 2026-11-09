package desktopdemo

import (
	"fmt"
	"slices"
	"strings"

	"github.com/egoist/mygo/ui"
	"github.com/uniclipboard/unic/app/desktop/internal/uikit"
)

func (d *demo) sidebar(c *ui.Context, k uikit.Kit) {
	pinned := 0
	for _, v := range d.entries {
		if v.pinned {
			pinned++
		}
	}
	tags := []uikit.SidebarNavItem{}
	for _, tag := range d.tags {
		if !tag.sidebar {
			continue
		}
		tags = append(tags, uikit.SidebarNavItem{ID: "tag:" + tag.id, Label: "#" + tag.name, Swatch: tag.color, Count: fmt.Sprint(d.tagCount(tag.id))})
	}
	devices := []uikit.SidebarNavItem{}
	for _, v := range d.devices {
		if v.id == "local" {
			continue
		}
		tone := uikit.StatusNeutral
		if v.online {
			tone = uikit.StatusPositive
		}
		state := ""
		if !v.online {
			state = "离线"
		}
		devices = append(devices, uikit.SidebarNavItem{ID: "device:" + v.id, Label: v.name, Status: &tone, Count: state})
	}
	k.SidebarNav(c, &d.nav, uikit.SidebarNavProps{Label: "资料库", Sections: []uikit.SidebarNavSection{
		{ID: "library", Items: []uikit.SidebarNavItem{{ID: "all", Label: "全部项目", Icon: uikit.IconHistory, Count: fmt.Sprint(len(d.entries))}, {ID: "pinned", Label: "已置顶", Icon: uikit.IconPin, Count: fmt.Sprint(pinned)}}},
		{ID: "tags", Title: "标签", Items: tags, Action: &uikit.NavAction{Label: "管理标签", Icon: uikit.IconAdd}, More: &uikit.NavAction{Label: fmt.Sprintf("全部标签 · %d", len(d.tags))}},
		{ID: "devices", Title: "设备", Items: devices},
	}, Footer: []uikit.SidebarNavItem{{ID: "manage", Label: "管理设备…", Icon: uikit.IconDevices}, {ID: "settings", Label: "设置…", Icon: uikit.IconSettings, ShortcutHint: "⌘,"}}}).Grow(1)
	if cmd, _ := d.nav.Command(); cmd == uikit.NavSectionAction || cmd == uikit.NavMore {
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

func (d *demo) history(c *ui.Context, k uikit.Kit) {
	items := d.filtered()
	if !slices.ContainsFunc(items, func(v entry) bool { return v.id == d.selected }) {
		d.selectEntry("")
		if len(items) > 0 {
			d.selectEntry(items[0].id)
		}
	}
	k.SplitView(c, uikit.SplitViewProps{
		Title: "全部", Subtitle: fmt.Sprint(len(items)),
		Sidebar: func() { d.sidebar(c, k) }, SidebarHidden: &d.sidebarHidden, SidebarOverlay: d.compact,
		SidebarToggleLabel: "切换侧栏", DetailOpen: &d.detailOpen, BackLabel: "返回历史",
		Search: func() {
			d.historySearch(c.Key("search"), k)
		},
		ToolbarLeading: func() {
			if d.layout == "grid" && !d.compact {
				k.Slider(c.Key("card-size"), &d.gridSize, uikit.SliderProps{Label: "卡片大小", Min: 0, Max: 1, Step: .05}).Width(80)
			}
			r := k.SegmentedControl(c.Key("layout"), uikit.SegmentedControlProps{Label: "显示方式", Value: &d.layout, Toolbar: true, Items: []uikit.SegmentedItem{{Value: "list", Label: "列表", Icon: uikit.IconList, IconOnly: true}, {Value: "grid", Label: "网格", Icon: uikit.IconCopy, IconOnly: true}}})
			if r.Changed {
				d.inspector, d.detailOpen = false, false
			}
		},
		Toolbar: func() { k.ToolbarGroup(c, func() { d.historyCommands(c, k) }) },
	}, func() {
		pane := uikit.PaneList
		if d.layout == "grid" {
			pane = uikit.PaneMain
		}
		k.Pane(c.Key("browse"), uikit.PaneProps{Kind: pane, Label: "历史列表", DetailOpen: &d.detailOpen}, func() {
			d.filters(c, k)
			if len(items) == 0 {
				k.ContentUnavailable(c, uikit.ContentUnavailableProps{Icon: uikit.IconSearch, Title: "没有匹配的历史", Description: "修改搜索词或清除筛选。"}, func() {
					if k.Button(c, "清除筛选", uikit.ButtonProps{}).Clicked() {
						d.query, d.kind, d.source, d.tag, d.timeRange, d.nav.Value = "", "", "", "", "", "all"
					}
				})
			} else {
				d.historyList(c, k, items)
			}
		})
		if d.layout == "list" || d.inspector || (d.compact && d.detailOpen) {
			k.Pane(c.Key("detail"), uikit.PaneProps{Kind: uikit.PaneDetail, Label: "历史详情", DetailOpen: &d.detailOpen}, func() { d.detail(c, k) })
		}
	})
	if d.quickLook {
		k.QuickLook(c.Key("quick-look"), uikit.QuickLookProps{Open: &d.quickLook, Title: "桌面布局.png", CloseLabel: "关闭预览", Src: fixtureImage(), Alt: "桌面布局示例", PrimaryLabel: "复制图片"}, nil)
	}
}

func (d *demo) filters(c *ui.Context, k uikit.Kit) {
	ui.Row(c.Key("filters")).Padding(5, 16, 5).Gap(6).Wrap().Shrink(0).Children(func() {
		k.Select(c.Key("kind"), uikit.SelectProps{Label: "内容类型", Placeholder: "类型", Filter: true, Value: &d.kind, Items: []uikit.SelectItem{{Value: "all", Label: "所有类型"}, {Value: "文本", Label: "文本"}, {Value: "代码", Label: "代码"}, {Value: "链接", Label: "链接"}, {Value: "图片", Label: "图片"}, {Value: "文件", Label: "文件"}}})
		sources := []uikit.SelectItem{{Value: "all", Label: "所有设备"}}
		for _, v := range d.devices {
			sources = append(sources, uikit.SelectItem{Value: v.id, Label: v.name})
		}
		k.Select(c.Key("source"), uikit.SelectProps{Label: "来源设备", Placeholder: "设备", Filter: true, Value: &d.source, Items: sources})
		tagLabel := "标签"
		if n := len(selectedSearchTags(d.tag)); n > 1 {
			tagLabel = fmt.Sprintf("标签 · %d", n)
		}
		tagItems := []uikit.SelectItem{{Value: "all", Label: "所有标签"}}
		for _, tag := range d.tags {
			tagItems = append(tagItems, uikit.SelectItem{Value: tag.id, Label: "#" + tag.name})
		}
		k.Select(c.Key("tag"), uikit.SelectProps{Label: "标签筛选", Placeholder: tagLabel, Filter: true, Value: &d.tag, Items: tagItems})
		k.Select(c.Key("time"), uikit.SelectProps{Label: "时间范围", Placeholder: "时间", Filter: true, Value: &d.timeRange, Items: []uikit.SelectItem{{Value: "all", Label: "所有时间"}, {Value: "today", Label: "今天"}, {Value: "yesterday", Label: "昨天"}, {Value: "last_7d", Label: "最近 7 天"}, {Value: "last_30d", Label: "最近 30 天"}, {Value: "this_week", Label: "本周"}, {Value: "this_month", Label: "本月"}}})
	})
}

func (d *demo) historyCommands(c *ui.Context, k uikit.Kit) {
	fit := func(e ui.Element) ui.Element {
		if k.Skin == uikit.MacOS {
			e.Size(32, 26).Radius(13)
		}
		return e
	}
	v := d.current()
	b, command := k.Menu(c.Key("send"), uikit.MenuProps{Label: "发送到设备", Icon: uikit.IconSend, IconOnly: true, Variant: uikit.Subtle, Items: d.sendMenuItems()})
	fit(b)
	if command != "" {
		d.sendTo(command)
	}
	if fit(k.Button(c.Key("pin"), "", uikit.ButtonProps{Icon: uikit.IconPin, Label: "切换置顶", Variant: uikit.Subtle, Disabled: v == nil})).Clicked() {
		d.togglePin()
	}
	b, command = k.Menu(c.Key("more"), uikit.MenuProps{Label: "更多操作", Icon: uikit.IconMore, IconOnly: true, Variant: uikit.Subtle, Items: []uikit.MenuEntry{{ID: "tags", Label: "管理标签", Icon: uikit.IconTag}, {ID: "delete", Label: "删除", Icon: uikit.IconDelete, Destructive: true, Disabled: v == nil}, uikit.MenuSeparator("sep"), {ID: "setup", Label: "查看首次引导"}, {ID: "unlock", Label: "查看解锁界面"}, {ID: "quick-panel", Label: "查看快捷面板"}}})
	fit(b)
	switch command {
	case "tags":
		d.tagsOpen = true
	case "delete":
		if v != nil {
			d.requestDelete(d.list.Selection())
		}
	case "setup", "unlock", "quick-panel":
		d.page = command
	}
}

func (d *demo) historyList(c *ui.Context, k uikit.Kit, items []entry) {
	groups := []uikit.ListGroup{}
	for _, v := range items {
		day := strings.Fields(v.date)[0]
		if len(groups) == 0 || groups[len(groups)-1].Title != day {
			groups = append(groups, uikit.ListGroup{ID: day, Title: day})
		}
		groups[len(groups)-1].Count++
	}
	for i := range groups {
		groups[i].Trailing = fmt.Sprintf("%d 项", groups[i].Count)
		if groups[i].Title == "今天" {
			groups[i].Detail = "11月9日 周一"
		} else {
			groups[i].Detail = "11月8日 周日"
		}
	}
	if d.layout == "grid" {
		cards := make([]uikit.GridItem, 0, len(items))
		for _, v := range items {
			card := uikit.GridItem{ID: v.id, Title: v.title, Subtitle: d.sourceName(v.source) + " · " + v.date, Kind: v.kind, Text: v.text}
			if v.kind == uikit.KindImage {
				card.Media = func() { ui.Image(c, fixtureImage()).FillWidth().Height(110) }
			}
			cards = append(cards, card)
		}
		k.GroupedGrid(c.Key("grid"), &d.list, uikit.GroupedGridProps{Label: "历史网格", Groups: groups, Items: cards, Size: float32(d.gridSize)}).Grow(1).MinHeight(0)
	} else {
		rows := make([]uikit.ItemListItem, 0, len(items))
		for _, v := range items {
			rows = append(rows, uikit.ItemListItem{ID: v.id, Title: v.title, Subtitle: d.sourceName(v.source) + " · " + strings.TrimPrefix(strings.TrimPrefix(v.date, "今天 "), "昨天 "), Leading: func() {
				labels := []string{"TXT", "CODE", "URL", "图片", "FILE"}
				p := uikit.KindBadgeProps{Kind: v.kind, Label: labels[v.kind]}
				if v.kind == uikit.KindImage {
					p.Image = fixtureImage()
				}
				k.KindBadge(c.Key(v.id+"-icon"), p)
			}, Trailing: func() {
				if v.pinned {
					k.Icon(c.Key(v.id+"-pin"), uikit.IconPin, 12)
				}
				for _, name := range v.tags {
					k.Tag(c.Key(v.id+name), "#"+d.tagLabel(name), uikit.TagProps{Color: d.tagColor(name)})
				}
			}})
		}
		k.GroupedList(c.Key("list"), &d.list, uikit.GroupedListProps{Label: "历史条目", Groups: groups, Items: rows}).Grow(1).MinHeight(0).Margin(0, 0, 0, 8)
	}
	if d.list.Changed() {
		if ids := d.list.Selection(); len(ids) > 0 {
			d.selected, d.notice, d.detailOpen, d.inspector = ids[0], "", true, true
		}
	}
	if id, ok := d.list.Activated(); ok {
		d.selected = id
		d.notice = "演示：已复制"
		d.detailOpen, d.inspector = true, true
	}
	if ids, ok := d.list.DeleteRequested(); ok && len(ids) > 0 {
		d.requestDelete(ids)
	}
}

func (d *demo) detail(c *ui.Context, k uikit.Kit) {
	if ids := d.list.Selection(); len(ids) > 1 {
		k.ContentUnavailable(c, uikit.ContentUnavailableProps{Icon: uikit.IconCopy, Title: fmt.Sprintf("已选择 %d 项", len(ids)), Description: "通过工具栏置顶或删除所选历史。"}, nil)
		return
	}
	v := d.current()
	if v == nil {
		k.ContentUnavailable(c, uikit.ContentUnavailableProps{Title: "未选择历史", Description: "选择一条历史以查看内容。"}, nil)
		return
	}
	k.Scroll(c.Key(v.id)).Grow(1).MinHeight(0).Children(func() {
		ui.Column(c).Padding(1, 24, 112).Gap(16).Children(func() {
			r := k.DetailHeader(c, uikit.DetailHeaderProps{PinLabel: "切换置顶", DeleteLabel: "删除"}, func() {
				k.KindBadge(c, uikit.KindBadgeProps{Kind: v.kind, Label: kindName(v.kind), Variant: uikit.KindPill})
				k.Text(c, d.sourceName(v.source)+" · 11月9日 · "+strings.TrimPrefix(v.date, "今天 "), uikit.TextProps{Role: uikit.RoleCallout, Tone: uikit.ToneSecondary, Truncate: true}).Grow(1)
			})
			if r.PinClicked {
				v.pinned = !v.pinned
			}
			if r.DeleteClicked {
				d.requestDelete([]string{v.id})
			}
			kind := uikit.PreviewPlain
			if v.kind == uikit.KindCode {
				kind = uikit.PreviewCode
			}
			if v.kind == uikit.KindImage {
				kind = uikit.PreviewImage
			}
			if v.kind == uikit.KindFile {
				kind = uikit.PreviewFile
			}
			p := uikit.PreviewFrameProps{Kind: kind}
			if v.kind == uikit.KindImage {
				p.BottomStart = func() {
					if k.Button(c, "快速查看", uikit.ButtonProps{Size: uikit.SizeSmall}).Clicked() {
						d.quickLook = true
					}
				}
			}
			k.PreviewFrame(c, p, func() {
				if v.kind == uikit.KindImage {
					ui.Image(c, fixtureImage()).FillWidth().Height(220)
					return
				}
				if v.kind == uikit.KindFile {
					for i, name := range strings.Split(v.text, "\n") {
						k.SettingsRow(c.Key(i), uikit.SettingsRowProps{Label: name, Description: "已接收"}, nil, func() { k.Icon(c, uikit.IconFile, 18) })
					}
					return
				}
				e := k.Text(c, v.text, uikit.TextProps{}).Padding(24).FillWidth()
				if v.kind == uikit.KindCode {
					e.Font("monospace").TextColor(ui.RGBA(235, 235, 240, 1))
				}
			})
			if v.kind == uikit.KindImage {
				k.FactList(c, uikit.FactListProps{Label: "图片信息", Facts: []uikit.Fact{{ID: "size", Label: "尺寸", Value: "1600 × 1000"}, {ID: "format", Label: "格式", Value: "PNG · 286 KB"}}})
			}
			ui.Row(c).Gap(6).Wrap().Children(func() {
				for _, tag := range v.tags {
					k.Tag(c.Key(tag), "#"+d.tagLabel(tag), uikit.TagProps{Color: d.tagColor(tag)})
				}
				if k.Button(c, "添加标签", uikit.ButtonProps{Variant: uikit.Subtle, Icon: uikit.IconAdd, Size: uikit.SizeSmall}).Clicked() {
					d.assignTagsOpen = true
				}
			})
			ui.Column(c).Gap(6).Children(func() {
				rows := []uikit.InsetListRow{}
				for _, peer := range d.devices {
					if peer.id == "local" {
						continue
					}
					rows = append(rows, uikit.InsetListRow{ID: peer.id, Title: peer.name, Status: func() { k.Icon(c, uikit.IconInfo, 16).TextColor(k.Sem("mutedForeground")) }})
				}
				ui.Row(c).Gap(8).AlignItems(ui.Center).Children(func() {
					k.Text(c, "送达", uikit.TextProps{Emphasized: true})
					k.Text(c, fmt.Sprintf("0/%d 台设备", len(rows)), uikit.TextProps{Role: uikit.RoleCallout, Tone: uikit.ToneSecondary})
				})
				k.InsetList(c, uikit.InsetListProps{Label: "送达情况", Rows: rows})
				k.Text(c, "本地加密存储 · 设备之间端到端加密", uikit.TextProps{Role: uikit.RoleCaption, Tone: uikit.ToneTertiary})
			})
		})
	})
	actions := func() {
		k.ActionBar(c, uikit.ActionBarProps{Hint: "↩ 复制 · T 标签 · …"}, func() {
			copy := "复制"
			if d.notice != "" {
				copy = d.notice
			}
			if k.Button(c, copy, uikit.ButtonProps{Variant: uikit.Primary, Icon: uikit.IconCopy, Size: uikit.SizeXLarge}).Clicked() {
				d.notice = "演示：已复制"
			}
			_, target := k.Menu(c.Key("send-target"), uikit.MenuProps{Label: "发送到…", Variant: uikit.Glass, Icon: uikit.IconSend, Size: uikit.SizeLarge, Items: d.sendMenuItems()})
			if target != "" {
				d.sendTo(target)
			}
		}).FillWidth()
	}
	if k.Skin == uikit.MacOS {
		ui.Column(c.Key("floating-actions")).Absolute().Left(16).Right(16).Bottom(16).ZIndex(2).Children(actions)
	} else {
		actions()
	}
}

func (d *demo) assignTagSheet(c *ui.Context, k uikit.Kit) {
	if k.Sheet(c, uikit.SheetProps{Open: &d.assignTagsOpen, Title: "添加标签", Variant: uikit.SheetTask, Done: "完成"}, func() {
		ui.Column(c).Gap(8).Padding(12).Children(func() {
			v := d.current()
			for _, tag := range d.tags {
				if tag.builtin {
					continue
				}
				id := tag.id
				on := v != nil && slices.Contains(v.tags, id)
				before := on
				k.Checkbox(c.Key(id), &on, uikit.CheckboxProps{Label: "#" + tag.name, Disabled: v == nil})
				if on != before {
					if on {
						v.tags = append(v.tags, id)
					} else {
						v.tags = slices.DeleteFunc(v.tags, func(s string) bool { return s == id })
					}
				}
			}
		})
	}) {
		d.assignTagsOpen = false
	}
}

// Send menus share the current remote-device registry, including offline peers.
func (d *demo) sendMenuItems() []uikit.MenuEntry {
	items := []uikit.MenuEntry{{ID: "all", Label: "所有设备", Icon: uikit.IconDevices}}
	for _, peer := range d.devices {
		if peer.id == "local" {
			continue
		}
		if len(items) == 1 {
			items = append(items, uikit.MenuSeparator("devices"))
		}
		items = append(items, uikit.MenuEntry{ID: peer.id, Label: peer.name})
	}
	items[0].Disabled = len(items) == 1
	return items
}

func (d *demo) sendTo(target string) {
	name := d.sourceName(target)
	if target == "all" {
		name = "所有设备"
	}
	d.notice = "演示：已发送到" + name
}
