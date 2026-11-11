// Package e2e verifies desktop startup through real processes without opening a GUI.
package e2e

import (
	"bytes"
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"reflect"
	"runtime"
	"strconv"
	"strings"
	"testing"
	"time"

	"github.com/uniclipboard/unic/engine"
	api "github.com/uniclipboard/unic/internal/localapi"
	"github.com/uniclipboard/unic/internal/servicelifecycle"
)

type child struct {
	cmd  *exec.Cmd
	done chan struct{}
	err  error
	log  string
}

func TestStartupProcesses(t *testing.T) {
	if os.Getenv("UNIC_DESKTOP_STARTUP_E2E") != "1" {
		t.Skip("set UNIC_DESKTOP_STARTUP_E2E=1")
	}
	root, err := filepath.Abs("../../..")
	must(t, err)
	artifacts := os.Getenv("UNIC_DESKTOP_STARTUP_ARTIFACTS")
	if artifacts == "" {
		artifacts = t.TempDir()
	}
	artifacts, err = filepath.Abs(artifacts)
	must(t, err)
	must(t, os.MkdirAll(filepath.Join(artifacts, "bin"), 0700))
	checks := []string{}
	manifest := map[string]any{"status": "running", "go": runtime.Version(), "os": runtime.GOOS, "arch": runtime.GOARCH,
		"gui_launched": false, "native_login_tested": false, "native_visual_tested": false,
		"normal_mode_host": "nonvisual executable using production lifecycle package", "login_host": "file-backed OS fixture", "race": true}
	source := sourceHashes(t, root)
	manifest["source_sha256"] = source
	binaries := map[string]string{}
	manifest["binary_sha256"] = binaries
	defer func() {
		manifest["checks"] = checks
		unchanged := reflect.DeepEqual(source, sourceHashes(t, root))
		manifest["source_unchanged"] = unchanged
		if !unchanged {
			t.Error("source changed during acceptance")
		}
		manifest["status"] = "passed"
		if t.Failed() {
			manifest["status"] = "failed"
		}
		raw, _ := json.MarshalIndent(manifest, "", "  ")
		must(t, os.WriteFile(filepath.Join(artifacts, "manifest.json"), raw, 0600))
	}()
	bin := filepath.Join(artifacts, "bin")
	for _, target := range []struct{ name, source string }{
		{"unicd", "./app/service/cmd/unicd"}, {"unic", "./app/service/cmd/unic"},
		{"unic-desktop", "./app/desktop/cmd/unic-desktop"}, {"owner", "./app/desktop/internal/lifecycle/testdata/owner.go"},
	} {
		path := filepath.Join(bin, target.name)
		cmd := exec.Command("go", "build", "-race", "-o", path, target.source)
		cmd.Dir = root
		out, err := cmd.CombinedOutput()
		if err != nil {
			t.Fatalf("build %s: %v\n%s", target.name, err, out)
		}
		binaries[target.name] = hash(t, path)
	}
	base, err := os.MkdirTemp("", "uc-start-")
	must(t, err)
	t.Cleanup(func() { _ = os.RemoveAll(base) })
	envFor := func(home string) []string {
		env := []string{}
		for _, entry := range os.Environ() {
			if !strings.HasPrefix(entry, "UNIC_HOME=") && !strings.HasPrefix(entry, "UNIC_SOCK=") {
				env = append(env, entry)
			}
		}
		return append(env, "UNIC_HOME="+home, "UNIC_RENDEZVOUS_URL=http://127.0.0.1:1")
	}
	launchCount := 0
	launch := func(home, name string, args ...string) *child {
		t.Helper()
		launchCount++
		logPath := filepath.Join(artifacts, fmt.Sprintf("%02d-%s.log", launchCount, name))
		log, err := os.Create(logPath)
		must(t, err)
		p := &child{cmd: exec.Command(filepath.Join(bin, name), args...), done: make(chan struct{}), log: logPath}
		p.cmd.Env = envFor(home)
		p.cmd.Stdout, p.cmd.Stderr = log, log
		must(t, p.cmd.Start())
		go func() { p.err = p.cmd.Wait(); _ = log.Close(); close(p.done) }()
		t.Cleanup(func() {
			_ = p.cmd.Process.Kill()
			select {
			case <-p.done:
			case <-time.After(5 * time.Second):
			}
		})
		return p
	}
	run := func(home, name string, args ...string) ([]byte, error) {
		t.Helper()
		ctx, cancel := context.WithTimeout(context.Background(), 50*time.Second)
		defer cancel()
		cmd := exec.CommandContext(ctx, filepath.Join(bin, name), args...)
		cmd.Env = envFor(home)
		return cmd.CombinedOutput()
	}
	success := func(home, name string, args ...string) []byte {
		t.Helper()
		out, err := run(home, name, args...)
		if err != nil {
			t.Fatalf("%s %v: %v\n%s", name, args, err, out)
		}
		return out
	}
	home := filepath.Join(base, "a")
	other := filepath.Join(base, "b")
	for _, dir := range []string{home, other} {
		must(t, os.MkdirAll(dir, 0700))
	}
	c := api.NewClient(filepath.Join(home, "unic.sock"))
	b := api.NewClient(filepath.Join(other, "unic.sock"))
	// Clean up services created by short-lived lightweight launchers even when an assertion fails.
	t.Cleanup(func() {
		for _, client := range []*api.Client{c, b} {
			ctx, cancel := context.WithTimeout(context.Background(), 15*time.Second)
			v, err := client.Service(ctx)
			if err == nil && (v.Directory == home || v.Directory == other) {
				_ = client.StopService(ctx, v.ID)
				for ctx.Err() == nil {
					if _, err := os.Stat(client.Path); os.IsNotExist(err) {
						break
					}
					time.Sleep(50 * time.Millisecond)
				}
			}
			cancel()
		}
	})
	instance := func(c *api.Client, match func(servicelifecycle.Instance) bool) servicelifecycle.Instance {
		t.Helper()
		var result servicelifecycle.Instance
		eventually(t, func() bool {
			ctx, cancel := context.WithTimeout(context.Background(), time.Second)
			defer cancel()
			var err error
			result, err = c.Service(ctx)
			return err == nil && match(result)
		})
		return result
	}
	stopped := func(c *api.Client) {
		t.Helper()
		eventually(t, func() bool { _, err := os.Stat(c.Path); return os.IsNotExist(err) })
	}
	quit := func(p *child) {
		t.Helper()
		must(t, p.cmd.Process.Signal(os.Interrupt))
		select {
		case <-p.done:
			must(t, p.err)
		case <-time.After(20 * time.Second):
			t.Fatal("owner failed to exit")
		}
	}
	launch(home, "unicd")
	old := instance(c, func(v servicelifecycle.Instance) bool { return v.Mode == servicelifecycle.ModeStandalone })
	launch(other, "unicd")
	unrelated := instance(b, func(v servicelifecycle.Instance) bool { return v.Mode == servicelifecycle.ModeStandalone })
	normal := launch(home, "owner")
	owned := instance(c, func(v servicelifecycle.Instance) bool {
		return v.Mode == servicelifecycle.ModeDesktop && v.ID != old.ID
	})
	manifest["normal_owner_pid"] = normal.cmd.Process.Pid
	manifest["normal_service_pid"] = owned.PID
	// Readiness must include working Engine operations, not only a process or socket.
	var submission engine.SubmissionResult
	success(home, "unic", "send", "--to", "all", "--request-id", "startup-e2e", "persist through mode switch")
	must(t, json.Unmarshal(success(home, "unic", "dev", "submission", "startup-e2e"), &submission))
	if submission.HistoryID == "" {
		t.Fatal("submission has no durable history identity")
	}
	success(home, "unic", "config", "set", "desktop.start_in_background", "true")
	success(home, "unic", "config", "set", "desktop.mode", `"lightweight"`)
	cfg := success(home, "unic", "config", "--json")
	if !bytes.Contains(cfg, []byte(`"start_in_background":true`)) && !bytes.Contains(cfg, []byte(`"start_in_background": true`)) {
		t.Fatalf("desktop settings missing: %s", cfg)
	}
	success(home, "unic", "config", "set", "desktop.launch_at_login", "true")
	if out, err := run(home, "unic", "config", "set", "desktop.mode", `"invalid"`); err == nil {
		t.Fatalf("invalid mode accepted: %s", out)
	}
	if out, err := run(home, "unic-desktop", "--mode", "lightweight"); err == nil {
		t.Fatalf("lightweight adopted a GUI-owned service: %s", out)
	}
	ctx, cancel := context.WithTimeout(context.Background(), time.Second)
	if err := c.StopService(ctx, old.ID); err == nil {
		t.Fatal("stale generation stopped replacement")
	}
	cancel()
	instance(c, func(v servicelifecycle.Instance) bool { return v.ID == owned.ID })
	checks = append(checks, "normal startup replaces existing service and admits Engine submissions", "strict desktop config persists through CLI; invalid mode rejected", "stale shutdown generation rejected", "lightweight refuses a live normal-mode owner")
	quit(normal)
	stopped(c)
	exited(t, owned.PID)
	instance(b, func(v servicelifecycle.Instance) bool { return v.ID == unrelated.ID })
	checks = append(checks, "normal owner signal stops owned service without touching another profile")

	success(home, "unic-desktop") // Persisted lightweight mode; no GUI is started.
	light := instance(c, func(v servicelifecycle.Instance) bool { return v.Mode == servicelifecycle.ModeStandalone })
	success(home, "unic-desktop")
	instance(c, func(v servicelifecycle.Instance) bool { return v.ID == light.ID })
	history := success(home, "unic", "history", "show", string(submission.HistoryID))
	if !bytes.Contains(history, []byte("persist through mode switch")) {
		t.Fatal("history was lost across service replacement")
	}
	checks = append(checks, "production desktop lightweight launcher exits and leaves one usable standalone service")

	normal = launch(home, "owner")
	owned = instance(c, func(v servicelifecycle.Instance) bool {
		return v.Mode == servicelifecycle.ModeDesktop && v.ID != light.ID
	})
	manifest["crash_service_pid"] = owned.PID
	must(t, normal.cmd.Process.Kill())
	<-normal.done
	stopped(c)
	exited(t, owned.PID)
	checks = append(checks, "abrupt normal owner death closes the parent pipe and stops unicd")
	normal = launch(home, "owner")
	owned = instance(c, func(v servicelifecycle.Instance) bool { return v.Mode == servicelifecycle.ModeDesktop })
	must(t, normal.cmd.Process.Kill())
	<-normal.done
	success(home, "unic-desktop", "--mode", "lightweight")
	instance(c, func(v servicelifecycle.Instance) bool {
		return v.Mode == servicelifecycle.ModeStandalone && v.ID != owned.ID
	})
	exited(t, owned.PID)
	checks = append(checks, "immediate lightweight restart after owner death waits for service retirement")

	// Standalone operation and configuration validation must not require a desktop.
	success(home, "unic-desktop", "--mode", "lightweight")
	light = instance(c, func(v servicelifecycle.Instance) bool { return v.Mode == servicelifecycle.ModeStandalone })
	raw, err := os.ReadFile(filepath.Join(home, "config.toml"))
	must(t, err)
	must(t, os.WriteFile(filepath.Join(home, "config.toml"), append(raw, []byte("\n[unexpected]\nkey = true\n")...), 0600))
	if out, err := run(home, "unic-desktop", "--mode", "lightweight"); err == nil {
		t.Fatalf("invalid config accepted: %s", out)
	}
	instance(c, func(v servicelifecycle.Instance) bool { return v.ID == light.ID })
	must(t, os.WriteFile(filepath.Join(home, "config.toml"), raw, 0600))
	checks = append(checks, "invalid startup config leaves an existing service untouched")
	// A service failure ends its normal owner; the owner does not reconnect to a new generation.
	normal = launch(home, "owner")
	owned = instance(c, func(v servicelifecycle.Instance) bool {
		return v.Mode == servicelifecycle.ModeDesktop && v.ID != light.ID
	})
	eventually(t, func() bool {
		raw, err := os.ReadFile(normal.log)
		return err == nil && bytes.Contains(raw, []byte("ready"))
	})
	ctx, cancel = context.WithTimeout(context.Background(), 5*time.Second)
	must(t, c.StopService(ctx, owned.ID))
	cancel()
	select {
	case <-normal.done:
		must(t, normal.err)
	case <-time.After(20 * time.Second):
		t.Fatal("normal owner outlived its service")
	}
	stopped(c)
	exited(t, owned.PID)
	success(home, "unic-desktop", "--mode", "lightweight")
	light = instance(c, func(v servicelifecycle.Instance) bool { return v.Mode == servicelifecycle.ModeStandalone })
	checks = append(checks, "normal owner exits when its service terminates")
	// A lightweight launcher must not adopt or replace a CLI-temporary generation (0021 C18b).
	ctx, cancel = context.WithTimeout(context.Background(), 5*time.Second)
	must(t, c.StopService(ctx, light.ID))
	cancel()
	stopped(c)
	exited(t, light.PID)
	watcher := launch(home, "unic", "dev", "watch")
	temporary := instance(c, func(v servicelifecycle.Instance) bool {
		return v.Mode == servicelifecycle.ModeCLITemporary && v.Leases == 1
	})
	refused, err := run(home, "unic-desktop", "--mode", "lightweight")
	if err == nil || !bytes.Contains(refused, []byte("temporary service")) {
		t.Fatalf("lightweight did not refuse a temporary generation: %v %s", err, refused)
	}
	instance(c, func(v servicelifecycle.Instance) bool { return v.ID == temporary.ID && v.Leases == 1 })
	must(t, watcher.cmd.Process.Signal(os.Interrupt))
	select {
	case <-watcher.done:
	case <-time.After(20 * time.Second):
		t.Fatal("watch outlived its interrupt")
	}
	stopped(c)
	exited(t, temporary.PID)
	success(home, "unic-desktop", "--mode", "lightweight")
	light = instance(c, func(v servicelifecycle.Instance) bool { return v.Mode == servicelifecycle.ModeStandalone })
	checks = append(checks, "lightweight launcher refuses a CLI-temporary generation and leaves it with its user")
	// Missing sibling must be detected before stopping an existing service.
	must(t, os.Rename(filepath.Join(bin, "unicd"), filepath.Join(bin, "unicd.saved")))
	if out, err := run(home, "owner"); err == nil {
		t.Fatalf("missing sibling accepted: %s", out)
	}
	must(t, os.Rename(filepath.Join(bin, "unicd.saved"), filepath.Join(bin, "unicd")))
	instance(c, func(v servicelifecycle.Instance) bool { return v.ID == light.ID })
	checks = append(checks, "missing sibling daemon does not stop the running service")
	// Validate the directory before allowing any shutdown through a socket override.
	mismatch := exec.Command(filepath.Join(bin, "owner"))
	mismatch.Env = append(envFor(other), "UNIC_SOCK="+c.Path)
	if out, err := mismatch.CombinedOutput(); err == nil {
		t.Fatalf("mismatched profile accepted: %s", out)
	}
	instance(c, func(v servicelifecycle.Instance) bool { return v.ID == light.ID })
	instance(b, func(v servicelifecycle.Instance) bool { return v.ID == unrelated.ID })
	checks = append(checks, "socket/profile mismatch cannot stop either service")

	// Separate processes exercise persistent intent, external disable and explicit retry.
	success(home, "owner", "--login", "--requested")
	must(t, os.Remove(filepath.Join(home, "native-login")))
	out := success(home, "owner", "--login", "--requested")
	if !bytes.Contains(out, []byte("enabled=false")) {
		t.Fatal("external disable was reversed")
	}
	success(home, "owner", "--login", "--requested", "--retry")
	success(home, "owner", "--login")
	if out, err := run(home, "owner", "--login", "--requested", "--fail"); err == nil {
		t.Fatalf("native error swallowed: %s", out)
	}
	success(home, "owner", "--login", "--requested")
	checks = append(checks, "file-backed login fixture: enable, external disable preserved, explicit retry, disable, failure and retry")

	for _, client := range []*api.Client{c, b} {
		v := instance(client, func(servicelifecycle.Instance) bool { return true })
		ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
		must(t, client.StopService(ctx, v.ID))
		cancel()
		stopped(client)
	}
	checks = append(checks, "standalone service shuts down through generation-scoped IPC")
	for _, dir := range []string{artifacts, home, other} {
		entries, err := os.ReadDir(dir)
		must(t, err)
		for _, entry := range entries {
			if !strings.HasSuffix(entry.Name(), ".log") {
				continue
			}
			raw, err := os.ReadFile(filepath.Join(dir, entry.Name()))
			must(t, err)
			if bytes.Contains(raw, []byte("DATA RACE")) {
				t.Fatalf("race in %s", entry.Name())
			}
		}
	}
}

func must(t *testing.T, err error) {
	t.Helper()
	if err != nil {
		t.Fatal(err)
	}
}
func eventually(t *testing.T, check func() bool) {
	t.Helper()
	deadline := time.Now().Add(35 * time.Second)
	for time.Now().Before(deadline) {
		if check() {
			return
		}
		time.Sleep(50 * time.Millisecond)
	}
	t.Fatal("process state did not converge")
}
func hash(t *testing.T, path string) string {
	t.Helper()
	raw, err := os.ReadFile(path)
	must(t, err)
	sum := sha256.Sum256(raw)
	return hex.EncodeToString(sum[:])
}
func sourceHashes(t *testing.T, root string) map[string]string {
	t.Helper()
	result := map[string]string{}
	for _, dir := range []string{"app/desktop", "app/service", "engine", "internal", "plugin"} {
		must(t, filepath.WalkDir(filepath.Join(root, dir), func(path string, entry os.DirEntry, err error) error {
			if err != nil {
				return err
			}
			if entry.IsDir() {
				return nil
			}
			if filepath.Ext(path) != ".go" {
				return nil
			}
			rel, err := filepath.Rel(root, path)
			if err != nil {
				return err
			}
			result[filepath.ToSlash(rel)] = hash(t, path)
			return nil
		}))
	}
	for _, name := range []string{"go.mod", "go.sum"} {
		result[name] = hash(t, filepath.Join(root, name))
	}
	return result
}

// Unix process state distinguishes an exited process from a removed socket.
func exited(t *testing.T, pid int) {
	t.Helper()
	if runtime.GOOS == "windows" {
		return
	}
	eventually(t, func() bool {
		out, err := exec.Command("ps", "-p", strconv.Itoa(pid), "-o", "stat=").Output()
		return err != nil || strings.HasPrefix(strings.TrimSpace(string(out)), "Z")
	})
}
