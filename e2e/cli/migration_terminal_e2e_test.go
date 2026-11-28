//go:build !windows

package e2e

import (
	"bytes"
	"context"
	"database/sql"
	"os"
	"os/exec"
	"path/filepath"
	"reflect"
	"runtime"
	"strings"
	"testing"
	"time"

	"github.com/creack/pty"
	"golang.org/x/term"

	"github.com/uniclipboard/unic/internal/climigration"
	"github.com/uniclipboard/unic/internal/servicelaunch"
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
	targetDir := filepath.Join(root, "target")
	for _, artifact := range []string{"cli-migration.json", "unic.db"} {
		if _, statErr := os.Stat(filepath.Join(targetDir, artifact)); !os.IsNotExist(statErr) {
			t.Fatalf("cancelled pre-unlock migration created %s", artifact)
		}
	}
	// Verify `unic init --fresh` unblocks cleanly after SIGINT unlock cancellation.
	freshCmd := exec.CommandContext(ctx, filepath.Join(binDir, "unic"), "init", "--fresh", "--json")
	freshCmd.Env = append(os.Environ(), "UNIC_HOME="+targetDir)
	if out, ferr := freshCmd.CombinedOutput(); ferr != nil {
		t.Fatalf("init --fresh after SIGINT failed: %v (%s)", ferr, out)
	}
	t.Log("private PTY state restored after cancelling actual CLI passphrase prompt; no pending state left and init --fresh succeeded")
}

func TestMigrationInteractivePTYFlows(t *testing.T) {
	if os.Getenv("UNIC_E2E") == "" {
		t.Skip("set UNIC_E2E=1")
	}
	root := shortTemp(t)
	userHome := filepath.Join(root, "home")
	dataHome := filepath.Join(root, "data")
	cfgHome := filepath.Join(root, "cfg")
	for _, d := range []string{userHome, dataHome, cfgHome} {
		if err := os.MkdirAll(d, 0o700); err != nil {
			t.Fatal(err)
		}
	}
	v1Dir := filepath.Join(dataHome, "app.uniclipboard.desktop")
	if runtime.GOOS == "darwin" {
		v1Dir = filepath.Join(userHome, "Library", "Application Support", "app.uniclipboard.desktop")
	}
	_, dbpath, fixture, err := v1fixture.Build(v1Dir)
	if err != nil {
		t.Fatal(err)
	}
	db, err := sql.Open("sqlite", dbpath)
	if err != nil {
		t.Fatal(err)
	}
	if _, err := db.Exec(`INSERT INTO clipboard_snapshot_representation VALUES('rep-bad','event-bad','text/plain','text/plain',?,NULL,9)`, []byte(fixture.Repaired)); err != nil {
		_ = db.Close()
		t.Fatal(err)
	}
	_ = db.Close()

	baseEnv := append(os.Environ(),
		"HOME="+userHome,
		"XDG_DATA_HOME="+dataHome,
		"XDG_CONFIG_HOME="+cfgHome,
		"LOCALAPPDATA="+dataHome,
		"UNIC_PROFILE=",
		"UC_PROFILE=",
	)

	// 1. Bare `unic migrate` in a PTY discovers default v1 source, prints credential failure,
	// retries wrong passphrase -> right passphrase -> prints human summary and completes.
	t.Run("bare migrate with passphrase retry", func(t *testing.T) {
		target := filepath.Join(root, "bare-target")
		master, slave, err := pty.Open()
		if err != nil {
			t.Fatal(err)
		}
		defer func() { _ = master.Close(); _ = slave.Close() }()
		ctx, cancel := context.WithTimeout(context.Background(), 20*time.Second)
		defer cancel()

		cmd := exec.CommandContext(ctx, filepath.Join(binDir, "unic"), "migrate", "--service", "unic-e2e-missing-cred")
		cmd.Env = append(baseEnv, "UNIC_HOME="+target)
		cmd.Stdin = slave
		var out lockedBuffer
		cmd.Stdout = &out
		cmd.Stderr = &out
		if err := cmd.Start(); err != nil {
			t.Fatal(err)
		}
		waitPrompt := func(substr string) {
			t.Helper()
			deadline := time.Now().Add(10 * time.Second)
			for !strings.Contains(out.String(), substr) {
				if time.Now().After(deadline) {
					_ = cmd.Process.Kill()
					t.Fatalf("timed out waiting for %q; output:\n%s", substr, out.String())
				}
				time.Sleep(15 * time.Millisecond)
			}
		}
		waitPrompt("Old UniClipboard passphrase:")
		_, _ = master.Write([]byte("wrong-one\n"))
		waitPrompt("2 attempt(s) remaining")
		_, _ = master.Write([]byte(fixture.Passphrase + "\n"))
		if err := cmd.Wait(); err != nil {
			t.Fatalf("bare migrate retry failed: %v\noutput:\n%s", err, out.String())
		}
		if !strings.Contains(out.String(), "Migration complete") || !strings.Contains(out.String(), "unic daemon start") {
			t.Fatalf("missing human summary:\n%s", out.String())
		}
		if strings.Contains(out.String(), fixture.Passphrase) {
			t.Fatal("passphrase leaked in output")
		}
	})

	// 2. `unic init` in a PTY presents 2-choice menu: [1] imports directly; [2] starts fresh.
	t.Run("init interactive menu choice 1", func(t *testing.T) {
		target := filepath.Join(root, "init-choice-1")
		master, slave, err := pty.Open()
		if err != nil {
			t.Fatal(err)
		}
		defer func() { _ = master.Close(); _ = slave.Close() }()
		ctx, cancel := context.WithTimeout(context.Background(), 20*time.Second)
		defer cancel()

		cmd := exec.CommandContext(ctx, filepath.Join(binDir, "unic"), "init", "--name", "MigratedMac")
		cmd.Env = append(baseEnv, "UNIC_HOME="+target)
		cmd.Stdin = slave
		var out lockedBuffer
		cmd.Stdout = &out
		cmd.Stderr = &out
		if err := cmd.Start(); err != nil {
			t.Fatal(err)
		}
		waitOut := func(substr string) {
			t.Helper()
			deadline := time.Now().Add(10 * time.Second)
			for !strings.Contains(out.String(), substr) {
				if time.Now().After(deadline) {
					_ = cmd.Process.Kill()
					t.Fatalf("timed out waiting for %q; output:\n%s", substr, out.String())
				}
				time.Sleep(15 * time.Millisecond)
			}
		}
		waitOut("[1] Import v1 history (recommended)")
		_, _ = master.Write([]byte("1\n"))
		waitOut("Old UniClipboard passphrase:")
		_, _ = master.Write([]byte(fixture.Passphrase + "\n"))
		if err := cmd.Wait(); err != nil {
			t.Fatalf("init choice 1 failed: %v\n%s", err, out.String())
		}
		if !strings.Contains(out.String(), "Migration complete") {
			t.Fatalf("unexpected init choice 1 output:\n%s", out.String())
		}
		cfgBytes, rerr := os.ReadFile(filepath.Join(target, "config.toml"))
		if rerr != nil || !strings.Contains(string(cfgBytes), "MigratedMac") {
			t.Fatalf("config.toml missing MigratedMac after choice 1: %v (%s)", rerr, cfgBytes)
		}
	})

	t.Run("init interactive menu choice 2", func(t *testing.T) {
		target := filepath.Join(root, "init-choice-2")
		master, slave, err := pty.Open()
		if err != nil {
			t.Fatal(err)
		}
		defer func() { _ = master.Close(); _ = slave.Close() }()
		ctx, cancel := context.WithTimeout(context.Background(), 15*time.Second)
		defer cancel()

		cmd := exec.CommandContext(ctx, filepath.Join(binDir, "unic"), "init")
		cmd.Env = append(baseEnv, "UNIC_HOME="+target)
		cmd.Stdin = slave
		var out lockedBuffer
		cmd.Stdout = &out
		cmd.Stderr = &out
		if err := cmd.Start(); err != nil {
			t.Fatal(err)
		}
		deadline := time.Now().Add(10 * time.Second)
		for !strings.Contains(out.String(), "[2] Start fresh without v1 history") {
			if time.Now().After(deadline) {
				_ = cmd.Process.Kill()
				t.Fatalf("menu did not appear:\n%s", out.String())
			}
			time.Sleep(15 * time.Millisecond)
		}
		_, _ = master.Write([]byte("2\n"))
		if err := cmd.Wait(); err != nil {
			t.Fatalf("init choice 2 failed: %v\n%s", err, out.String())
		}
		if !strings.Contains(out.String(), "Initialized profile") {
			t.Fatalf("unexpected init output:\n%s", out.String())
		}
	})

	// 3. `--passphrase-stdin` on a PTY disables echo (no plaintext leaked) and restores terminal state.
	t.Run("passphrase-stdin on PTY hides echo", func(t *testing.T) {
		target := filepath.Join(root, "pty-stdin-noecho")
		master, slave, err := pty.Open()
		if err != nil {
			t.Fatal(err)
		}
		defer func() { _ = master.Close(); _ = slave.Close() }()
		before, err := term.GetState(int(slave.Fd()))
		if err != nil {
			t.Fatal(err)
		}
		ctx, cancel := context.WithTimeout(context.Background(), 15*time.Second)
		defer cancel()

		cmd := exec.CommandContext(ctx, filepath.Join(binDir, "unic"), "migrate", "--passphrase-stdin", "--json")
		cmd.Env = append(baseEnv, "UNIC_HOME="+target)
		cmd.Stdin = slave
		cmd.Stdout = slave
		cmd.Stderr = slave
		if err := cmd.Start(); err != nil {
			t.Fatal(err)
		}
		var echoed lockedBuffer
		readerDone := make(chan struct{})
		go func() {
			defer close(readerDone)
			buf := make([]byte, 4096)
			for {
				n, rerr := master.Read(buf)
				if n > 0 {
					_, _ = echoed.Write(buf[:n])
				}
				if rerr != nil {
					return
				}
			}
		}()
		deadline := time.Now().Add(8 * time.Second)
		for {
			st, gerr := term.GetState(int(slave.Fd()))
			if gerr != nil {
				t.Fatal(gerr)
			}
			if !reflect.DeepEqual(before, st) {
				break
			}
			if time.Now().After(deadline) {
				_ = cmd.Process.Kill()
				t.Fatal("passphrase-stdin on PTY did not disable echo")
			}
			time.Sleep(10 * time.Millisecond)
		}
		const synthetic = "synthetic-e2e-tty-passphrase-42"
		_, _ = master.Write([]byte(synthetic + "\n"))
		if err := cmd.Wait(); err == nil {
			t.Fatal("wrong passphrase unexpectedly succeeded")
		}
		after, err := term.GetState(int(slave.Fd()))
		if err != nil {
			t.Fatal(err)
		}
		if !reflect.DeepEqual(before, after) {
			t.Fatal("terminal state not restored after --passphrase-stdin on PTY")
		}
		time.Sleep(50 * time.Millisecond)
		_ = master.Close()
		_ = slave.Close()
		<-readerDone
		if strings.Contains(echoed.String(), synthetic) {
			t.Fatal("passphrase-stdin echoed plaintext on PTY")
		}
	})

	// 4. Concurrent pending published during lock wait still requires [y/N] confirmation on TTY.
	t.Run("discard-pending lock race requires confirmation", func(t *testing.T) {
		target := filepath.Join(root, "discard-race")
		if err := os.MkdirAll(target, 0o700); err != nil {
			t.Fatal(err)
		}
		ctx, cancel := context.WithTimeout(context.Background(), 15*time.Second)
		defer cancel()
		unlock, err := servicelaunch.LockContext(ctx, target)
		if err != nil {
			t.Fatal(err)
		}
		master, slave, err := pty.Open()
		if err != nil {
			unlock()
			t.Fatal(err)
		}
		defer func() { _ = master.Close(); _ = slave.Close() }()

		cmd := exec.CommandContext(ctx, filepath.Join(binDir, "unic"), "init", "--fresh", "--discard-pending")
		cmd.Env = append(baseEnv, "UNIC_HOME="+target)
		cmd.Stdin = slave
		var out lockedBuffer
		cmd.Stdout = &out
		cmd.Stderr = &out
		if err := cmd.Start(); err != nil {
			unlock()
			t.Fatal(err)
		}
		time.Sleep(200 * time.Millisecond)
		if err := climigration.Write(target, climigration.State{Status: "pending", SourceID: "race-e2e"}); err != nil {
			unlock()
			_ = cmd.Process.Kill()
			t.Fatal(err)
		}
		unlock()

		deadline := time.Now().Add(8 * time.Second)
		for !strings.Contains(out.String(), "[y/N]") {
			if time.Now().After(deadline) {
				_ = cmd.Process.Kill()
				t.Fatalf("expected [y/N] prompt after lock wait; output:\n%s", out.String())
			}
			time.Sleep(15 * time.Millisecond)
		}
		// Zero-line EOF (Ctrl+D) must reject and keep pending intact.
		_, _ = master.Write([]byte{4})
		if err := cmd.Wait(); err == nil {
			t.Fatal("discard-pending race succeeded without confirmation")
		}
		st, err := climigration.Read(target)
		if err != nil || st.Status != "pending" {
			t.Fatalf("pending state overwritten after EOF: %+v (%v)", st, err)
		}
	})

	// 5. If `unicd` starts while `--discard-pending` awaits [y/N], answering `y` still fails in_use and preserves pending.
	t.Run("discard-pending revalidates owner after confirmation", func(t *testing.T) {
		target := filepath.Join(root, "discard-owner-race")
		if err := os.MkdirAll(target, 0o700); err != nil {
			t.Fatal(err)
		}
		if err := os.WriteFile(filepath.Join(target, "config.toml"), []byte("[telemetry]\nenabled=false\n[sync]\nenabled=false\n"), 0o600); err != nil {
			t.Fatal(err)
		}
		ctx, cancel := context.WithTimeout(context.Background(), 20*time.Second)
		defer cancel()
		// Seed a pending state first via partial migration.
		seed := exec.CommandContext(ctx, filepath.Join(binDir, "unic"), "migrate", "--passphrase-stdin", "--json")
		seed.Env = append(baseEnv, "UNIC_HOME="+target)
		seed.Stdin = strings.NewReader(fixture.Passphrase + "\n")
		if seedOut, serr := seed.CombinedOutput(); serr != nil {
			t.Fatalf("seed migrate failed: %v (%s)", serr, seedOut)
		}
		if err := climigration.Write(target, climigration.State{Status: "pending", SourceID: "owner-race-e2e"}); err != nil {
			t.Fatal(err)
		}

		master, slave, err := pty.Open()
		if err != nil {
			t.Fatal(err)
		}
		defer func() { _ = master.Close(); _ = slave.Close() }()

		cmd := exec.CommandContext(ctx, filepath.Join(binDir, "unic"), "init", "--fresh", "--discard-pending")
		cmd.Env = append(baseEnv, "UNIC_HOME="+target, "UNIC_SOCK="+filepath.Join(target, "unic.sock"))
		cmd.Stdin = slave
		var out lockedBuffer
		cmd.Stdout = &out
		cmd.Stderr = &out
		if err := cmd.Start(); err != nil {
			t.Fatal(err)
		}
		deadline := time.Now().Add(8 * time.Second)
		for !strings.Contains(out.String(), "[y/N]") {
			if time.Now().After(deadline) {
				_ = cmd.Process.Kill()
				t.Fatalf("expected [y/N] prompt; output:\n%s", out.String())
			}
			time.Sleep(15 * time.Millisecond)
		}

		// Start real unicd while init waits for user confirmation.
		daemon := exec.CommandContext(ctx, filepath.Join(binDir, "unicd"))
		daemon.Env = append(baseEnv, "UNIC_HOME="+target, "UNIC_SOCK="+filepath.Join(target, "unic.sock"))
		if err := daemon.Start(); err != nil {
			_ = cmd.Process.Kill()
			t.Fatal(err)
		}
		defer func() {
			if daemon.Process != nil {
				_ = daemon.Process.Signal(os.Interrupt)
				_ = daemon.Wait()
			}
		}()
		sockPath := filepath.Join(target, "unic.sock")
		sockDeadline := time.Now().Add(8 * time.Second)
		for {
			if _, serr := os.Stat(sockPath); serr == nil {
				break
			}
			if time.Now().After(sockDeadline) {
				_ = cmd.Process.Kill()
				t.Fatal("unicd did not become ready during [y/N] prompt")
			}
			time.Sleep(25 * time.Millisecond)
		}

		_, _ = master.Write([]byte("y\n"))
		if err := cmd.Wait(); err == nil {
			t.Fatalf("discard-pending succeeded despite live unicd:\n%s", out.String())
		}
		st, err := climigration.Read(target)
		if err != nil || st.Status != "pending" || st.SourceID != "owner-race-e2e" {
			t.Fatalf("pending state mutated despite live unicd: %+v (%v)", st, err)
		}
	})
}
