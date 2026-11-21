package desktopkit

import (
	"github.com/egoist/mygo/ui"
)

type NavAction struct {
	Label string
	Icon  IconName
}

type SidebarNavItem struct {
	ID    string
	Label string
	Icon  IconName
	// Swatch 是智能视图/标签的色点。
	Swatch TagColor
	// Count 是已格式化的尾部计数。
	Count string
	// Status 非 nil 时用状态点代替图标（如设备连接状态）。
	Status *StatusTone
	// ShortcutHint 仅显示，如 `⌘,`。
	ShortcutHint string
}

// NavPlaceholder 在分区没有条目时代替条目显示。
type NavPlaceholder struct {
	Text   string
	Action *NavAction
}

type NavCollapse struct {
	Collapsed bool
	Label     string
}

type SidebarNavSection struct {
	ID    string
	Title string
	Items []SidebarNavItem
	// Action 是标题上的命令（图标按钮），More 是条目后的链接行。
	Action      *NavAction
	More        *NavAction
	Placeholder *NavPlaceholder
	// Icon 是紧凑模式下该分区折成的一个图标（点击弹出其条目）。
	Icon     IconName
	Collapse *NavCollapse
}

type SidebarNavProps struct {
	Label    string
	Sections []SidebarNavSection
	// Footer 固定在底部（如设置），与其他条目一样选中。
	Footer  []SidebarNavItem
	Compact bool
}

// NavCommandKind 区分 NavState.Command 报告的命令。
type NavCommandKind int

const (
	NavNoCommand      NavCommandKind = iota
	NavSectionAction                 // 分区标题上的图标按钮
	NavMore                          // "更多" 链接行
	NavPlaceholderAct                // 占位里的按钮
	NavToggleCollapse                // 展开/收起（调用方翻转 Collapse.Collapsed）
)

// NavState 是侧边导航的持久状态；Value 即选中的条目 id，调用方可直接读写。
type NavState struct {
	Value string

	changed bool
	cmd     NavCommandKind
	cmdSec  string
}

// Changed 报告本帧 Value 被点击或键盘改变。
func (s *NavState) Changed() bool { return s.changed }

// Command 返回本帧触发的命令与所属分区 id。
func (s *NavState) Command() (NavCommandKind, string) { return s.cmd, s.cmdSec }

func (s *NavState) pick(id string) {
	if s.Value != id {
		s.Value, s.changed = id, true
	}
}

type navLook struct {
	h, radius, padX, gap, icon float32
	heading                    float32
}

func (k Kit) navLook() navLook {
	{
	}

	return navLook{h: 36, radius: 10, padX: 8, gap: 8, icon: 20, heading: 12}
}

func (k Kit) navSelectedFill() ui.Color {
	{
	}

	return k.Color("background-tertiary-default")
}

func (k Kit) navHoverFill() ui.Color {
	{
	}

	return k.Color("background-secondary-hover")
}

func (k Kit) navStatusColor(t StatusTone) ui.Color {

	switch t {
	case StatusPositive:
		return k.Color("notification-success-foreground")
	case StatusInfo:
		return k.Color("accent-500")
	case StatusWarning:

		return k.Color("status-yellow-text")
	case StatusCritical:
		return k.Color("text-error-primary")
	}

	return k.Color("text-tertiary")
}

// SidebarNavEntry 画一条导航条目（图标/色点/状态点、标签、计数、快捷键提示），点击选中。
// 在 SidebarNav 内使用；单独使用时由调用方读 NavState。
func (k Kit) SidebarNavEntry(c *ui.Context, st *NavState, item SidebarNavItem) ui.Element {
	lk := k.navLook()
	selected := st.Value == item.ID
	row := ui.Row(c.Key(item.ID)).AlignItems(ui.Center).Gap(lk.gap).Padding(0, lk.padX).Height(lk.h).Shrink(0).
		Radius(lk.radius).Label(item.Label).Role(ui.RoleListItem)
	row.AccessSelected(selected)
	fg := k.Color("text-primary")
	mute := k.Color("text-secondary")
	if selected {
		// The fixed official NavItem uses an accent gradient and white ink.
		// A neutral selected fill is identical to its hover token in both schemes.
		row.Gradient(k.Color("accent-500"), k.Color("accent-600"), 180)
		fg, mute = k.Color("text-white"), k.Color("text-white")
	} else if row.Hovered() {
		row.Background(k.navHoverFill())
	}
	showPill := false
	row.DrawOver(func(p *ui.Painter, rc ui.Rect) {
		if showPill {
			p.Fill(ui.Rect{X: rc.X, Y: rc.Y + rc.H/2 - 8, W: 3, H: 16}, k.Color("accent-500"), 1.5)
		}
	})
	row.Children(func() {
		switch {
		case item.Status != nil:
			box := ui.Row(c).Size(lk.icon, lk.icon).Justify(ui.Center).AlignItems(ui.Center).Shrink(0)
			box.Children(func() {
				ui.Box(c).Size(8, 8).Radius(4).Background(k.navStatusColor(*item.Status))
			})
		case item.Swatch != "":
			box := ui.Row(c).Size(lk.icon, lk.icon).Justify(ui.Center).AlignItems(ui.Center).Shrink(0)
			box.Children(func() {
				ui.Box(c).Size(10, 10).Radius(3).Background(k.SwatchColor(item.Swatch))

			})
		case item.Icon != "":
			ic := k.Icon(c, item.Icon, lk.icon)

			{
				ic.TextColor(fg)
			}
		}
		k.native().Text(c, item.Label, "body-medium").TextColor(fg).SingleLine().Grow(1).MinWidth(0)
		if item.Count != "" {
			k.native().Text(c, item.Count, "caption-1-medium").TextColor(mute).SingleLine().Shrink(0)
		}
		if item.ShortcutHint != "" {
			k.native().Text(c, item.ShortcutHint, "caption-1-medium").TextColor(mute).SingleLine().Shrink(0)
		}
	})
	if row.Clicked() {
		st.pick(item.ID)
	}
	return row
}

func (k Kit) navFont() float32 {

	return 14
}

// navOrder 列出可选条目 id（按显示顺序，跳过收起分区的条目）。
func navOrder(p SidebarNavProps) []string {
	var ids []string
	for _, s := range p.Sections {
		if s.Collapse != nil && s.Collapse.Collapsed {
			continue
		}
		for _, it := range s.Items {
			ids = append(ids, it.ID)
		}
	}
	for _, it := range p.Footer {
		ids = append(ids, it.ID)
	}
	return ids
}

// SidebarNav 是单选导航：整个侧栏一个 Tab 停靠点，Up/Down 移动并选中，Home/End 跳转。
// 返回的元素可继续设尺寸。
func (k Kit) SidebarNav(c *ui.Context, st *NavState, p SidebarNavProps) ui.Element {
	st.changed, st.cmd, st.cmdSec = false, NavNoCommand, ""
	if p.Compact {
		return k.SidebarNavCompact(c, st, p)
	}
	lk := k.navLook()
	// SplitView contributes 8 DIP; this inner inset produces one effective 12-DIP margin.
	nav := ui.Column(c).Label(p.Label).Role(ui.RoleList).Focusable().MinHeight(0).Gap(0).Padding(4)

	nav.Children(func() {
		// The native overlay indicator and its hit target need a content-free lane.
		// Keep it stable while idle so rows do not shift when scrolling starts.
		body := k.Scroll(c).Grow(1).MinHeight(0).Gap(12).Padding(0, 12, 0, 0)
		body.Children(func() {
			for _, s := range p.Sections {
				k.navSection(c, st, s, lk)
			}
		})
		if len(p.Footer) > 0 {
			foot := ui.Column(c).Gap(4).Shrink(0).Padding(12, 0, 0).
				InsetShadow(0, 1, 0, 0, k.Color("separator-border"))
			foot.Children(func() {
				for _, it := range p.Footer {
					k.SidebarNavEntry(c, st, it)
				}
			})
		}
	})
	k.navKeys(nav, st, navOrder(p))
	return nav
}

func (k Kit) navKeys(nav ui.Element, st *NavState, order []string) {
	if len(order) == 0 {
		return
	}
	idx := -1
	for i, id := range order {
		if id == st.Value {
			idx = i
		}
	}
	switch {
	case nav.Shortcut(0, ui.KeyDown):
		st.pick(order[min(idx+1, len(order)-1)])
	case nav.Shortcut(0, ui.KeyUp):
		st.pick(order[max(idx-1, 0)])
	case nav.Shortcut(0, ui.KeyHome):
		st.pick(order[0])
	case nav.Shortcut(0, ui.KeyEnd):
		st.pick(order[len(order)-1])
	}
}

func (k Kit) navSection(c *ui.Context, st *NavState, s SidebarNavSection, lk navLook) {
	sec := ui.Column(c.Key(s.ID)).Gap(4).Shrink(0)

	sec.Children(func() {
		collapsed := s.Collapse != nil && s.Collapse.Collapsed
		if s.Title != "" || s.Action != nil || s.Collapse != nil {
			head := ui.Row(c).AlignItems(ui.Center).Height(28).Padding(0, lk.padX).Gap(4)

			head.Children(func() {
				k.native().Text(c, s.Title, "caption-1-semibold").TextColor(k.Color("text-secondary")).SingleLine().Grow(1)
				if s.Collapse != nil {
					ic := IconChevronDown
					if collapsed {
						ic = IconChevronRight
					}
					b := k.navSectionAction(c.Key("collapse"), s.Collapse.Label, ic)
					if b.Clicked() {
						st.cmd, st.cmdSec = NavToggleCollapse, s.ID
					}
				}
				if s.Action != nil {
					b := k.navSectionAction(c.Key("action"), s.Action.Label, s.Action.Icon)
					if b.Clicked() {
						st.cmd, st.cmdSec = NavSectionAction, s.ID
					}
				}
			})
		}
		if collapsed {
			return
		}
		for _, it := range s.Items {
			k.SidebarNavEntry(c, st, it)
		}
		if len(s.Items) == 0 && s.Placeholder != nil {
			ph := ui.Column(c).Gap(6).Padding(4, lk.padX)
			ph.Children(func() {
				if s.Placeholder.Text != "" {
					k.Text(c, s.Placeholder.Text, TextProps{Tone: ToneSecondary, Role: RoleCaption})
				}
				if a := s.Placeholder.Action; a != nil {
					if k.Button(c.Key("ph"), a.Label, ButtonProps{Variant: Subtle, Size: SizeSmall, Icon: a.Icon}).Clicked() {
						st.cmd, st.cmdSec = NavPlaceholderAct, s.ID
					}
				}
			})
		}
		if s.More != nil {
			more := ui.ButtonBase(c.Key("more")).Label(s.More.Label).Height(24).Shrink(0).Justify(ui.Start).AlignItems(ui.Center).
				Padding(0, lk.padX, 0, lk.padX+lk.icon+lk.gap).Radius(6).
				FocusRingColor(k.Color("border-focus-ring")).FocusRingOffset(2)
			if more.Hovered() {
				more.Background(k.navHoverFill())
			}
			more.Children(func() {
				k.native().Text(c, s.More.Label, "caption-1-medium").TextColor(k.Color("text-secondary")).SingleLine()
			})
			if more.Clicked() {
				st.cmd, st.cmdSec = NavMore, s.ID
			}
		}
	})
}

// navSectionAction is a quiet inline section command, not a page CTA.
func (k Kit) navSectionAction(c *ui.Context, label string, icon IconName) ui.Element {
	b := ui.ButtonBase(c).Label(label).Size(24, 24).Radius(6).Justify(ui.Center).AlignItems(ui.Center).
		FocusRingColor(k.Color("border-focus-ring")).FocusRingOffset(2)
	if b.Hovered() {
		b.Background(k.navHoverFill())
	}
	b.Children(func() { k.Icon(c, icon, 16).TextColor(k.Color("foreground-icon-secondary")).AccessHidden(true) })
	return b
}
