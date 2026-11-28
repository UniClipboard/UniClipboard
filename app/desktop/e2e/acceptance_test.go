package e2e

import (
	"os"
	"testing"
)

// TestDesktopAcceptance is called only by the canonical Desktop runner.
func TestDesktopAcceptance(t *testing.T) {
	if os.Getenv("UNIC_DESKTOP_E2E") != "1" {
		t.Skip("run make desktop-e2e")
	}
	t.Run("startup-lifecycle", scenarioStartupProcesses)
	t.Run("desktop-processes", scenarioDesktopProcesses)
	t.Run("desktop-lightweight-close", scenarioLightweightClose)
	t.Run("desktop-lightweight-invalid-config-close", scenarioLightweightClose)
	t.Run("first-install-setup", scenarioFirstInstallSetup)
	t.Run("production-boundary", scenarioProductionBoundary)
	t.Run("standalone-quick-panel-lifecycle", scenarioStandaloneQuickPanelLifecycle)
	t.Run("resource-telemetry", scenarioResourceTelemetry)
}
