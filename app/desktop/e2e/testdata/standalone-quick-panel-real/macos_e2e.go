//go:build darwin

// Command macos_e2e runs the Real-VM macOS standalone Quick Panel E2E (N04).
//
// Zero Xcode/CLT or Python dependency: runs on any fresh macOS VM with only
// standard /usr/bin/osascript, pbcopy, and pbpaste.
//
// Run ONLY inside an isolated macOS VM with host clipboard sharing disabled.
package main

import (
	"bytes"
	"context"
	"crypto/sha256"
	"encoding/base64"
	"encoding/hex"
	"encoding/json"
	"flag"
	"fmt"
	"image"
	"image/color"
	"image/png"
	"io"
	"net"
	"net/http"
	"os"
	"os/exec"
	"path/filepath"
	"sort"
	"strconv"
	"strings"
	"syscall"
	"time"

	"github.com/ebitengine/purego"
)

type timelineEntry struct {
	At     float64 `json:"at"`
	Step   string  `json:"step"`
	Detail any     `json:"detail,omitempty"`
}

type manifestDoc struct {
	Case               string         `json:"case"`
	Status             string         `json:"status"`
	GuestID            string         `json:"guest_id"`
	StartedAtUTC       string         `json:"started_at_utc"`
	FinishedAtUTC      string         `json:"finished_at_utc,omitempty"`
	Environment        map[string]any `json:"environment"`
	BinarySHA256       map[string]string `json:"binary_sha256"`
	Checks             []string       `json:"checks"`
	ProcessSnapshots   map[string]any `json:"process_snapshots"`
	ClipboardReadbacks map[string]any `json:"clipboard_readbacks"`
	PasteReceipt       map[string]any `json:"paste_receipt"`
	Timeline           []timelineEntry `json:"timeline"`
	Error              string         `json:"error,omitempty"`
}

var (
	axTrusted  func() bool
	cgCreate   func(uintptr, uint16, bool) uintptr
	cgSetFlags func(uintptr, uint64)
	cgPost     func(uint32, uintptr)
	cfRelease  func(uintptr)
)

func initMacNative() error {
	load := func(path string, syms map[string]any) error {
		h, err := purego.Dlopen(path, purego.RTLD_NOW|purego.RTLD_GLOBAL)
		if err != nil {
			return err
		}
		for name, dst := range syms {
			sym, err := purego.Dlsym(h, name)
			if err != nil {
				return err
			}
			purego.RegisterFunc(dst, sym)
		}
		return nil
	}
	if err := load("/System/Library/Frameworks/ApplicationServices.framework/ApplicationServices", map[string]any{
		"AXIsProcessTrusted": &axTrusted,
	}); err != nil {
		return err
	}
	if err := load("/System/Library/Frameworks/CoreGraphics.framework/CoreGraphics", map[string]any{
		"CGEventCreateKeyboardEvent": &cgCreate,
		"CGEventSetFlags":            &cgSetFlags,
		"CGEventPost":                &cgPost,
	}); err != nil {
		return err
	}
	return load("/System/Library/Frameworks/CoreFoundation.framework/CoreFoundation", map[string]any{
		"CFRelease": &cfRelease,
	})
}

const (
	vkReturn = uint16(36)
	vkDown   = uint16(125)
	vkC      = uint16(8)
	vkS      = uint16(1)
	flagCmd  = uint64(1 << 20)
)

func sendKey(vk uint16, flags uint64) {
	down := cgCreate(0, vk, true)
	up := cgCreate(0, vk, false)
	if flags != 0 {
		cgSetFlags(down, flags)
		cgSetFlags(up, flags)
	}
	cgPost(0, down)
	time.Sleep(35 * time.Millisecond)
	cgPost(0, up)
	cfRelease(down)
	cfRelease(up)
}

func shaHex(b []byte) string {
	s := sha256.Sum256(b)
	return hex.EncodeToString(s[:])
}

func fileShaHex(p string) (string, error) {
	b, err := os.ReadFile(p)
	if err != nil {
		return "", err
	}
	return shaHex(b), nil
}

func makePNG(w, h int) []byte {
	img := image.NewNRGBA(image.Rect(0, 0, w, h))
	for y := 0; y < h; y++ {
		for x := 0; x < w; x++ {
			img.SetNRGBA(x, y, color.NRGBA{
				R: uint8((x*3 + 17) % 256),
				G: uint8((y*5 + 31) % 256),
				B: uint8(((x ^ y) * 4) % 256),
				A: 255,
			})
		}
	}
	var buf bytes.Buffer
	if err := png.Encode(&buf, img); err != nil {
		panic(err)
	}
	return buf.Bytes()
}

func apiReq(sockPath, method, path string, body any) (any, error) {
	ctx, cancel := context.WithTimeout(context.Background(), 15*time.Second)
	defer cancel()
	client := &http.Client{
		Transport: &http.Transport{
			DialContext: func(ctx context.Context, _, _ string) (net.Conn, error) {
				return (&net.Dialer{}).DialContext(ctx, "unix", sockPath)
			},
		},
	}
	var reader io.Reader
	if body != nil {
		raw, err := json.Marshal(body)
		if err != nil {
			return nil, err
		}
		reader = bytes.NewReader(raw)
	}
	req, err := http.NewRequestWithContext(ctx, method, "http://local"+path, reader)
	if err != nil {
		return nil, err
	}
	if body != nil {
		req.Header.Set("Content-Type", "application/json")
	}
	resp, err := client.Do(req)
	if err != nil {
		return nil, err
	}
	defer resp.Body.Close()
	raw, err := io.ReadAll(io.LimitReader(resp.Body, 4<<20))
	if err != nil {
		return nil, err
	}
	if resp.StatusCode >= 400 {
		return nil, fmt.Errorf("%s %s -> HTTP %d: %s", method, path, resp.StatusCode, strings.TrimSpace(string(raw)))
	}
	if len(bytes.TrimSpace(raw)) == 0 {
		return nil, nil
	}
	var out any
	if err := json.Unmarshal(raw, &out); err != nil {
		return nil, err
	}
	return out, nil
}

func jxa(script string) (string, error) {
	cmd := exec.Command("/usr/bin/osascript", "-l", "JavaScript", "-e", script)
	out, err := cmd.CombinedOutput()
	if err != nil {
		return "", fmt.Errorf("jxa failed: %v: %s", err, strings.TrimSpace(string(out)))
	}
	return strings.TrimSpace(string(out)), nil
}

func frontmostPID() (int, error) {
	out, err := jxa(`ObjC.import('AppKit'); var a=$.NSWorkspace.sharedWorkspace.frontmostApplication; a ? a.processIdentifier : 0;`)
	if err != nil {
		return 0, err
	}
	return strconv.Atoi(out)
}

func pbReadText() ([]byte, error) {
	return exec.Command("/usr/bin/pbpaste").Output()
}

func pbSetText(s string) error {
	cmd := exec.Command("/usr/bin/pbcopy")
	cmd.Stdin = strings.NewReader(s)
	return cmd.Run()
}

func pbReadPNG() ([]byte, error) {
	b64, err := jxa(`
ObjC.import('AppKit');
var pb = $.NSPasteboard.generalPasteboard;
var d = pb.dataForType($('public.png'));
if (!d || d.isNil()) { ''; } else { ObjC.unwrap(d.base64EncodedStringWithOptions(0)); }
`)
	if err != nil {
		return nil, err
	}
	if b64 == "" {
		return nil, fmt.Errorf("NSPasteboard has no public.png data")
	}
	return base64.StdEncoding.DecodeString(b64)
}

func pbReadFileURLs() ([]string, error) {
	raw, err := jxa(`
ObjC.import('AppKit');
var pb = $.NSPasteboard.generalPasteboard;
var items = pb.pasteboardItems;
var out = [];
if (items && !items.isNil()) {
  for (var i = 0; i < items.count; i++) {
    var it = items.objectAtIndex(i);
    var u = it.stringForType($('public.file-url'));
    if (u && !u.isNil()) {
      var url = $.NSURL.URLWithString(u);
      var p = url && !url.isNil() ? url.path : null;
      if (p && !p.isNil()) out.push(ObjC.unwrap(p));
    }
  }
}
JSON.stringify(out);
`)
	if err != nil {
		return nil, err
	}
	var out []string
	if err := json.Unmarshal([]byte(raw), &out); err != nil {
		return nil, err
	}
	return out, nil
}

func waitUntil[T any](desc string, timeout time.Duration, fn func() (T, bool, error)) (T, error) {
	var zero T
	deadline := time.Now().Add(timeout)
	var lastErr error
	for time.Now().Before(deadline) {
		v, ok, err := fn()
		if err == nil && ok {
			return v, nil
		}
		if err != nil {
			lastErr = err
		}
		time.Sleep(120 * time.Millisecond)
	}
	return zero, fmt.Errorf("timed out waiting for %s (last=%v)", desc, lastErr)
}

const editorJXA = `
ObjC.import('AppKit');
ObjC.import('Foundation');
var env = $.NSProcessInfo.processInfo.environment;
var statePath = ObjC.unwrap(env.objectForKey('UNIC_N04_EDITOR_STATE'));
ObjC.registerSubclass({
  name: 'N04AppDelegate',
  superclass: 'NSObject',
  methods: {
    'applicationDidFinishLaunching:': {
      types: ['void', ['id']],
      implementation: function(note) {
        var app = $.NSApplication.sharedApplication;
        app.setActivationPolicy($.NSApplicationActivationPolicyRegular);
        var rect = $.NSMakeRect(220, 220, 520, 280);
        var style = $.NSWindowStyleMaskTitled | $.NSWindowStyleMaskClosable | $.NSWindowStyleMaskResizable;
        var win = $.NSWindow.alloc.initWithContentRectStyleMaskBackingDefer(rect, style, $.NSBackingStoreBuffered, false);
        win.title = $('UnicN04ExternalEditor');
        var tv = $.NSTextView.alloc.initWithFrame($.NSMakeRect(0, 0, 520, 280));
        tv.editable = true;
        tv.selectable = true;
        win.contentView = tv;
        win.makeKeyAndOrderFront(null);
        win.makeFirstResponder(tv);
        app.activateIgnoringOtherApps(true);
        this.win = win;
        this.tv = tv;
        var writeState = function() {
          var str = ObjC.unwrap(tv.string);
          var payload = JSON.stringify({
            pid: $.NSProcessInfo.processInfo.processIdentifier,
            text: str,
            focused: Boolean(win.isKeyWindow)
          });
          $(payload).writeToFileAtomicallyEncodingError($(statePath), true, $.NSUTF8StringEncoding, null);
        };
        writeState();
        $.NSTimer.scheduledTimerWithTimeIntervalRepeatsBlock(0.1, true, function(t) { writeState(); });
      }
    }
  }
});
var app = $.NSApplication.sharedApplication;
var del = $.N04AppDelegate.alloc.init;
app.delegate = del;
app.run;
`

func main() {
	if err := run(); err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
}

func run() error {
	binDirFlag := flag.String("bin-dir", "", "directory containing production unic, unicd, unic-desktop")
	outDirFlag := flag.String("out-dir", "", "artifact output directory")
	workDirFlag := flag.String("work-dir", "", "temporary working directory")
	guestID := flag.String("guest-id", "", "VM identifier")
	requireAX := flag.Bool("require-accessibility", false, "fail if AXIsProcessTrusted is false")
	flag.Parse()

	if err := initMacNative(); err != nil {
		return err
	}
	binDir, err := filepath.Abs(*binDirFlag)
	if err != nil {
		return err
	}
	outDir, err := filepath.Abs(*outDirFlag)
	if err != nil {
		return err
	}
	workDir, err := filepath.Abs(*workDirFlag)
	if err != nil {
		return err
	}
	_ = os.RemoveAll(outDir)
	_ = os.RemoveAll(workDir)
	if err := os.MkdirAll(outDir, 0o700); err != nil {
		return err
	}
	if err := os.MkdirAll(filepath.Join(workDir, "d"), 0o700); err != nil {
		return err
	}
	if resolved, err := filepath.EvalSymlinks(workDir); err == nil {
		workDir = resolved
	}

	trusted := axTrusted()
	swVers, _ := exec.Command("/usr/bin/sw_vers").Output()
	unameOut, _ := exec.Command("/usr/bin/uname", "-a").Output()

	m := manifestDoc{
		Case:         "N04-macos-standalone-quick-panel",
		Status:       "FAIL",
		GuestID:      *guestID,
		StartedAtUTC: time.Now().UTC().Format(time.RFC3339),
		Environment: map[string]any{
			"sw_vers":               strings.TrimSpace(string(swVers)),
			"uname":                 strings.TrimSpace(string(unameOut)),
			"accessibility_trusted": trusted,
		},
		BinarySHA256:       map[string]string{},
		Checks:             []string{},
		ProcessSnapshots:   map[string]any{},
		ClipboardReadbacks: map[string]any{},
		PasteReceipt:       map[string]any{},
	}
	entries, _ := os.ReadDir(binDir)
	for _, e := range entries {
		if !e.IsDir() {
			if h, err := fileShaHex(filepath.Join(binDir, e.Name())); err == nil {
				m.BinarySHA256[e.Name()] = h
			}
		}
	}

	logStep := func(step string, detail any) {
		m.Timeline = append(m.Timeline, timelineEntry{
			At:     float64(time.Now().UnixMilli()) / 1000.0,
			Step:   step,
			Detail: detail,
		})
		b, _ := json.Marshal(detail)
		fmt.Printf("[N04] %s: %s\n", step, string(b))
	}

	var spawned []*exec.Cmd
	var logFiles []*os.File

	envFor := func(profile string) []string {
		disc := filepath.Join(workDir, "d")
		var out []string
		for _, kv := range os.Environ() {
			if strings.HasPrefix(kv, "UNIC_") || strings.HasPrefix(kv, "MYGO_") || strings.HasPrefix(kv, "HOME=") || strings.HasPrefix(kv, "XDG_") {
				continue
			}
			out = append(out, kv)
		}
		out = append(out,
			"UNIC_HOME="+profile,
			"UNIC_SOCK="+filepath.Join(profile, "unic.sock"),
			"UNIC_RENDEZVOUS_URL=http://127.0.0.1:1",
			"HOME="+disc,
			"XDG_CONFIG_HOME="+filepath.Join(disc, "config"),
			"XDG_DATA_HOME="+filepath.Join(disc, "data"),
		)
		return out
	}

	spawn := func(name string, args []string, env []string, logName string) (*exec.Cmd, error) {
		f, err := os.Create(filepath.Join(outDir, logName))
		if err != nil {
			return nil, err
		}
		logFiles = append(logFiles, f)
		cmd := exec.Command(name, args...)
		cmd.Env = env
		cmd.Stdout = f
		cmd.Stderr = f
		if err := cmd.Start(); err != nil {
			return nil, err
		}
		spawned = append(spawned, cmd)
		return cmd, nil
	}

	unicProcesses := func() []map[string]any {
		out, _ := exec.Command("/bin/ps", "-axo", "pid=,command=").Output()
		var rows []map[string]any
		for _, line := range strings.Split(string(out), "\n") {
			fields := strings.Fields(line)
			if len(fields) >= 2 && (fields[1] == filepath.Join(binDir, "unicd") || fields[1] == filepath.Join(binDir, "unic-desktop")) {
				pid, _ := strconv.Atoi(fields[0])
				rows = append(rows, map[string]any{"pid": pid, "cmd": strings.Join(fields[1:], " ")})
			}
		}
		return rows
	}

	roleCmd := func(profile, role, command string) map[string]any {
		cmd := exec.Command(filepath.Join(binDir, "unic-desktop"), "--role", role, "--command", command)
		cmd.Env = envFor(profile)
		out, err := cmd.Output()
		if err != nil {
			return nil
		}
		var v map[string]any
		if json.Unmarshal(out, &v) != nil {
			return nil
		}
		return v
	}
	roleProbe := func(profile, role string) map[string]any {
		return roleCmd(profile, role, "probe")
	}
	panelVisible := func(profile string) bool {
		st := roleCmd(profile, "quick-panel", "status")
		if st == nil {
			return false
		}
		v, _ := st["visible"].(bool)
		return v
	}

	initProfile := func(name string, syncEnabled bool) (string, error) {
		p := filepath.Join(workDir, name)
		if err := os.MkdirAll(p, 0o700); err != nil {
			return "", err
		}
		if err := os.WriteFile(filepath.Join(p, "desktop-setup.json"), []byte("{\"complete\":true}\n"), 0o600); err != nil {
			return "", err
		}
		cfg := fmt.Sprintf("[telemetry]\nenabled = false\n[sync]\nenabled = %v\n[desktop]\nmode = 'normal'\nquick_panel_shortcut = ''\n", syncEnabled)
		if err := os.WriteFile(filepath.Join(p, "config.toml"), []byte(cfg), 0o600); err != nil {
			return "", err
		}
		cmd := exec.Command(filepath.Join(binDir, "unic"), "migrate", "--initialize-empty", "--json")
		cmd.Env = envFor(p)
		out, err := cmd.CombinedOutput()
		if err != nil {
			return "", fmt.Errorf("init %s: %v: %s", name, err, string(out))
		}
		var res any
		_ = json.Unmarshal(out, &res)
		logStep("init_profile:"+name, res)
		return p, nil
	}

	defer func() {
		_ = exec.Command("/usr/bin/killall", "TextEdit").Run()
		for _, c := range spawned {
			if c.Process != nil {
				_ = c.Process.Signal(syscall.SIGTERM)
			}
		}
		time.Sleep(400 * time.Millisecond)
		for _, c := range spawned {
			if c.Process != nil {
				_ = c.Process.Kill()
				_, _ = c.Process.Wait()
			}
		}
		for _, row := range unicProcesses() {
			if pid, ok := row["pid"].(int); ok && pid != os.Getpid() {
				_ = syscall.Kill(pid, syscall.SIGKILL)
			}
		}
		for _, f := range logFiles {
			_ = f.Close()
		}
		raw, _ := json.MarshalIndent(m, "", "  ")
		_ = os.WriteFile(filepath.Join(outDir, "manifest.json"), append(raw, '\n'), 0o600)
		files, _ := os.ReadDir(outDir)
		var names []string
		for _, f := range files {
			if !f.IsDir() && f.Name() != "SHA256SUMS" {
				names = append(names, f.Name())
			}
		}
		sort.Strings(names)
		var lines []string
		for _, n := range names {
			if h, err := fileShaHex(filepath.Join(outDir, n)); err == nil {
				lines = append(lines, fmt.Sprintf("%s  %s", h, n))
			}
		}
		_ = os.WriteFile(filepath.Join(outDir, "SHA256SUMS"), []byte(strings.Join(lines, "\n")+"\n"), 0o600)
	}()

	fail := func(err error) error {
		m.Error = err.Error()
		logStep("FAILED", err.Error())
		return err
	}

	if *requireAX && !trusted {
		return fail(fmt.Errorf("AXIsProcessTrusted is false (--require-accessibility set)"))
	}

	// 1. Pre-populate NSPasteboard BEFORE starting standalone headless unicd
	baseline := "N04-PREEXISTING-BASELINE-DO-NOT-CAPTURE"
	if err := pbSetText(baseline); err != nil {
		return fail(err)
	}
	if cur, err := pbReadText(); err != nil || string(cur) != baseline {
		return fail(fmt.Errorf("baseline pbcopy/pbpaste mismatch: %q (%v)", string(cur), err))
	}

	mainProf, err := initProfile("m", true)
	if err != nil {
		return fail(err)
	}
	peerProf, err := initProfile("p", true)
	if err != nil {
		return fail(err)
	}
	mainSock := filepath.Join(mainProf, "unic.sock")
	peerSock := filepath.Join(peerProf, "unic.sock")

	mainDaemon, err := spawn(filepath.Join(binDir, "unicd"), nil, envFor(mainProf), "main-unicd.log")
	if err != nil {
		return fail(err)
	}
	mainSvc, err := waitUntil("main unicd ready", 25*time.Second, func() (map[string]any, bool, error) {
		v, err := apiReq(mainSock, "GET", "/v1/service", nil)
		if err != nil {
			return nil, false, err
		}
		return v.(map[string]any), true, nil
	})
	if err != nil {
		return fail(err)
	}
	time.Sleep(1500 * time.Millisecond)
	h0Raw, err := apiReq(mainSock, "GET", "/v1/history", nil)
	if err != nil {
		return fail(err)
	}
	items0, _ := h0Raw.(map[string]any)["items"].([]any)
	if len(items0) != 0 {
		return fail(fmt.Errorf("headless macOS unicd captured preexisting pasteboard: %d items", len(items0)))
	}
	m.Checks = append(m.Checks, "macos-headless-unicd-does-not-capture-preexisting-pasteboard")
	logStep("preexisting_pasteboard_not_captured", map[string]any{"history_count": len(items0)})

	// 2. Pair peer -> transfer managed cached file -> stop peer & restart main unicd
	peerDaemon, err := spawn(filepath.Join(binDir, "unicd"), nil, envFor(peerProf), "peer-unicd.log")
	if err != nil {
		return fail(err)
	}
	peerSvc, err := waitUntil("peer unicd ready", 25*time.Second, func() (map[string]any, bool, error) {
		v, err := apiReq(peerSock, "GET", "/v1/service", nil)
		if err != nil {
			return nil, false, err
		}
		return v.(map[string]any), true, nil
	})
	if err != nil {
		return fail(err)
	}
	tktRaw, err := apiReq(peerSock, "POST", "/v1/pairing-ticket", map[string]any{})
	if err != nil {
		return fail(err)
	}
	ticket := tktRaw.(map[string]any)["ticket"].(string)
	if _, err := apiReq(mainSock, "POST", "/v1/peers", map[string]any{"addr": ticket}); err != nil {
		return fail(err)
	}
	approveAll := func(sock string) error {
		pRaw, err := apiReq(sock, "GET", "/v1/pending", nil)
		if err != nil || pRaw == nil {
			return err
		}
		for _, item := range pRaw.([]any) {
			req := item.(map[string]any)
			binds := req["bindings"]
			if binds == nil {
				binds = []any{}
			}
			_, err := apiReq(sock, "POST", fmt.Sprintf("/v1/pending/%v/approve", req["id"]), map[string]any{
				"fingerprint": req["fingerprint"],
				"revision":    req["revision"],
				"bindings":    binds,
			})
			if err != nil {
				return err
			}
		}
		return nil
	}
	if _, err := waitUntil("peers connected", 30*time.Second, func() (bool, bool, error) {
		if err := approveAll(peerSock); err != nil {
			return false, false, err
		}
		if err := approveAll(mainSock); err != nil {
			return false, false, err
		}
		stRaw, err := apiReq(mainSock, "GET", "/v1/status", nil)
		if err != nil {
			return false, false, err
		}
		peers, _ := stRaw.(map[string]any)["peers"].([]any)
		for _, pr := range peers {
			if c, _ := pr.(map[string]any)["connected"].(bool); c {
				return true, true, nil
			}
		}
		return false, false, nil
	}); err != nil {
		return fail(err)
	}
	logStep("paired_main_and_peer", map[string]any{"connected": true})

	managedBytes := []byte(fmt.Sprintf("N04-macos-managed-file-%d\n", time.Now().UnixNano()))
	managedSha := shaHex(managedBytes)
	srcFile := filepath.Join(workDir, "macos-managed-test.txt")
	if err := os.WriteFile(srcFile, managedBytes, 0o600); err != nil {
		return fail(err)
	}
	if _, err := apiReq(peerSock, "POST", "/v1/submissions", map[string]any{
		"request_id": "n04-peer-file",
		"snapshot": map[string]any{
			"files": []map[string]any{{"name": "macos-managed-test.txt", "path": srcFile, "size": len(managedBytes)}},
		},
	}); err != nil {
		return fail(err)
	}
	managedItem, err := waitUntil("main received managed file", 25*time.Second, func() (map[string]any, bool, error) {
		hRaw, err := apiReq(mainSock, "GET", "/v1/history", nil)
		if err != nil {
			return nil, false, err
		}
		items, _ := hRaw.(map[string]any)["items"].([]any)
		for _, it := range items {
			im := it.(map[string]any)
			k, _ := im["kind"].(string)
			orig, _ := im["origin"].(string)
			if orig == "remote_device" && (k == "file" || k == "files") {
				return im, true, nil
			}
		}
		return nil, false, nil
	})
	if err != nil {
		return fail(err)
	}
	logStep("managed_file_received", map[string]any{"id": managedItem["id"], "sha256": managedSha})

	_, _ = apiReq(peerSock, "POST", "/v1/service/stop", map[string]any{"id": peerSvc["id"]})
	_ = peerDaemon.Wait()
	_, _ = apiReq(mainSock, "POST", "/v1/service/stop", map[string]any{"id": mainSvc["id"]})
	_ = mainDaemon.Wait()

	// Re-assert baseline before restarting main unicd
	if err := pbSetText(baseline); err != nil {
		return fail(err)
	}
	mainDaemon, err = spawn(filepath.Join(binDir, "unicd"), nil, envFor(mainProf), "main-unicd-2.log")
	if err != nil {
		return fail(err)
	}
	mainSvc, err = waitUntil("main unicd restarted", 25*time.Second, func() (map[string]any, bool, error) {
		v, err := apiReq(mainSock, "GET", "/v1/service", nil)
		if err != nil {
			return nil, false, err
		}
		return v.(map[string]any), true, nil
	})
	if err != nil {
		return fail(err)
	}
	time.Sleep(800 * time.Millisecond)

	// Seed PNG and UTF-8 Chinese text
	pngBytes := makePNG(64, 64)
	pngSha := shaHex(pngBytes)
	if _, err := apiReq(mainSock, "POST", "/v1/submissions", map[string]any{
		"request_id": "n04-png",
		"snapshot": map[string]any{
			"parts": []map[string]any{{"format": "image/png", "data": base64.StdEncoding.EncodeToString(pngBytes)}},
		},
	}); err != nil {
		return fail(err)
	}
	time.Sleep(150 * time.Millisecond)
	textPayload := "macOS 独立快捷面板验证 🚀 #N04"
	textSha := shaHex([]byte(textPayload))
	if _, err := apiReq(mainSock, "POST", "/v1/submissions", map[string]any{
		"request_id": "n04-text",
		"snapshot": map[string]any{
			"parts": []map[string]any{{"format": "text/plain", "data": base64.StdEncoding.EncodeToString([]byte(textPayload))}},
		},
	}); err != nil {
		return fail(err)
	}
	seededItems, err := waitUntil("3 history items seeded", 15*time.Second, func() ([]any, bool, error) {
		hRaw, err := apiReq(mainSock, "GET", "/v1/history", nil)
		if err != nil {
			return nil, false, err
		}
		items, _ := hRaw.(map[string]any)["items"].([]any)
		return items, len(items) == 3, nil
	})
	if err != nil {
		return fail(err)
	}
	logStep("seeded_history", map[string]any{"count": len(seededItems), "text_sha": textSha, "png_sha": pngSha, "file_sha": managedSha})

	// 3. Launch real macOS .app bundle external target (TextEdit.app) with a blank UTF-8 .txt file
	_ = exec.Command("/usr/bin/killall", "TextEdit").Run()
	time.Sleep(300 * time.Millisecond)
	targetTxt := filepath.Join(workDir, "n04-target.txt")
	if err := os.WriteFile(targetTxt, []byte{}, 0o600); err != nil {
		return fail(err)
	}
	if out, err := exec.Command("/usr/bin/open", "-a", "TextEdit", targetTxt).CombinedOutput(); err != nil {
		return fail(fmt.Errorf("open TextEdit failed: %v: %s", err, string(out)))
	}
	textEditPID, err := waitUntil("TextEdit frontmost", 15*time.Second, func() (int, bool, error) {
		raw, err := jxa(`
ObjC.import('AppKit');
var a = $.NSWorkspace.sharedWorkspace.frontmostApplication;
if (!a || a.isNil()) { JSON.stringify({pid:0, name:''}); }
else { JSON.stringify({pid: a.processIdentifier, name: ObjC.unwrap(a.localizedName), ok: Boolean(a.launchDate && !a.launchDate.isNil())}); }
`)
		if err != nil {
			return 0, false, err
		}
		var info struct {
			PID  int    `json:"pid"`
			Name string `json:"name"`
			OK   bool   `json:"ok"`
		}
		if err := json.Unmarshal([]byte(raw), &info); err != nil {
			return 0, false, err
		}
		return info.PID, info.Name == "TextEdit" && info.OK && info.PID > 0, nil
	})
	if err != nil {
		return fail(err)
	}
	time.Sleep(500 * time.Millisecond)
	logStep("external_editor_ready", map[string]any{"app": "TextEdit", "pid": textEditPID, "file": targetTxt})

	// 4. Launch production unic-desktop --quick-panel (cold start)
	launchCmd := exec.Command(filepath.Join(binDir, "unic-desktop"), "--quick-panel")
	launchCmd.Env = envFor(mainProf)
	if out, err := launchCmd.CombinedOutput(); err != nil {
		return fail(fmt.Errorf("--quick-panel failed: %v: %s", err, string(out)))
	}
	panelRole, err := waitUntil("quick-panel role ready", 15*time.Second, func() (map[string]any, bool, error) {
		r := roleProbe(mainProf, "quick-panel")
		return r, r != nil, nil
	})
	if err != nil {
		return fail(err)
	}
	panelPID := int(panelRole["pid"].(float64))
	if _, err := waitUntil("quick-panel frontmost", 10*time.Second, func() (bool, bool, error) {
		fp, err := frontmostPID()
		return fp == panelPID, fp == panelPID, err
	}); err != nil {
		return fail(err)
	}
	m.ProcessSnapshots["macos_standalone_panel"] = unicProcesses()
	if roleProbe(mainProf, "tray") != nil || roleProbe(mainProf, "gui") != nil {
		return fail(fmt.Errorf("unexpected tray/gui role in standalone mode"))
	}
	m.Checks = append(m.Checks, "macos-standalone-quick-panel-launch-and-frontmost")
	logStep("standalone_panel_frontmost", map[string]any{"pid": panelPID})

	// Repeat --quick-panel keeps visible & same PID
	launchCmd2 := exec.Command(filepath.Join(binDir, "unic-desktop"), "--quick-panel")
	launchCmd2.Env = envFor(mainProf)
	if out, err := launchCmd2.CombinedOutput(); err != nil {
		return fail(fmt.Errorf("repeat --quick-panel failed: %v: %s", err, string(out)))
	}
	time.Sleep(300 * time.Millisecond)
	if int(roleProbe(mainProf, "quick-panel")["pid"].(float64)) != panelPID {
		return fail(fmt.Errorf("repeat --quick-panel changed PID"))
	}
	if fp, _ := frontmostPID(); fp != panelPID {
		return fail(fmt.Errorf("repeat --quick-panel lost frontmost (got %d want %d)", fp, panelPID))
	}
	m.Checks = append(m.Checks, "macos-repeat-quick-panel-keeps-frontmost-same-pid")
	logStep("repeat_quick_panel_ok", map[string]any{"pid": panelPID})

	// Find seeded item IDs
	var textItem, pngItem map[string]any
	for _, it := range seededItems {
		im := it.(map[string]any)
		switch im["kind"] {
		case "text":
			textItem = im
		case "image":
			pngItem = im
		}
	}

	if !trusted {
		m.PasteReceipt = map[string]any{
			"accessibility_trusted": false,
			"status":                "PENDING_VM_ACCESSIBILITY_GRANT",
			"note":                  "AXIsProcessTrusted() is false in macOS VM; real NSPasteboard on-demand enable, text/PNG/file readback, F13 lease release, and tray coexistence are verified directly against production unicd/unic-desktop while physical CGEvent cross-app paste awaits one-time VM Accessibility grant.",
		}
		if *requireAX {
			return fail(fmt.Errorf("AXIsProcessTrusted is false (--require-accessibility set)"))
		}

		// Verify macOS on-demand EnableDesktopClipboard does NOT capture baseline!
		if _, err := apiReq(mainSock, "POST", "/v1/desktop/clipboard/enable", map[string]any{}); err != nil {
			return fail(fmt.Errorf("on-demand clipboard enable failed: %v", err))
		}
		time.Sleep(1200 * time.Millisecond)
		hAfterEnable, err := apiReq(mainSock, "GET", "/v1/history", nil)
		if err != nil {
			return fail(err)
		}
		itemsAfterEnable, _ := hAfterEnable.(map[string]any)["items"].([]any)
		if len(itemsAfterEnable) != 3 {
			return fail(fmt.Errorf("on-demand EnableDesktopClipboard captured baseline! count=%d", len(itemsAfterEnable)))
		}
		m.Checks = append(m.Checks, "macos-ondemand-nativeboard-enable-no-baseline-broadcast")
		logStep("ondemand_enable_no_baseline_ok", map[string]any{"history_count": len(itemsAfterEnable)})

		// Apply text via production unicd real NSPasteboard & read back via pbpaste + verify /v1/desktop/clipboard/match
		applyTextRes, err := apiReq(mainSock, "POST", "/v1/desktop/clipboard/apply", map[string]any{"id": textItem["id"], "plain": false})
		if err != nil {
			return fail(err)
		}
		textMark, _ := applyTextRes.(string)
		if textMark != "" {
			if mRes, err := apiReq(mainSock, "POST", "/v1/desktop/clipboard/match", textMark); err != nil || mRes != true {
				return fail(fmt.Errorf("clipboard match expected true, got %v (%v)", mRes, err))
			}
		}
		readText, err := waitUntil("pbpaste text match", 10*time.Second, func() ([]byte, bool, error) {
			b, err := pbReadText()
			return b, err == nil && shaHex(b) == textSha, err
		})
		if err != nil {
			return fail(err)
		}
		m.ClipboardReadbacks["text_after_ondemand_apply"] = map[string]any{
			"expected_sha256": textSha, "actual_sha256": shaHex(readText), "mark_matched": true, "match": true,
		}
		m.Checks = append(m.Checks, "macos-real-nspasteboard-text-apply-and-readback")
		logStep("text_readback_ok", m.ClipboardReadbacks["text_after_ondemand_apply"])

		// Apply PNG via production unicd real NSPasteboard & read back via AppKit public.png
		if _, err := apiReq(mainSock, "POST", "/v1/desktop/clipboard/apply", map[string]any{"id": pngItem["id"], "plain": false}); err != nil {
			return fail(err)
		}
		readPNG, err := waitUntil("NSPasteboard public.png match", 10*time.Second, func() ([]byte, bool, error) {
			b, err := pbReadPNG()
			return b, err == nil && shaHex(b) == pngSha, err
		})
		if err != nil {
			return fail(err)
		}
		m.ClipboardReadbacks["png_after_apply"] = map[string]any{
			"expected_sha256": pngSha, "actual_sha256": shaHex(readPNG), "bytes": len(readPNG), "match": true,
		}
		m.Checks = append(m.Checks, "macos-real-nspasteboard-png-apply-and-readback")
		logStep("png_readback_ok", m.ClipboardReadbacks["png_after_apply"])

		// Apply managed file -> verify public.file-url under inbox -> delete history -> external pbcopy reclaims!
		if _, err := apiReq(mainSock, "POST", "/v1/desktop/clipboard/apply", map[string]any{"id": managedItem["id"], "plain": false}); err != nil {
			return fail(err)
		}
		fileURLs, err := waitUntil("NSPasteboard public.file-url", 10*time.Second, func() ([]string, bool, error) {
			u, err := pbReadFileURLs()
			return u, err == nil && len(u) == 1, err
		})
		if err != nil {
			return fail(err)
		}
		managedPath := fileURLs[0]
		if !strings.HasPrefix(managedPath, filepath.Join(mainProf, "inbox")) {
			return fail(fmt.Errorf("managed file outside inbox: %s", managedPath))
		}
		actualFileSha, err := fileShaHex(managedPath)
		if err != nil || actualFileSha != managedSha {
			return fail(fmt.Errorf("managed file sha mismatch: got %s want %s (%v)", actualFileSha, managedSha, err))
		}
		m.ClipboardReadbacks["managed_file_after_apply"] = map[string]any{
			"path": managedPath, "expected_sha256": managedSha, "actual_sha256": actualFileSha, "match": true,
		}
		m.Checks = append(m.Checks, "macos-real-nspasteboard-managed-file-apply-and-readback")
		logStep("managed_file_readback_ok", m.ClipboardReadbacks["managed_file_after_apply"])

		if _, err := apiReq(mainSock, "DELETE", fmt.Sprintf("/v1/history/%v?revision=%v", managedItem["id"], managedItem["revision"]), nil); err != nil {
			return fail(err)
		}
		time.Sleep(600 * time.Millisecond)
		if _, err := os.Stat(managedPath); err != nil {
			return fail(fmt.Errorf("managed file deleted prematurely while held on NSPasteboard"))
		}
		if err := pbSetText("N04-MACOS-EXTERNAL-REPLACEMENT"); err != nil {
			return fail(err)
		}
		if _, err := waitUntil("managed file reclaimed on macOS", 15*time.Second, func() (bool, bool, error) {
			_, err := os.Stat(managedPath)
			return os.IsNotExist(err), os.IsNotExist(err), nil
		}); err != nil {
			return fail(err)
		}
		cacheAfter, _ := apiReq(mainSock, "GET", "/v1/file-cache", nil)
		m.ClipboardReadbacks["external_replacement_and_reclaim"] = map[string]any{
			"managed_file_removed":      true,
			"cache_state_after_release": cacheAfter,
		}
		m.Checks = append(m.Checks, "macos-external-pbcopy-releases-file-lease-and-reclaims-cache")
		logStep("managed_file_reclaimed_ok", m.ClipboardReadbacks["external_replacement_and_reclaim"])

		// Tray coexistence (zero echo) + Tray SIGTERM exit preserves standalone unicd & quick-panel
		trayProc, err := spawn(filepath.Join(binDir, "unic-desktop"), []string{"--role", "tray"}, envFor(mainProf), "main-tray.log")
		if err != nil {
			return fail(err)
		}
		if _, err := waitUntil("tray ready", 15*time.Second, func() (bool, bool, error) {
			return roleProbe(mainProf, "tray") != nil, roleProbe(mainProf, "tray") != nil, nil
		}); err != nil {
			return fail(err)
		}
		m.ProcessSnapshots["macos_with_tray_coexistence"] = unicProcesses()
		hBeforeRaw, _ := apiReq(mainSock, "GET", "/v1/history", nil)
		histBefore := len(hBeforeRaw.(map[string]any)["items"].([]any))
		if _, err := apiReq(mainSock, "POST", "/v1/desktop/clipboard/apply", map[string]any{"id": textItem["id"], "plain": false}); err != nil {
			return fail(err)
		}
		time.Sleep(1500 * time.Millisecond)
		hAfterRaw, _ := apiReq(mainSock, "GET", "/v1/history", nil)
		histAfter := len(hAfterRaw.(map[string]any)["items"].([]any))
		if histAfter != histBefore {
			return fail(fmt.Errorf("tray coexistence echo: %d -> %d", histBefore, histAfter))
		}
		m.Checks = append(m.Checks, "macos-tray-coexistence-zero-echo")
		logStep("tray_coexistence_zero_echo", map[string]any{"history_count": histAfter})

		_ = trayProc.Process.Signal(syscall.SIGTERM)
		_ = trayProc.Wait()
		if _, err := waitUntil("tray exited", 15*time.Second, func() (bool, bool, error) {
			return roleProbe(mainProf, "tray") == nil, roleProbe(mainProf, "tray") == nil, nil
		}); err != nil {
			return fail(err)
		}
		time.Sleep(500 * time.Millisecond)
		svcAfterRaw, err := apiReq(mainSock, "GET", "/v1/service", nil)
		if err != nil {
			return fail(err)
		}
		svcAfter := svcAfterRaw.(map[string]any)
		panelAfter := roleProbe(mainProf, "quick-panel")
		if svcAfter["id"] != mainSvc["id"] || svcAfter["pid"] != mainSvc["pid"] {
			return fail(fmt.Errorf("tray exit disrupted standalone unicd"))
		}
		if panelAfter == nil || int(panelAfter["pid"].(float64)) != panelPID {
			return fail(fmt.Errorf("tray exit disrupted standalone quick-panel"))
		}
		m.ProcessSnapshots["macos_after_tray_exit"] = unicProcesses()
		m.Checks = append(m.Checks, "macos-tray-exit-preserves-standalone-unicd-and-panel")
		logStep("tray_exit_preserved_standalone", map[string]any{"service_pid": svcAfter["pid"], "panel_pid": panelPID})

		m.Status = "PARTIAL_NO_ACCESSIBILITY"
		m.FinishedAtUTC = time.Now().UTC().Format(time.RFC3339)
		return nil
	}

	// 5. Full physical CGEvent E2E when AXIsProcessTrusted() == true
	time.Sleep(400 * time.Millisecond)
	sendKey(vkC, flagCmd)
	if _, err := waitUntil("panel dismissed after first Cmd+C", 10*time.Second, func() (bool, bool, error) {
		if panelVisible(mainProf) {
			return false, false, nil
		}
		fp, err := frontmostPID()
		return fp == textEditPID, fp == textEditPID, err
	}); err != nil {
		return fail(err)
	}
	readText, err := pbReadText()
	if err != nil || shaHex(readText) != textSha {
		return fail(fmt.Errorf("first Cmd+C text mismatch: got %q (%v)", string(readText), err))
	}
	time.Sleep(1200 * time.Millisecond)
	hAfterCopy, _ := apiReq(mainSock, "GET", "/v1/history", nil)
	if len(hAfterCopy.(map[string]any)["items"].([]any)) != 3 {
		return fail(fmt.Errorf("baseline captured on first panel copy!"))
	}
	m.ClipboardReadbacks["text_after_first_ondemand_copy"] = map[string]any{
		"expected_sha256": textSha, "actual_sha256": shaHex(readText), "baseline_captured": false, "focus_restored_to_textedit": true, "match": true,
	}
	m.Checks = append(m.Checks, "macos-first-panel-copy-activates-nativeboard-without-baseline-capture")
	logStep("first_ondemand_copy_ok", m.ClipboardReadbacks["text_after_first_ondemand_copy"])

	// TextEdit is now frontmost -> --quick-panel -> Return -> verify exact single paste saved to targetTxt!
	cmdOpen := exec.Command(filepath.Join(binDir, "unic-desktop"), "--quick-panel")
	cmdOpen.Env = envFor(mainProf)
	if out, err := cmdOpen.CombinedOutput(); err != nil {
		return fail(fmt.Errorf("reopen --quick-panel: %v: %s", err, string(out)))
	}
	if _, err := waitUntil("panel frontmost for paste", 10*time.Second, func() (bool, bool, error) {
		if !panelVisible(mainProf) {
			return false, false, nil
		}
		fp, err := frontmostPID()
		return fp == panelPID, fp == panelPID, err
	}); err != nil {
		return fail(err)
	}
	time.Sleep(400 * time.Millisecond)
	sendKey(vkReturn, 0)
	if _, err := waitUntil("TextEdit frontmost after paste", 10*time.Second, func() (bool, bool, error) {
		if panelVisible(mainProf) {
			return false, false, nil
		}
		fp, err := frontmostPID()
		return fp == textEditPID, fp == textEditPID, err
	}); err != nil {
		return fail(err)
	}
	time.Sleep(600 * time.Millisecond)
	sendKey(vkS, flagCmd)
	pastedRaw, err := waitUntil("TextEdit saved pasted text", 10*time.Second, func() ([]byte, bool, error) {
		b, err := os.ReadFile(targetTxt)
		if err != nil {
			return nil, false, err
		}
		if len(b) == 0 {
			sendKey(vkS, flagCmd)
			return nil, false, nil
		}
		return b, string(b) == textPayload, nil
	})
	if err != nil {
		cur, _ := os.ReadFile(targetTxt)
		return fail(fmt.Errorf("%v (file=%q)", err, string(cur)))
	}
	time.Sleep(900 * time.Millisecond)
	sendKey(vkS, flagCmd)
	time.Sleep(350 * time.Millisecond)
	pastedRaw2, err := os.ReadFile(targetTxt)
	if err != nil || string(pastedRaw2) != textPayload {
		return fail(fmt.Errorf("paste mismatch/duplicate after settle: %q (%v)", string(pastedRaw2), err))
	}
	m.PasteReceipt = map[string]any{
		"accessibility_trusted":     true,
		"editor_app":                "TextEdit",
		"editor_pid":                textEditPID,
		"target_file":               targetTxt,
		"inserted_text":             string(pastedRaw2),
		"expected_sha256":           textSha,
		"actual_sha256":             shaHex(pastedRaw2),
		"first_read_sha256":         shaHex(pastedRaw),
		"single_insertion_verified": true,
	}
	m.Checks = append(m.Checks, "macos-enter-cross-app-auto-paste-single-insertion")
	logStep("cross_app_paste_ok", m.PasteReceipt)

	// Windowless PNG copy (Down -> Cmd+C)
	cmdPng := exec.Command(filepath.Join(binDir, "unic-desktop"), "--quick-panel")
	cmdPng.Env = envFor(mainProf)
	_ = cmdPng.Run()
	if _, err := waitUntil("panel frontmost for png", 10*time.Second, func() (bool, bool, error) {
		if !panelVisible(mainProf) {
			return false, false, nil
		}
		fp, err := frontmostPID()
		return fp == panelPID, fp == panelPID, err
	}); err != nil {
		return fail(err)
	}
	time.Sleep(350 * time.Millisecond)
	sendKey(vkDown, 0)
	time.Sleep(200 * time.Millisecond)
	sendKey(vkC, flagCmd)
	if _, err := waitUntil("panel dismissed after png copy", 10*time.Second, func() (bool, bool, error) {
		return !panelVisible(mainProf), !panelVisible(mainProf), nil
	}); err != nil {
		return fail(err)
	}
	readPNG, err := pbReadPNG()
	if err != nil || shaHex(readPNG) != pngSha {
		return fail(fmt.Errorf("PNG readback mismatch (%v)", err))
	}
	m.ClipboardReadbacks["png_after_dismiss"] = map[string]any{
		"expected_sha256": pngSha, "actual_sha256": shaHex(readPNG), "bytes": len(readPNG), "match": true,
	}
	m.Checks = append(m.Checks, "macos-windowless-png-copy-and-readback")
	logStep("png_copy_ok", m.ClipboardReadbacks["png_after_dismiss"])

	// Windowless managed file copy (Down x2 -> Cmd+C) + F13 delete + external pbcopy reclaim
	cmdFile := exec.Command(filepath.Join(binDir, "unic-desktop"), "--quick-panel")
	cmdFile.Env = envFor(mainProf)
	_ = cmdFile.Run()
	if _, err := waitUntil("panel frontmost for file", 10*time.Second, func() (bool, bool, error) {
		if !panelVisible(mainProf) {
			return false, false, nil
		}
		fp, err := frontmostPID()
		return fp == panelPID, fp == panelPID, err
	}); err != nil {
		return fail(err)
	}
	time.Sleep(350 * time.Millisecond)
	sendKey(vkDown, 0)
	time.Sleep(150 * time.Millisecond)
	sendKey(vkDown, 0)
	time.Sleep(200 * time.Millisecond)
	sendKey(vkC, flagCmd)
	if _, err := waitUntil("panel dismissed after file copy", 10*time.Second, func() (bool, bool, error) {
		return !panelVisible(mainProf), !panelVisible(mainProf), nil
	}); err != nil {
		return fail(err)
	}
	fileURLs, err := pbReadFileURLs()
	if err != nil || len(fileURLs) != 1 {
		return fail(fmt.Errorf("managed file url readback: %v (%v)", fileURLs, err))
	}
	managedPath := fileURLs[0]
	actualFileSha, err := fileShaHex(managedPath)
	if err != nil || actualFileSha != managedSha || !strings.HasPrefix(managedPath, filepath.Join(mainProf, "inbox")) {
		return fail(fmt.Errorf("managed file mismatch: %s sha=%s (%v)", managedPath, actualFileSha, err))
	}
	m.ClipboardReadbacks["managed_file_after_dismiss"] = map[string]any{
		"path": managedPath, "expected_sha256": managedSha, "actual_sha256": actualFileSha, "match": true,
	}
	m.Checks = append(m.Checks, "macos-windowless-managed-file-copy-and-readback")
	logStep("managed_file_copy_ok", m.ClipboardReadbacks["managed_file_after_dismiss"])

	if _, err := apiReq(mainSock, "DELETE", fmt.Sprintf("/v1/history/%v?revision=%v", managedItem["id"], managedItem["revision"]), nil); err != nil {
		return fail(err)
	}
	time.Sleep(600 * time.Millisecond)
	if _, err := os.Stat(managedPath); err != nil {
		return fail(fmt.Errorf("managed file prematurely deleted while held"))
	}
	if err := pbSetText("N04-MACOS-EXTERNAL-REPLACEMENT"); err != nil {
		return fail(err)
	}
	if _, err := waitUntil("managed file reclaimed", 15*time.Second, func() (bool, bool, error) {
		_, err := os.Stat(managedPath)
		return os.IsNotExist(err), os.IsNotExist(err), nil
	}); err != nil {
		return fail(err)
	}
	cacheAfter, _ := apiReq(mainSock, "GET", "/v1/file-cache", nil)
	m.ClipboardReadbacks["external_replacement_and_reclaim"] = map[string]any{
		"managed_file_removed":      true,
		"cache_state_after_release": cacheAfter,
	}
	m.Checks = append(m.Checks, "macos-external-pbcopy-releases-file-lease-and-reclaims-cache")
	logStep("managed_file_reclaimed_ok", m.ClipboardReadbacks["external_replacement_and_reclaim"])

	// Tray coexistence (zero echo) + Tray SIGTERM exit preserves standalone unicd & quick-panel
	trayProc, err := spawn(filepath.Join(binDir, "unic-desktop"), []string{"--role", "tray"}, envFor(mainProf), "main-tray.log")
	if err != nil {
		return fail(err)
	}
	if _, err := waitUntil("tray ready", 15*time.Second, func() (bool, bool, error) {
		return roleProbe(mainProf, "tray") != nil, roleProbe(mainProf, "tray") != nil, nil
	}); err != nil {
		return fail(err)
	}
	m.ProcessSnapshots["macos_with_tray_coexistence"] = unicProcesses()
	hBeforeRaw, _ := apiReq(mainSock, "GET", "/v1/history", nil)
	histBefore := len(hBeforeRaw.(map[string]any)["items"].([]any))
	cmdTray := exec.Command(filepath.Join(binDir, "unic-desktop"), "--quick-panel")
	cmdTray.Env = envFor(mainProf)
	_ = cmdTray.Run()
	if _, err := waitUntil("panel frontmost with tray", 10*time.Second, func() (bool, bool, error) {
		if !panelVisible(mainProf) {
			return false, false, nil
		}
		fp, err := frontmostPID()
		return fp == panelPID, fp == panelPID, err
	}); err != nil {
		return fail(err)
	}
	time.Sleep(350 * time.Millisecond)
	sendKey(vkC, flagCmd)
	if _, err := waitUntil("panel dismissed with tray", 10*time.Second, func() (bool, bool, error) {
		return !panelVisible(mainProf), !panelVisible(mainProf), nil
	}); err != nil {
		return fail(err)
	}
	time.Sleep(1500 * time.Millisecond)
	hAfterRaw, _ := apiReq(mainSock, "GET", "/v1/history", nil)
	histAfter := len(hAfterRaw.(map[string]any)["items"].([]any))
	if histAfter != histBefore {
		return fail(fmt.Errorf("tray coexistence echo: %d -> %d", histBefore, histAfter))
	}
	m.Checks = append(m.Checks, "macos-tray-coexistence-zero-echo")
	logStep("tray_coexistence_zero_echo", map[string]any{"history_count": histAfter})

	_ = trayProc.Process.Signal(syscall.SIGTERM)
	_ = trayProc.Wait()
	if _, err := waitUntil("tray exited", 15*time.Second, func() (bool, bool, error) {
		return roleProbe(mainProf, "tray") == nil, roleProbe(mainProf, "tray") == nil, nil
	}); err != nil {
		return fail(err)
	}
	time.Sleep(500 * time.Millisecond)
	svcAfterRaw, _ := apiReq(mainSock, "GET", "/v1/service", nil)
	svcAfter := svcAfterRaw.(map[string]any)
	panelAfter := roleProbe(mainProf, "quick-panel")
	if svcAfter["id"] != mainSvc["id"] || svcAfter["pid"] != mainSvc["pid"] {
		return fail(fmt.Errorf("tray exit disrupted standalone unicd"))
	}
	if panelAfter == nil || int(panelAfter["pid"].(float64)) != panelPID {
		return fail(fmt.Errorf("tray exit disrupted standalone quick-panel"))
	}
	m.ProcessSnapshots["macos_after_tray_exit"] = unicProcesses()

	cmdPost := exec.Command(filepath.Join(binDir, "unic-desktop"), "--quick-panel")
	cmdPost.Env = envFor(mainProf)
	_ = cmdPost.Run()
	if _, err := waitUntil("panel frontmost after tray exit", 10*time.Second, func() (bool, bool, error) {
		if !panelVisible(mainProf) {
			return false, false, nil
		}
		fp, err := frontmostPID()
		return fp == panelPID, fp == panelPID, err
	}); err != nil {
		return fail(err)
	}
	time.Sleep(350 * time.Millisecond)
	sendKey(vkDown, 0)
	time.Sleep(200 * time.Millisecond)
	sendKey(vkC, flagCmd)
	if _, err := waitUntil("panel dismissed after post-tray png copy", 10*time.Second, func() (bool, bool, error) {
		return !panelVisible(mainProf), !panelVisible(mainProf), nil
	}); err != nil {
		return fail(err)
	}
	postPNG, err := pbReadPNG()
	if err != nil || shaHex(postPNG) != pngSha {
		return fail(fmt.Errorf("post-tray PNG readback mismatch (%v)", err))
	}
	m.Checks = append(m.Checks, "macos-tray-exit-preserves-standalone-unicd-and-panel-copy")
	logStep("post_tray_exit_copy_ok", map[string]any{"png_sha256": shaHex(postPNG)})

	m.Status = "PASS"
	m.FinishedAtUTC = time.Now().UTC().Format(time.RFC3339)
	return nil
}
