package uikit

import "github.com/egoist/mygo/ui"

// ItemListItem 是 ItemList / GroupedList 的一行（contract.ts ItemListItem）。
type ItemListItem struct {
	ID       string
	Title    string
	Subtitle string
	// Monospace 以等宽字体显示标题。
	Monospace bool
	// Leading 是行首视觉（类型徽标/缩略图），Trailing 是行尾内容（标签/置顶标记）。
	// 在行内构建（用同一个 *ui.Context）。
	Leading  func()
	Trailing func()
	// Highlight 是标题里要标出的搜索词（系统查找高亮）。
	Highlight []string
}

// ItemListProps 对应 contract.ts ItemListProps。选中集合与事件在 *ListState 里。
//
// 键盘（各平台一致）：Up/Down 移动、Home/End 跳转、Enter 或双击激活（st.Activated）、
// Delete 报告选区（st.DeleteRequested）。MultiSelect 下每行带勾选标记，Space 切换当前行；
// 点击怎么扩选随平台（macOS Cmd+点击加选，Windows/Linux Ctrl+点击）。
type ItemListProps struct {
	Label       string
	Items       []ItemListItem
	MultiSelect bool
	// Menu 为选区的右键菜单（nil 无菜单）；选中的条目由 st.MenuChosen 读出。
	Menu func(ids []string) []MenuEntry
}

// ItemList 是带 listbox 语义的可选列表。返回的元素可继续设尺寸（如 .Grow(1)）。
func (k Kit) ItemList(c *ui.Context, st *ListState, p ItemListProps) ui.Element {
	ids := make([]string, len(p.Items))
	for i, it := range p.Items {
		ids[i] = it.ID
	}
	return k.runList(c, st, listRun{
		label: p.Label, ids: ids, multi: p.MultiSelect, menu: p.Menu,
		text: func(i int) string { return p.Items[i].Title },
		row: func(r ui.ListRow, i int) {
			it := p.Items[i]
			sp := listRowSpec{title: it.Title, subtitle: it.Subtitle, mono: it.Monospace, highlight: it.Highlight,
				leading: it.Leading, trailing: it.Trailing, first: i == 0}
			if p.MultiSelect {
				on := r.Selected()
				sp.check = &on
			}
			k.listRow(r.Context, r, sp)
		},
	})
}

// listRun 描述一次列表构建。
type listRun struct {
	label  string
	ids    []string // 行 -> 条目 id；"" 表示分组头/占位
	header func(int) bool
	multi  bool
	menu   func(ids []string) []MenuEntry
	text   func(i int) string
	row    func(r ui.ListRow, i int)
	// loaded 是真实行的区间 [lo,hi)；hi>lo 时，键盘选到区间外的占位行会被撤销并转成 MoveRequest。
	loaded [2]int
	// abs 把行号映射为窗口化列表的绝对条目序号（nil 则不报告 VisibleRange）。
	abs func(row int) int
}

// runList 用 mygo 的虚拟 List 构建列表：只构建可见行；选择/键盘由 mygo 处理，
// 皮肤色通过覆盖主题注入。
func (k Kit) runList(c *ui.Context, st *ListState, r listRun) ui.Element {
	st.begin()
	st.bind(r.ids, r.header)
	st.list.Label = r.text
	if !r.multi {
		st.list.Selection = nil
		// 单选：*Selected 是唯一选中行；把对外的集合与之对齐。
		if st.cur < 0 || st.cur >= len(r.ids) || r.ids[st.cur] == "" {
			st.cur = -1
			if len(st.ids) > 0 {
				for id := range st.sel.All() {
					if row, ok := st.rowOf[id]; ok {
						st.cur = row
					}
				}
			}
		}
	}
	restore := k.withListTheme(c)
	e := ui.List(c, &st.list, len(r.ids)).Rows(func(lr ui.ListRow) { r.row(lr, lr.Index) })
	restore()
	e.Label(r.label)
	if !r.multi {
		st.sel.Clear()
		if st.cur >= 0 && st.cur < len(r.ids) && r.ids[st.cur] != "" {
			st.sel.Add(r.ids[st.cur])
		}
	} else if e.Shortcut(0, ui.KeySpace) && st.cur >= 0 && st.cur < len(r.ids) && r.ids[st.cur] != "" {
		id := r.ids[st.cur]
		if st.sel.Has(id) {
			st.sel.Remove(id)
		} else {
			st.sel.Add(id)
		}
	}
	if r.loaded[1] > r.loaded[0] {
		st.scrubPlaceholders(r.loaded[0], r.loaded[1])
	}
	st.settle(e, k)
	st.visible(len(r.ids), r.loaded, r.abs)
	st.listContextMenu(e, r.menu)
	return e
}
