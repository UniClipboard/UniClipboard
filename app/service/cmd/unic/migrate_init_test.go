//go:build !windows

package main

import (
	"bytes"
	"context"
	"database/sql"
	"encoding/json"
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"reflect"
	"runtime"
	"strings"
	"sync"
	"testing"
	"time"

	"github.com/creack/pty"
	"golang.org/x/term"

	"github.com/uniclipboard/unic/engine"
	"github.com/uniclipboard/unic/internal/climigration"
	"github.com/uniclipboard/unic/internal/servicelaunch"
	"github.com/uniclipboard/unic/internal/v1migration"
	"github.com/uniclipboard/unic/internal/v1migration/v1fixture"
)

func setIsolatedCLIEnv(t *testing.T, root string) (userHome, dataHome, target string) {
	t.Helper()
	userHome = filepath.Join(root, "home")
	cfgHome := filepath.Join(root, "cfg")
	dataHome = filepath.Join(root, "data")
	target = filepath.Join(root, "target")
	for _, d := range []string{userHome, cfgHome, dataHome} {
		if err := os.MkdirAll(d, 0o700); err != nil {
			t.Fatal(err)
		}
	}
	t.Setenv("HOME", userHome)
	t.Setenv("XDG_CONFIG_HOME", cfgHome)
	t.Setenv("XDG_DATA_HOME", dataHome)
	t.Setenv("LOCALAPPDATA", dataHome)
	t.Setenv("UNIC_HOME", target)
	t.Setenv("UNIC_PROFILE", "")
	t.Setenv("UC_PROFILE", "")
	return userHome, dataHome, target
}

func defaultV1DirForTest(userHome, dataHome string) string {
	if runtime.GOOS == "darwin" {
		return filepath.Join(userHome, "Library", "Application Support", "app.uniclipboard.desktop")
	}
	return filepath.Join(dataHome, "app.uniclipboard.desktop")
}

func TestMigrateHelpAndUnknownFreshFlag(t *testing.T) {
	root := t.TempDir()
	_, _, target := setIsolatedCLIEnv(t, root)

	for _, args := range [][]string{{"--help"}, {"-h"}} {
		var out, errOut bytes.Buffer
		code := runMigration(args, nil, &out, &errOut)
		if code != 0 {
			t.Fatalf("migrate %v exit=%d stderr=%s", args, code, errOut.String())
		}
		got := out.String()
		if lines := strings.Count(got, "\n"); lines > 25 {
			t.Fatalf("migrate help too long: %d lines\n%s", lines, got)
		}
		for _, want := range []string{"--dry-run", "--passphrase-stdin", "--initialize-empty", "unic init --fresh", "pair devices again"} {
			if !strings.Contains(got, want) {
				t.Fatalf("migrate help missing %q:\n%s", want, got)
			}
		}
		if strings.Contains(got, "and favorites do not") || !strings.Contains(got, "Persisted favorites") {
			t.Fatalf("migrate help favorites wording inaccurate:\n%s", got)
		}
		if _, err := os.Stat(target); !os.IsNotExist(err) {
			t.Fatalf("migrate %v created target dir", args)
		}
	}
	if text, ok := helpForCommand("migrate"); !ok || text != migrateUsage {
		t.Fatalf("helpForCommand(migrate) mismatch")
	}
	// Positional "help" is rejected as usage error (exit 2), not treated as --help.
	{
		var out, errOut bytes.Buffer
		if code := runMigration([]string{"help"}, nil, &out, &errOut); code != 2 || out.Len() != 0 || !strings.Contains(errOut.String(), "unic migrate --help") {
			t.Fatalf("migrate help positional exit=%d stdout=%q stderr=%q", code, out.String(), errOut.String())
		}
		if _, err := os.Stat(target); !os.IsNotExist(err) {
			t.Fatal("migrate help created target dir")
		}
	}

	var out, errOut bytes.Buffer
	code := runMigration([]string{"--fresh"}, nil, &out, &errOut)
	if code != 2 {
		t.Fatalf("migrate --fresh exit=%d want 2", code)
	}
	if !strings.Contains(errOut.String(), "unic init --fresh") {
		t.Fatalf("migrate --fresh should guide to unic init --fresh, got %q", errOut.String())
	}

	out.Reset()
	errOut.Reset()
	code = runMigration([]string{"--fresh", "--json"}, nil, &out, &errOut)
	if code != 2 || out.Len() != 0 {
		t.Fatalf("migrate --fresh --json exit=%d stdout=%q", code, out.String())
	}
	var payload map[string]string
	if err := json.Unmarshal(errOut.Bytes(), &payload); err != nil || payload["next_command"] != "unic init --fresh" {
		t.Fatalf("unexpected --fresh --json stderr: %q (%v)", errOut.String(), err)
	}
}

func TestMigrateBareWithoutSourceFailsCleanly(t *testing.T) {
	root := t.TempDir()
	_, _, target := setIsolatedCLIEnv(t, root)

	var out, errOut bytes.Buffer
	code := runMigration(nil, nil, &out, &errOut)
	if code != 1 {
		t.Fatalf("bare migrate without source exit=%d want 1 (stderr=%s)", code, errOut.String())
	}
	if !strings.Contains(errOut.String(), "unic init") || !strings.Contains(errOut.String(), "--source DIR") {
		t.Fatalf("missing guidance in stderr: %q", errOut.String())
	}
	if _, err := os.Stat(target); !os.IsNotExist(err) {
		t.Fatal("bare migrate without source created target directory")
	}

	out.Reset()
	errOut.Reset()
	code = runMigration([]string{"--json"}, nil, &out, &errOut)
	if code != 1 || out.Len() != 0 {
		t.Fatalf("bare migrate --json without source exit=%d stdout=%q", code, out.String())
	}
	var payload map[string]string
	if err := json.Unmarshal(errOut.Bytes(), &payload); err != nil || payload["reason"] != "source_invalid" || payload["next_command"] != "unic init" {
		t.Fatalf("unexpected stderr JSON: %q (%v)", errOut.String(), err)
	}
}

func TestMigrateUnlockFailureDoesNotLockProfileAndRetriesOnPTY(t *testing.T) {
	root, err := filepath.EvalSymlinks(t.TempDir())
	if err != nil {
		t.Fatal(err)
	}
	userHome, dataHome, target := setIsolatedCLIEnv(t, root)
	v1Dir := defaultV1DirForTest(userHome, dataHome)
	source, dbpath, fixture, err := v1fixture.Build(v1Dir)
	if err != nil {
		t.Fatal(err)
	}
	_ = dbpath

	failingReader := func(context.Context, string, string) ([]byte, error) {
		return nil, v1migration.ErrUnlock
	}

	// 1. Non-interactive failure leaves no pending state and no unic.db; unic init --fresh succeeds.
	var out, errOut bytes.Buffer
	code := executeMigration(context.Background(), migrateOptions{
		target:       target,
		service:      "UniClipboard",
		in:           nil,
		out:          &out,
		errOut:       &errOut,
		secretReader: failingReader,
	})
	if code != 1 {
		t.Fatalf("non-TTY unlock failure exit=%d want 1", code)
	}
	if !strings.Contains(errOut.String(), "--passphrase-stdin") {
		t.Fatalf("missing --passphrase-stdin hint: %q", errOut.String())
	}
	for _, f := range []string{"cli-migration.json", "unic.db"} {
		if _, statErr := os.Stat(filepath.Join(target, f)); !os.IsNotExist(statErr) {
			t.Fatalf("unlock failure created %s", f)
		}
	}

	// 2. PTY: 3 failed attempts (empty + 2 wrong) leave no pending state/unic.db, and unic init --fresh works.
	master, slave, err := pty.Open()
	if err != nil {
		t.Fatal(err)
	}
	defer func() { _ = master.Close(); _ = slave.Close() }()

	done := make(chan int, 1)
	out.Reset()
	errOut.Reset()
	go func() {
		done <- executeMigration(context.Background(), migrateOptions{
			source:       source,
			target:       target,
			service:      "unic-test-nonexistent-service",
			in:           slave,
			out:          &out,
			errOut:       &errOut,
			secretReader: nil,
		})
	}()
	// Send empty, wrong1, wrong2.
	for _, line := range []string{"\n", "wrong1\n", "wrong2\n"} {
		time.Sleep(80 * time.Millisecond)
		if _, werr := master.Write([]byte(line)); werr != nil {
			t.Fatal(werr)
		}
	}
	select {
	case code = <-done:
	case <-time.After(10 * time.Second):
		t.Fatal("timed out waiting for 3-attempt unlock failure")
	}
	if code != 1 {
		t.Fatalf("3 failed attempts exit=%d want 1; stderr=%s", code, errOut.String())
	}
	if !strings.Contains(errOut.String(), "supply the old passphrase") || !strings.Contains(errOut.String(), "Passphrase cannot be empty.") || !strings.Contains(errOut.String(), "Incorrect passphrase.") {
		t.Fatalf("unexpected retry stderr:\n%s", errOut.String())
	}
	for _, f := range []string{"cli-migration.json", "unic.db"} {
		if _, statErr := os.Stat(filepath.Join(target, f)); !os.IsNotExist(statErr) {
			t.Fatalf("3 failed attempts created %s", f)
		}
	}

	// Verify unic init --fresh --json is NOT soft-locked after unlock failure.
	freshTarget := filepath.Join(root, "fresh-after-fail")
	t.Setenv("UNIC_HOME", freshTarget)
	out.Reset()
	errOut.Reset()
	if initCode := runInit([]string{"--fresh", "--json"}, nil, &out, &errOut, false); initCode != 0 {
		t.Fatalf("init --fresh after unlock fail exit=%d stderr=%s", initCode, errOut.String())
	}

	// 3. PTY: empty + wrong + right passphrase succeeds with grouped human summary (partial fixture -> exit 3).
	t.Setenv("UNIC_HOME", target)
	out.Reset()
	errOut.Reset()
	go func() {
		done <- executeMigration(context.Background(), migrateOptions{
			target:       target,
			service:      "UniClipboard",
			in:           slave,
			out:          &out,
			errOut:       &errOut,
			secretReader: failingReader,
		})
	}()
	for _, line := range []string{"\n", "wrong\n", fixture.Passphrase + "\n"} {
		time.Sleep(80 * time.Millisecond)
		if _, werr := master.Write([]byte(line)); werr != nil {
			t.Fatal(werr)
		}
	}
	select {
	case code = <-done:
	case <-time.After(10 * time.Second):
		t.Fatal("timed out waiting for retry success")
	}
	if code != 3 {
		t.Fatalf("partial fixture retry exit=%d want 3; out=%s stderr=%s", code, out.String(), errOut.String())
	}
	if !strings.Contains(out.String(), "Migration partially completed") || !strings.Contains(out.String(), "History") {
		t.Fatalf("missing grouped summary:\n%s", out.String())
	}
}

func TestMigratePreUnlockSourceMismatchDoesNotTouchPending(t *testing.T) {
	root := t.TempDir()
	_, _, target := setIsolatedCLIEnv(t, root)
	srcA, _, _, err := v1fixture.Build(filepath.Join(root, "srcA"))
	if err != nil {
		t.Fatal(err)
	}
	srcB, _, _, err := v1fixture.Build(filepath.Join(root, "srcB"))
	if err != nil {
		t.Fatal(err)
	}
	// Make srcB's keyslot bytes differ from srcA's.
	slotPath := filepath.Join(srcB, "vault", "keyslot.json")
	raw, err := os.ReadFile(slotPath)
	if err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(slotPath, append(raw, ' '), 0o600); err != nil {
		t.Fatal(err)
	}

	sumA, err := climigration.KeyslotSHA256(srcA)
	if err != nil {
		t.Fatal(err)
	}
	if err := climigration.Write(target, climigration.State{Status: "pending", KeyslotSHA256: sumA}); err != nil {
		t.Fatal(err)
	}

	calledUnlock := false
	var out, errOut bytes.Buffer
	code := executeMigration(context.Background(), migrateOptions{
		source:   srcB,
		target:   target,
		service:  "UniClipboard",
		jsonMode: true,
		out:      &out,
		errOut:   &errOut,
		secretReader: func(context.Context, string, string) ([]byte, error) {
			calledUnlock = true
			return nil, v1migration.ErrUnlock
		},
	})
	if code != 1 || calledUnlock || out.Len() != 0 {
		t.Fatalf("expected pre-unlock source_mismatch exit=1 calledUnlock=%v stdout=%q", calledUnlock, out.String())
	}
	if !strings.Contains(errOut.String(), `"reason":"source_mismatch"`) {
		t.Fatalf("expected source_mismatch in stderr: %q", errOut.String())
	}
	st, err := climigration.Read(target)
	if err != nil || st.Status != "pending" || st.KeyslotSHA256 != sumA {
		t.Fatalf("pending state mutated: %+v (%v)", st, err)
	}
}

func TestInitInteractiveMenuAndDiscardPending(t *testing.T) {
	root, err := filepath.EvalSymlinks(t.TempDir())
	if err != nil {
		t.Fatal(err)
	}
	userHome, dataHome, target := setIsolatedCLIEnv(t, root)
	v1Dir := defaultV1DirForTest(userHome, dataHome)
	_, _, _, err = v1fixture.Build(v1Dir)
	if err != nil {
		t.Fatal(err)
	}

	// 1. --discard-pending alone fails with exit 2.
	var out, errOut bytes.Buffer
	if code := runInit([]string{"--discard-pending"}, nil, &out, &errOut, false); code != 2 {
		t.Fatalf("--discard-pending without --fresh exit=%d want 2", code)
	}

	// 2. Non-interactive init refuses source_present without leaking v1Dir path in JSON.
	out.Reset()
	errOut.Reset()
	if code := runInit([]string{"--json"}, nil, &out, &errOut, false); code != 1 || !strings.Contains(errOut.String(), `"source_present"`) {
		t.Fatalf("non-TTY init should refuse source_present: code=%d stderr=%s", code, errOut.String())
	}
	if strings.Contains(errOut.String(), v1Dir) {
		t.Fatalf("init --json leaked absolute v1 source path %q in %q", v1Dir, errOut.String())
	}

	// 3. Interactive PTY init menu: choosing [2] starts fresh.
	master, slave, err := pty.Open()
	if err != nil {
		t.Fatal(err)
	}
	defer func() { _ = master.Close(); _ = slave.Close() }()

	done := make(chan int, 1)
	out.Reset()
	errOut.Reset()
	go func() {
		done <- runInit([]string{"--name", "FreshMac"}, slave, &out, &errOut, false)
	}()
	time.Sleep(80 * time.Millisecond)
	if _, err := master.Write([]byte("2\n")); err != nil {
		t.Fatal(err)
	}
	select {
	case code := <-done:
		if code != 0 {
			t.Fatalf("interactive init choice 2 exit=%d stderr=%s", code, errOut.String())
		}
	case <-time.After(10 * time.Second):
		t.Fatal("timed out on interactive init choice 2")
	}
	if !strings.Contains(errOut.String(), "[1] Import v1 history (recommended)") || !strings.Contains(errOut.String(), "[2] Start fresh without v1 history") {
		t.Fatalf("missing interactive menu options:\n%s", errOut.String())
	}
	st, err := climigration.Read(target)
	if err != nil || st.Status != "complete" || st.Outcome != "fresh" {
		t.Fatalf("unexpected state after choice 2: %+v (%v)", st, err)
	}

	// 4. Real pending state: ordinary --fresh refuses, --fresh --discard-pending on TTY asks y/N, JSON unlocks directly and keeps unic.db.
	pendingTarget := filepath.Join(root, "pending-target")
	t.Setenv("UNIC_HOME", pendingTarget)
	e, err := engine.New(context.Background(), engine.Options{DataDir: pendingTarget, MigrationMode: true})
	if err != nil {
		t.Fatal(err)
	}
	if err := e.Close(context.Background()); err != nil {
		t.Fatal(err)
	}
	if err := climigration.Write(pendingTarget, climigration.State{Status: "pending", SourceID: "src-123"}); err != nil {
		t.Fatal(err)
	}
	out.Reset()
	errOut.Reset()
	if code := runInit([]string{"--fresh", "--json"}, nil, &out, &errOut, false); code != 1 || !strings.Contains(errOut.String(), `"migration_pending"`) {
		t.Fatalf("ordinary --fresh must refuse pending: code=%d stderr=%s", code, errOut.String())
	}

	// Interactive y/N -> answer "n" preserves pending.
	out.Reset()
	errOut.Reset()
	go func() {
		done <- runInit([]string{"--fresh", "--discard-pending"}, slave, &out, &errOut, false)
	}()
	time.Sleep(80 * time.Millisecond)
	if _, err := master.Write([]byte("n\n")); err != nil {
		t.Fatal(err)
	}
	if code := <-done; code != 1 {
		t.Fatalf("discard-pending answered n should exit 1, got %d", code)
	}
	if st, rerr := climigration.Read(pendingTarget); rerr != nil || st.Status != "pending" {
		t.Fatalf("answering n mutated pending state: %+v (%v)", st, rerr)
	}

	// When engine is actively open (in_use), --discard-pending --json must refuse and preserve pending.
	live, err := engine.New(context.Background(), engine.Options{DataDir: pendingTarget, MigrationMode: true})
	if err != nil {
		t.Fatal(err)
	}
	out.Reset()
	errOut.Reset()
	if code := runInit([]string{"--fresh", "--discard-pending", "--json"}, nil, &out, &errOut, false); code != 1 || !strings.Contains(errOut.String(), `"in_use"`) {
		_ = live.Close(context.Background())
		t.Fatalf("discard-pending while in_use should fail with in_use: code=%d stderr=%s", code, errOut.String())
	}
	_ = live.Close(context.Background())

	// Now --fresh --discard-pending --json succeeds and keeps unic.db intact.
	out.Reset()
	errOut.Reset()
	if code := runInit([]string{"--fresh", "--discard-pending", "--json"}, nil, &out, &errOut, false); code != 0 {
		t.Fatalf("discard-pending --json exit=%d stderr=%s", code, errOut.String())
	}
	if st, rerr := climigration.Read(pendingTarget); rerr != nil || st.Status != "complete" || st.Outcome != "fresh" {
		t.Fatalf("discard-pending did not complete fresh: %+v (%v)", st, rerr)
	}
	if _, statErr := os.Stat(filepath.Join(pendingTarget, "unic.db")); statErr != nil {
		t.Fatalf("discard-pending deleted unic.db: %v", statErr)
	}
}

func TestReadMigrationPassphraseRestoresPTYOnCancel(t *testing.T) {
	master, slave, err := pty.Open()
	if err != nil {
		t.Fatal(err)
	}
	before, err := term.GetState(int(slave.Fd()))
	if err != nil {
		_ = master.Close()
		_ = slave.Close()
		t.Fatal(err)
	}
	ctx, cancel := context.WithCancel(context.Background())
	done := make(chan error, 1)
	go func() {
		_, rerr := readMigrationPassphrase(ctx, slave, true)
		done <- rerr
	}()
	time.Sleep(60 * time.Millisecond)
	cancel()
	if rerr := <-done; !errors.Is(rerr, context.Canceled) {
		_ = master.Close()
		_ = slave.Close()
		t.Fatalf("expected context.Canceled, got %v", rerr)
	}
	after, err := term.GetState(int(slave.Fd()))
	if err != nil {
		_ = master.Close()
		_ = slave.Close()
		t.Fatal(err)
	}
	if !reflect.DeepEqual(before, after) {
		_ = master.Close()
		_ = slave.Close()
		t.Fatal("PTY state not restored after cancellation")
	}
	// Unblock the inner ReadPassword goroutine before closing slave.
	_, _ = master.Write([]byte("\n"))
	time.Sleep(30 * time.Millisecond)
	_ = master.Close()
	_ = slave.Close()
}

func TestPassphraseStdinOnPTYHidesEchoAndPipeClearsBytes(t *testing.T) {
	root, err := filepath.EvalSymlinks(t.TempDir())
	if err != nil {
		t.Fatal(err)
	}
	userHome, dataHome, target := setIsolatedCLIEnv(t, root)
	v1Dir := defaultV1DirForTest(userHome, dataHome)
	source, _, _, err := v1fixture.Build(v1Dir)
	if err != nil {
		t.Fatal(err)
	}

	master, slave, err := pty.Open()
	if err != nil {
		t.Fatal(err)
	}
	before, err := term.GetState(int(slave.Fd()))
	if err != nil {
		_ = master.Close()
		_ = slave.Close()
		t.Fatal(err)
	}
	defer func() { _ = master.Close(); _ = slave.Close() }()

	const syntheticSecret = "synthetic-tty-stdin-secret-9981"
	var ptyMu sync.Mutex
	var ptyEcho bytes.Buffer
	readerDone := make(chan struct{})
	go func() {
		defer close(readerDone)
		buf := make([]byte, 4096)
		for {
			n, rerr := master.Read(buf)
			if n > 0 {
				ptyMu.Lock()
				ptyEcho.Write(buf[:n])
				ptyMu.Unlock()
			}
			if rerr != nil {
				return
			}
		}
	}()

	var out, errOut bytes.Buffer
	done := make(chan int, 1)
	go func() {
		done <- executeMigration(context.Background(), migrateOptions{
			source:        source,
			target:        target,
			service:       "UniClipboard",
			jsonMode:      true,
			stdinPassword: true,
			in:            slave,
			out:           &out,
			errOut:        &errOut,
		})
	}()
	// Wait for ReadPassword to disable echo on the PTY before writing.
	deadline := time.Now().Add(5 * time.Second)
	for {
		st, gerr := term.GetState(int(slave.Fd()))
		if gerr != nil {
			t.Fatal(gerr)
		}
		if !reflect.DeepEqual(before, st) {
			break
		}
		if time.Now().After(deadline) {
			t.Fatal("passphrase-stdin on PTY did not disable echo")
		}
		time.Sleep(10 * time.Millisecond)
	}
	if _, err := master.Write([]byte(syntheticSecret + "\n")); err != nil {
		t.Fatal(err)
	}
	select {
	case code := <-done:
		if code != 1 {
			t.Fatalf("expected unlock failure exit 1, got %d", code)
		}
	case <-time.After(10 * time.Second):
		t.Fatal("timed out waiting for --passphrase-stdin on PTY")
	}
	after, err := term.GetState(int(slave.Fd()))
	if err != nil {
		t.Fatal(err)
	}
	if !reflect.DeepEqual(before, after) {
		t.Fatal("PTY state not restored after --passphrase-stdin")
	}
	time.Sleep(50 * time.Millisecond)
	_ = master.Close()
	_ = slave.Close()
	<-readerDone
	ptyMu.Lock()
	echoed := ptyEcho.String()
	ptyMu.Unlock()
	if strings.Contains(echoed, syntheticSecret) || strings.Contains(out.String(), syntheticSecret) || strings.Contains(errOut.String(), syntheticSecret) {
		t.Fatal("passphrase-stdin echoed or leaked secret on PTY")
	}
	if _, err := os.Stat(target); !os.IsNotExist(err) {
		t.Fatal("failed --passphrase-stdin on PTY created target")
	}

	// Non-TTY pipe boundary tests: 4096B + LF/CRLF/EOF pass with identical payload;
	// 4097B + LF/CRLF/EOF fail; cancellation wakes deadline-capable pipe and zeroes buffers.
	maxPayload := bytes.Repeat([]byte("a"), maxPassphraseBytes)
	for _, tc := range []struct {
		name   string
		ending []byte
	}{
		{"4096-lf", []byte("\n")},
		{"4096-crlf", []byte("\r\n")},
		{"4096-eof", nil},
	} {
		pr, pw, perr := os.Pipe()
		if perr != nil {
			t.Fatal(perr)
		}
		go func(end []byte) {
			_, _ = pw.Write(append(bytes.Clone(maxPayload), end...))
			_ = pw.Close()
		}(tc.ending)
		got, gerr := readMigrationPassphrase(context.Background(), pr, false)
		_ = pr.Close()
		if gerr != nil || !bytes.Equal(got, maxPayload) {
			t.Fatalf("%s failed: len=%d err=%v", tc.name, len(got), gerr)
		}
		clear(got)
	}
	overPayload := bytes.Repeat([]byte("b"), maxPassphraseBytes+1)
	for _, tc := range []struct {
		name   string
		ending []byte
	}{
		{"4097-lf", []byte("\n")},
		{"4097-crlf", []byte("\r\n")},
		{"4097-eof", nil},
	} {
		pr, pw, perr := os.Pipe()
		if perr != nil {
			t.Fatal(perr)
		}
		go func(end []byte) {
			_, _ = pw.Write(append(bytes.Clone(overPayload), end...))
			_ = pw.Close()
		}(tc.ending)
		got, gerr := readMigrationPassphrase(context.Background(), pr, false)
		_ = pr.Close()
		if gerr == nil || len(got) != 0 {
			t.Fatalf("%s should be rejected, got len=%d err=%v", tc.name, len(got), gerr)
		}
	}
	// Cancellation immediately wakes a blocked pipe reader via SetReadDeadline.
	pr, pw, err := os.Pipe()
	if err != nil {
		t.Fatal(err)
	}
	defer func() { _ = pw.Close(); _ = pr.Close() }()
	cancelCtx, cancelPipe := context.WithCancel(context.Background())
	pipeDone := make(chan error, 1)
	go func() {
		_, rerr := readMigrationPassphrase(cancelCtx, pr, false)
		pipeDone <- rerr
	}()
	if _, err := pw.Write([]byte("partial-secret")); err != nil {
		t.Fatal(err)
	}
	time.Sleep(50 * time.Millisecond)
	cancelPipe()
	select {
	case rerr := <-pipeDone:
		if !errors.Is(rerr, context.Canceled) {
			t.Fatalf("expected context.Canceled, got %v", rerr)
		}
	case <-time.After(2 * time.Second):
		t.Fatal("blocked pipe read did not wake on cancellation")
	}
}

type syncBuffer struct {
	mu  sync.Mutex
	buf bytes.Buffer
}

func (b *syncBuffer) Write(p []byte) (int, error) {
	b.mu.Lock()
	defer b.mu.Unlock()
	return b.buf.Write(p)
}

func (b *syncBuffer) String() string {
	b.mu.Lock()
	defer b.mu.Unlock()
	return b.buf.String()
}

func TestInitDiscardPendingRaceConfirmationUnderLock(t *testing.T) {
	root, err := filepath.EvalSymlinks(t.TempDir())
	if err != nil {
		t.Fatal(err)
	}
	_, _, target := setIsolatedCLIEnv(t, root)
	if err := os.MkdirAll(target, 0o700); err != nil {
		t.Fatal(err)
	}
	// Hold profile lock before any cli-migration.json exists.
	unlock, err := servicelaunch.LockContext(context.Background(), target)
	if err != nil {
		t.Fatal(err)
	}

	master, slave, err := pty.Open()
	if err != nil {
		unlock()
		t.Fatal(err)
	}
	defer func() { _ = master.Close(); _ = slave.Close() }()

	var out, errOut syncBuffer
	done := make(chan int, 1)
	go func() {
		done <- runInit([]string{"--fresh", "--discard-pending"}, slave, &out, &errOut, false)
	}()

	// Let runInit reach and block on LockContext, then publish pending and release lock.
	time.Sleep(150 * time.Millisecond)
	if err := climigration.Write(target, climigration.State{Status: "pending", SourceID: "race-src"}); err != nil {
		unlock()
		t.Fatal(err)
	}
	unlock()

	// Wait for the [y/N] confirmation prompt bound to the locked pending state.
	deadline := time.Now().Add(5 * time.Second)
	for !strings.Contains(errOut.String(), "[y/N]") {
		if time.Now().After(deadline) {
			t.Fatalf("expected [y/N] confirmation prompt after lock acquired; stderr=%q", errOut.String())
		}
		time.Sleep(15 * time.Millisecond)
	}
	// Send Ctrl+D (EOF / zero input lines): must default to deny and preserve pending.
	_, _ = master.Write([]byte{4})
	select {
	case code := <-done:
		if code != 1 {
			t.Fatalf("expected exit 1 on zero-line EOF under lock race, got %d", code)
		}
	case <-time.After(5 * time.Second):
		t.Fatal("timed out waiting for EOF rejection on discard-pending race")
	}
	st, rerr := climigration.Read(target)
	if rerr != nil || st.Status != "pending" {
		t.Fatalf("race with zero-line input overwrote pending state: %+v (%v)", st, rerr)
	}
}

func TestInitConfigUnavailableJSONOmitsAbsolutePath(t *testing.T) {
	root, err := filepath.EvalSymlinks(t.TempDir())
	if err != nil {
		t.Fatal(err)
	}
	_, _, target := setIsolatedCLIEnv(t, root)

	var out, errOut bytes.Buffer
	if code := runInit([]string{"--fresh", "--json"}, nil, &out, &errOut, false); code != 0 {
		t.Fatalf("initial init failed: code=%d stderr=%s", code, errOut.String())
	}
	cfgPath := filepath.Join(target, "config.toml")
	if err := os.Remove(cfgPath); err != nil {
		t.Fatal(err)
	}
	if err := os.Mkdir(cfgPath, 0o700); err != nil {
		t.Fatal(err)
	}

	out.Reset()
	errOut.Reset()
	code := runInit([]string{"--name", "demo", "--json"}, nil, &out, &errOut, false)
	if code != 1 || out.Len() != 0 {
		t.Fatalf("expected exit 1 with empty stdout, got code=%d out=%q", code, out.String())
	}
	var payload map[string]string
	if err := json.Unmarshal(errOut.Bytes(), &payload); err != nil {
		t.Fatalf("invalid JSON stderr %q: %v", errOut.String(), err)
	}
	if payload["reason"] != "config_unavailable" {
		t.Fatalf("unexpected reason: %+v", payload)
	}
	if strings.Contains(errOut.String(), target) || strings.Contains(errOut.String(), cfgPath) {
		t.Fatalf("config_unavailable JSON leaked absolute path: %q", errOut.String())
	}
}

func TestInitDiscardPendingRevalidatesExclusiveOwnerAfterConfirm(t *testing.T) {
	root, err := filepath.EvalSymlinks(t.TempDir())
	if err != nil {
		t.Fatal(err)
	}
	_, _, target := setIsolatedCLIEnv(t, root)
	e, err := engine.New(context.Background(), engine.Options{DataDir: target, MigrationMode: true})
	if err != nil {
		t.Fatal(err)
	}
	if err := e.Close(context.Background()); err != nil {
		t.Fatal(err)
	}
	if err := climigration.Write(target, climigration.State{Status: "pending", SourceID: "owner-race"}); err != nil {
		t.Fatal(err)
	}

	master, slave, err := pty.Open()
	if err != nil {
		t.Fatal(err)
	}
	defer func() { _ = master.Close(); _ = slave.Close() }()

	var out, errOut syncBuffer
	done := make(chan int, 1)
	go func() {
		done <- runInit([]string{"--fresh", "--discard-pending"}, slave, &out, &errOut, false)
	}()

	deadline := time.Now().Add(5 * time.Second)
	for !strings.Contains(errOut.String(), "[y/N]") {
		if time.Now().After(deadline) {
			t.Fatalf("expected [y/N] prompt; stderr=%q", errOut.String())
		}
		time.Sleep(15 * time.Millisecond)
	}

	// Simulate daemon acquiring exclusive Engine ownership while prompt waits for y/N.
	live, err := engine.New(context.Background(), engine.Options{DataDir: target, MigrationMode: true})
	if err != nil {
		t.Fatalf("failed to acquire live engine during prompt: %v", err)
	}
	defer func() { _ = live.Close(context.Background()) }()

	if _, err := master.Write([]byte("y\n")); err != nil {
		t.Fatal(err)
	}
	select {
	case code := <-done:
		if code != 1 {
			t.Fatalf("expected exit 1 when owner started during [y/N], got %d (out=%q err=%q)", code, out.String(), errOut.String())
		}
	case <-time.After(5 * time.Second):
		t.Fatal("timed out waiting for discard-pending revalidation")
	}
	st, rerr := climigration.Read(target)
	if rerr != nil || st.Status != "pending" || st.SourceID != "owner-race" {
		t.Fatalf("pending overwritten despite live owner: %+v (%v)", st, rerr)
	}
}

func TestFlagParseErrorSanitizationInitAndMigrate(t *testing.T) {
	root, err := filepath.EvalSymlinks(t.TempDir())
	if err != nil {
		t.Fatal(err)
	}
	const secretVal = "/secret/abs/path"

	cases := []struct {
		name string
		run  func(out, errOut *bytes.Buffer) int
		json bool
	}{
		{
			name: "init --fresh=secret text",
			run:  func(out, errOut *bytes.Buffer) int { return runInit([]string{"--fresh=" + secretVal}, nil, out, errOut, false) },
		},
		{
			name: "init --fresh=secret json",
			run:  func(out, errOut *bytes.Buffer) int { return runInit([]string{"--fresh=" + secretVal, "--json"}, nil, out, errOut, false) },
			json: true,
		},
		{
			name: "migrate --fresh=secret text",
			run:  func(out, errOut *bytes.Buffer) int { return runMigration([]string{"--fresh=" + secretVal}, nil, out, errOut) },
		},
		{
			name: "migrate --fresh=secret json",
			run:  func(out, errOut *bytes.Buffer) int { return runMigration([]string{"--fresh=" + secretVal, "--json"}, nil, out, errOut) },
			json: true,
		},
		{
			name: "migrate --dry-run=secret text",
			run:  func(out, errOut *bytes.Buffer) int { return runMigration([]string{"--dry-run=" + secretVal}, nil, out, errOut) },
		},
		{
			name: "migrate --dry-run=secret json",
			run:  func(out, errOut *bytes.Buffer) int { return runMigration([]string{"--dry-run=" + secretVal, "--json"}, nil, out, errOut) },
			json: true,
		},
		{
			name: "init --name missing arg json",
			run:  func(out, errOut *bytes.Buffer) int { return runInit([]string{"--json", "--name"}, nil, out, errOut, false) },
			json: true,
		},
		{
			name: "migrate --bogus=secret json",
			run:  func(out, errOut *bytes.Buffer) int { return runMigration([]string{"--bogus=" + secretVal, "--json"}, nil, out, errOut) },
			json: true,
		},
	}

	for i, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			subRoot := filepath.Join(root, fmt.Sprintf("sub-%d", i))
			_, _, target := setIsolatedCLIEnv(t, subRoot)
			var out, errOut bytes.Buffer
			code := tc.run(&out, &errOut)
			if code != 2 {
				t.Fatalf("exit=%d want 2 (out=%q err=%q)", code, out.String(), errOut.String())
			}
			if strings.Contains(out.String(), secretVal) || strings.Contains(errOut.String(), secretVal) {
				t.Fatalf("leaked raw flag value %q in out=%q err=%q", secretVal, out.String(), errOut.String())
			}
			if tc.json {
				if out.Len() != 0 {
					t.Fatalf("expected empty stdout in JSON error, got %q", out.String())
				}
				var payload map[string]string
				if err := json.Unmarshal(errOut.Bytes(), &payload); err != nil || payload["reason"] != "invalid_argument" {
					t.Fatalf("expected invalid_argument JSON, got %q (%v)", errOut.String(), err)
				}
			}
			if _, statErr := os.Stat(target); !os.IsNotExist(statErr) {
				t.Fatal("invalid flag created target directory")
			}
		})
	}
}

func TestReadMigrationPassphraseLineEndingsAndLimits(t *testing.T) {
	readPipe := func(data []byte) ([]byte, error) {
		pr, pw, err := os.Pipe()
		if err != nil {
			t.Fatal(err)
		}
		go func() {
			_, _ = pw.Write(data)
			_ = pw.Close()
		}()
		got, rerr := readMigrationPassphrase(context.Background(), pr, false)
		_ = pr.Close()
		return got, rerr
	}

	for _, tc := range []struct {
		name  string
		input []byte
		want  []byte
	}{
		{"lf", []byte("pass\n"), []byte("pass")},
		{"crlf", []byte("pass\r\n"), []byte("pass")},
		{"cr-eof", []byte("pass\r"), []byte("pass")},
		{"pure-eof", []byte("pass"), []byte("pass")},
		{"inner-cr-crlf", []byte("pass\r\r\n"), []byte("pass\r")},
		{"4096-cr-eof", append(bytes.Repeat([]byte("k"), maxPassphraseBytes), '\r'), bytes.Repeat([]byte("k"), maxPassphraseBytes)},
		{"4096-crlf", append(bytes.Repeat([]byte("k"), maxPassphraseBytes), '\r', '\n'), bytes.Repeat([]byte("k"), maxPassphraseBytes)},
	} {
		got, err := readPipe(tc.input)
		if err != nil || !bytes.Equal(got, tc.want) {
			t.Fatalf("%s: got len=%d err=%v, want len=%d", tc.name, len(got), err, len(tc.want))
		}
		clear(got)
	}

	for _, tc := range []struct {
		name  string
		input []byte
	}{
		{"4097-cr-eof", append(bytes.Repeat([]byte("z"), maxPassphraseBytes+1), '\r')},
		{"4097-crlf", append(bytes.Repeat([]byte("z"), maxPassphraseBytes+1), '\r', '\n')},
	} {
		got, err := readPipe(tc.input)
		if err == nil || len(got) != 0 {
			t.Fatalf("%s should reject over-limit passphrase, got len=%d err=%v", tc.name, len(got), err)
		}
	}
}

func TestMigratePublishFailureOmitsCompleteReportAndPreservesPending(t *testing.T) {
	root, err := filepath.EvalSymlinks(t.TempDir())
	if err != nil {
		t.Fatal(err)
	}
	userHome, dataHome, _ := setIsolatedCLIEnv(t, root)
	v1Dir := defaultV1DirForTest(userHome, dataHome)
	source, dbpath, fixture, err := v1fixture.Build(v1Dir)
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

	for _, jsonMode := range []bool{true, false} {
		modeName := "text"
		if jsonMode {
			modeName = "json"
		}
		t.Run(modeName, func(t *testing.T) {
			target := filepath.Join(root, "publish-fail-"+modeName)
			pr, pw, err := os.Pipe()
			if err != nil {
				t.Fatal(err)
			}
			go func() {
				_, _ = pw.Write([]byte(fixture.Passphrase + "\n"))
				_ = pw.Close()
			}()

			var out, errOut bytes.Buffer
			code := executeMigration(context.Background(), migrateOptions{
				source:        source,
				target:        target,
				service:       "UniClipboard",
				jsonMode:      jsonMode,
				stdinPassword: true,
				in:            pr,
				out:           &out,
				errOut:        &errOut,
				writeState: func(string, climigration.State) error {
					return os.ErrPermission
				},
			})
			_ = pr.Close()
			if code != 1 {
				t.Fatalf("publish failure exit=%d want 1 (out=%q err=%q)", code, out.String(), errOut.String())
			}
			if strings.Contains(out.String(), `"status":"complete"`) || strings.Contains(out.String(), "Migration complete") {
				t.Fatalf("stdout emitted completion signal despite write failure:\n%s", out.String())
			}
			if jsonMode {
				var payload map[string]string
				if err := json.Unmarshal(errOut.Bytes(), &payload); err != nil || payload["reason"] != "state_unavailable" {
					t.Fatalf("expected state_unavailable JSON on stderr, got %q (%v)", errOut.String(), err)
				}
			}
			st, rerr := climigration.Read(target)
			if rerr != nil || st.Status != "pending" {
				t.Fatalf("expected pending state preserved for resume, got %+v (%v)", st, rerr)
			}

			// Resume with normal publish succeeds and emits complete.
			pr2, pw2, err := os.Pipe()
			if err != nil {
				t.Fatal(err)
			}
			go func() {
				_, _ = pw2.Write([]byte(fixture.Passphrase + "\n"))
				_ = pw2.Close()
			}()
			out.Reset()
			errOut.Reset()
			code = executeMigration(context.Background(), migrateOptions{
				source:        source,
				target:        target,
				service:       "UniClipboard",
				jsonMode:      jsonMode,
				stdinPassword: true,
				in:            pr2,
				out:           &out,
				errOut:        &errOut,
			})
			_ = pr2.Close()
			if code != 0 {
				t.Fatalf("resume after publish failure exit=%d (out=%q err=%q)", code, out.String(), errOut.String())
			}
			st, rerr = climigration.Read(target)
			if rerr != nil || st.Status != "complete" || st.Outcome != "imported" {
				t.Fatalf("expected complete after resume, got %+v (%v)", st, rerr)
			}
		})
	}
}
