package main

import (
	"fmt"
	"os"
	"path/filepath"
	"strings"

	"golang.org/x/term"

	paths "github.com/uniclipboard/unic/internal/localpaths"
)

const upgradeNotice = `WARNING: v1 data directory detected. UniClipboard v2 is incompatible with v1.
Back up v1 data, quit v1, and stop unicd before migration.
Run: unic migrate --help
Preflight: unic migrate --dry-run --json
Import: unic migrate --json
`

// Only inspect known locations. Do not create a profile or read credentials.
func showUpgradeNotice(args []string) {
	if !term.IsTerminal(int(os.Stderr.Fd())) {
		return
	}
	for _, arg := range args {
		if arg == "--json" || arg == "-json" || strings.HasPrefix(arg, "--json=") || strings.HasPrefix(arg, "-json=") {
			return
		}
	}
	dir, err := paths.V1DataDir()
	if err != nil {
		return
	}
	dirs := []string{dir}
	if profile := os.Getenv("UC_PROFILE"); profile != "" && !strings.ContainsAny(profile, `/\`) {
		dirs = append(dirs, filepath.Join(filepath.Dir(dir), filepath.Base(dir)+"-"+profile))
	}
	for _, candidate := range dirs {
		if st, err := os.Stat(candidate); err == nil && st.IsDir() {
			_, _ = fmt.Fprint(os.Stderr, upgradeNotice)
			return
		}
	}
}
