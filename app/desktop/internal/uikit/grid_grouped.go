package uikit

import (
	"math"

	"github.com/egoist/mygo/ui"
)

// gridSel 是 GroupedGrid 的专有状态（嵌在 ListState 里）。
type gridSel struct {
	cols   int
	width  float32
	anchor int      // Shift 扩选的起点（条目序号）
	rowOf  []int    // 条目序号 -> 可视行号
	hover  string   // 本帧被指针悬停的卡片（右键菜单作用对象）
	ids    []string // 条目序号 -> id
}

// GridItem 是 GroupedGrid 的一张卡片（contract.ts GridItem）。
type GridItem struct {
	ID       string
	Title    string
	Subtitle string
	// Kind 决定卡片主体画什么：图片（Media）、深色代码片段、链接或文本。
	Kind ContentKind
	// Text 是主体里的文字（代码/链接/文本）。
	Text string
	// Media 构建图片主体（如缩略图元素）。
	Media func()
	// Badges 画在卡片上的小标记（如置顶）。
	Badges func()
}

// GroupedGridProps 对应 contract.ts GroupedGridProps。语义同 GroupedList：整个网格是一个
// Tab 停靠点，方向键在二维里移动，Shift 扩选，全选键选中已加载卡片，Enter 激活，平台删除键
// 报告选区；事件从 *ListState 读出。
type GroupedGridProps struct {
	Viewport *HistoryWindowViewport
	Label    string
	Groups   []ListGroup
	Items    []GridItem
	// Size 是工具栏滑块给出的卡片尺寸，0 最小到 1 最大。
	Size   float32
	Menu   func(ids []string) []MenuEntry
	Footer func()
}

type ggRow struct {
	header int // >=0：分组头（Groups 下标）
	first  int // 卡片行：首个条目序号；-1 为占位
	n      int
}

// GroupedGrid 是分组的卡片网格（Finder 图标视图）。
func (k Kit) GroupedGrid(c *ui.Context, st *ListState, p GroupedGridProps) ui.Element {
	lk := k.listLook()
	st.begin()
	g := &st.grid
	size := float32(math.Max(0, math.Min(1, float64(p.Size))))
	cardMin := 120 + 100*size
	gap := float32(12)
	width := g.width
	if width <= 0 {
		width = 640
	}
	cols := max(1, int(math.Floor(float64((width+gap)/(cardMin+gap)))))
	g.cols = cols
	cardH := cardMin*0.62 + 44
	if g.cols == 0 {
		g.cols = 1
	}

	off := 0
	if p.Viewport != nil {
		off = max(p.Viewport.Offset, 0)
	}
	var rows []ggRow
	holes := func(n int) {
		for n > 0 {
			rows = append(rows, ggRow{header: -1, first: -1, n: min(cols, n)})
			n -= cols
		}
	}
	if p.Viewport != nil {
		holes(off)
	}
	loLoaded := len(rows)
	g.rowOf = g.rowOf[:0]
	g.ids = g.ids[:0]
	for _, it := range p.Items {
		g.ids = append(g.ids, it.ID)
		g.rowOf = append(g.rowOf, 0)
	}
	next := 0
	cardRows := func(count int) {
		for count > 0 {
			n := min(cols, count)
			for j := 0; j < n; j++ {
				g.rowOf[next+j] = len(rows)
			}
			rows = append(rows, ggRow{header: -1, first: next, n: n})
			next += n
			count -= n
		}
	}
	for gi, grp := range p.Groups {
		n := min(grp.Count, len(p.Items)-next)
		if n <= 0 && grp.Count <= 0 {
			continue
		}
		rows = append(rows, ggRow{header: gi, first: -1})
		cardRows(max(n, 0))
	}
	cardRows(len(p.Items) - next)
	hiLoaded := len(rows)
	if p.Viewport != nil {
		holes(p.Viewport.Total - off - len(p.Items))
	} else {
		loLoaded, hiLoaded = 0, 0
	}

	// 行 -> id（用于 mygo 列表的稳定 key）；网格自己管理选择，所以 mygo 列表不选行。
	keys := make([]string, len(rows))
	for i, r := range rows {
		switch {
		case r.header >= 0:
			keys[i] = "\x00h" + p.Groups[r.header].ID
		case r.first >= 0:
			keys[i] = "\x00c" + p.Items[r.first].ID
		default:
			keys[i] = "\x00hole" + itoaPos(i)
		}
	}
	st.ids = nil
	if st.rowOf == nil {
		st.rowOf = map[string]int{}
	}
	clear(st.rowOf)
	for i, it := range p.Items {
		st.rowOf[it.ID] = i
	}
	st.list.Selected, st.list.Selection = nil, nil
	st.list.Key = func(i int) any {
		if i >= 0 && i < len(keys) {
			return keys[i]
		}
		return i
	}
	st.list.Header = func(i int) bool { return i >= 0 && i < len(rows) && rows[i].header >= 0 }
	st.list.Label = nil
	// vpCommands consumes item indexes; translate a new reveal to the grid's visual row.
	reveal := p.Viewport != nil && p.Viewport.Reveal != "" && p.Viewport.Reveal != st.vpReveal
	st.vpCommands(p.Viewport)
	if reveal {
		if i, ok := st.rowOf[p.Viewport.Reveal]; ok {
			st.list.ScrollIntoView(g.rowOf[i])
		}
	}
	if st.cur >= len(p.Items) {
		st.cur = len(p.Items) - 1
	}
	g.hover = ""

	shell := ui.Column(c)
	shell.Children(func() {
		restore := k.withListTheme(c)
		e := ui.List(c, &st.list, len(rows)).Rows(func(r ui.ListRow) {
			row := rows[r.Index]
			rc := r.Context
			switch {
			case row.header >= 0:
				k.groupHeader(rc, p.Groups[row.header])
			default:
				k.gridCardRow(rc, st, p, row, cols, cardH, gap)
			}
		})
		restore()
		e.Label(p.Label).Grow(1).Padding(0, lk.cellPadX).Focusable()
		e.Multiselectable(true)
		g.width = max(e.Bounds().W-2*lk.cellPadX, 0)
		k.gridKeys(e, st, p, cols)
		st.gridSettle(e)
		st.visibleGrid(len(rows), [2]int{loLoaded, hiLoaded}, rows, off)
		if p.Menu != nil {
			e.ContextMenu(func(m *ui.Menu) {
				if h := g.hover; h != "" && !st.sel.Has(h) {
					st.sel.Clear()
					st.sel.Add(h)
				}
				ids := st.Selection()
				if len(ids) == 0 {
					return
				}
				var chosen string
				menuFromEntries(m, p.Menu(ids), &chosen)
				if chosen != "" {
					st.menuPick, st.menuIDs, st.menuB = chosen, ids, true
				}
			})
		}
		if p.Footer != nil {
			k.GroupedListFooter(c, p.Footer)
		}
	})
	return shell.Grow(1)
}

func itoaPos(i int) string {
	return string(rune('0'+i%10)) + func() string {
		if i >= 10 {
			return itoaPos(i / 10)
		}
		return ""
	}()
}

// gridCardRow 画一行卡片（cols 张，不足补空位使卡片等宽）。
func (k Kit) gridCardRow(c *ui.Context, st *ListState, p GroupedGridProps, row ggRow, cols int, cardH, gap float32) {
	r := ui.Row(c).Gap(gap).PaddingY(gap / 2).AlignItems(ui.Stretch).Role(ui.RoleNone)
	r.Children(func() {
		if row.first < 0 { // 占位行
			for j := 0; j < cols; j++ {
				ui.Box(c.Key(j)).Grow(1).Basis(0).MinWidth(0).Height(cardH)
			}
			return
		}
		for j := 0; j < row.n; j++ {
			i := row.first + j
			k.gridCard(c.Key(p.Items[i].ID), st, p.Items[i], i, cardH)
		}
		for j := row.n; j < cols; j++ {
			ui.Box(c.Key("pad" + itoaPos(j))).Grow(1).Basis(0).MinWidth(0)
		}
	})
}

func (k Kit) gridCard(c *ui.Context, st *ListState, it GridItem, i int, cardH float32) {
	selected := st.sel.Has(it.ID)
	active := c.WindowActive()
	rad := float32(10)
	switch k.Skin {
	case Windows:
		rad = k.Px("borderRadiusLarge")
	case Linux:
		rad = 12
	}
	card := ui.Column(c).Grow(1).Basis(0).MinWidth(0).Height(cardH).Radius(rad).Clip().
		Background(k.surfaceSolid()).Label(it.Title)
	card.Role(ui.RoleListItem)
	card.AccessSelected(selected)
	card.Border(1, k.Sem("border"))
	if card.Hovered() {
		st.grid.hover = it.ID
		card.Background(k.Sem("accent").MixOklab(k.surfaceSolid(), .5))
	}
	ring := k.Sem("primary")
	if k.Skin == MacOS && selected && !active {
		ring = k.Color("selection-off")
	}
	card.DrawOver(func(p *ui.Painter, rc ui.Rect) {
		if selected {
			p.Stroke(rc, ring, rad, 2.5)
		}
	})
	card.Children(func() {
		body := ui.Box(c).Grow(1).Basis(0).MinHeight(0).Clip()
		switch it.Kind {
		case KindImage:
			body.Background(k.Color(k.thumbToken()))
			body.Children(func() {
				if it.Media != nil {
					it.Media()
				} else {
					ui.Row(c).Grow(1).Justify(ui.Center).AlignItems(ui.Center).Children(func() {
						k.Icon(c, IconFile, 24).TextColor(k.Sem("mutedForeground"))
					})
				}
			})
		case KindCode:
			body.Background(k.Color(k.codeBgToken())).Padding(8)
			body.Children(func() {
				ui.Text(c, it.Text).Font("monospace").FontSize(11).TextColor(k.codeInk()).MaxLines(4)
			})
		case KindLink:
			body.Background(k.Sem("secondary")).Padding(8)
			body.Children(func() {
				ui.Text(c, it.Text).FontSize(12).TextColor(k.Sem("primary")).Underline().MaxLines(3)
			})
		default:
			body.Background(k.Sem("secondary")).Padding(8)
			body.Children(func() {
				ui.Text(c, it.Text).FontSize(12).TextColor(k.Sem("foreground")).MaxLines(4)
			})
		}
		if it.Badges != nil {
			badges := ui.Row(c).Absolute().Top(6).Right(6).Gap(4)
			badges.Children(it.Badges)
		}
		cap := ui.Column(c).Padding(6, 8).Gap(1).Shrink(0)
		cap.Children(func() {
			tc, mc := k.Sem("foreground"), k.Sem("mutedForeground")
			ui.Text(c, it.Title).FontSize(12).TextColor(tc).SingleLine()
			if it.Subtitle != "" {
				ui.Text(c, it.Subtitle).FontSize(10.5).TextColor(mc).SingleLine()
			}
		})
	})
	// 点击选择：普通点击单选，平台修饰键加/减，Shift 选区间。
	if card.Clicked() {
		mods := card.ClickModifiers()
		switch {
		case mods&ui.Shift != 0:
			st.gridRange(st.grid.anchor, i)
		case mods&ui.Cmd != 0 || mods&ui.Ctrl != 0 && k.Skin != MacOS:
			if st.sel.Has(it.ID) {
				st.sel.Remove(it.ID)
			} else {
				st.sel.Add(it.ID)
			}
			st.grid.anchor = i
		default:
			st.sel.Clear()
			st.sel.Add(it.ID)
			st.grid.anchor = i
		}
		st.cur = i
	}
	if card.DoubleClicked() {
		st.activated, st.activatedB = it.ID, true
	}
}

func (k Kit) thumbToken() string {
	switch k.Skin {
	case MacOS:
		return "thumb"
	case Windows:
		return "w11-checker"
	}
	return "thumb"
}

func (k Kit) codeBgToken() string {
	switch k.Skin {
	case MacOS:
		return "code-bg"
	case Windows:
		return "w11-code-bg"
	}
	return "code"
}

func (k Kit) codeInk() ui.Color {
	if k.Skin == Linux {
		return k.Color("code-ink")
	}
	return ui.RGBA(230, 233, 238, 255)
}

// gridRange 把选区设为 anchor..to（含）。
func (s *ListState) gridRange(from, to int) {
	if from < 0 || from >= len(s.grid.ids) {
		from = to
	}
	lo, hi := min(from, to), max(from, to)
	s.sel.Clear()
	for j := lo; j <= hi && j < len(s.grid.ids); j++ {
		s.sel.Add(s.grid.ids[j])
	}
}

// gridKeys 处理二维方向键、Home/End、全选、Enter、删除。
func (k Kit) gridKeys(e ui.Element, st *ListState, p GroupedGridProps, cols int) {
	n := len(p.Items)
	if n == 0 {
		return
	}
	if st.cur < 0 {
		st.cur = -1
	}
	move := func(to int, extend bool) {
		to = max(0, min(n-1, to))
		if extend {
			st.gridRange(st.grid.anchor, to)
		} else {
			st.sel.Clear()
			st.sel.Add(st.grid.ids[to])
			st.grid.anchor = to
		}
		st.cur = to
		st.list.ScrollIntoView(st.grid.rowOf[to])
	}
	for _, ext := range []bool{false, true} {
		var mods ui.Modifiers
		if ext {
			mods = ui.Shift
		}
		base := st.cur
		if base < 0 {
			base = 0
		}
		switch {
		case e.Shortcut(mods, ui.KeyRight):
			move(base+b2i(st.cur >= 0), ext)
		case e.Shortcut(mods, ui.KeyLeft):
			move(base-1, ext)
		case e.Shortcut(mods, ui.KeyDown):
			move(base+cols*b2i(st.cur >= 0), ext)
		case e.Shortcut(mods, ui.KeyUp):
			move(base-cols, ext)
		case e.Shortcut(mods, ui.KeyHome):
			move(0, ext)
		case e.Shortcut(mods, ui.KeyEnd):
			move(n-1, ext)
		}
	}
	if e.Shortcut(ui.Cmd, ui.KeyA) {
		st.sel.Clear()
		for _, id := range st.grid.ids {
			st.sel.Add(id)
		}
	}
	if e.Shortcut(0, ui.KeyEnter) && st.cur >= 0 && st.cur < n {
		st.activated, st.activatedB = st.grid.ids[st.cur], true
	}
	if k.deleteKey(e) {
		if sel := st.Selection(); len(sel) > 0 {
			st.deleted, st.deletedB = sel, true
		}
	}
}

func b2i(b bool) int {
	if b {
		return 1
	}
	return 0
}

// gridSettle 在网格构建后计算选区变化。
func (s *ListState) gridSettle(e ui.Element) {
	// Selection() 依赖 s.ids（行 -> id）；网格用条目序号表。
	s.ids = s.grid.ids
	if now := s.selKey(); now != s.prevSel {
		s.prevSel, s.changed = now, true
	}
}

// visibleGrid 记录可见条目区间、翻页请求与“滚到末尾”边沿。
func (s *ListState) visibleGrid(n int, loaded [2]int, rows []ggRow, off int) {
	if n == 0 {
		s.first, s.last = 0, -1
		return
	}
	f, l := s.list.Visible()
	s.first, s.last = off, off
	for i := f; i <= l && i < n; i++ {
		if rows[i].first >= 0 {
			s.first = off + rows[i].first
			break
		}
	}
	for i := l; i >= f && i >= 0; i-- {
		if i < n && rows[i].first >= 0 {
			s.last = off + rows[i].first + rows[i].n - 1
			break
		}
	}
	if loaded[1] > loaded[0] {
		if f < loaded[0] {
			s.nav = NavNewer
		} else if l >= loaded[1] {
			s.nav = NavOlder
		}
	}
	near := l >= n-1-1
	s.endReached = near && !s.endWas
	s.endWas = near
}
