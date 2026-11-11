package main

import (
	"os"
	"testing"
)

// TestDesktopAcceptance is called only by the canonical Desktop runner.
func TestDesktopAcceptance(t *testing.T) {
	if os.Getenv("UNIC_DESKTOP_E2E") != "1" {
		t.Skip("run make desktop-e2e")
	}
	t.Run("gui-navigation", scenarioGUINavigation)
	t.Run("gui-approval-reconnect", scenarioGUIApprovalAndReconnect)
	t.Run("gui-empty-layout", scenarioGUIEmptyHistoryLayout)
}
