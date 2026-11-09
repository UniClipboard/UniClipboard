package uikit

import "github.com/egoist/mygo/ui"

// WithTooltip attaches a hover/focus hint to anchor (contract.ts WithTooltip) and returns
// anchor. An empty text attaches nothing. The tip shows above the anchor (below when there is
// no room), never takes the pointer, and is described to assistive technology through the
// anchor's description, so icon-only controls should still carry their own Label.
func (k Kit) WithTooltip(c *ui.Context, anchor ui.Element, text string) ui.Element {
	if text == "" {
		return anchor
	}
	anchor.Description(text)
	ui.TooltipBase(c, anchor, func(tip ui.Element) {
		k.tooltipSurface(tip)
		tip.Children(func() { ui.Text(c, text).FontSize(k.tooltipFont()) })
	})
	return anchor
}

func (k Kit) tooltipFont() float32 {
	switch k.Skin {
	case MacOS:
		return 11
	case Windows:
		return k.Px("fontSizeBase200")
	}
	return 13
}

// tooltipSurface is the skin's tooltip material: dark toast (Adwaita), content card with menu
// shadow (AppKit help tag), Fluent elevated card.
func (k Kit) tooltipSurface(tip ui.Element) {
	switch k.Skin {
	case MacOS:
		tip.Padding(3, 7).Radius(6).Background(k.Color("content")).TextColor(k.Color("label"))
		k.shadowed(tip, "menu-shadow")
	case Windows:
		tip.Padding(4, 8).Radius(k.Px("borderRadiusMedium")).Background(k.Color("colorNeutralBackground1")).
			Border(1, k.Color("w11-stroke-elevation")).TextColor(k.Color("colorNeutralForeground1"))
		k.shadowed(tip, "shadow8")
	default:
		tip.Padding(4, 9).Radius(6).Background(k.Color("toast")).TextColor(k.Color("toast-fg"))
		k.shadowed(tip, "toast-shadow")
	}
}
