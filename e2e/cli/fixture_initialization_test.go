package e2e

import (
	"context"
	"errors"
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"time"
)

// initializeFixtureProfile completes the production CLI's explicit no-source
// initialization once, before a fixture starts any service. Discovery runs in
// a private empty home; it must never inspect a developer's legacy profile.
// Business-command helpers do not call this function.
func initializeFixtureProfile(executable, home string, environment []string) (result error) {
	discovery, err := os.MkdirTemp("", "unic-cli-source-")
	if err != nil {
		return err
	}
	defer func() { result = errors.Join(result, os.RemoveAll(discovery)) }()
	ctx, cancel := context.WithTimeout(context.Background(), 30*time.Second)
	defer cancel()
	cmd := exec.CommandContext(ctx, executable, "migrate", "--initialize-empty", "--target", home, "--json")
	cmd.Env = append(environment, "UNIC_HOME="+home, "HOME="+discovery, "USERPROFILE="+discovery,
		"XDG_DATA_HOME="+filepath.Join(discovery, "data"), "LOCALAPPDATA="+filepath.Join(discovery, "local"), "UC_PROFILE=")
	if out, err := cmd.CombinedOutput(); err != nil {
		return fmt.Errorf("initialize isolated CLI fixture: %w: %s", err, out)
	}
	return nil
}
