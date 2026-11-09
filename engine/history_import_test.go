package engine

import (
	"bytes"
	"context"
	"io"
	"strings"
	"testing"
	"time"
)

func TestHistoryMigrationMergeResumeAndReopen(t *testing.T) {
	ctx := context.Background()
	dir := t.TempDir()
	open := func(migrate bool) *Engine {
		e, err := New(ctx, Options{DataDir: dir, MigrationMode: migrate})
		if err != nil {
			t.Fatal(err)
		}
		return e
	}
	e := open(false)
	if err := e.Start(ctx); err != nil {
		t.Fatal(err)
	}
	h, err := e.ImportContent(ctx, ContentInput{Representations: []RepresentationInput{{MIME: MimeText, Size: 4, Reader: strings.NewReader("same")}}})
	if err != nil {
		t.Fatal(err)
	}
	existing, err := e.CommitContent(ctx, CommitRequest{RequestID: "existing", LeaseID: h.LeaseID})
	if err != nil {
		t.Fatal(err)
	}
	_ = e.ReleaseContent(ctx, h.LeaseID)
	if err = e.Close(ctx); err != nil {
		t.Fatal(err)
	}
	e = open(true)
	if err = e.Start(ctx); err == nil {
		t.Fatal("migration instance must not start networking")
	}
	importOne := func(id string, created int64) HistoryImportResult {
		t.Helper()
		r, err := e.ImportHistory(ctx, HistoryImportRequest{
			Source: "source-a", RecordID: id, CreatedAtMs: created, UpdatedAtMs: created + 1,
			Tags: []string{strings.Repeat("中", 64), "Work"},
			Content: ContentInput{Representations: []RepresentationInput{{MIME: MimeText, Size: 4, Reader: strings.NewReader("same")}},
				Files: []FileInput{{Name: "test.bin", Size: 3, Reader: bytes.NewReader([]byte{0, 1, 2})}}},
		})
		if err != nil {
			t.Fatal(err)
		}
		return r
	}
	now := time.Now().UnixMilli()
	old := importOne("old", now-100000)
	newer := importOne("newer", now-50000)
	retry := importOne("old", now-100000)
	if !retry.AlreadyImported || retry.HistoryID != old.HistoryID {
		t.Fatal("retry duplicated history")
	}
	if err = e.ImportHistoryTags(ctx, []string{"unassigned"}); err != nil {
		t.Fatal(err)
	}
	if err = e.Close(ctx); err != nil {
		t.Fatal(err)
	}
	e = open(false)
	defer func() { _ = e.Close(ctx) }()
	page, err := e.ListHistory(ctx, ListHistoryRequest{Limit: 1})
	if err != nil || len(page.Items) != 1 || page.Items[0].ID != existing.HistoryID {
		t.Fatal("existing history was changed or reordered", err)
	}
	ids := []HistoryID{page.Items[0].ID}
	for page.NextCursor != "" {
		page, err = e.ListHistory(ctx, ListHistoryRequest{Limit: 1, Cursor: page.NextCursor})
		if err != nil {
			t.Fatal(err)
		}
		for _, it := range page.Items {
			ids = append(ids, it.ID)
		}
		if len(ids) > 3 {
			t.Fatal("pagination repeated")
		}
	}
	if len(ids) != 3 || ids[1] != newer.HistoryID || ids[2] != old.HistoryID {
		t.Fatal("migration order or record identity lost")
	}
	it, err := e.GetItem(ctx, old.HistoryID)
	if err != nil || it.CreatedAtMs != now-100000 || it.UpdatedAtMs != now-99999 || len(it.Tags) != 2 || it.SyncState != HistorySyncNotRequested {
		t.Fatal("metadata lost", err)
	}
	h, err = e.RetainHistoryContent(ctx, old.HistoryID)
	if err != nil {
		t.Fatal(err)
	}
	for i, want := range [][]byte{[]byte("same"), {0, 1, 2}} {
		rd, err := e.ReadContent(ctx, h.LeaseID, h.Content.Parts[i].ID)
		if err != nil {
			t.Fatal(err)
		}
		got, err := io.ReadAll(rd)
		_ = rd.Close()
		if err != nil || !bytes.Equal(got, want) {
			t.Fatal("migrated bytes changed")
		}
	}
	_ = e.ReleaseContent(ctx, h.LeaseID)
	tags, err := e.ListHistoryTags(ctx)
	if err != nil || len(tags) != 3 {
		t.Fatal("tag definitions lost", err)
	}
}

func TestHistoryMigrationFailureDoesNotPublishOrEvict(t *testing.T) {
	ctx := context.Background()
	dir := t.TempDir()
	e, err := New(ctx, Options{DataDir: dir, MigrationMode: true, Settings: func() Settings { s := DefaultSettings(); s.History.MaxItems = 1; return s }()})
	if err != nil {
		t.Fatal(err)
	}
	defer func() { _ = e.Close(ctx) }()
	input := func(id string, reader io.Reader) HistoryImportRequest {
		return HistoryImportRequest{Source: "s", RecordID: id, CreatedAtMs: 1000,
			Content: ContentInput{Files: []FileInput{{Name: "x", Size: 4, Reader: reader}}}}
	}
	first, err := e.ImportHistory(ctx, input("a", strings.NewReader("1234")))
	if err != nil {
		t.Fatal(err)
	}
	if _, err = e.ImportHistory(ctx, input("bad", strings.NewReader("12"))); err == nil {
		t.Fatal("accepted truncated content")
	}
	if _, err = e.ImportHistory(ctx, input("b", strings.NewReader("1234"))); err != nil {
		t.Fatal(err)
	}
	p, err := e.ListHistory(ctx, ListHistoryRequest{})
	if err != nil || len(p.Items) != 2 {
		t.Fatal("migration evicted records", err)
	}
	if _, err = e.GetItem(ctx, first.HistoryID); err != nil {
		t.Fatal(err)
	}
	if _, known, err := e.LookupHistoryImport(ctx, "s", "bad"); err != nil || known {
		t.Fatal("failure published dedup mapping")
	}
}
