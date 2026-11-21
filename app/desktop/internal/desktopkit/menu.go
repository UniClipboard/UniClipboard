package desktopkit

import "github.com/egoist/mygo/ui"

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

type MenuProps struct {
	Items    []MenuEntry
	Label    string // trigger text, or its accessible name when IconOnly
	Icon     IconName
	IconOnly bool
	Variant  ButtonVariant
	Size     ControlSize
	// Count is a badge on the trigger, e.g. active filter values; 0 hides it.
	Count int
	// Open is the optional open state; nil lets the menu keep its own.
	Open *bool
}

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

	// Native triggers sit directly beside commands, without a MenuBase host.

	if p.Open == nil {
		host := ui.Row(c)
		open := ui.Local(host, "desktopkit.menu.open", func() bool { return false })
		host.Children(func() {
			trigger, chosen = k.Menu(c, MenuProps{
				Items: p.Items, Label: p.Label, Icon: p.Icon, IconOnly: p.IconOnly,
				Variant: p.Variant, Size: p.Size, Count: p.Count, Open: open,
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

func (k Kit) ContextMenu(c *ui.Context, p ContextMenuProps, body func()) (chosen string) {
	area := ui.Column(c)
	area.Children(body)

	open := p.Open
	if open == nil {
		open = ui.Local(area, "desktopkit.ctxmenu.open", func() bool { return false })
	}
	m := ui.ContextMenuBase(c, area, open)
	m.Popup(func(panel ui.Element) {
		k.popupSurface(panel)
		chosen = k.MenuEntries(c, m, p.Items)
	})
	return chosen
}

func (k Kit) MenuEntries(c *ui.Context, m ui.MenuParts, items []MenuEntry) (chosen string) {
	prim, _, tert := k.fgTone()
	gutter := false
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

			{
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

		item.TextColor(col).Disabled(e.Disabled)
		if item.Highlighted() && !e.Disabled {
			item.Background(hi)

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
	{
	}

	return 14
}

func (k Kit) menuRowStyle(item ui.Element) {
	{

		item.Height(32).Padding(0, 12).Radius(6).Gap(10)
	}

	item.AlignItems(ui.Center)
}

// menuTrigger gives the MenuBase trigger the look of a button of the requested variant.
func (k Kit) menuTrigger(c *ui.Context, t ui.Element, p MenuProps) {
	label := p.Label
	if p.IconOnly {
		label = ""
	}
	k.dressButton(c, t, label, ButtonProps{Variant: p.Variant, Size: p.Size, Icon: p.Icon, Label: p.Label}, false)
	t.AlignSelf(ui.Center)
	if p.Count > 0 {
		t.Description(itoa(p.Count))
	}
}
