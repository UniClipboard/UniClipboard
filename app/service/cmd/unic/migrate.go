package main

import (
	"errors"
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"runtime"
)

const migrationGuide = `UniClipboard v1 to v2 history migration

v2 cannot sync with v1 or directly use its database, settings or device identity.
Upgrade all devices to v2 and pair them again. Configure v2 settings separately.

1. Back up your v1 data. Keep the backup until you have checked the v2 history.
2. Quit v1 and stop unicd for the target profile. Migration does not stop them.
3. Check recoverable history without writing the target:
     unic migrate --dry-run --json
4. Import history and verify the copied content:
     unic migrate --json
5. Check the report and v2 history before starting unicd.

Default source: the local v1 desktop data directory.
Default target: UNIC_HOME or the normal unic data directory.
Custom directories: add --source DIR --target DIR to both commands.
Named profiles: add --source DIR --service UniClipboard-PROFILE for that profile.

Migration imports supported text, HTML, images, files and tags. It does not
import settings, device identities, trust or favorites. Missing or damaged
content is reported as partial. Exit codes: 0 ready/complete, 3 partial, 1 failed.
The source remains unchanged. Rerun the same command after an interruption;
successful records are not duplicated. A dry run is not a completed import.

Allow the system credential prompt or enter the old passphrase when requested.
For automation use --passphrase-stdin. Never put a passphrase in arguments or
environment variables. Custom or portable v1 profiles require --source DIR.
`

// Run the shipped helper directly, without shell interpolation or exposing
// the passphrase in arguments. The helper owns locking and import recovery.
func runMigration(args []string) error {
	if len(args) == 0 || len(args) == 1 && (args[0] == "--help" || args[0] == "-h" || args[0] == "help") {
		_, err := fmt.Fprint(os.Stdout, migrationGuide)
		return err
	}
	executable, err := os.Executable()
	if err != nil {
		return errors.New("cannot locate migration helper")
	}
	name := "unic-migrate"
	if runtime.GOOS == "windows" {
		name += ".exe"
	}
	helper := filepath.Join(filepath.Dir(executable), name)
	if st, err := os.Stat(helper); err != nil || !st.Mode().IsRegular() {
		return errors.New("unic-migrate must be installed beside unic")
	}
	cmd := exec.Command(helper, args...)
	cmd.Stdin, cmd.Stdout, cmd.Stderr = os.Stdin, os.Stdout, os.Stderr
	if err := cmd.Run(); err != nil {
		return fmt.Errorf("migration did not complete; see its report and rerun to resume: %w", err)
	}
	return nil
}
