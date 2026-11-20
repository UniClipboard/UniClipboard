package desktopkit

import "github.com/egoist/mygo/ui"

type ActionBarProps struct {
	// Hint is a display-only hint at the trailing end, e.g. keyboard shortcuts.
	Hint string
}

func (k Kit) ActionBar(c *ui.Context, p ActionBarProps, body func()) ui.Element {
	_, sec, _ := k.fgTone()
	hint := func(size float32) {
		if p.Hint != "" {
			ui.Spacer(c)
			ui.Text(c, p.Hint).FontSize(size).TextColor(sec).Label(p.Hint).SingleLine().MinWidth(0).Shrink(100)
		}
	}
	{
	}

	return ui.Row(c).Gap(10).AlignItems(ui.Center).Children(func() { body(); hint(12.5) })
}
