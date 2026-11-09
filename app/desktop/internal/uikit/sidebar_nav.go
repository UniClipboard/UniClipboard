package uikit

import (
	"github.com/egoist/mygo/ui"
)

// NavAction 对应 contract.ts LabelledAction（可带图标）。点击由 NavState.Command 报告。
type NavAction struct {
	Label string
	Icon  IconName
}

// SidebarNavItem 对应 contract.ts SidebarNavItem。
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

// NavCollapse 是分区标题上的展开/收起（macOS 悬停显示）。Label 命名当前动作，如 "Hide Devices"。
type NavCollapse struct {
	Collapsed bool
	Label     string
}

// SidebarNavSection 对应 contract.ts SidebarNavSection。
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

// SidebarNavProps 对应 contract.ts SidebarNavProps。选中值在 *NavState.Value。
type SidebarNavProps struct {
	Label    string
	Sections []SidebarNavSection
	// Footer 固定在底部（如设置），与其他条目一样选中。
	Footer []SidebarNavItem
	// Compact 画成 48px 图标栏（Windows 紧凑窗格，见 SidebarNavCompact）。
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

// navLook 是皮肤的导航条目几何。
type navLook struct {
	h, radius, padX, gap, icon float32
	heading                    float32
}

func (k Kit) navLook() navLook {
	switch k.Skin {
	case MacOS:
		return navLook{h: 28, radius: 8, padX: 8, gap: 8, icon: 16, heading: 11}
	case Windows:
		return navLook{h: 36, radius: k.Px("borderRadiusMedium"), padX: 12, gap: 12, icon: 16, heading: 12}
	}
	return navLook{h: 38, radius: 6, padX: 10, gap: 10, icon: 16, heading: 12}
}

func (k Kit) navSelectedFill() ui.Color {
	switch k.Skin {
	case MacOS:
		return k.Color("fill-2")
	case Windows:
		return k.Color("w11-selected")
	}
	return k.Color("accent-tint-14")
}

func (k Kit) navHoverFill() ui.Color {
	switch k.Skin {
	case MacOS:
		return k.Color("fill-3")
	case Windows:
		return k.Color("colorSubtleBackgroundHover")
	}
	return k.Color("hover")
}

func (k Kit) navStatusColor(t StatusTone) ui.Color {
	if k.Skin == Windows {
		names := map[StatusTone]string{StatusPositive: "w11-status-positive", StatusInfo: "w11-status-info",
			StatusWarning: "w11-status-warning", StatusCritical: "w11-status-critical", StatusNeutral: "w11-status-neutral"}
		return k.Color(names[t])
	}
	switch t {
	case StatusPositive:
		return k.Color("green")
	case StatusInfo:
		return k.Color("accent")
	case StatusWarning:
		if k.Skin == MacOS {
			return k.Color("orange")
		}
		return k.Color("warning")
	case StatusCritical:
		return k.Color("red")
	}
	return k.Color("fg3")
}

// SidebarNavEntry 画一条导航条目（图标/色点/状态点、标签、计数、快捷键提示），点击选中。
// 在 SidebarNav 内使用；单独使用时由调用方读 NavState。
func (k Kit) SidebarNavEntry(c *ui.Context, st *NavState, item SidebarNavItem) ui.Element {
	lk := k.navLook()
	selected := st.Value == item.ID
	row := ui.Row(c.Key(item.ID)).AlignItems(ui.Center).Gap(lk.gap).Padding(0, lk.padX).Height(lk.h).Shrink(0).
		Radius(lk.radius).Label(item.Label).Role(ui.RoleListItem)
	row.AccessSelected(selected)
	if selected {
		row.Background(k.navSelectedFill())
	} else if row.Hovered() {
		row.Background(k.navHoverFill())
	}
	fg := k.Sem("foreground")
	mute := k.Sem("mutedForeground")
	showPill := k.Skin == Windows && selected
	row.DrawOver(func(p *ui.Painter, rc ui.Rect) {
		if showPill {
			p.Fill(ui.Rect{X: rc.X, Y: rc.Y + rc.H/2 - 8, W: 3, H: 16}, k.Sem("primary"), 1.5)
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
			if selected && k.Skin != Linux {
				ic.TextColor(k.Sem("primary"))
			} else {
				ic.TextColor(fg)
			}
		}
		ui.Text(c, item.Label).FontSize(k.navFont()).TextColor(fg).SingleLine().Grow(1).MinWidth(0)
		if item.Count != "" {
			ui.Text(c, item.Count).FontSize(k.navFont() - 1).TextColor(mute).SingleLine().Shrink(0)
		}
		if item.ShortcutHint != "" {
			ui.Text(c, item.ShortcutHint).FontSize(k.navFont() - 1).TextColor(mute).SingleLine().Shrink(0)
		}
	})
	if row.Clicked() {
		st.pick(item.ID)
	}
	return row
}

func (k Kit) navFont() float32 {
	if k.Skin == MacOS {
		return 13
	}
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
	nav := ui.Column(c).Label(p.Label).Role(ui.RoleList).Focusable().Gap(2).Padding(8, 8)
	nav.Children(func() {
		body := ui.Scroll(c).Grow(1).MinHeight(0).Gap(2)
		body.Children(func() {
			for _, s := range p.Sections {
				k.navSection(c, st, s, lk)
			}
		})
		if len(p.Footer) > 0 {
			foot := ui.Column(c).Gap(2).Shrink(0)
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
	sec := ui.Column(c.Key(s.ID)).Gap(2).Shrink(0)
	sec.Children(func() {
		collapsed := s.Collapse != nil && s.Collapse.Collapsed
		if s.Title != "" || s.Action != nil || s.Collapse != nil {
			head := ui.Row(c).AlignItems(ui.Center).Height(lk.h-6).Padding(0, lk.padX).Gap(4)
			if k.Skin == Windows {
				head.Height(lk.h)
			}
			head.Children(func() {
				ui.Text(c, s.Title).FontSize(lk.heading).TextColor(k.Sem("mutedForeground")).SingleLine().Grow(1).
					FontWeight(600)
				if s.Collapse != nil {
					ic := IconChevronDown
					if collapsed {
						ic = IconChevronRight
					}
					b := k.Button(c.Key("collapse"), "", ButtonProps{Variant: Subtle, Size: SizeSmall, Icon: ic,
						Label: s.Collapse.Label})
					if b.Clicked() {
						st.cmd, st.cmdSec = NavToggleCollapse, s.ID
					}
				}
				if s.Action != nil {
					b := k.Button(c.Key("action"), "", ButtonProps{Variant: Subtle, Size: SizeSmall, Icon: s.Action.Icon,
						Label: s.Action.Label})
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
			if k.Button(c.Key("more"), s.More.Label, ButtonProps{Variant: Link, Size: SizeSmall}).Clicked() {
				st.cmd, st.cmdSec = NavMore, s.ID
			}
		}
	})
}
