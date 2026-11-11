// Package e2eproof captures software-rendered view evidence for Desktop acceptance.
// It is imported by acceptance tests only, never by a release executable.
package e2eproof

import (
	"crypto/sha256"
	"fmt"
	"image/png"
	"os"
	"path/filepath"
	"sync/atomic"
	"testing"

	"github.com/egoist/mygo/ui"
)

var sequence atomic.Uint64

// Capture retains the final exercised state, including a failing subcase.
// These checkpoints are software-view images, not native-window recordings.
func Capture(t *testing.T, view *ui.Tester) {
	t.Helper()
	id := sequence.Add(1)
	t.Cleanup(func() {
		dir := os.Getenv("UNIC_DESKTOP_E2E_CASE_ARTIFACTS")
		if dir == "" {
			t.Fatal("acceptance artifact directory is required")
		}
		name := fmt.Sprintf("view-%04d-%x.png", id, sha256.Sum256([]byte(t.Name())))
		file, err := os.Create(filepath.Join(dir, name))
		if err != nil {
			t.Fatal(err)
		}
		encodeErr := png.Encode(file, view.Image())
		closeErr := file.Close()
		if encodeErr != nil {
			t.Error(encodeErr)
		}
		if closeErr != nil {
			t.Error(closeErr)
		}
	})
}
