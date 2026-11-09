package uikit

import "github.com/egoist/mygo/ui"

// SidebarNavCompact 是 48px 图标栏（Windows NavigationView 紧凑窗格）：没有 Icon 的分区
// 把条目平铺成图标按钮，有 Icon 的分区折成一个按钮并在弹出菜单里列出其条目；
// 标签即无障碍名与提示。选中项带强调条。
func (k Kit) SidebarNavCompact(c *ui.Context, st *NavState, p SidebarNavProps) ui.Element {
	rail := ui.Column(c).Label(p.Label).Role(ui.RoleList).Focusable().AlignItems(ui.Center).Gap(2).Padding(4, 0).Width(48)
	rail.Children(func() {
		entry := func(it SidebarNavItem) {
			if it.Icon == "" {
				return
			}
			k.navRailButton(c.Key(it.ID), st, it.Label, it.Icon, st.Value == it.ID, func() { st.pick(it.ID) })
		}
		var sectioned []SidebarNavSection
		for _, s := range p.Sections {
			if s.Icon != "" {
				sectioned = append(sectioned, s)
				continue
			}
			for _, it := range s.Items {
				entry(it)
			}
		}
		if len(sectioned) > 0 {
			ui.Box(c).Size(24, 1).Background(k.Color("w11-divider")).Margin(6, 0)
		}
		for _, s := range sectioned {
			k.navFlyout(c.Key(s.ID), st, s)
		}
		ui.Box(c).Grow(1)
		for _, it := range p.Footer {
			entry(it)
		}
	})
	k.navKeys(rail, st, navOrder(p))
	return rail
}

// navRailButton 是 40x36 的图标按钮，选中时带底色与 3x16 强调条。
func (k Kit) navRailButton(c *ui.Context, st *NavState, label string, icon IconName, current bool, onClick func()) ui.Element {
	b := ui.Row(c).Size(40, 36).Justify(ui.Center).AlignItems(ui.Center).Shrink(0).Radius(k.navLook().radius).
		Label(label).Role(ui.RoleListItem)
	b.AccessSelected(current)
	if current {
		b.Background(k.navSelectedFill())
	} else if b.Hovered() {
		b.Background(k.navHoverFill())
	}
	b.DrawOver(func(p *ui.Painter, rc ui.Rect) {
		if current {
			p.Fill(ui.Rect{X: rc.X, Y: rc.Y + rc.H/2 - 8, W: 3, H: 16}, k.Sem("primary"), 1.5)
		}
	})
	b.Children(func() {
		ic := k.Icon(c, icon, 16)
		if current {
			ic.TextColor(k.Sem("primary"))
		} else {
			ic.TextColor(k.Sem("foreground"))
		}
	})
	b.Tooltip(label)
	if b.Clicked() {
		onClick()
	}
	return b
}

// navFlyout 是折叠分区的图标按钮：点击弹出该分区的条目、占位动作与“更多”。
func (k Kit) navFlyout(c *ui.Context, st *NavState, s SidebarNavSection) {
	var items []MenuEntry
	for _, it := range s.Items {
		items = append(items, MenuEntry{ID: "i:" + it.ID, Label: it.Label, Icon: it.Icon, Count: it.Count,
			Swatch: it.Swatch, Checked: it.ID == st.Value})
	}
	if len(s.Items) == 0 && s.Placeholder != nil && s.Placeholder.Action != nil {
		items = append(items, MenuEntry{ID: "ph", Label: s.Placeholder.Action.Label})
	}
	if s.More != nil {
		items = append(items, MenuSeparator("sep"), MenuEntry{ID: "more", Label: s.More.Label})
	}
	title := s.Title
	if title == "" {
		title = s.ID
	}
	_, chosen := k.Menu(c, MenuProps{Items: items, Label: title, Icon: s.Icon, IconOnly: true, Variant: Subtle})
	switch {
	case chosen == "ph":
		st.cmd, st.cmdSec = NavPlaceholderAct, s.ID
	case chosen == "more":
		st.cmd, st.cmdSec = NavMore, s.ID
	case len(chosen) > 2 && chosen[:2] == "i:":
		st.pick(chosen[2:])
	}
}
