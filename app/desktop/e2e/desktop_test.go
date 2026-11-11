package e2e

import (
	"os"
	"os/exec"
	"testing"

	"github.com/rogpeppe/go-internal/testscript"
)

// Contract failure inventory is written before the runner/driver implementation.
func TestDesktopCatalog(t *testing.T) {
	cmd := exec.Command("python3", "run.py", "catalog", "--check")
	if out, err := cmd.CombinedOutput(); err != nil {
		t.Fatalf("catalogue: %v\n%s", err, out)
	}
}
func TestDesktopScenarios(t *testing.T) {
	if os.Getenv("UNIC_DESKTOP_E2E") != "1" {
		t.Skip("run make desktop-e2e")
	}
	testscript.Run(t, testscript.Params{Dir: "cases", Setup: setupDesktopScenario, Cmds: desktopCommands(), WorkdirRoot: desktopWorkdir(t)})
}
