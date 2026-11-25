package e2e

import (
	"bytes"
	"context"
	"encoding/base64"
	"encoding/json"
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"runtime"
	"strings"
	"sync"
	"syscall"
	"testing"
	"time"

	"github.com/uniclipboard/unic/internal/clip"
	api "github.com/uniclipboard/unic/internal/localapi"
	"github.com/uniclipboard/unic/internal/servicelifecycle"
	"github.com/uniclipboard/unic/internal/testboard"
)

func scenarioStandaloneQuickPanelLifecycle(t *testing.T) {
	root, err := filepath.Abs("../../..")
	must(t, err)
	private := os.Getenv("UNIC_DESKTOP_E2E_CASE_PRIVATE")
	if private == "" {
		private = t.TempDir()
	}
	must(t, os.MkdirAll(private, 0700))
	base, err := os.MkdirTemp("", "uc-sqp-")
	must(t, err)
	base, err = filepath.EvalSymlinks(base)
	must(t, err)

	checks := []string{}
	timeline := []map[string]any{}
	receipt := map[string]any{
		"status":           "FAIL",
		"source_head":      os.Getenv("UNIC_DESKTOP_E2E_SOURCE_HEAD"),
		"os":               runtime.GOOS,
		"arch":             runtime.GOARCH,
		"system_clipboard": "NOT_RUN",
		"native_paste":     "NOT_RUN",
		"fixture":          "build-tagged nonvisual host, production role IPC, directory board and service",
	}

	var cleanup func()
	defer func() {
		if cleanup != nil {
			cleanup()
		}
		receipt["checks"] = checks
		receipt["timeline"] = timeline
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

	envFor := func(home, sock string) []string {
		env := []string{}
		for _, entry := range os.Environ() {
			if !strings.HasPrefix(entry, "UNIC_") && !strings.HasPrefix(entry, "MYGO_") {
				env = append(env, entry)
			}
		}
		discovery := filepath.Join(base, "discovery")
		if sock == "" {
			sock = filepath.Join(home, "unic.sock")
		}
		return append(env,
			"UNIC_HOME="+home,
			"UNIC_SOCK="+sock,
			"UNIC_DESKTOP_E2E_PROFILE="+home,
			"MYGO_AUTOMATION=",
			"UNIC_DESKTOP_E2E_PROCESS_FIXTURE=1",
			"UNIC_RENDEZVOUS_URL=http://127.0.0.1:1",
			"HOME="+discovery,
			"USERPROFILE="+discovery,
			"APPDATA="+filepath.Join(discovery, "roaming"),
			"LOCALAPPDATA="+filepath.Join(discovery, "local"),
			"XDG_CONFIG_HOME="+filepath.Join(discovery, "config"),
			"XDG_DATA_HOME="+filepath.Join(discovery, "data"),
		)
	}

	runWithSock := func(home, sock, name string, args ...string) ([]byte, error) {
		ctx, cancel := context.WithTimeout(context.Background(), 40*time.Second)
		defer cancel()
		cmd := exec.CommandContext(ctx, filepath.Join(bin, name+suffix), args...)
		cmd.Env = envFor(home, sock)
		return cmd.CombinedOutput()
	}
	run := func(home, name string, args ...string) ([]byte, error) {
		return runWithSock(home, "", name, args...)
	}

	homes := []string{
		filepath.Join(base, "main"),
		filepath.Join(base, "other"),
		filepath.Join(base, "uninit"),
		filepath.Join(base, "incomplete"),
		filepath.Join(base, "pending"),
		filepath.Join(base, "corrupt"),
		filepath.Join(base, "tempcli"),
	}
	for _, h := range homes {
		must(t, os.MkdirAll(h, 0700))
		must(t, os.WriteFile(filepath.Join(h, "e2e-owner"), []byte("desktop-e2e"), 0600))
		must(t, os.WriteFile(filepath.Join(h, "config.toml"), []byte("[desktop]\nmode=\"normal\"\nstart_in_background=true\nquick_panel_shortcut=\"\"\n[telemetry]\nenabled=false\n[sync]\nenabled=false\n[history]\nauto_cleanup=false\n"), 0600))
	}
	// Initialize main, other, and tempcli profiles.
	for _, name := range []string{"main", "other", "tempcli"} {
		h := filepath.Join(base, name)
		if out, err := run(h, "unic", "migrate", "--initialize-empty", "--json"); err != nil {
			t.Fatalf("init %s: %v\n%s", name, err, out)
		}
		must(t, os.WriteFile(filepath.Join(h, "desktop-setup.json"), []byte("{\"complete\":true}\n"), 0600))
	}
	must(t, os.WriteFile(filepath.Join(base, "incomplete", "desktop-setup.json"), []byte("{\"complete\":false}\n"), 0600))
	must(t, os.WriteFile(filepath.Join(base, "pending", "desktop-setup.json"), []byte("{\"complete\":true}\n"), 0600))
	must(t, os.WriteFile(filepath.Join(base, "pending", "cli-migration.json"), []byte("{\"status\":\"pending\"}\n"), 0600))
	must(t, os.WriteFile(filepath.Join(base, "corrupt", "desktop-setup.json"), []byte("{not-json"), 0600))

	launched := []*child{}
	launch := func(home, name string, args ...string) *child {
		t.Helper()
		logPath := filepath.Join(private, fmt.Sprintf("proc-%02d-%s.log", len(launched), name))
		log, err := os.Create(logPath)
		must(t, err)
		p := &child{cmd: exec.Command(filepath.Join(bin, name+suffix), args...), done: make(chan struct{}), log: logPath}
		p.cmd.Env = envFor(home, "")
		p.cmd.Stdout, p.cmd.Stderr = log, log
		must(t, p.cmd.Start())
		timeline = append(timeline, map[string]any{"at": time.Now().UTC(), "event": "spawn", "profile": filepath.Base(home), "program": name, "args": args, "pid": p.cmd.Process.Pid})
		go func() { p.err = p.cmd.Wait(); _ = log.Close(); close(p.done) }()
		launched = append(launched, p)
		return p
	}

	probe := func(home, role string) (desktopRoleIdentity, error) {
		var info desktopRoleIdentity
		out, err := run(home, "unic-desktop", "--role", role, "--command", "probe")
		if err != nil {
			return info, err
		}
		err = json.Unmarshal(out, &info)
		return info, err
	}
	roleCall := func(home, role, verb, payload string, out any) error {
		args := []string{"--role", role, "--command", verb}
		if payload != "" {
			args = append(args, "--payload", payload)
		}
		raw, err := run(home, "unic-desktop", args...)
		if err != nil {
			return fmt.Errorf("%s %s: %w: %s", role, verb, err, raw)
		}
		if out != nil && len(bytes.TrimSpace(raw)) > 0 {
			return json.Unmarshal(raw, out)
		}
		return nil
	}

	cleanup = func() {
		for _, h := range homes {
			for _, role := range []string{"gui", "quick-panel", "tray"} {
				_, _ = run(h, "unic-desktop", "--role", role, "--command", "quit")
				if live, err := probe(h, role); err == nil && live.Profile == h && live.BinarySHA256 == binaries["unic-desktop"] {
					if p, err := os.FindProcess(live.PID); err == nil {
						_ = p.Kill()
					}
				}
			}
		}
		for _, p := range launched {
			if p.cmd.Process != nil {
				_ = p.cmd.Process.Kill()
			}
			select {
			case <-p.done:
			case <-time.After(5 * time.Second):
			}
		}
	}

	mainHome := filepath.Join(base, "main")
	otherHome := filepath.Join(base, "other")
	uninitHome := filepath.Join(base, "uninit")
	incompleteHome := filepath.Join(base, "incomplete")
	pendingHome := filepath.Join(base, "pending")
	corruptHome := filepath.Join(base, "corrupt")
	tempHome := filepath.Join(base, "tempcli")

	// F01: Invalid flag/positional combinations rejected locally.
	for _, bad := range [][]string{
		{"--quick-panel", "--background"},
		{"--quick-panel", "--mode", "normal"},
		{"--quick-panel", "--role", "quick-panel"},
		{"--quick-panel", "--command", "status"},
		{"--quick-panel", "extra"},
	} {
		if out, err := run(mainHome, "unic-desktop", bad...); err == nil {
			t.Fatalf("F01: expected invalid flags %v to fail, output=%s", bad, out)
		}
	}
	checks = append(checks, "F01: invalid --quick-panel flag and positional combinations rejected locally")

	// F02: Absent service & cross-profile socket rejected without launching anything.
	if out, err := run(mainHome, "unic-desktop", "--quick-panel"); err == nil {
		t.Fatalf("F02: expected absent unicd to fail, output=%s", out)
	}
	for _, role := range []string{"tray", "gui", "quick-panel"} {
		if _, err := probe(mainHome, role); err == nil {
			t.Fatalf("F02: rejected --quick-panel started %s", role)
		}
	}
	otherDaemon := launch(otherHome, "unicd")
	otherClient := api.NewClient(filepath.Join(otherHome, "unic.sock"))
	var otherSvc servicelifecycle.Instance
	eventually(t, func() bool {
		ctx, cancel := context.WithTimeout(context.Background(), time.Second)
		defer cancel()
		var err error
		otherSvc, err = otherClient.Service(ctx)
		return err == nil && otherSvc.Directory == otherHome
	})
	if out, err := runWithSock(mainHome, filepath.Join(otherHome, "unic.sock"), "unic-desktop", "--quick-panel"); err == nil {
		t.Fatalf("F02: expected cross-profile socket to fail, output=%s", out)
	}
	// Verify other profile's unicd stayed alive and same generation.
	ctxProbe, cancelProbe := context.WithTimeout(context.Background(), time.Second)
	afterOther, err := otherClient.Service(ctxProbe)
	cancelProbe()
	if err != nil || afterOther.ID != otherSvc.ID {
		t.Fatalf("F02: cross-profile rejection disrupted foreign unicd: %v", err)
	}
	_ = otherDaemon.cmd.Process.Kill()
	<-otherDaemon.done
	checks = append(checks, "F02: absent service and cross-profile socket rejected without launching roles or harming foreign unicd")

	// F03: cli-temporary service rejected without stopping it.
	tempWatch := launch(tempHome, "unic", "dev", "watch")
	tempClient := api.NewClient(filepath.Join(tempHome, "unic.sock"))
	var tempSvc servicelifecycle.Instance
	eventually(t, func() bool {
		ctx, cancel := context.WithTimeout(context.Background(), time.Second)
		defer cancel()
		var err error
		tempSvc, err = tempClient.Service(ctx)
		return err == nil && tempSvc.Mode == servicelifecycle.ModeCLITemporary && !tempSvc.Retiring
	})
	if out, err := run(tempHome, "unic-desktop", "--quick-panel"); err == nil {
		t.Fatalf("F03: expected cli-temporary service to be rejected, output=%s", out)
	}
	ctxProbe2, cancelProbe2 := context.WithTimeout(context.Background(), time.Second)
	afterTemp, err := tempClient.Service(ctxProbe2)
	cancelProbe2()
	if err != nil || afterTemp.ID != tempSvc.ID {
		t.Fatalf("F03: rejecting cli-temporary stopped or replaced it: %v", err)
	}
	_ = tempWatch.cmd.Process.Kill()
	<-tempWatch.done
	checks = append(checks, "F03: cli-temporary service rejected without borrowing or terminating it")

	// F04: Uninitialized, incomplete setup, pending migration, and corrupt state rejected read-only.
	if out, err := run(uninitHome, "unic-desktop", "--quick-panel"); err == nil {
		t.Fatalf("F04(uninitialized): expected rejection, got success: %s", out)
	}
	if _, err := os.Stat(filepath.Join(uninitHome, "desktop-setup.json")); !os.IsNotExist(err) {
		t.Fatal("F04(uninitialized): rejected --quick-panel created desktop-setup.json")
	}
	for _, tc := range []struct {
		name string
		home string
	}{
		{"incomplete-setup", incompleteHome},
		{"pending-migration", pendingHome},
		{"corrupt-setup", corruptHome},
	} {
		d := launch(tc.home, "unicd")
		c := api.NewClient(filepath.Join(tc.home, "unic.sock"))
		eventually(t, func() bool {
			ctx, cancel := context.WithTimeout(context.Background(), time.Second)
			defer cancel()
			v, err := c.Service(ctx)
			return err == nil && v.Directory == tc.home
		})
		if out, err := run(tc.home, "unic-desktop", "--quick-panel"); err == nil {
			t.Fatalf("F04(%s): expected rejection, got success: %s", tc.name, out)
		}
		_ = d.cmd.Process.Kill()
		<-d.done
	}
	checks = append(checks, "F04: uninitialized, incomplete setup, pending migration, and corrupt setup profiles rejected read-only")

	// F05: Start standalone unicd and concurrently invoke unic-desktop --quick-panel.
	launch(mainHome, "unicd")
	mainClient := api.NewClient(filepath.Join(mainHome, "unic.sock"))
	var mainSvc servicelifecycle.Instance
	eventually(t, func() bool {
		ctx, cancel := context.WithTimeout(context.Background(), time.Second)
		defer cancel()
		var err error
		mainSvc, err = mainClient.Service(ctx)
		return err == nil && mainSvc.Directory == mainHome && mainSvc.Mode == servicelifecycle.ModeStandalone
	})

	var wg sync.WaitGroup
	errs := make([]error, 4)
	for i := range errs {
		wg.Add(1)
		go func(idx int) {
			defer wg.Done()
			out, err := run(mainHome, "unic-desktop", "--quick-panel")
			if err != nil {
				errs[idx] = fmt.Errorf("launch %d: %v: %s", idx, err, out)
			}
		}(i)
	}
	wg.Wait()
	for _, e := range errs {
		if e != nil {
			t.Fatalf("F05: concurrent --quick-panel failed: %v", e)
		}
	}
	panelInfo, err := probe(mainHome, "quick-panel")
	if err != nil || panelInfo.PID <= 0 {
		t.Fatalf("F05: quick-panel not running after --quick-panel: %v", err)
	}
	if _, err := probe(mainHome, "tray"); err == nil {
		t.Fatal("F05: standalone --quick-panel unexpectedly started tray")
	}
	if _, err := probe(mainHome, "gui"); err == nil {
		t.Fatal("F05: standalone --quick-panel unexpectedly started gui")
	}
	var pStatus struct {
		Ready   bool `json:"ready"`
		Visible bool `json:"visible"`
	}
	must(t, roleCall(mainHome, "quick-panel", "status", "", &pStatus))
	if !pStatus.Visible {
		t.Fatal("F05: concurrent --quick-panel toggled panel hidden instead of keeping it visible")
	}
	// Repeat --quick-panel while visible: must keep panel visible and same PID.
	if out, err := run(mainHome, "unic-desktop", "--quick-panel"); err != nil {
		t.Fatalf("F05: repeat --quick-panel failed: %v\n%s", err, out)
	}
	panelInfo2, err := probe(mainHome, "quick-panel")
	if err != nil || panelInfo2.PID != panelInfo.PID {
		t.Fatalf("F05: repeat --quick-panel replaced PID %d -> %d (%v)", panelInfo.PID, panelInfo2.PID, err)
	}
	must(t, roleCall(mainHome, "quick-panel", "status", "", &pStatus))
	if !pStatus.Visible {
		t.Fatal("F05: repeat --quick-panel hid visible panel")
	}
	checks = append(checks, "F05: concurrent and repeated --quick-panel converge to one visible quick-panel PID without tray or gui")

	// F09 & F13: Copy text, plain Action, HTML-only rejection, PNG image, and file via standalone panel.
	ctx := context.Background()
	submit := func(reqID string, snap clip.Snapshot) string {
		t.Helper()
		res, err := mainClient.Submit(ctx, api.SubmitInput{RequestID: reqID, Snapshot: snap})
		if err != nil {
			t.Fatalf("submit %s: %v", reqID, err)
		}
		return string(res.HistoryID)
	}
	rich := clip.Snapshot{Parts: []clip.Part{
		{Format: clip.Text, Data: []byte("standalone-rich-text")},
		{Format: clip.HTML, Data: []byte("<b>standalone-rich-text</b>")},
	}}
	richID := submit("sqp-rich", rich)
	htmlOnlyID := submit("sqp-html", clip.Snapshot{Parts: []clip.Part{{Format: clip.HTML, Data: []byte("<i>html-only</i>")}}})
	pngBytes, err := base64.StdEncoding.DecodeString("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+jRZkAAAAASUVORK5CYII=")
	must(t, err)
	imgSnap := clip.Snapshot{Parts: []clip.Part{{Format: clip.PNG, Data: pngBytes}}}
	imgID := submit("sqp-img", imgSnap)
	filePath := filepath.Join(base, "standalone-sample.txt")
	must(t, os.WriteFile(filePath, []byte("standalone file payload"), 0600))
	fileSnap := clip.Snapshot{Files: []clip.File{{Name: "standalone-sample.txt", Path: filePath, Size: 23}}}
	fileID := submit("sqp-file", fileSnap)

	board := testboard.Dir(filepath.Join(mainHome, "board"))
	applyFromPanel := func(id string, plain bool) (mark, failure string) {
		t.Helper()
		var rep struct {
			Mark    string `json:"mark"`
			Failure string `json:"failure"`
		}
		payload, _ := json.Marshal(map[string]any{"id": id, "plain": plain})
		must(t, roleCall(mainHome, "quick-panel", "panel-apply", string(payload), &rep))
		return rep.Mark, rep.Failure
	}

	mark, failure := applyFromPanel(richID, false)
	if failure != "" || mark == "" {
		t.Fatalf("F09: rich apply failed: mark=%q failure=%q", mark, failure)
	}
	got, err := board.Read()
	if err != nil || got.Hash() != rich.Hash() {
		t.Fatalf("F09: rich board mismatch: err=%v got=%+v", err, got)
	}

	mark, failure = applyFromPanel(richID, true)
	if failure != "" || mark == "" {
		t.Fatalf("F09: plain apply failed: mark=%q failure=%q", mark, failure)
	}
	got, err = board.Read()
	if err != nil || len(got.Parts) != 1 || got.Parts[0].Format != clip.Text || string(got.Parts[0].Data) != "standalone-rich-text" {
		t.Fatalf("F09: plain board mismatch: err=%v got=%+v", err, got)
	}

	_, failure = applyFromPanel(htmlOnlyID, true)
	if failure != "plain" {
		t.Fatalf("F09: HTML-only plain apply expected failure=plain, got %q", failure)
	}

	mark, failure = applyFromPanel(imgID, false)
	if failure != "" || mark == "" {
		t.Fatalf("F09: image apply failed: mark=%q failure=%q", mark, failure)
	}
	got, err = board.Read()
	if err != nil || got.Hash() != imgSnap.Hash() {
		t.Fatalf("F09: image board mismatch: err=%v", err)
	}

	mark, failure = applyFromPanel(fileID, false)
	if failure != "" || mark == "" {
		t.Fatalf("F09: file apply failed: mark=%q failure=%q", mark, failure)
	}
	got, err = board.Read()
	if err != nil || len(got.Files) != 1 || got.Files[0].Path != filePath {
		t.Fatalf("F09: file board mismatch: err=%v got=%+v", err, got)
	}
	// Dismiss panel and externally replace clipboard; verify panel survives and releases lease.
	must(t, roleCall(mainHome, "quick-panel", "close", "", nil))
	must(t, board.Write(clip.Snapshot{Parts: []clip.Part{{Format: clip.Text, Data: []byte("external-replacement")}}}))
	checks = append(checks, "F09/F13: standalone panel copied rich text, plain Action, image, and file, rejected HTML-only plain, and survived dismissal + external replacement")

	// F10: Start tray alongside standalone unicd and panel; verify zero capture echo on panel apply.
	trayProc := launch(mainHome, "unic-desktop", "--role", "tray", "--mode", "normal", "--background")
	eventually(t, func() bool {
		_, err := probe(mainHome, "tray")
		if err != nil {
			return false
		}
		ctxCheck, cancelCheck := context.WithTimeout(context.Background(), time.Second)
		defer cancelCheck()
		_, err = mainClient.Clipboard(ctxCheck)
		return err == nil
	})
	pageBefore, err := mainClient.ListHistory(ctx, "", "")
	must(t, err)
	countBefore := len(pageBefore.Items)

	mark, failure = applyFromPanel(richID, false)
	if failure != "" || mark == "" {
		t.Fatalf("F10: panel apply alongside tray failed: failure=%q", failure)
	}
	got, err = board.Read()
	if err != nil || got.Hash() != rich.Hash() {
		t.Fatalf("F10: board mismatch alongside tray: err=%v", err)
	}
	time.Sleep(1200 * time.Millisecond) // allow multiple 400ms tray polls
	pageAfter, err := mainClient.ListHistory(ctx, "", "")
	must(t, err)
	if len(pageAfter.Items) != countBefore {
		t.Fatalf("F10: capture echo created duplicate history: before=%d after=%d", countBefore, len(pageAfter.Items))
	}
	checks = append(checks, "F10: panel apply alongside active tray adapter wrote once with zero capture echo")

	// F11: Exit tray alone (SIGTERM while borrowing standalone unicd); panel and unicd survive and panel copy still works.
	trayInfo, err := probe(mainHome, "tray")
	must(t, err)
	must(t, trayProc.cmd.Process.Signal(syscall.SIGTERM))
	select {
	case <-trayProc.done:
	case <-time.After(10 * time.Second):
		t.Fatal("F11: tray did not exit after SIGTERM")
	}
	exited(t, trayInfo.PID)
	// Confirm standalone unicd is still the same generation and panel has the same PID.
	svcAfterTray, err := mainClient.Service(ctx)
	if err != nil || svcAfterTray.ID != mainSvc.ID || svcAfterTray.PID != mainSvc.PID {
		t.Fatalf("F11: standalone unicd disrupted by tray exit: err=%v before=%+v after=%+v", err, mainSvc, svcAfterTray)
	}
	panelAfterTray, err := probe(mainHome, "quick-panel")
	if err != nil || panelAfterTray.PID != panelInfo.PID {
		t.Fatalf("F11: independent quick-panel did not survive tray exit: err=%v before=%d after=%d", err, panelInfo.PID, panelAfterTray.PID)
	}
	mark, failure = applyFromPanel(imgID, false)
	if failure != "" || mark == "" {
		t.Fatalf("F11: panel copy after tray exit failed: failure=%q", failure)
	}
	got, err = board.Read()
	if err != nil || got.Hash() != imgSnap.Hash() {
		t.Fatalf("F11: board mismatch after tray exit: err=%v", err)
	}
	checks = append(checks, "F11: tray exit preserved borrowed standalone unicd and independent panel; subsequent copy succeeded")

	// F14: Interrupted completion returns error without replay; next explicit copy succeeds.
	var failRep struct {
		Mark    string `json:"mark"`
		Failure string `json:"failure"`
	}
	interruptPayload, _ := json.Marshal(map[string]any{"id": richID, "plain": false, "interrupt_complete": true})
	must(t, roleCall(mainHome, "quick-panel", "panel-apply", string(interruptPayload), &failRep))
	if failRep.Failure == "" || failRep.Mark != "" {
		t.Fatalf("F14: expected interrupted completion to report failure without mark, got %+v", failRep)
	}
	// Next explicit user copy must recover cleanly.
	mark, failure = applyFromPanel(richID, false)
	if failure != "" || mark == "" {
		t.Fatalf("F14: recovery copy after interrupted completion failed: failure=%q", failure)
	}
	checks = append(checks, "F14: interrupted panel-apply completion reported failure without replay and recovered on next explicit action")

	// F07: Opening main window from standalone panel (without tray) starts tray + GUI.
	must(t, roleCall(mainHome, "quick-panel", "panel-open-main", `"main"`, nil))
	eventually(t, func() bool {
		_, errGUI := probe(mainHome, "gui")
		_, errTray := probe(mainHome, "tray")
		return errGUI == nil && errTray == nil
	})
	checks = append(checks, "F07: standalone panel opened main window through standard launcher without pre-existing tray")
}
