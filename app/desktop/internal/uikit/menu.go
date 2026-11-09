package uikit

import "github.com/egoist/mygo/ui"

// MenuEntry is one row of a menu (contract.ts MenuEntry): an action, a submenu when
// Children is non-empty, or a separator.
type MenuEntry struct {
	ID    string // stable id; returned when the entry is chosen
	Label string
	Icon  IconName
	// Shortcut is a display-only hint.
	Shortcut    string
	Destructive bool
	Disabled    bool
	// Count is a trailing count, already formatted by the caller.
	Count  string
	Swatch TagColor
	// Checked marks the current choice, e.g. a tag's colour.
	Checked bool
	// Children makes the entry a submenu such as "Merge into ›".
	Children []MenuEntry
	// Separator draws a divider line; every other field is ignored.
	Separator bool
}

// MenuSeparator is a divider entry.
func MenuSeparator(id string) MenuEntry { return MenuEntry{ID: id, Separator: true} }

// MenuProps mirrors contract.ts MenuProps: a button that opens an action menu.
type MenuProps struct {
	Items    []MenuEntry
	Label    string // trigger text, or its accessible name when IconOnly
	Icon     IconName
	IconOnly bool
	Variant  ButtonVariant
	// Count is a badge on the trigger, e.g. active filter values; 0 hides it.
	Count int
	// Open is the optional open state; nil lets the menu keep its own.
	Open *bool
}

// ContextMenuProps mirrors contract.ts ContextMenuProps.
type ContextMenuProps struct {
	Items []MenuEntry
	// Open is the optional open state; nil lets the menu keep its own.
	Open *bool
}

// Menu draws the trigger button and its menu. It returns the trigger and the id of the entry
// chosen this frame ("" when none); the menu closes on a choice. Items are chosen with
// pointer, Enter/Space; Up/Down/Home/End/type-ahead move, Right/Left open/close submenus,
// Esc closes and focus returns to the trigger (MenuBase).
func (k Kit) Menu(c *ui.Context, p MenuProps) (trigger ui.Element, chosen string) {
	if p.Open == nil {
		host := ui.Row(c)
		open := ui.Local(host, "uikit.menu.open", func() bool { return false })
		host.Children(func() {
			trigger, chosen = k.Menu(c, MenuProps{
				Items: p.Items, Label: p.Label, Icon: p.Icon, IconOnly: p.IconOnly,
				Variant: p.Variant, Count: p.Count, Open: open,
			})
		})
		return trigger, chosen
	}
	m := ui.MenuBase(c, p.Open)
	trigger = m.Trigger
	k.menuTrigger(c, trigger, p)
	m.Popup(func(panel ui.Element) {
		k.popupSurface(panel)
		chosen = k.MenuEntries(c, m, p.Items)
	})
	return trigger, chosen
}

// ContextMenu opens Items at the pointer on right-click (Control-click on macOS) or at the
// focused element with the menu key / Shift+F10 inside body. It returns the chosen id.
func (k Kit) ContextMenu(c *ui.Context, p ContextMenuProps, body func()) (chosen string) {
	area := ui.Column(c)
	area.Children(body)
	open := p.Open
	if open == nil {
		open = ui.Local(area, "uikit.ctxmenu.open", func() bool { return false })
	}
	m := ui.ContextMenuBase(c, area, open)
	m.Popup(func(panel ui.Element) {
		k.popupSurface(panel)
		chosen = k.MenuEntries(c, m, p.Items)
	})
	return chosen
}

// MenuEntries builds items into the popup of m (inside m.Popup / m.Submenu) with the skin's
// row look and returns the id chosen this frame. Menu and ContextMenu use it; call it
// directly to host the same rows in another MenuBase.
func (k Kit) MenuEntries(c *ui.Context, m ui.MenuParts, items []MenuEntry) (chosen string) {
	prim, _, tert := k.fgTone()
	gutter := k.Skin == MacOS
	anyChecked := false
	for _, e := range items {
		if e.Icon != "" || e.Swatch != "" || e.Checked {
			gutter = true
		}
		anyChecked = anyChecked || e.Checked
	}
	for _, e := range items {
		if e.Separator {
			sep := ui.Box(c).Height(1).Background(k.dividerColor())
			if k.Skin == MacOS {
				sep.Margin(4, 8)
			} else {
				sep.Margin(4, 0)
			}
			continue
		}
		var item ui.Element
		switch {
		case len(e.Children) > 0:
			item = m.SubmenuItem()
		case anyChecked:
			item = m.RadioItem(e.Checked)
		default:
			item = m.Item()
		}
		k.menuRowStyle(item)
		col, hi := prim, k.hoverFill()
		switch {
		case e.Disabled:
			col = tert
		case e.Destructive:
			col = k.dangerText()
		}
		if k.Skin == MacOS {
			hi = k.Color("accent")
		}
		item.TextColor(col).Disabled(e.Disabled)
		if item.Highlighted() && !e.Disabled {
			item.Background(hi)
			if k.Skin == MacOS {
				item.TextColor(k.Color("on-accent"))
			}
		}
		item.Children(func() {
			if gutter {
				g := ui.Row(c).Size(16, 16).Justify(ui.Center).AlignItems(ui.Center)
				g.Children(func() {
					switch {
					case e.Icon != "":
						k.Icon(c, e.Icon, 14)
					case e.Swatch != "":
						ui.Box(c).Size(12, 12).Radius(3).Background(k.swatchColor(e.Swatch))
					case e.Checked:
						drawCheck(ui.Box(c).Size(14, 14), col, 1.6)
					}
				})
			}
			ui.Text(c, e.Label).Grow(1).FontSize(k.menuFont())
			if e.Count != "" {
				ui.Text(c, e.Count).FontSize(k.menuFont() - 1).Opacity(.6)
			}
			if e.Shortcut != "" {
				ui.Text(c, e.Shortcut).FontSize(k.menuFont() - 1).Opacity(.6)
			}
			if len(e.Children) > 0 {
				k.Icon(c, IconChevronRight, 10)
			}
		})
		if len(e.Children) > 0 {
			m.Submenu(item, func(panel ui.Element) {
				k.popupSurface(panel)
				if id := k.MenuEntries(c, m, e.Children); id != "" {
					chosen = id
				}
			})
			continue
		}
		if item.Clicked() && !e.Disabled {
			chosen = e.ID
		}
	}
	return chosen
}

func (k Kit) menuFont() float32 {
	switch k.Skin {
	case MacOS:
		return 13
	case Windows:
		return 14
	}
	return 14
}

// menuRowStyle sizes one menu row per skin.
func (k Kit) menuRowStyle(item ui.Element) {
	switch k.Skin {
	case MacOS:
		item.Height(24).Padding(0, 10).Radius(7).Gap(10)
	case Windows:
		item.Height(32).Padding(0, 12).Radius(k.Px("borderRadiusMedium")).Gap(10)
	default:
		item.Height(32).Padding(0, 12).Radius(6).Gap(10)
	}
	item.AlignItems(ui.Center)
}

// menuTrigger gives the MenuBase trigger the look of a button of the requested variant.
func (k Kit) menuTrigger(c *ui.Context, t ui.Element, p MenuProps) {
	h := map[SkinID]float32{Linux: 34, MacOS: 22, Windows: 32}[k.Skin]
	prim, _, _ := k.fgTone()
	t.AlignSelf(ui.Start).Height(h).Radius(k.ControlRadius()).Gap(6).AlignItems(ui.Center).Justify(ui.Center).
		Label(p.Label).TextColor(prim)
	if p.IconOnly {
		t.Width(h).Tooltip(p.Label)
	} else {
		t.Padding(0, 12)
	}
	switch p.Variant {
	case Primary:
		t.Background(k.Sem("primary")).TextColor(k.Sem("primaryForeground"))
	case Subtle, Link:
		if t.Hovered() {
			t.Background(k.hoverFill())
		}
	case Destructive:
		t.Background(k.Sem("destructive")).TextColor(k.Sem("primaryForeground"))
	default:
		switch k.Skin {
		case MacOS:
			t.Background(k.Color("ctrl"))
			k.shadowed(t, "ctrl-shadow")
		case Windows:
			t.Background(k.Color("colorNeutralBackground1")).Border(1, k.Color("colorNeutralStroke1"))
		default:
			t.Background(k.Color("btn"))
			if t.Hovered() {
				t.Background(k.Color("btn-hover"))
			}
		}
	}
	t.Children(func() {
		if p.Icon != "" {
			k.Icon(c, p.Icon, 16)
		}
		if !p.IconOnly {
			ui.Text(c, p.Label).FontSize(k.menuFont())
			if p.Count > 0 {
				ui.Text(c, itoa(p.Count)).FontSize(11).Padding(1, 6).Radius(8).
					Background(k.Sem("primary")).TextColor(k.Sem("primaryForeground"))
			}
		}
	})
}
