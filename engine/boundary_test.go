package engine

import (
	"os/exec"
	"strings"
	"testing"
)

// The public engine is what Android, iOS and other hosts will embed. It must
// not depend on anything that only makes sense on a desktop: the GUI toolkit,
// file watching, the configuration file, or the desktop host itself.
func TestPublicEngineHasNoDesktopDependencies(t *testing.T) {
	if _, err := exec.LookPath("go"); err != nil {
		t.Skip("go tool not available")
	}
	out, err := exec.Command("go", "list", "-deps", ".").Output()
	if err != nil {
		t.Skipf("go list: %v", err)
	}
	for _, dep := range strings.Fields(string(out)) {
		for _, banned := range []string{"/app/", "egoist/mygo", "fsnotify", "BurntSushi/toml", "internal/session", "internal/tcnet"} {
			if strings.Contains(dep, banned) {
				t.Errorf("the public engine depends on %s", dep)
			}
		}
	}
}
