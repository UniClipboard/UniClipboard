package desktopkit

import (
	"time"

	"github.com/egoist/mygo/ui"

	"github.com/uniclipboard/unic/app/desktop/boardui"
)

// Toolbar is a row of commands with roving focus: one tab stop, Left/Right/Home/End move
// between items (mygo Toolbar), and items that do not fit fold into an overflow menu.
func (k Kit) Toolbar(c *ui.Context, label string, body func()) ui.Element {
	t := ui.Toolbar(c, body).Label(label).AlignItems(ui.Center)
	{

		t.Gap(6)
	}

	return t
}

func (k Kit) ToolbarGroup(c *ui.Context, body func()) ui.Element {
	g := ui.Row(c).AlignItems(ui.Center)

	{
		g.Gap(2)
	}
	return g.Children(body)
}

type ToolbarButtonProps struct {
	Variant ButtonVariant
	Icon    IconName
	// Label is the accessible name of an icon-only item (empty text); ignored otherwise.
	Label string
	// Tooltip is the hover/focus hint; icon-only items default to Label.
	Tooltip  string
	Disabled bool
}

// ToolbarButton is a command-bar item; read the click with .Clicked(). text == "" makes it
// icon-only, which then needs Label. Primary is an accent button, the rest are subtle.
func (k Kit) ToolbarButton(c *ui.Context, text string, p ToolbarButtonProps) ui.Element {
	v := p.Variant
	size := SizeMedium
	if v == Standard {
		v = Subtle
	}
	if v == Subtle {
		size = SizeSmall
	}
	tip := p.Tooltip
	if tip == "" && text == "" {
		tip = p.Label
	}
	return k.Button(c, text, ButtonProps{
		Variant: v, Size: size, Icon: p.Icon, Label: p.Label, Tooltip: tip, Disabled: p.Disabled,
	})
}

// ToolbarIconButtonProps configures a quiet 28×28 neutral icon command in the top toolbar.
type ToolbarIconButtonProps struct {
	Icon        IconName
	Label       string
	Tooltip     string
	Active      bool
	ActiveColor ui.Color
	Disabled    bool
	Busy        bool
}

// ToolbarIconButton renders a quiet 28×28 neutral toolbar icon action with 0.98 press motion.
func (k Kit) ToolbarIconButton(c *ui.Context, p ToolbarIconButtonProps) ui.Element {
	return k.dressQuietButton(c, ui.ButtonBase(c), "", quietButtonProps{
		Size:        SizeSmall,
		Icon:        p.Icon,
		Label:       p.Label,
		Tooltip:     p.Tooltip,
		Active:      p.Active,
		ActiveColor: p.ActiveColor,
		Disabled:    p.Disabled,
		Busy:        p.Busy,
	})
}

// ToolbarQuietButtonProps configures a quiet 28-DIP neutral toolbar command with an optional icon and label.
type ToolbarQuietButtonProps struct {
	Icon        IconName
	Label       string
	Tooltip     string
	Active      bool
	ActiveColor ui.Color
	Disabled    bool
	Busy        bool
}

// ToolbarQuietButton renders a quiet 28-DIP neutral toolbar command matching ToolbarIconButton's
// surface, neutral ink, hover/press feedback, and 0.98 press motion.
func (k Kit) ToolbarQuietButton(c *ui.Context, text string, p ToolbarQuietButtonProps) ui.Element {
	return k.dressQuietButton(c, ui.ButtonBase(c), text, quietButtonProps{
		Size:        SizeSmall,
		Icon:        p.Icon,
		Label:       p.Label,
		Tooltip:     p.Tooltip,
		Active:      p.Active,
		ActiveColor: p.ActiveColor,
		Disabled:    p.Disabled,
		Busy:        p.Busy,
	})
}

// ToolbarIconMenuProps configures a quiet 28×28 neutral icon menu trigger in the top toolbar.
type ToolbarIconMenuProps struct {
	Items    []MenuEntry
	Icon     IconName
	Label    string
	Tooltip  string
	Disabled bool
	Open     *bool
}

// ToolbarIconMenu renders a quiet 28×28 neutral toolbar menu trigger backed by MenuBase.
func (k Kit) ToolbarIconMenu(c *ui.Context, p ToolbarIconMenuProps) (trigger ui.Element, chosen string) {
	if p.Open == nil {
		host := ui.Row(c)
		open := ui.Local(host, "desktopkit.toolbar.menu.open", func() bool { return false })
		host.Children(func() {
			trigger, chosen = k.ToolbarIconMenu(c, ToolbarIconMenuProps{
				Items: p.Items, Icon: p.Icon, Label: p.Label, Tooltip: p.Tooltip,
				Disabled: p.Disabled, Open: open,
			})
		})
		return trigger, chosen
	}
	if p.Disabled && *p.Open {
		*p.Open = false
	}
	m := ui.MenuBase(c, p.Open)
	trigger = m.Trigger
	k.dressQuietButton(c, trigger, "", quietButtonProps{
		Size: SizeSmall, Icon: p.Icon, Label: p.Label, Tooltip: p.Tooltip, Disabled: p.Disabled,
	})
	m.Popup(func(panel ui.Element) {
		k.popupSurface(panel)
		k.menuEntriesInto(c, m, p.Items, &chosen)
	})
	return trigger, chosen
}

type quietButtonProps struct {
	Size        ControlSize
	Icon        IconName
	Label       string
	Tooltip     string
	Active      bool
	ActiveColor ui.Color
	Disabled    bool
	Busy        bool
}

func (k Kit) dressQuietButton(c *ui.Context, e ui.Element, text string, p quietButtonProps) ui.Element {
	name := p.Label
	if name == "" {
		name = text
	}
	tip := p.Tooltip
	if tip == "" && text == "" {
		tip = name
	}
	disabled := p.Disabled || p.Busy
	e.AlignItems(ui.Center).Justify(ui.Center).Shrink(0).
		Label(name).Disabled(disabled).DimDisabled(false).Busy(p.Busy).
		FocusRingColor(k.Color("border-focus-ring")).FocusRingOffset(2)
	if tip != "" {
		e.Tooltip(tip)
	}
	switch {
	case text == "":
		e.Size(28, 28).Radius(7)
	case p.Size == SizeSmall:
		e.Height(28).Padding(0, 8).Gap(5).Radius(7)
	default:
		e.Height(32).Padding(0, 10).Gap(6).Radius(8)
	}

	pressed := !disabled && (e.Pressed() || (e.Focused() && c.KeyDown(ui.KeySpace)))
	hoverBg := ui.RGB(243, 243, 246)
	pressBg := ui.RGB(236, 236, 240)
	if k.Dark {
		hoverBg = ui.RGBA(255, 255, 255, .06)
		pressBg = ui.RGBA(255, 255, 255, .10)
	}
	ink := k.Color("foreground-icon-secondary")
	textInk := k.Color("text-secondary")
	if p.Active && !disabled && p.ActiveColor.A > 0 {
		ink = p.ActiveColor
		textInk = p.ActiveColor
	}
	switch {
	case p.Busy:
		e.Opacity(.65)
	case disabled:
		e.Opacity(.35)
	case pressed:
		e.Background(pressBg)
		if !p.Active {
			ink = k.Color("foreground-icon-primary")
			textInk = k.Color("text-primary")
		}
	case e.Hovered():
		e.Background(hoverBg)
		if !p.Active {
			ink = k.Color("foreground-icon-primary")
			textInk = k.Color("text-primary")
		}
	}
	e.TextColor(ink)
	e.Children(func() {
		if p.Busy {
			ui.Spinner(c.Key("busy")).Size(15, 15).Shrink(0).TextColor(ink).AccessHidden(true)
		} else if p.Icon != "" {
			k.Icon(c.Key("ic"), p.Icon, 15).TextColor(ink).AccessHidden(true)
		}
		if text != "" {
			k.Text(c.Key("lbl"), text, TextProps{Role: RoleCallout, Emphasized: true, Truncate: true}).TextColor(textInk)
		}
	})

	// Match BoardUI source 0.98 press / return motion.
	scale := float32(1)
	duration := time.Duration(boardui.Number("return-ms")) * time.Millisecond
	if pressed && !c.Preferences().ReduceMotion {
		scale = boardui.Number("press-scale")
		duration = time.Duration(boardui.Number("press-ms")) * time.Millisecond
	}
	if c.Preferences().ReduceMotion {
		duration = 0
	}
	s := e.AnimateTo("toolbar-press-scale", scale, ui.Animation{Duration: duration, Ease: ui.EaseOut})
	if s != 1 {
		e.Scale(s, s)
	}
	return e
}

// ToolbarDivider is a thin vertical separator between toolbar items.
func (k Kit) ToolbarDivider(c *ui.Context) ui.Element {
	return ui.Box(c).Width(1).Height(20).Margin(0, 4).Background(k.dividerColor())
}
