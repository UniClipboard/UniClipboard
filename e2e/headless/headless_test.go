// Package headless verifies production binaries without a display server.
package headless

import (
	"bytes"
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"fmt"
	"image"
	"image/png"
	"io"
	"os"
	"os/exec"
	"path/filepath"
	"reflect"
	"runtime"
	"strings"
	"testing"
	"time"

	"github.com/uniclipboard/unic/engine"
	"github.com/uniclipboard/unic/internal/clip"
	"github.com/uniclipboard/unic/internal/clipboardintegration"
	api "github.com/uniclipboard/unic/internal/localapi"
	"github.com/uniclipboard/unic/internal/testboard"
)

func TestAdapterProcess(t *testing.T) {
	if os.Getenv("UNIC_ADAPTER_FIXTURE") != "1" {
		t.Skip("subprocess fixture")
	}
	ctx := context.Background()
	c := api.NewClient(os.Getenv("UNIC_SOCK"))
	if err := clipboardintegration.RunRemote(ctx, c, testboard.Dir(os.Getenv("UNIC_BOARD")), nil); err != nil {
		t.Fatal(err)
	}
}

type process struct {
	cmd  *exec.Cmd
	done chan struct{}
	log  *os.File
}

func start(t *testing.T, exe string, args []string, env []string, log string) *process {
	t.Helper()
	f, e := os.Create(log)
	if e != nil {
		t.Fatal(e)
	}
	p := &process{cmd: exec.Command(exe, args...), done: make(chan struct{}), log: f}
	p.cmd.Env = env
	p.cmd.Stdout = f
	p.cmd.Stderr = f
	if e = p.cmd.Start(); e != nil {
		t.Fatal(e)
	}
	cmd := p.cmd
	go func() { _ = cmd.Wait(); _ = f.Close(); close(p.done) }()
	t.Cleanup(func() { p.stop() })
	return p
}
func (p *process) stop() {
	if p.cmd == nil {
		return
	}
	_ = p.cmd.Process.Kill()
	select {
	case <-p.done:
	case <-time.After(5 * time.Second):
	}
	p.cmd = nil
}
func until(t *testing.T, what string, f func() bool) {
	t.Helper()
	deadline := time.Now().Add(40 * time.Second)
	for time.Now().Before(deadline) {
		if f() {
			return
		}
		time.Sleep(100 * time.Millisecond)
	}
	t.Fatalf("timeout: %s", what)
}
func need(t *testing.T, e error) {
	t.Helper()
	if e != nil {
		t.Fatal(e)
	}
}
func TestProductionHeadless(t *testing.T) {
	if os.Getenv("UNIC_HEADLESS_E2E") == "" {
		t.Skip("set UNIC_HEADLESS_E2E=1")
	}
	ctx, cancel := context.WithTimeout(context.Background(), 4*time.Minute)
	defer cancel()
	root, e := filepath.Abs("../..")
	need(t, e)
	artifacts := os.Getenv("UNIC_HEADLESS_ARTIFACTS")
	if artifacts == "" {
		artifacts = t.TempDir()
	}
	need(t, os.MkdirAll(artifacts, 0700))
	artifacts, e = filepath.Abs(artifacts)
	need(t, e)
	bin := filepath.Join(artifacts, "bin")
	need(t, os.MkdirAll(bin, 0700))
	manifest := map[string]any{"go": runtime.Version(), "os": runtime.GOOS, "arch": runtime.GOARCH, "production_build_tags": []string{}, "display_server": false, "race": true, "status": "running"}
	checks := []string{}
	hashes := map[string]string{}
	defer func() {
		manifest["checks"] = checks
		manifest["binary_sha256"] = hashes
		if t.Failed() {
			manifest["status"] = "failed"
		} else {
			manifest["status"] = "passed"
		}
		b, _ := json.MarshalIndent(manifest, "", "  ")
		_ = os.WriteFile(filepath.Join(artifacts, "manifest.json"), b, 0600)
	}()
	// Record source identity, including uncommitted implementation files.
	source, e := sourceHashes(root)
	need(t, e)
	manifest["source_sha256"] = source
	t.Cleanup(func() {
		after, e := sourceHashes(root)
		unchanged := e == nil && reflect.DeepEqual(source, after)
		manifest["source_unchanged"] = unchanged
		if !unchanged {
			manifest["status"] = "failed"
			t.Error("source changed during acceptance")
		}
		b, _ := json.MarshalIndent(manifest, "", "  ")
		_ = os.WriteFile(filepath.Join(artifacts, "manifest.json"), b, 0600)
	})
	for _, name := range []string{"unic", "unicd"} {
		path := filepath.Join(bin, name)
		cmd := exec.CommandContext(ctx, "go", "build", "-race", "-o", path, "./app/service/cmd/"+name)
		cmd.Dir = root
		if out, e := cmd.CombinedOutput(); e != nil {
			t.Fatalf("build %s: %v\n%s", name, e, out)
		}
		b, e := os.ReadFile(path)
		need(t, e)
		sum := sha256.Sum256(b)
		hashes[name] = hex.EncodeToString(sum[:])
	}
	cmd := exec.CommandContext(ctx, "go", "list", "-deps", "./app/service/cmd/unic", "./app/service/cmd/unicd")
	cmd.Dir = root
	deps, e := cmd.Output()
	need(t, e)
	need(t, os.WriteFile(filepath.Join(artifacts, "dependencies.txt"), deps, 0600))
	for _, bad := range []string{"github.com/egoist/mygo", "/uikit", "/app/desktop/"} {
		if bytes.Contains(deps, []byte(bad)) {
			t.Fatalf("UI dependency: %s", bad)
		}
	}
	checks = append(checks, "production binaries have no UI dependencies")
	env := []string{}
	for _, v := range os.Environ() {
		if strings.HasPrefix(v, "DISPLAY=") || strings.HasPrefix(v, "WAYLAND_DISPLAY=") || strings.HasPrefix(v, "UNIC_") {
			continue
		}
		env = append(env, v)
	}
	homes := []string{}
	clients := []*api.Client{}
	procs := []*process{}
	tempRoot := "/tmp"
	if runtime.GOOS == "darwin" {
		tempRoot = "/private/tmp"
	}
	base, e := os.MkdirTemp(tempRoot, "uh")
	need(t, e)
	defer func() { _ = os.RemoveAll(base) }()
	for i := 0; i < 2; i++ {
		home := filepath.Join(base, fmt.Sprint(i))
		need(t, os.MkdirAll(home, 0700))
		homes = append(homes, home)
		need(t, os.WriteFile(filepath.Join(home, "config.toml"), []byte("[telemetry]\nenabled=false\n"), 0600))
		if i == 1 {
			pluginDir := filepath.Join(home, "plugins", "lua", "transient")
			need(t, os.MkdirAll(pluginDir, 0700))
			need(t, os.WriteFile(filepath.Join(pluginDir, "spec.toml"), []byte("manifest_version = 1\nkind = \"lua\"\nentry = \"main.lua\"\nid = \"test.transient\"\nversion = \"1.0.0\"\napi_version = 1\ncapabilities = [\"processor\"]\n"), 0600))
			need(t, os.WriteFile(filepath.Join(pluginDir, "main.lua"), []byte(`return {id="test.transient",version="1.0.0",api_version=1,capabilities={"processor"},process=function(ctx) if ctx.item.text=="transient receive" then ctx:set_persist(false) end end}`), 0600))
			need(t, os.WriteFile(filepath.Join(home, "config.toml"), []byte("[telemetry]\nenabled=false\n[plugins]\nlua = [\"test.transient\"]\n"), 0600))
		}
		socket := filepath.Join(home, "unic.sock")
		c := api.NewClient(socket)
		clients = append(clients, c)
		p := start(t, filepath.Join(bin, "unicd"), nil, append(env, "UNIC_HOME="+home, "UNIC_RENDEZVOUS_URL=http://127.0.0.1:1"), filepath.Join(artifacts, fmt.Sprintf("node-%d.log", i)))
		procs = append(procs, p)
		until(t, "daemon ready", func() bool { _, e := c.Status(ctx); return e == nil })
	}
	cli := func(node int, args ...string) []byte {
		t.Helper()
		cmd := exec.CommandContext(ctx, filepath.Join(bin, "unic"), args...)
		cmd.Env = append(env, "UNIC_HOME="+homes[node])
		var stderr bytes.Buffer
		cmd.Stderr = &stderr
		out, e := cmd.Output()
		if e != nil {
			t.Fatalf("CLI %v: %v %s", args, e, stderr.Bytes())
		}
		return out
	}
	a, b := clients[0], clients[1]
	caps, e := a.Capabilities(ctx)
	need(t, e)
	if caps.Host.Observe != engine.AbilityUnavailable || caps.Host.Apply != engine.AbilityUnavailable || len(caps.Host.WriteFormats) != 0 || caps.Host.WriteFiles {
		t.Fatalf("headless caps: %+v", caps)
	}
	text := "headless first"
	input := api.SubmitInput{RequestID: "before-pair", Snapshot: snapshot(text)}
	first, e := a.Submit(ctx, input)
	need(t, e)
	if first.HistoryID == "" || first.SyncDisposition != engine.SyncNoTargets {
		t.Fatalf("commit: %+v", first)
	}
	retry, e := a.Submit(ctx, input)
	need(t, e)
	if retry.HistoryID != first.HistoryID {
		t.Fatal("idempotency duplicated history")
	}
	input.Snapshot = snapshot("different")
	_, e = a.Submit(ctx, input)
	if api.ReasonOf(e) != "conflict" {
		t.Fatalf("idempotency conflict: %v", e)
	}
	checks = append(checks, "headless commit, no targets and same-key retry")
	// submit --json keeps the local commit and the network outcome in separate fields.
	type submitJSON struct {
		Submission engine.SubmissionResult `json:"submission"`
		Send       struct {
			Outcome string `json:"outcome"`
		} `json:"send"`
	}
	var cliReport submitJSON
	need(t, json.Unmarshal(cli(0, "send", "--to", "all", "--request-id", "cli-commit", "--json", "CLI content"), &cliReport))
	fromCLI := cliReport.Submission
	if cliReport.Send.Outcome != "none" {
		t.Fatalf("no peer is paired, so no send operation exists: %+v", cliReport.Send)
	}
	if fromCLI.HistoryID == "" {
		t.Fatal("CLI did not commit")
	}
	var queried engine.SubmissionResult
	need(t, json.Unmarshal(cli(0, "dev", "submission", "cli-commit"), &queried))
	if queried.HistoryID != fromCLI.HistoryID {
		t.Fatal("CLI submission query differs")
	}
	cli(0, "history", "export", string(fromCLI.HistoryID), "--output", filepath.Join(base, "cli-export"))
	cli(0, "config", "set", "sync.enabled", "false")
	var pausedReport submitJSON
	need(t, json.Unmarshal(cli(0, "send", "--to", "all", "--json", "--request-id", "disabled", "paused"), &pausedReport))
	paused := pausedReport.Submission
	if paused.HistoryID == "" || paused.SyncDisposition == engine.SyncQueued {
		t.Fatalf("disabled commit: %+v", paused)
	}
	cli(0, "config", "set", "sync.enabled", "true")
	checks = append(checks, "production CLI submit, query, export and disabled-sync commit")
	_, e = a.Clipboard(ctx)
	if api.ReasonOf(e) != "unavailable" {
		t.Fatalf("paste without adapter: %v", e)
	}
	_, e = a.ExecuteAction(ctx, string(first.HistoryID), "builtin.text/upper", api.ActionInput{RequestID: "preflight", Apply: true})
	if api.ReasonOf(e) != "unavailable" {
		t.Fatalf("apply preflight: %v", e)
	}
	action, e := a.ExecuteAction(ctx, string(first.HistoryID), "builtin.text/upper", api.ActionInput{RequestID: "action"})
	need(t, e)
	if action.Item.ID == "" || action.Applied {
		t.Fatalf("action: %+v", action)
	}
	checks = append(checks, "Action commits without clipboard; apply checks availability")
	ticket, e := a.ExportTicket(ctx)
	need(t, e)
	_, e = b.Pair(ctx, ticket)
	need(t, e)
	var pa, pb api.PendingPeer
	until(t, "mutual pending", func() bool {
		x, e := a.Pending(ctx)
		y, f := b.Pending(ctx)
		if e != nil || f != nil || len(x) != 1 || len(y) != 1 {
			return false
		}
		pa, pb = x[0], y[0]
		return pa.Fingerprint == pb.Fingerprint
	})
	// The fixture verifies both displayed fingerprints and explicitly approves both revisions.
	for i, c := range clients {
		p := []api.PendingPeer{pa, pb}[i]
		_, e = c.Approve(ctx, p.ID, api.Approval{Fingerprint: p.Fingerprint, Revision: p.Revision, Bindings: p.Bindings})
		need(t, e)
	}
	until(t, "connected", func() bool {
		x, e := a.Status(ctx)
		y, f := b.Status(ctx)
		return e == nil && f == nil && len(x.Peers) == 1 && len(y.Peers) == 1 && x.Peers[0].Connected && y.Peers[0].Connected
	})
	checks = append(checks, "two production daemons pair with mutual explicit approval")
	expectedSources := map[string]*engine.SourceApplication{}
	verify := func(id string, want []byte) {
		t.Helper()
		var receipt engine.ReceivedContent
		until(t, "headless receipt "+id, func() bool {
			rs, e := b.Received(ctx)
			if e != nil {
				return false
			}
			for _, r := range rs {
				var out bytes.Buffer
				if e = b.CopyReceivedContent(ctx, string(r.ReceiptID), 0, &out); e == nil && bytes.Equal(out.Bytes(), want) {
					receipt = r
					return true
				}
			}
			return false
		})
		if receipt.ApplyState != engine.ApplyAvailable {
			t.Fatal("headless receipt was marked applied")
		}
		if receipt.HistoryID == "" {
			t.Fatal("receipt missing history")
		}
		if source := expectedSources[id]; source != nil {
			if !reflect.DeepEqual(receipt.Content.SourceApplication, source) {
				t.Fatal("received descriptor lost source application")
			}
			item, err := b.GetHistory(ctx, string(receipt.HistoryID))
			need(t, err)
			if !reflect.DeepEqual(item.SourceApplication, source) {
				t.Fatal("received history lost source application")
			}
		}
		var content bytes.Buffer
		need(t, b.CopyHistoryContent(ctx, string(receipt.HistoryID), 0, &content))
		if !bytes.Equal(content.Bytes(), want) {
			t.Fatal("history content differs")
		}
		dest := filepath.Join(base, "export-"+id)
		names, e := b.ExportHistory(ctx, string(receipt.HistoryID), dest)
		need(t, e)
		if len(names) != len(receipt.Content.Parts) {
			t.Fatal("export part count")
		}
		got, e := os.ReadFile(filepath.Join(dest, names[0]))
		need(t, e)
		if !bytes.Equal(got, want) {
			t.Fatal("export differs")
		}
		if _, e = b.ExportHistory(ctx, string(receipt.HistoryID), dest); e == nil {
			t.Fatal("export overwrote existing file")
		}
		need(t, b.DismissReceived(ctx, string(receipt.ReceiptID)))
	}
	_, e = a.Submit(ctx, api.SubmitInput{RequestID: "text", Snapshot: snapshot("network text")})
	need(t, e)
	verify("text", []byte("network text"))
	var pngbuf bytes.Buffer
	need(t, png.Encode(&pngbuf, image.NewRGBA(image.Rect(0, 0, 2, 2))))
	_, e = a.Submit(ctx, api.SubmitInput{RequestID: "png", Snapshot: clipSnapshotPNG(pngbuf.Bytes())})
	need(t, e)
	verify("png", pngbuf.Bytes())
	src := filepath.Join(base, "sample.txt")
	need(t, os.WriteFile(src, []byte("network file"), 0600))
	_, e = a.Submit(ctx, api.SubmitInput{RequestID: "file", Snapshot: clipSnapshotFile(src)})
	need(t, e)
	verify("file", []byte("network file"))
	checks = append(checks, "headless text, PNG and file receive, read, export and dismiss")
	// These are production HTTP/Engine/session/wire/storage flows. No Engine fixture.
	provenance := &engine.SourceApplication{Platform: "darwin", Identifier: "org.example.Source", Name: "Source Fixture", Attribution: "source_marker", IconPNG: pngbuf.Bytes()}
	rich := snapshot("metadata rich text")
	rich.Parts = append(rich.Parts, clip.Part{Format: clip.HTML, Data: []byte("<b>metadata rich text</b>")})
	for i, snap := range []clip.Snapshot{rich, clipSnapshotPNG(pngbuf.Bytes()), clipSnapshotFile(src)} {
		id := fmt.Sprintf("source-%d", i)
		snap.SourceApplication = provenance.Clone()
		expectedSources[id] = provenance.Clone()
		result, err := a.Submit(ctx, api.SubmitInput{RequestID: id, Snapshot: snap})
		need(t, err)
		local, err := a.GetHistory(ctx, string(result.HistoryID))
		need(t, err)
		if !reflect.DeepEqual(local.SourceApplication, provenance) {
			t.Fatal("local history lost source application")
		}
		verify(id, [][]byte{[]byte("metadata rich text"), pngbuf.Bytes(), []byte("network file")}[i])
	}
	beforeInvalid, err := a.ListHistory(ctx, "", "")
	need(t, err)
	for i, invalid := range []*engine.SourceApplication{
		{Platform: "darwin", Identifier: "/private/source.app", Attribution: "source_marker"},
		{Platform: "darwin", Identifier: "org.example.Source", Attribution: "source_marker", IconPNG: []byte("not a PNG")},
		{Platform: "darwin", Identifier: "org.example.Source", Attribution: "source_marker", IconPNG: make([]byte, (16<<10)+1)},
		{Platform: "darwin", Identifier: "org.example.Source", Attribution: "unverified"},
	} {
		snap := snapshot("rejected application metadata")
		snap.SourceApplication = invalid
		_, err := a.Submit(ctx, api.SubmitInput{RequestID: fmt.Sprintf("invalid-source-%d", i), Snapshot: snap})
		if api.ReasonOf(err) != "invalid_argument" {
			t.Fatalf("invalid metadata accepted: %v", err)
		}
	}
	afterInvalid, err := a.ListHistory(ctx, "", "")
	need(t, err)
	if len(beforeInvalid.Items) != len(afterInvalid.Items) {
		t.Fatal("invalid metadata committed history")
	}
	checks = append(checks, "source identity and PNG survive rich text, image and file sync; malformed and oversized metadata rejected before commit")

	// Referenced files fail with source_changed after mutation, without corrupting history.
	fileResult, e := a.Submit(ctx, api.SubmitInput{RequestID: "source-check", Snapshot: clipSnapshotFile(src)})
	need(t, e)
	need(t, os.WriteFile(src, []byte("mutated source bytes"), 0600))
	var changed bytes.Buffer
	e = a.CopyHistoryContent(ctx, string(fileResult.HistoryID), 0, &changed)
	if api.ReasonOf(e) != "source_changed" {
		t.Fatalf("changed source: %v", e)
	}
	// Exclusive output creation rejects a symlink and preserves its external target.
	symlinkDir := filepath.Join(base, "symlink-export")
	need(t, os.MkdirAll(symlinkDir, 0700))
	outside := filepath.Join(base, "outside.txt")
	need(t, os.WriteFile(outside, []byte("keep"), 0600))
	need(t, os.Symlink(outside, filepath.Join(symlinkDir, "part-1.txt")))
	if _, e = a.ExportHistory(ctx, string(first.HistoryID), symlinkDir); e == nil {
		t.Fatal("export followed existing symlink")
	}
	preserved, e := os.ReadFile(outside)
	need(t, e)
	if string(preserved) != "keep" {
		t.Fatal("export changed external target")
	}
	checks = append(checks, "source mutation and export symlink rejection")
	_, e = a.Submit(ctx, api.SubmitInput{RequestID: "transient", Snapshot: snapshot("transient receive")})
	need(t, e)
	var transient engine.ReceivedContent
	until(t, "transient Received content", func() bool {
		rs, e := b.Received(ctx)
		if e != nil {
			return false
		}
		for _, r := range rs {
			if r.HistoryID != "" {
				continue
			}
			var out bytes.Buffer
			if b.CopyReceivedContent(ctx, string(r.ReceiptID), 0, &out) == nil && out.String() == "transient receive" {
				transient = r
				return true
			}
		}
		return false
	})
	if string(cli(1, "recv", "show", string(transient.ReceiptID))) != "transient receive" {
		t.Fatal("CLI transient content differs")
	}
	cli(1, "recv", "dismiss", string(transient.ReceiptID))
	checks = append(checks, "persist=false remains readable through Received without forced history")
	// Start the real IPC adapter state machine with a directory board in a separate process.
	board := filepath.Join(base, "board")
	need(t, os.MkdirAll(board, 0700))
	need(t, os.WriteFile(filepath.Join(board, "text"), []byte("baseline"), 0600))
	exe, e := os.Executable()
	need(t, e)
	adapter := start(t, exe, []string{"-test.run=^TestAdapterProcess$"}, append(env, "UNIC_ADAPTER_FIXTURE=1", "UNIC_SOCK="+a.Path, "UNIC_BOARD="+board), filepath.Join(artifacts, "adapter.log"))
	until(t, "adapter ready", func() bool { c, e := a.Capabilities(ctx); return e == nil && c.Host.Apply == engine.AbilityAvailable })
	until(t, "adapter commands ready", func() bool { _, e := a.Clipboard(ctx); return e == nil })
	var other string
	e = a.AdapterCall(ctx, "", "register", nil, &other)
	if !isEngineCode(e, engine.CodeInUse) {
		t.Fatalf("second adapter: %v", e)
	}
	value := "explicit clipboard"
	need(t, a.SetClipboard(ctx, api.SetClipboard{Text: &value}))
	data, e := os.ReadFile(filepath.Join(board, "text"))
	need(t, e)
	if string(data) != value {
		t.Fatal("adapter did not write clipboard")
	}
	page, e := a.QueryHistory(ctx, api.HistoryQuery{Limit: 200})
	need(t, e)
	beforeCount := len(page.Items)
	time.Sleep(900 * time.Millisecond)
	page, e = a.QueryHistory(ctx, api.HistoryQuery{Limit: 200})
	need(t, e)
	if len(page.Items) != beforeCount {
		t.Fatal("explicit copy echoed into history")
	}
	need(t, os.WriteFile(filepath.Join(board, "text"), []byte("automatic capture"), 0600))
	until(t, "automatic capture", func() bool {
		page, e := a.QueryHistory(ctx, api.HistoryQuery{Limit: 1})
		if e != nil || len(page.Items) == 0 {
			return false
		}
		var out bytes.Buffer
		return a.CopyHistoryContent(ctx, page.Items[0].ID, 0, &out) == nil && out.String() == "automatic capture"
	})
	adapter.stop()
	until(t, "adapter expires", func() bool { c, e := a.Capabilities(ctx); return e == nil && c.Host.Apply == engine.AbilityUnavailable })
	_, e = a.Clipboard(ctx)
	if api.ReasonOf(e) != "unavailable" {
		t.Fatal("expired adapter still usable")
	}
	checks = append(checks, "exclusive IPC adapter copy and crash expiry")
	_, e = b.Submit(ctx, api.SubmitInput{RequestID: "offline", Snapshot: snapshot("offline backlog")})
	need(t, e)
	until(t, "offline receipt", func() bool {
		rs, e := a.Received(ctx)
		if e != nil {
			return false
		}
		for _, r := range rs {
			var out bytes.Buffer
			if a.CopyReceivedContent(ctx, string(r.ReceiptID), 0, &out) == nil && out.String() == "offline backlog" {
				return true
			}
		}
		return false
	})
	reconnected := start(t, exe, []string{"-test.run=^TestAdapterProcess$"}, append(env, "UNIC_ADAPTER_FIXTURE=1", "UNIC_SOCK="+a.Path, "UNIC_BOARD="+board), filepath.Join(artifacts, "adapter-reconnect.log"))
	until(t, "adapter reconnect", func() bool { _, e := a.Clipboard(ctx); return e == nil })
	time.Sleep(900 * time.Millisecond)
	current, e := os.ReadFile(filepath.Join(board, "text"))
	need(t, e)
	if string(current) != "automatic capture" {
		t.Fatal("adapter replayed offline backlog")
	}
	_, e = b.Submit(ctx, api.SubmitInput{RequestID: "fresh-after-reconnect", Snapshot: snapshot("new remote item")})
	need(t, e)
	until(t, "fresh receive applies", func() bool {
		current, e := os.ReadFile(filepath.Join(board, "text"))
		return e == nil && string(current) == "new remote item"
	})
	reconnected.stop()
	until(t, "reconnected adapter expires", func() bool { c, e := a.Capabilities(ctx); return e == nil && c.Host.Apply == engine.AbilityUnavailable })
	checks = append(checks, "automatic capture, no copy echo, no backlog replay and fresh remote application")

	// Exercise generation cleanup, malformed reports and fixed resource limits through IPC.
	var generation string
	need(t, a.AdapterCall(ctx, "", "register", nil, &generation))
	setupGeneration := func(g string) {
		need(t, a.AdapterCall(ctx, g, "capabilities", engine.HostCapabilities{ReadFormats: []string{engine.MimeText}, WriteFormats: []string{engine.MimeText}, ReadFiles: true, WriteFiles: true, Observe: engine.AbilityAvailable, Apply: engine.AbilityAvailable}, nil))
		need(t, a.AdapterCall(ctx, g, "observe", engine.Observation{Epoch: "fixture-" + g, Sequence: 1, Mode: engine.ObserveBaseline}, nil))
		need(t, a.AdapterCall(ctx, g, "ready", nil, nil))
	}
	setupGeneration(generation)
	var tk engine.ApplyTicket
	need(t, a.AdapterCall(ctx, generation, "apply-history", engine.ApplyHistoryRequest{HistoryID: first.HistoryID, ExpectedEpoch: "fixture-" + generation, ExpectedSequence: 1}, &tk))
	e = a.AdapterCall(ctx, generation, "report", engine.ApplyResult{Token: tk.Token, Outcome: "invalid"}, nil)
	if !isEngineCode(e, engine.CodeInvalidArgument) {
		t.Fatalf("invalid report: %v", e)
	}
	need(t, a.AdapterCall(ctx, generation, "close", nil, nil))
	oldGeneration := generation
	need(t, a.AdapterCall(ctx, "", "register", nil, &generation))
	setupGeneration(generation)
	need(t, a.AdapterCall(ctx, generation, "apply-history", engine.ApplyHistoryRequest{HistoryID: first.HistoryID, ExpectedEpoch: "fixture-" + generation, ExpectedSequence: 1}, &tk))
	need(t, a.AdapterCall(ctx, generation, "report", engine.ApplyResult{Token: tk.Token, Outcome: engine.ApplyOutcomeFailed}, nil))
	need(t, a.AdapterCall(ctx, generation, "release", tk.LeaseID, nil))
	for i := 0; i < 128; i++ {
		var h engine.ContentHandle
		need(t, a.AdapterCall(ctx, generation, "import", snapshot("lease"), &h))
	}
	e = a.AdapterCall(ctx, generation, "import", snapshot("extra"), nil)
	if !isEngineCode(e, engine.CodeResourceExhausted) {
		t.Fatalf("lease bound: %v", e)
	}
	need(t, a.AdapterCall(ctx, generation, "close", nil, nil))
	e = a.AdapterCall(ctx, oldGeneration, "heartbeat", nil, nil)
	if !isEngineCode(e, engine.CodeResourceExpired) {
		t.Fatalf("stale generation: %v", e)
	}
	checks = append(checks, "invalid apply report cleanup, stale generations and 128-lease limit")
	need(t, a.AdapterCall(ctx, "", "register", nil, &generation))
	large := filepath.Join(base, "large.bin")
	file, e := os.Create(large)
	need(t, e)
	need(t, file.Truncate(8<<20))
	need(t, file.Close())
	var largeHandle engine.ContentHandle
	need(t, a.AdapterCall(ctx, generation, "import", clip.Snapshot{Files: []clip.File{{Name: "large.bin", Path: large, Size: 8 << 20}}}, &largeHandle))
	readers := []io.ReadCloser{}
	for i := 0; i < 4; i++ {
		reader, e := a.AdapterContent(ctx, generation, largeHandle.LeaseID, largeHandle.Content.Parts[0].ID)
		need(t, e)
		readers = append(readers, reader)
	}
	extra, e := a.AdapterContent(ctx, generation, largeHandle.LeaseID, largeHandle.Content.Parts[0].ID)
	if e == nil {
		_ = extra.Close()
		t.Fatal("fifth content stream admitted")
	}
	if !isEngineCode(e, engine.CodeResourceExhausted) {
		t.Fatalf("stream bound: %v", e)
	}
	var blocked bytes.Buffer
	e = a.CopyHistoryContent(ctx, string(first.HistoryID), 0, &blocked)
	if api.ReasonOf(e) != "resource_exhausted" {
		t.Fatalf("history did not share content bound: %v", e)
	}
	_, e = a.Status(ctx)
	need(t, e)
	need(t, a.AdapterCall(ctx, generation, "close", nil, nil))
	for _, reader := range readers {
		_ = reader.Close()
	}
	checks = append(checks, "shared four-stream bound and session cleanup do not block service status")
	// Service restart keeps identity/history and invalidates old session handles.
	old, e := a.Status(ctx)
	need(t, e)
	procs[0].stop()
	procs[0] = start(t, filepath.Join(bin, "unicd"), nil, append(env, "UNIC_HOME="+homes[0], "UNIC_RENDEZVOUS_URL=http://127.0.0.1:1"), filepath.Join(artifacts, "restart.log"))
	until(t, "restart", func() bool { s, e := a.Status(ctx); return e == nil && s.ID == old.ID })
	_, e = a.GetHistory(ctx, string(first.HistoryID))
	need(t, e)
	checks = append(checks, "restart preserves identity and history")
	for _, p := range procs {
		p.stop()
	}
	for _, name := range []string{"node-0.log", "node-1.log", "adapter.log", "adapter-reconnect.log", "restart.log"} {
		log, e := os.ReadFile(filepath.Join(artifacts, name))
		need(t, e)
		if bytes.Contains(log, []byte("DATA RACE")) {
			t.Fatalf("race in %s", name)
		}
	}
}
func snapshot(s string) clip.Snapshot {
	return clip.Snapshot{Parts: []clip.Part{{Format: clip.Text, Data: []byte(s)}}}
}
func clipSnapshotPNG(b []byte) clip.Snapshot {
	return clip.Snapshot{Parts: []clip.Part{{Format: clip.PNG, Data: b}}}
}
func clipSnapshotFile(p string) clip.Snapshot {
	return clip.Snapshot{Files: []clip.File{{Name: filepath.Base(p), Path: p, Size: 12}}}
}
func isEngineCode(err error, code engine.Code) bool {
	e, ok := err.(*engine.Error)
	return ok && e.Code == code
}

func sourceHashes(root string) (map[string]string, error) {
	hashes := map[string]string{}
	add := func(path string) error {
		data, e := os.ReadFile(path)
		if e != nil {
			return e
		}
		sum := sha256.Sum256(data)
		rel, e := filepath.Rel(root, path)
		if e != nil {
			return e
		}
		hashes[filepath.ToSlash(rel)] = hex.EncodeToString(sum[:])
		return nil
	}
	for _, dir := range []string{"app/service", "engine", "internal", "plugin", "e2e/headless", "sourceapp"} {
		if e := filepath.WalkDir(filepath.Join(root, dir), func(path string, d os.DirEntry, e error) error {
			if e != nil {
				return e
			}
			if d.IsDir() {
				return nil
			}
			return add(path)
		}); e != nil {
			return nil, e
		}
	}
	for _, name := range []string{"go.mod", "go.sum"} {
		if e := add(filepath.Join(root, name)); e != nil {
			return nil, e
		}
	}
	return hashes, nil
}
