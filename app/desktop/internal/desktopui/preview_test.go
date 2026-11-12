package desktopui

import "github.com/uniclipboard/unic/app/desktop/internal/desktopdemo/fixtures"

const quickPanelReference = fixtures.QuickReference

func newDemo(page, theme string) *viewState {
	return NewPreview(fixtures.Desktop(page, theme)).d
}
func newQuickDemo(theme, mode string) *quickPanelState {
	return newPreviewQuick(fixtures.Quick(theme, mode))
}
