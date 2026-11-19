package e2e

import (
	"bytes"
	"context"
	"crypto/sha256"
	"database/sql"
	"encoding/hex"
	"encoding/json"
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"runtime"
	"strings"
	"sync"
	"testing"
	"time"

	"github.com/uniclipboard/unic/internal/v1migration/v1fixture"
)

// TestMigrationFirstRun drives real CLI and service processes. It deliberately
// uses no initialization helper: refusal before explicit completion is the subject.
func TestMigrationFirstRun(t *testing.T) {
	if os.Getenv("UNIC_E2E") == "" {
		t.Skip("set UNIC_E2E=1")
	}
	work := shortTemp(t)
	artifacts := os.Getenv("UNIC_FIRST_RUN_ARTIFACTS")
	if artifacts == "" {
		artifacts = t.TempDir()
	}
	must := func(err error) {
		t.Helper()
		if err != nil {
			t.Fatal(err)
		}
	}
	must(os.MkdirAll(artifacts, 0700))
	checks := []string{}
	receipt := map[string]any{"os": runtime.GOOS, "arch": runtime.GOARCH, "checks": &checks, "replay": "UNIC_E2E=1 go test ./e2e/cli -run '^TestMigrationFirstRun$' -count=1 -v"}
	defer func() {
		err := os.RemoveAll(work)
		if err != nil {
			t.Error(err)
		}
		receipt["profiles_removed"] = err == nil
		receipt["passed"] = !t.Failed()
		raw, err := json.MarshalIndent(receipt, "", "  ")
		must(err)
		must(os.WriteFile(filepath.Join(artifacts, "receipt.json"), raw, 0600))
		sum := sha256.Sum256(raw)
		must(os.WriteFile(filepath.Join(artifacts, "SHA256SUMS"), []byte(hex.EncodeToString(sum[:])+"  receipt.json\n"), 0600))
	}()
	root, err := repoRoot()
	must(err)
	head, err := exec.Command("git", "-C", root, "rev-parse", "HEAD").Output()
	must(err)
	receipt["source_commit"] = strings.TrimSpace(string(head))
	bins := map[string]string{}
	for _, name := range []string{"unic", "unicd"} {
		raw, err := os.ReadFile(filepath.Join(binDir, name))
		must(err)
		sum := sha256.Sum256(raw)
		bins[name] = hex.EncodeToString(sum[:])
	}
	receipt["binary_sha256"] = bins
	userHome := filepath.Join(work, "user")
	must(os.MkdirAll(userHome, 0700))
	baseEnv := append(os.Environ(), "HOME="+userHome, "XDG_DATA_HOME="+filepath.Join(userHome, "data"), "LOCALAPPDATA="+filepath.Join(userHome, "data"))
	profile := func(name string) string {
		p := filepath.Join(work, name)
		must(os.MkdirAll(p, 0700))
		must(os.WriteFile(filepath.Join(p, "config.toml"), []byte("[telemetry]\nenabled=false\n[history]\nauto_cleanup=false\n"), 0600))
		return p
	}
	call := func(home, input string, args ...string) (int, string, string) {
		c := exec.Command(filepath.Join(binDir, "unic"), args...)
		c.Env = append(baseEnv, "UNIC_HOME="+home, "UNIC_SOCK="+filepath.Join(home, "unic.sock"), "UNIC_RENDEZVOUS_URL=http://127.0.0.1:1")
		c.Stdin = strings.NewReader(input)
		var out, stderr bytes.Buffer
		c.Stdout = &out
		c.Stderr = &stderr
		err := c.Run()
		code := 0
		if err != nil {
			if e, ok := err.(*exec.ExitError); ok {
				code = e.ExitCode()
			} else {
				t.Fatal(err)
			}
		}
		return code, out.String(), stderr.String()
	}
	blocked := func(home string) {
		t.Helper()
		code, _, stderr := call(home, "", "status", "--json")
		var result map[string]any
		if code != 6 || json.Unmarshal([]byte(stderr), &result) != nil || result["reason"] != "migration_required" || result["next_command"] != "unic migrate --help" {
			t.Fatalf("expected actionable migration refusal: %d %s", code, stderr)
		}
		for _, name := range []string{"unic.sock", "unic.db"} {
			if _, err := os.Stat(filepath.Join(home, name)); !os.IsNotExist(err) {
				t.Fatalf("blocked command created %s", name)
			}
		}
	}
	legacyDefault := filepath.Join(userHome, "data", "app.uniclipboard.desktop")
	if runtime.GOOS == "darwin" {
		legacyDefault = filepath.Join(userHome, "Library", "Application Support", "app.uniclipboard.desktop")
	}
	code, _, _ := call(filepath.Join(work, "unused"), "", "migrate", "--initialize-empty", "--target", legacyDefault, "--json")
	if code == 0 {
		t.Fatal("no-source initialization allowed the v1 source directory as target")
	}
	if _, err := os.Stat(legacyDefault); !os.IsNotExist(err) {
		t.Fatal("no-source preflight modified the v1 source directory")
	}
	a := profile("fresh")
	blocked(a)
	for _, jsonFlag := range []string{"-json", "--json=true", "-json=true"} {
		code, _, stderr := call(a, "", "status", jsonFlag)
		var refusal map[string]any
		if code != 6 || json.Unmarshal([]byte(stderr), &refusal) != nil || refusal["reason"] != "migration_required" {
			t.Fatalf("JSON flag %s lost machine refusal: %d %s", jsonFlag, code, stderr)
		}
	}
	for _, args := range [][]string{{"help", "--json"}, {"--version", "--json"}, {"migrate", "--help"}} {
		code, _, stderr := call(a, "", args...)
		if code != 0 {
			t.Fatalf("guide inaccessible: %v %s", args, stderr)
		}
	}
	code, out, stderr := call(a, "", "migrate", "--initialize-empty", "--json")
	if code != 0 || !strings.Contains(out, `"status":"complete"`) {
		t.Fatalf("no-source initialization failed %d %s %s", code, out, stderr)
	}
	// Starting and retiring a temporary service is repeated across fresh processes.
	for range 2 {
		code, _, stderr = call(a, "", "status", "--json")
		if code != 0 {
			t.Fatalf("completed state lost after restart %d %s", code, stderr)
		}
	}
	checks = append(checks, "fresh JSON refusal has actionable reason and creates no service/database", "help/version/migrate remain accessible", "explicit no-source initialization and two service restarts")
	b := profile("failed")
	code, _, _ = call(b, "", "migrate", "--source", filepath.Join(work, "absent"), "--json")
	if code == 0 {
		t.Fatal("missing source succeeded")
	}
	blocked(b)
	code, _, _ = call(b, "", "migrate", "--initialize-empty", "--json")
	if code != 0 {
		t.Fatal("a nonexistent source permanently blocked explicit no-source initialization")
	}
	checks = append(checks, "invalid source cannot complete implicitly; explicit no-source initialization remains available")
	source, dbpath, fixture, err := v1fixture.Build(filepath.Join(work, "source"))
	must(err)

	// No target preflight may write inside the read-only source tree.
	for _, target := range []string{source, filepath.Join(source, "nested"), work} {
		code, _, _ = call(a, fixture.Passphrase+"\n", "migrate", "--source", source, "--target", target, "--passphrase-stdin", "--json")
		if code == 0 {
			t.Fatal("overlapping source/target succeeded")
		}
	}
	for _, name := range []string{"cli-migration.json", "unic.db", "nested"} {
		if _, err := os.Stat(filepath.Join(source, name)); !os.IsNotExist(err) {
			t.Fatalf("source preflight wrote %s", name)
		}
	}
	checks = append(checks, "source overlap is rejected before any target write")
	c := profile("import")
	code, _, stderr = call(c, fixture.Passphrase+"\n", "migrate", "--source", source, "--passphrase-stdin", "--dry-run", "--json")
	if code != 3 {
		t.Fatalf("dry run failed %d %s", code, stderr)
	}
	blocked(c)
	// The canonical fixture contains a damaged record. Partial is never complete.
	code, _, stderr = call(c, fixture.Passphrase+"\n", "migrate", "--source", source, "--passphrase-stdin", "--json")
	if code != 3 {
		t.Fatalf("expected partial fixture %d %s", code, stderr)
	}
	code, _, stderr = call(c, "", "history", "list", "--json")
	if code != 6 {
		t.Fatalf("partial import bypassed gate %d %s", code, stderr)
	}
	code, _, _ = call(c, "", "migrate", "--initialize-empty", "--json")
	if code == 0 {
		t.Fatal("partial import bypassed by no-source initialization")
	}
	checks = append(checks, "dry-run and partial import never complete first run")
	db, err := sql.Open("sqlite", dbpath)
	must(err)
	_, err = db.Exec(`INSERT INTO clipboard_snapshot_representation VALUES('rep-bad','event-bad','text/plain','text/plain',?,NULL,9)`, []byte(fixture.Repaired))
	must(err)
	must(db.Close())
	for range 2 {
		code, out, stderr = call(c, fixture.Passphrase+"\n", "migrate", "--source", source, "--passphrase-stdin", "--json")
		if code != 0 || !strings.Contains(out, `"status":"complete"`) {
			t.Fatalf("repair/resume failed %d %s %s", code, out, stderr)
		}
	}
	var previous string
	for range 2 {
		code, out, stderr = call(c, "", "history", "list", "--json")
		if code != 0 {
			t.Fatalf("completed import blocked after restart %d %s", code, stderr)
		}
		// Stable IDs, timestamps and imported rows survive fresh service generations.
		var rows []map[string]any
		must(json.Unmarshal([]byte(out), &rows))
		if len(rows) != 5 {
			t.Fatalf("reopened import has %d rows: %s", len(rows), out)
		}
		ids := []string{}
		for _, row := range rows {
			ids = append(ids, fmt.Sprint(row["id"]))
		}
		current := strings.Join(ids, ",")
		if previous != "" && current != previous {
			t.Fatal("restarting changed imported identities")
		}
		previous = current
	}
	checks = append(checks, "partial repair, repeat idempotence and two service restarts preserve five imported identities")
	// A target with a database still requires explicit completion for this CLI.
	must(os.Remove(filepath.Join(c, "cli-migration.json")))
	code, _, _ = call(c, "", "status", "--json")
	if code != 6 {
		t.Fatal("database presence bypassed first run")
	}
	// Explicit target never completes the caller profile.
	other := profile("other")
	code, _, stderr = call(other, fixture.Passphrase+"\n", "migrate", "--source", source, "--target", c, "--passphrase-stdin", "--json")
	if code != 0 {
		t.Fatalf("custom target failed %d %s", code, stderr)
	}
	blocked(other)
	checks = append(checks, "database presence cannot bypass first run; completion belongs to exact target")
	// Corrupt state fails closed; guidance remains accessible.
	corrupt := profile("corrupt")
	must(os.WriteFile(filepath.Join(corrupt, "cli-migration.json"), []byte("broken"), 0600))
	code, _, _ = call(corrupt, "", "status", "--json")
	if code == 0 {
		t.Fatal("corrupt state allowed service")
	}
	code, _, _ = call(corrupt, "", "migrate", "--help")
	if code != 0 {
		t.Fatal("corrupt state blocked guidance")
	}
	checks = append(checks, "corrupt state fails closed and keeps guidance accessible")
	// An interrupt while waiting for a supplied passphrase must not mark completion.
	cancelled := profile("cancelled")
	ctx, stop := context.WithTimeout(context.Background(), 15*time.Second)
	defer stop()
	proc := exec.CommandContext(ctx, filepath.Join(binDir, "unic"), "migrate", "--source", source, "--passphrase-stdin", "--json")
	proc.Env = append(baseEnv, "UNIC_HOME="+cancelled)
	stdin, err := proc.StdinPipe()
	must(err)
	defer func() { _ = stdin.Close() }()
	var cancelErr bytes.Buffer
	proc.Stderr = &cancelErr
	must(proc.Start())
	// Pending state is published before reading any secret.
	deadline := time.Now().Add(5 * time.Second)
	for {
		if _, e := os.Stat(filepath.Join(cancelled, "cli-migration.json")); e == nil {
			break
		}
		if time.Now().After(deadline) {
			_ = proc.Process.Kill()
			t.Fatal("pending state not published before credential input")
		}
		time.Sleep(10 * time.Millisecond)
	}
	waiter := exec.CommandContext(ctx, filepath.Join(binDir, "unic"), "migrate", "--source", source, "--passphrase-stdin", "--json")
	waiter.Env = append(baseEnv, "UNIC_HOME="+cancelled)
	waiterIn, err := waiter.StdinPipe()
	must(err)
	defer func() { _ = waiterIn.Close() }()
	must(waiter.Start())
	time.Sleep(100 * time.Millisecond)
	must(waiter.Process.Signal(os.Interrupt))
	waiterDone := make(chan error, 1)
	go func() { waiterDone <- waiter.Wait() }()
	select {
	case err := <-waiterDone:
		if err == nil {
			t.Fatal("cancelled lock waiter succeeded")
		}
	case <-time.After(3 * time.Second):
		_ = waiter.Process.Kill()
		t.Fatal("SIGINT did not interrupt lock wait")
	}
	checks = append(checks, "SIGINT interrupts a second process waiting for the migration lock")
	must(proc.Process.Signal(os.Interrupt))
	if err := proc.Wait(); err == nil {
		t.Fatal("cancelled import succeeded")
	}
	if ctx.Err() != nil {
		t.Fatal("interrupt did not release passphrase input")
	}
	code, _, _ = call(cancelled, "", "status", "--json")
	if code != 6 {
		t.Fatal("cancelled migration bypassed gate")
	}
	if _, err := os.Stat(filepath.Join(cancelled, "unic.sock")); !os.IsNotExist(err) {
		t.Fatal("cancelled migration started service")
	}
	checks = append(checks, "SIGINT during secret input exits and leaves pending state")

	// A different authenticated identity must not complete a pending source.
	mismatch := profile("mismatch")
	must(os.WriteFile(filepath.Join(mismatch, "cli-migration.json"), []byte(`{"status":"pending","source_id":"`+strings.Repeat("f", 64)+`"}`), 0600))
	code, _, stderr = call(mismatch, fixture.Passphrase+"\n", "migrate", "--source", source, "--passphrase-stdin", "--json")
	if code == 0 || !strings.Contains(stderr, "source_mismatch") {
		t.Fatalf("another source completed pending migration: %d %s", code, stderr)
	}
	code, _, _ = call(mismatch, "", "status", "--json")
	if code != 6 {
		t.Fatal("source mismatch released gate")
	}
	checks = append(checks, "authenticated source identity cannot replace another pending migration")
	// Multiple explicit initializers serialize and publish only valid state.
	d := profile("concurrent")
	var wg sync.WaitGroup
	codes := make(chan int, 4)
	for range 4 {
		wg.Add(1)
		go func() {
			defer wg.Done()
			code, _, _ := call(d, "", "migrate", "--initialize-empty", "--json")
			codes <- code
		}()
	}
	wg.Wait()
	close(codes)
	for code := range codes {
		if code != 0 {
			t.Fatalf("concurrent initializer failed %d", code)
		}
	}
	code, _, stderr = call(d, "", "status", "--json")
	if code != 0 {
		t.Fatalf("concurrent state invalid %d %s", code, stderr)
	}
	checks = append(checks, "concurrent explicit initialization produces durable completion")
	receipt["completed_at"] = time.Now().UTC().Format(time.RFC3339)
}
