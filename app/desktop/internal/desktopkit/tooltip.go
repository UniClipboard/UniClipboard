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
