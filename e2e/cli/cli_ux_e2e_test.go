package e2e

import (
	"bytes"
	"encoding/json"
	"os"
	"os/exec"
	"path/filepath"
	"runtime"
	"strings"
	"testing"
)

// TestCLIUXInitProfileDaemonDoctor verifies the first-run UX, global --profile,
// unic init (including --fresh), unic daemon start/status/restart/logs/stop,
// non-starting unic status, unic profile list, unic doctor, and unic completion.
func TestCLIUXInitProfileDaemonDoctor(t *testing.T) {
	if os.Getenv("UNIC_E2E") == "" {
		t.Skip("set UNIC_E2E=1")
	}
	work := shortTemp(t)
	userHome := filepath.Join(work, "home")
	configHome := filepath.Join(work, "cfg")
	dataHome := filepath.Join(work, "data")
	for _, d := range []string{userHome, configHome, dataHome} {
		if err := os.MkdirAll(d, 0o700); err != nil {
			t.Fatal(err)
		}
	}

	var baseEnv []string
	for _, e := range os.Environ() {
		if strings.HasPrefix(e, "UNIC_HOME=") || strings.HasPrefix(e, "UNIC_SOCK=") || strings.HasPrefix(e, "UNIC_PROFILE=") || strings.HasPrefix(e, "UC_PROFILE=") {
			continue
		}
		baseEnv = append(baseEnv, e)
	}
	baseEnv = append(baseEnv,
		"HOME="+userHome,
		"XDG_CONFIG_HOME="+configHome,
		"XDG_DATA_HOME="+dataHome,
		"LOCALAPPDATA="+dataHome,
		"UNIC_RENDEZVOUS_URL=http://127.0.0.1:1",
	)

	run := func(args ...string) (int, string, string) {
		t.Helper()
		cmd := exec.Command(filepath.Join(binDir, "unic"), args...)
		cmd.Env = baseEnv
		var stdout, stderr bytes.Buffer
		cmd.Stdout, cmd.Stderr = &stdout, &stderr
		err := cmd.Run()
		code := 0
		if err != nil {
			if ee, ok := err.(*exec.ExitError); ok {
				code = ee.ExitCode()
			} else {
				t.Fatal(err)
			}
		}
		return code, stdout.String(), stderr.String()
	}

	// 1. Per-command --help and completion work before init without starting unicd.
	for _, helpArgs := range [][]string{
		{"help"},
		{"help", "exit-codes"},
		{"init", "--help"},
		{"daemon", "--help"},
		{"daemon", "start", "--help"},
		{"device", "--help"},
		{"send", "--help"},
		{"recv", "--help"},
		{"status", "--help"},
		{"doctor", "--help"},
		{"completion", "bash"},
		{"completion", "zsh"},
		{"completion", "fish"},
	} {
		code, out, stderr := run(helpArgs...)
		if code != 0 || strings.TrimSpace(out) == "" {
			t.Fatalf("unic %v failed: code=%d stderr=%q", helpArgs, code, stderr)
		}
	}
	if code, _, _ := run("nosuchcommand"); code != 2 {
		t.Fatalf("unknown command should exit 2, got %d", code)
	}

	// 2. Uninitialized profile: doctor warns cleanly without mutating, status exits 6.
	code, out, stderr := run("--profile", "work", "doctor", "--json")
	if code != 0 {
		t.Fatalf("doctor before init failed: %d %s %s", code, out, stderr)
	}
	if code == 2 {
		t.Fatalf("unreachable")
	}
	code, _, stderr = run("--profile", "work", "status", "--json")
	if code != 6 || !strings.Contains(stderr, `"next_command":"unic init"`) || !strings.Contains(stderr, `"migrate_command":"unic migrate"`) {
		t.Fatalf("uninitialized status should exit 6 with unic init/migrate hint: %d %s", code, stderr)
	}
	if code, _, _ := run("init", "--discard-pending"); code != 2 {
		t.Fatalf("init --discard-pending without --fresh should exit 2, got %d", code)
	}

	// 3. Initialize named profile with post-command --profile flag and --name.
	code, out, stderr = run("init", "--profile", "work", "--name", "WorkLab", "--json")
	if code != 0 {
		t.Fatalf("unic init --profile work failed: %d %s %s", code, out, stderr)
	}
	var initResp struct {
		Status    string `json:"status"`
		Profile   string `json:"profile"`
		Directory string `json:"directory"`
		Config    string `json:"config"`
	}
	if err := json.Unmarshal([]byte(out), &initResp); err != nil || initResp.Status != "complete" || initResp.Profile != "work" {
		t.Fatalf("unexpected init JSON: %q (%v)", out, err)
	}
	// Disable telemetry for the test profile without duplicating [telemetry].
	cfgBytes, err := os.ReadFile(initResp.Config)
	if err != nil {
		t.Fatal(err)
	}
	if !strings.Contains(string(cfgBytes), "WorkLab") {
		t.Fatalf("config.toml missing device_name WorkLab:\n%s", cfgBytes)
	}
	updatedCfg := strings.Replace(string(cfgBytes), "[telemetry]\n  enabled = true", "[telemetry]\n  enabled = false", 1)
	if updatedCfg == string(cfgBytes) {
		updatedCfg = string(cfgBytes) + "\n[telemetry]\nenabled = false\n"
	}
	if err := os.WriteFile(initResp.Config, []byte(updatedCfg), 0o600); err != nil {
		t.Fatal(err)
	}
	// Idempotent repeat init.
	code, out, stderr = run("-p", "work", "init", "--json")
	if code != 0 || !strings.Contains(out, `"already_initialized":true`) {
		t.Fatalf("repeat init failed: %d %s %s", code, out, stderr)
	}

	// 4. v1 detection blocks default init, while --fresh explicitly initializes.
	v1Dir := filepath.Join(dataHome, "app.uniclipboard.desktop")
	if runtime.GOOS == "darwin" {
		v1Dir = filepath.Join(userHome, "Library", "Application Support", "app.uniclipboard.desktop")
	}
	if err := os.MkdirAll(v1Dir, 0o700); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(v1Dir, "marker"), []byte("v1"), 0o600); err != nil {
		t.Fatal(err)
	}
	code, _, stderr = run("-p", "fresh1", "init", "--json")
	if code == 0 || !strings.Contains(stderr, `"source_present"`) || strings.Contains(stderr, v1Dir) {
		t.Fatalf("init should refuse source_present without leaking v1Dir path %q: %d %s", v1Dir, code, stderr)
	}
	code, out, stderr = run("-p", "fresh1", "init", "--fresh", "--json")
	if code != 0 || !strings.Contains(out, `"outcome":"fresh"`) {
		t.Fatalf("init --fresh failed: %d %s %s", code, out, stderr)
	}
	_ = os.RemoveAll(v1Dir)

	// Invalid/unreadable config.toml when running `init --name` omits absolute path from JSON error.
	code, out, stderr = run("-p", "cfgbad", "init", "--fresh", "--json")
	if code != 0 {
		t.Fatalf("cfgbad init failed: %d %s %s", code, out, stderr)
	}
	var cfgBadResp struct {
		Directory string `json:"directory"`
		Config    string `json:"config"`
	}
	if err := json.Unmarshal([]byte(out), &cfgBadResp); err != nil {
		t.Fatal(err)
	}
	if err := os.Remove(cfgBadResp.Config); err != nil {
		t.Fatal(err)
	}
	if err := os.Mkdir(cfgBadResp.Config, 0o700); err != nil {
		t.Fatal(err)
	}
	code, out, stderr = run("-p", "cfgbad", "init", "--name", "BadCfg", "--json")
	if code != 1 || out != "" || !strings.Contains(stderr, `"config_unavailable"`) {
		t.Fatalf("expected config_unavailable JSON on stderr, got code=%d out=%q err=%q", code, out, stderr)
	}
	if strings.Contains(stderr, cfgBadResp.Config) || strings.Contains(stderr, cfgBadResp.Directory) {
		t.Fatalf("config_unavailable JSON leaked absolute path: %s", stderr)
	}

	// 5. unic status while stopped returns quickly and does NOT start a temporary unicd.
	code, out, stderr = run("-p", "work", "status", "--json")
	if code != 0 || !strings.Contains(out, `"running":false`) {
		t.Fatalf("stopped status failed: %d %s %s", code, out, stderr)
	}
	if _, err := os.Stat(filepath.Join(initResp.Directory, "unic.sock")); !os.IsNotExist(err) {
		t.Fatal("unic status unexpectedly created unic.sock")
	}

	// 6. unic daemon start / status / restart / logs / stop lifecycle.
	t.Cleanup(func() {
		_, _, _ = run("-p", "work", "daemon", "stop")
	})
	code, out, stderr = run("-p", "work", "daemon", "start", "--json")
	if code != 0 || !strings.Contains(out, `"started":true`) {
		logData, _ := os.ReadFile(filepath.Join(initResp.Directory, "daemon.log"))
		t.Fatalf("daemon start failed: %d %s %s\ndaemon.log:\n%s", code, out, stderr, logData)
	}
	var startResp struct {
		Instance struct {
			PID  int    `json:"pid"`
			Mode string `json:"mode"`
		} `json:"instance"`
	}
	if err := json.Unmarshal([]byte(out), &startResp); err != nil || startResp.Instance.PID == 0 || startResp.Instance.Mode != "standalone" {
		t.Fatalf("unexpected daemon start JSON: %s", out)
	}
	// Idempotent start.
	code, out, stderr = run("-p", "work", "daemon", "start", "--json")
	if code != 0 || !strings.Contains(out, `"started":false`) {
		t.Fatalf("idempotent daemon start failed: %d %s %s", code, out, stderr)
	}

	// Running status & doctor & profile list.
	code, out, stderr = run("-p", "work", "status", "--json")
	if code != 0 || !strings.Contains(out, `"running":true`) || !strings.Contains(out, `"WorkLab"`) {
		t.Fatalf("running status failed: %d %s %s", code, out, stderr)
	}
	code, out, stderr = run("-p", "work", "doctor", "--json")
	if code != 0 || !strings.Contains(out, `"ok":true`) {
		t.Fatalf("doctor while running failed: %d %s %s", code, out, stderr)
	}
	code, out, stderr = run("-p", "work", "profile", "list", "--json")
	if code != 0 || !strings.Contains(out, `"work"`) || !strings.Contains(out, `"running (standalone)"`) {
		t.Fatalf("profile list failed: %d %s %s", code, out, stderr)
	}

	// Restart creates a new standalone PID.
	code, out, stderr = run("-p", "work", "daemon", "restart", "--json")
	if code != 0 || !strings.Contains(out, `"restarted":true`) {
		t.Fatalf("daemon restart failed: %d %s %s", code, out, stderr)
	}
	var restartResp struct {
		Instance struct {
			PID int `json:"pid"`
		} `json:"instance"`
	}
	if err := json.Unmarshal([]byte(out), &restartResp); err != nil || restartResp.Instance.PID == startResp.Instance.PID {
		t.Fatalf("restart did not launch new PID: old=%d new=%s", startResp.Instance.PID, out)
	}

	// Logs command succeeds.
	code, _, stderr = run("-p", "work", "daemon", "logs", "-n", "10")
	if code != 0 {
		t.Fatalf("daemon logs failed: %d %s", code, stderr)
	}

	// Stop removes socket cleanly; second stop is idempotent.
	code, out, stderr = run("-p", "work", "daemon", "stop", "--json")
	if code != 0 || !strings.Contains(out, `"stopped":true`) {
		t.Fatalf("daemon stop failed: %d %s %s", code, out, stderr)
	}
	code, out, stderr = run("-p", "work", "daemon", "stop", "--json")
	if code != 0 || !strings.Contains(out, `"stopped":false`) {
		t.Fatalf("idempotent daemon stop failed: %d %s %s", code, out, stderr)
	}
	code, out, stderr = run("-p", "work", "daemon", "status", "--json")
	if code != 0 || !strings.Contains(out, `"running":false`) {
		t.Fatalf("daemon status after stop failed: %d %s %s", code, out, stderr)
	}
}
