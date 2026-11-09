package uikit

import "github.com/egoist/mygo/ui"

// ListGroup 是 GroupedList 里一段连续条目的标题（contract.ts ListGroup）。
type ListGroup struct {
	ID    string
	Title string
	// Detail 是标题后的次要文字，如日期。
	Detail string
	// Trailing 是右对齐文字，如 "5 items"。
	Trailing string
	// Count 是其后属于本组的条目数。
	Count int
}

// HistoryWindowViewport 描述一个窗口化（惰性加载）的结果集：Items 只是其中 [Offset,
// Offset+len(Items)) 这一窗，窗外按 Total 用占位行撑出滚动高度。可见区间、翻页请求与键盘
// 越界请求从 ListState 读出（VisibleRange / Navigate / MoveRequest）。
type HistoryWindowViewport struct {
	// Total 是渲染的结果总数（含仅头部的待传输行）。
	Total int
	// Offset 是 Items[0] 在结果集里的绝对序号。
	Offset int
	// ResetKey 变化表示新的查询：不继承旧结果的锚点。
	ResetKey string
	// ScrollToTop 单调递增的命令版本：复制后把结果头滚回顶部。
	ScrollToTop int
	// Reveal 非空时在同一次布局里把该条目滚到可见。
	Reveal string
}

// GroupedListProps 对应 contract.ts GroupedListProps。
//
// 只构建可见行（mygo 虚拟 List，行高可变）。键盘：Up/Down 移动选中，Shift 扩选，Home/End 跳转，
// 平台全选键选中所有已加载行，Enter 激活（st.Activated），平台删除键报告选区（st.DeleteRequested）。
// 指针：点击选一行，平台修饰键加/减行，Shift 选区间。多选的呈现随平台（macOS 无勾选框，
// Windows 多选时每行首带勾选）。分组标题在顶部粘性停靠。
type GroupedListProps struct {
	Viewport *HistoryWindowViewport
	Label    string
	Groups   []ListGroup
	Items    []ItemListItem
	// Menu 为选区（或被右键且未选中的那一行）的右键菜单；选中的条目读 st.MenuChosen。
	Menu func(ids []string) []MenuEntry
	// Footer 画在最后一行之后（加载指示/提示）；近似为列表下方固定的一条，不随行滚动。
	Footer func()
}

type glRowKind uint8

const (
	glItem glRowKind = iota
	glHeader
	glHole // 窗口外的占位行
)

// GroupedList 是分组的长列表（历史列表）。返回列表元素（含 Footer 的外壳），可设 .Grow(1)。
func (k Kit) GroupedList(c *ui.Context, st *ListState, p GroupedListProps) ui.Element {
	lk := k.listLook()
	var (
		ids     []string
		kinds   []glRowKind
		itemAt  []int // 行 -> Items 下标（头/占位为 -1）
		groupAt []int
		abs     []int
	)
	off, total := 0, 0
	if p.Viewport != nil {
		off, total = max(p.Viewport.Offset, 0), p.Viewport.Total
	}
	hole := func(abside int) {
		ids = append(ids, "")
		kinds = append(kinds, glHole)
		itemAt = append(itemAt, -1)
		groupAt = append(groupAt, -1)
		abs = append(abs, abside)
	}
	for i := 0; i < off && p.Viewport != nil; i++ {
		hole(i)
	}
	loLoaded := len(ids)
	next := 0
	for gi, g := range p.Groups {
		n := min(g.Count, len(p.Items)-next)
		if n <= 0 && g.Count <= 0 {
			continue
		}
		ids = append(ids, "")
		kinds = append(kinds, glHeader)
		itemAt = append(itemAt, -1)
		groupAt = append(groupAt, gi)
		abs = append(abs, off+next)
		for j := 0; j < n; j++ {
			ids = append(ids, p.Items[next+j].ID)
			kinds = append(kinds, glItem)
			itemAt = append(itemAt, next+j)
			groupAt = append(groupAt, gi)
			abs = append(abs, off+next+j)
		}
		next += n
	}
	// 没有分组时，条目直接平铺。
	for ; next < len(p.Items); next++ {
		ids = append(ids, p.Items[next].ID)
		kinds = append(kinds, glItem)
		itemAt = append(itemAt, next)
		groupAt = append(groupAt, -1)
		abs = append(abs, off+next)
	}
	hiLoaded := len(ids)
	if p.Viewport != nil {
		for i := off + len(p.Items); i < total; i++ {
			hole(i)
		}
	}
	if p.Viewport == nil {
		loLoaded, hiLoaded = 0, 0
	}
	st.vpCommands(p.Viewport)

	shell := ui.Column(c)
	shell.Children(func() {
		list := k.runList(c, st, listRun{
			label: p.Label, ids: ids, multi: true, menu: p.Menu,
			header: func(i int) bool { return i >= 0 && i < len(kinds) && kinds[i] == glHeader },
			text: func(i int) string {
				if it := itemAt[i]; it >= 0 {
					return p.Items[it].Title
				}
				return ""
			},
			loaded: [2]int{loLoaded, hiLoaded},
			abs: func(r int) int {
				if r < 0 || r >= len(abs) {
					return 0
				}
				return abs[r]
			},
			row: func(r ui.ListRow, i int) {
				switch kinds[i] {
				case glHeader:
					k.groupHeader(r.Context, p.Groups[groupAt[i]])
				case glHole:
					ui.Box(r.Context).Height(lk.rowH)
				default:
					k.GroupedListRow(r, p.Items[itemAt[i]], st.sel.Len() > 1)
				}
			},
		})
		// Reserve a separate right gutter for the overlay scrollbar.
		list.Grow(1).Padding(0, lk.cellPadX+12, 0, lk.cellPadX)
		if lk.rowGap > 0 {
			list.Gap(lk.rowGap)
		}
		if p.Footer != nil {
			k.GroupedListFooter(c, p.Footer)
		}
	})
	return shell.Grow(1)
}

// vpCommands 处理窗口化命令：新查询重置、回到顶部、定位到条目。
func (s *ListState) vpCommands(vp *HistoryWindowViewport) {
	if vp == nil {
		return
	}
	if vp.ResetKey != s.vpReset {
		s.vpReset = vp.ResetKey
		s.cur = -1
		s.list.ScrollTo(0, ui.Start)
	}
	if vp.ScrollToTop != s.vpTop {
		s.vpTop = vp.ScrollToTop
		s.list.ScrollTo(0, ui.Start)
	}
	if vp.Reveal != "" && vp.Reveal != s.vpReveal {
		s.vpReveal = vp.Reveal
		if r, ok := s.rowOf[vp.Reveal]; ok {
			s.list.ScrollIntoView(r)
		}
	} else if vp.Reveal == "" {
		s.vpReveal = ""
	}
}

// groupHeader 画分组标题：粗体标题 + 次要详情，右侧对齐的计数文字；底色不透明以便粘性停靠。
func (k Kit) groupHeader(c *ui.Context, g ListGroup) {
	lk := k.listLook()
	h := ui.Row(c).AlignItems(ui.Center).Gap(8).Height(lk.headerH).Padding(0, lk.padX).Background(k.surfaceSolid())
	h.Role(ui.RoleNone)
	h.Children(func() {
		ui.Text(c, g.Title).FontSize(lk.titleSize - 1).FontWeight(600).TextColor(k.Sem("foreground")).SingleLine()
		if g.Detail != "" {
			ui.Text(c, g.Detail).FontSize(lk.metaSize).TextColor(k.Sem("mutedForeground")).SingleLine()
		}
		ui.Spacer(c)
		if g.Trailing != "" {
			col := k.Sem("mutedForeground")
			if k.Skin == MacOS {
				col = k.Color("label-3")
			}
			ui.Text(c, g.Trailing).FontSize(lk.metaSize).TextColor(col).SingleLine()
		}
	})
}

// GroupedListRow 画 GroupedList 的一条行；r 来自 mygo 列表的行回调。multi 为真时（Windows）
// 每行首带勾选标记代替强调条。独立导出以便自建列表复用同样的行外观。
func (k Kit) GroupedListRow(r ui.ListRow, item ItemListItem, multi bool) ui.Element {
	sp := listRowSpec{title: item.Title, subtitle: item.Subtitle, mono: item.Monospace,
		highlight: item.Highlight, leading: item.Leading, trailing: item.Trailing, first: r.Index == 0}
	if multi && k.Skin == Windows {
		on := r.Selected()
		sp.check = &on
	}
	// 分组头紧邻的第一行不画分隔线。
	if k.Skin == Linux {
		sp.first = true
	}
	return k.listRow(r.Context, r, sp)
}

// GroupedListFooter 是最后一行之后的内容（加载指示/提示），居中的小号次要文字区。
func (k Kit) GroupedListFooter(c *ui.Context, body func()) ui.Element {
	lk := k.listLook()
	f := ui.Row(c).Justify(ui.Center).AlignItems(ui.Center).Padding(8, 16).Shrink(0)
	f.TextColor(k.Sem("mutedForeground")).FontSize(lk.metaSize)
	f.Children(body)
	return f
}
