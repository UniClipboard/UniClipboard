package quickpanel_test

import (
	"bytes"
	"context"
	"crypto/sha256"
	"encoding/base64"
	"encoding/hex"
	"encoding/json"
	"errors"
	"os"
	"os/exec"
	"path/filepath"
	"runtime"
	"strings"
	"sync"
	"testing"
	"time"

	"github.com/BurntSushi/toml"

	"github.com/uniclipboard/unic/app/desktop/internal/quickpanel"
	"github.com/uniclipboard/unic/engine"
	"github.com/uniclipboard/unic/internal/clip"
	"github.com/uniclipboard/unic/internal/clipboardintegration"
	api "github.com/uniclipboard/unic/internal/localapi"
	"github.com/uniclipboard/unic/internal/testboard"
)

// Failure cases precede implementation in desktop-quick-panel-contract.md.
// This uses production unicd and its real IPC adapter. OS target/input is a
// fixture: no native window, system clipboard, hotkey or renderer is started.
func TestQuickPanelProcessE2E(t *testing.T) {
	if os.Getenv("UNIC_QUICK_SYSTEM_E2E") != "1" {
		t.Skip("set UNIC_QUICK_SYSTEM_E2E=1")
	}
	ctx, cancel := context.WithTimeout(context.Background(), 3*time.Minute)
	defer cancel()
	root, err := filepath.Abs("../../../..")
	if err != nil {
		t.Fatal(err)
	}
	private, err := os.MkdirTemp("/private/tmp", "unic-quick-system-")
	if err != nil {
		t.Fatal(err)
	}
	defer func() {
		if err := os.RemoveAll(private); err != nil {
			t.Error(err)
		}
	}()
	hash := func(b []byte) string { s := sha256.Sum256(b); return hex.EncodeToString(s[:]) }
	sources := map[string]string{}
	for _, dir := range []string{"app/desktop", "app/service", "internal/localconfig", "internal/localapi", "internal/clipboardintegration", "engine", "internal/clip", "internal/store", "internal/plugins/builtintext"} {
		err = filepath.WalkDir(filepath.Join(root, dir), func(p string, d os.DirEntry, e error) error {
			if e != nil {
				return e
			}
			if d.IsDir() || !strings.HasSuffix(p, ".go") {
				return nil
			}
			b, e := os.ReadFile(p)
			if e != nil {
				return e
			}
			rel, _ := filepath.Rel(root, p)
			sources[rel] = hash(b)
			return nil
		})
		if err != nil {
			t.Fatal(err)
		}
	}
	for _, name := range []string{"go.mod", "go.sum"} {
		b, e := os.ReadFile(filepath.Join(root, name))
		if e != nil {
			t.Fatal(e)
		}
		sources[name] = hash(b)
	}
	checks := []string{}
	succeeded := false
	receipt := map[string]any{"source_sha256": sources, "native_ui": false, "system_clipboard": false, "native_input": false}
	defer func() {
		receipt["passed"] = succeeded && !t.Failed()
		receipt["checks"] = checks
		for p, want := range sources {
			b, e := os.ReadFile(filepath.Join(root, p))
			if e != nil || hash(b) != want {
				receipt["passed"] = false
				t.Errorf("source changed: %s", p)
			}
		}
		if p := os.Getenv("UNIC_QUICK_SYSTEM_RECEIPT"); p != "" {
			b, _ := json.MarshalIndent(receipt, "", "  ")
			_ = os.MkdirAll(filepath.Dir(p), 0700)
			if e := os.WriteFile(p, b, 0600); e != nil {
				t.Error(e)
			}
		}
	}()
	exe := filepath.Join(private, "unicd")
	build := exec.CommandContext(ctx, "go", "build", "-race", "-o", exe, "./app/service/cmd/unicd")
	build.Dir = root
	if out, e := build.CombinedOutput(); e != nil {
		t.Fatalf("build: %v %s", e, out)
	}
	binary, e := os.ReadFile(exe)
	if e != nil {
		t.Fatal(e)
	}
	receipt["unicd_sha256"] = hash(binary)
	home := filepath.Join(private, "home")
	if e = os.MkdirAll(home, 0700); e != nil {
		t.Fatal(e)
	}
	var logs bytes.Buffer
	cmd := exec.CommandContext(ctx, exe)
	for _, v := range os.Environ() {
		if !strings.HasPrefix(v, "UNIC_") {
			cmd.Env = append(cmd.Env, v)
		}
	}
	cmd.Env = append(cmd.Env, "UNIC_HOME="+home, "UNIC_RENDEZVOUS_URL=http://127.0.0.1:1")
	cmd.Stdout = &logs
	cmd.Stderr = &logs
	if e = cmd.Start(); e != nil {
		t.Fatal(e)
	}
	defer func() { _ = cmd.Process.Kill(); _ = cmd.Wait() }()
	client := api.NewClient(filepath.Join(home, "unic.sock"))
	queue := make(chan func(), 256)
	pump := func() {
		for {
			select {
			case f := <-queue:
				f()
			default:
				return
			}
		}
	}
	wait := func(label string, f func() bool) {
		t.Helper()
		until := time.Now().Add(10 * time.Second)
		for time.Now().Before(until) {
			pump()
			if f() {
				return
			}
			time.Sleep(5 * time.Millisecond)
		}
		t.Fatal("timeout: " + label)
	}
	wait("service", func() bool { _, e := client.Status(ctx); return e == nil })
	submit := func(id string, s clip.Snapshot) string {
		t.Helper()
		out, e := client.Submit(ctx, api.SubmitInput{RequestID: id, Snapshot: s})
		if e != nil {
			t.Fatal(e)
		}
		return string(out.HistoryID)
	}
	rich := clip.Snapshot{Parts: []clip.Part{{Format: clip.Text, Data: []byte("quick-private-body")}, {Format: clip.HTML, Data: []byte("<b>quick-private-body</b>")}}}
	id := submit("rich", rich)
	fileBoard := testboard.Dir(filepath.Join(private, "board"))
	gatedBoard := &fixtureBoard{base: fileBoard}
	board := quickpanel.NewClipboard(gatedBoard)
	native := &fixturePlatform{active: true, ready: true}
	visible := true
	controller := quickpanel.New(quickpanel.Options{Context: ctx, Client: client, Clipboard: board, Platform: native, Dispatch: func(f func()) { queue <- f }, Visible: func(v bool) { visible = v; native.events = append(native.events, "visible") }})
	open := func() { controller.Open(); visible = true }
	open()
	if !controller.CanPaste || controller.TargetName != "Fixture editor" {
		t.Fatal("target capture failed")
	}
	if controller.Start(id, false, true, false) != nil {
		t.Fatal("start should be accepted; missing adapter fails asynchronously")
	}
	wait("missing adapter", func() bool { return !controller.Busy })
	if controller.Sent || !visible || controller.Message == "" {
		t.Fatal("missing adapter falsely pasted")
	}
	adapterCtx, stopAdapter := context.WithCancel(ctx)
	defer stopAdapter()
	done := make(chan error, 1)
	go func() { done <- clipboardintegration.RunRemote(adapterCtx, client, board, nil) }()
	defer func() {
		stopAdapter()
		if err := <-done; err != nil && !errors.Is(err, context.Canceled) {
			t.Error(err)
		}
	}()
	wait("adapter", func() bool {
		out, e := client.Capabilities(ctx)
		return e == nil && out.Host.Apply == engine.AbilityAvailable
	})
	native.onPaste = func() {
		got, e := fileBoard.Read()
		if e != nil || got.Hash() != rich.Hash() {
			t.Fatal("paste preceded exact clipboard apply")
		}
		native.events = append(native.events, "clipboard-verified")
	}
	open()
	if e := controller.Start(id, false, true, false); e != nil {
		t.Fatal(e)
	}
	if e := controller.Start(id, false, true, false); e == nil {
		t.Fatal("duplicate accepted")
	}
	wait("paste", func() bool { return !controller.Busy })
	if !controller.Sent || visible || native.pastes != 1 {
		t.Fatalf("paste failed: %s", controller.Message)
	}
	got, e := fileBoard.Read()
	if e != nil || got.Hash() != rich.Hash() {
		t.Fatal("multi-format content mismatch")
	}
	checks = append(checks, "captures external target; missing adapter fails; exact multi-format apply precedes one native-fixture paste; duplicate Enter rejected")
	var pasteAt time.Time
	native.onPaste = func() { pasteAt = time.Now() }
	open()
	if e := controller.Start(id, true, true, true); e != nil {
		t.Fatal(e)
	}
	wait("plain keep", func() bool { return !controller.Busy })
	got, e = fileBoard.Read()
	if e != nil || len(got.Parts) != 1 || got.Parts[0].Format != clip.Text || string(got.Parts[0].Data) != "quick-private-body" || !visible || !controller.Sent {
		t.Fatalf("plain/keep failed: %s", controller.Message)
	}
	if time.Since(pasteAt) < 120*time.Millisecond {
		t.Fatal("keep-open reclaimed focus before input dispatch settled")
	}
	native.onPaste = nil
	checks = append(checks, "plain Action applies exact text-only content; keep-open preserves target")
	open()
	native.onPaste = func() {
		time.AfterFunc(40*time.Millisecond, func() { queue <- func() { native.readyErr = quickpanel.ErrFocus } })
	}
	if err := controller.Start(id, false, true, true); err != nil {
		t.Fatal(err)
	}
	wait("keep-open respects user focus switch", func() bool { return !controller.Busy })
	if !controller.Sent || visible {
		t.Fatal("keep-open stole focus after user switch")
	}
	native.onPaste = nil
	native.readyErr = nil
	checks = append(checks, "keep-open waits for dispatch and never reopens after a user focus switch")

	htmlOnly := submit("html-only", clip.Snapshot{Parts: []clip.Part{{Format: clip.HTML, Data: []byte("<b>HTML only</b>")}}})
	open()
	pasteBefore := native.pastes
	if e := controller.Start(htmlOnly, true, true, false); e != nil {
		t.Fatal(e)
	}
	wait("plain without text representation", func() bool { return !controller.Busy })
	if controller.Sent || native.pastes != pasteBefore || !strings.Contains(controller.Message, "没有纯文本表示") {
		t.Fatal("HTML-only plain mode was not explicitly rejected")
	}
	checks = append(checks, "HTML-only history cannot silently become empty plain text")

	// Exercise non-text payloads through the same production restore path.
	png, e := base64.StdEncoding.DecodeString("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+jRZkAAAAASUVORK5CYII=")
	if e != nil {
		t.Fatal(e)
	}
	file := filepath.Join(private, "payload.txt")
	if e = os.WriteFile(file, []byte("file payload"), 0600); e != nil {
		t.Fatal(e)
	}
	for key, snapshot := range map[string]clip.Snapshot{"image": {Parts: []clip.Part{{Format: clip.PNG, Data: png}}}, "file": {Files: []clip.File{{Name: "payload.txt", Path: file, Size: 12}}}} {
		item := submit(key, snapshot)
		open()
		if e = controller.Start(item, false, true, false); e != nil {
			t.Fatal(e)
		}
		wait(key, func() bool { return !controller.Busy })
		got, e := fileBoard.Read()
		if e != nil || got.Hash() != snapshot.Hash() || !controller.Sent {
			t.Fatalf("%s restore mismatch: %s", key, controller.Message)
		}
	}
	checks = append(checks, "image and file history restore through the same real IPC adapter before paste")

	native.captureErr = quickpanel.ErrTarget
	open()
	if controller.CanPaste {
		t.Fatal("self target accepted")
	}
	if e := controller.Start(id, false, false, false); e != nil {
		t.Fatal(e)
	}
	wait("copy without target", func() bool { return !controller.Busy })
	if controller.Sent {
		t.Fatal("Copy injected input")
	}
	native.captureErr = nil
	checks = append(checks, "unknown/self target disables paste but allows Copy")
	for _, mode := range []string{"permission", "target-exit", "modifier-timeout", "focus-theft", "input-failure", "clipboard-replaced", "cancel", "superseded"} {
		native.ready = true
		native.checkErr = nil
		native.readyErr = nil
		native.pasteErr = nil
		native.onActivate = nil
		open()
		before := native.pastes
		switch mode {
		case "permission":
			native.checkErr = quickpanel.ErrPermission
		case "target-exit":
			native.readyErr = quickpanel.ErrTarget
		case "modifier-timeout":
			native.ready = false
		case "focus-theft":
			native.readyErr = quickpanel.ErrFocus
		case "input-failure":
			native.pasteErr = quickpanel.ErrInput
		case "clipboard-replaced":
			native.onActivate = func() {
				_ = fileBoard.Write(clip.Snapshot{Parts: []clip.Part{{Format: clip.Text, Data: []byte("other content")}}})
			}
		}
		err := controller.Start(id, false, true, false)
		if mode == "permission" {
			if !errors.Is(err, quickpanel.ErrPermission) {
				t.Fatal("permission was not rejected before apply")
			}
		} else {
			if err != nil {
				t.Fatal(err)
			}
			if mode == "cancel" {
				controller.Dismiss(false)
			}
			if mode == "superseded" {
				open()
			}
			wait(mode, func() bool { return !controller.Busy })
			if mode == "cancel" || mode == "superseded" {
				time.Sleep(100 * time.Millisecond)
				pump()
			}
		}
		if controller.Sent || native.pastes != before {
			t.Fatal("unexpected native input: " + mode)
		}
	}
	checks = append(checks, "permission/target exit/modifier timeout/focus theft/input failure/clipboard replacement/cancellation/new generation never report paste or retry input")
	native.checkErr = nil
	native.readyErr = nil
	native.pasteErr = nil
	native.onActivate = nil
	native.ready = true
	for _, supersede := range []bool{false, true} {
		open()
		block := gatedBoard.arm()
		defer block.release()
		before := native.pastes
		if err := controller.Start(id, false, true, false); err != nil {
			t.Fatal(err)
		}
		wait("blocked native-board write", func() bool {
			select {
			case <-block.started:
				return true
			default:
				return false
			}
		})
		if supersede {
			open()
		} else {
			controller.Dismiss(false)
		}
		block.release()
		wait("old write completed", func() bool {
			select {
			case <-block.done:
				return true
			default:
				return false
			}
		})
		time.Sleep(100 * time.Millisecond)
		pump()
		if controller.Busy || controller.Sent || native.pastes != before {
			t.Fatal("late apply pasted into canceled/new session")
		}
	}
	checks = append(checks, "cancellation and supersession during a blocked real adapter write ignore its late completion")

	native.ready = true
	native.readyErr = nil
	native.pasteErr = nil
	native.onActivate = nil
	open()
	before := native.activations
	controller.Dismiss(false)
	if native.activations != before {
		t.Fatal("external blur stole focus")
	}
	open()
	controller.Dismiss(true)
	if native.activations != before+1 {
		t.Fatal("explicit dismissal did not restore target")
	}
	checks = append(checks, "external blur does not restore focus; explicit dismissal restores original target")
	open()
	native.delay = 3
	delayedAt := time.Now()
	before = native.pastes
	if e := controller.Start(id, false, true, false); e != nil {
		t.Fatal(e)
	}
	wait("delayed activation", func() bool { return !controller.Busy })
	if !controller.Sent || native.pastes != before+1 || time.Since(delayedAt) < 40*time.Millisecond {
		t.Fatal("bounded focus wait failed")
	}
	checks = append(checks, "delayed activation is polled without blocking event dispatch")

	registered := map[string]func(){}
	conflict := "Ctrl+Alt+V"
	shortcuts := quickpanel.NewShortcut(func(k string, f func()) error {
		if k == conflict {
			return quickpanel.ErrShortcut
		}
		registered[k] = f
		return nil
	}, func(k string) { delete(registered, k) }, func() {})
	if e := shortcuts.Set("Ctrl+`"); e != nil {
		t.Fatal(e)
	}
	if e := shortcuts.Set(conflict); e == nil || shortcuts.Effective != "Ctrl+`" {
		t.Fatal("conflict dropped old shortcut")
	}
	conflict = ""
	if e := shortcuts.Set("Ctrl+Alt+V"); e != nil || len(registered) != 1 {
		t.Fatal("retry did not replace")
	}
	if e := shortcuts.Set(""); e != nil || len(registered) != 0 {
		t.Fatal("disable failed")
	}
	_ = shortcuts.Set("Ctrl+`")
	shortcuts.Close()
	if len(registered) != 0 {
		t.Fatal("quit leaked shortcut")
	}
	st, e := client.Config(ctx)
	if e != nil {
		t.Fatal(e)
	}
	defaultKey := "Ctrl+Alt+V"
	if runtime.GOOS == "darwin" {
		defaultKey = "Cmd+Ctrl+V"
	}
	if st.Config.Desktop.QuickPanelShortcut != defaultKey {
		t.Fatalf("default shortcut: got %q, want %q", st.Config.Desktop.QuickPanelShortcut, defaultKey)
	}
	checks = append(checks, "platform default shortcut matches macOS Command+Ctrl+V or Windows/Linux Ctrl+Alt+V")
	st.Config.Desktop.QuickPanelShortcut = "Ctrl+Alt+V"
	raw, e := toml.Marshal(st.Config)
	if e != nil {
		t.Fatal(e)
	}
	_, e = client.SaveConfig(ctx, st.Revision, raw)
	if e != nil {
		t.Fatal(e)
	}
	st, e = client.Config(ctx)
	if e != nil || st.Config.Desktop.QuickPanelShortcut != "Ctrl+Alt+V" {
		t.Fatal("shortcut config not persisted")
	}
	oldRevision := st.Revision
	st.Config.Desktop.QuickPanelShortcut = "Ctrl+unsupported"
	raw, e = toml.Marshal(st.Config)
	if e != nil {
		t.Fatal(e)
	}
	if _, e = client.SaveConfig(ctx, st.Revision, raw); e == nil {
		t.Fatal("invalid shortcut accepted")
	}
	st.Config.Desktop.QuickPanelShortcut = ""
	raw, e = toml.Marshal(st.Config)
	if e != nil {
		t.Fatal(e)
	}
	if _, e = client.SaveConfig(ctx, st.Revision, raw); e != nil {
		t.Fatal(e)
	}
	if _, e = client.SaveConfig(ctx, oldRevision, raw); e == nil {
		t.Fatal("stale shortcut config accepted")
	}
	checks = append(checks, "shortcut conflict preserves active registration; retry/disable/quit work; preference persists through real revision-checked service config")
	receipt["platform_events"] = native.events
	succeeded = true
}

type fixturePlatform struct {
	delay                                    int
	onPaste                                  func()
	active, ready                            bool
	captureErr, checkErr, readyErr, pasteErr error
	events                                   []string
	pastes, activations                      int
	onActivate                               func()
}

func (f *fixturePlatform) Capture() (quickpanel.Target, error) {
	f.events = append(f.events, "capture")
	return quickpanel.Target{ID: 42, PID: 10, Born: 100, Name: "Fixture editor"}, f.captureErr
}
func (f *fixturePlatform) Check(quickpanel.Target) error { return f.checkErr }
func (f *fixturePlatform) Activate(quickpanel.Target) error {
	f.events = append(f.events, "activate")
	f.activations++
	if f.onActivate != nil {
		f.onActivate()
	}
	return nil
}
func (f *fixturePlatform) Ready(quickpanel.Target) (bool, error) {
	if f.delay > 0 {
		f.delay--
		return false, nil
	}
	return f.ready, f.readyErr
}
func (f *fixturePlatform) Paste(quickpanel.Target) error {
	if f.pasteErr != nil {
		return f.pasteErr
	}
	if f.onPaste != nil {
		f.onPaste()
	}
	f.events = append(f.events, "paste")
	f.pastes++
	return nil
}
func (f *fixturePlatform) OwnsForeground() bool { return f.active }

// fixtureBoard delays one real adapter write; all content still crosses unicd IPC.
type fixtureBoard struct {
	base testboard.Dir
	mu   sync.Mutex
	next *blockedWrite
}
type blockedWrite struct {
	started, gate, done chan struct{}
	once                sync.Once
}

func (b *blockedWrite) release() { b.once.Do(func() { close(b.gate) }) }
func (b *fixtureBoard) arm() *blockedWrite {
	block := &blockedWrite{started: make(chan struct{}), gate: make(chan struct{}), done: make(chan struct{})}
	b.mu.Lock()
	b.next = block
	b.mu.Unlock()
	return block
}
func (b *fixtureBoard) Read() (clip.Snapshot, error) { return b.base.Read() }
func (b *fixtureBoard) Write(s clip.Snapshot) error {
	b.mu.Lock()
	block := b.next
	b.next = nil
	b.mu.Unlock()
	if block != nil {
		close(block.started)
		<-block.gate
		defer close(block.done)
	}
	return b.base.Write(s)
}
