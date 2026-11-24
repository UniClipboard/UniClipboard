package boardui

import "github.com/egoist/mygo/ui"

// Kit selects a color scheme. It never stores mutable control state.
type Kit struct {
	Dark   bool
	Colors *ColorTheme
}

// Theme maps canonical source colors to mature native primitive defaults.
// LoadFonts must succeed before the first view is created.
func (k Kit) Theme() *ui.Theme {
	t := ui.LightTheme()
	if k.Dark {
		t = ui.DarkTheme()
	}
	t.Font = "BoardUI Inter"
	t.FontSize = Typography("body-regular").Size
	t.Background = k.Color("background-full")
	t.Text = k.Color("text-primary")
	t.TextMuted = k.Color("text-secondary")
	t.Surface = k.Color("background-primary-default")
	t.SurfaceHover = k.Color("background-primary-hover")
	t.SurfacePressed = k.Color("background-primary-active")
	t.Border = k.Color("border-button-default")
	t.Focus = k.Color("border-focus-ring")
	t.FocusRingWidth, t.FocusRingOffset = 2, 2
	t.Accent = k.Color("accent-500")
	t.AccentText = k.Color("text-on-accent")
	t.AccentHover = k.Color("accent-600")
	t.AccentPressed = k.Color("accent-800")
	t.InactiveAccent = k.Color("native-inactive-accent")
	t.Danger = k.Color("text-error-primary")
	t.Success = k.Color("notification-success-foreground")
	t.Warning = k.Color("status-yellow-text")
	t.Radius = Geometry("input", "medium").Radius
	t.Selection = k.Color("native-selection")
	t.Inverse = k.Color("native-inverse")
	t.InverseText = k.Color("native-inverse-text")
	t.Scrollbar = k.Color("native-scrollbar")
	t.ScrollbarWidth = 4
	return t
}

// Apply installs the source theme for this view.
func (k Kit) Apply(c *ui.Context) {
	c.SetTheme(k.Theme())
	c.SetMenuPresentation(ui.PopoverMenus)
}

// Text applies one complete BoardUI typography record to a native text element.
func (k Kit) Text(c *ui.Context, text, style string) ui.Element {
	e := ui.Text(c, text).TextColor(k.Color("text-primary"))
	dressType(e, style)
	return e
}

func dressType(e ui.Element, name string) {
	t := Typography(name)
	e.Font("BoardUI Inter").FontSize(t.Size).FontWeight(t.Weight).
		FixedLineHeight(t.LineHeight).LetterSpacing(t.LetterSpacing)
}

func (k Kit) shadows(e ui.Element, name string) {
	for _, s := range k.Shadows(name) {
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
