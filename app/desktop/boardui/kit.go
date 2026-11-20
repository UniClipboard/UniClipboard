package boardui

import "github.com/egoist/mygo/ui"

// Kit selects a color scheme. It never stores mutable control state.
type Kit struct{ Dark bool }

// Apply installs the source-derived theme for controls in this view. LoadFonts
// must have succeeded before the first view is created.
func (k Kit) Apply(c *ui.Context) {
	t := ui.LightTheme()
	if k.Dark {
		t = ui.DarkTheme()
	}
	t.Font = "BoardUI Inter"
	t.FontSize = Typography("body-regular").Size
	t.Background = Token(k.Dark, "background-full")
	t.Text = Token(k.Dark, "text-primary")
	t.TextMuted = Token(k.Dark, "text-secondary")
	t.Surface = Token(k.Dark, "background-primary-default")
	t.SurfaceHover = Token(k.Dark, "background-primary-hover")
	t.SurfacePressed = Token(k.Dark, "background-primary-active")
	t.Border = Token(k.Dark, "border-button-default")
	t.Focus = Token(k.Dark, "border-focus-ring")
	t.FocusRingWidth, t.FocusRingOffset = 2, 2
	t.Accent = Token(k.Dark, "accent-500")
	t.AccentText = Token(k.Dark, "text-white")
	t.Selection = t.Accent.Alpha(.25)
	c.SetTheme(t)
}

// Text applies one complete BoardUI typography record to a native text element.
func (k Kit) Text(c *ui.Context, text, style string) ui.Element {
	e := ui.Text(c, text).TextColor(Token(k.Dark, "text-primary"))
	dressType(e, style)
	return e
}

func dressType(e ui.Element, name string) {
	t := Typography(name)
	e.Font("BoardUI Inter").FontSize(t.Size).FontWeight(t.Weight).
		FixedLineHeight(t.LineHeight).LetterSpacing(t.LetterSpacing)
}

func (k Kit) shadows(e ui.Element, name string) {
	for _, s := range Shadows(k.Dark, name) {
		if s.Inset {
			e.InsetShadow(s.X, s.Y, s.Blur, s.Spread, s.Color)
		} else {
			e.Shadow(s.X, s.Y, s.Blur, s.Spread, s.Color)
		}
	}
}

func normalizedSize(size string) string {
	if size == "" {
		return "medium"
	}
	return size
}
