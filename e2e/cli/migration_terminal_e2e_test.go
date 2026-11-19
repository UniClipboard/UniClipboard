//go:build !windows

package e2e

import (
	"bytes"
	"context"
	"os"
	"os/exec"
	"path/filepath"
	"reflect"
	"strings"
	"testing"
	"time"

	"github.com/creack/pty"
	"golang.org/x/term"

	"github.com/uniclipboard/unic/internal/v1migration/v1fixture"
)

// A private PTY verifies the actual CLI's terminal restoration after SIGINT.
func TestMigrationTerminalCancellation(t *testing.T) {
	if os.Getenv("UNIC_E2E") == "" {
		t.Skip("set UNIC_E2E=1")
	}
	root := shortTemp(t)
	source, _, _, err := v1fixture.Build(filepath.Join(root, "source"))
	if err != nil {
		t.Fatal(err)
	}
	master, slave, err := pty.Open()
	if err != nil {
		t.Fatal(err)
	}
	defer func() { _ = master.Close(); _ = slave.Close() }()
	before, err := term.GetState(int(slave.Fd()))
	if err != nil {
		t.Fatal(err)
	}
	ctx, cancel := context.WithTimeout(context.Background(), 20*time.Second)
	defer cancel()
	cmd := exec.CommandContext(ctx, filepath.Join(binDir, "unic"), "migrate", "--source", source, "--service", "unic-e-task-nonexistent-credential", "--json")
	cmd.Env = append(os.Environ(), "UNIC_HOME="+filepath.Join(root, "target"))
	cmd.Stdin = slave
	var stderr lockedBuffer
	cmd.Stderr = &stderr
	cmd.Stdout = &stderr
	if err = cmd.Start(); err != nil {
		t.Fatal(err)
	}
	deadline := time.Now().Add(10 * time.Second)
	for !strings.Contains(stderr.String(), "Old UniClipboard passphrase:") {
		if time.Now().After(deadline) {
			_ = cmd.Process.Kill()
			t.Fatalf("private terminal prompt did not appear: %s", stderr.String())
		}
		time.Sleep(10 * time.Millisecond)
	}
	// Wait until ReadPassword actually disabled echo, before cancellation.
	for {
		state, e := term.GetState(int(slave.Fd()))
		if e != nil {
			t.Fatal(e)
		}
		if !reflect.DeepEqual(before, state) {
			break
		}
		if time.Now().After(deadline) {
			_ = cmd.Process.Kill()
			t.Fatal("passphrase did not hide echo")
		}
		time.Sleep(10 * time.Millisecond)
	}
	if err = cmd.Process.Signal(os.Interrupt); err != nil {
		t.Fatal(err)
	}
	if err = cmd.Wait(); err == nil {
		t.Fatal("interrupted prompt succeeded")
	}
	after, err := term.GetState(int(slave.Fd()))
	if err != nil {
		t.Fatal(err)
	}
	if !reflect.DeepEqual(before, after) {
		t.Fatal("SIGINT left terminal echo/state changed")
	}
	if bytes.Contains([]byte(stderr.String()), []byte("fixture-passphrase")) {
		t.Fatal("passphrase leaked")
	}
	t.Log("private PTY state restored after cancelling actual CLI passphrase prompt; no real credential or GUI prompt was used")
}
