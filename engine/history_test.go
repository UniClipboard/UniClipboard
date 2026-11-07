package engine

import (
	"context"
	"database/sql"
	"fmt"
	"github.com/uniclipboard/unic/internal/store"
	"io"
	"os"
	"path/filepath"
	"sync"
	"testing"
	"time"
)

var commitLeases = map[string]LeaseID{}

func leaseOf(t *testing.T, _ *Engine, id string) LeaseID { return commitLeases[id] }

func commitText(t *testing.T, e *Engine, id, text string) SubmissionResult {
	t.Helper()
	h := importText(t, e, text)
	commitLeases[id] = h.LeaseID
	r, err := e.CommitContent(bg, CommitRequest{RequestID: RequestID(id), LeaseID: h.LeaseID})
	if err != nil {
		t.Fatal(err)
	}
	_ = e.ReleaseContent(bg, h.LeaseID)
	return r
}

func TestCommitPersistsHistoryWithoutTargets(t *testing.T) {
	n := newContentNode(t, nil)
	start(t, n)
	r := commitText(t, n.e, "h1", "hello history")
	if r.HistoryID == "" || r.Phase != SubmissionCommitted || r.SyncDisposition != SyncNoTargets {
		t.Fatalf("%+v", r)
	}
	it, err := n.e.GetItem(bg, r.HistoryID)
	if err != nil {
		t.Fatal(err)
	}
	if it.Revision != 1 || it.Kind != HistoryText || it.Origin.Kind != OriginKindExplicit || it.TotalBytes != 13 || it.SyncState != HistorySyncNotRequested || len(it.Parts) != 1 || it.Tags == nil {
		t.Fatalf("%+v", it)
	}
	// Retry of the same request returns the same record, nothing new is stored.
	if r2, err := n.e.CommitContent(bg, CommitRequest{RequestID: "h1", LeaseID: leaseOf(t, n.e, "h1")}); err != nil || r2.HistoryID != r.HistoryID {
		t.Fatal("retry created a second record", r2, err)
	}
	h, err := n.e.RetainHistoryContent(bg, r.HistoryID)
	if err != nil {
		t.Fatal(err)
	}
	rd, err := n.e.ReadContent(bg, h.LeaseID, h.Content.Parts[0].ID)
	if err != nil {
		t.Fatal(err)
	}
	b, _ := io.ReadAll(rd)
	_ = rd.Close()
	if string(b) != "hello history" {
		t.Fatalf("read %q", b)
	}
	if _, err := n.e.GetItem(bg, "nope"); code(err) != CodeNotFound {
		t.Fatal(err)
	}
}

func TestPersistFalseLeavesNoRecord(t *testing.T) {
	n := newContentNode(t, nil)
	n.e.processors = []*pipelineProcessor{newPipelineProcessor("nop", []pipelineEvent{pipelineCaptured}, false, func(_ context.Context, c *processingContext) error {
		return c.SetPersist(false)
	})}
	start(t, n)
	r := commitText(t, n.e, "np", "secret")
	if r.HistoryID != "" || r.Phase != SubmissionCommitted {
		t.Fatalf("%+v", r)
	}
	page, err := n.e.ListHistory(bg, ListHistoryRequest{})
	if err != nil || len(page.Items) != 0 {
		t.Fatal(page, err)
	}
}

func TestListHistoryPagingAndCursorRules(t *testing.T) {
	n := newContentNode(t, nil)
	start(t, n)
	for i := 0; i < 5; i++ {
		commitText(t, n.e, "p"+string(rune('a'+i)), "item "+string(rune('a'+i)))
	}
	p1, err := n.e.ListHistory(bg, ListHistoryRequest{Limit: 2})
	if err != nil || len(p1.Items) != 2 || p1.NextCursor == "" || p1.HistoryRevision != 5 {
		t.Fatalf("%+v %v", p1, err)
	}
	commitText(t, n.e, "late", "added during the walk")
	p2, err := n.e.ListHistory(bg, ListHistoryRequest{Limit: 10, Cursor: p1.NextCursor})
	if err != nil || len(p2.Items) != 3 || p2.NextCursor != "" {
		t.Fatalf("%+v %v", p2, err)
	}
	seen := map[HistoryID]bool{}
	for _, it := range append(p1.Items, p2.Items...) {
		if seen[it.ID] {
			t.Fatal("duplicate across pages")
		}
		seen[it.ID] = true
	}
	if _, err := n.e.ListHistory(bg, ListHistoryRequest{Cursor: p1.NextCursor, Filter: HistoryFilter{Kinds: []HistoryKind{HistoryImage}}}); code(err) != CodeConflict {
		t.Fatalf("changed filter: %v", err)
	}
	if _, err := n.e.ListHistory(bg, ListHistoryRequest{Cursor: p1.NextCursor + "x"}); code(err) != CodeInvalidArgument {
		t.Fatalf("tampered: %v", err)
	}
	if _, err := n.e.ListHistory(bg, ListHistoryRequest{Limit: 500}); code(err) != CodeInvalidArgument {
		t.Fatal(err)
	}
	if _, err := n.e.ListHistory(bg, ListHistoryRequest{Filter: HistoryFilter{Kinds: []HistoryKind{"bogus"}}}); code(err) != CodeInvalidArgument {
		t.Fatal(err)
	}
	all, _ := n.e.ListHistory(bg, ListHistoryRequest{Filter: HistoryFilter{Kinds: []HistoryKind{HistoryText}}})
	if len(all.Items) != 6 {
		t.Fatalf("%d", len(all.Items))
	}
}

func TestHistorySurvivesRestartAndKeepsFiles(t *testing.T) {
	dir := t.TempDir()
	open := func() *Engine {
		opts, _ := newOpts(t, dir)
		e, err := New(bg, opts)
		if err != nil {
			t.Fatal(err)
		}
		return e
	}
	e := open()
	h, err := e.ImportContent(bg, ContentInput{Files: []FileInput{fileIn("doc.txt", "file body")}})
	if err != nil {
		t.Fatal(err)
	}
	if err := e.Start(bg); err != nil {
		t.Fatal(err)
	}
	r, err := e.CommitContent(bg, CommitRequest{RequestID: "f1", LeaseID: h.LeaseID})
	if err != nil || r.HistoryID == "" {
		t.Fatal(r, err)
	}
	_ = e.ReleaseContent(bg, h.LeaseID) // the inbox copy is gone; history owns its own
	if err := e.Close(bg); err != nil {
		t.Fatal(err)
	}
	e = open()
	defer func() { _ = e.Close(bg) }()
	it, err := e.GetItem(bg, r.HistoryID)
	if err != nil || it.Kind != HistoryFiles {
		t.Fatalf("%+v %v", it, err)
	}
	rh, err := e.RetainHistoryContent(bg, r.HistoryID)
	if err != nil {
		t.Fatal(err)
	}
	rd, err := e.ReadContent(bg, rh.LeaseID, rh.Content.Parts[0].ID)
	if err != nil {
		t.Fatal(err)
	}
	b, _ := io.ReadAll(rd)
	_ = rd.Close()
	if string(b) != "file body" || rh.Content.Parts[0].Name != "doc.txt" {
		t.Fatalf("%q %+v", b, rh.Content.Parts[0])
	}
}

func TestOrphanBlobsAreRemovedAtStartup(t *testing.T) {
	dir := t.TempDir()
	opts, _ := newOpts(t, dir)
	e, err := New(bg, opts)
	if err != nil {
		t.Fatal(err)
	}
	orphan := e.hist.dir + "/deadbeef"
	_ = e.Close(bg)
	if err := os.MkdirAll(orphan, 0o700); err != nil {
		t.Fatal(err)
	}
	e, err = New(bg, opts)
	if err != nil {
		t.Fatal(err)
	}
	defer func() { _ = e.Close(bg) }()
	if _, err := os.Stat(orphan); !os.IsNotExist(err) {
		t.Fatal("unreferenced blob survived startup")
	}
}

type refSource struct{ pathSource }

func (r refSource) PersistentRef() string { return r.path }

func TestReferencedFilesAreRememberedNotCopied(t *testing.T) {
	dir := t.TempDir()
	file := filepath.Join(t.TempDir(), "big.bin")
	if err := os.WriteFile(file, []byte("original"), 0o600); err != nil {
		t.Fatal(err)
	}
	open := func(resolve bool) *Engine {
		opts, _ := newOpts(t, dir)
		if resolve {
			opts.ResolveFileRef = func(ref string) FileSource { return pathSource{ref} }
		}
		e, err := New(bg, opts)
		if err != nil {
			t.Fatal(err)
		}
		return e
	}
	e := open(true)
	h, err := e.ImportContent(bg, ContentInput{Files: []FileInput{{Name: "big.bin", Size: 8, Source: refSource{pathSource{file}}}}})
	if err != nil {
		t.Fatal(err)
	}
	if err := e.Start(bg); err != nil {
		t.Fatal(err)
	}
	r, err := e.CommitContent(bg, CommitRequest{RequestID: "ref", LeaseID: h.LeaseID})
	if err != nil || r.HistoryID == "" {
		t.Fatal(r, err)
	}
	if ents, _ := os.ReadDir(e.hist.dir); len(ents) != 0 {
		t.Fatalf("a referenced file was copied into history: %d entries", len(ents))
	}
	_ = e.Close(bg)

	read := func(e *Engine) error {
		rh, err := e.RetainHistoryContent(bg, r.HistoryID)
		if err != nil {
			t.Fatal(err)
		}
		rd, err := e.ReadContent(bg, rh.LeaseID, rh.Content.Parts[0].ID)
		if err != nil {
			return err
		}
		defer func() { _ = rd.Close() }()
		b, _ := io.ReadAll(rd)
		if string(b) != "original" {
			t.Fatalf("read %q", b)
		}
		return nil
	}
	e = open(true)
	if err := read(e); err != nil {
		t.Fatal(err)
	}
	_ = e.Close(bg)

	// Without a resolver, or once the file changed, reading says source_changed.
	e = open(false)
	if err := read(e); code(err) != CodeSourceChanged {
		t.Fatalf("no resolver: %v", err)
	}
	_ = e.Close(bg)
	if err := os.WriteFile(file, []byte("CHANGED!"), 0o600); err != nil {
		t.Fatal(err)
	}
	later := time.Now().Add(3 * time.Second)
	_ = os.Chtimes(file, later, later)
	e = open(true)
	defer func() { _ = e.Close(bg) }()
	rh, _ := e.RetainHistoryContent(bg, r.HistoryID)
	if _, err := e.ReadContent(bg, rh.LeaseID, rh.Content.Parts[0].ID); code(err) != CodeSourceChanged {
		t.Fatalf("changed file: %v", err)
	}
	if it, err := e.GetItem(bg, r.HistoryID); err != nil || it.TotalBytes != 8 {
		t.Fatal(it, err)
	}
}

func TestUpdateAndDeleteRules(t *testing.T) {
	n := newContentNode(t, nil)
	start(t, n)
	sub, err := n.e.SubscribeEvents(bg)
	if err != nil {
		t.Fatal(err)
	}
	defer func() { _ = sub.Close() }()
	r := commitText(t, n.e, "u1", "to update")
	ev, _ := sub.Next(bg)
	if ev.Kind != EventHistory || len(ev.Objects) != 1 || ev.Objects[0].ID != string(r.HistoryID) {
		t.Fatalf("event %+v", ev)
	}
	if st := state(t, n.e); st.HistoryRevision != 1 {
		t.Fatalf("HistoryRevision = %d", st.HistoryRevision)
	}
	tags := []string{"b", "a", "a"}
	it, err := n.e.UpdateItem(bg, UpdateItemRequest{ID: r.HistoryID, ExpectedRevision: 1, Tags: &tags})
	if err != nil || it.Revision != 2 || len(it.Tags) != 2 || it.Tags[0] != "a" {
		t.Fatalf("%+v %v", it, err)
	}
	if _, err := n.e.UpdateItem(bg, UpdateItemRequest{ID: r.HistoryID, ExpectedRevision: 1, Tags: &tags}); code(err) != CodeConflict {
		t.Fatalf("stale revision: %v", err)
	}
	same := []string{"a", "b"}
	if it2, err := n.e.UpdateItem(bg, UpdateItemRequest{ID: r.HistoryID, ExpectedRevision: 2, Tags: &same}); err != nil || it2.Revision != 2 {
		t.Fatalf("no-op update changed the revision: %+v %v", it2, err)
	}
	past := int64(1)
	for _, bad := range []UpdateItemRequest{
		{ID: r.HistoryID, ExpectedRevision: 2},
		{ID: r.HistoryID, ExpectedRevision: 2, ExpiresAtMs: &past},
		{ID: r.HistoryID, Tags: &tags},
	} {
		if _, err := n.e.UpdateItem(bg, bad); code(err) != CodeInvalidArgument {
			t.Fatalf("%+v: %v", bad, err)
		}
	}
	// A reader opened before the delete keeps working; new requests do not.
	h, err := n.e.RetainHistoryContent(bg, r.HistoryID)
	if err != nil {
		t.Fatal(err)
	}
	if err := n.e.DeleteItem(bg, DeleteItemRequest{ID: r.HistoryID, ExpectedRevision: 1}); code(err) != CodeConflict {
		t.Fatal(err)
	}
	if err := n.e.DeleteItem(bg, DeleteItemRequest{ID: r.HistoryID, ExpectedRevision: 2}); err != nil {
		t.Fatal(err)
	}
	if _, err := n.e.GetItem(bg, r.HistoryID); code(err) != CodeNotFound {
		t.Fatal(err)
	}
	if err := n.e.DeleteItem(bg, DeleteItemRequest{ID: r.HistoryID, ExpectedRevision: 2}); code(err) != CodeNotFound {
		t.Fatalf("delete is not idempotent: %v", err)
	}
	if _, err := n.e.RetainHistoryContent(bg, r.HistoryID); code(err) != CodeNotFound {
		t.Fatal(err)
	}
	rd, err := n.e.ReadContent(bg, h.LeaseID, h.Content.Parts[0].ID)
	if err != nil {
		t.Fatal(err)
	}
	if b, _ := io.ReadAll(rd); string(b) != "to update" {
		t.Fatalf("%q", b)
	}
	_ = rd.Close()
}

func TestDeletedFileBlobSurvivesUntilReleased(t *testing.T) {
	n := newContentNode(t, nil)
	start(t, n)
	in, err := n.e.ImportContent(bg, ContentInput{Files: []FileInput{fileIn("keep.txt", "still here")}})
	if err != nil {
		t.Fatal(err)
	}
	r, err := n.e.CommitContent(bg, CommitRequest{RequestID: "blob", LeaseID: in.LeaseID})
	if err != nil {
		t.Fatal(err)
	}
	_ = n.e.ReleaseContent(bg, in.LeaseID)
	h, _ := n.e.RetainHistoryContent(bg, r.HistoryID)
	if err := n.e.DeleteItem(bg, DeleteItemRequest{ID: r.HistoryID, ExpectedRevision: 1}); err != nil {
		t.Fatal(err)
	}
	rd, err := n.e.ReadContent(bg, h.LeaseID, h.Content.Parts[0].ID)
	if err != nil {
		t.Fatalf("a held blob vanished with its record: %v", err)
	}
	_ = rd.Close()
	_ = n.e.ReleaseContent(bg, h.LeaseID)
	eventually(t, "blob removed once nothing holds it", func() bool {
		ents, _ := os.ReadDir(n.e.hist.dir)
		return len(ents) == 0
	})
}

func TestExpirySweepRemovesRecords(t *testing.T) {
	old := historySweepEvery
	historySweepEvery = 20 * time.Millisecond
	t.Cleanup(func() { historySweepEvery = old })
	n := newContentNode(t, nil)
	start(t, n)
	r := commitText(t, n.e, "exp", "short lived")
	at := time.Now().Add(150 * time.Millisecond).UnixMilli()
	if _, err := n.e.UpdateItem(bg, UpdateItemRequest{ID: r.HistoryID, ExpectedRevision: 1, ExpiresAtMs: &at}); err != nil {
		t.Fatal(err)
	}
	eventually(t, "expired and hidden", func() bool {
		_, err := n.e.GetItem(bg, r.HistoryID)
		return code(err) == CodeNotFound
	})
	eventually(t, "row physically gone", func() bool {
		items, _, _, _ := n.e.st.ListHistory(store.HistoryQuery{Limit: 10})
		return len(items) == 0
	})
}

func TestHistoryEvictsOldestOverCount(t *testing.T) {
	n := newContentNode(t, func(o *Options) { o.History.MaxItems = 2 })
	start(t, n)
	a := commitText(t, n.e, "e1", "one")
	commitText(t, n.e, "e2", "two")
	c := commitText(t, n.e, "e3", "three")
	if _, err := n.e.GetItem(bg, a.HistoryID); code(err) != CodeNotFound {
		t.Fatalf("oldest survived: %v", err)
	}
	if _, err := n.e.GetItem(bg, c.HistoryID); err != nil {
		t.Fatal(err)
	}
	page, _ := n.e.ListHistory(bg, ListHistoryRequest{})
	if len(page.Items) != 2 {
		t.Fatalf("%d items", len(page.Items))
	}
}

func TestBeginApplyHistoryUsesTheSharedSlot(t *testing.T) {
	n := newContentNode(t, withHost)
	start(t, n)
	hb := newHost(t, n.e)
	hb.must(ObserveBaseline, "")
	r := commitText(t, n.e, "ah1", "from history")
	req := ApplyHistoryRequest{HistoryID: r.HistoryID, ExpectedEpoch: hb.epoch, ExpectedSequence: hb.seq}

	stale := req
	stale.ExpectedSequence--
	if _, err := n.e.BeginApplyHistory(bg, stale); code(err) != CodeConflict {
		t.Fatalf("stale observation: %v", err)
	}
	if _, err := n.e.BeginApplyHistory(bg, ApplyHistoryRequest{HistoryID: "nope", ExpectedEpoch: hb.epoch, ExpectedSequence: hb.seq}); code(err) != CodeNotFound {
		t.Fatal(err)
	}
	// Pausing sync does not matter for a local record, and does not void the ticket.
	updateSettings(t, n.e, func(s *Settings) { s.Sync.Enabled = false })
	tk, err := n.e.BeginApplyHistory(bg, req)
	if err != nil {
		t.Fatal(err)
	}
	updateSettings(t, n.e, func(s *Settings) { s.Sync.Enabled = false; s.DeviceName = "renamed" })
	if st := state(t, n.e); st.ActiveApply == nil || st.ActiveApply.Kind != ApplyKindHistory || st.ActiveApply.HistoryID != r.HistoryID {
		t.Fatalf("%+v", st.ActiveApply)
	}
	if _, err := n.e.BeginApplyHistory(bg, req); code(err) != CodeInUse {
		t.Fatalf("second apply: %v", err)
	}
	// Deleting the record mid-application leaves the ticket's content readable.
	if err := n.e.DeleteItem(bg, DeleteItemRequest{ID: r.HistoryID, ExpectedRevision: 1}); err != nil {
		t.Fatal(err)
	}
	if got := readParts(t, n.e, tk); got[MimeText] != "from history" {
		t.Fatalf("ticket content = %v", got)
	}
	// The write comes back marked with the token: an echo, not a new item.
	if res, err := hb.observe(ObserveChange, "from history", tk.Token); err != nil || res.Disposition != DispEcho {
		t.Fatalf("echo = %+v, %v", res, err)
	}
	back := importText(t, n.e, "from history")
	hb.seq++
	if err := n.e.ReportApplyResult(bg, ApplyResult{Token: tk.Token, Outcome: ApplyOutcomeApplied, BaselineLeaseID: back.LeaseID, Epoch: hb.epoch, Sequence: hb.seq}); err != nil {
		t.Fatal(err)
	}
	if st := state(t, n.e); st.ActiveApply != nil {
		t.Fatal("slot not freed")
	}
	if page, _ := n.e.ListHistory(bg, ListHistoryRequest{}); len(page.Items) != 0 {
		t.Fatalf("applying created history: %d items", len(page.Items))
	}
}

func TestBeginApplyHistoryNeedsHostAbility(t *testing.T) {
	n := newContentNode(t, nil) // no host abilities
	start(t, n)
	r := commitText(t, n.e, "ah2", "x")
	if _, err := n.e.BeginApplyHistory(bg, ApplyHistoryRequest{HistoryID: r.HistoryID}); code(err) != CodeUnsupported {
		t.Fatal(err)
	}
}

// ---- fault injection (history-contract §8) ----

func entries(t *testing.T, dir string) int {
	t.Helper()
	ents, err := os.ReadDir(dir)
	if err != nil {
		t.Fatal(err)
	}
	return len(ents)
}

func TestFailedDatabaseWriteFailsTheCommitAndLeavesNoDebris(t *testing.T) {
	n := newContentNode(t, nil)
	start(t, n)
	good := commitText(t, n.e, "keep", "earlier history")

	// Make every further insert fail from outside, as a full or broken database would.
	db, err := sql.Open("sqlite", "file:"+filepath.Join(n.dir, "unic.db"))
	if err != nil {
		t.Fatal(err)
	}
	defer func() { _ = db.Close() }()
	if _, err := db.Exec(`CREATE TRIGGER inject BEFORE INSERT ON clip_items BEGIN SELECT RAISE(ABORT, 'injected'); END`); err != nil {
		t.Fatal(err)
	}
	in, err := n.e.ImportContent(bg, ContentInput{Files: []FileInput{fileIn("big.bin", "payload")}})
	if err != nil {
		t.Fatal(err)
	}
	r, err := n.e.CommitContent(bg, CommitRequest{RequestID: "doomed", LeaseID: in.LeaseID})
	if code(err) != CodeUnavailable || r.Phase != SubmissionFailed || r.HistoryID != "" || r.OperationID != "" {
		t.Fatalf("%+v %v", r, err)
	}
	if got := entries(t, n.e.hist.dir); got != 0 {
		t.Fatalf("%d blobs left behind by the failed commit", got)
	}
	// The failure is the cached answer for this request, and earlier history is untouched.
	if r2, err := n.e.GetSubmission(bg, "doomed"); err != nil || r2.Phase != SubmissionFailed {
		t.Fatalf("%+v %v", r2, err)
	}
	if _, err := n.e.GetItem(bg, good.HistoryID); err != nil {
		t.Fatal(err)
	}
	// Once the database works again a fresh request succeeds.
	if _, err := db.Exec(`DROP TRIGGER inject`); err != nil {
		t.Fatal(err)
	}
	if r3, err := n.e.CommitContent(bg, CommitRequest{RequestID: "retry", LeaseID: in.LeaseID}); err != nil || r3.HistoryID == "" {
		t.Fatalf("%+v %v", r3, err)
	}
}

func TestUnwritableBlobDirectoryFailsTheCommit(t *testing.T) {
	if os.Geteuid() == 0 {
		t.Skip("permissions do not bind root")
	}
	n := newContentNode(t, nil)
	start(t, n)
	if err := os.Chmod(n.e.hist.dir, 0o500); err != nil {
		t.Fatal(err)
	}
	defer func() { _ = os.Chmod(n.e.hist.dir, 0o700) }()
	in, _ := n.e.ImportContent(bg, ContentInput{Files: []FileInput{fileIn("f.bin", "x")}})
	r, err := n.e.CommitContent(bg, CommitRequest{RequestID: "ro", LeaseID: in.LeaseID})
	if err == nil || r.Phase != SubmissionFailed {
		t.Fatalf("%+v %v", r, err)
	}
	if page, _ := n.e.ListHistory(bg, ListHistoryRequest{}); len(page.Items) != 0 {
		t.Fatal("a record exists although its blob could not be written")
	}
}

func TestOversizedItemIsRefusedWithoutDeletingHistory(t *testing.T) {
	n := newContentNode(t, func(o *Options) { o.History.MaxBytes = 16 })
	start(t, n)
	old := commitText(t, n.e, "small", "tiny")
	big := importText(t, n.e, "this text is longer than sixteen bytes")
	r, err := n.e.CommitContent(bg, CommitRequest{RequestID: "big", LeaseID: big.LeaseID})
	if code(err) != CodeResourceExhausted || r.Phase != SubmissionFailed {
		t.Fatalf("%+v %v", r, err)
	}
	if _, err := n.e.GetItem(bg, old.HistoryID); err != nil {
		t.Fatalf("an oversized commit evicted older history: %v", err)
	}
}

func TestByteLimitEvictsOldestAndFreesTheirBlobs(t *testing.T) {
	n := newContentNode(t, func(o *Options) { o.History.MaxBytes = 20 })
	start(t, n)
	file := func(id, body string) SubmissionResult {
		in, _ := n.e.ImportContent(bg, ContentInput{Files: []FileInput{fileIn(id+".bin", body)}})
		r, err := n.e.CommitContent(bg, CommitRequest{RequestID: RequestID(id), LeaseID: in.LeaseID})
		if err != nil {
			t.Fatal(err)
		}
		_ = n.e.ReleaseContent(bg, in.LeaseID)
		return r
	}
	a := file("a", "0123456789") // 10
	b := file("b", "0123456789") // 20: fits
	c := file("c", "0123456789") // 30: the oldest must go
	if _, err := n.e.GetItem(bg, a.HistoryID); code(err) != CodeNotFound {
		t.Fatalf("oldest survived: %v", err)
	}
	for _, r := range []SubmissionResult{b, c} {
		if _, err := n.e.GetItem(bg, r.HistoryID); err != nil {
			t.Fatal(err)
		}
	}
	eventually(t, "evicted blob removed", func() bool { return entries(t, n.e.hist.dir) == 2 })
}

func TestConcurrentCommitsKeepTheLimitAndNeverDuplicate(t *testing.T) {
	n := newContentNode(t, func(o *Options) { o.History.MaxItems = 5 })
	start(t, n)
	var wg sync.WaitGroup
	for i := 0; i < 20; i++ {
		wg.Add(1)
		go func() {
			defer wg.Done()
			h := importText(t, n.e, fmt.Sprintf("item %d", i))
			if _, err := n.e.CommitContent(bg, CommitRequest{RequestID: RequestID(fmt.Sprintf("c%d", i)), LeaseID: h.LeaseID}); err != nil {
				t.Error(err)
			}
		}()
	}
	wg.Wait()
	page, err := n.e.ListHistory(bg, ListHistoryRequest{Limit: 200})
	if err != nil || len(page.Items) != 5 {
		t.Fatalf("%d items, %v", len(page.Items), err)
	}
	seen := map[HistoryID]bool{}
	for _, it := range page.Items {
		if seen[it.ID] {
			t.Fatal("duplicate")
		}
		seen[it.ID] = true
	}
}

func TestHistoryInStoppedEngine(t *testing.T) {
	n := newContentNode(t, nil)
	start(t, n)
	r := commitText(t, n.e, "s1", "survives stop")
	if err := n.e.Stop(bg); err != nil {
		t.Fatal(err)
	}
	// Reads work after Stop; changes do not; nothing times out in the background.
	if _, err := n.e.GetItem(bg, r.HistoryID); err != nil {
		t.Fatal(err)
	}
	if _, err := n.e.RetainHistoryContent(bg, r.HistoryID); err != nil {
		t.Fatal(err)
	}
	tags := []string{"x"}
	if _, err := n.e.UpdateItem(bg, UpdateItemRequest{ID: r.HistoryID, ExpectedRevision: 1, Tags: &tags}); code(err) != CodeInvalidState {
		t.Fatalf("update after stop: %v", err)
	}
	if err := n.e.DeleteItem(bg, DeleteItemRequest{ID: r.HistoryID, ExpectedRevision: 1}); code(err) != CodeInvalidState {
		t.Fatalf("delete after stop: %v", err)
	}
	if _, err := n.e.BeginApplyHistory(bg, ApplyHistoryRequest{HistoryID: r.HistoryID}); code(err) != CodeInvalidState {
		t.Fatalf("apply after stop: %v", err)
	}
	if err := n.e.Close(bg); err != nil {
		t.Fatal(err)
	}
	if _, err := n.e.GetItem(bg, r.HistoryID); code(err) != CodeClosed {
		t.Fatalf("after close: %v", err)
	}
}

func TestCrashLeftoversAreCleanedAndValidHistoryKept(t *testing.T) {
	dir := t.TempDir()
	opts, _ := newOpts(t, dir)
	e, err := New(bg, opts)
	if err != nil {
		t.Fatal(err)
	}
	if err := e.Start(bg); err != nil {
		t.Fatal(err)
	}
	in, _ := e.ImportContent(bg, ContentInput{Files: []FileInput{fileIn("keep.bin", "keep me")}})
	r, err := e.CommitContent(bg, CommitRequest{RequestID: "k", LeaseID: in.LeaseID})
	if err != nil {
		t.Fatal(err)
	}
	histDir := e.hist.dir
	_ = e.Close(bg)
	// A crash mid-commit leaves a half-written temporary directory and an unreferenced blob.
	_ = os.MkdirAll(filepath.Join(histDir, "tmp-abc"), 0o700)
	_ = os.WriteFile(filepath.Join(histDir, "tmp-abc", "data"), []byte("half"), 0o600)
	_ = os.MkdirAll(filepath.Join(histDir, "ffff"), 0o700)
	e, err = New(bg, opts)
	if err != nil {
		t.Fatal(err)
	}
	defer func() { _ = e.Close(bg) }()
	if got := entries(t, histDir); got != 1 {
		t.Fatalf("%d entries after cleanup, want only the referenced blob", got)
	}
	h, err := e.RetainHistoryContent(bg, r.HistoryID)
	if err != nil {
		t.Fatal(err)
	}
	rd, err := e.ReadContent(bg, h.LeaseID, h.Content.Parts[0].ID)
	if err != nil {
		t.Fatal(err)
	}
	if b, _ := io.ReadAll(rd); string(b) != "keep me" {
		t.Fatalf("%q", b)
	}
	_ = rd.Close()
}
