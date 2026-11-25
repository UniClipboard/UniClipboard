package host

import (
	"context"
	"encoding/base64"
	"encoding/json"
	"io"
	"os"
	"path/filepath"
	"sync"
	"testing"
	"time"

	"github.com/uniclipboard/unic/engine"
	"github.com/uniclipboard/unic/internal/clip"
	"github.com/uniclipboard/unic/internal/clipboardintegration"
	api "github.com/uniclipboard/unic/internal/localapi"
	config "github.com/uniclipboard/unic/internal/localconfig"
	"github.com/uniclipboard/unic/internal/servicelifecycle"
	"github.com/uniclipboard/unic/internal/testboard"
)

func openPanelTestNode(t *testing.T, mode servicelifecycle.Mode, retiring *bool, syncOnRestore bool) (*Node, string, string, func()) {
	t.Helper()
	dir := t.TempDir()
	dir, err := filepath.EvalSymlinks(dir)
	if err != nil {
		t.Fatal(err)
	}
	cfgText := "[telemetry]\nenabled=false\n[history]\nauto_cleanup=false\n[sync]\nenabled=false\n"
	if syncOnRestore {
		cfgText = "[telemetry]\nenabled=false\n[history]\nauto_cleanup=false\n[sync]\nenabled=true\non_restore=true\n"
	}
	cfgPath := filepath.Join(dir, "config.toml")
	if err := os.WriteFile(cfgPath, []byte(cfgText), 0600); err != nil {
		t.Fatal(err)
	}
	cfg, err := config.Open(cfgPath, t.Logf)
	if err != nil {
		t.Fatal(err)
	}
	genID := "gen-" + randomID()
	var mu sync.Mutex
	infoFn := func() servicelifecycle.Instance {
		mu.Lock()
		defer mu.Unlock()
		ret := false
		if retiring != nil {
			ret = *retiring
		}
		return servicelifecycle.Instance{
			ID:        genID,
			PID:       os.Getpid(),
			Directory: dir,
			Mode:      mode,
			Retiring:  ret,
		}
	}
	n, err := Open(context.Background(), Options{
		Dir:         dir,
		Settings:    cfg,
		Logf:        t.Logf,
		Transport:   newLoopback(t),
		ServiceInfo: infoFn,
	})
	if err != nil {
		t.Fatal(err)
	}
	if err := n.Start(context.Background()); err != nil {
		_ = n.Close(context.Background())
		t.Fatal(err)
	}
	cleanup := func() {
		ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
		defer cancel()
		_ = n.Close(ctx)
	}
	return n, dir, genID, cleanup
}

func TestPanelApplyStandaloneAndValidation(t *testing.T) {
	ctx := context.Background()
	retiring := false
	n, dir, genID, cleanup := openPanelTestNode(t, servicelifecycle.ModeStandalone, &retiring, true)
	defer cleanup()

	rich := clip.Snapshot{Parts: []clip.Part{
		{Format: clip.Text, Data: []byte("panel-hello")},
		{Format: clip.HTML, Data: []byte("<b>panel-hello</b>")},
	}}
	richRes, err := n.Submit(ctx, api.SubmitInput{RequestID: "sub-rich", Snapshot: rich})
	if err != nil {
		t.Fatal(err)
	}
	htmlOnlyRes, err := n.Submit(ctx, api.SubmitInput{RequestID: "sub-html", Snapshot: clip.Snapshot{
		Parts: []clip.Part{{Format: clip.HTML, Data: []byte("<i>only</i>")}},
	}})
	if err != nil {
		t.Fatal(err)
	}
	pngBytes, _ := base64.StdEncoding.DecodeString("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+jRZkAAAAASUVORK5CYII=")
	imgRes, err := n.Submit(ctx, api.SubmitInput{RequestID: "sub-img", Snapshot: clip.Snapshot{
		Parts: []clip.Part{{Format: clip.PNG, Data: pngBytes}},
	}})
	if err != nil {
		t.Fatal(err)
	}
	sampleFile := filepath.Join(dir, "sample.txt")
	if err := os.WriteFile(sampleFile, []byte("file-body"), 0600); err != nil {
		t.Fatal(err)
	}
	fileRes, err := n.Submit(ctx, api.SubmitInput{RequestID: "sub-file", Snapshot: clip.Snapshot{
		Files: []clip.File{{Name: "sample.txt", Path: sampleFile, Size: 9}},
	}})
	if err != nil {
		t.Fatal(err)
	}

	// Profile mismatch rejected.
	if _, err := n.PanelApplyBegin(ctx, api.PanelApplyBeginRequest{
		Profile:   filepath.Join(dir, "wrong"),
		RequestID: "req-bad-profile",
		HistoryID: string(richRes.HistoryID),
	}); err == nil {
		t.Fatal("expected profile mismatch error")
	}

	// Generation mismatch rejected.
	if _, err := n.PanelApplyBegin(ctx, api.PanelApplyBeginRequest{
		Profile:   dir,
		ServiceID: "wrong-gen",
		RequestID: "req-bad-gen",
		HistoryID: string(richRes.HistoryID),
	}); err == nil {
		t.Fatal("expected service_id mismatch error")
	}

	// HTML-only plain rejected without writing or creating Action history.
	if _, err := n.PanelApplyBegin(ctx, api.PanelApplyBeginRequest{
		Profile:   dir,
		RequestID: "req-html-plain",
		HistoryID: string(htmlOnlyRes.HistoryID),
		Plain:     true,
	}); api.ReasonOf(err) != "plain" {
		t.Fatalf("expected reason=plain, got %v (reason=%q)", err, api.ReasonOf(err))
	}

	// Normal rich restore without observer.
	before := clip.Snapshot{Parts: []clip.Part{{Format: clip.Text, Data: []byte("before")}}}
	grant, err := n.PanelApplyBegin(ctx, api.PanelApplyBeginRequest{
		Profile:        dir,
		RequestID:      "req-rich-1",
		HistoryID:      string(richRes.HistoryID),
		ObservedBefore: &before,
	})
	if err != nil {
		t.Fatalf("begin rich: %v", err)
	}
	if grant.ServiceID != genID || grant.Token == "" || grant.HasFiles || len(grant.Snapshot.Parts) != 2 {
		t.Fatalf("unexpected rich grant: %+v", grant)
	}
	// Stale token on complete rejected.
	if _, err := n.PanelApplyComplete(ctx, api.PanelApplyCompleteRequest{
		Profile:   dir,
		ServiceID: genID,
		RequestID: "req-rich-1",
		Token:     "wrong-token",
		Outcome:   "applied",
		ReadBack:  &grant.Snapshot,
	}); err == nil {
		t.Fatal("expected wrong token to be rejected")
	}
	rep, err := n.PanelApplyComplete(ctx, api.PanelApplyCompleteRequest{
		Profile:   dir,
		ServiceID: genID,
		RequestID: "req-rich-1",
		Token:     grant.Token,
		Outcome:   "applied",
		ReadBack:  &grant.Snapshot,
	})
	if err != nil || !rep.Applied || rep.Restore == nil || !rep.Restore.LocalApplied {
		t.Fatalf("complete rich failed: rep=%+v err=%v", rep, err)
	}
	// Idempotent replay with same request_id + history_id returns cached result.
	replayed, err := n.PanelApplyBegin(ctx, api.PanelApplyBeginRequest{
		Profile:   dir,
		RequestID: "req-rich-1",
		HistoryID: string(richRes.HistoryID),
	})
	if err != nil || !replayed.Replayed || replayed.Restore == nil || !replayed.Restore.LocalApplied {
		t.Fatalf("expected idempotent replay, got %+v err=%v", replayed, err)
	}
	// Conflicting history_id for same request_id rejected.
	if _, err := n.PanelApplyBegin(ctx, api.PanelApplyBeginRequest{
		Profile:   dir,
		RequestID: "req-rich-1",
		HistoryID: string(imgRes.HistoryID),
	}); api.ReasonOf(err) != "conflict" {
		t.Fatalf("expected conflict for mismatched request_id, got %v", err)
	}

	// Plain Action creates new origin=action item and does NOT trigger restore sync.
	plainGrant, err := n.PanelApplyBegin(ctx, api.PanelApplyBeginRequest{
		Profile:   dir,
		RequestID: "req-plain-1",
		HistoryID: string(richRes.HistoryID),
		Plain:     true,
	})
	if err != nil {
		t.Fatalf("begin plain: %v", err)
	}
	if len(plainGrant.Snapshot.Parts) != 1 || plainGrant.Snapshot.Parts[0].Format != clip.Text || string(plainGrant.Snapshot.Parts[0].Data) != "panel-hello" {
		t.Fatalf("unexpected plain snapshot: %+v", plainGrant.Snapshot)
	}
	plainRep, err := n.PanelApplyComplete(ctx, api.PanelApplyCompleteRequest{
		Profile:   dir,
		ServiceID: genID,
		RequestID: "req-plain-1",
		Token:     plainGrant.Token,
		Outcome:   "applied",
		ReadBack:  &plainGrant.Snapshot,
	})
	if err != nil || !plainRep.Applied || plainRep.Restore != nil {
		t.Fatalf("expected plain apply without restore sync, got rep=%+v err=%v", plainRep, err)
	}

	// File apply holds panelHeldLease until release with changed paths.
	fileGrant, err := n.PanelApplyBegin(ctx, api.PanelApplyBeginRequest{
		Profile:   dir,
		RequestID: "req-file-1",
		HistoryID: string(fileRes.HistoryID),
	})
	if err != nil || !fileGrant.HasFiles || len(fileGrant.Snapshot.Files) != 1 || fileGrant.Snapshot.Files[0].Path != sampleFile {
		t.Fatalf("begin file failed: grant=%+v err=%v", fileGrant, err)
	}
	if _, err := n.PanelApplyComplete(ctx, api.PanelApplyCompleteRequest{
		Profile:   dir,
		ServiceID: genID,
		RequestID: "req-file-1",
		Token:     fileGrant.Token,
		Outcome:   "applied",
		ReadBack:  &fileGrant.Snapshot,
	}); err != nil {
		t.Fatalf("complete file: %v", err)
	}
	n.adapter.mu.Lock()
	heldAfterComplete := n.adapter.panelHeldLease
	n.adapter.mu.Unlock()
	if heldAfterComplete == "" {
		t.Fatal("expected panelHeldLease to be retained after file apply")
	}
	// Release with identical paths keeps lease.
	if err := n.PanelApplyRelease(ctx, api.PanelApplyReleaseRequest{
		Profile:      dir,
		ServiceID:    genID,
		Token:        fileGrant.Token,
		CurrentPaths: []string{sampleFile},
	}); err != nil {
		t.Fatal(err)
	}
	n.adapter.mu.Lock()
	stillHeld := n.adapter.panelHeldLease
	n.adapter.mu.Unlock()
	if stillHeld == "" {
		t.Fatal("expected lease to remain when clipboard still holds same paths")
	}
	// Release with empty/changed paths drops lease.
	if err := n.PanelApplyRelease(ctx, api.PanelApplyReleaseRequest{
		Profile:      dir,
		ServiceID:    genID,
		Token:        fileGrant.Token,
		CurrentPaths: nil,
	}); err != nil {
		t.Fatal(err)
	}
	n.adapter.mu.Lock()
	afterRelease := n.adapter.panelHeldLease
	n.adapter.mu.Unlock()
	if afterRelease != "" {
		t.Fatalf("expected panelHeldLease cleared after release, got %q", afterRelease)
	}

	// Retiring service rejects new panel-apply begin.
	retiring = true
	if _, err := n.PanelApplyBegin(ctx, api.PanelApplyBeginRequest{
		Profile:   dir,
		RequestID: "req-retiring",
		HistoryID: string(imgRes.HistoryID),
	}); err == nil {
		t.Fatal("expected retiring service to reject begin")
	}
}

func TestPanelApplyRejectsCLITemporary(t *testing.T) {
	n, dir, _, cleanup := openPanelTestNode(t, servicelifecycle.ModeCLITemporary, nil, false)
	defer cleanup()
	res, err := n.Submit(context.Background(), api.SubmitInput{
		RequestID: "cli-temp-item",
		Snapshot:  clip.Snapshot{Parts: []clip.Part{{Format: clip.Text, Data: []byte("temp")}}},
	})
	if err != nil {
		t.Fatal(err)
	}
	if _, err := n.PanelApplyBegin(context.Background(), api.PanelApplyBeginRequest{
		Profile:   dir,
		RequestID: "req-temp",
		HistoryID: string(res.HistoryID),
	}); err == nil {
		t.Fatal("expected cli-temporary service to reject panel apply")
	}
}

func TestPanelApplyExpiresAbandonedTransaction(t *testing.T) {
	oldTTL := panelApplyTTL
	panelApplyTTL = 150 * time.Millisecond
	defer func() { panelApplyTTL = oldTTL }()

	ctx := context.Background()
	n, dir, genID, cleanup := openPanelTestNode(t, servicelifecycle.ModeStandalone, nil, false)
	defer cleanup()
	res, err := n.Submit(ctx, api.SubmitInput{
		RequestID: "sub-expire",
		Snapshot:  clip.Snapshot{Parts: []clip.Part{{Format: clip.Text, Data: []byte("expire-me")}}},
	})
	if err != nil {
		t.Fatal(err)
	}
	grant, err := n.PanelApplyBegin(ctx, api.PanelApplyBeginRequest{
		Profile:   dir,
		RequestID: "req-abandoned",
		HistoryID: string(res.HistoryID),
	})
	if err != nil {
		t.Fatal(err)
	}
	time.Sleep(250 * time.Millisecond)
	// Late complete must fail as resource_expired.
	if _, err := n.PanelApplyComplete(ctx, api.PanelApplyCompleteRequest{
		Profile:   dir,
		ServiceID: genID,
		RequestID: "req-abandoned",
		Token:     grant.Token,
		Outcome:   "applied",
		ReadBack:  &grant.Snapshot,
	}); api.ReasonOf(err) != "resource_expired" {
		t.Fatalf("expected resource_expired after TTL, got %v", err)
	}
	// Next explicit panel apply must succeed immediately (Engine ticket freed!).
	grant2, err := n.PanelApplyBegin(ctx, api.PanelApplyBeginRequest{
		Profile:   dir,
		RequestID: "req-recovered",
		HistoryID: string(res.HistoryID),
	})
	if err != nil {
		t.Fatalf("expected recovery after expired tx, got %v", err)
	}
	if _, err := n.PanelApplyComplete(ctx, api.PanelApplyCompleteRequest{
		Profile:   dir,
		ServiceID: genID,
		RequestID: "req-recovered",
		Token:     grant2.Token,
		Outcome:   "applied",
		ReadBack:  &grant2.Snapshot,
	}); err != nil {
		t.Fatalf("complete recovery: %v", err)
	}
}

func TestPanelApplyAlongsideActiveObserverZeroEchoAndSeqContinuity(t *testing.T) {
	ctx, cancel := context.WithCancel(context.Background())
	defer cancel()

	n, dir, genID, cleanup := openPanelTestNode(t, servicelifecycle.ModeStandalone, nil, false)
	defer cleanup()

	// Drive observer session directly against Node.AdapterCall using clipboardintegration.Clipboard!
	board := testboard.Dir(filepath.Join(dir, "board"))
	if err := board.Write(clip.Snapshot{Parts: []clip.Part{{Format: clip.Text, Data: []byte("initial-board")}}}); err != nil {
		t.Fatal(err)
	}
	// Submit two history entries.
	sub1, err := n.Submit(ctx, api.SubmitInput{
		RequestID: "sub-1",
		Snapshot:  clip.Snapshot{Parts: []clip.Part{{Format: clip.Text, Data: []byte("from-history-1")}}},
	})
	if err != nil {
		t.Fatal(err)
	}

	// Simulate observer session calling AdapterCall directly.
	var sess string
	if rep := n.AdapterCall(ctx, api.AdapterRequest{Method: "register"}); rep.Error != nil {
		t.Fatal(rep.Error)
	} else {
		sess = string(rep.Value[1 : len(rep.Value)-1])
	}
	remote := &directAdapterEngine{n: n, session: sess}
	if err := remote.setCaps(ctx); err != nil {
		t.Fatal(err)
	}
	cb := clipboardintegration.New(remote, board, 25*time.Millisecond, nil)
	cb.Initialize(ctx)
	runCtx, stopObs := context.WithCancel(ctx)
	obsDone := make(chan struct{})
	go func() {
		defer close(obsDone)
		cb.Run(runCtx)
	}()
	defer func() { stopObs(); <-obsDone }()

	// Wait until observer baseline is established.
	deadline := time.Now().Add(3 * time.Second)
	for !cb.Ready() && time.Now().Before(deadline) {
		time.Sleep(10 * time.Millisecond)
	}
	if !cb.Ready() {
		t.Fatal("observer never became ready")
	}

	pageBefore, err := n.ListHistory(ctx, api.HistoryQuery{Limit: 50})
	if err != nil {
		t.Fatal(err)
	}

	// Perform panel apply while observer is actively polling every 25ms!
	grant, err := n.PanelApplyBegin(ctx, api.PanelApplyBeginRequest{
		Profile:   dir,
		ServiceID: genID,
		RequestID: "req-with-obs",
		HistoryID: string(sub1.HistoryID),
	})
	if err != nil {
		t.Fatalf("panel begin with observer: %v", err)
	}
	if err := board.Write(grant.Snapshot); err != nil {
		t.Fatal(err)
	}
	back, err := board.Read()
	if err != nil {
		t.Fatal(err)
	}
	if _, err := n.PanelApplyComplete(ctx, api.PanelApplyCompleteRequest{
		Profile:   dir,
		ServiceID: genID,
		RequestID: "req-with-obs",
		Token:     grant.Token,
		Outcome:   "applied",
		ReadBack:  &back,
	}); err != nil {
		t.Fatalf("panel complete with observer: %v", err)
	}

	// Wait multiple observer poll intervals: must NOT create duplicate history (zero echo)!
	time.Sleep(200 * time.Millisecond)
	pageAfter, err := n.ListHistory(ctx, api.HistoryQuery{Limit: 50})
	if err != nil {
		t.Fatal(err)
	}
	if len(pageAfter.Items) != len(pageBefore.Items) {
		t.Fatalf("observer echoed panel write: before=%d after=%d", len(pageBefore.Items), len(pageAfter.Items))
	}

	// Now verify sequence continuity: a real subsequent user copy MUST still be captured by observer on the SAME epoch!
	n.adapter.mu.Lock()
	epochBefore := n.adapter.obsEpoch
	n.adapter.mu.Unlock()

	if err := board.Write(clip.Snapshot{Parts: []clip.Part{{Format: clip.Text, Data: []byte("real-user-copy-after-panel")}}}); err != nil {
		t.Fatal(err)
	}
	deadline = time.Now().Add(3 * time.Second)
	captured := false
	for time.Now().Before(deadline) {
		p, err := n.ListHistory(ctx, api.HistoryQuery{Limit: 50})
		if err == nil && len(p.Items) == len(pageBefore.Items)+1 {
			captured = true
			break
		}
		time.Sleep(20 * time.Millisecond)
	}
	if !captured {
		t.Fatal("observer failed to capture subsequent user copy after panel apply")
	}
	n.adapter.mu.Lock()
	epochAfter := n.adapter.obsEpoch
	n.adapter.mu.Unlock()
	if epochAfter != epochBefore {
		t.Fatalf("observer lost epoch after panel apply: before=%q after=%q", epochBefore, epochAfter)
	}
}

// directAdapterEngine routes clipboardintegration.Engine calls directly through Node.AdapterCall.
type directAdapterEngine struct {
	n       *Node
	session string
}

func (d *directAdapterEngine) call(ctx context.Context, method string, in, out any) error {
	var raw []byte
	if in != nil {
		var err error
		raw, err = json.Marshal(in)
		if err != nil {
			return err
		}
	}
	rep := d.n.AdapterCall(ctx, api.AdapterRequest{Session: d.session, Method: method, Value: raw})
	if rep.Error != nil {
		return rep.Error
	}
	if out != nil && len(rep.Value) > 0 {
		return json.Unmarshal(rep.Value, out)
	}
	return nil
}

func (d *directAdapterEngine) setCaps(ctx context.Context) error {
	return d.call(ctx, "capabilities", engine.HostCapabilities{
		ReadFormats:  []string{engine.MimeText, engine.MimeHTML, engine.MimePNG},
		WriteFormats: []string{engine.MimeText, engine.MimeHTML, engine.MimePNG},
		ReadFiles:    true,
		WriteFiles:   true,
		Observe:      engine.AbilityAvailable,
		Apply:        engine.AbilityAvailable,
	}, nil)
}

func (d *directAdapterEngine) ImportContent(ctx context.Context, in engine.ContentInput) (out engine.ContentHandle, err error) {
	s := clip.Snapshot{CopyID: in.ObservationKey}
	for _, p := range in.Representations {
		b := make([]byte, p.Size)
		_, _ = p.Reader.Read(b)
		s.Parts = append(s.Parts, clip.Part{Format: p.MIME, Data: b})
	}
	err = d.call(ctx, "import", api.AdapterImport{Snapshot: s}, &out)
	return
}
func (d *directAdapterEngine) RetainContent(ctx context.Context, id engine.ContentID) (out engine.ContentHandle, err error) {
	err = d.call(ctx, "retain", id, &out)
	return
}
func (d *directAdapterEngine) ReleaseContent(ctx context.Context, id engine.LeaseID) error {
	return d.call(ctx, "release", id, nil)
}
func (d *directAdapterEngine) ReadContent(ctx context.Context, l engine.LeaseID, p engine.PartID) (io.ReadCloser, error) {
	return d.n.AdapterContent(ctx, d.session, l, p)
}
func (d *directAdapterEngine) LocalFilePath(ctx context.Context, l engine.LeaseID, p engine.PartID) (out string, err error) {
	err = d.call(ctx, "path", struct {
		Lease engine.LeaseID
		Part  engine.PartID
	}{l, p}, &out)
	return
}
func (d *directAdapterEngine) FileReference(ctx context.Context, l engine.LeaseID, p engine.PartID) (out string, err error) {
	err = d.call(ctx, "reference", struct {
		Lease engine.LeaseID
		Part  engine.PartID
	}{l, p}, &out)
	return
}
func (d *directAdapterEngine) ObserveClipboard(ctx context.Context, o engine.Observation) (out engine.ObservationResult, err error) {
	err = d.call(ctx, "observe", o, &out)
	return
}
func (d *directAdapterEngine) BeginApply(ctx context.Context, r engine.ApplyRequest) (out engine.ApplyTicket, err error) {
	err = d.call(ctx, "apply", r, &out)
	return
}
func (d *directAdapterEngine) BeginApplyHistory(ctx context.Context, r engine.ApplyHistoryRequest) (out engine.ApplyTicket, err error) {
	err = d.call(ctx, "apply-history", r, &out)
	return
}
func (d *directAdapterEngine) ReportApplyResult(ctx context.Context, r engine.ApplyResult) error {
	return d.call(ctx, "report", r, nil)
}
func (d *directAdapterEngine) ListReceived(ctx context.Context) (out []engine.ReceivedContent, err error) {
	err = d.call(ctx, "received", nil, &out)
	return
}
func (d *directAdapterEngine) DismissReceived(ctx context.Context, id engine.ReceiptID) error {
	return d.call(ctx, "dismiss", id, nil)
}
func (d *directAdapterEngine) ReconcileClipboardFiles(ctx context.Context, paths []string) error {
	return d.call(ctx, "reconcile", paths, nil)
}
func (d *directAdapterEngine) SubmitContent(ctx context.Context, r engine.SubmitRequest) (out engine.OperationID, err error) {
	err = d.call(ctx, "submit", r, &out)
	return
}
func (d *directAdapterEngine) SyncRestoredHistory(ctx context.Context, token engine.ApplyToken, lease engine.LeaseID) (out engine.RestoreSyncResult, err error) {
	err = d.call(ctx, "restore-sync", struct {
		Token engine.ApplyToken
		Lease engine.LeaseID
	}{token, lease}, &out)
	return
}
