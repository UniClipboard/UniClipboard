package e2e_test

import (
	"encoding/json"
	"os"
	"os/exec"
	"path/filepath"
	"runtime"
	"strconv"
	"strings"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"
)

// Software-view resource measurements are descriptive. No GC, working-set
// trimming or native-window/memory-reduction claim is involved.
func measure(t *testing.T, tt *ui.Tester, first time.Duration, name string) {
	t.Helper()
	start := time.Now()
	for i := 0; i < 100; i++ {
		mustClick(t, tt, "Role")
		tt.Key(0, ui.KeyEscape)
	}
	elapsed := time.Since(start)
	var mem runtime.MemStats
	runtime.ReadMemStats(&mem)
	m := map[string]any{"scope": "software-view E2E process; first ready frame includes tester creation and explicit en-US/scale setup, excludes font registration; cumulative process RSS; no native window", "first_frame_ms": float64(first) / float64(time.Millisecond), "popup_cycles": 100, "popup_cycles_ms": float64(elapsed) / float64(time.Millisecond), "go_heap_alloc_bytes": mem.HeapAlloc, "go_heap_inuse_bytes": mem.HeapInuse, "pid": os.Getpid(), "os": runtime.GOOS, "arch": runtime.GOARCH, "go": runtime.Version(), "forced_gc": false}
	if runtime.GOOS == "darwin" || runtime.GOOS == "linux" {
		if b, err := exec.Command("ps", "-o", "rss=", "-p", strconv.Itoa(os.Getpid())).Output(); err == nil {
			if n, err := strconv.ParseInt(strings.TrimSpace(string(b)), 10, 64); err == nil {
				m["process_rss_kib"] = n
			}
		}
	}
	b, err := json.MarshalIndent(m, "", "  ")
	if err != nil {
		t.Fatal(err)
	}
	root := os.Getenv("UNIC_DESKTOP_E2E_CASE_ARTIFACTS")
	if root == "" {
		root = os.Getenv("BOARDUI_E2E_ARTIFACTS")
	}
	if err = os.WriteFile(filepath.Join(root, name+"-metrics.json"), b, 0644); err != nil {
		t.Fatal(err)
	}
}
