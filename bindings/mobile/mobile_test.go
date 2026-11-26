package mobile

import (
	"bytes"
	"context"
	"database/sql"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"net"
	"os"
	"path/filepath"
	"slices"
	"strings"
	"sync"
	"testing"
	"time"

	_ "modernc.org/sqlite"

	"github.com/uniclipboard/unic/engine"
	"github.com/uniclipboard/unic/internal/plugins/tailcat"
	"github.com/uniclipboard/unic/internal/v1migration/v1fixture"
)

type testSink struct {
	mu     sync.Mutex
	events []engine.Event
	resync int
}

func (s *testSink) OnEvent(eventJSON string) {
	var ev engine.Event
	if err := json.Unmarshal([]byte(eventJSON), &ev); err == nil {
		s.mu.Lock()
		s.events = append(s.events, ev)
		s.mu.Unlock()
	}
}

func (s *testSink) OnResyncRequired(_ string) {
	s.mu.Lock()
	s.resync++
	s.mu.Unlock()
}

func (s *testSink) countKind(kind engine.EventKind) int {
	s.mu.Lock()
	defer s.mu.Unlock()
	n := 0
	for _, ev := range s.events {
		if ev.Kind == kind {
			n++
		}
	}
	return n
}

type memFileStream struct {
	r   *bytes.Reader
	err error
}

func (s *memFileStream) ReadChunk(maxBytes int) ([]byte, error) {
	if s.err != nil {
		return nil, s.err
	}
	if maxBytes <= 0 {
		maxBytes = 4
	}
	buf := make([]byte, maxBytes)
	n, err := s.r.Read(buf)
	if n > 0 {
		return buf[:n], nil
	}
	if errors.Is(err, io.EOF) {
		return nil, nil
	}
	return nil, err
}

func (s *memFileStream) Close() error { return nil }

type memFileOpener struct {
	data    []byte
	version string
}

func (o *memFileOpener) Open() (FileStream, error) {
	return &memFileStream{r: bytes.NewReader(o.data)}, nil
}
func (o *memFileOpener) Size() int64     { return int64(len(o.data)) }
func (o *memFileOpener) Version() string { return o.version }

type memSecretProvider struct {
	kek []byte
	err error
}

func (p *memSecretProvider) ReadSecret(_, account string) ([]byte, error) {
	if p.err != nil {
		return nil, p.err
	}
	if account == "kek:v1:profile:default" {
		return bytes.Clone(p.kek), nil
	}
	return nil, errors.New("not found")
}

type progressCollector struct {
	mu     sync.Mutex
	stages []string
	cancel *CancelToken
	stopAt int
}

func (p *progressCollector) OnProgress(stage string, processed int, _ int) {
	p.mu.Lock()
	p.stages = append(p.stages, stage)
	cancel := p.cancel
	stopAt := p.stopAt
	p.mu.Unlock()
	if cancel != nil && stopAt > 0 && stage == "import" && processed >= stopAt {
		cancel.Cancel()
	}
}

func tempDirReal(t *testing.T) string {
	t.Helper()
	dir, err := resolveDirPath(t.TempDir())
	if err != nil {
		t.Fatal(err)
	}
	return dir
}

func decodeMobileErr(t *testing.T, err error) mobileError {
	t.Helper()
	if err == nil {
		t.Fatal("expected error, got nil")
	}
	var me mobileError
	if jerr := json.Unmarshal([]byte(err.Error()), &me); jerr != nil {
		t.Fatalf("error is not structured mobileError JSON: %v (%v)", err, jerr)
	}
	return me
}

func TestEngineLifecycleCommitHistorySearchAndAntiLoopback(t *testing.T) {
	dataDir := filepath.Join(tempDirReal(t), "unic-mobile")
	cfg := map[string]any{
		"data_dir":    dataDir,
		"device_name": "Pixel Test",
		"host_capabilities": map[string]any{
			"read_formats":  []string{"text/plain", "text/html"},
			"write_formats": []string{"text/plain"},
			"read_files":    true,
			"write_files":   true,
			"observe":       "available",
			"apply":         "available",
		},
	}
	cfgBytes, _ := json.Marshal(cfg)

	eng, err := NewEngine(string(cfgBytes))
	if err != nil {
		t.Fatalf("NewEngine: %v", err)
	}
	closed := false
	defer func() {
		if !closed {
			_ = eng.Close(nil)
		}
	}()

	sink := &testSink{}
	snapJSON, err := eng.Subscribe(sink)
	if err != nil {
		t.Fatalf("Subscribe: %v", err)
	}
	var snap engine.State
	if err := json.Unmarshal([]byte(snapJSON), &snap); err != nil {
		t.Fatalf("unmarshal snapshot: %v", err)
	}
	if snap.Self.Name != "Pixel Test" || snap.Lifecycle != engine.LifecycleCreated {
		t.Fatalf("unexpected initial snapshot: %+v", snap.Self)
	}

	if err := eng.Start(nil); err != nil {
		t.Fatalf("Start: %v", err)
	}
	if err := eng.SetRuntimeState("suspended"); err != nil {
		t.Fatalf("SetRuntimeState(suspended): %v", err)
	}
	if err := eng.SetRuntimeState("active"); err != nil {
		t.Fatalf("SetRuntimeState(active): %v", err)
	}

	// Import UTF-8 text with emoji/CJK and commit it locally.
	handleJSON, err := eng.ImportText("你好世界 🎉 https://example.com", "", "")
	if err != nil {
		t.Fatalf("ImportText: %v", err)
	}
	var handle engine.ContentHandle
	if err := json.Unmarshal([]byte(handleJSON), &handle); err != nil {
		t.Fatal(err)
	}
	commitJSON, err := eng.CommitContent(nil, `{"request_id":"req-1","lease_id":"`+string(handle.LeaseID)+`"}`)
	if err != nil {
		t.Fatalf("CommitContent: %v", err)
	}
	_ = eng.ReleaseContent(string(handle.LeaseID))
	var sub engine.SubmissionResult
	if err := json.Unmarshal([]byte(commitJSON), &sub); err != nil {
		t.Fatal(err)
	}
	if sub.Phase != engine.SubmissionCommitted || sub.HistoryID == "" {
		t.Fatalf("unexpected submission: %+v", sub)
	}

	// Read bounded text preview without cutting a multi-byte UTF-8 rune.
	prevJSON, err := eng.ReadHistoryTextPreview(string(sub.HistoryID), 14)
	if err != nil {
		t.Fatalf("ReadHistoryTextPreview: %v", err)
	}
	var prev textPreviewDTO
	if err := json.Unmarshal([]byte(prevJSON), &prev); err != nil {
		t.Fatal(err)
	}
	if !prev.Truncated || !strings.HasPrefix("你好世界 🎉 https://example.com", prev.Text) || prev.Text == "" {
		t.Fatalf("unexpected preview: %+v", prev)
	}

	// Search history & run builtin.text/upper action.
	searchJSON, err := eng.SearchHistory(nil, `{"query":{"text_terms":["example"]},"limit":10}`)
	if err != nil {
		t.Fatalf("SearchHistory: %v", err)
	}
	var searchPage engine.HistorySearchPage
	if err := json.Unmarshal([]byte(searchJSON), &searchPage); err != nil {
		t.Fatal(err)
	}
	if len(searchPage.Hits) != 1 || searchPage.Hits[0].Item.ID != sub.HistoryID {
		t.Fatalf("unexpected search hits: %+v", searchPage.Hits)
	}

	// Anti-loopback baseline + BeginApplyHistory + ReportApplyResult.
	baseHandleJSON, err := eng.ImportText("initial", "", "")
	if err != nil {
		t.Fatal(err)
	}
	var baseHandle engine.ContentHandle
	_ = json.Unmarshal([]byte(baseHandleJSON), &baseHandle)
	obsResJSON, err := eng.ObserveClipboard(`{"epoch":"ep1","sequence":"1","mode":"baseline","lease_id":"` + string(baseHandle.LeaseID) + `"}`)
	if err != nil {
		t.Fatalf("ObserveClipboard baseline: %v", err)
	}
	_ = eng.ReleaseContent(string(baseHandle.LeaseID))
	if !strings.Contains(obsResJSON, `"baseline"`) {
		t.Fatalf("expected baseline disposition, got %s", obsResJSON)
	}

	ticketJSON, err := eng.BeginApplyHistory(string(sub.HistoryID), "ep1", "1", true)
	if err != nil {
		t.Fatalf("BeginApplyHistory: %v", err)
	}
	var ticket struct {
		Token   string `json:"token"`
		LeaseID string `json:"lease_id"`
	}
	_ = json.Unmarshal([]byte(ticketJSON), &ticket)
	if ticket.Token == "" || ticket.LeaseID == "" {
		t.Fatalf("invalid ticket: %s", ticketJSON)
	}
	appliedLeaseJSON, err := eng.ImportText("你好世界 🎉 https://example.com", "", "")
	if err != nil {
		t.Fatal(err)
	}
	var appliedHandle engine.ContentHandle
	_ = json.Unmarshal([]byte(appliedLeaseJSON), &appliedHandle)
	if err := eng.ReportApplyResult(`{"token":"` + ticket.Token + `","outcome":"applied","baseline_lease_id":"` + string(appliedHandle.LeaseID) + `","epoch":"ep1","sequence":"2"}`); err != nil {
		t.Fatalf("ReportApplyResult: %v", err)
	}
	_ = eng.ReleaseContent(string(appliedHandle.LeaseID))
	_ = eng.ReleaseContent(ticket.LeaseID)

	// Streaming FileOpener import & export.
	opener := &memFileOpener{data: []byte("hello-stream-file-payload"), version: "v1"}
	fileHandleJSON, err := eng.ImportFileSource("notes.txt", "text/plain", true, opener)
	if err != nil {
		t.Fatalf("ImportFileSource: %v", err)
	}
	var fileHandle engine.ContentHandle
	_ = json.Unmarshal([]byte(fileHandleJSON), &fileHandle)
	destPath := filepath.Join(tempDirReal(t), "exported.txt")
	written, err := eng.ExportPartToFile(string(fileHandle.LeaseID), string(fileHandle.Content.Parts[0].ID), destPath)
	if err != nil || written != int64(len(opener.data)) {
		t.Fatalf("ExportPartToFile: %d, %v", written, err)
	}
	_ = eng.ReleaseContent(string(fileHandle.LeaseID))
	gotBytes, err := os.ReadFile(destPath)
	if err != nil || !bytes.Equal(gotBytes, opener.data) {
		t.Fatalf("exported content mismatch: %v", err)
	}

	// UpdateSettings persists across Engine restarts.
	stJSON, err := eng.GetState()
	if err != nil {
		t.Fatal(err)
	}
	var curState engine.State
	_ = json.Unmarshal([]byte(stJSON), &curState)
	curState.Settings.Value.DeviceName = "Pixel Renamed"
	updReq, _ := json.Marshal(map[string]any{
		"expected_revision": curState.Settings.Revision,
		"value":             curState.Settings.Value,
	})
	if _, err := eng.UpdateSettings(string(updReq)); err != nil {
		t.Fatalf("UpdateSettings: %v", err)
	}

	deadline := time.Now().Add(2 * time.Second)
	for time.Now().Before(deadline) && sink.countKind(engine.EventHistory) == 0 {
		time.Sleep(10 * time.Millisecond)
	}
	if sink.countKind(engine.EventHistory) == 0 {
		t.Fatal("expected EventHistory on EventSink")
	}

	if err := eng.Stop(nil); err != nil {
		t.Fatalf("Stop: %v", err)
	}
	if err := eng.Close(nil); err != nil {
		t.Fatalf("Close: %v", err)
	}
	closed = true

	// Reopen and verify persisted settings + history.
	eng2, err := NewEngine(string(cfgBytes))
	if err != nil {
		t.Fatalf("reopen NewEngine: %v", err)
	}
	defer func() { _ = eng2.Close(nil) }()
	st2JSON, err := eng2.GetState()
	if err != nil {
		t.Fatal(err)
	}
	var st2 engine.State
	_ = json.Unmarshal([]byte(st2JSON), &st2)
	if st2.Settings.Value.DeviceName != "Pixel Renamed" {
		t.Fatalf("settings not persisted: %q", st2.Settings.Value.DeviceName)
	}
	pageJSON, err := eng2.ListHistory(`{"limit":10}`)
	if err != nil {
		t.Fatal(err)
	}
	var page engine.HistoryPage
	_ = json.Unmarshal([]byte(pageJSON), &page)
	if len(page.Items) != 1 {
		t.Fatalf("expected 1 persisted item, got %d", len(page.Items))
	}
}

func TestSetupFlowDetectionConflictCancelResumeAndBypass(t *testing.T) {
	base := tempDirReal(t)
	v1Root, _, f, err := v1fixture.Build(filepath.Join(base, "v1"))
	if err != nil {
		t.Fatal(err)
	}
	target := filepath.Join(base, "unic-v2")
	missingDefault := filepath.Join(base, "does-not-exist")

	// 1. Fresh install with missing default v1 dir -> none, incomplete.
	sess, err := OpenSetupFlow(target, missingDefault)
	if err != nil {
		t.Fatalf("OpenSetupFlow: %v", err)
	}
	detJSON, err := sess.DetectSource("")
	if err != nil {
		t.Fatal(err)
	}
	if !strings.Contains(detJSON, `"none"`) {
		t.Fatalf("expected none, got %s", detJSON)
	}

	// 1b. Existing empty default dir without vault/ -> none (not invalid).
	emptyDefault := filepath.Join(base, "empty-default")
	if err := os.MkdirAll(emptyDefault, 0o700); err != nil {
		t.Fatal(err)
	}
	sessEmpty, err := OpenSetupFlow(filepath.Join(base, "unic-v2-empty"), emptyDefault)
	if err != nil {
		t.Fatalf("OpenSetupFlow empty default: %v", err)
	}
	if emptyDetJSON, err := sessEmpty.DetectSource(""); err != nil || !strings.Contains(emptyDetJSON, `"none"`) {
		t.Fatalf("expected none for empty default dir, got %q, err=%v", emptyDetJSON, err)
	}
	if explicitDetJSON, err := sessEmpty.DetectSource(emptyDefault); err != nil || !strings.Contains(explicitDetJSON, `"invalid"`) {
		t.Fatalf("expected invalid for explicit empty dir, got %q, err=%v", explicitDetJSON, err)
	}

	// 2. Invalid directory rejected.
	badDir := filepath.Join(base, "not-v1")
	if err := os.MkdirAll(badDir, 0o700); err != nil {
		t.Fatal(err)
	}
	if _, err := sess.SelectSource(badDir); decodeMobileErr(t, err).Code != string(engine.CodeInvalidArgument) {
		t.Fatal("expected invalid_argument for invalid v1 source")
	}

	// 3. Detect valid v1 fixture directory.
	detJSON, err = sess.DetectSource(v1Root)
	if err != nil {
		t.Fatal(err)
	}
	if !strings.Contains(detJSON, `"found"`) {
		t.Fatalf("expected found, got %s", detJSON)
	}

	// 4. Exclusive target ownership conflict when Engine is already open on target.
	cfgBytes, _ := json.Marshal(map[string]any{"data_dir": target})
	runningEng, err := NewEngine(string(cfgBytes))
	if err != nil {
		t.Fatal(err)
	}
	_, runErr := sess.Run(nil, []byte(f.Passphrase), nil, nil)
	if me := decodeMobileErr(t, runErr); me.Code != string(engine.CodeInUse) {
		t.Fatalf("expected in_use conflict, got %+v", me)
	}
	_ = runningEng.Close(nil)

	// 5. Wrong passphrase returns permission_denied (retryable in UI).
	_, runErr = sess.Run(nil, []byte("wrong-passphrase"), nil, nil)
	if me := decodeMobileErr(t, runErr); me.Code != string(engine.CodePermissionDenied) {
		t.Fatalf("expected permission_denied, got %+v", me)
	}

	// 6. Cancel mid-import retains committed records without marking setup complete.
	tok := NewCancelToken()
	prog := &progressCollector{cancel: tok, stopAt: 2}
	resJSON, err := sess.Run(tok, nil, &memSecretProvider{kek: f.KEK}, prog)
	if err != nil {
		t.Fatalf("cancelled Run returned error instead of cancelled report: %v", err)
	}
	var cancelRes migrationRunResult
	_ = json.Unmarshal([]byte(resJSON), &cancelRes)
	if cancelRes.Status != "cancelled" || cancelRes.Imported == 0 {
		t.Fatalf("expected cancelled with committed records, got %+v", cancelRes)
	}

	// 7. Resuming skips already-imported records and reports partial (fixture has 1 intentionally broken row).
	resJSON2, err := sess.Run(nil, []byte(f.Passphrase), nil, &progressCollector{})
	if err != nil {
		t.Fatalf("resume Run: %v", err)
	}
	var resumeRes migrationRunResult
	_ = json.Unmarshal([]byte(resJSON2), &resumeRes)
	if resumeRes.Status != "partial" || resumeRes.Skipped != cancelRes.Imported || resumeRes.Failed != 1 {
		t.Fatalf("unexpected resume result: %+v (prev imported=%d)", resumeRes, cancelRes.Imported)
	}

	// Setup remains incomplete until explicit MarkComplete().
	stJSON, _ := sess.GetState()
	if strings.Contains(stJSON, `"complete":true`) {
		t.Fatalf("setup must not mark complete before explicit entry: %s", stJSON)
	}
	if err := sess.MarkComplete(); err != nil {
		t.Fatal(err)
	}
	stJSON, _ = sess.GetState()
	if !strings.Contains(stJSON, `"complete":true`) {
		t.Fatalf("expected complete:true after MarkComplete, got %s", stJSON)
	}

	// 8. Existing initialized v2 database without mobile-setup.json bypasses setup.
	if err := os.Remove(sess.markerPath()); err != nil {
		t.Fatal(err)
	}
	sessReopened, err := OpenSetupFlow(target, missingDefault)
	if err != nil {
		t.Fatal(err)
	}
	stReopened, _ := sessReopened.GetState()
	if !strings.Contains(stReopened, `"complete":true`) {
		t.Fatalf("existing v2 DB without marker must bypass setup: %s", stReopened)
	}
}

type loopbackTransport struct {
	mu   sync.Mutex
	addr string
	ln   net.Listener
}

func newLoopbackTransport(t *testing.T) *loopbackTransport {
	t.Helper()
	ln, err := net.Listen("tcp", "127.0.0.1:0")
	if err != nil {
		t.Fatal(err)
	}
	tr := &loopbackTransport{ln: ln, addr: ln.Addr().String()}
	t.Cleanup(func() {
		tr.mu.Lock()
		defer tr.mu.Unlock()
		if tr.ln != nil {
			_ = tr.ln.Close()
		}
	})
	return tr
}

func (tr *loopbackTransport) Listen(context.Context) (net.Listener, error) {
	tr.mu.Lock()
	defer tr.mu.Unlock()
	if tr.ln != nil {
		ln := tr.ln
		tr.ln = nil
		return ln, nil
	}
	return net.Listen("tcp", tr.addr)
}

func (tr *loopbackTransport) Dial(ctx context.Context, a string) (net.Conn, error) {
	var d net.Dialer
	return d.DialContext(ctx, "tcp", a)
}

func (tr *loopbackTransport) Addr() string { return tr.addr }

func (tr *loopbackTransport) Binding(addr string) ([]byte, error) {
	return []byte("loopback\x00" + addr), nil
}

func (tr *loopbackTransport) Start() error { return nil }
func (tr *loopbackTransport) Close() error { return nil }

func eventuallyMobile(t *testing.T, what string, fn func() bool) {
	t.Helper()
	deadline := time.Now().Add(5 * time.Second)
	for time.Now().Before(deadline) {
		if fn() {
			return
		}
		time.Sleep(15 * time.Millisecond)
	}
	t.Fatalf("timed out waiting for %s", what)
}

func TestTwoEnginesPairApproveAndSync(t *testing.T) {
	root := t.TempDir()
	cfgA, _ := json.Marshal(map[string]any{
		"data_dir":    filepath.Join(root, "node-a"),
		"device_name": "iPhone A",
	})
	cfgB, _ := json.Marshal(map[string]any{
		"data_dir":    filepath.Join(root, "node-b"),
		"device_name": "Pixel B",
	})

	engA, err := newEngineWithBackend(string(cfgA), tailcat.NewOn(newLoopbackTransport(t)))
	if err != nil {
		t.Fatalf("newEngineWithBackend A: %v", err)
	}
	defer func() { _ = engA.Close(nil) }()

	engB, err := newEngineWithBackend(string(cfgB), tailcat.NewOn(newLoopbackTransport(t)))
	if err != nil {
		t.Fatalf("newEngineWithBackend B: %v", err)
	}
	defer func() { _ = engB.Close(nil) }()

	sinkA := &testSink{}
	sinkB := &testSink{}
	snapAJSON, err := engA.Subscribe(sinkA)
	if err != nil {
		t.Fatal(err)
	}
	snapBJSON, err := engB.Subscribe(sinkB)
	if err != nil {
		t.Fatal(err)
	}
	var snapA, snapB engine.State
	_ = json.Unmarshal([]byte(snapAJSON), &snapA)
	_ = json.Unmarshal([]byte(snapBJSON), &snapB)

	if err := engA.Start(nil); err != nil {
		t.Fatalf("Start A: %v", err)
	}
	if err := engB.Start(nil); err != nil {
		t.Fatalf("Start B: %v", err)
	}

	// 1. Export ticket from A and join from B.
	tkJSON, err := engA.ExportPairingTicket(nil)
	if err != nil {
		t.Fatalf("ExportPairingTicket: %v", err)
	}
	var tk struct {
		Ticket string `json:"ticket"`
	}
	if err := json.Unmarshal([]byte(tkJSON), &tk); err != nil || tk.Ticket == "" {
		t.Fatalf("invalid exported ticket json: %s", tkJSON)
	}
	joinReq, _ := json.Marshal(map[string]any{
		"request_id": "join-b-to-a",
		"ticket":     tk.Ticket,
	})
	opID, err := engB.JoinByTicket(nil, string(joinReq))
	if err != nil {
		t.Fatalf("JoinByTicket: %v", err)
	}
	eventuallyMobile(t, "join operation succeeded", func() bool {
		opJSON, err := engB.GetOperation(opID)
		if err != nil {
			t.Fatal(err)
		}
		var op engine.Operation
		_ = json.Unmarshal([]byte(opJSON), &op)
		if op.Phase == engine.PhaseFailed {
			t.Fatalf("join failed: %+v", op.Error)
		}
		return op.Phase == engine.PhaseSucceeded
	})

	// 2. Both sides approve the pending device and its reviewed bindings.
	approvePeer := func(owner *Engine, peerID engine.DeviceID) {
		var peer engine.Device
		eventuallyMobile(t, "pending device "+string(peerID), func() bool {
			raw, err := owner.ListDevices()
			if err != nil {
				t.Fatal(err)
			}
			var ds []engine.Device
			_ = json.Unmarshal([]byte(raw), &ds)
			for _, d := range ds {
				if d.ID == string(peerID) && len(d.Bindings) > 0 {
					peer = d
					return true
				}
			}
			return false
		})
		bindings := make([]map[string]any, len(peer.Bindings))
		for i, b := range peer.Bindings {
			bindings[i] = map[string]any{
				"id":                b.ID,
				"expected_revision": b.Revision,
			}
		}
		req, _ := json.Marshal(map[string]any{
			"id":                   peer.ID,
			"expected_revision":    peer.Revision,
			"expected_fingerprint": peer.Fingerprint,
			"bindings":             bindings,
		})
		if _, err := owner.ApproveDevice(string(req)); err != nil {
			t.Fatalf("ApproveDevice(%s): %v", peer.ID, err)
		}
	}
	approvePeer(engA, snapB.Self.ID)
	approvePeer(engB, snapA.Self.ID)

	eventuallyMobile(t, "peers connected", func() bool {
		raw, err := engA.ListDevices()
		if err != nil {
			return false
		}
		var ds []engine.Device
		_ = json.Unmarshal([]byte(raw), &ds)
		for _, d := range ds {
			if d.ID == string(snapB.Self.ID) {
				for _, b := range d.Bindings {
					if b.Connection == engine.Connected {
						return true
					}
				}
			}
		}
		return false
	})

	// 3. Commit text on A and verify B receives it in its history and event stream.
	handleJSON, err := engA.ImportText("sync from A 🚀", "", "")
	if err != nil {
		t.Fatalf("ImportText A: %v", err)
	}
	var handle engine.ContentHandle
	_ = json.Unmarshal([]byte(handleJSON), &handle)
	commitReq, _ := json.Marshal(map[string]any{
		"request_id": "commit-sync-1",
		"lease_id":   handle.LeaseID,
		"device_ids": []string{string(snapB.Self.ID)},
	})
	commitJSON, err := engA.CommitContent(nil, string(commitReq))
	if err != nil {
		t.Fatalf("CommitContent A: %v", err)
	}
	_ = engA.ReleaseContent(string(handle.LeaseID))
	var sub engine.SubmissionResult
	_ = json.Unmarshal([]byte(commitJSON), &sub)
	if sub.Phase != engine.SubmissionCommitted {
		t.Fatalf("unexpected commit phase: %+v", sub)
	}

	var syncedID engine.HistoryID
	eventuallyMobile(t, "peer B receives synced history", func() bool {
		raw, err := engB.ListHistory(`{"limit":10}`)
		if err != nil {
			return false
		}
		var page engine.HistoryPage
		_ = json.Unmarshal([]byte(raw), &page)
		for _, item := range page.Items {
			if item.Origin.Kind == engine.OriginKindRemoteDevice {
				syncedID = item.ID
				return true
			}
		}
		return false
	})

	prevJSON, err := engB.ReadHistoryTextPreview(string(syncedID), 128)
	if err != nil {
		t.Fatalf("ReadHistoryTextPreview B: %v", err)
	}
	var prev textPreviewDTO
	_ = json.Unmarshal([]byte(prevJSON), &prev)
	if prev.Text != "sync from A 🚀" || prev.Truncated {
		t.Fatalf("unexpected synced preview on B: %+v", prev)
	}
	if sinkB.countKind(engine.EventReceived) == 0 && sinkB.countKind(engine.EventHistory) == 0 {
		t.Fatalf("expected EventReceived or EventHistory on B sink")
	}
}

func TestSetupFlowCompleteMigrationAndEngineReadbackIntegrity(t *testing.T) {
	base := tempDirReal(t)
	v1Root, dbPath, f, err := v1fixture.Build(filepath.Join(base, "v1"))
	if err != nil {
		t.Fatal(err)
	}

	// Repair the fixture and move timestamps to recent minutes so live 180-day AutoCleanup keeps all records.
	db, err := sql.Open("sqlite", dbPath)
	if err != nil {
		t.Fatal(err)
	}
	if _, err := db.Exec(
		`INSERT INTO clipboard_snapshot_representation VALUES('rep-bad','event-bad','text/plain','text/plain',?,NULL,9)`,
		[]byte(f.Repaired),
	); err != nil {
		_ = db.Close()
		t.Fatal(err)
	}
	now := time.Now().UnixMilli()
	for idx, id := range []string{"entry-text", "entry-duplicate", "entry-file", "entry-image", "entry-bad"} {
		ts := now - int64(5-idx)*60_000
		if _, err := db.Exec(
			`UPDATE clipboard_entry SET created_at_ms = ?, active_time_ms = ? WHERE entry_id = ?`,
			ts, ts+100, id,
		); err != nil {
			_ = db.Close()
			t.Fatal(err)
		}
	}
	if err := db.Close(); err != nil {
		t.Fatal(err)
	}

	wantPixel, err := os.ReadFile(filepath.Join("..", "..", "internal", "v1migration", "testdata", "pixel.png"))
	if err != nil {
		t.Fatalf("read pixel.png: %v", err)
	}
	wantFile := []byte("binary\x00fixture")

	target := filepath.Join(base, "unic-v2")
	cacheDir := filepath.Join(base, "unic-cache")
	sess, err := OpenSetupFlow(target, v1Root)
	if err != nil {
		t.Fatalf("OpenSetupFlow: %v", err)
	}
	detJSON, err := sess.DetectSource("")
	if err != nil || !strings.Contains(detJSON, `"found"`) {
		t.Fatalf("DetectSource: %q err=%v", detJSON, err)
	}

	prog := &progressCollector{}
	resJSON, err := sess.Run(nil, []byte(f.Passphrase), nil, prog)
	if err != nil {
		t.Fatalf("Run complete fixture: %v", err)
	}
	var res migrationRunResult
	if err := json.Unmarshal([]byte(resJSON), &res); err != nil {
		t.Fatalf("unmarshal run result: %v (%s)", err, resJSON)
	}
	if res.Status != "complete" || res.Imported != 5 || res.Verified != 5 || res.Failed != 0 || res.MissingParts != 0 || res.Tags != 2 {
		t.Fatalf("unexpected complete migration result: %+v", res)
	}

	prog.mu.Lock()
	stages := append([]string(nil), prog.stages...)
	prog.mu.Unlock()
	if !slices.Contains(stages, "import") || !slices.Contains(stages, "verify") {
		t.Fatalf("expected both import and verify progress stages, got %v", stages)
	}

	// Prove source independence and setup completion persistence.
	if err := os.RemoveAll(v1Root); err != nil {
		t.Fatalf("RemoveAll v1Root: %v", err)
	}
	if err := sess.MarkComplete(); err != nil {
		t.Fatalf("MarkComplete: %v", err)
	}

	cfgBytes, _ := json.Marshal(map[string]any{
		"data_dir":    target,
		"cache_dir":   cacheDir,
		"device_name": "Mobile Verify",
	})
	eng, err := NewEngine(string(cfgBytes))
	if err != nil {
		t.Fatalf("NewEngine after complete migration: %v", err)
	}
	defer func() {
		_ = eng.Stop(nil)
		_ = eng.Close(nil)
	}()
	if err := eng.Start(nil); err != nil {
		t.Fatalf("Start: %v", err)
	}

	pageJSON, err := eng.ListHistory(`{"limit":20}`)
	if err != nil {
		t.Fatalf("ListHistory: %v", err)
	}
	var page engine.HistoryPage
	if err := json.Unmarshal([]byte(pageJSON), &page); err != nil {
		t.Fatalf("unmarshal HistoryPage: %v", err)
	}
	if len(page.Items) != 5 {
		t.Fatalf("expected 5 migrated items after startup/AutoCleanup, got %d", len(page.Items))
	}

	tagsJSON, err := eng.ListHistoryTags()
	if err != nil {
		t.Fatalf("ListHistoryTags: %v", err)
	}
	var allTags []string
	if err := json.Unmarshal([]byte(tagsJSON), &allTags); err != nil {
		t.Fatal(err)
	}
	if !slices.Contains(allTags, "标签") || !slices.Contains(allTags, "unused") {
		t.Fatalf("missing expected migrated tags in %v", allTags)
	}

	readPartBytes := func(historyID engine.HistoryID) []byte {
		hJSON, err := eng.RetainHistoryContent(string(historyID))
		if err != nil {
			t.Fatalf("RetainHistoryContent(%s): %v", historyID, err)
		}
		var h engine.ContentHandle
		if err := json.Unmarshal([]byte(hJSON), &h); err != nil {
			t.Fatal(err)
		}
		defer func() { _ = eng.ReleaseContent(string(h.LeaseID)) }()
		if len(h.Content.Parts) == 0 {
			t.Fatalf("expected parts on %s", historyID)
		}
		stream, err := eng.OpenPart(string(h.LeaseID), string(h.Content.Parts[0].ID))
		if err != nil {
			t.Fatalf("OpenPart(%s): %v", historyID, err)
		}
		defer func() { _ = stream.Close() }()
		var buf bytes.Buffer
		for {
			chunk, err := stream.ReadChunk(4096)
			if err != nil {
				t.Fatalf("ReadChunk(%s): %v", historyID, err)
			}
			if len(chunk) == 0 {
				break
			}
			buf.Write(chunk)
		}
		return buf.Bytes()
	}

	fixtureTextCount := 0
	taggedFixtureTextCount := 0
	recoveredCount := 0
	imageCount := 0
	fileCount := 0

	for _, item := range page.Items {
		if item.Origin.Kind != engine.OriginKindMigration {
			t.Fatalf("expected migration origin, got %+v", item.Origin)
		}
		switch item.Kind {
		case engine.HistoryText:
			prevJSON, err := eng.ReadHistoryTextPreview(string(item.ID), 512)
			if err != nil {
				t.Fatalf("ReadHistoryTextPreview(%s): %v", item.ID, err)
			}
			var prev textPreviewDTO
			if err := json.Unmarshal([]byte(prevJSON), &prev); err != nil {
				t.Fatal(err)
			}
			switch prev.Text {
			case "fixture text":
				fixtureTextCount++
				if slices.Contains(item.Tags, "标签") {
					taggedFixtureTextCount++
				}
			case "recovered":
				recoveredCount++
			default:
				t.Fatalf("unexpected migrated text item: %q", prev.Text)
			}
		case engine.HistoryImage:
			imageCount++
			got := readPartBytes(item.ID)
			if !bytes.Equal(got, wantPixel) {
				t.Fatalf("migrated image bytes mismatch: got %d bytes, want %d", len(got), len(wantPixel))
			}
		case engine.HistoryFiles:
			fileCount++
			got := readPartBytes(item.ID)
			if !bytes.Equal(got, wantFile) {
				t.Fatalf("migrated file bytes mismatch: got %x, want %x", got, wantFile)
			}
		default:
			t.Fatalf("unexpected migrated item kind %q (%+v)", item.Kind, item)
		}
	}

	if fixtureTextCount != 2 || taggedFixtureTextCount != 1 || recoveredCount != 1 || imageCount != 1 || fileCount != 1 {
		t.Fatalf("unexpected item breakdown: fixtureText=%d tagged=%d recovered=%d image=%d file=%d",
			fixtureTextCount, taggedFixtureTextCount, recoveredCount, imageCount, fileCount)
	}
}

func TestApplyWithoutBaselineRepeatedAndFailureRecovery(t *testing.T) {
	dataDir := filepath.Join(tempDirReal(t), "unic-apply")
	cfgBytes, _ := json.Marshal(map[string]any{
		"data_dir":    dataDir,
		"device_name": "Apply Test",
		"host_capabilities": map[string]any{
			"read_formats":  []string{"text/plain", "text/html", "image/png"},
			"write_formats": []string{"text/plain", "text/html", "image/png"},
			"read_files":    true,
			"write_files":   true,
			"observe":       "permission_required",
			"apply":         "available",
		},
	})
	eng, err := NewEngine(string(cfgBytes))
	if err != nil {
		t.Fatalf("NewEngine: %v", err)
	}
	defer func() { _ = eng.Close(nil) }()
	if err := eng.Start(nil); err != nil {
		t.Fatalf("Start: %v", err)
	}

	hJSON, err := eng.ImportText("copy-me-without-baseline", "", "")
	if err != nil {
		t.Fatalf("ImportText: %v", err)
	}
	var h engine.ContentHandle
	_ = json.Unmarshal([]byte(hJSON), &h)
	cJSON, err := eng.CommitContent(nil, `{"request_id":"req-apply-1","lease_id":"`+string(h.LeaseID)+`"}`)
	if err != nil {
		t.Fatalf("CommitContent: %v", err)
	}
	_ = eng.ReleaseContent(string(h.LeaseID))
	var sub engine.SubmissionResult
	_ = json.Unmarshal([]byte(cJSON), &sub)

	// 1. Cold start (no ObserveClipboard baseline): repeat BeginApplyHistory + ReportApplyResult + ReleaseContent 20 times.
	for i := 0; i < 20; i++ {
		tJSON, err := eng.BeginApplyHistory(string(sub.HistoryID), "", "0", true)
		if err != nil {
			t.Fatalf("iteration %d BeginApplyHistory without baseline: %v", i, err)
		}
		var tk struct {
			Token   string `json:"token"`
			LeaseID string `json:"lease_id"`
		}
		_ = json.Unmarshal([]byte(tJSON), &tk)
		if tk.Token == "" || tk.LeaseID == "" {
			t.Fatalf("iteration %d invalid ticket: %s", i, tJSON)
		}
		if err := eng.ReportApplyResult(`{"token":"` + tk.Token + `","outcome":"applied","epoch":"","sequence":"0"}`); err != nil {
			_ = eng.ReleaseContent(tk.LeaseID)
			t.Fatalf("iteration %d ReportApplyResult: %v", i, err)
		}
		if err := eng.ReleaseContent(tk.LeaseID); err != nil {
			t.Fatalf("iteration %d ReleaseContent: %v", i, err)
		}
	}

	// 2. Failure path releases ticket slot so subsequent apply succeeds.
	tJSON, err := eng.BeginApplyHistory(string(sub.HistoryID), "", "0", true)
	if err != nil {
		t.Fatalf("BeginApplyHistory before failure: %v", err)
	}
	var tkFail struct {
		Token   string `json:"token"`
		LeaseID string `json:"lease_id"`
	}
	_ = json.Unmarshal([]byte(tJSON), &tkFail)
	if err := eng.ReportApplyResult(`{"token":"` + tkFail.Token + `","outcome":"failed","error_code":"unavailable"}`); err != nil {
		t.Fatalf("ReportApplyResult(failed): %v", err)
	}
	if err := eng.ReleaseContent(tkFail.LeaseID); err != nil {
		t.Fatalf("ReleaseContent after failure: %v", err)
	}

	// 3. After baseline + suspend + resume, Engine clears obs epoch, so empty expectedEpoch works again.
	if _, err := eng.ObserveClipboard(`{"epoch":"ep-1","sequence":"1","mode":"baseline"}`); err != nil {
		t.Fatalf("ObserveClipboard baseline: %v", err)
	}
	if err := eng.SetRuntimeState("suspended"); err != nil {
		t.Fatalf("SetRuntimeState(suspended): %v", err)
	}
	if err := eng.SetRuntimeState("active"); err != nil {
		t.Fatalf("SetRuntimeState(active): %v", err)
	}
	tJSON2, err := eng.BeginApplyHistory(string(sub.HistoryID), "", "0", true)
	if err != nil {
		t.Fatalf("BeginApplyHistory after resume: %v", err)
	}
	var tk2 struct {
		Token   string `json:"token"`
		LeaseID string `json:"lease_id"`
	}
	_ = json.Unmarshal([]byte(tJSON2), &tk2)
	if err := eng.ReportApplyResult(`{"token":"` + tk2.Token + `","outcome":"applied","epoch":"","sequence":"0"}`); err != nil {
		t.Fatalf("ReportApplyResult after resume: %v", err)
	}
	if err := eng.ReleaseContent(tk2.LeaseID); err != nil {
		t.Fatalf("ReleaseContent after resume: %v", err)
	}
}

func TestM1ContractsBindingSyncReceivedResendFullTextAndPagination(t *testing.T) {
	root := t.TempDir()
	caps := map[string]any{
		"read_formats":  []string{"text/plain", "text/html", "image/png"},
		"write_formats": []string{"text/plain", "text/html", "image/png"},
		"read_files":    true,
		"write_files":   true,
		"observe":       "available",
		"apply":         "available",
	}
	cfgA, _ := json.Marshal(map[string]any{
		"data_dir":          filepath.Join(root, "node-a"),
		"device_name":       "Phone A",
		"host_capabilities": caps,
	})
	cfgB, _ := json.Marshal(map[string]any{
		"data_dir":          filepath.Join(root, "node-b"),
		"device_name":       "Phone B",
		"host_capabilities": caps,
	})

	engA, err := newEngineWithBackend(string(cfgA), tailcat.NewOn(newLoopbackTransport(t)))
	if err != nil {
		t.Fatalf("newEngineWithBackend A: %v", err)
	}
	defer func() { _ = engA.Close(nil) }()

	engB, err := newEngineWithBackend(string(cfgB), tailcat.NewOn(newLoopbackTransport(t)))
	if err != nil {
		t.Fatalf("newEngineWithBackend B: %v", err)
	}
	defer func() { _ = engB.Close(nil) }()

	if err := engA.Start(nil); err != nil {
		t.Fatalf("Start A: %v", err)
	}
	if err := engB.Start(nil); err != nil {
		t.Fatalf("Start B: %v", err)
	}

	// 1. GetPairingCode when empty returns exists: false.
	emptyCodeJSON, err := engA.GetPairingCode()
	if err != nil {
		t.Fatalf("GetPairingCode(empty): %v", err)
	}
	var codeResp struct {
		Exists      bool   `json:"exists"`
		Code        string `json:"code"`
		ExpiresAtMs int64  `json:"expires_at_ms"`
	}
	if err := json.Unmarshal([]byte(emptyCodeJSON), &codeResp); err != nil {
		t.Fatalf("unmarshal GetPairingCode: %v", err)
	}
	if codeResp.Exists || codeResp.Code != "" {
		t.Fatalf("expected empty pairing code, got %+v", codeResp)
	}

	// 2. PairCandidate rejects missing request_id / unknown device cleanly.
	if _, err := engA.PairCandidate(`{"device_id":"unknown","backend_id":"builtin.tailcat"}`); err == nil {
		t.Fatal("expected error for PairCandidate without request_id")
	}
	if _, err := engA.PairCandidate(`{"request_id":"pc-1","device_id":"unknown","backend_id":"builtin.tailcat"}`); err == nil {
		t.Fatal("expected error for unknown PairCandidate")
	}

	// 3. Full text (> 10 KB), SearchHistory tail keyword + date/source pushdown, and >120 items pagination on A.
	largeText := strings.Repeat("alpha beta gamma 0123456789 ", 450) + "TAIL_NEEDLE_987654"
	if len(largeText) <= 10*1024 {
		t.Fatalf("expected largeText > 10 KB, got %d", len(largeText))
	}
	lhJSON, err := engA.ImportText(largeText, "", "")
	if err != nil {
		t.Fatalf("ImportText large: %v", err)
	}
	var lh engine.ContentHandle
	_ = json.Unmarshal([]byte(lhJSON), &lh)
	lSubJSON, err := engA.CommitContent(nil, `{"request_id":"req-large","lease_id":"`+string(lh.LeaseID)+`"}`)
	if err != nil {
		t.Fatalf("CommitContent large: %v", err)
	}
	_ = engA.ReleaseContent(string(lh.LeaseID))
	var lSub engine.SubmissionResult
	_ = json.Unmarshal([]byte(lSubJSON), &lSub)

	shortPrevJSON, err := engA.ReadHistoryTextPreview(string(lSub.HistoryID), 512)
	if err != nil {
		t.Fatalf("ReadHistoryTextPreview: %v", err)
	}
	var shortPrev textPreviewDTO
	_ = json.Unmarshal([]byte(shortPrevJSON), &shortPrev)
	if !shortPrev.Truncated || strings.Contains(shortPrev.Text, "TAIL_NEEDLE_987654") {
		t.Fatalf("expected truncated 512B preview without tail needle, got len=%d trunc=%v", len(shortPrev.Text), shortPrev.Truncated)
	}

	fullPrevJSON, err := engA.ReadHistoryFullText(string(lSub.HistoryID), 0)
	if err != nil {
		t.Fatalf("ReadHistoryFullText: %v", err)
	}
	var fullPrev textPreviewDTO
	_ = json.Unmarshal([]byte(fullPrevJSON), &fullPrev)
	if fullPrev.Truncated || fullPrev.Text != largeText {
		t.Fatalf("expected full untruncated text len=%d, got len=%d trunc=%v", len(largeText), len(fullPrev.Text), fullPrev.Truncated)
	}

	nowMs := time.Now().UnixMilli()
	searchReq, _ := json.Marshal(map[string]any{
		"query": map[string]any{
			"text_terms":        []string{"TAIL_NEEDLE_987654"},
			"sources":           []map[string]any{{"kind": "local"}},
			"created_from_ms":   nowMs - 60_000,
			"created_before_ms": nowMs + 60_000,
		},
		"limit": 20,
	})
	sJSON, err := engA.SearchHistory(nil, string(searchReq))
	if err != nil {
		t.Fatalf("SearchHistory tail needle: %v", err)
	}
	var sPage engine.HistorySearchPage
	_ = json.Unmarshal([]byte(sJSON), &sPage)
	if len(sPage.Hits) != 1 || sPage.Hits[0].Item.ID != lSub.HistoryID {
		t.Fatalf("expected 1 search hit for tail needle, got %+v", sPage.Hits)
	}

	// CountHistorySearches returns string-encoded counts.
	cntJSON, err := engA.CountHistorySearches(nil, `{"queries":[{"text_terms":["TAIL_NEEDLE_987654"]},{"text_terms":["no_such_token_xyz"]}]}`)
	if err != nil {
		t.Fatalf("CountHistorySearches: %v", err)
	}
	var cntResp struct {
		Counts []string `json:"counts"`
	}
	_ = json.Unmarshal([]byte(cntJSON), &cntResp)
	if len(cntResp.Counts) != 2 || cntResp.Counts[0] != "1" || cntResp.Counts[1] != "0" {
		t.Fatalf("unexpected CountHistorySearches response: %s", cntJSON)
	}

	// 4. Pair A and B, verify SetBindingSync(false/true) with "enabled" JSON field.
	var snapA, snapB engine.State
	stA, _ := engA.GetState()
	stB, _ := engB.GetState()
	_ = json.Unmarshal([]byte(stA), &snapA)
	_ = json.Unmarshal([]byte(stB), &snapB)

	tkJSON, err := engA.ExportPairingTicket(nil)
	if err != nil {
		t.Fatalf("ExportPairingTicket: %v", err)
	}
	var tk struct {
		Ticket string `json:"ticket"`
	}
	_ = json.Unmarshal([]byte(tkJSON), &tk)
	jReq, _ := json.Marshal(map[string]any{"request_id": "m1-join", "ticket": tk.Ticket})
	opID, err := engB.JoinByTicket(nil, string(jReq))
	if err != nil {
		t.Fatalf("JoinByTicket: %v", err)
	}
	eventuallyMobile(t, "m1 join succeeded", func() bool {
		raw, err := engB.GetOperation(opID)
		if err != nil {
			return false
		}
		var op engine.Operation
		_ = json.Unmarshal([]byte(raw), &op)
		return op.Phase == engine.PhaseSucceeded
	})
	approve := func(owner *Engine, peerID engine.DeviceID) engine.Device {
		var peer engine.Device
		eventuallyMobile(t, "m1 pending "+string(peerID), func() bool {
			raw, _ := owner.ListDevices()
			var ds []engine.Device
			_ = json.Unmarshal([]byte(raw), &ds)
			for _, d := range ds {
				if d.ID == string(peerID) && len(d.Bindings) > 0 {
					peer = d
					return true
				}
			}
			return false
		})
		bs := make([]map[string]any, len(peer.Bindings))
		for i, b := range peer.Bindings {
			bs[i] = map[string]any{"id": b.ID, "expected_revision": b.Revision}
		}
		req, _ := json.Marshal(map[string]any{
			"id":                   peer.ID,
			"expected_revision":    peer.Revision,
			"expected_fingerprint": peer.Fingerprint,
			"bindings":             bs,
		})
		out, err := owner.ApproveDevice(string(req))
		if err != nil {
			t.Fatalf("ApproveDevice: %v", err)
		}
		var approved engine.Device
		_ = json.Unmarshal([]byte(out), &approved)
		return approved
	}
	approvedBOnA := approve(engA, snapB.Self.ID)
	approve(engB, snapA.Self.ID)

	connected := func(owner *Engine, peerID engine.DeviceID) bool {
		raw, _ := owner.ListDevices()
		var ds []engine.Device
		_ = json.Unmarshal([]byte(raw), &ds)
		for _, d := range ds {
			if d.ID == string(peerID) {
				for _, b := range d.Bindings {
					if b.SyncEnabled && b.Connection == engine.Connected {
						return true
					}
				}
			}
		}
		return false
	}
	peersConnected := func() bool {
		return connected(engA, snapB.Self.ID) && connected(engB, snapA.Self.ID)
	}
	eventuallyMobile(t, "m1 peers connected", peersConnected)

	// Toggle SetBindingSync false then true using "enabled".
	binding := approvedBOnA.Bindings[0]
	pauseReq, _ := json.Marshal(map[string]any{
		"id":                binding.ID,
		"expected_revision": binding.Revision,
		"enabled":           false,
	})
	pausedJSON, err := engA.SetBindingSync(string(pauseReq))
	if err != nil {
		t.Fatalf("SetBindingSync(false): %v", err)
	}
	var pausedBinding engine.DeviceBinding
	_ = json.Unmarshal([]byte(pausedJSON), &pausedBinding)
	if pausedBinding.SyncEnabled {
		t.Fatalf("expected SyncEnabled=false after pause, got %+v", pausedBinding)
	}
	resumeReq, _ := json.Marshal(map[string]any{
		"id":                pausedBinding.ID,
		"expected_revision": pausedBinding.Revision,
		"enabled":           true,
	})
	resumedJSON, err := engA.SetBindingSync(string(resumeReq))
	if err != nil {
		t.Fatalf("SetBindingSync(true): %v", err)
	}
	var resumedBinding engine.DeviceBinding
	_ = json.Unmarshal([]byte(resumedJSON), &resumedBinding)
	if !resumedBinding.SyncEnabled {
		t.Fatalf("expected SyncEnabled=true after resume, got %+v", resumedBinding)
	}
	eventuallyMobile(t, "m1 peers reconnected after resuming sync", peersConnected)

	// 5. Resend existing history item (largeText) from A -> B, verify B receives in ListReceived, applies via BeginApply + ReadLeasedText + ReportApplyResult + ReleaseContent, and dismisses.
	withoutTargets, _ := json.Marshal(map[string]any{
		"request_id": "resend-without-targets",
		"history_id": lSub.HistoryID,
	})
	if _, err := engA.ResendHistoryItem(nil, string(withoutTargets)); err == nil || !strings.Contains(err.Error(), `"code":"invalid_argument"`) {
		t.Errorf("ResendHistoryItem without an explicit target: %v", err)
	}
	resendReq, _ := json.Marshal(map[string]any{
		"request_id": "resend-large-1",
		"history_id": lSub.HistoryID,
		"device_ids": []string{string(snapB.Self.ID)},
	})
	resendJSON, err := engA.ResendHistoryItem(nil, string(resendReq))
	if err != nil {
		t.Fatalf("ResendHistoryItem: %v", err)
	}
	var resend engine.SubmissionResult
	if err := json.Unmarshal([]byte(resendJSON), &resend); err != nil || resend.HistoryID != lSub.HistoryID || resend.SyncDisposition != engine.SyncQueued || resend.OperationID == "" {
		t.Fatalf("resend must use the original history record and queue a send: %s (%v)", resendJSON, err)
	}
	replayJSON, err := engA.ResendHistoryItem(nil, string(resendReq))
	var replay engine.SubmissionResult
	if err != nil || json.Unmarshal([]byte(replayJSON), &replay) != nil || replay.OperationID != resend.OperationID || replay.HistoryID != resend.HistoryID {
		t.Fatalf("resend replay must return the original send: %s (%v)", replayJSON, err)
	}
	cntJSON, err = engA.CountHistorySearches(nil, `{"queries":[{"text_terms":["TAIL_NEEDLE_987654"]}]}`)
	var sentCount struct{ Counts []string }
	if err != nil || json.Unmarshal([]byte(cntJSON), &sentCount) != nil || len(sentCount.Counts) != 1 || sentCount.Counts[0] != "1" {
		t.Fatalf("resend must leave one sender history item: %s (%v)", cntJSON, err)
	}

	var recvEntry engine.ReceivedContent
	eventuallyMobile(t, "B receives resent item in ListReceived", func() bool {
		raw, err := engB.ListReceived()
		if err != nil {
			return false
		}
		var rs []engine.ReceivedContent
		_ = json.Unmarshal([]byte(raw), &rs)
		if len(rs) > 0 {
			recvEntry = rs[0]
			return true
		}
		return false
	})

	applyReq, _ := json.Marshal(map[string]any{
		"receipt_id":        recvEntry.ReceiptID,
		"expected_epoch":    "",
		"expected_sequence": "0",
	})
	applyTicketJSON, err := engB.BeginApply(string(applyReq))
	if err != nil {
		t.Fatalf("BeginApply on B: %v", err)
	}
	var applyTk struct {
		Token   string `json:"token"`
		LeaseID string `json:"lease_id"`
	}
	_ = json.Unmarshal([]byte(applyTicketJSON), &applyTk)
	leasedTextJSON, err := engB.ReadLeasedText(applyTk.LeaseID, string(recvEntry.Content.Parts[0].ID), 0)
	if err != nil {
		_ = engB.ReleaseContent(applyTk.LeaseID)
		t.Fatalf("ReadLeasedText on B: %v", err)
	}
	var leasedText textPreviewDTO
	_ = json.Unmarshal([]byte(leasedTextJSON), &leasedText)
	if leasedText.Text != largeText {
		_ = engB.ReleaseContent(applyTk.LeaseID)
		t.Fatalf("resent received text mismatch: got len %d, want %d", len(leasedText.Text), len(largeText))
	}
	if err := engB.ReportApplyResult(`{"token":"` + applyTk.Token + `","outcome":"applied","epoch":"","sequence":"0"}`); err != nil {
		_ = engB.ReleaseContent(applyTk.LeaseID)
		t.Fatalf("ReportApplyResult on B: %v", err)
	}
	if err := engB.ReleaseContent(applyTk.LeaseID); err != nil {
		t.Fatalf("ReleaseContent on B: %v", err)
	}
	if err := engB.DismissReceived(string(recvEntry.ReceiptID)); err != nil {
		t.Fatalf("DismissReceived on B: %v", err)
	}
	afterDismissJSON, err := engB.ListReceived()
	if err != nil {
		t.Fatalf("ListReceived after dismiss: %v", err)
	}
	if strings.TrimSpace(afterDismissJSON) != "[]" {
		t.Fatalf("expected empty received list after dismiss, got %s", afterDismissJSON)
	}

	// 6. Pagination with >120 items on A.
	for i := 0; i < 125; i++ {
		ihJSON, err := engA.ImportText(fmt.Sprintf("page-item-%03d", i), "", "")
		if err != nil {
			t.Fatalf("ImportText %d: %v", i, err)
		}
		var ih engine.ContentHandle
		_ = json.Unmarshal([]byte(ihJSON), &ih)
		_, err = engA.CommitContent(nil, fmt.Sprintf(`{"request_id":"req-page-%03d","lease_id":"%s"}`, i, ih.LeaseID))
		_ = engA.ReleaseContent(string(ih.LeaseID))
		if err != nil {
			t.Fatalf("CommitContent %d: %v", i, err)
		}
	}
	seenIDs := map[engine.HistoryID]bool{}
	cursor := ""
	pages := 0
	for {
		reqObj := map[string]any{"limit": 50}
		if cursor != "" {
			reqObj["cursor"] = cursor
		}
		reqB, _ := json.Marshal(reqObj)
		pJSON, err := engA.ListHistory(string(reqB))
		if err != nil {
			t.Fatalf("ListHistory page %d: %v", pages, err)
		}
		var page engine.HistoryPage
		_ = json.Unmarshal([]byte(pJSON), &page)
		pages++
		for _, it := range page.Items {
			if seenIDs[it.ID] {
				t.Fatalf("duplicate item %s across pages", it.ID)
			}
			seenIDs[it.ID] = true
		}
		if page.NextCursor == "" {
			break
		}
		cursor = page.NextCursor
	}
	if pages < 3 || len(seenIDs) < 126 {
		t.Fatalf("expected >=3 pages and >=126 unique items, got pages=%d items=%d", pages, len(seenIDs))
	}
}

func TestSetupFlowLegacyMobileExpoSQLiteAutoDetectAndMigration(t *testing.T) {
	base := tempDirReal(t)
	target := filepath.Join(base, "unic-v2")
	missingV1 := filepath.Join(base, "no-v1-vault")

	// Empty legacy DB should be skipped during auto-detection.
	emptyDB := filepath.Join(base, "empty-group", "Databases", "uniclipboard.db")
	if err := os.MkdirAll(filepath.Dir(emptyDB), 0o700); err != nil {
		t.Fatal(err)
	}
	edb, err := sql.Open("sqlite", emptyDB)
	if err != nil {
		t.Fatal(err)
	}
	_, err = edb.Exec(`CREATE TABLE clipboard_history (
		profileHash TEXT PRIMARY KEY COLLATE NOCASE,
		type TEXT NOT NULL,
		text TEXT NOT NULL DEFAULT '',
		dataName TEXT,
		size INTEGER,
		fileUri TEXT,
		timestamp INTEGER NOT NULL DEFAULT 0,
		lastAccessed INTEGER NOT NULL DEFAULT 0,
		lastModified INTEGER NOT NULL DEFAULT 0,
		starred INTEGER NOT NULL DEFAULT 0,
		pinned INTEGER NOT NULL DEFAULT 0,
		isDeleted INTEGER NOT NULL DEFAULT 0
	)`)
	_ = edb.Close()
	if err != nil {
		t.Fatal(err)
	}

	// Populated legacy mobile DB (iOS App Group layout: Databases/uniclipboard.db + payloads/).
	groupDir := filepath.Join(base, "legacy-group")
	legacyDB := filepath.Join(groupDir, "Databases", "uniclipboard.db")
	payloadsDir := filepath.Join(groupDir, "payloads")
	if err := os.MkdirAll(filepath.Dir(legacyDB), 0o700); err != nil {
		t.Fatal(err)
	}
	if err := os.MkdirAll(payloadsDir, 0o700); err != nil {
		t.Fatal(err)
	}
	wantPixel, err := os.ReadFile(filepath.Join("..", "..", "internal", "v1migration", "testdata", "pixel.png"))
	if err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(payloadsDir, "Image-mob-img-1"), wantPixel, 0o600); err != nil {
		t.Fatal(err)
	}

	now := time.Now().UnixMilli()
	ldb, err := sql.Open("sqlite", legacyDB)
	if err != nil {
		t.Fatal(err)
	}
	_, err = ldb.Exec(`CREATE TABLE clipboard_history (
		profileHash TEXT PRIMARY KEY COLLATE NOCASE,
		type TEXT NOT NULL,
		text TEXT NOT NULL DEFAULT '',
		dataName TEXT,
		size INTEGER,
		fileUri TEXT,
		timestamp INTEGER NOT NULL DEFAULT 0,
		lastAccessed INTEGER NOT NULL DEFAULT 0,
		lastModified INTEGER NOT NULL DEFAULT 0,
		starred INTEGER NOT NULL DEFAULT 0,
		pinned INTEGER NOT NULL DEFAULT 0,
		isDeleted INTEGER NOT NULL DEFAULT 0
	);
	INSERT INTO clipboard_history (profileHash, type, text, timestamp, lastAccessed, lastModified, starred, pinned, isDeleted)
	VALUES ('mob-txt-1', 'Text', 'legacy mobile text', ?, ?, ?, 1, 1, 0);
	INSERT INTO clipboard_history (profileHash, type, text, dataName, size, timestamp, lastAccessed, lastModified, starred, pinned, isDeleted)
	VALUES ('mob-img-1', 'Image', '', 'pixel.png', ?, ?, ?, ?, 0, 0, 0);`,
		now-2000, now-2000, now-2000, len(wantPixel), now-1000, now-1000, now-1000)
	_ = ldb.Close()
	if err != nil {
		t.Fatal(err)
	}

	sess, err := OpenSetupFlow(target, missingV1)
	if err != nil {
		t.Fatalf("OpenSetupFlow: %v", err)
	}
	cfgJSON, _ := json.Marshal(map[string]any{
		"database_candidates": []string{emptyDB, legacyDB},
		"attachment_roots":    []string{payloadsDir},
	})
	if err := sess.ConfigureLegacyMobile(string(cfgJSON)); err != nil {
		t.Fatalf("ConfigureLegacyMobile: %v", err)
	}

	detJSON, err := sess.DetectSource("")
	if err != nil {
		t.Fatalf("DetectSource: %v", err)
	}
	var det struct {
		Status         string `json:"status"`
		Source         string `json:"source"`
		SourceKind     string `json:"source_kind"`
		RequiresUnlock bool   `json:"requires_unlock"`
	}
	if err := json.Unmarshal([]byte(detJSON), &det); err != nil {
		t.Fatalf("unmarshal detect: %v (%s)", err, detJSON)
	}
	if det.Status != "found" || det.SourceKind != sourceKindMobileSQLite || det.RequiresUnlock {
		t.Fatalf("unexpected detection: %+v", det)
	}

	// Run WITHOUT passphrase and WITHOUT secret provider.
	prog := &progressCollector{}
	resJSON, err := sess.Run(nil, nil, nil, prog)
	if err != nil {
		t.Fatalf("Run legacy mobile migration: %v", err)
	}
	var runRes migrationRunResult
	if err := json.Unmarshal([]byte(resJSON), &runRes); err != nil {
		t.Fatalf("unmarshal run result: %v (%s)", err, resJSON)
	}
	if runRes.Status != "complete" || runRes.Imported != 2 || runRes.Verified != 2 || runRes.Failed != 0 {
		t.Fatalf("unexpected run result: %+v", runRes)
	}

	if err := sess.MarkComplete(); err != nil {
		t.Fatalf("MarkComplete: %v", err)
	}

	// Open real mobile Engine on target and verify records and tags.
	engCfg, _ := json.Marshal(map[string]any{"data_dir": target})
	eng, err := NewEngine(string(engCfg))
	if err != nil {
		t.Fatalf("NewEngine: %v", err)
	}
	defer func() { _ = eng.Close(nil) }()
	if err := eng.Start(nil); err != nil {
		t.Fatalf("Start: %v", err)
	}
	pageJSON, err := eng.ListHistory(`{"limit":20}`)
	if err != nil {
		t.Fatalf("ListHistory: %v", err)
	}
	var page engine.HistoryPage
	if err := json.Unmarshal([]byte(pageJSON), &page); err != nil {
		t.Fatal(err)
	}
	if len(page.Items) != 2 {
		t.Fatalf("expected 2 migrated items, got %d", len(page.Items))
	}
}
