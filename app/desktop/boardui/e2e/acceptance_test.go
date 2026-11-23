package e2e_test

import (
	"os"
	"testing"
)

func TestDesktopAcceptance(t *testing.T) {
	if os.Getenv("UNIC_DESKTOP_E2E") != "1" {
		t.Skip("run make desktop-e2e CASE=boardui-settings")
	}
	t.Run("boardui-settings", scenarioSettingsEndToEnd)
	t.Run("boardui-states", scenarioStatesEndToEnd)
	t.Run("boardui-theme", scenarioThemeEndToEnd)
	t.Run("boardui-palette", scenarioPaletteEndToEnd)
}
