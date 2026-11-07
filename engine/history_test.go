package engine

import (
	"context"
	"io"
	"os"
	"testing"
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
