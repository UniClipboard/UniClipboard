package mobile

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"io"
	"net"
	"os"
	"path/filepath"
	"strings"
	"sync"
	"testing"
	"time"

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
