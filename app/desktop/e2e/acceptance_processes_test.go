package e2e

import (
	"bytes"
	"context"
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

	api "github.com/uniclipboard/unic/internal/localapi"
	"github.com/uniclipboard/unic/internal/servicelifecycle"
)

type desktopRoleIdentity struct {
	Role         string    `json:"role"`
	PID          int       `json:"pid"`
	StartedAt    time.Time `json:"started_at"`
	Profile      string    `json:"profile"`
	BinaryPath   string    `json:"binary_path"`
	BinarySHA256 string    `json:"binary_sha256"`
}

// Failure inventory: docs/exec/desktop-process-failures.md. The first run of
// this scenario failed against the baseline before role implementation.
func scenarioDesktopProcesses(t *testing.T) {
	root, err := filepath.Abs("../../..")
	must(t, err)
	private := os.Getenv("UNIC_DESKTOP_E2E_CASE_PRIVATE")
	if private == "" {
		private = t.TempDir()
	}
	must(t, os.MkdirAll(private, 0700))
	base, err := os.MkdirTemp("", "uc-roles-")
	must(t, err)
	base, err = filepath.EvalSymlinks(base)
	must(t, err)
	checks := []string{}
	services := []servicelifecycle.Instance{}
	identities := []desktopRoleIdentity{}
	timeline := []map[string]any{}
	receipt := map[string]any{"status": "FAIL", "source_head": os.Getenv("UNIC_DESKTOP_E2E_SOURCE_HEAD"), "os": runtime.GOOS, "arch": runtime.GOARCH, "native_tray": "NOT_RUN", "native_shortcut": "NOT_RUN", "native_paste": "NOT_RUN", "fixture": "build-tagged nonvisual host, production role IPC and service", "role_start_time_semantics": "IPC owner acquisition timestamp; not kernel process creation time"}
	var cleanup func()
	cleanupPassed := true
	defer func() {
		if cleanup != nil {
			cleanup()
		}
		receipt["checks"], receipt["roles"], receipt["timeline"], receipt["services"] = checks, identities, timeline, services
		receipt["cleanup"] = "PASS"
		if !cleanupPassed {
			receipt["cleanup"] = "FAIL"
		}
		for _, directory := range []string{base, private} {
			_ = filepath.WalkDir(directory, func(path string, entry os.DirEntry, err error) error {
				if err != nil || entry.IsDir() || !strings.HasSuffix(path, ".log") {
					return err
				}
				raw, err := os.ReadFile(path)
				if err == nil && bytes.Contains(raw, []byte("DATA RACE")) {
					t.Errorf("race detected in owned process log %s", path)
				}
				return err
			})
		}
		if err := os.RemoveAll(base); err != nil {
			t.Error(err)
			receipt["cleanup"] = "FAIL"
		}
		if !t.Failed() {
			receipt["status"] = "PASS"
		}
		raw, _ := json.MarshalIndent(receipt, "", "  ")
		must(t, os.WriteFile(filepath.Join(private, "manifest.json"), raw, 0600))
	}()
	bin := filepath.Join(private, "bin")
	must(t, os.MkdirAll(bin, 0700))
	suffix := ""
	if runtime.GOOS == "windows" {
		suffix = ".exe"
	}
	binary := filepath.Join(bin, "unic-desktop"+suffix)
	race := runtime.GOOS != "windows" || runtime.GOARCH != "arm64"
	receipt["race_detector"] = race
	binaries := map[string]string{}
	for _, name := range []string{"unic", "unicd", "unic-desktop"} {
		args := []string{"build"}
		if race {
			args = append(args, "-race")
		}
		if name == "unic-desktop" {
			args = append(args, "-tags", "desktop_e2e")
		}
		args = append(args, "-o", filepath.Join(bin, name+suffix))
		pkg := "./app/service/cmd/" + name
		if name == "unic-desktop" {
			pkg = "./app/desktop/cmd/unic-desktop"
		}
		args = append(args, pkg)
		cmd := exec.Command("go", args...)
		cmd.Dir = root
		if out, err := cmd.CombinedOutput(); err != nil {
			t.Fatalf("build %s: %v\n%s", name, err, out)
		}
		binaries[name] = hash(t, filepath.Join(bin, name+suffix))
	}
	receipt["binary_sha256"] = binaries
	homes := []string{filepath.Join(base, "external"), filepath.Join(base, "owned"), filepath.Join(base, "foreign"), filepath.Join(base, "fresh")}
	envFor := func(home string) []string {
		env := []string{}
		for _, entry := range os.Environ() {
			if !strings.HasPrefix(entry, "UNIC_") && !strings.HasPrefix(entry, "MYGO_") {
				env = append(env, entry)
			}
		}
		discovery := filepath.Join(base, "discovery")
		return append(env, "UNIC_HOME="+home, "UNIC_SOCK="+filepath.Join(home, "unic.sock"), "UNIC_DESKTOP_E2E_PROFILE="+home, "MYGO_AUTOMATION=", "UNIC_DESKTOP_E2E_PROCESS_FIXTURE=1", "UNIC_RENDEZVOUS_URL=http://127.0.0.1:1", "HOME="+discovery, "USERPROFILE="+discovery, "APPDATA="+filepath.Join(discovery, "roaming"), "LOCALAPPDATA="+filepath.Join(discovery, "local"), "XDG_CONFIG_HOME="+filepath.Join(discovery, "config"), "XDG_DATA_HOME="+filepath.Join(discovery, "data"))
	}
	run := func(home, name string, args ...string) ([]byte, error) {
		ctx, cancel := context.WithTimeout(context.Background(), 40*time.Second)
		defer cancel()
		cmd := exec.CommandContext(ctx, filepath.Join(bin, name+suffix), args...)
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
	for _, home := range homes {
		must(t, os.MkdirAll(home, 0700))
		must(t, os.WriteFile(filepath.Join(home, "e2e-owner"), []byte("desktop-e2e"), 0600))
		must(t, os.WriteFile(filepath.Join(home, "config.toml"), []byte("[desktop]\nmode=\"normal\"\nstart_in_background=true\nquick_panel_shortcut=\"\"\n[telemetry]\nenabled=false\n[sync]\nenabled=false\n"), 0600))
		if filepath.Base(home) != "fresh" {
			success(home, "unic", "migrate", "--initialize-empty", "--json")
			must(t, os.WriteFile(filepath.Join(home, "desktop-setup.json"), []byte("{\"complete\":true}"), 0600))
		}
	}
	launched := []*child{}
	launch := func(home, name string, args ...string) *child {
		t.Helper()
		logPath := filepath.Join(private, fmt.Sprintf("role-%02d-%s.log", len(launched), name))
		log, err := os.Create(logPath)
		must(t, err)
		p := &child{cmd: exec.Command(filepath.Join(bin, name+suffix), args...), done: make(chan struct{}), log: logPath}
		p.cmd.Env = envFor(home)
		p.cmd.Stdout, p.cmd.Stderr = log, log
		createdAt := time.Now().UTC()
		must(t, p.cmd.Start())
		timeline = append(timeline, map[string]any{"at": createdAt, "event": "spawn", "profile": filepath.Base(home), "program": name, "args": args, "pid": p.cmd.Process.Pid})
		go func() { p.err = p.cmd.Wait(); _ = log.Close(); close(p.done) }()
		launched = append(launched, p)
		return p
	}
	command := func(home, role, verb string, result any) error {
		out, err := run(home, "unic-desktop", "--role", role, "--command", verb)
		timeline = append(timeline, map[string]any{"at": time.Now().UTC(), "profile": filepath.Base(home), "role": role, "command": verb, "success": err == nil, "result": string(bytes.TrimSpace(out))})
		// Failure output is plain diagnostic text, so retain it as text in the receipt.
		if err != nil {
			timeline[len(timeline)-1]["result"] = string(out)
			return fmt.Errorf("%s %s: %w: %s", role, verb, err, out)
		}
		if result != nil {
			return json.Unmarshal(out, result)
		}
		return nil
	}
	probe := func(home, role string) (desktopRoleIdentity, error) {
		var info desktopRoleIdentity
		err := command(home, role, "probe", &info)
		return info, err
	}
	waitRole := func(home, role string) desktopRoleIdentity {
		t.Helper()
		var info desktopRoleIdentity
		var last error
		deadline := time.Now().Add(35 * time.Second)
		for time.Now().Before(deadline) {
			info, last = probe(home, role)
			if last == nil {
				break
			}
			time.Sleep(80 * time.Millisecond)
		}
		if last != nil {
			t.Fatalf("%s role did not start: %v", role, last)
		}
		if info.PID <= 0 || info.Role != role || info.Profile != home || info.StartedAt.IsZero() || info.BinarySHA256 != binaries["unic-desktop"] {
			t.Fatalf("unverified role identity: %+v", info)
		}
		identities = append(identities, info)
		return info
	}
	waitGone := func(home, role string, pid int) {
		t.Helper()
		deadline := time.Now().Add(20 * time.Second)
		for time.Now().Before(deadline) {
			if _, err := probe(home, role); err != nil {
				exited(t, pid)
				return
			}
			time.Sleep(50 * time.Millisecond)
		}
		t.Fatalf("%s role PID %d outlived close", role, pid)
	}
	cFor := func(home string) *api.Client { return api.NewClient(filepath.Join(home, "unic.sock")) }
	service := func(home string) servicelifecycle.Instance {
		t.Helper()
		var v servicelifecycle.Instance
		eventually(t, func() bool {
			ctx, cancel := context.WithTimeout(context.Background(), time.Second)
			defer cancel()
			var err error
			v, err = cFor(home).Service(ctx)
			return err == nil && v.Directory == home
		})
		return v
	}
	cleanup = func() {
		// Graceful profile-scoped Quit precedes the bounded fallback, including failures.
		for _, home := range homes {
			_, _ = run(home, "unic-desktop", "--role", "tray", "--command", "quit")
		}
		for _, home := range homes {
			for _, role := range []string{"gui", "quick-panel", "tray"} {
				_, _ = run(home, "unic-desktop", "--role", role, "--command", "quit")
				var live desktopRoleIdentity
				if out, err := run(home, "unic-desktop", "--role", role, "--command", "probe"); err == nil && json.Unmarshal(out, &live) == nil && live.Profile == home && live.BinarySHA256 == binaries["unic-desktop"] {
					p, err := os.FindProcess(live.PID)
					if err == nil {
						_ = p.Kill()
					}
				}
			}
		}
		for _, p := range launched {
			_ = p.cmd.Process.Kill()
			select {
			case <-p.done:
			case <-time.After(5 * time.Second):
				t.Error("launch process cleanup timeout")
				cleanupPassed = false
			}
		}
		for _, home := range homes {
			ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
			v, err := cFor(home).Service(ctx)
			if err == nil && v.Directory == home {
				_ = cFor(home).StopService(ctx, v.ID)
			}
			cancel()
		}
	}
	freshHome := homes[3]
	launch(freshHome, "unic-desktop")
	freshTray := waitRole(freshHome, "tray")
	freshGUI := waitRole(freshHome, "gui")
	var setupStatus map[string]any
	must(t, command(freshHome, "gui", "status", &setupStatus))
	if setupStatus["setup_required"] != true {
		t.Fatalf("first launch bypassed setup: %v", setupStatus)
	}
	ctx, cancel := context.WithTimeout(context.Background(), time.Second)
	_, noService := cFor(freshHome).Service(ctx)
	cancel()
	if noService == nil {
		t.Fatal("fresh setup started service before explicit completion")
	}
	must(t, command(freshHome, "gui", "complete-setup", nil))
	services = append(services, service(freshHome))
	must(t, command(freshHome, "tray", "quit", nil))
	waitGone(freshHome, "tray", freshTray.PID)
	waitGone(freshHome, "gui", freshGUI.PID)
	checks = append(checks, "fresh manual normal launch shows setup in an independent GUI and starts the service only after explicit completion")
	// A CLI stream holds its temporary generation. Desktop must leave it intact.
	temporaryHome := homes[2]
	watcher := launch(temporaryHome, "unic", "dev", "watch")
	temporary := service(temporaryHome)
	if temporary.Mode != servicelifecycle.ModeCLITemporary {
		t.Fatalf("watch did not create temporary generation: %+v", temporary)
	}
	if out, err := run(temporaryHome, "unic-desktop", "--role", "tray"); err == nil || !bytes.Contains(out, []byte("temporary")) {
		t.Fatalf("tray did not refuse active temporary service: %v %s", err, out)
	}
	if service(temporaryHome).ID != temporary.ID {
		t.Fatal("tray disturbed active temporary generation")
	}
	ctx, cancel = context.WithTimeout(context.Background(), 5*time.Second)
	must(t, cFor(temporaryHome).StopService(ctx, temporary.ID))
	cancel()
	select {
	case <-watcher.done:
	case <-time.After(10 * time.Second):
		t.Fatal("CLI watcher did not exit after service shutdown")
	}
	exited(t, temporary.PID)
	services = append(services, temporary)
	checks = append(checks, "tray refuses an active CLI-temporary generation without replacing or stopping it")
	home := homes[0]
	launch(home, "unicd")
	external := service(home)
	services = append(services, external)
	launch(home, "unic-desktop", "--background")
	tray := waitRole(home, "tray")
	if _, err := probe(home, "gui"); err == nil {
		t.Fatal("explicit normal background launch ignored the saved background preference")
	}
	if service(home).ID != external.ID {
		t.Fatal("normal background tray replaced independently running unicd")
	}
	checks = append(checks, "explicit normal background launch retains a tray without GUI and reuses an independently running unicd generation")
	var status map[string]any
	must(t, command(home, "tray", "status", &status))
	for round := 0; round < 3; round++ {
		must(t, command(home, "tray", "open-gui", nil))
		gui := waitRole(home, "gui")
		if gui.PID == tray.PID || gui.PID == external.PID {
			t.Fatal("GUI does not have an independent OS process")
		}
		must(t, command(home, "gui", "status", &status))
		if status["visible"] != true {
			t.Fatalf("opened GUI is not visible: %v", status)
		}
		must(t, command(home, "tray", "open-gui", nil))
		same := waitRole(home, "gui")
		if same.PID != gui.PID {
			t.Fatal("second activation duplicated the GUI")
		}
		must(t, command(home, "gui", "close", nil))
		waitGone(home, "gui", gui.PID)
		if waitRole(home, "tray").PID != tray.PID || service(home).ID != external.ID {
			t.Fatal("closing GUI disturbed tray or unicd")
		}
	}
	checks = append(checks, "three GUI open/activate/close rounds create and terminate independent OS processes without disturbing tray or service")
	// Concurrent commands cross separate caller processes and the same production IPC.
	var workers sync.WaitGroup
	errs := make(chan error, 2)
	for i := 0; i < 2; i++ {
		workers.Add(1)
		go func() {
			defer workers.Done()
			_, err := run(home, "unic-desktop", "--role", "tray", "--command", "open-gui")
			errs <- err
		}()
	}
	workers.Wait()
	close(errs)
	for err := range errs {
		must(t, err)
	}
	gui := waitRole(home, "gui")
	must(t, command(home, "tray", "open-quick-panel", nil))
	panel := waitRole(home, "quick-panel")
	if panel.PID == gui.PID || panel.PID == tray.PID || panel.PID == external.PID {
		t.Fatal("quick panel shares another role process")
	}
	must(t, command(home, "quick-panel", "close", nil))
	hiddenPanel := waitRole(home, "quick-panel")
	if hiddenPanel.PID != panel.PID {
		t.Fatal("persistent panel changed process after hiding")
	}
	must(t, command(home, "quick-panel", "status", &status))
	if status["visible"] != false {
		t.Fatalf("closed quick panel is still visible: %v", status)
	}
	must(t, command(home, "gui", "close", nil))
	waitGone(home, "gui", gui.PID)
	must(t, command(home, "tray", "open-quick-panel", nil))
	if waitRole(home, "quick-panel").PID != panel.PID {
		t.Fatal("reopening quick panel duplicated its persistent process")
	}
	checks = append(checks, "concurrent GUI activation has one owner; quick panel has a distinct persistent process and opens without GUI")
	// Exercise the real adapter across role lifetimes, then verify exact history state.
	success(home, "unic", "send", "--to", "all", "--request-id", "role-durable", "desktop role durable fixture")
	var submitted struct {
		HistoryID string `json:"history_id"`
	}
	must(t, json.Unmarshal(success(home, "unic", "dev", "submission", "role-durable"), &submitted))
	if submitted.HistoryID == "" {
		t.Fatal("durable submission has no history ID")
	}
	success(home, "unic", "config", "set", "desktop.mode", `"normal"`)
	before, err := cFor(home).ListHistory(context.Background(), "", "")
	must(t, err)
	must(t, cFor(home).UseHistory(context.Background(), submitted.HistoryID))
	eventually(t, func() bool {
		raw, err := os.ReadFile(filepath.Join(home, "board", "text"))
		return err == nil && string(raw) == "desktop role durable fixture"
	})
	time.Sleep(700 * time.Millisecond)
	after, err := cFor(home).ListHistory(context.Background(), "", "")
	must(t, err)
	if len(after.Items) != len(before.Items) {
		t.Fatal("adapter restore created a history echo")
	}
	checks = append(checks, "tray-owned production adapter applies history with exact bytes and no extra history echo while GUI is absent")
	// A copied executable has a different installation identity and cannot control roles.
	foreignBin := filepath.Join(private, "foreign-unic-desktop"+suffix)
	raw, err := os.ReadFile(binary)
	must(t, err)
	must(t, os.WriteFile(foreignBin, raw, 0700))
	foreign := exec.Command(foreignBin, "--role", "tray", "--command", "status")
	foreign.Env = envFor(home)
	if out, err := foreign.CombinedOutput(); err == nil {
		t.Fatalf("different executable path controlled tray: %s", out)
	}
	if err := command(homes[2], "tray", "open-gui", nil); err == nil {
		t.Fatal("foreign profile activated another profile tray")
	}
	checks = append(checks, "foreign profile and different executable installation cannot activate the tray")
	for _, role := range []string{"gui", "quick-panel"} {
		verb := "open-gui"
		if role == "quick-panel" {
			verb = "open-quick-panel"
		}
		must(t, command(home, "tray", verb, nil))
		old := waitRole(home, role)
		p, err := os.FindProcess(old.PID)
		must(t, err)
		must(t, p.Kill())
		waitGone(home, role, old.PID)
		must(t, command(home, "tray", verb, nil))
		fresh := waitRole(home, role)
		if fresh.PID == old.PID {
			t.Fatal("crashed UI role was not recreated")
		}
		if waitRole(home, "tray").PID != tray.PID || service(home).ID != external.ID {
			t.Fatal("UI role crash disturbed tray or service")
		}
	}
	checks = append(checks, "GUI and quick-panel crashes release role ownership and permit independent recreation")
	must(t, command(home, "tray", "quit", nil))
	waitGone(home, "tray", tray.PID)
	if service(home).ID != external.ID {
		t.Fatal("explicit quit stopped independently started unicd")
	}
	checks = append(checks, "explicit Quit preserves an independently started service")
	// Restart through the saved normal mode. It must reuse the external service and durable state.
	launch(home, "unic-desktop")
	tray = waitRole(home, "tray")
	gui = waitRole(home, "gui")
	if service(home).ID != external.ID {
		t.Fatal("normal restart replaced the independent service")
	}
	history := success(home, "unic", "history", "show", submitted.HistoryID)
	if !bytes.Contains(history, []byte("desktop role durable fixture")) {
		t.Fatal("history lost across role restart")
	}
	cfg := success(home, "unic", "config", "--json")
	if !bytes.Contains(cfg, []byte(`"normal"`)) {
		t.Fatal("saved mode lost across role restart")
	}
	checks = append(checks, "saved normal mode opens GUI after full restart and retains history and settings")
	must(t, command(home, "tray", "quit", nil))
	waitGone(home, "tray", tray.PID)
	waitGone(home, "gui", gui.PID)
	// No independent service exists here. The tray must own and clean up the service it creates.
	ownedHome := homes[1]
	launch(ownedHome, "unic-desktop")
	ownedTray := waitRole(ownedHome, "tray")
	ownedService := service(ownedHome)
	services = append(services, ownedService)
	must(t, command(ownedHome, "tray", "open-gui", nil))
	ownedGUI := waitRole(ownedHome, "gui")
	must(t, command(ownedHome, "tray", "open-quick-panel", nil))
	ownedPanel := waitRole(ownedHome, "quick-panel")
	serviceProcess, err := os.FindProcess(ownedService.PID)
	must(t, err)
	must(t, serviceProcess.Kill())
	var recoveredService servicelifecycle.Instance
	eventually(t, func() bool {
		ctx, cancel := context.WithTimeout(context.Background(), time.Second)
		defer cancel()
		var err error
		recoveredService, err = cFor(ownedHome).Service(ctx)
		return err == nil && recoveredService.ID != ownedService.ID
	})
	if waitRole(ownedHome, "tray").PID != ownedTray.PID {
		t.Fatal("service restart replaced tray")
	}
	must(t, command(ownedHome, "tray", "open-gui", nil))
	if waitRole(ownedHome, "gui").PID != ownedGUI.PID {
		t.Fatal("service restart replaced GUI")
	}
	checks = append(checks, "tray recovers a crashed owned service without replacing independent GUI and panel roles")
	ownedService = recoveredService
	services = append(services, ownedService)
	trayProcess, err := os.FindProcess(ownedTray.PID)
	must(t, err)
	must(t, trayProcess.Kill())
	waitGone(ownedHome, "tray", ownedTray.PID)
	eventually(t, func() bool {
		ctx, cancel := context.WithTimeout(context.Background(), time.Second)
		defer cancel()
		_, err := cFor(ownedHome).Service(ctx)
		return err != nil
	})
	exited(t, ownedService.PID)
	launch(ownedHome, "unic-desktop")
	ownedTray = waitRole(ownedHome, "tray")
	ownedService = service(ownedHome)
	services = append(services, ownedService)
	must(t, command(ownedHome, "tray", "open-gui", nil))
	must(t, command(ownedHome, "tray", "open-quick-panel", nil))
	ownedGUI = waitRole(ownedHome, "gui")
	ownedPanel = waitRole(ownedHome, "quick-panel")
	checks = append(checks, "tray crash closes its owned service pipe and restart reclaims the profile role lock")
	success(ownedHome, "unic", "config", "set", "desktop.system_window_frame", "true")
	// A failed GUI close must reach the caller and leave the GUI available for a retry.
	rejectQuit := filepath.Join(ownedHome, "fixture-reject-gui-quit")
	must(t, os.WriteFile(rejectQuit, nil, 0600))
	if err := command(ownedHome, "tray", "restart-gui", nil); err == nil || !strings.Contains(err.Error(), "fixture rejected GUI quit") {
		t.Fatalf("expected GUI quit failure from restart, got %v", err)
	}
	if current := waitRole(ownedHome, "gui"); current.PID != ownedGUI.PID {
		t.Fatal("failed restart replaced the GUI")
	}
	must(t, os.Remove(rejectQuit))
	must(t, command(ownedHome, "tray", "restart-gui", nil))
	exited(t, ownedGUI.PID)
	reopenedGUI := waitRole(ownedHome, "gui")
	if reopenedGUI.PID == ownedGUI.PID || waitRole(ownedHome, "quick-panel").PID != ownedPanel.PID ||
		waitRole(ownedHome, "tray").PID != ownedTray.PID || service(ownedHome).ID != ownedService.ID {
		t.Fatal("GUI-only restart must replace only the GUI")
	}
	ownedGUI = reopenedGUI
	checks = append(checks, "GUI-only restart reports a close failure, accepts a retry, and preserves tray, quick panel and service")

	// Tray restart command closes graphical views, restarts the tray-owned service, and reopens GUI.
	cfgCtx, cfgCancel := context.WithTimeout(context.Background(), 3*time.Second)
	beforeRestartCfg, err := cFor(ownedHome).Config(cfgCtx)
	cfgCancel()
	must(t, err)
	if !beforeRestartCfg.Config.Desktop.SystemWindowFrame || len(beforeRestartCfg.RestartRequired) != 0 {
		t.Fatal("the frame preference must persist without a pending service restart")
	}
	existingRaw, err := os.ReadFile(filepath.Join(ownedHome, "config.toml"))
	must(t, err)
	updatedRaw := append([]byte(strings.TrimSpace(string(existingRaw))), []byte("\n[plugins.instances.\"builtin.classify\"]\nenabled = false\n")...)
	putCtx, putCancel := context.WithTimeout(context.Background(), 3*time.Second)
	savedRestartCfg, err := cFor(ownedHome).SaveConfig(putCtx, beforeRestartCfg.Revision, updatedRaw)
	putCancel()
	must(t, err)
	if len(savedRestartCfg.RestartRequired) == 0 {
		t.Fatal("expected pending restart requirement after disabling builtin.classify")
	}
	must(t, command(ownedHome, "tray", "restart", nil))
	waitGone(ownedHome, "quick-panel", ownedPanel.PID)
	exited(t, ownedGUI.PID)
	exited(t, ownedService.PID)
	var restartedService servicelifecycle.Instance
	eventually(t, func() bool {
		ctx, cancel := context.WithTimeout(context.Background(), time.Second)
		defer cancel()
		var err error
		restartedService, err = cFor(ownedHome).Service(ctx)
		return err == nil && restartedService.ID != ownedService.ID
	})
	var restartedGUI desktopRoleIdentity
	eventually(t, func() bool {
		id, err := probe(ownedHome, "gui")
		if err != nil || id.PID == ownedGUI.PID {
			return false
		}
		restartedGUI = id
		return true
	})
	afterCtx, afterCancel := context.WithTimeout(context.Background(), 3*time.Second)
	afterRestartCfg, err := cFor(ownedHome).Config(afterCtx)
	afterCancel()
	must(t, err)
	if len(afterRestartCfg.RestartRequired) != 0 {
		t.Fatalf("restart_required not cleared after tray restart: %v", afterRestartCfg.RestartRequired)
	}
	ownedGUI = restartedGUI
	ownedService = restartedService
	services = append(services, ownedService)
	checks = append(checks, "tray restart closes views, replaces its owned service to clear restart_required, and reopens GUI")
	must(t, command(ownedHome, "tray", "quit", nil))
	waitGone(ownedHome, "tray", ownedTray.PID)
	waitGone(ownedHome, "gui", ownedGUI.PID)
	eventually(t, func() bool {
		ctx, cancel := context.WithTimeout(context.Background(), time.Second)
		defer cancel()
		_, err := cFor(ownedHome).Service(ctx)
		return err != nil
	})
	exited(t, ownedService.PID)
	checks = append(checks, "explicit Quit retires the tray-owned service and every owned graphical role")
	receipt["setup_migration"] = "NOT_RUN in this process scenario; separate canonical first-install-setup and native setup cases"
	receipt["shortcut_permission_failure"] = "NOT_RUN in this process scenario; requires explicit fixture/native failure injection"
	receipt["service_restart"] = "PASS: real owned unicd crash and replacement"
}

// Updated failure inventory lives in cases/desktop-lightweight-close.txtar.
// This scenario uses actual OS role processes, the production entry and IPC,
// and an independent production unicd. The nonvisual host replaces rendering.
func scenarioLightweightClose(t *testing.T) {
	root, err := filepath.Abs("../../..")
	must(t, err)
	private := os.Getenv("UNIC_DESKTOP_E2E_CASE_PRIVATE")
	if private == "" {
		private = t.TempDir()
	}
	must(t, os.MkdirAll(private, 0700))
	base, err := os.MkdirTemp("", "uc-light-")
	must(t, err)
	base, err = filepath.EvalSymlinks(base)
	must(t, err)
	profile := filepath.Join(base, "profile")
	must(t, os.MkdirAll(profile, 0700))
	bin := filepath.Join(private, "bin")
	must(t, os.MkdirAll(bin, 0700))
	suffix := ""
	if runtime.GOOS == "windows" {
		suffix = ".exe"
	}
	race := runtime.GOOS != "windows" || runtime.GOARCH != "arm64"
	checks := []string{}
	roles := []desktopRoleIdentity{}
	timeline := []map[string]any{}
	receipt := map[string]any{"status": "FAIL", "source_head": os.Getenv("UNIC_DESKTOP_E2E_SOURCE_HEAD"), "race_detector": race, "os": runtime.GOOS, "arch": runtime.GOARCH, "fixture": "nonvisual host with production role IPC, service and OS processes", "native_window_focus": "NOT_RUN", "close_command_semantics": "gui close represents user window close; quit represents explicit whole-application Quit", "manual_intent": "no arguments; saved background preference does not hide the GUI", "background_argument": "--background is explicit login/background intent; manual launch has no arguments"}
	binaries := map[string]string{}
	for _, name := range []string{"unic", "unicd", "unic-desktop"} {
		args := []string{"build"}
		if race {
			args = append(args, "-race")
		}
		pkg := "./app/service/cmd/" + name
		if name == "unic-desktop" {
			args = append(args, "-tags", "desktop_e2e")
			pkg = "./app/desktop/cmd/unic-desktop"
		}
		args = append(args, "-o", filepath.Join(bin, name+suffix), pkg)
		build := exec.Command("go", args...)
		build.Dir = root
		if out, err := build.CombinedOutput(); err != nil {
			t.Fatalf("build %s: %v\n%s", name, err, out)
		}
		binaries[name] = hash(t, filepath.Join(bin, name+suffix))
	}
	receipt["binary_sha256"] = binaries
	env := []string{}
	for _, entry := range os.Environ() {
		if !strings.HasPrefix(entry, "UNIC_") && !strings.HasPrefix(entry, "MYGO_") {
			env = append(env, entry)
		}
	}
	discovery := filepath.Join(base, "discovery")
	env = append(env, "UNIC_HOME="+profile, "UNIC_SOCK="+filepath.Join(profile, "unic.sock"), "UNIC_DESKTOP_E2E_PROFILE="+profile, "UNIC_DESKTOP_E2E_PROCESS_FIXTURE=1", "MYGO_AUTOMATION=", "UNIC_RENDEZVOUS_URL=http://127.0.0.1:1", "HOME="+discovery, "USERPROFILE="+discovery, "APPDATA="+filepath.Join(discovery, "roaming"), "LOCALAPPDATA="+filepath.Join(discovery, "local"), "XDG_CONFIG_HOME="+filepath.Join(discovery, "config"), "XDG_DATA_HOME="+filepath.Join(discovery, "data"))
	run := func(name string, args ...string) ([]byte, error) {
		ctx, cancel := context.WithTimeout(context.Background(), 45*time.Second)
		defer cancel()
		cmd := exec.CommandContext(ctx, filepath.Join(bin, name+suffix), args...)
		cmd.Env = env
		return cmd.CombinedOutput()
	}
	success := func(name string, args ...string) []byte {
		t.Helper()
		out, err := run(name, args...)
		timeline = append(timeline, map[string]any{"at": time.Now().UTC(), "program": name, "args": args, "success": err == nil, "output": string(out)})
		if err != nil {
			t.Fatalf("%s %v: %v %s", name, args, err, out)
		}
		return out
	}
	command := func(role, verb string, result any) {
		t.Helper()
		raw := success("unic-desktop", "--role", role, "--command", verb)
		if result != nil {
			must(t, json.Unmarshal(raw, result))
		}
	}
	c := api.NewClient(filepath.Join(profile, "unic.sock"))
	type observed struct {
		role    string
		pid     int
		wait    func(context.Context) error
		release func()
	}
	observers := []observed{}
	var daemon *child
	var raceLaunchers []*child
	var raceCancels []context.CancelFunc
	cleanupPassed := true
	defer func() {
		for _, cancel := range raceCancels {
			cancel()
		}
		_, _ = run("unic-desktop", "--role", "tray", "--command", "quit")
		for _, role := range []string{"gui", "quick-panel"} {
			_, _ = run("unic-desktop", "--role", role, "--command", "quit")
		}
		for _, observer := range observers {
			ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
			if err := observer.wait(ctx); err != nil {
				t.Error(err)
				cleanupPassed = false
			}
			cancel()
			observer.release()
		}
		ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
		instance, err := c.Service(ctx)
		if err == nil && instance.Directory == profile {
			if err = c.StopService(ctx, instance.ID); err != nil {
				t.Error(err)
				cleanupPassed = false
			}
		}
		cancel()
		if daemon != nil {
			select {
			case <-daemon.done:
			case <-time.After(12 * time.Second):
				t.Error("task-owned independent daemon cleanup timed out")
				cleanupPassed = false
			}
		}
		for _, launcher := range raceLaunchers {
			select {
			case <-launcher.done:
			case <-time.After(5 * time.Second):
				t.Error("task-owned cold-start launcher cleanup timed out")
				cleanupPassed = false
			}
		}
		receipt["checks"], receipt["roles"], receipt["timeline"] = checks, roles, timeline
		receipt["cleanup"] = "PASS"
		if !cleanupPassed {
			receipt["cleanup"] = "FAIL"
		}
		if cleanupPassed {
			if err := os.RemoveAll(base); err != nil {
				t.Error(err)
				receipt["cleanup"] = "FAIL"
			}
		}
		if !t.Failed() {
			receipt["status"] = "PASS"
		}
		raw, _ := json.MarshalIndent(receipt, "", "  ")
		must(t, os.WriteFile(filepath.Join(private, "manifest.json"), raw, 0600))
	}()
	must(t, os.WriteFile(filepath.Join(profile, "e2e-owner"), []byte("desktop-e2e"), 0600))
	must(t, os.WriteFile(filepath.Join(profile, "config.toml"), []byte("[desktop]\nmode='lightweight'\nstart_in_background=true\nquick_panel_shortcut=''\n[telemetry]\nenabled=false\n[sync]\nenabled=false\n"), 0600))
	success("unic", "migrate", "--initialize-empty", "--json")
	must(t, os.WriteFile(filepath.Join(profile, "desktop-setup.json"), []byte(`{"complete":true}`), 0600))
	logPath := filepath.Join(private, "independent-daemon.log")
	log, err := os.Create(logPath)
	must(t, err)
	daemon = &child{cmd: exec.Command(filepath.Join(bin, "unicd"+suffix)), done: make(chan struct{}), log: logPath}
	daemon.cmd.Env = env
	daemon.cmd.Stdout, daemon.cmd.Stderr = log, log
	must(t, daemon.cmd.Start())
	go func() { daemon.err = daemon.cmd.Wait(); _ = log.Close(); close(daemon.done) }()
	var service servicelifecycle.Instance
	eventually(t, func() bool {
		ctx, cancel := context.WithTimeout(context.Background(), time.Second)
		defer cancel()
		var err error
		service, err = c.Service(ctx)
		return err == nil && service.Mode == servicelifecycle.ModeStandalone && service.PID == daemon.cmd.Process.Pid
	})
	receipt["independent_service"] = service
	sameService := func() {
		t.Helper()
		ctx, cancel := context.WithTimeout(context.Background(), 3*time.Second)
		defer cancel()
		current, err := c.Service(ctx)
		must(t, err)
		if current.ID != service.ID || current.PID != service.PID {
			t.Fatalf("independent service changed: before=%+v after=%+v", service, current)
		}
	}
	probe := func(role string) (desktopRoleIdentity, error) {
		var identity desktopRoleIdentity
		out, err := run("unic-desktop", "--role", role, "--command", "probe")
		if err == nil {
			err = json.Unmarshal(out, &identity)
		}
		return identity, err
	}
	waitRole := func(role string) desktopRoleIdentity {
		t.Helper()
		var identity desktopRoleIdentity
		var last error
		deadline := time.Now().Add(20 * time.Second)
		for time.Now().Before(deadline) {
			identity, last = probe(role)
			if last == nil {
				break
			}
			time.Sleep(80 * time.Millisecond)
		}
		if last != nil {
			t.Fatalf("manual lightweight launch did not create required %s OS process: %v", role, last)
		}
		if identity.Role != role || identity.Profile != profile || identity.PID <= 0 || identity.BinarySHA256 != binaries["unic-desktop"] {
			t.Fatalf("unverified role identity: %+v", identity)
		}
		wait, release, err := observeProcessExit(identity.PID)
		must(t, err)
		observers = append(observers, observed{role: role, pid: identity.PID, wait: wait, release: release})
		roles = append(roles, identity)
		return identity
	}
	success("unic-desktop") // Manual user launch, even with saved background intent.
	gui := waitRole("gui")
	tray := waitRole("tray")
	var status map[string]any
	command("gui", "status", &status)
	if status["visible"] != true {
		t.Fatalf("manual lightweight launch hid the GUI: %v", status)
	}
	sameService()
	checks = append(checks, "no-argument manual lightweight launch shows GUI despite saved start_in_background and reuses independent service")
	if strings.HasSuffix(t.Name(), "/desktop-lightweight-invalid-config-close") {
		success("unic", "config", "set", "desktop.mode", `"lightweight"`)
		ctx, cancel := context.WithTimeout(context.Background(), 3*time.Second)
		lastValid, err := c.Config(ctx)
		cancel()
		must(t, err)
		if lastValid.Config.Desktop.Mode != "lightweight" {
			t.Fatal("production CLI did not establish valid lightweight mode")
		}
		command("tray", "open-quick-panel", nil)
		waitRole("quick-panel")
		configPath := filepath.Join(profile, "config.toml")
		validRaw, err := os.ReadFile(configPath)
		must(t, err)
		defer func() {
			if err := os.WriteFile(configPath, validRaw, 0600); err != nil {
				t.Error(err)
			} else {
				receipt["private_config_restored_for_cleanup"] = true
			}
		}()
		invalidRaw := append(append([]byte{}, validRaw...), []byte("\n[desktop\nmode='lightweight'\n")...)
		must(t, os.WriteFile(configPath, invalidRaw, 0600))
		receipt["last_valid_mode"] = "lightweight"
		receipt["task_config_edit"] = "malformed TOML written after a valid production CLI save; no personal profile"
		command("gui", "close", nil)
		for _, observer := range observers {
			ctx, cancel := context.WithTimeout(context.Background(), 12*time.Second)
			err := observer.wait(ctx)
			cancel()
			if err != nil {
				t.Fatalf("malformed TOML stranded %s PID %d after lightweight GUI close: %v", observer.role, observer.pid, err)
			}
			if _, err := probe(observer.role); err == nil {
				t.Fatalf("malformed TOML retained %s IPC after lightweight GUI close", observer.role)
			}
		}
		sameService()
		checks = append(checks, "malformed TOML does not override the last valid lightweight close policy: every UI process and endpoint exits while the independent service identity remains")
		return
	}
	command("tray", "open-quick-panel", nil)
	panel := waitRole("quick-panel")
	if gui.PID == tray.PID || gui.PID == panel.PID || tray.PID == panel.PID || service.PID == gui.PID || service.PID == tray.PID || service.PID == panel.PID {
		t.Fatal("roles did not have independent OS processes")
	}
	command("gui", "close", nil) // Updated contract: user window close, not Explicit Quit.
	for _, observer := range observers {
		ctx, cancel := context.WithTimeout(context.Background(), 12*time.Second)
		err := observer.wait(ctx)
		cancel()
		if err != nil {
			t.Fatalf("lightweight GUI close retained %s PID %d: %v", observer.role, observer.pid, err)
		}
		if _, err := probe(observer.role); err == nil {
			t.Fatalf("lightweight GUI close retained %s IPC", observer.role)
		}
	}
	sameService()
	checks = append(checks, "lightweight GUI window close terminates GUI, tray and panel OS processes and IPC while retaining the exact service PID and generation")
	success("unic-desktop")
	reopened := waitRole("gui")
	waitRole("tray")
	command("gui", "status", &status)
	if reopened.PID == gui.PID && reopened.StartedAt.Equal(gui.StartedAt) {
		t.Fatal("manual reopening retained the exited GUI process identity")
	}
	if status["visible"] != true {
		t.Fatal("manual reopening hid the new GUI")
	}
	sameService()
	checks = append(checks, "a later no-argument manual launch creates a fresh visible GUI and preserves the independent service")
	command("tray", "quit", nil)
	sameService()
	checks = append(checks, "explicit Quit also preserves the independently started service")

	// Explicit login/background intent is separate from manual launch.
	success("unic-desktop", "--background")
	for _, role := range []string{"gui", "quick-panel", "tray"} {
		if _, err := probe(role); err == nil {
			t.Fatalf("lightweight background launch retained %s UI role", role)
		}
	}
	sameService()
	checks = append(checks, "explicit lightweight background launch keeps only the existing independent service")

	// Retire the driver-owned S0. A manual lightweight launch must now own S1.
	stopCtx, stopCancel := context.WithTimeout(context.Background(), 5*time.Second)
	must(t, c.StopService(stopCtx, service.ID))
	stopCancel()
	select {
	case <-daemon.done:
	case <-time.After(15 * time.Second):
		t.Fatal("independent S0 did not exit before owned-service scenario")
	}
	success("unic-desktop")
	ownedGUI := waitRole("gui")
	waitRole("tray")
	command("tray", "open-quick-panel", nil)
	waitRole("quick-panel")
	probeCtx, probeCancel := context.WithTimeout(context.Background(), 3*time.Second)
	ownedService, err := c.Service(probeCtx)
	probeCancel()
	must(t, err)
	if ownedService.Mode != servicelifecycle.ModeDesktop {
		t.Fatalf("manual launch did not create a tray-owned service: %+v", ownedService)
	}
	receipt["owned_service_before_close"] = ownedService
	service = ownedService
	command("gui", "close", nil)
	for _, observer := range observers {
		ctx, cancel := context.WithTimeout(context.Background(), 12*time.Second)
		err := observer.wait(ctx)
		cancel()
		if err != nil {
			t.Fatalf("owned lightweight close retained %s PID %d: %v", observer.role, observer.pid, err)
		}
		if _, err := probe(observer.role); err == nil {
			t.Fatalf("owned lightweight close retained %s IPC", observer.role)
		}
	}
	sameService()
	probeCtx, probeCancel = context.WithTimeout(context.Background(), 3*time.Second)
	detached, err := c.Service(probeCtx)
	probeCancel()
	must(t, err)
	if detached.Mode != servicelifecycle.ModeStandalone {
		t.Fatalf("lightweight close did not detach owned service: %+v", detached)
	}
	receipt["owned_service_after_close"] = detached
	checks = append(checks, "lightweight GUI close detaches its tray-owned service without changing PID or generation and ends every UI process")
	success("unic-desktop")
	nextGUI := waitRole("gui")
	waitRole("tray")
	sameService()
	if nextGUI.PID == ownedGUI.PID && nextGUI.StartedAt.Equal(ownedGUI.StartedAt) {
		t.Fatal("owned-service reopening reused the exited GUI identity")
	}
	command("gui", "close", nil)

	// The close reply may precede complete retirement. Later manual activation
	// must survive that transition and concurrent launch requests.
	results := make(chan error, 2)
	for index := 0; index < 2; index++ {
		go func() { _, err := run("unic-desktop"); results <- err }()
	}
	var reopenError error
	for index := 0; index < 2; index++ {
		if err := <-results; err != nil {
			reopenError = err
		}
	}
	must(t, reopenError)
	concurrentGUI := waitRole("gui")
	waitRole("tray")
	command("gui", "status", &status)
	if concurrentGUI.PID == nextGUI.PID && concurrentGUI.StartedAt.Equal(nextGUI.StartedAt) {
		t.Fatal("concurrent reopen retained the closing GUI identity")
	}
	if status["visible"] != true {
		t.Fatal("concurrent manual reopening lost the visible GUI intent")
	}
	sameService()
	checks = append(checks, "concurrent no-argument reopening after close acknowledgement creates one new visible GUI and preserves detached unicd")
	command("gui", "close", nil)
	for _, observer := range observers {
		ctx, cancel := context.WithTimeout(context.Background(), 12*time.Second)
		err := observer.wait(ctx)
		cancel()
		must(t, err)
	}
	sameService()

	// The same profile can explicitly start normal mode in the background.
	configPath := filepath.Join(profile, "normal-background.toml")
	must(t, os.WriteFile(configPath, []byte("[desktop]\nmode='normal'\nstart_in_background=true\nquick_panel_shortcut=''\n[telemetry]\nenabled=false\n[sync]\nenabled=false\n"), 0600))
	success("unic", "config", "apply", configPath)
	success("unic-desktop", "--background")
	normalTray := waitRole("tray")
	if _, err := probe("gui"); err == nil {
		t.Fatal("normal background launch created an unwanted GUI process")
	}
	if _, err := probe("quick-panel"); err == nil {
		t.Fatal("normal background launch created an unwanted panel process")
	}
	sameService()
	success("unic-desktop")
	manualNormalGUI := waitRole("gui")
	command("gui", "status", &status)
	if status["visible"] != true {
		t.Fatal("manual normal activation obeyed the saved background preference")
	}
	command("gui", "close", nil)
	ctx, cancel := context.WithTimeout(context.Background(), 12*time.Second)
	for _, observer := range observers {
		if observer.pid == manualNormalGUI.PID {
			must(t, observer.wait(ctx))
		}
	}
	cancel()
	retainedTray, err := probe("tray")
	must(t, err)
	if retainedTray.PID != normalTray.PID || !retainedTray.StartedAt.Equal(normalTray.StartedAt) {
		t.Fatal("normal GUI close changed the background tray identity")
	}
	sameService()
	checks = append(checks, "explicit normal background launch retains only tray and service; manual activation shows GUI and normal close preserves tray")
	command("tray", "quit", nil)
	sameService()

	// A mode save affects the next window-close decision without restarting
	// unicd. End the borrowed detached service so this slice owns a normal one.
	for _, observer := range observers {
		if observer.pid == normalTray.PID {
			ctx, cancel := context.WithTimeout(context.Background(), 12*time.Second)
			err := observer.wait(ctx)
			cancel()
			must(t, err)
		}
	}
	waitServiceExit, releaseService, err := observeProcessExit(service.PID)
	must(t, err)
	defer releaseService()
	stopCtx, stopCancel = context.WithTimeout(context.Background(), 5*time.Second)
	must(t, c.StopService(stopCtx, service.ID))
	stopCancel()
	ctx, cancel = context.WithTimeout(context.Background(), 12*time.Second)
	err = waitServiceExit(ctx)
	cancel()
	must(t, err)
	success("unic-desktop")
	waitRole("gui")
	waitRole("tray")
	command("tray", "open-quick-panel", nil)
	waitRole("quick-panel")
	probeCtx, probeCancel = context.WithTimeout(context.Background(), 3*time.Second)
	service, err = c.Service(probeCtx)
	probeCancel()
	must(t, err)
	if service.Mode != servicelifecycle.ModeDesktop {
		t.Fatalf("normal live-save slice did not own its service: %+v", service)
	}
	receipt["live_mode_owned_service_before_save"] = service
	liveConfigPath := filepath.Join(profile, "live-lightweight.toml")
	must(t, os.WriteFile(liveConfigPath, []byte("[desktop]\nmode='lightweight'\nstart_in_background=false\nquick_panel_shortcut=''\n[telemetry]\nenabled=false\n[sync]\nenabled=false\n"), 0600))
	// Remove only the race detector's exit delay for this CLI call. The real
	// production CLI/API save must precede GUI close without a watcher sleep.
	save := exec.Command(filepath.Join(bin, "unic"+suffix), "config", "apply", liveConfigPath)
	save.Env = append(append([]string{}, env...), "GORACE=atexit_sleep_ms=0")
	savedOutput, err := save.CombinedOutput()
	timeline = append(timeline, map[string]any{"at": time.Now().UTC(), "program": "unic", "args": []string{"config", "apply", liveConfigPath}, "race_exit_delay_ms": 0, "success": err == nil, "output": string(savedOutput)})
	if err != nil {
		t.Fatalf("production CLI live settings save failed: %v %s", err, savedOutput)
	}
	configCtx, configCancel := context.WithTimeout(context.Background(), 3*time.Second)
	savedState, err := c.Config(configCtx)
	configCancel()
	must(t, err)
	receipt["live_save_restart_required"] = savedState.RestartRequired
	if savedState.Config.Desktop.Mode != "lightweight" || savedState.Config.Desktop.StartInBackground {
		t.Fatal("production config did not save the requested live settings")
	}
	for _, key := range savedState.RestartRequired {
		if key == "desktop.mode" || key == "desktop.start_in_background" {
			t.Fatalf("live Desktop setting incorrectly requires restart: %s", key)
		}
	}
	command("gui", "close", nil)
	for _, observer := range observers {
		ctx, cancel := context.WithTimeout(context.Background(), 12*time.Second)
		err := observer.wait(ctx)
		cancel()
		must(t, err)
		if _, err := probe(observer.role); err == nil {
			t.Fatalf("live normal-to-lightweight close retained %s IPC", observer.role)
		}
	}
	sameService()
	probeCtx, probeCancel = context.WithTimeout(context.Background(), 3*time.Second)
	liveDetached, err := c.Service(probeCtx)
	probeCancel()
	must(t, err)
	if liveDetached.Mode != servicelifecycle.ModeStandalone {
		t.Fatalf("live normal-to-lightweight close did not detach service: %+v", liveDetached)
	}
	receipt["live_mode_owned_service_after_close"] = liveDetached
	checks = append(checks, "production CLI mode/background save reports no restart requirement and immediate GUI close uses the new lightweight mode, ending UI while detaching the same owned service")

	// A background launcher must not expose its short desktop-owned acquisition
	// as a foreign owner to a concurrent manual tray. No production hook widens
	// this window; two rounds additionally trigger from actual service state.
	coldRaces := []map[string]any{}
	receipt["cold_background_manual_race"] = "NOT_RUN"
	startRaceLauncher := func(round int, name string, args ...string) *child {
		t.Helper()
		path := filepath.Join(private, fmt.Sprintf("cold-%02d-%s.log", round, name))
		output, err := os.Create(path)
		must(t, err)
		ctx, cancel := context.WithTimeout(context.Background(), 45*time.Second)
		raceCancels = append(raceCancels, cancel)
		process := &child{cmd: exec.CommandContext(ctx, filepath.Join(bin, "unic-desktop"+suffix), args...), done: make(chan struct{}), log: path}
		process.cmd.Env = env
		process.cmd.Stdout, process.cmd.Stderr = output, output
		must(t, process.cmd.Start())
		go func() { process.err = process.cmd.Wait(); _ = output.Close(); close(process.done) }()
		raceLaunchers = append(raceLaunchers, process)
		return process
	}
	for round := 1; round <= 4; round++ {
		serviceWait, serviceRelease, err := observeProcessExit(service.PID)
		must(t, err)
		ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
		err = c.StopService(ctx, service.ID)
		cancel()
		if err != nil {
			serviceRelease()
			must(t, err)
		}
		ctx, cancel = context.WithTimeout(context.Background(), 12*time.Second)
		err = serviceWait(ctx)
		cancel()
		serviceRelease()
		must(t, err)
		for _, role := range []string{"gui", "tray", "quick-panel"} {
			if _, err := probe(role); err == nil {
				t.Fatalf("cold race %d began with an existing %s role", round, role)
			}
		}
		record := map[string]any{"round": round, "background_start_at": time.Now().UTC(), "trigger": "near-simultaneous launches", "transient_desktop_seen": false}
		background := startRaceLauncher(round, "background", "--background")
		record["background_launcher_pid"] = background.cmd.Process.Pid
		var firstService servicelifecycle.Instance
		if round >= 3 {
			record["trigger"] = "first observed actual service state, then manual launch"
			deadline := time.Now().Add(20 * time.Second)
			for time.Now().Before(deadline) {
				ctx, cancel := context.WithTimeout(context.Background(), 200*time.Millisecond)
				candidate, err := c.Service(ctx)
				cancel()
				if err == nil {
					firstService = candidate
					record["first_service_observed_at"] = time.Now().UTC()
					record["first_service"] = candidate
					record["transient_desktop_seen"] = candidate.Mode == servicelifecycle.ModeDesktop
					break
				}
				select {
				case <-background.done:
					deadline = time.Now()
				default:
				}
				time.Sleep(2 * time.Millisecond)
			}
		}
		record["manual_start_at"] = time.Now().UTC()
		manual := startRaceLauncher(round, "manual")
		record["manual_launcher_pid"] = manual.cmd.Process.Pid
		<-background.done
		<-manual.done
		backgroundOutput, readErr := os.ReadFile(background.log)
		must(t, readErr)
		manualOutput, readErr := os.ReadFile(manual.log)
		must(t, readErr)
		record["background_output"] = string(backgroundOutput)
		record["manual_output"] = string(manualOutput)
		record["background_success"] = background.err == nil
		record["manual_success"] = manual.err == nil
		record["launchers_finished_at"] = time.Now().UTC()
		coldRaces = append(coldRaces, record)
		receipt["cold_background_manual_rounds"] = coldRaces
		if background.err != nil || manual.err != nil {
			receipt["cold_background_manual_race"] = "FAILED_DIFFERENT_REASON"
			if bytes.Contains(backgroundOutput, []byte("a desktop already owns this service")) || bytes.Contains(manualOutput, []byte("a desktop already owns this service")) {
				receipt["cold_background_manual_race"] = "REPRODUCED"
			}
			t.Fatalf("cold background/manual launch race round %d: background=%v (%s); manual=%v (%s)", round, background.err, backgroundOutput, manual.err, manualOutput)
		}
		observerStart := len(observers)
		concurrentColdGUI := waitRole("gui")
		concurrentColdTray := waitRole("tray")
		command("gui", "status", &status)
		if status["visible"] != true {
			t.Fatalf("cold race %d lost manual visible GUI intent", round)
		}
		ctx, cancel = context.WithTimeout(context.Background(), 3*time.Second)
		service, err = c.Service(ctx)
		cancel()
		must(t, err)
		record["settled_service"] = service
		if firstService.PID != 0 && (service.ID != firstService.ID || service.PID != firstService.PID) {
			t.Fatalf("cold race %d replaced the observed service generation", round)
		}
		if concurrentColdGUI.PID == concurrentColdTray.PID || service.PID == concurrentColdGUI.PID || service.PID == concurrentColdTray.PID {
			t.Fatalf("cold race %d shared role OS processes", round)
		}
		command("gui", "close", nil)
		for _, observer := range observers[observerStart:] {
			ctx, cancel := context.WithTimeout(context.Background(), 12*time.Second)
			err := observer.wait(ctx)
			cancel()
			must(t, err)
		}
		for _, role := range []string{"gui", "tray", "quick-panel"} {
			if _, err := probe(role); err == nil {
				t.Fatalf("cold race %d lightweight close retained %s IPC", round, role)
			}
		}
		sameService()
		ctx, cancel = context.WithTimeout(context.Background(), 3*time.Second)
		afterClose, err := c.Service(ctx)
		cancel()
		must(t, err)
		if afterClose.Mode != servicelifecycle.ModeStandalone {
			t.Fatalf("cold race %d did not preserve standalone core after UI close", round)
		}
		record["service_after_lightweight_close"] = afterClose
	}
	receipt["cold_background_manual_race"] = "NOT_REPRODUCED"
	checks = append(checks, "four real cold-service background/manual races completed without reproduction; both launchers succeeded, manual GUI stayed visible and lightweight close retained the same standalone core")
}
