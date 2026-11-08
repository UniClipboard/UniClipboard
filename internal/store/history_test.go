package store

import (
	"errors"
	"path/filepath"
	"testing"
)

func openTemp(t *testing.T) (*Store, string) {
	t.Helper()
	path := filepath.Join(t.TempDir(), "db")
	s, err := Open(path)
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = s.Close() })
	return s, path
}

func item(id string, created int64, tags ...string) HistoryItem {
	return HistoryItem{ID: id, Kind: "text", OriginKind: "local_system", CreatedMs: created, TotalBytes: 5, Tags: tags,
		Parts: []HistoryPart{{Kind: "rep", MIME: "text/plain", Size: 5, Inline: []byte("hello")}}}
}

func TestHistoryRoundTripAndPersistence(t *testing.T) {
	s, path := openTemp(t)
	it := item("a", 100, "x", "y")
	it.Parts = append(it.Parts, HistoryPart{Kind: "file", Name: "f.bin", Size: 9, Blob: "b1"})
	it.BackendStates = map[string]BackendSync{"builtin.tailcat": {State: "pending", Disposition: "queued"}}
	seq, rev, err := s.InsertHistory(it)
	if err != nil || seq == 0 || rev != 1 {
		t.Fatal(seq, rev, err)
	}
	got, err := s.GetHistory("a", 0, true)
	if err != nil || got.Revision != 1 || string(got.Parts[0].Inline) != "hello" || got.Parts[1].Blob != "b1" || len(got.Tags) != 2 || got.BackendStates["builtin.tailcat"].Disposition != "queued" {
		t.Fatalf("%+v %v", got, err)
	}
	if meta, _ := s.GetHistory("a", 0, false); meta.Parts[0].Inline != nil {
		t.Fatal("metadata read returned inline data")
	}
	_ = s.Close()
	s2, err := Open(path)
	if err != nil {
		t.Fatal(err)
	}
	defer func() { _ = s2.Close() }()
	if r, _ := s2.HistoryRevision(); r != 1 {
		t.Fatalf("revision %d after reopen", r)
	}
	if _, err := s2.GetHistory("a", 0, false); err != nil {
		t.Fatal(err)
	}
	if _, err := s2.GetHistory("missing", 0, false); !errors.Is(err, ErrNotFound) {
		t.Fatal(err)
	}
}

func TestHistoryListKeysetFilterAndNoSeqReuse(t *testing.T) {
	s, _ := openTemp(t)
	for i, id := range []string{"a", "b", "c", "d"} {
		it := item(id, 100) // same millisecond for all
		if i%2 == 0 {
			it.Tags = []string{"even"}
		}
		if _, _, err := s.InsertHistory(it); err != nil {
			t.Fatal(err)
		}
	}
	page, _, newest, err := s.ListHistory(HistoryQuery{Limit: 2})
	if err != nil || len(page) != 2 || page[0].ID != "d" || page[1].ID != "c" {
		t.Fatalf("%v %v", page, err)
	}
	if _, _, err := s.InsertHistory(item("e", 100)); err != nil { // after the pinned bound
		t.Fatal(err)
	}
	next, _, _, _ := s.ListHistory(HistoryQuery{Limit: 10, Upper: newest, After: page[1].Seq})
	if len(next) != 2 || next[0].ID != "b" || next[1].ID != "a" {
		t.Fatalf("%v", next)
	}
	even, _, _, _ := s.ListHistory(HistoryQuery{Limit: 10, Filter: HistoryFilter{Tags: []string{"even"}}})
	if len(even) != 2 || even[0].ID != "c" {
		t.Fatalf("%v", even)
	}
	e, _ := s.GetHistory("e", 0, false)
	if _, _, err := s.DeleteHistory("e", 0); err != nil {
		t.Fatal(err)
	}
	seq, _, _ := s.InsertHistory(item("f", 100))
	if seq <= e.Seq {
		t.Fatalf("seq reused: %d <= %d", seq, e.Seq)
	}
}

func TestHistoryUpdateDeleteExpiry(t *testing.T) {
	s, _ := openTemp(t)
	a := item("a", 100)
	a.Parts = []HistoryPart{{Kind: "file", Name: "n", Size: 5, Blob: "shared"}}
	b := item("b", 101)
	b.Parts = []HistoryPart{{Kind: "file", Name: "n", Size: 5, Blob: "shared"}}
	_, _, _ = s.InsertHistory(a)
	_, _, _ = s.InsertHistory(b)

	tags := []string{"t"}
	if _, _, _, err := s.UpdateHistory("a", 7, &tags, nil, 200); !errors.Is(err, ErrConflict) {
		t.Fatal(err)
	}
	got, changed, rev, err := s.UpdateHistory("a", 1, &tags, nil, 200)
	if err != nil || !changed || got.Revision != 2 || rev != 3 {
		t.Fatal(got, changed, rev, err)
	}
	if _, changed, rev2, err := s.UpdateHistory("a", 2, &tags, nil, 300); err != nil || changed || rev2 != rev {
		t.Fatal("same values must be a silent no-op", changed, rev2, err)
	}
	exp := int64(500)
	if _, _, _, err := s.UpdateHistory("a", 2, nil, &exp, 300); err != nil {
		t.Fatal(err)
	}
	if _, err := s.GetHistory("a", 499, false); err != nil {
		t.Fatal(err)
	}
	if _, err := s.GetHistory("a", 500, false); !errors.Is(err, ErrNotFound) {
		t.Fatal("expired item still readable")
	}
	ids, blobs, _, err := s.ExpiredHistory(500)
	if err != nil || len(ids) != 1 || ids[0] != "a" || len(blobs) != 1 {
		t.Fatal(ids, blobs, err)
	}
	if in, _ := s.BlobInUse("shared"); !in {
		t.Fatal("shared blob is still referenced by b")
	}
	if _, _, err := s.DeleteHistory("b", 9); !errors.Is(err, ErrConflict) {
		t.Fatal(err)
	}
	if _, _, err := s.DeleteHistory("b", 1); err != nil {
		t.Fatal(err)
	}
	if in, _ := s.BlobInUse("shared"); in {
		t.Fatal("blob still referenced after both items are gone")
	}
	if n, by, _ := s.HistoryTotals(); n != 0 || by != 0 {
		t.Fatal(n, by)
	}
}

func TestSchemaVersionIsTrackedAndNewerRefused(t *testing.T) {
	s, path := openTemp(t)
	var v int
	if err := s.db.QueryRow(`PRAGMA user_version`).Scan(&v); err != nil || v != len(versioned) {
		t.Fatal(v, err)
	}
	if _, err := s.db.Exec(`PRAGMA user_version = 99`); err != nil {
		t.Fatal(err)
	}
	_ = s.Close()
	if _, err := Open(path); err == nil {
		t.Fatal("a database from a newer version must be refused")
	}
}

func TestSyncSummaryAndRestartInterruption(t *testing.T) {
	s, path := openTemp(t)
	_, _, _ = s.InsertHistory(item("a", 100))
	_, _, _ = s.InsertHistory(item("b", 101))
	if _, err := s.SetSyncState("a", "pending", map[string]BackendSync{"x": {State: "pending", Disposition: "queued"}, "y": {State: "failed", Disposition: "failed", ErrorCode: "unavailable"}}, 200); err != nil {
		t.Fatal(err)
	}
	if _, err := s.SetSyncState("b", "sent", map[string]BackendSync{"x": {State: "sent", Disposition: "queued"}}, 200); err != nil {
		t.Fatal(err)
	}
	if _, err := s.SetSyncState("nope", "sent", nil, 200); !errors.Is(err, ErrNotFound) {
		t.Fatal(err)
	}
	_ = s.Close()
	s, err := Open(path)
	if err != nil {
		t.Fatal(err)
	}
	defer func() { _ = s.Close() }()
	ids, err := s.InterruptPending(300)
	if err != nil || len(ids) != 1 || ids[0] != "a" {
		t.Fatal(ids, err)
	}
	a, _ := s.GetHistory("a", 0, false)
	if a.SyncState != "interrupted" || a.Revision != 3 || a.BackendStates["x"].State != "interrupted" || a.BackendStates["x"].Disposition != "queued" ||
		a.BackendStates["y"].State != "failed" || a.BackendStates["y"].ErrorCode != "unavailable" {
		t.Fatalf("%+v", a)
	}
	if b, _ := s.GetHistory("b", 0, false); b.SyncState != "sent" || b.Revision != 2 {
		t.Fatalf("a finished sync must be left alone: %+v", b)
	}
	if ids, _ := s.InterruptPending(400); len(ids) != 0 {
		t.Fatal("second call changed things again")
	}
}

func TestDedupRecordsAreBoundedByAgeAndCount(t *testing.T) {
	s, _ := openTemp(t)
	k := func(n string) DedupKey { return DedupKey{Source: "dev", Transfer: n, View: "h"} }
	put := func(n string, now int64) {
		t.Helper()
		if err := s.PutDedup(Dedup{Key: k(n), NowMs: now, TTLMs: 1000, Max: 3}); err != nil {
			t.Fatal(err)
		}
	}
	has := func(n string, now int64) bool {
		ok, err := s.HasDedup(k(n), now, 1000)
		if err != nil {
			t.Fatal(err)
		}
		return ok
	}
	put("a", 100)
	put("a", 150) // writing again keeps the first record
	put("b", 200)
	put("c", 300)
	put("d", 400) // over capacity: the oldest goes
	if has("a", 400) || !has("b", 400) || !has("d", 400) {
		t.Fatal("capacity eviction wrong")
	}
	// A record older than the TTL no longer counts, even before it is pruned.
	if has("b", 1250) {
		t.Fatal("expired record still counted")
	}
	put("e", 5000) // pruning removes what has expired
	if n := count(t, s); n != 1 {
		t.Fatalf("%d records after pruning", n)
	}
	// Written in the same transaction as a history item.
	it := item("x", 1)
	if _, _, err := s.InsertHistoryDedup(it, &Dedup{Key: k("with-item"), NowMs: 5100, TTLMs: 1000, Max: 3}); err != nil {
		t.Fatal(err)
	}
	if !has("with-item", 5100) {
		t.Fatal("dedup record missing after commit")
	}
	if _, _, err := s.InsertHistoryDedup(it, &Dedup{Key: k("rolled-back"), NowMs: 5100, TTLMs: 1000, Max: 3}); err == nil {
		t.Fatal("duplicate item ID must fail")
	}
	if has("rolled-back", 5100) {
		t.Fatal("a failed insert left its dedup record")
	}
	if err := s.DeleteDedup(k("with-item")); err != nil || has("with-item", 5100) {
		t.Fatal("DeleteDedup")
	}
}

func count(t *testing.T, s *Store) int {
	t.Helper()
	var n int
	if err := s.db.QueryRow(`SELECT COUNT(*) FROM sync_dedup`).Scan(&n); err != nil {
		t.Fatal(err)
	}
	return n
}
