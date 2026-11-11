package uikit

import (
	"strconv"
	"strings"

	"github.com/egoist/mygo/ui"
)

// 列表族（ItemList / GroupedList / GroupedGrid / Table）共用的状态与皮肤几何。
//
// 选择、键盘（Up/Down/Home/End/Shift 扩选/全选）、窗口化滚动都交给 mygo 的 ui.List /
// ui.Table；皮肤差异通过「覆盖主题里的 Accent/InactiveAccent/SurfaceHover」让 mygo 把
// 选中行画成本皮肤的样子，行内容（标题/副标题/前后缀）由 uikit 自己画。

// ListState 是一个列表实例的持久状态（放在应用/视图状态里，每个列表一个，不要共用）。
// 选中集合按条目 id 保存；每帧的交互结果通过访问器读取（immediate-mode，无回调）。
type ListState struct {
	tableWidth        float32
	tableColumnWidths map[string]float32
	list              ui.ListState
	grid              gridSel
	sel               ui.Selection[string]
	cur               int // 键盘移动的起点行（ui.ListState.Selected）

	ids   []string       // 当前帧 行号 -> 条目 id（分组头/占位行为 ""）
	rowOf map[string]int // id -> 行号

	// 本帧事件。
	changed    bool
	prevSel    string
	activated  string
	activatedB bool
	deleted    []string
	deletedB   bool
	menuPick   string
	menuIDs    []string
	menuB      bool
	endReached bool
	endWas     bool
	first      int // 可见区间（绝对序号，含 viewport.Offset）
	last       int
	nav        ListNav
	move       ListNav
	started    bool

	// 窗口化列表（HistoryWindowViewport）的命令版本，用于边沿触发。
	vpTop    int
	vpReset  string
	vpReveal string
}

// ListNav 是窗口化列表请求的翻页方向（HistoryWindowViewport.onNavigate）。
type ListNav int

const (
	NavNone  ListNav = iota
	NavNewer         // 向前（更新的结果）
	NavOlder         // 向后（更旧的结果）
)

// Selection 返回选中的条目 id（按列表顺序；未加载的选中项排在最后）。
func (s *ListState) Selection() []string {
	var out []string
	seen := map[string]bool{}
	for _, id := range s.ids {
		if id != "" && s.sel.Has(id) && !seen[id] {
			out = append(out, id)
			seen[id] = true
		}
	}
	var rest []string
	for id := range s.sel.All() {
		if !seen[id] {
			rest = append(rest, id)
		}
	}
	sortStrings(rest)
	return append(out, rest...)
}

// Has 报告 id 是否被选中。
func (s *ListState) Has(id string) bool { return s.sel.Has(id) }

// Select 以 ids 替换选中集合（最后一个成为键盘起点）。不计入 Changed。
func (s *ListState) Select(ids ...string) {
	s.sel.Clear()
	s.cur = -1
	for _, id := range ids {
		s.sel.Add(id)
		if r, ok := s.rowOf[id]; ok {
			s.cur = r
		}
	}
	s.prevSel = s.selKey()
}

// Changed 报告本帧选中集合是否变化（点击、键盘）。
func (s *ListState) Changed() bool { return s.changed }

// Activated 返回本帧被激活（Enter 或双击）的条目 id。
func (s *ListState) Activated() (string, bool) { return s.activated, s.activatedB }

// DeleteRequested 返回本帧按下平台删除键时的选中集合。
func (s *ListState) DeleteRequested() ([]string, bool) { return s.deleted, s.deletedB }

// MenuChosen 返回本帧右键菜单选中的条目 id 与作用的选中集合。
func (s *ListState) MenuChosen() (id string, ids []string, ok bool) {
	return s.menuPick, s.menuIDs, s.menuB
}

// EndReached 在列表滚动到已加载行末尾附近时的第一帧为 true（用于加载更多）。
func (s *ListState) EndReached() bool { return s.endReached }

// VisibleRange 返回可见条目的绝对区间 [first,last]（窗口化列表含 Offset）。
func (s *ListState) VisibleRange() (first, last int) { return s.first, s.last }

// Navigate 返回窗口化列表请求的翻页方向（滚入占位区）。
func (s *ListState) Navigate() ListNav { return s.nav }

// MoveRequest 返回键盘选择越过已加载窗口边界时请求的方向。
func (s *ListState) MoveRequest() ListNav { return s.move }

// ScrollTo 把条目 id 滚到可见。
func (s *ListState) ScrollTo(id string) {
	if r, ok := s.rowOf[id]; ok {
		s.list.ScrollIntoView(r)
	}
}

func (s *ListState) selKey() string {
	var ks []string
	for id := range s.sel.All() {
		ks = append(ks, id)
	}
	sortStrings(ks)
	return strings.Join(ks, "\x00")
}

func sortStrings(a []string) {
	for i := 1; i < len(a); i++ {
		for j := i; j > 0 && a[j] < a[j-1]; j-- {
			a[j], a[j-1] = a[j-1], a[j]
		}
	}
}

// begin 在一帧开始时清空事件。
func (s *ListState) begin() {
	s.changed, s.activatedB, s.deletedB, s.menuB = false, false, false, false
	s.activated, s.deleted, s.menuPick, s.menuIDs = "", nil, "", nil
	s.nav, s.move = NavNone, NavNone
	s.endReached = false
	if !s.started {
		s.started = true
		s.cur = -1
		s.prevSel = s.selKey()
	}
}

// bind 把行号->id 映射接到 mygo 的 ListState 上。
func (s *ListState) bind(ids []string, header func(int) bool) {
	s.ids = ids
	if s.rowOf == nil {
		s.rowOf = map[string]int{}
	} else {
		clear(s.rowOf)
	}
	for i, id := range ids {
		if id != "" {
			s.rowOf[id] = i
		}
	}
	s.list.Selected = &s.cur
	s.list.Selection = &s.sel
	s.list.Key = func(i int) any {
		if i >= 0 && i < len(ids) && ids[i] != "" {
			return ids[i]
		}
		return "\x00row" + strconv.Itoa(i)
	}
	s.list.Header = header
}

// settle 在列表构建后收集事件：选中变化、删除键、末尾。
func (s *ListState) settle(e ui.Element, k Kit) {
	if e.Submitted() && s.cur >= 0 && s.cur < len(s.ids) && s.ids[s.cur] != "" {
		s.activated, s.activatedB = s.ids[s.cur], true
	}
	if k.deleteKey(e) {
		if sel := s.Selection(); len(sel) > 0 {
			s.deleted, s.deletedB = sel, true
		}
	}
	if now := s.selKey(); now != s.prevSel {
		s.prevSel, s.changed = now, true
	}
}

// deleteKey 判断平台删除键：macOS 的 Delete(⌫)/Forward Delete，其余平台的 Delete。
func (k Kit) deleteKey(e ui.Element) bool {
	if e.Shortcut(0, ui.KeyDelete) {
		return true
	}
	return k.Skin == MacOS && e.Shortcut(0, ui.KeyBackspace)
}

// listLook 是一个皮肤的列表行几何。
type listLook struct {
	rowH       float32 // 行高（最小）
	padX       float32
	gap        float32
	radius     float32
	rowGap     float32 // 行间距
	titleSize  float32
	metaSize   float32
	headerH    float32
	cellPadX   float32 // 分组列表里行外侧的左右留白
	separators bool    // 行间 1px 分隔线（Adwaita）
	pill       bool    // 选中行首的 3x16 强调条（Windows）
}

func (k Kit) listLook() listLook {
	switch k.Skin {
	case MacOS:
		return listLook{rowH: 48, padX: 10, gap: 10, radius: 10, titleSize: 13, metaSize: 11, headerH: 28, cellPadX: 6}
	case Windows:
		return listLook{rowH: 56, padX: 12, gap: 12, radius: k.Px("borderRadiusMedium"), rowGap: 2,
			titleSize: k.Px("fontSizeBase300"), metaSize: k.Px("fontSizeBase200"), headerH: 36, cellPadX: 4, pill: true}
	}
	return listLook{rowH: 60, padX: 12, gap: 12, radius: 0, titleSize: 14, metaSize: 12.5, headerH: 34, separators: true}
}

// selectionFill 是选中行的底色；focused=false 为窗口失活的灰色（仅 macOS 区分）。
func (k Kit) selectionFill(active bool) ui.Color {
	switch k.Skin {
	case MacOS:
		if active {
			return k.Color("selection")
		}
		return k.Color("selection-off")
	case Windows:
		return k.Color("w11-selected")
	}
	return k.Color("accent-tint-14")
}

func (k Kit) listHoverFill() ui.Color {
	switch k.Skin {
	case MacOS:
		return ui.Transparent
	case Windows:
		return k.Color("colorSubtleBackgroundHover")
	}
	return k.Color("hover")
}

// rowColors 返回行内标题/副标题色（选中时 macOS 变白）。
func (k Kit) rowColors(selected, active bool) (title, meta ui.Color) {
	if k.Skin == MacOS && selected && active {
		return k.Color("selection-fg"), k.Color("selection-fg-secondary")
	}
	return k.Sem("foreground"), k.Sem("mutedForeground")
}

// surfaceSolid 是需要不透明时（粘性分组头）用的内容底色。
func (k Kit) surfaceSolid() ui.Color {
	switch k.Skin {
	case MacOS:
		return k.Color("content")
	case Windows:
		return k.Color("w11-card-solid")
	}
	return k.Color("card")
}

// withListTheme 把选中/悬停/焦点环换成本皮肤的，返回恢复函数。
func (k Kit) withListTheme(c *ui.Context) (restore func()) {
	old := c.Theme()
	t := *old
	lk := k.listLook()
	t.Accent = k.selectionFill(true)
	t.InactiveAccent = k.selectionFill(false)
	if k.Skin != MacOS {
		t.InactiveAccent = t.Accent
	}
	t.AccentText = k.Sem("foreground")
	if k.Skin == MacOS {
		t.AccentText = k.Color("selection-fg")
	}
	t.SurfaceHover = k.listHoverFill()
	t.Radius = lk.radius
	t.Focus = k.Sem("ring")
	c.SetTheme(&t)
	return func() { c.SetTheme(old) }
}

// highlightRuns 把 title 按 terms（不区分大小写）切成 [文本, 是否命中] 片段。
func highlightRuns(title string, terms []string) (runs []string, hit []bool) {
	raw := []rune(title)
	low := []rune(strings.ToLower(title))
	mark := make([]bool, len(raw))
	found := false
	for _, term := range terms {
		needle := []rune(strings.ToLower(term))
		if len(needle) == 0 {
			continue
		}
		for at := 0; at+len(needle) <= len(low); at++ {
			if string(low[at:at+len(needle)]) == string(needle) {
				for j := at; j < at+len(needle); j++ {
					mark[j] = true
				}
				found = true
			}
		}
	}
	if !found {
		return []string{title}, []bool{false}
	}
	start := 0
	for i := 1; i <= len(raw); i++ {
		if i == len(raw) || mark[i] != mark[start] {
			runs = append(runs, string(raw[start:i]))
			hit = append(hit, mark[start])
			start = i
		}
	}
	return
}

// rowTitle 画标题（可高亮命中词，可等宽），一行省略。
func (k Kit) rowTitle(c *ui.Context, title string, mono bool, terms []string, col ui.Color, size float32) {
	runs, hit := highlightRuns(title, terms)
	if len(runs) == 1 && !hit[0] {
		e := ui.Text(c, title).FontSize(size).TextColor(col).SingleLine()
		if mono {
			e.Font("monospace")
		}
		return
	}
	row := ui.Row(c).AlignItems(ui.Center).MinWidth(0).Shrink(1)
	row.Children(func() {
		for i, r := range runs {
			t := ui.Text(c.Key(i), r).FontSize(size).TextColor(col).SingleLine()
			if mono {
				t.Font("monospace")
			}
			if hit[i] {
				t.Background(k.Sem("chart3").Alpha(.28)).Radius(2)
			}
		}
	})
}

// listRowFrame 画一条标准行：选中态/悬停由 mygo 的行容器负责，这里画内容、Windows 强调条
// 与 Adwaita 分隔线。content 在行内按 leading | 文本列 | trailing 摆放。
type listRowSpec struct {
	title, subtitle string
	mono            bool
	highlight       []string
	leading         func()
	trailing        func()
	check           *bool // 勾选框（multiSelect）：非 nil 时显示
	first           bool
}

func (k Kit) listRow(c *ui.Context, r ui.ListRow, sp listRowSpec) ui.Element {
	lk := k.listLook()
	selected := r.Selected()
	active := c.WindowActive()
	titleCol, metaCol := k.rowColors(selected, active)
	row := ui.Row(c).AlignItems(ui.Center).Gap(lk.gap).Padding(0, lk.padX).MinHeight(lk.rowH).Grow(1)
	row.Radius(lk.radius)
	row.DrawOver(func(p *ui.Painter, rc ui.Rect) {
		if lk.pill && selected && sp.check == nil {
			p.Fill(ui.Rect{X: rc.X, Y: rc.Y + rc.H/2 - 8, W: 3, H: 16}, k.Sem("primary"), 1.5)
		}
		if lk.separators && !sp.first {
			p.Fill(ui.Rect{X: rc.X, Y: rc.Y, W: rc.W, H: 1}, k.Color("border"), 0)
		}
	})
	row.Children(func() {
		if sp.check != nil {
			k.rowCheck(c, *sp.check, selected, active)
		}
		if sp.leading != nil {
			sp.leading()
		}
		col := ui.Column(c).Gap(2).Grow(1).MinWidth(0).Shrink(1)
		col.Children(func() {
			k.rowTitle(c, sp.title, sp.mono, sp.highlight, titleCol, lk.titleSize)
			if sp.subtitle != "" {
				ui.Text(c, sp.subtitle).FontSize(lk.metaSize).TextColor(metaCol).SingleLine()
			}
		})
		if sp.trailing != nil {
			t := ui.Row(c).Gap(4).AlignItems(ui.Center).Shrink(0)
			t.Children(sp.trailing)
		}
	})
	return row
}

// rowCheck 是行首的勾选标记（Windows 多选 / ItemList.multiSelect）。仅显示，状态由行选择承载。
func (k Kit) rowCheck(c *ui.Context, on, selected, active bool) {
	b := ui.Box(c).Size(20, 20).Shrink(0).Radius(k.Px("borderRadiusMedium"))
	if k.Skin != Windows {
		b.Radius(4)
	}
	if on || selected {
		fill := k.Sem("primary")
		if k.Skin == MacOS && selected && active {
			fill = ui.RGBA(255, 255, 255, 56)
		}
		b.Background(fill)
		fg := k.Sem("primaryForeground")
		b.Draw(func(p *ui.Painter, r ui.Rect) {
			path := (&ui.Path{}).MoveTo(r.X+5, r.Y+10.5).LineTo(r.X+8.5, r.Y+14).LineTo(r.X+15, r.Y+6.5)
			p.StrokePath(path, 1.8, fg)
		})
		return
	}
	b.Border(1, k.Sem("mutedForeground")).Background(k.surfaceSolid())
}

// menuFromEntries 把 MenuEntry 填进 mygo 菜单；选中的条目 id 写入 *chosen。
func menuFromEntries(m *ui.Menu, items []MenuEntry, chosen *string) {
	for _, e := range items {
		if e.Separator {
			m.Separator()
			continue
		}
		if len(e.Children) > 0 {
			m.Submenu(e.Label, func(sub *ui.Menu) { menuFromEntries(sub, e.Children, chosen) }).Disabled(e.Disabled).Icon(menuEntryImage(e))
			continue
		}
		it := m.Item(e.Label).ID(e.ID).Disabled(e.Disabled).Icon(menuEntryImage(e))
		if e.Destructive {
			it.Danger()
		}
		if e.Checked {
			it.Checked(true)
		}
		if e.Count != "" {
			it.Trailing(e.Count)
		}
		if it.Chosen() {
			*chosen = e.ID
		}
	}
}

// listContextMenu 给列表元素挂上选区右键菜单：右键未选中的行会先选中它（mygo 行为）。
func (s *ListState) listContextMenu(e ui.Element, items func(ids []string) []MenuEntry) {
	if items == nil {
		return
	}
	e.ContextMenu(func(m *ui.Menu) {
		ids := s.Selection()
		if len(ids) == 0 {
			return
		}
		var chosen string
		menuFromEntries(m, items(ids), &chosen)
		if chosen != "" {
			s.menuPick, s.menuIDs, s.menuB = chosen, ids, true
		}
	})
}

// scrubPlaceholders 撤销落在占位行（窗口外）的选择，并把它转成翻页请求。
func (s *ListState) scrubPlaceholders(lo, hi int) {
	var drop []string
	for id := range s.sel.All() {
		if len(id) > 0 && id[0] == 0 {
			drop = append(drop, id)
		}
	}
	for _, id := range drop {
		s.sel.Remove(id)
	}
	if s.cur >= 0 && (s.cur < lo || s.cur >= hi) {
		if s.cur < lo {
			s.move = NavNewer
			s.cur = lo
		} else {
			s.move = NavOlder
			s.cur = hi - 1
		}
		if s.cur < len(s.ids) && s.ids[s.cur] != "" && len(drop) > 0 {
			s.sel.Add(s.ids[s.cur])
		}
	}
}

// visible 记录可见区间、窗口翻页请求与“滚到末尾”的边沿。
func (s *ListState) visible(n int, loaded [2]int, abs func(int) int) {
	if n == 0 {
		s.first, s.last = 0, -1
		return
	}
	f, l := s.list.Visible()
	if abs != nil {
		s.first, s.last = abs(f), abs(l)
	} else {
		s.first, s.last = f, l
	}
	if loaded[1] > loaded[0] {
		if f < loaded[0] {
			s.nav = NavNewer
		} else if l >= loaded[1] {
			s.nav = NavOlder
		}
	}
	near := l >= n-1-3
	s.endReached = near && !s.endWas
	s.endWas = near
}
