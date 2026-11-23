package desktopkit

import (
	"runtime"
	"strconv"
	"strings"

	"github.com/egoist/mygo/ui"
)

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
	clicked    string
	clickedB   bool
	activated  string
	activatedB bool
	deleted    []string
	deletedB   bool
	menuPick   string
	menuIDs    []string
	menuB      bool
	hoverID    string
	menuOpen   bool
	menuTarget []string
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

// Hovered returns the item under the pointer in the current layout.
func (s *ListState) Hovered(grid bool) string {
	if grid {
		return s.grid.hover
	}
	return s.hoverID
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

// Clicked 返回本帧被主点击的条目 id（即使该条目本已选中）。
func (s *ListState) Clicked() (string, bool) { return s.clicked, s.clickedB }

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
	s.changed, s.clickedB, s.activatedB, s.deletedB, s.menuB = false, false, false, false, false
	s.clicked, s.activated, s.deleted, s.menuPick, s.menuIDs = "", "", nil, "", nil
	s.hoverID = ""
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

func (k Kit) deleteKey(e ui.Element) bool {
	return e.Shortcut(0, ui.KeyDelete) || runtime.GOOS == "darwin" && e.Shortcut(0, ui.KeyBackspace)
}

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
	{
	}

	return listLook{rowH: 60, padX: 12, gap: 12, radius: 0, titleSize: 14, metaSize: 12.5, headerH: 34, separators: true}
}

func (k Kit) selectionFill(active bool) ui.Color {
	{
	}

	return k.Color("background-tertiary-default")
}

func (k Kit) listHoverFill() ui.Color {
	{
	}

	return k.Color("background-secondary-hover")
}

func (k Kit) rowColors(selected, active bool) (title, meta ui.Color) {

	return k.Color("text-primary"), k.Color("text-secondary")
}

// surfaceSolid 是需要不透明时（粘性分组头）用的内容底色。
func (k Kit) surfaceSolid() ui.Color {
	return k.workspaceCanvas()
}

func (k Kit) withListTheme(c *ui.Context) (restore func()) {
	old := c.Theme()
	t := *old
	lk := k.listLook()
	t.Accent = k.selectionFill(true)
	t.InactiveAccent = k.selectionFill(false)
	{
		t.InactiveAccent = t.Accent
	}
	t.AccentText = k.Color("text-primary")

	t.SurfaceHover = k.listHoverFill()
	t.Radius = lk.radius
	t.Focus = k.Color("border-focus-ring")
	c.SetTheme(&t)
	return func() { c.SetTheme(old) }
}

// withCardListTheme suppresses mygo's flat edge-to-edge row selection fill so L1 + S-A
// rows paint their own inset rounded selection surface and soft accent border.
func (k Kit) withCardListTheme(c *ui.Context) (restore func()) {
	old := c.Theme()
	t := *old
	t.Accent = ui.Transparent
	t.InactiveAccent = ui.Transparent
	t.AccentText = k.Color("text-primary")
	t.SurfaceHover = ui.Transparent
	t.Radius = 10
	t.Focus = k.Color("border-focus-ring")
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
	k.rowTitleWeighted(c, title, mono, terms, col, size, 0)
}

func (k Kit) rowTitleWeighted(c *ui.Context, title string, mono bool, terms []string, col ui.Color, size float32, weight int) {
	runs, hit := highlightRuns(title, terms)
	if len(runs) == 1 && !hit[0] {
		e := ui.Text(c, title).FontSize(size).TextColor(col).SingleLine()
		if weight > 0 {
			e.FontWeight(weight)
		}
		if mono {
			e.Font("monospace")
		}
		return
	}
	row := ui.Row(c).AlignItems(ui.Center).MinWidth(0).Shrink(1)
	row.Children(func() {
		for i, r := range runs {
			t := ui.Text(c.Key(i), r).FontSize(size).TextColor(col).SingleLine()
			if weight > 0 {
				t.FontWeight(weight)
			}
			if mono {
				t.Font("monospace")
			}
			if hit[i] {
				t.Background(k.Color("status-yellow-background").Alpha(.28)).Radius(2)
			}
		}
	})
}

// l1ListRow renders the L1 + S-A floating-selection card row: rounded 10-DIP inset row
// with soft blue tint + accent border when selected, 28x28 Source App icon with 13x13
// bottom-right kind corner badge (or 32x32 image thumbnail), 2.5-line rich text hierarchy,
// user colored tag pills, and hover quick copy.
func (k Kit) l1ListRow(c *ui.Context, r ui.ListRow, it ItemListItem) ui.Element {
	selected := r.Selected()
	row := ui.Row(c).AlignItems(ui.Start).Gap(10).Padding(9, 11).Margin(0, 6, 3, 8).Radius(10).Grow(1)
	hovered := row.Hovered()
	hoverFill := k.Color("background-secondary-default")
	if k.Dark {
		hoverFill = k.Color("background-primary-default")
	}
	if selected {
		bgAlpha := float32(.09)
		borderAlpha := float32(.48)
		if k.Dark {
			bgAlpha = .15
			borderAlpha = .62
		}
		row.Background(k.Color("accent-500").Alpha(bgAlpha)).
			Border(1.25, k.Color("accent-500").Alpha(borderAlpha))
	} else if hovered {
		row.Background(hoverFill).Border(1.25, ui.Transparent)
	} else {
		row.Border(1.25, ui.Transparent)
	}

	badgeRing := k.surfaceSolid()
	if selected {
		alpha := float32(.09)
		if k.Dark {
			alpha = .15
		}
		badgeRing = k.Color("accent-500").Alpha(alpha).Over(badgeRing)
	} else if hovered {
		badgeRing = hoverFill
	}

	row.Children(func() {
		// Leading provenance / media tile.
		if it.Kind == KindImage && it.Image != nil {
			ui.Image(c.Key("thumb"), it.Image).Size(34, 34).Radius(7).Fit(ui.Cover).
				Border(1, k.Color("separator-border")).Shrink(0).AccessHidden(true)
		} else if it.SourceAppName != "" {
			bg, fg := k.gridKindTint(it.Kind)
			ui.Box(c.Key("app-tile")).Size(28, 28).Margin(2, 0, 0, 0).Shrink(0).AccessHidden(true).Children(func() {
				if it.SourceAppIcon != nil {
					ui.Image(c.Key("app-icon"), it.SourceAppIcon).Size(28, 28).Radius(7).Fit(ui.Contain)
				} else {
					ui.Box(c.Key("app-fallback")).Size(28, 28).Radius(7).
						Background(k.Color("background-secondary-default")).
						Border(1, k.Color("separator-border")).Center().Children(func() {
						k.Icon(c, IconPackage, 14).TextColor(k.Color("text-secondary"))
					})
				}
				ui.Box(c.Key("kind-corner")).Absolute().Bottom(-2).Right(-2).Size(14, 14).Radius(4.5).
					Background(bg).Border(1.5, badgeRing).Center().Children(func() {
					k.Icon(c, kindIcon(it.Kind), 8.5).TextColor(fg)
				})
			})
		} else {
			bg, fg := k.gridKindTint(it.Kind)
			ui.Box(c.Key("kind-tile")).Size(28, 28).Margin(2, 0, 0, 0).Radius(7).
				Background(bg).Center().Shrink(0).AccessHidden(true).Children(func() {
				k.Icon(c, kindIcon(it.Kind), 14).TextColor(fg)
			})
		}

		// Main 2.5-line stack: Title + Time (fixed 20-DIP top row so hover never shifts layout),
		// optional Excerpt, Meta + User Tags.
		ui.Column(c.Key("col")).Gap(2.5).Grow(1).MinWidth(0).Shrink(1).Children(func() {
			ui.Row(c.Key("top")).Height(20).AlignItems(ui.Center).Gap(6).MinWidth(0).Children(func() {
				ui.Box(c.Key("title-wrap")).Grow(1).MinWidth(0).Shrink(1).Children(func() {
					k.rowTitleWeighted(c, it.Title, it.Monospace, it.Highlight, k.Color("text-primary"), 13, 600)
				})
				trail := ui.Row(c.Key("trail")).Gap(4).AlignItems(ui.Center).Shrink(0)
				if hovered && it.OnCopy != nil {
					trail.Opacity(0)
				}
				trail.Children(func() {
					if it.Pinned {
						k.Icon(c.Key("pin"), IconPin, 11).TextColor(k.Color("status-orange-text"))
					}
					if it.Time != "" {
						ui.Text(c.Key("time"), it.Time).FontSize(11).TextColor(k.Color("text-tertiary")).SingleLine()
					}
				})
			})

			if it.Excerpt != "" {
				ex := ui.Text(c.Key("excerpt"), it.Excerpt).FontSize(11.8).TextColor(k.Color("text-secondary")).SingleLine()
				switch it.ExcerptKind {
				case KindCode:
					ex.Font("monospace")
				case KindLink:
					ex.Font("monospace").TextColor(k.Color("accent-500"))
				}
			}

			meta := it.Meta
			if meta == "" {
				meta = it.Subtitle
			}
			if meta != "" || len(it.Tags) > 0 {
				ui.Row(c.Key("bot")).AlignItems(ui.Center).Gap(6).MinWidth(0).Margin(1, 0, 0, 0).Children(func() {
					if meta != "" {
						ui.Text(c.Key("meta"), meta).FontSize(10.8).TextColor(k.Color("text-tertiary")).SingleLine().Grow(1).MinWidth(0)
					} else {
						ui.Spacer(c)
					}
					if len(it.Tags) > 0 {
						ui.Row(c.Key("tags")).Gap(4).AlignItems(ui.Center).Shrink(0).Children(func() {
							limit := min(2, len(it.Tags))
							tagBg := k.Color("background-tertiary-default")
							if k.Dark {
								tagBg = k.Color("text-primary").Alpha(.08)
							}
							for idx := 0; idx < limit; idx++ {
								tag := it.Tags[idx]
								ui.Row(c.Key(idx)).Height(17).PaddingX(5.5).Radius(8.5).Gap(3.5).AlignItems(ui.Center).
									Background(tagBg).Children(func() {
									ui.Box(c.Key("dot")).Size(5, 5).Radius(2.5).Background(k.SwatchColor(tag.Color)).Shrink(0).AccessHidden(true)
									ui.Text(c.Key("lbl"), tag.Label).FontSize(10).FontWeight(500).TextColor(k.Color("text-secondary")).SingleLine()
								})
							}
						})
					}
				})
			}
		})
		// Float quick-copy over the top-right slot so hovering causes zero vertical or horizontal jitter.
		if hovered && it.OnCopy != nil {
			lbl := it.CopyLabel
			if lbl == "" {
				lbl = "Copy"
			}
			btn := ui.ButtonBase(c.Key("quick-copy")).Absolute().Top(9).Right(11).ZIndex(2).
				Height(20).PaddingX(6.5).Radius(5.5).Gap(3.5).
				AlignItems(ui.Center).Shrink(0).TabStop(false).Label(lbl).
				Background(k.CardSurface()).
				Border(1, k.Color("border-button-default"))
			ink := k.Color("text-secondary")
			if btn.Hovered() {
				btn.Background(k.Color("accent-500")).Border(1, k.Color("accent-500"))
				ink = k.Color("text-on-accent")
			}
			btn.Children(func() {
				k.Icon(c, IconCopy, 10.5).TextColor(ink)
				ui.Text(c, lbl).FontSize(10.5).FontWeight(500).TextColor(ink).SingleLine()
			})
			if btn.Clicked() {
				it.OnCopy()
			}
		}
	})
	return row
}

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
			p.Fill(ui.Rect{X: rc.X, Y: rc.Y + rc.H/2 - 8, W: 3, H: 16}, k.Color("accent-500"), 1.5)
		}
		if lk.separators && !sp.first {
			p.Fill(ui.Rect{X: rc.X, Y: rc.Y, W: rc.W, H: 1}, k.Color("separator-border"), 0)
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

func (k Kit) rowCheck(c *ui.Context, on, selected, active bool) {
	b := ui.Box(c).Size(20, 20).Shrink(0).Radius(float32(8))
	{
		b.Radius(4)
	}
	if on || selected {
		fill := k.Color("accent-500")

		b.Background(fill)
		fg := k.Color("text-on-accent")
		b.Draw(func(p *ui.Painter, r ui.Rect) {
			path := (&ui.Path{}).MoveTo(r.X+5, r.Y+10.5).LineTo(r.X+8.5, r.Y+14).LineTo(r.X+15, r.Y+6.5)
			p.StrokePath(path, 1.8, fg)
		})
		return
	}
	b.Border(1, k.Color("text-secondary")).Background(k.surfaceSolid())
}

// menuFromEntries 把 MenuEntry 填进 mygo 菜单；选中的条目 id 写入 *chosen。
func (k Kit) menuFromEntries(m *ui.Menu, items []MenuEntry, chosen *string) {
	for _, e := range items {
		if e.Separator {
			m.Separator()
			continue
		}
		if len(e.Children) > 0 {
			m.Submenu(e.Label, func(sub *ui.Menu) { k.menuFromEntries(sub, e.Children, chosen) }).Disabled(e.Disabled).Icon(k.menuEntryImage(e))
			continue
		}
		it := m.Item(e.Label).ID(e.ID).Disabled(e.Disabled).Icon(k.menuEntryImage(e))
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

// AttachContextMenu binds a native popover context menu built from MenuEntry items to e.
func (k Kit) AttachContextMenu(e ui.Element, items func() []MenuEntry, onChosen func(string)) {
	if items == nil || onChosen == nil {
		return
	}
	e.ContextMenu(func(m *ui.Menu) {
		var chosen string
		k.menuFromEntries(m, items(), &chosen)
		if chosen != "" {
			onChosen(chosen)
		}
	})
}

// listContextMenu binds the shared BoardUI popover context menu (matching ActionDockMenu
// and toolbar Menu) to a list, grid, or table element. Right-clicking an unselected item
// highlights that item without triggering an unintended detail-drawer open.
func (k Kit) listContextMenu(c *ui.Context, e ui.Element, s *ListState, items func(ids []string) []MenuEntry, hoverID string) {
	if items == nil {
		return
	}
	wasOpen := s.menuOpen
	m := ui.ContextMenuBase(c.Key("ctxmenu"), e, &s.menuOpen)
	if !wasOpen && s.menuOpen {
		if hoverID != "" && !s.sel.Has(hoverID) {
			s.sel.Clear()
			s.sel.Add(hoverID)
			if row, ok := s.rowOf[hoverID]; ok {
				s.cur = row
			}
			s.prevSel = s.selKey()
		}
		s.menuTarget = s.Selection()
		if len(s.menuTarget) == 0 {
			s.menuOpen = false
		}
	}
	if !s.menuOpen {
		s.menuTarget = nil
	}
	var chosen string
	var chosenIDs []string
	m.Popup(func(panel ui.Element) {
		ids := s.menuTarget
		if len(ids) == 0 {
			ids = s.Selection()
		}
		if len(ids) == 0 {
			return
		}
		chosenIDs = ids
		k.popupSurface(panel)
		k.menuEntriesInto(c, m, items(ids), &chosen)
	})
	if chosen != "" {
		s.menuPick, s.menuIDs, s.menuB = chosen, chosenIDs, true
	}
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
