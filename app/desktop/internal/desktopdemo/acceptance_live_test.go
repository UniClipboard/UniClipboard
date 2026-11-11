package desktopdemo

import (
	"bytes"
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"fmt"
	"image"
	"image/png"
	"os"
	"os/exec"
	"path/filepath"
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

// Failure scenarios are specified in docs/exec/desktop-live-data.md.
// This drives the production binding and a production unicd process. It never
// starts mygo, a renderer, a native window, or the system clipboard.
func scenarioDesktopLiveData(t *testing.T) {
	ctx, cancel := context.WithTimeout(context.Background(), 3*time.Minute)
	defer cancel()
	root, err := filepath.Abs("../../../..")
	if err != nil {
		t.Fatal(err)
	}
	private, err := os.MkdirTemp("/tmp", "udata-")
	if err != nil {
		t.Fatal(err)
	}
	defer func() {
		if e := os.RemoveAll(private); e != nil {
			t.Error(e)
		}
	}()
	checks := []string{}
	receipt := map[string]any{"go": runtime.Version(), "os": runtime.GOOS, "native_ui": false, "renderer": false, "system_clipboard": false}
	hash := func(b []byte) string { v := sha256.Sum256(b); return hex.EncodeToString(v[:]) }
	sources := map[string]string{}
	for _, dir := range []string{"app/desktop", "internal/localapi", "internal/localconfig", "app/service", "engine"} {
		err = filepath.WalkDir(filepath.Join(root, dir), func(p string, e os.DirEntry, err error) error {
			if err != nil {
				return err
			}
			if e.IsDir() || (!strings.HasSuffix(p, ".go") && !strings.HasSuffix(p, ".mod")) {
				return nil
			}
			b, err := os.ReadFile(p)
			if err != nil {
				return err
			}
			rel, _ := filepath.Rel(root, p)
			sources[rel] = hash(b)
			return nil
		})
		if err != nil {
			t.Fatal(err)
		}
	}
	receipt["source_sha256"] = sources
	defer func() {
		receipt["checks"] = checks
		receipt["passed"] = !t.Failed()
		if out := os.Getenv("UNIC_DESKTOP_E2E_CASE_RECEIPT"); out != "" {
			b, _ := json.MarshalIndent(receipt, "", "  ")
			if e := os.MkdirAll(filepath.Dir(out), 0700); e != nil {
				t.Error(e)
			}
			if e := os.WriteFile(out, b, 0600); e != nil {
				t.Error(e)
			}
		}
	}()
	exe := filepath.Join(private, "unicd")
	build := exec.CommandContext(ctx, "go", "build", "-race", "-o", exe, "./app/service/cmd/unicd")
	build.Dir = root
	if out, e := build.CombinedOutput(); e != nil {
		t.Fatalf("build: %v\n%s", e, out)
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
	if e = os.WriteFile(filepath.Join(home, "config.toml"), []byte("[telemetry]\nenabled=false\n[sync]\nenabled=false\n"), 0600); e != nil {
		t.Fatal(e)
	}
	env := []string{}
	for _, v := range os.Environ() {
		if !strings.HasPrefix(v, "UNIC_") {
			env = append(env, v)
		}
	}
	var logs bytes.Buffer
	cmd := exec.CommandContext(ctx, exe)
	cmd.Env = append(env, "UNIC_HOME="+home, "UNIC_RENDEZVOUS_URL=http://127.0.0.1:1")
	cmd.Stdout = &logs
	cmd.Stderr = &logs
	if e = cmd.Start(); e != nil {
		t.Fatal(e)
	}
	defer func() {
		_ = cmd.Process.Kill()
		_ = cmd.Wait()
		if t.Failed() {
			t.Log(logs.String())
		}
	}()
	client := api.NewClient(filepath.Join(home, "unic.sock"))
	wait := func(label string, f func() bool) {
		t.Helper()
		deadline := time.Now().Add(20 * time.Second)
		for time.Now().Before(deadline) {
			if f() {
				return
			}
			time.Sleep(20 * time.Millisecond)
		}
		t.Fatalf("timeout: %s", label)
	}
	wait("service", func() bool { _, e := client.Status(ctx); return e == nil })
	for _, page := range []string{"setup", "unlock", "startup", "recovery"} {
		if e := Run([]string{"--page", page}); e == nil {
			t.Fatalf("removed page %s accepted", page)
		}
	}
	desktop := NewDesktop(ctx, client)
	d := desktop.d
	defer desktop.Close()
	spin := func(label string, f func() bool) { t.Helper(); wait(label, func() bool { d.live.poll(d); return f() }) }
	spin("empty history", func() bool { return d.live.loaded && !d.live.loading })
	if d.page != "history" || len(d.entries) != 0 || len(d.devices) != 1 || len(d.tags) != 0 {
		t.Fatal("fixture or entry flow leaked")
	}
	checks = append(checks, "opens empty history directly without fixtures")
	submit := func(id string, s clip.Snapshot) string {
		t.Helper()
		r, e := client.Submit(ctx, api.SubmitInput{RequestID: id, Snapshot: s})
		if e != nil {
			t.Fatal(e)
		}
		return string(r.HistoryID)
	}
	id := submit("text", clip.Snapshot{Parts: []clip.Part{{Format: "text/plain", Data: []byte("Desktop live acceptance")}}})
	spin("event refresh", func() bool { return len(d.entries) == 1 && d.entries[0].id == id })
	d.selectEntry(id)
	spin("text detail", func() bool {
		return d.current() != nil && d.current().text == "Desktop live acceptance" && !d.live.detailBusy
	})
	checks = append(checks, "real submission appears through event/reconciliation and content read")
	d.setEntryTags(id, []string{"acceptance"})
	spin("tag update", func() bool { return !d.live.busy && d.current() != nil && len(d.current().tags) == 1 })
	current, e := client.GetHistory(ctx, id)
	if e != nil || len(current.Tags) != 1 {
		t.Fatal("tags not persisted", e)
	}
	checks = append(checks, "revision-checked tag assignment persists")
	// A concurrent update must reject the stale UI revision without overwriting it.
	tags := []string{"concurrent"}
	_, e = client.UpdateHistory(ctx, id, api.HistoryUpdate{Revision: current.Revision, Tags: &tags})
	if e != nil {
		t.Fatal(e)
	}
	d.setEntryTags(id, []string{"stale"})
	spin("stale update", func() bool { return !d.live.busy && d.live.message != "" })
	current, e = client.GetHistory(ctx, id)
	if e != nil || len(current.Tags) != 1 || current.Tags[0] != "concurrent" {
		t.Fatal("stale write replaced tags", e)
	}
	checks = append(checks, "stale mutation fails and preserves concurrent tags")
	d.useEntry()
	spin("restore unavailable", func() bool { return !d.live.busy && d.live.message != "" })
	checks = append(checks, "restore without adapter reports failure")
	adapterCtx, stopAdapter := context.WithCancel(ctx)
	defer stopAdapter()
	board := testboard.Dir(filepath.Join(private, "board"))
	done := make(chan struct{})
	go func() { defer close(done); _ = clipboardintegration.RunRemote(adapterCtx, client, board, nil) }()
	defer func() {
		stopAdapter()
		select {
		case <-done:
		case <-time.After(5 * time.Second):
			t.Error("adapter did not stop")
		}
	}()
	wait("adapter", func() bool {
		c, e := client.Capabilities(ctx)
		return e == nil && c.Host.Apply == engine.AbilityAvailable
	})
	// RunRemote's command loop is ready once the clipboard read command succeeds.
	wait("adapter commands", func() bool { _, e := client.Clipboard(ctx); return e == nil })
	d.useEntry()
	spin("restore completes", func() bool { return !d.live.busy })
	wait("clipboard bytes", func() bool {
		s, e := board.Read()
		return e == nil && len(s.Parts) > 0 && string(s.Parts[0].Data) == "Desktop live acceptance"
	})
	checks = append(checks, "history restore writes exact bytes through real IPC adapter")
	var pngBytes bytes.Buffer
	if e = png.Encode(&pngBytes, image.NewRGBA(image.Rect(0, 0, 3, 2))); e != nil {
		t.Fatal(e)
	}
	imageID := submit("image", clip.Snapshot{Parts: []clip.Part{{Format: "image/png", Data: pngBytes.Bytes()}}})
	spin("image metadata", func() bool { return len(d.entries) == 2 })
	d.selectEntry(imageID)
	spin("image payload", func() bool {
		return d.current() != nil && d.current().id == imageID && d.current().image != nil && !d.live.detailBusy
	})
	checks = append(checks, "real image content decoded without fixture image")
	d.selectEntry(id)
	d.live.poll(d)
	d.selectEntry(imageID)
	spin("selection race", func() bool {
		return !d.live.detailBusy && d.live.detailKey == imageID && d.current() != nil && d.current().image != nil
	})
	if len(d.live.details) > 1 {
		t.Fatal("full payload cache grew beyond one record")
	}
	checks = append(checks, "late content reads cannot replace the selected record and full payload cache stays bounded")

	filePath := filepath.Join(private, "acceptance.txt")
	if e = os.WriteFile(filePath, []byte("file bytes"), 0600); e != nil {
		t.Fatal(e)
	}
	fileID := submit("file", clip.Snapshot{Files: []clip.File{{Name: "acceptance.txt", Path: filePath, Size: 10}}})
	spin("file metadata", func() bool { return len(d.entries) == 3 })
	d.selectEntry(fileID)
	spin("file detail", func() bool { return d.current() != nil && d.current().text == "acceptance.txt" && !d.live.detailBusy })
	checks = append(checks, "file preview uses real content-disposition name")
	d.kind = "图片"
	spin("server filter", func() bool { return !d.live.loading && len(d.entries) == 1 && d.entries[0].id == imageID })
	d.kind = ""
	spin("clear filter", func() bool { return !d.live.loading && len(d.entries) == 3 })
	checks = append(checks, "type filters query complete server history")

	for n := 0; n < 53; n++ {
		submit(fmt.Sprintf("page-%d", n), clip.Snapshot{Parts: []clip.Part{{Format: "text/plain", Data: []byte(fmt.Sprintf("row %d", n))}}})
	}
	spin("first page", func() bool { return len(d.entries) == 50 && d.live.cursor != "" && !d.live.loading })
	d.live.loadHistory(d, true)
	spin("next page", func() bool { return len(d.entries) == 56 && !d.live.loading })
	checks = append(checks, "cursor pagination keeps earlier loaded records")
	cfg := d.live.config.Config
	cfg.Sync.Automatic = false
	d.saveLiveConfig(cfg)
	spin("save config", func() bool { return !d.live.busy && !d.live.config.Config.Sync.Automatic })
	saved, e := client.Config(ctx)
	if e != nil || saved.Config.Sync.Automatic {
		t.Fatal("config not saved", e)
	}
	checks = append(checks, "configuration CAS persists existing settings")
	// Pair a second production daemon through the desktop action and both explicit approvals.
	peerHome := filepath.Join(private, "peer")
	if e = os.MkdirAll(peerHome, 0700); e != nil {
		t.Fatal(e)
	}
	if e = os.WriteFile(filepath.Join(peerHome, "config.toml"), []byte("[telemetry]\nenabled=false\n[sync]\nenabled=false\n"), 0600); e != nil {
		t.Fatal(e)
	}
	peer := exec.CommandContext(ctx, exe)
	peer.Env = append(env, "UNIC_HOME="+peerHome, "UNIC_RENDEZVOUS_URL=http://127.0.0.1:1")
	if e = peer.Start(); e != nil {
		t.Fatal(e)
	}
	defer func() { _ = peer.Process.Kill(); _ = peer.Wait() }()
	remote := api.NewClient(filepath.Join(peerHome, "unic.sock"))
	wait("peer", func() bool { _, e := remote.Status(ctx); return e == nil })
	var ticket string
	wait("peer ticket", func() bool { ticket, e = remote.ExportTicket(ctx); return e == nil })
	d.pairLive(ticket, "")
	spin("pending approval", func() bool { return !d.live.busy && len(d.pending) == 1 })
	reviewed := d.pending[0]
	d.approveLive(reviewed.id, true)
	spin("local approval", func() bool { return !d.live.busy })
	wait("remote pending", func() bool { p, e := remote.Pending(ctx); return e == nil && len(p) == 1 })
	requests, e := remote.Pending(ctx)
	if e != nil {
		t.Fatal(e)
	}
	r := requests[0]
	if _, e = remote.Approve(ctx, r.ID, api.Approval{Fingerprint: r.Fingerprint, Revision: r.Revision, Bindings: r.Bindings}); e != nil {
		t.Fatal(e)
	}
	spin("paired devices", func() bool { return len(d.devices) == 2 && d.live.join == nil })
	checks = append(checks, "desktop ticket pairing and exact fingerprint/revision approval connect two production services")
	// The snapshot revision remains authoritative for settings too.
	staleConfig := d.live.config.Config
	active, e := client.Config(ctx)
	if e != nil {
		t.Fatal(e)
	}
	raw := []byte("device_name = \"Concurrent name\"\n[telemetry]\nenabled=false\n[sync]\nenabled=false\n")
	if _, e = client.SaveConfig(ctx, active.Revision, raw); e != nil {
		t.Fatal(e)
	}
	d.saveLiveConfig(staleConfig)
	spin("config conflict", func() bool { return !d.live.busy && d.live.message != "" })
	actual, e := client.Config(ctx)
	if e != nil || actual.Config.DeviceName != "Concurrent name" {
		t.Fatal("stale config replaced current settings", e)
	}
	checks = append(checks, "stale configuration cannot replace concurrent settings")
	spin("post-conflict refresh", func() bool { return !d.live.loading && len(d.entries) == 56 })

	d.selectEntry(id)
	d.deleteLiveEntries([]string{id})
	spin("delete", func() bool {
		if d.live.busy {
			return false
		}
		for _, it := range d.entries {
			if it.id == id {
				return false
			}
		}
		return true
	})
	if _, e = client.GetHistory(ctx, id); e == nil {
		t.Fatal("deleted record remains")
	}
	checks = append(checks, "revision-checked deletion updates history")
	_ = cmd.Process.Kill()
	_ = cmd.Wait()
	spin("disconnect", func() bool { return !d.live.connected })
	checks = append(checks, "service disconnect is visible without setup or unlock")
}
