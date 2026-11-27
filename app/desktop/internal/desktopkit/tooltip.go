package desktopkit

import "github.com/egoist/mygo/ui"

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

// WithRichTooltip attaches a styled multi-line hover tooltip surface to anchor.
func (k Kit) WithRichTooltip(c *ui.Context, anchor ui.Element, description string, body func()) ui.Element {
	if body == nil {
		return anchor
	}
	if description != "" {
		anchor.Description(description)
	}
	ui.TooltipBase(c, anchor, func(tip ui.Element) {
		tip.Padding(9, 12).Radius(8).MaxWidth(400).
			Background(k.Color("background-popover")).
			Border(1, k.Color("separator-border")).
			TextColor(k.Color("text-primary"))
		k.shadowed(tip, "dropdown")
		tip.Children(body)
	})
	return anchor
}

func (k Kit) tooltipFont() float32 {
	{
	}

	return 13
}

func (k Kit) tooltipSurface(tip ui.Element) {
	{

		tip.Padding(4, 9).Radius(6).Background(k.Color("background-primary-default")).TextColor(k.Color("text-primary"))
		k.shadowed(tip, "dropdown")
	}

}
