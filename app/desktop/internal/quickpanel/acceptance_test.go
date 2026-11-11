package quickpanel_test

import (
	"os"
	"testing"
)

// TestDesktopAcceptance is called only by the canonical Desktop runner.
func TestDesktopAcceptance(t *testing.T) {
	if os.Getenv("UNIC_DESKTOP_E2E") != "1" {
		t.Skip("run make desktop-e2e")
	}
	t.Run("quick-panel-contract", scenarioQuickPanelProcess)
}
