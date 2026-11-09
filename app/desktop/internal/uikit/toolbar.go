package uikit

import "github.com/egoist/mygo/ui"

// Toolbar is a row of commands with roving focus: one tab stop, Left/Right/Home/End move
// between items (mygo Toolbar), and items that do not fit fold into an overflow menu.
// label is its accessible name. Windows draws a WinUI CommandBar card; macOS and Adwaita draw
// a bare row (the window places it in the title bar).
func (k Kit) Toolbar(c *ui.Context, label string, body func()) ui.Element {
	t := ui.Toolbar(c, body).Label(label).AlignItems(ui.Center)
	switch k.Skin {
	case Windows:
		t.Gap(4).Padding(8).Radius(k.Px("borderRadiusLarge")).
			Background(k.Color("w11-card")).Border(1, k.Color("colorNeutralStroke1"))
	case MacOS:
		t.Gap(8)
	default:
		t.Gap(6)
	}
	return t
}

// ToolbarGroup holds items that belong together (Send, Pin, More). macOS draws one glass
// capsule around them; Windows and Adwaita draw a plain run of items.
func (k Kit) ToolbarGroup(c *ui.Context, body func()) ui.Element {
	g := ui.Row(c).AlignItems(ui.Center)
	if k.Skin == MacOS {
		g.Gap(0).Height(32).Padding(0, 4).Radius(16)
		k.glassSurface(g)
	} else {
		g.Gap(2)
	}
	return g.Children(body)
}

// ToolbarButtonProps mirrors contract.ts ToolbarButtonProps (a ButtonProps without Size, Busy
// and submit-form). Variant Standard is drawn Subtle, the default of a toolbar item.
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
	if v == Standard {
		v = Subtle
	}
	tip := p.Tooltip
	if tip == "" && text == "" {
		tip = p.Label
	}
	return k.Button(c, text, ButtonProps{
		Variant: v, Icon: p.Icon, Label: p.Label, Tooltip: tip, Disabled: p.Disabled,
	})
}

// ToolbarDivider is a thin vertical separator between toolbar items.
func (k Kit) ToolbarDivider(c *ui.Context) ui.Element {
	if k.Skin == Linux {
		return ui.Box(c).Width(6).Height(20)
	}
	return ui.Box(c).Width(1).Height(20).Margin(0, 4).Background(k.dividerColor())
}
