package desktopui

import (
	"crypto/sha256"
	"encoding/hex"
	"fmt"
	"maps"
	"slices"
	"strings"

	"github.com/egoist/mygo/ui"

	"github.com/uniclipboard/unic/app/desktop/internal/desktopkit"
	"github.com/uniclipboard/unic/plugin"
)

type historyTag struct {
	id, name         string
	color            desktopkit.TagColor
	builtin, sidebar bool
}
type tagManagerState struct {
	filter, sort, editing, name, error string
	newColor                           desktopkit.TagColor
	colorPop                           string
	next                               int
	selected                           []string
}

func builtinTags() []historyTag {
	return []historyTag{
		{id: plugin.TagText, name: "文本", color: "blue", builtin: true, sidebar: true},
		{id: plugin.TagImage, name: "图片", color: "green", builtin: true, sidebar: true},
		{id: plugin.TagFiles, name: "文件", color: "orange", builtin: true, sidebar: true},
		{id: plugin.TagFolder, name: "文件夹", color: "gray", builtin: true, sidebar: false},
		{id: plugin.TagCode, name: "代码", color: "purple", builtin: true, sidebar: false},
		{id: plugin.TagLink, name: "链接", color: "blue", builtin: true, sidebar: false},
		{id: plugin.TagEmail, name: "邮箱地址", color: "gray", builtin: true, sidebar: false},
		{id: plugin.TagColor, name: "颜色", color: "gray", builtin: true},
		{id: plugin.TagVersion, name: "版本号", color: "gray", builtin: true},
		{id: plugin.TagUUID, name: "UUID", color: "gray", builtin: true},
		{id: plugin.TagIPv4, name: "IPv4 地址", color: "gray", builtin: true},
	}
}
func (d *viewState) projectTags(ids []string) {
	visibility := make(map[string]bool, len(d.tags))
	for _, tag := range d.tags {
		visibility[tag.id] = tag.sidebar
	}
	sidebar := func(id string, fallback bool) bool {
		if d.live != nil {
			if saved, ok := d.live.config.Config.Desktop.TagSidebar[tagColorKey(id)]; ok {
				return saved
			}
			return fallback
		}
		if previous, ok := visibility[id]; ok {
			return previous
		}
		return fallback
	}
	builtins := builtinTags()
	for i := range builtins {
		if d.live != nil {
			if saved := d.live.config.Config.Desktop.TagColors[tagColorKey(builtins[i].id)]; saved != "" {
				builtins[i].color = desktopkit.TagColor(saved)
			}
		}
		builtins[i].sidebar = sidebar(builtins[i].id, builtins[i].sidebar)
	}
	d.tags = builtins
	for _, id := range ids {
		if d.tagByID(id) == nil {
			color := desktopkit.TagColor("gray")
			if d.live != nil {
				if saved := d.live.config.Config.Desktop.TagColors[tagColorKey(id)]; saved != "" {
					color = desktopkit.TagColor(saved)
				}
			}
			d.tags = append(d.tags, historyTag{id: id, name: id, color: color, sidebar: sidebar(id, true)})
		}
	}
}
func (d *viewState) tagByID(id string) *historyTag {
	for i := range d.tags {
		if d.tags[i].id == id {
			return &d.tags[i]
		}
	}
	return nil
}
func (d *viewState) tagLabel(id string) string {
	if t := d.tagByID(id); t != nil {
		if t.builtin {
			return d.tr(t.name)
		}
		return t.name
	}
	return id
}
func (d *viewState) tagColor(id string) desktopkit.TagColor {
	if t := d.tagByID(id); t != nil {
		return t.color
	}
	return "gray"
}
func (d *viewState) entryHasTag(e entry, id string) bool {
	return slices.Contains(e.tags, id)
}
func (d *viewState) tagCount(id string) int {
	n := 0
	for _, e := range d.entries {
		if d.entryHasTag(e, id) {
			n++
		}
	}
	return n
}
func (d *viewState) localTagIDs(ids []string) []string {
	var out []string
	for _, id := range ids {
		if tag := d.tagByID(id); tag != nil && !tag.builtin {
			out = append(out, id)
		}
	}
	return out
}

func (d *viewState) removeTags(ids []string, target string) {
	if d.live != nil {
		return
	}
	ids = d.localTagIDs(ids)
	if len(ids) == 0 {
		return
	}
	if target != "" {
		tag := d.tagByID(target)
		if tag == nil || tag.builtin || slices.Contains(ids, target) {
			return
		}
	}
	for i := range d.entries {
		e := &d.entries[i]
		had := slices.ContainsFunc(e.tags, func(id string) bool { return slices.Contains(ids, id) })
		e.tags = slices.DeleteFunc(e.tags, func(id string) bool { return slices.Contains(ids, id) })
		if had && target != "" && !slices.Contains(e.tags, target) {
			e.tags = append(e.tags, target)
		}
	}
	d.tags = slices.DeleteFunc(d.tags, func(tag historyTag) bool { return slices.Contains(ids, tag.id) })
	selected := selectedSearchTags(d.tag)
	had := slices.ContainsFunc(selected, func(id string) bool { return slices.Contains(ids, id) })
	selected = slices.DeleteFunc(selected, func(id string) bool { return slices.Contains(ids, id) })
	if had && target != "" && !slices.Contains(selected, target) {
		selected = append(selected, target)
	}
	d.tag = strings.Join(selected, "\x1f")
	if strings.HasPrefix(d.nav.Value, "tag:") && slices.Contains(ids, strings.TrimPrefix(d.nav.Value, "tag:")) {
		d.nav.Value = "all"
	}
	d.tagManager.selected = slices.DeleteFunc(d.tagManager.selected, func(id string) bool { return slices.Contains(ids, id) })
}

func (d *viewState) tagMenu(ids []string) []desktopkit.MenuEntry {
	own := d.localTagIDs(ids)
	var out []desktopkit.MenuEntry
	var single *historyTag
	if len(ids) == 1 {
		single = d.tagByID(ids[0])
	}
	if single != nil && !single.builtin && d.live == nil {
		out = append(out, desktopkit.MenuEntry{ID: "rename", Label: d.tr("重命名…"), Icon: desktopkit.IconEdit})
	}
	if len(own) > 0 && d.live == nil {
		var targets []desktopkit.MenuEntry
		for _, tag := range d.tags {
			if !tag.builtin && !slices.Contains(ids, tag.id) {
				targets = append(targets, desktopkit.MenuEntry{ID: "merge:" + tag.id, Label: "#" + d.tagLabel(tag.id)})
			}
		}
		if len(targets) > 0 {
			out = append(out, desktopkit.MenuEntry{ID: "merge", Label: d.tr("合并到"), Icon: desktopkit.IconMerge, Children: targets})
		}
	}
	if single != nil {
		out = append(out, desktopkit.MenuEntry{ID: "show", Label: d.tr("显示条目"), Icon: desktopkit.IconSearch})
	}
	if single != nil && !single.builtin {
		out = append(out, desktopkit.MenuSeparator("colors"))
		for _, v := range [][2]string{{"orange", "橙色"}, {"blue", "蓝色"}, {"green", "绿色"}, {"purple", "紫色"}, {"gray", "灰色"}} {
			out = append(out, desktopkit.MenuEntry{ID: "color:" + v[0], Label: d.tr(v[1]), Swatch: desktopkit.TagColor(v[0]), Checked: single.color == desktopkit.TagColor(v[0]), Disabled: d.live != nil && d.live.busy})
		}
	}
	if len(own) > 0 && d.live == nil {
		label := d.tr("删除标签（保留条目）")
		if len(own) > 1 {
			label = fmt.Sprintf(d.tr("删除 %d 个标签（保留条目）"), len(own))
		}
		out = append(out, desktopkit.MenuSeparator("delete-separator"), desktopkit.MenuEntry{ID: "delete", Label: label, Icon: desktopkit.IconDelete, Destructive: true})
	}
	return out
}

func (d *viewState) tagAction(action string, ids []string) {
	m := &d.tagManager
	switch {
	case action == "rename" && len(ids) == 1:
		if tag := d.tagByID(ids[0]); tag != nil && !tag.builtin {
			m.editing, m.name, m.error = tag.id, tag.name, ""
		}
	case action == "show" && len(ids) == 1:
		d.tag = ids[0]
		d.nav.Value = "all"
		d.tagsOpen = false
	case action == "delete":
		d.removeTags(ids, "")
	case strings.HasPrefix(action, "merge:"):
		d.removeTags(ids, strings.TrimPrefix(action, "merge:"))
	case strings.HasPrefix(action, "color:") && len(ids) == 1:
		if tag := d.tagByID(ids[0]); tag != nil && !tag.builtin {
			d.setTagColor(tag.id, desktopkit.TagColor(strings.TrimPrefix(action, "color:")))
		}
	}
}
func (d *viewState) submitTagEdit() {
	m := &d.tagManager
	name := strings.TrimSpace(m.name)
	if name == "" {
		m.editing, m.error = "", ""
		return
	}
	if name == plugin.TagFavorite {
		m.error = "已存在同名标签"
		return
	}
	for _, tag := range d.tags {
		if tag.id != m.editing && strings.EqualFold(d.tagLabel(tag.id), name) {
			m.error = "已存在同名标签"
			return
		}
	}
	if m.editing == "new" {
		m.next++
		id := fmt.Sprintf("custom:%d", m.next)
		color := m.newColor
		if color == "" {
			color = "blue"
		}
		d.tags = append(d.tags, historyTag{id: id, name: name, color: color, sidebar: true})
		m.selected = []string{id}
	} else if tag := d.tagByID(m.editing); tag != nil && !tag.builtin {
		tag.name = name
	}
	m.editing, m.name, m.error = "", "", ""
}

func (d *viewState) tagSheet(c *ui.Context, k desktopkit.Kit) {
	m := &d.tagManager
	if m.newColor == "" {
		m.newColor = "blue"
	}
	palette := [][2]string{{"blue", "蓝色"}, {"purple", "紫色"}, {"green", "绿色"}, {"orange", "橙色"}, {"gray", "灰色"}}
	shown := slices.Clone(d.tags)
	shown = slices.DeleteFunc(shown, func(tag historyTag) bool {
		return !strings.Contains(strings.ToLower(d.tagLabel(tag.id)), strings.ToLower(strings.TrimSpace(m.filter)))
	})
	var custom, builtin []historyTag
	for _, tag := range shown {
		if tag.builtin {
			builtin = append(builtin, tag)
		} else {
			custom = append(custom, tag)
		}
	}
	slices.SortStableFunc(custom, func(a, b historyTag) int {
		switch m.sort {
		case "mostUsed":
			if n := d.tagCount(b.id) - d.tagCount(a.id); n != 0 {
				return n
			}
		case "newest":
			return slices.IndexFunc(d.tags, func(t historyTag) bool { return t.id == b.id }) - slices.IndexFunc(d.tags, func(t historyTag) bool { return t.id == a.id })
		}
		return strings.Compare(strings.ToLower(a.name), strings.ToLower(b.name))
	})

	selectedCustom := d.localTagIDs(m.selected)
	foot := func() {
		if len(selectedCustom) > 0 && d.live == nil {
			k.Text(c.Key("bulk-count"), fmt.Sprintf(d.tr("已选择 %d 项"), len(selectedCustom)), desktopkit.TextProps{Role: desktopkit.RoleCallout, Emphasized: true}).Shrink(0)
			if k.Button(c.Key("delete"), d.tr("删除选中标签"), desktopkit.ButtonProps{Variant: desktopkit.Destructive, Size: desktopkit.SizeSmall, Icon: desktopkit.IconDelete}).Clicked() {
				d.removeTags(selectedCustom, "")
			}
			if k.Button(c.Key("clear-sel"), d.tr("取消选择"), desktopkit.ButtonProps{Variant: desktopkit.Subtle, Size: desktopkit.SizeSmall}).Clicked() {
				m.selected = nil
			}
			return
		}
		sidebar, unused := 0, 0
		for _, tag := range d.tags {
			if tag.sidebar {
				sidebar++
			}
			if !tag.builtin && d.tagCount(tag.id) == 0 {
				unused++
			}
		}
		k.Text(c.Key("summary"), fmt.Sprintf(d.tr("%d 个标签 · %d 个在侧边栏 · %d 个未使用"), len(d.tags), sidebar, unused), desktopkit.TextProps{Role: desktopkit.RoleCaption, Tone: desktopkit.ToneSecondary}).MinWidth(0)
	}

	if k.Sheet(c, desktopkit.SheetProps{
		Variant: desktopkit.SheetTask,
		Open:    &d.tagsOpen,
		Title:   d.tr("管理标签"),
		Footer:  foot,
		Done:    d.tr("完成"),
		OnClosed: func() {
			m.selected = nil
			m.editing, m.name, m.error, m.colorPop = "", "", "", ""
		},
	}, func() {
		ui.Column(c.Key("tag-manager-body")).Gap(12).Children(func() {
			// Controls row: search, sort, and inline create trigger.
			ui.Row(c.Key("controls")).Gap(8).AlignItems(ui.Center).Shrink(0).Children(func() {
				k.SearchField(c.Key("filter"), desktopkit.SearchFieldProps{
					Label:       d.tr("筛选标签"),
					Value:       &m.filter,
					Placeholder: fmt.Sprintf(d.tr("在 %d 个标签中筛选"), len(d.tags)),
				}).Element.Grow(1).MinWidth(120)
				k.Select(c.Key("sort"), desktopkit.SelectProps{
					Label: d.tr("标签排序"),
					Value: &m.sort,
					Items: []desktopkit.SelectItem{
						{Value: "mostUsed", Label: d.tr("最常用")},
						{Value: "name", Label: d.tr("名称")},
						{Value: "newest", Label: d.tr("最新")},
					},
				}).Element.Shrink(0)
				if k.Button(c.Key("add"), d.tr("新建标签"), desktopkit.ButtonProps{
					Variant:  desktopkit.Primary,
					Size:     desktopkit.SizeSmall,
					Icon:     desktopkit.IconAdd,
					Disabled: d.live != nil,
				}).Clicked() {
					if m.editing == "new" {
						m.editing, m.name, m.error = "", "", ""
					} else {
						m.editing, m.name, m.error, m.colorPop = "new", "", "", ""
						m.filter = ""
					}
				}
			})

			// Inline creation bar (expands cleanly under controls).
			if m.editing == "new" {
				ui.Row(c.Key("create-bar")).Gap(8).AlignItems(ui.Center).Padding(8, 12).Radius(10).Shrink(0).
					Background(k.Color("background-secondary-default")).
					Border(1, k.Color("separator-border")).Children(func() {
					ui.Row(c.Key("new-colors")).Gap(3).AlignItems(ui.Center).Shrink(0).Children(func() {
						for _, v := range palette {
							col := desktopkit.TagColor(v[0])
							btn := ui.ButtonBase(c.Key("nc-"+v[0])).Label(d.tr(v[1])).Size(22, 22).Radius(11).
								Justify(ui.Center).AlignItems(ui.Center)
							if m.newColor == col {
								btn.Border(1.5, k.Color("text-primary"))
							}
							btn.Children(func() {
								ui.Box(c).Size(10, 10).Radius(5).Background(k.SwatchColor(col))
							})
							if btn.Clicked() {
								m.newColor = col
							}
						}
					})
					field := k.TextField(c.Key("new-editor"), desktopkit.TextFieldProps{
						Label:     d.tr("新标签名称"),
						Value:     &m.name,
						AutoFocus: true,
						MaxLength: 64,
						Error:     d.tr(m.error),
					})
					field.Element.Grow(1).MinWidth(0)
					if field.Edited {
						m.error = ""
					}
					if field.Submitted {
						d.submitTagEdit()
					}
					if field.Escaped {
						m.editing, m.name, m.error = "", "", ""
					}
					if k.Button(c.Key("confirm-create"), d.tr("创建"), desktopkit.ButtonProps{Variant: desktopkit.Primary, Size: desktopkit.SizeSmall}).Clicked() {
						d.submitTagEdit()
					}
					if k.Button(c.Key("cancel-create"), d.tr("取消"), desktopkit.ButtonProps{Variant: desktopkit.Standard, Size: desktopkit.SizeSmall}).Clicked() {
						m.editing, m.name, m.error = "", "", ""
					}
				})
			}

			// Grouped tag list with inline quick actions and right-click context menu.
			list := k.Scroll(c.Key("tag-list")).Height(360).MinHeight(120).Gap(14).
				Padding(0, 12, 0, 0).Role(ui.RoleList).Label(d.tr("标签"))
			list.Children(func() {
				if len(custom) == 0 && len(builtin) == 0 {
					ui.Row(c.Key("empty")).Height(120).Justify(ui.Center).AlignItems(ui.Center).Children(func() {
						k.Text(c, d.tr("没有匹配的标签"), desktopkit.TextProps{Tone: desktopkit.ToneSecondary})
					})
					return
				}

				renderRow := func(t historyTag) {
					id := t.id
					count := d.tagCount(id)
					isSelected := slices.Contains(m.selected, id)
					menuIDs := []string{id}
					if isSelected && len(m.selected) > 1 {
						menuIDs = slices.Clone(m.selected)
					}
					row := ui.Row(c.Key("row-"+id)).Height(40).Padding(0, 10).Radius(9).Gap(8).
						AlignItems(ui.Center).Shrink(0).Role(ui.RoleListItem).Label("#" + d.tagLabel(id))
					row.AccessSelected(isSelected)
					hovered := row.Hovered()
					if isSelected {
						if k.Dark {
							row.Background(k.Color("accent-500").Alpha(.18))
						} else {
							row.Background(k.Color("accent-500").Alpha(.10))
						}
					} else if hovered {
						if k.Dark {
							row.Background(k.Color("text-primary").Alpha(.05))
						} else {
							row.Background(k.Color("background-secondary-hover"))
						}
					}
					k.AttachContextMenu(row, func() []desktopkit.MenuEntry { return d.tagMenu(menuIDs) }, func(action string) {
						d.tagAction(action, menuIDs)
					})
					row.Children(func() {
						// Leading swatch: clickable color picker for custom tags, static dot for built-in types.
						if !t.builtin && (d.live == nil || !d.live.busy) {
							sw := ui.ButtonBase(c.Key("swatch")).Label(d.tr("修改颜色")+" #"+d.tagLabel(id)).
								Size(22, 22).Radius(11).Justify(ui.Center).AlignItems(ui.Center).Shrink(0)
							if sw.Hovered() || m.colorPop == id {
								sw.Border(1.5, k.Color("border-button-hover"))
							}
							sw.Children(func() {
								ui.Box(c).Size(9, 9).Radius(4.5).Background(k.SwatchColor(t.color))
							})
							if sw.Clicked() {
								if m.colorPop == id {
									m.colorPop = ""
								} else {
									m.colorPop = id
								}
							}
							if m.colorPop == id {
								ui.Row(c.Key("color-pop")).Gap(2).Padding(2, 4).Radius(999).AlignItems(ui.Center).Shrink(0).
									Background(k.Color("background-primary-default")).
									Border(1, k.Color("separator-border")).Children(func() {
									for _, v := range palette {
										col := desktopkit.TagColor(v[0])
										cb := ui.ButtonBase(c.Key("cp-"+v[0])).Label(d.tr(v[1])).Size(20, 20).Radius(10).
											Justify(ui.Center).AlignItems(ui.Center)
										if t.color == col {
											cb.Border(1.5, k.Color("text-primary"))
										}
										cb.Children(func() {
											ui.Box(c).Size(9, 9).Radius(4.5).Background(k.SwatchColor(col))
										})
										if cb.Clicked() {
											d.setTagColor(id, col)
											m.colorPop = ""
										}
									}
								})
							}
						} else {
							ui.Row(c.Key("swatch-static")).Size(22, 22).Justify(ui.Center).AlignItems(ui.Center).Shrink(0).Children(func() {
								ui.Box(c).Size(9, 9).Radius(4.5).Background(k.SwatchColor(t.color))
							})
						}

						// Tag name or inline rename field.
						if m.editing == id {
							field := k.TextField(c.Key("editor"), desktopkit.TextFieldProps{
								Label:     d.tr("标签名称"),
								Value:     &m.name,
								AutoFocus: true,
								MaxLength: 64,
								Error:     d.tr(m.error),
							})
							field.Element.Grow(1).MinWidth(0)
							if field.Edited {
								m.error = ""
							}
							if field.Submitted {
								d.submitTagEdit()
							}
							if field.Escaped {
								m.editing, m.name, m.error = "", "", ""
							}
						} else {
							ui.Row(c.Key("name-col")).Gap(6).AlignItems(ui.Center).Grow(1).MinWidth(0).Children(func() {
								k.Text(c.Key("name"), "#"+d.tagLabel(id), desktopkit.TextProps{Emphasized: !t.builtin, Truncate: true}).MinWidth(0)
								badge := ""
								if t.builtin {
									badge = "内置"
								} else if count == 0 {
									badge = "未使用"
								}
								if badge != "" {
									ui.Row(c.Key("badge")).Height(18).Padding(0, 6).Radius(5).
										Justify(ui.Center).AlignItems(ui.Center).Shrink(0).
										Background(k.Color("background-tertiary-default")).Children(func() {
										k.Text(c, d.tr(badge), desktopkit.TextProps{Role: desktopkit.RoleCaption, Tone: desktopkit.ToneSecondary})
									})
								}
							})
						}

						// Inline quick actions revealed on hover or selection.
						if m.editing != id && m.colorPop != id && (hovered || isSelected) {
							ui.Row(c.Key("inline-acts")).Gap(2).AlignItems(ui.Center).Shrink(0).Children(func() {
								quickBtn := func(key, label string, icon desktopkit.IconName, danger bool) bool {
									b := ui.ButtonBase(c.Key(key)).Label(label).Tooltip(label).
										Size(24, 24).Radius(6).Justify(ui.Center).AlignItems(ui.Center)
									icColor := k.Color("foreground-icon-secondary")
									if b.Hovered() {
										if danger {
											b.Background(k.Color("background-tertiary-error"))
											icColor = k.Color("text-error-primary")
										} else {
											b.Background(k.Color("background-secondary-hover"))
											icColor = k.Color("foreground-icon-primary")
										}
									}
									b.Children(func() { k.Icon(c, icon, 14).TextColor(icColor).AccessHidden(true) })
									return b.Clicked()
								}
								if quickBtn("act-show", d.tr("显示条目"), desktopkit.IconSearch, false) {
									d.tagAction("show", []string{id})
								}
								if !t.builtin && d.live == nil {
									if quickBtn("act-rename", d.tr("重命名…"), desktopkit.IconEdit, false) {
										d.tagAction("rename", []string{id})
									}
									if quickBtn("act-del", d.tr("删除标签（保留条目）"), desktopkit.IconDelete, true) {
										d.tagAction("delete", []string{id})
									}
								}
							})
						}

						// Entry count + sidebar toggle switch.
						k.Text(c.Key("count"), fmt.Sprintf(d.tr("%d 项"), count), desktopkit.TextProps{Role: desktopkit.RoleCaption, Tone: desktopkit.ToneSecondary}).Shrink(0)
						if tag := d.tagByID(id); tag != nil {
							visible := tag.sidebar
							k.Switch(c.Key("sb"), &visible, desktopkit.SwitchProps{Label: d.tr("在侧边栏显示 #") + d.tagLabel(tag.id), Disabled: d.live != nil && d.live.busy})
							if visible != tag.sidebar {
								d.setTagSidebar(id, visible)
							}
						}
					})

					if row.Clicked() {
						m.colorPop = ""
						if c.Modifiers()&(ui.Cmd|ui.Ctrl) != 0 {
							if isSelected {
								m.selected = slices.DeleteFunc(m.selected, func(s string) bool { return s == id })
							} else {
								m.selected = append(m.selected, id)
							}
						} else {
							m.selected = []string{id}
						}
					}
				}

				if len(custom) > 0 {
					ui.Column(c.Key("sec-custom")).Gap(2).Shrink(0).Children(func() {
						ui.Row(c.Key("head-custom")).Padding(0, 10, 4, 10).AlignItems(ui.Center).Children(func() {
							k.Text(c, fmt.Sprintf("%s · %d", d.tr("我的标签"), len(custom)), desktopkit.TextProps{Role: desktopkit.RoleCaption, Tone: desktopkit.ToneSecondary, Emphasized: true}).Grow(1)
							k.Text(c, d.tr("点击色点改色 · 悬停或右键编辑"), desktopkit.TextProps{Role: desktopkit.RoleCaption, Tone: desktopkit.ToneTertiary})
						})
						for _, t := range custom {
							renderRow(t)
						}
					})
				}

				if len(builtin) > 0 {
					ui.Column(c.Key("sec-builtin")).Gap(2).Shrink(0).Children(func() {
						ui.Row(c.Key("head-builtin")).Padding(0, 10, 4, 10).AlignItems(ui.Center).Children(func() {
							k.Text(c, fmt.Sprintf("%s · %d", d.tr("系统类型"), len(builtin)), desktopkit.TextProps{Role: desktopkit.RoleCaption, Tone: desktopkit.ToneSecondary, Emphasized: true}).Grow(1)
							k.Text(c, d.tr("自动分类 · 可按需固定到侧边栏"), desktopkit.TextProps{Role: desktopkit.RoleCaption, Tone: desktopkit.ToneTertiary})
						})
						for _, t := range builtin {
							renderRow(t)
						}
					})
				}
			})
		})
	}) {
		d.tagsOpen = false
	}
}

// Hash IDs so the presentation-only configuration never contains user tag names.
func tagColorKey(id string) string {
	sum := sha256.Sum256([]byte(id))
	return hex.EncodeToString(sum[:])
}

func (d *viewState) setTagSidebar(id string, visible bool) {
	if d.live == nil {
		if tag := d.tagByID(id); tag != nil {
			tag.sidebar = visible
		}
		return
	}
	cfg := d.live.config.Config
	cfg.Desktop.TagSidebar = maps.Clone(cfg.Desktop.TagSidebar)
	if cfg.Desktop.TagSidebar == nil {
		cfg.Desktop.TagSidebar = map[string]bool{}
	}
	cfg.Desktop.TagSidebar[tagColorKey(id)] = visible
	d.saveLiveConfigThen(cfg, func() {
		if tag := d.tagByID(id); tag != nil {
			tag.sidebar = visible
		}
	})
}

func (d *viewState) setTagColor(id string, color desktopkit.TagColor) {
	tag := d.tagByID(id)
	if tag == nil || tag.builtin {
		return
	}
	if d.live == nil {
		tag.color = color
		return
	}
	cfg := d.live.config.Config
	cfg.Desktop.TagColors = maps.Clone(cfg.Desktop.TagColors)
	if cfg.Desktop.TagColors == nil {
		cfg.Desktop.TagColors = map[string]string{}
	}
	cfg.Desktop.TagColors[tagColorKey(id)] = string(color)
	d.saveLiveConfig(cfg)
}
