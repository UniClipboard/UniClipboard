package desktopui

import "github.com/uniclipboard/unic/app/desktop/boardui"

// SetColorTheme selects code-defined colors without replacing caller state.
// Call on the view thread and invalidate the host window to paint the change.
// A nil theme restores the source presentation. Independent role processes
// must receive the same code-defined theme from their own host entry.
func (s *Desktop) SetColorTheme(theme *boardui.ColorTheme) {
	s.d.colors = theme
	if s.d.quick != nil {
		s.d.quick.colors = theme
	}
}

// SetColorTheme selects the same palette for the panel and its satellite view.
// Call on the view thread. A live preview window is invalidated immediately.
func (p *QuickPreview) SetColorTheme(theme *boardui.ColorTheme) {
	p.p.colors = theme
	p.p.update()
}
