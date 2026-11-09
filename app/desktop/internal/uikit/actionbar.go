package uikit

import "github.com/egoist/mygo/ui"

// ActionBarProps mirrors contract.ts ActionBarProps.
type ActionBarProps struct {
	// Hint is a display-only hint at the trailing end, e.g. keyboard shortcuts.
	Hint string
}

// ActionBar is the bar of the main actions of a pane: a floating glass capsule on macOS, a
// plain row on Adwaita, a WinUI command bar over a hint status strip on Windows (the web skin
// moves the two into the pane's slots; here both stay in the returned column). Add the actions
// in body, typically Buttons.
func (k Kit) ActionBar(c *ui.Context, p ActionBarProps, body func()) ui.Element {
	_, sec, _ := k.fgTone()
	hint := func(size float32) {
		if p.Hint != "" {
			ui.Spacer(c)
			ui.Text(c, p.Hint).FontSize(size).TextColor(sec).Label(p.Hint)
		}
	}
	switch k.Skin {
	case MacOS:
		bar := ui.Row(c).Height(64).Padding(0, 14).Radius(20).Gap(10).AlignItems(ui.Center).
			Background(k.Color("glass"))
		k.shadowed(bar, "glass-shadow")
		bar.InsetShadow(0, 0, 0, .5, k.Color("glass-rim"))
		return bar.Children(func() { body(); hint(11) })
	case Windows:
		return ui.Column(c).Children(func() {
			ui.Row(c).Height(48).Padding(0, 12).Gap(4).AlignItems(ui.Center).
				Background(k.Color("w11-card-solid")).Children(body)
			ui.Box(c).Height(1).Background(k.Color("w11-divider"))
			if p.Hint != "" {
				ui.Row(c).Height(32).Padding(0, 16).AlignItems(ui.Center).Children(func() {
					ui.Text(c, p.Hint).FontSize(k.Px("fontSizeBase200")).TextColor(sec).Label(p.Hint)
				})
			}
		})
	}
	return ui.Row(c).Gap(10).AlignItems(ui.Center).Children(func() { body(); hint(12.5) })
}
