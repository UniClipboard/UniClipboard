package desktopui

import (
	"bufio"
	"bytes"
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"fmt"
	"io"
	"net/http"
	"os"
	"os/exec"
	"path/filepath"
	"slices"
	"strings"
	"sync"
	"sync/atomic"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"

	proof "github.com/uniclipboard/unic/app/desktop/internal/e2eproof"
	"github.com/uniclipboard/unic/engine"
	"github.com/uniclipboard/unic/internal/clip"
	api "github.com/uniclipboard/unic/internal/localapi"
)

type browsingTransport func(*http.Request) (*http.Response, error)

func (f browsingTransport) RoundTrip(r *http.Request) (*http.Response, error) { return f(r) }

// Gate the production SSE response to reproduce a missed notification while
// the connection itself remains alive. No replacement service is involved.
type browsingEvents struct {
	reader   *bufio.Reader
	closer   io.Closer
	suppress *atomic.Bool
	dropped  *atomic.Int64
	pending  []byte
}

func (b *browsingEvents) Close() error { return b.closer.Close() }
func (b *browsingEvents) Read(p []byte) (int, error) {
	for len(b.pending) == 0 {
		line, e := b.reader.ReadString('\n')
		if e != nil {
			return 0, e
		}
		if strings.HasPrefix(line, "data: ") && b.suppress.Load() {
			b.dropped.Add(1)
			continue
		}
		b.pending = []byte(line)
	}
	n := copy(p, b.pending)
	b.pending = b.pending[n:]
	return n, nil
}

// Failure inventory is recorded before implementation in the execution plan.
// Exercise real unicd and the production Desktop binding, without native UI.
func scenarioIncrementalBrowsing(t *testing.T) {
	ctx, cancel := context.WithTimeout(context.Background(), 4*time.Minute)
	defer cancel()
	root, e := filepath.Abs("../../../..")
	if e != nil {
		t.Fatal(e)
	}
	private, e := os.MkdirTemp("/tmp", "unic-browsing-")
	if e != nil {
		t.Fatal(e)
	}
	private, e = filepath.EvalSymlinks(private)
	if e != nil {
		t.Fatal(e)
	}
	defer func() {
		if e := os.RemoveAll(private); e != nil {
			t.Error(e)
		}
	}()
	hash := func(b []byte) string { s := sha256.Sum256(b); return hex.EncodeToString(s[:]) }
	sources := map[string]string{}
	for _, dir := range []string{"engine", "internal/store", "internal/localapi", "app/service", "app/desktop"} {
		if e = filepath.WalkDir(filepath.Join(root, dir), func(p string, d os.DirEntry, e error) error {
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
		}); e != nil {
			t.Fatal(e)
		}
	}
	checks := []string{}
	succeeded := false
	receipt := map[string]any{"source_sha256": sources, "native_ui": false, "renderer": true}
	defer func() {
		receipt["passed"] = succeeded && !t.Failed()
		receipt["checks"] = checks
		if path := os.Getenv("UNIC_DESKTOP_E2E_CASE_RECEIPT"); path != "" {
			b, _ := json.MarshalIndent(receipt, "", "  ")
			if e := os.MkdirAll(filepath.Dir(path), 0700); e != nil {
				t.Error(e)
			}
			if e := os.WriteFile(path, b, 0600); e != nil {
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
	if e = os.MkdirAll(filepath.Join(home, "inbox", "file-cache"), 0700); e != nil {
		t.Fatal(e)
	}
	// Offline import is a distinct production insertion path; reopen the same profile afterwards.
	importer, e := engine.New(ctx, engine.Options{DataDir: home, MigrationMode: true})
	if e != nil {
		t.Fatal(e)
	}
	imported := "Imported migration marker"
	_, e = importer.ImportHistory(ctx, engine.HistoryImportRequest{Source: "acceptance", RecordID: "1", CreatedAtMs: time.Now().Add(-48 * time.Hour).UnixMilli(), Content: engine.ContentInput{Representations: []engine.RepresentationInput{{MIME: "text/plain", Size: int64(len(imported)), Reader: strings.NewReader(imported)}}}})
	if e != nil {
		t.Fatal(e)
	}
	if e = importer.Close(ctx); e != nil {
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
	var proc *exec.Cmd
	var logs bytes.Buffer
	start := func() {
		t.Helper()
		proc = exec.CommandContext(ctx, exe)
		proc.Env = append(env, "UNIC_HOME="+home, "UNIC_RENDEZVOUS_URL=http://127.0.0.1:1")
		proc.Stdout = &logs
		proc.Stderr = &logs
		if e := proc.Start(); e != nil {
			t.Fatal(e)
		}
	}
	stop := func() {
		if proc != nil {
			_ = proc.Process.Kill()
			_ = proc.Wait()
			proc = nil
		}
	}
	defer stop()
	start()
	client := api.NewClient(filepath.Join(home, "unic.sock"))
	wait := func(label string, f func() bool) {
		t.Helper()
		deadline := time.Now().Add(25 * time.Second)
		for time.Now().Before(deadline) {
			if f() {
				return
			}
			time.Sleep(20 * time.Millisecond)
		}
		t.Fatalf("timeout: %s", label)
	}
	wait("service", func() bool { _, e := client.Status(ctx); return e == nil })

	ids := []string{}
	for n := 0; n < 620; n++ {
		out, err := client.Submit(ctx, api.SubmitInput{RequestID: fmt.Sprintf("browse-%d", n), Snapshot: clip.Snapshot{Parts: []clip.Part{{Format: "text/plain", Data: []byte(fmt.Sprintf("browsing row %03d", n))}}}})
		if err != nil {
			t.Fatal(err)
		}
		ids = append(ids, string(out.HistoryID))
	}
	var suppress atomic.Bool
	var dropped atomic.Int64
	var mu sync.Mutex
	var timeline []map[string]any
	started := time.Now()
	client.WrapTransport(func(next http.RoundTripper) http.RoundTripper {
		return browsingTransport(func(r *http.Request) (*http.Response, error) {
			mu.Lock()
			timeline = append(timeline, map[string]any{"ms": time.Since(started).Milliseconds(), "method": r.Method, "path": r.URL.Path})
			mu.Unlock()
			response, err := next.RoundTrip(r)
			if err == nil && r.URL.Path == "/v1/events" {
				response.Body = &browsingEvents{reader: bufio.NewReader(response.Body), closer: response.Body, suppress: &suppress, dropped: &dropped}
			}
			return response, err
		})
	})
	defer func() { mu.Lock(); receipt["request_timeline"] = slices.Clone(timeline); mu.Unlock() }()
	desktop := NewDesktop(ctx, client)
	defer desktop.Close()
	d := desktop.d
	spin := func(label string, ready func() bool) {
		t.Helper()
		wait(label, func() bool { d.live.poll(d); return ready() })
	}
	spin("initial page", func() bool { return d.live.loaded && !d.live.loading && len(d.entries) == 50 })
	selected := d.entries[20].id
	d.selectEntry(selected)
	d.detailOpen, d.inspector = true, true
	spin("selected detail", func() bool { return d.current() != nil && d.current().text != "" })
	selectedText := d.current().text
	serial := d.live.appliedSerial
	searches := func() int {
		mu.Lock()
		defer mu.Unlock()
		n := 0
		for _, r := range timeline {
			if r["path"] == "/v1/history/search" {
				n++
			}
		}
		return n
	}
	idleSearches := searches()
	idle := time.Now().Add(11500 * time.Millisecond)
	for time.Now().Before(idle) {
		d.live.poll(d)
		time.Sleep(20 * time.Millisecond)
	}
	if d.live.appliedSerial != serial || searches() != idleSearches {
		t.Fatal("unchanged idle history reloaded on the five-second schedule")
	}
	checks = append(checks, "unchanged idle list: no search reapplication across 11.5 seconds")
	// Exercise the real list renderer and its scroll-triggered pager.
	tt := ui.NewTester(d.view, 1100, 720)
	proof.Capture(t, tt)
	list, ok := tt.AccessByRole(ui.RoleList, "历史条目")
	if !ok {
		t.Fatal("history list missing")
	}
	box := list.Bounds
	tt.Scroll(box.X+box.W/2, box.Y+box.H/2, 0, 100000)
	tt.Frame()
	spin("automatic page", func() bool { tt.Frame(); return !d.live.loading && len(d.entries) > 50 })
	if _, ok := tt.AccessByRole(ui.RoleButton, "加载更早的历史"); ok {
		t.Fatal("manual pager remains")
	}
	checks = append(checks, "software renderer scroll requests the next page without a manual button")
	seen := map[string]bool{}
	for _, v := range d.entries {
		seen[v.id] = true
	}
	for attempts := 0; len(seen) < 621 && attempts < 20; attempts++ {
		serial := d.live.appliedSerial
		for n := 0; n < 20; n++ {
			d.live.loadHistory(d, true)
		}
		spin("coalesced page", func() bool { return !d.live.loading && d.live.appliedSerial > serial })
		window := map[string]bool{}
		for _, v := range d.entries {
			if window[v.id] {
				t.Fatal("duplicate history", v.id)
			}
			window[v.id] = true
			seen[v.id] = true
		}
		if len(d.entries) > 500 || len(d.live.items) > 501 {
			t.Fatal("unbounded history window", len(d.entries), len(d.live.items))
		}
	}
	for _, id := range ids {
		if !seen[id] {
			t.Fatal("missing history", id)
		}
	}
	if d.selected != selected || d.current() == nil || d.current().text != selectedText || !d.detailOpen || !d.inspector {
		t.Fatal("paging lost selected detail")
	}
	checks = append(checks, "621 fixture records traversed exactly once per window; at most 500 rows plus one selected detail")
	d.live.loadPrevious(d)
	spin("newer window", func() bool { return !d.live.loading })
	if d.current() == nil || d.current().text != selectedText {
		t.Fatal("backward paging lost selected detail")
	}
	checks = append(checks, "newer browsing reloads evicted rows while retaining selection")
	// Reconciliation must retain a surviving middle row after insertion.
	d.list.ScrollTo(d.entries[200].id)
	tt.Frame()
	first, _ := d.list.VisibleRange()
	if first >= 0 && first < len(d.entries) {
		anchor := d.entries[first].id
		for n := 0; n < 60; n++ {
			_, e := client.Submit(ctx, api.SubmitInput{RequestID: fmt.Sprintf("browse-new-%d", n), Snapshot: clip.Snapshot{Parts: []clip.Part{{Format: "text/plain", Data: []byte(fmt.Sprintf("new browsing row %d", n))}}}})
			if e != nil {
				t.Fatal(e)
			}
		}
		serial := d.live.appliedSerial
		spin("insert reconciliation", func() bool { return !d.live.loading && d.live.appliedSerial > serial })
		tt.Frame()
		now, _ := d.list.VisibleRange()
		if now < 0 || now >= len(d.entries) || d.entries[now].id != anchor {
			t.Fatalf("insertion moved scroll anchor: before=%d after=%d offset=%d want=%s", first, now, d.live.windowOffset, anchor)
		}
		checks = append(checks, "sixty new history entries preserve the visible keyed scroll anchor")
	}
	// Drop a real history notification and advance only the recovery deadline.
	suppress.Store(true)
	lost := d.entries[100].id
	lostItem, e := client.GetHistory(ctx, lost)
	if e != nil {
		t.Fatal(e)
	}
	serial = d.live.appliedSerial
	beforeDropped := dropped.Load()
	if e = client.DeleteHistory(ctx, lost, lostItem.Revision); e != nil {
		t.Fatal(e)
	}
	wait("suppressed history event", func() bool { return dropped.Load() > beforeDropped })
	if d.live.appliedSerial != serial {
		t.Fatal("missed-event test refreshed before recovery")
	}
	d.live.nextRefresh = time.Time{}
	spin("missed-event revision recovery", func() bool {
		_, ok := d.live.items[lost]
		return !ok && !d.live.loading && d.live.appliedSerial > serial
	})
	if d.selected != selected || d.current() == nil || d.current().text != selectedText {
		t.Fatal("recovery lost selection")
	}
	suppress.Store(false)
	checks = append(checks, "dropped SSE notification recovers through the bounded revision probe and preserves selection")
	item, err := client.GetHistory(ctx, selected)
	if err != nil {
		t.Fatal(err)
	}
	if err = client.DeleteHistory(ctx, selected, item.Revision); err != nil {
		t.Fatal(err)
	}
	spin("selected deletion", func() bool { _, ok := d.live.items[selected]; return !ok && !d.live.loading })
	if d.selected == selected || d.quickLook {
		t.Fatal("deleted selection remains active")
	}
	checks = append(checks, "history event removes deleted selection explicitly")
	if err := tt.Click("搜索历史"); err != nil {
		t.Fatal(err)
	}
	tt.Type("row 179")
	d.live.poll(d)
	tt.Key(ui.Cmd, ui.KeyA)
	tt.Type("row 001")
	if d.query != "row 001" {
		t.Fatalf("search input did not change query: %q", d.query)
	}
	spin("query cancellation", func() bool {
		return !d.live.loading && len(d.entries) == 1 && strings.Contains(d.entries[0].preview, "row 001")
	})
	checks = append(checks, "query generation cancels obsolete searches")
	selected = d.selected
	stop()
	start()
	spin("service restart", func() bool {
		return d.live.connected && !d.live.loading && len(d.entries) == 1 && d.selected == selected
	})
	// Force a post-disconnect recovery, including cursor instance renewal.
	serial = d.live.appliedSerial
	select {
	case d.live.recovery <- struct{}{}:
	default:
	}
	spin("reconnect reconciliation", func() bool { return !d.live.loading && d.live.appliedSerial > serial })
	if d.query != "row 001" || len(d.entries) != 1 || d.selected != selected {
		t.Fatal("restart lost query or selection")
	}
	checks = append(checks, "real service restart and reconnect retain the query and selected result")
	succeeded = true
}
