package desktopdemo

import (
	"fmt"
	"slices"
	"strings"

	"github.com/egoist/mygo/ui"

	"github.com/uniclipboard/unic/app/desktop/internal/uikit"
)

type demoTag struct {
	id, name         string
	color            uikit.TagColor
	builtin, sidebar bool
}
type tagManagerState struct {
	filter, sort, editing, name, error string
	next                               int
	table                              uikit.ListState
}

func (d *demo) initTags() {
	d.tags = []demoTag{{"工作", "工作", "blue", false, true}, {"设计", "设计", "purple", false, true}, {"开发", "开发", "green", false, true}, {"custom:0", "自定义标签", "orange", false, false}, {"link", "链接", "gray", true, true}, {"code", "代码", "gray", true, true}, {"image", "图片", "gray", true, true}, {"directory", "文件夹", "gray", true, true}}
	d.tagManager.sort = "mostUsed"
}
func (d *demo) tagByID(id string) *demoTag {
	for i := range d.tags {
		if d.tags[i].id == id {
			return &d.tags[i]
		}
	}
	return nil
}
func (d *demo) tagLabel(id string) string {
	if t := d.tagByID(id); t != nil {
		return t.name
	}
	return id
}
func (d *demo) tagColor(id string) uikit.TagColor {
	if t := d.tagByID(id); t != nil {
		return t.color
	}
	return "gray"
}
func (d *demo) entryHasTag(e entry, id string) bool {
	switch id {
	case "link":
		return e.kind == uikit.KindLink
	case "code":
		return e.kind == uikit.KindCode
	case "image":
		return e.kind == uikit.KindImage
	case "directory":
		return false
	}
	return slices.Contains(e.tags, id)
}
func (d *demo) tagCount(id string) int {
	n := 0
	for _, e := range d.entries {
		if d.entryHasTag(e, id) {
			n++
		}
	}
	return n
}
func (d *demo) localTagIDs(ids []string) []string {
	var out []string
	for _, id := range ids {
		if tag := d.tagByID(id); tag != nil && !tag.builtin {
			out = append(out, id)
		}
	}
	return out
}

func (d *demo) removeTags(ids []string, target string) {
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
	d.tags = slices.DeleteFunc(d.tags, func(tag demoTag) bool { return slices.Contains(ids, tag.id) })
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
	d.tagManager.table.Select()
}

func (d *demo) tagMenu(ids []string) []uikit.MenuEntry {
	own := d.localTagIDs(ids)
	var out []uikit.MenuEntry
	var single *demoTag
	if len(ids) == 1 {
		single = d.tagByID(ids[0])
	}
	if single != nil && !single.builtin {
		out = append(out, uikit.MenuEntry{ID: "rename", Label: "重命名…", Icon: uikit.IconEdit})
	}
	if len(own) > 0 {
		var targets []uikit.MenuEntry
		for _, tag := range d.tags {
			if !tag.builtin && !slices.Contains(ids, tag.id) {
				targets = append(targets, uikit.MenuEntry{ID: "merge:" + tag.id, Label: "#" + tag.name})
			}
		}
		if len(targets) > 0 {
			out = append(out, uikit.MenuEntry{ID: "merge", Label: "合并到", Children: targets})
		}
	}
	if single != nil {
		out = append(out, uikit.MenuEntry{ID: "show", Label: "显示条目", Icon: uikit.IconSearch})
	}
	if single != nil && !single.builtin {
		out = append(out, uikit.MenuSeparator("colors"))
		for _, v := range [][2]string{{"orange", "橙色"}, {"blue", "蓝色"}, {"green", "绿色"}, {"purple", "紫色"}, {"gray", "灰色"}} {
			out = append(out, uikit.MenuEntry{ID: "color:" + v[0], Label: v[1], Swatch: uikit.TagColor(v[0]), Checked: single.color == uikit.TagColor(v[0])})
		}
	}
	if len(own) > 0 {
		label := "删除标签（保留条目）"
		if len(own) > 1 {
			label = fmt.Sprintf("删除 %d 个标签（保留条目）", len(own))
		}
		out = append(out, uikit.MenuSeparator("delete-separator"), uikit.MenuEntry{ID: "delete", Label: label, Icon: uikit.IconDelete, Destructive: true})
	}
	return out
}

func (d *demo) tagAction(action string, ids []string) {
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
			tag.color = uikit.TagColor(strings.TrimPrefix(action, "color:"))
		}
	}
}
func (d *demo) submitTagEdit() {
	m := &d.tagManager
	name := strings.TrimSpace(m.name)
	if name == "" {
		m.editing, m.error = "", ""
		return
	}
	for _, tag := range d.tags {
		if tag.id != m.editing && strings.EqualFold(tag.name, name) {
			m.error = "已存在同名标签"
			return
		}
	}
	if m.editing == "new" {
		m.next++
		id := fmt.Sprintf("custom:%d", m.next)
		d.tags = append(d.tags, demoTag{id: id, name: name, color: "orange"})
		m.table.Select(id)
	} else if tag := d.tagByID(m.editing); tag != nil && !tag.builtin {
		tag.name = name
	}
	m.editing, m.name, m.error = "", "", ""
}

func (d *demo) tagSheet(c *ui.Context, k uikit.Kit) {
	m := &d.tagManager
	shown := slices.Clone(d.tags)
	shown = slices.DeleteFunc(shown, func(tag demoTag) bool {
		return !strings.Contains(strings.ToLower(tag.name), strings.ToLower(strings.TrimSpace(m.filter)))
	})
	slices.SortStableFunc(shown, func(a, b demoTag) int {
		if a.builtin != b.builtin {
			if a.builtin {
				return 1
			}
			return -1
		}
		if a.builtin {
			return 0
		}
		switch m.sort {
		case "mostUsed":
			if n := d.tagCount(b.id) - d.tagCount(a.id); n != 0 {
				return n
			}
		case "newest":
			return slices.IndexFunc(d.tags, func(t demoTag) bool { return t.id == b.id }) - slices.IndexFunc(d.tags, func(t demoTag) bool { return t.id == a.id })
		}
		return strings.Compare(strings.ToLower(a.name), strings.ToLower(b.name))
	})
	header := func() {
		k.Text(c.Key("subtitle"), fmt.Sprintf("标签 · %d", len(d.tags)), uikit.TextProps{Role: uikit.RoleCallout, Tone: uikit.ToneSecondary}).Shrink(0)
		ui.Spacer(c)
		k.Select(c.Key("sort"), uikit.SelectProps{Label: "标签排序", Glass: true, Value: &m.sort, Items: []uikit.SelectItem{{Value: "mostUsed", Label: "最常用"}, {Value: "name", Label: "名称"}, {Value: "newest", Label: "最新"}}}).Element.Shrink(0)
		k.SearchField(c.Key("filter"), uikit.SearchFieldProps{Label: "筛选标签", Value: &m.filter, Placeholder: fmt.Sprintf("在 %d 个标签中筛选", len(d.tags))}).Element.Grow(1).MinWidth(80)
	}
	foot := func() {
		if k.Button(c.Key("add"), "", uikit.ButtonProps{Icon: uikit.IconAdd, Label: "新建标签"}).Clicked() {
			m.editing, m.name, m.error = "new", "", ""
			m.filter = ""
		}
		if k.Button(c.Key("delete"), "", uikit.ButtonProps{Icon: uikit.IconDelete, Label: "删除选中标签", Disabled: len(d.localTagIDs(m.table.Selection())) == 0}).Clicked() {
			d.removeTags(m.table.Selection(), "")
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
		k.Text(c.Key("summary"), fmt.Sprintf("%d 个标签 · %d 个在侧边栏 · %d 个未使用", len(d.tags), sidebar, unused), uikit.TextProps{Role: uikit.RoleCaption, Tone: uikit.ToneSecondary}).MinWidth(0)
	}
	if k.Sheet(c, uikit.SheetProps{Open: &d.tagsOpen, Title: "管理", Header: header, Footer: foot, Done: "完成", OnClosed: func() {
		m.table.Select()
		m.editing, m.error = "", ""
	}}, func() {
		var rows []uikit.TableRow
		nameCell := func(id string) func(*ui.Context) {
			return func(rc *ui.Context) {
				if m.editing == id {
					label := "标签名称"
					if id == "new" {
						label = "新标签名称"
					}
					field := k.TextField(rc.Key("editor"), uikit.TextFieldProps{Label: label, Value: &m.name, AutoFocus: true, MaxLength: 64, Error: m.error})
					field.Element.Grow(1).MinWidth(0)
					if field.Edited {
						m.error = ""
					}
					if field.Submitted {
						d.submitTagEdit()
					}
					if field.Escaped {
						m.editing, m.error = "", ""
					}
					return
				}
				if tag := d.tagByID(id); tag != nil {
					ui.Box(rc.Key("swatch")).Size(8, 8).Radius(4).Shrink(0).Background(k.SwatchColor(tag.color))
					ui.Text(rc.Key("name"), "#"+tag.name).SingleLine()
					status := ""
					if tag.builtin {
						status = "内置"
					} else if d.tagCount(id) == 0 {
						status = "未使用"
					}
					if status != "" {
						k.Text(rc.Key("status"), status, uikit.TextProps{Role: uikit.RoleCaption, Tone: uikit.ToneSecondary}).SingleLine()
					}
				}
			}
		}
		if m.editing == "new" {
			rows = append(rows, uikit.TableRow{ID: "new", Cells: map[string]func(*ui.Context){"name": nameCell("new")}})
		}
		for _, t := range shown {
			id := t.id
			rows = append(rows, uikit.TableRow{ID: id, Cells: map[string]func(*ui.Context){"name": nameCell(id), "items": func(rc *ui.Context) { ui.Text(rc, fmt.Sprint(d.tagCount(id))) }, "sidebar": func(rc *ui.Context) {
				if tag := d.tagByID(id); tag != nil {
					k.Switch(rc, &tag.sidebar, uikit.SwitchProps{Label: "在侧边栏显示 #" + tag.name, LabelHidden: true})
				}
			}}})
		}
		height := float32(32 + len(rows)*48)
		if k.Skin == uikit.MacOS {
			height = float32(26 + len(rows)*32)
		}
		k.Table(c.Key("table"), &m.table, uikit.TableProps{Label: "标签", FixedColumns: true, Columns: []uikit.TableColumn{{ID: "name", Label: "名称", Fill: true}, {ID: "items", Label: "条目", Width: 80, Align: ui.End}, {ID: "sidebar", Label: "侧边栏", Width: 64, Align: ui.Center}}, Rows: rows, Menu: d.tagMenu, Empty: "没有匹配的标签"}).Height(min(360, height)).MinHeight(70)
		if action, ids, ok := m.table.MenuChosen(); ok {
			d.tagAction(action, ids)
		}
		if ids, ok := m.table.DeleteRequested(); ok && m.editing == "" {
			d.removeTags(ids, "")
		}
	}) {
		d.tagsOpen = false
	}
}
