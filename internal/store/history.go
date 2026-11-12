package store

import (
	"database/sql"
	"errors"
	"fmt"
	"strings"
)

// Low-level history persistence (0004 P2). This file knows rows, not rules:
// limits, eviction policy, IDs and blob files belong to the engine. Every
// visible change bumps the persisted history revision in the same transaction.

var (
	ErrNotFound = errors.New("store: no such history item")
	ErrConflict = errors.New("store: history item revision mismatch")
)

// versioned holds schema steps tracked by PRAGMA user_version. No version has
// been released, so there is one step: the current schema. Once a release
// exists, append steps; never edit one that has shipped.
var versioned = []string{
	`CREATE TABLE history_state (id INTEGER PRIMARY KEY CHECK (id = 1), revision INTEGER NOT NULL);
INSERT INTO history_state (id, revision) VALUES (1, 0);
-- seq is the private, never reused commit order (AUTOINCREMENT); id is public.
CREATE TABLE clip_items (
  seq         INTEGER PRIMARY KEY AUTOINCREMENT,
  id          TEXT NOT NULL UNIQUE,
  revision    INTEGER NOT NULL,
  kind        TEXT NOT NULL,
  origin_kind TEXT NOT NULL,
  device_id   TEXT NOT NULL DEFAULT '',
  plugin_id   TEXT NOT NULL DEFAULT '',
  source_app  TEXT NOT NULL DEFAULT '',
  created_ms  INTEGER NOT NULL,
  updated_ms  INTEGER NOT NULL,
  expires_ms  INTEGER NOT NULL DEFAULT 0,
  sync_allowed INTEGER NOT NULL DEFAULT 1,
  sync_state  TEXT NOT NULL DEFAULT 'not_requested',
  total_bytes INTEGER NOT NULL
);
CREATE INDEX clip_items_expires ON clip_items (expires_ms) WHERE expires_ms > 0;
CREATE INDEX clip_items_created ON clip_items (created_ms);
-- One row per part, in order. Small data is inline, the rest a blob name.
CREATE TABLE clip_item_parts (
  item_seq INTEGER NOT NULL REFERENCES clip_items (seq) ON DELETE CASCADE,
  ord      INTEGER NOT NULL,
  kind     TEXT NOT NULL,          -- 'rep' or 'file'
  mime     TEXT NOT NULL DEFAULT '',
  name     TEXT NOT NULL DEFAULT '', -- file name; files only
  size     INTEGER NOT NULL,
  inline   BLOB,
  blob     TEXT NOT NULL DEFAULT '',
  ref      TEXT NOT NULL DEFAULT '',      -- opaque host reference of a file kept in place; nothing is copied
  version  TEXT NOT NULL DEFAULT '',      -- size/version seen at commit
  PRIMARY KEY (item_seq, ord)
);
CREATE INDEX clip_item_parts_blob ON clip_item_parts (blob) WHERE blob <> '';
CREATE INDEX clip_item_parts_ref ON clip_item_parts (ref) WHERE ref <> '';
CREATE TABLE clip_tags (
  item_seq INTEGER NOT NULL REFERENCES clip_items (seq) ON DELETE CASCADE,
  tag      TEXT NOT NULL,
  PRIMARY KEY (item_seq, tag)
) WITHOUT ROWID;
CREATE INDEX clip_tags_tag ON clip_tags (tag);
CREATE TABLE clip_item_sync (
  item_seq   INTEGER NOT NULL REFERENCES clip_items (seq) ON DELETE CASCADE,
  backend_id TEXT NOT NULL,
  state      TEXT NOT NULL,
  disposition TEXT NOT NULL DEFAULT '',   -- the first commit decision
  error_code TEXT NOT NULL DEFAULT '',    -- a stable error class
  PRIMARY KEY (item_seq, backend_id)
) WITHOUT ROWID;
-- Received transfers already committed, so the same transfer arriving through
-- another backend is dropped. Bounded by age and count.
CREATE TABLE sync_dedup (
  source      TEXT NOT NULL,
  transfer_id TEXT NOT NULL,
  view_hash   TEXT NOT NULL,
  created_ms  INTEGER NOT NULL,
  PRIMARY KEY (source, transfer_id, view_hash)
) WITHOUT ROWID;
CREATE INDEX sync_dedup_created ON sync_dedup (created_ms);
` + "\n" + deviceSchema, // unified devices and their per-backend-instance bindings (docs/architecture/identity-and-trust.md)
	candidateSchema, // devices a peer vouched for (see docs/architecture/identity-and-trust.md)
}

func migrate(db *sql.DB) error {
	if _, err := db.Exec(`PRAGMA foreign_keys = ON`); err != nil {
		return err
	}
	var v int
	if err := db.QueryRow(`PRAGMA user_version`).Scan(&v); err != nil {
		return err
	}
	if v > len(versioned) {
		return fmt.Errorf("store: database schema %d is newer than this version supports (%d)", v, len(versioned))
	}
	for ; v < len(versioned); v++ {
		tx, err := db.Begin()
		if err != nil {
			return err
		}
		if _, err := tx.Exec(versioned[v]); err != nil {
			_ = tx.Rollback()
			return err
		}
		if _, err := tx.Exec(fmt.Sprintf(`PRAGMA user_version = %d`, v+1)); err != nil {
			_ = tx.Rollback()
			return err
		}
		if err := tx.Commit(); err != nil {
			return err
		}
	}
	return nil
}

// HistoryPart is one stored part. Inline and Blob are exclusive; reads of
// metadata leave Inline nil.
type HistoryPart struct {
	Kind    string // "rep" or "file"
	MIME    string
	Name    string
	Size    int64
	Inline  []byte
	Blob    string
	Ref     string // opaque host reference of a file kept in place; no blob
	Version string
}

// BackendSync is what is remembered of one backend's part in a commit.
type BackendSync struct {
	State       string
	Disposition string
	ErrorCode   string
}

// HistoryItem is a stored record. Seq is private commit order.
type HistoryItem struct {
	SearchDocs    []SearchDocument
	SearchReady   bool
	SearchBudget  int64
	SyncBlocked   bool
	Seq           int64
	OrderMs       int64
	ID            string
	Revision      uint64
	Kind          string
	OriginKind    string
	DeviceID      string
	PluginID      string
	SourceApp     string
	CreatedMs     int64
	UpdatedMs     int64
	ExpiresMs     int64
	SyncState     string
	TotalBytes    int64
	Tags          []string // sorted by UTF-8 bytes
	Parts         []HistoryPart
	BackendStates map[string]BackendSync
	ImportSource  string // offline import only; not part of public metadata
	ImportRecord  string
}

// HistoryFilter: fields AND together; Kinds/OriginKinds match any of their
// values, Tags must all match. Zero values mean no constraint.
type HistoryFilter struct {
	Kinds       []string
	OriginKinds []string
	Tags        []string
	DeviceID    string
	FromMs      int64
	BeforeMs    int64
}

type HistoryQuery struct {
	Filter    HistoryFilter
	Upper     int64 // only seq <= Upper; 0 = current newest (fixed by the first page)
	After     int64 // only seq < After; 0 = from the top
	AfterTime int64 // stable chronological order paired with After
	Limit     int
	NowMs     int64 // rows with 0 < expires_ms <= NowMs are hidden; 0 disables
}

// HistoryRevision returns the persisted collection revision.
func (s *Store) HistoryRevision() (uint64, error) {
	var r uint64
	err := s.db.QueryRow(`SELECT revision FROM history_state WHERE id = 1`).Scan(&r)
	return r, err
}

func bump(tx *sql.Tx) (uint64, error) {
	var r uint64
	if err := tx.QueryRow(`UPDATE history_state SET revision = revision + 1 WHERE id = 1 RETURNING revision`).Scan(&r); err != nil {
		return 0, err
	}
	return r, nil
}

// InsertHistory stores a new item with revision 1 and returns its seq and the
// new history revision. Tags must already be validated and unique.
func (s *Store) InsertHistory(it HistoryItem) (seq int64, rev uint64, err error) {
	return s.InsertHistoryDedup(it, nil)
}

// InsertHistoryDedup stores a new item and, in the same transaction, the
// dedup record of the transfer it came from.
func (s *Store) InsertHistoryDedup(it HistoryItem, d *Dedup) (seq int64, rev uint64, err error) {
	tx, err := s.db.Begin()
	if err != nil {
		return 0, 0, err
	}
	defer func() { _ = tx.Rollback() }()
	r, err := tx.Exec(`INSERT INTO clip_items (id, revision, kind, origin_kind, device_id, plugin_id, source_app,
created_ms, updated_ms, expires_ms, sync_state, total_bytes, sync_allowed) VALUES (?, 1, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)`,
		it.ID, it.Kind, it.OriginKind, it.DeviceID, it.PluginID, it.SourceApp,
		it.CreatedMs, max(it.CreatedMs, it.UpdatedMs), it.ExpiresMs, nonEmpty(it.SyncState, "not_requested"), it.TotalBytes, !it.SyncBlocked)
	if err != nil {
		return 0, 0, err
	}
	if seq, err = r.LastInsertId(); err != nil {
		return 0, 0, err
	}
	orderMs := it.CreatedMs
	if it.ImportSource == "" {
		// Normal commits remain monotonic even when the wall clock goes back.
		var last int64
		if err := tx.QueryRow(`SELECT COALESCE(MAX(`+orderExpr+`), 0) FROM clip_items WHERE seq <> ?`, seq).Scan(&last); err != nil {
			return 0, 0, err
		}
		orderMs = max(orderMs, last)
	}
	if _, err := tx.Exec(`INSERT INTO clip_item_order(item_seq,order_ms) VALUES(?,?)`, seq, orderMs); err != nil {
		return 0, 0, err
	}
	for i, p := range it.Parts {
		if _, err := tx.Exec(`INSERT INTO clip_item_parts (item_seq, ord, kind, mime, name, size, inline, blob, ref, version) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)`,
			seq, i, p.Kind, p.MIME, p.Name, p.Size, p.Inline, p.Blob, p.Ref, p.Version); err != nil {
			return 0, 0, err
		}
	}
	for _, t := range it.Tags {
		if _, err := tx.Exec(`INSERT INTO clip_tags (item_seq, tag) VALUES (?, ?)`, seq, t); err != nil {
			return 0, 0, err
		}
		if _, err := tx.Exec(`INSERT OR IGNORE INTO history_labels(name) VALUES(?)`, t); err != nil {
			return 0, 0, err
		}
	}
	if err := insertSearch(tx, seq, it.SearchDocs, it.SearchReady, it.SearchBudget); err != nil {
		return 0, 0, err
	}

	for b, st := range it.BackendStates {
		if err := insertBackendSync(tx, seq, b, st); err != nil {
			return 0, 0, err
		}
	}
	if d != nil {
		if err := putDedup(tx, *d); err != nil {
			return 0, 0, err
		}
	}
	if it.ImportSource != "" {
		if _, err := tx.Exec(`INSERT INTO history_imports(source,record_id,history_id) VALUES(?,?,?)`,
			it.ImportSource, it.ImportRecord, it.ID); err != nil {
			return 0, 0, err
		}
		// Persistent floors leave room for normal capture after a large import.
		if _, err := tx.Exec(`INSERT INTO history_import_limits(id,max_items,max_bytes,max_stored_bytes)
SELECT 1,COUNT(*)+1000,COALESCE(SUM(total_bytes),0)+1073741824,COALESCE(SUM(total_bytes),0)+8589934592 FROM clip_items WHERE 1
ON CONFLICT(id) DO UPDATE SET max_items=MAX(max_items,excluded.max_items),
 max_bytes=MAX(max_bytes,excluded.max_bytes), max_stored_bytes=MAX(max_stored_bytes,excluded.max_stored_bytes)`); err != nil {
			return 0, 0, err
		}
	}
	if rev, err = bump(tx); err != nil {
		return 0, 0, err
	}
	return seq, rev, tx.Commit()
}

func nonEmpty(s, d string) string {
	if s == "" {
		return d
	}
	return s
}

const orderExpr = `COALESCE((SELECT order_ms FROM clip_item_order WHERE item_seq=clip_items.seq),created_ms)`
const itemCols = `seq, id, revision, kind, origin_kind, device_id, plugin_id, source_app, created_ms, updated_ms, expires_ms, sync_state, total_bytes, ` + orderExpr + `, NOT sync_allowed`

type queryer interface {
	Query(string, ...any) (*sql.Rows, error)
	QueryRow(string, ...any) *sql.Row
}

func scanItem(sc interface{ Scan(...any) error }) (HistoryItem, error) {
	var it HistoryItem
	err := sc.Scan(&it.Seq, &it.ID, &it.Revision, &it.Kind, &it.OriginKind, &it.DeviceID, &it.PluginID, &it.SourceApp,
		&it.CreatedMs, &it.UpdatedMs, &it.ExpiresMs, &it.SyncState, &it.TotalBytes, &it.OrderMs, &it.SyncBlocked)
	return it, err
}

// fill loads tags, parts and backend states. withInline also loads inline data.
func fill(q queryer, it *HistoryItem, withInline bool) error {
	rows, err := q.Query(`SELECT tag FROM clip_tags WHERE item_seq = ? ORDER BY CAST(tag AS BLOB)`, it.Seq)
	if err != nil {
		return err
	}
	for rows.Next() {
		var t string
		if err := rows.Scan(&t); err != nil {
			_ = rows.Close()
			return err
		}
		it.Tags = append(it.Tags, t)
	}
	if err := rows.Close(); err != nil {
		return err
	}
	col := "NULL"
	if withInline {
		col = "inline"
	}
	rows, err = q.Query(`SELECT kind, mime, name, size, `+col+`, blob, ref, version FROM clip_item_parts WHERE item_seq = ? ORDER BY ord`, it.Seq)
	if err != nil {
		return err
	}
	for rows.Next() {
		var p HistoryPart
		if err := rows.Scan(&p.Kind, &p.MIME, &p.Name, &p.Size, &p.Inline, &p.Blob, &p.Ref, &p.Version); err != nil {
			_ = rows.Close()
			return err
		}
		it.Parts = append(it.Parts, p)
	}
	if err := rows.Close(); err != nil {
		return err
	}
	rows, err = q.Query(`SELECT backend_id, state, disposition, error_code FROM clip_item_sync WHERE item_seq = ?`, it.Seq)
	if err != nil {
		return err
	}
	defer func() { _ = rows.Close() }()
	for rows.Next() {
		var b string
		var st BackendSync
		if err := rows.Scan(&b, &st.State, &st.Disposition, &st.ErrorCode); err != nil {
			return err
		}
		if it.BackendStates == nil {
			it.BackendStates = map[string]BackendSync{}
		}
		it.BackendStates[b] = st
	}
	return rows.Err()
}

// GetHistory returns one item. Expired items (0 < expires <= nowMs) are not
// found when nowMs > 0. withInline loads inline part data.
func (s *Store) GetHistory(id string, nowMs int64, withInline bool) (HistoryItem, error) {
	it, err := scanItem(s.db.QueryRow(`SELECT `+itemCols+` FROM clip_items WHERE id = ?`, id))
	if errors.Is(err, sql.ErrNoRows) || (err == nil && nowMs > 0 && it.ExpiresMs > 0 && it.ExpiresMs <= nowMs) {
		return HistoryItem{}, ErrNotFound
	}
	if err != nil {
		return HistoryItem{}, err
	}
	return it, fill(s.db, &it, withInline)
}

// ListHistory returns one page, newest commit first, with the revision and the
// newest seq at read time, all from one read transaction.
func (s *Store) ListHistory(q HistoryQuery) (items []HistoryItem, rev uint64, newest int64, err error) {
	tx, err := s.db.Begin()
	if err != nil {
		return nil, 0, 0, err
	}
	defer func() { _ = tx.Rollback() }()
	if err := tx.QueryRow(`SELECT revision FROM history_state WHERE id = 1`).Scan(&rev); err != nil {
		return nil, 0, 0, err
	}
	if err := tx.QueryRow(`SELECT COALESCE(MAX(seq), 0) FROM clip_items`).Scan(&newest); err != nil {
		return nil, 0, 0, err
	}
	if q.After > 0 && q.AfterTime == 0 {
		_ = tx.QueryRow(`SELECT `+orderExpr+` FROM clip_items WHERE seq=?`, q.After).Scan(&q.AfterTime)
	}
	where, args := historyPredicate(q)

	sqlText := `SELECT ` + itemCols + ` FROM clip_items`
	if len(where) > 0 {
		sqlText += ` WHERE ` + strings.Join(where, " AND ")
	}
	sqlText += ` ORDER BY ` + orderExpr + ` DESC, seq DESC LIMIT ?`
	args = append(args, q.Limit)
	rows, err := tx.Query(sqlText, args...)
	if err != nil {
		return nil, 0, 0, err
	}
	for rows.Next() {
		it, err := scanItem(rows)
		if err != nil {
			_ = rows.Close()
			return nil, 0, 0, err
		}
		items = append(items, it)
	}
	if err := rows.Close(); err != nil {
		return nil, 0, 0, err
	}
	for i := range items {
		if err := fill(tx, &items[i], false); err != nil {
			return nil, 0, 0, err
		}
	}
	return items, rev, newest, nil
}

func anys(vs []string) []any {
	out := make([]any, len(vs))
	for i, v := range vs {
		out[i] = v
	}
	return out
}

// UpdateHistory replaces tags and/or sets the expiry (0 clears) when the
// revision matches. If nothing changes it is a no-op: no revision bump.
func (s *Store) UpdateHistory(id string, expected uint64, tags *[]string, expiresMs *int64, nowMs int64) (it HistoryItem, changed bool, rev uint64, err error) {
	tx, err := s.db.Begin()
	if err != nil {
		return it, false, 0, err
	}
	defer func() { _ = tx.Rollback() }()
	it, err = scanItem(tx.QueryRow(`SELECT `+itemCols+` FROM clip_items WHERE id = ?`, id))
	if errors.Is(err, sql.ErrNoRows) || (err == nil && it.ExpiresMs > 0 && it.ExpiresMs <= nowMs) {
		return it, false, 0, ErrNotFound
	}
	if err != nil {
		return it, false, 0, err
	}
	if it.Revision != expected {
		return it, false, 0, ErrConflict
	}
	if err := fill(tx, &it, false); err != nil {
		return it, false, 0, err
	}
	if tags != nil && !sameStrings(it.Tags, *tags) {
		if _, err := tx.Exec(`DELETE FROM clip_tags WHERE item_seq = ?`, it.Seq); err != nil {
			return it, false, 0, err
		}
		for _, t := range *tags {
			if _, err := tx.Exec(`INSERT INTO clip_tags (item_seq, tag) VALUES (?, ?)`, it.Seq, t); err != nil {
				return it, false, 0, err
			}
		}
		it.Tags, changed = append([]string(nil), *tags...), true
	}
	if expiresMs != nil && *expiresMs != it.ExpiresMs {
		it.ExpiresMs, changed = *expiresMs, true
	}
	if !changed {
		err = tx.QueryRow(`SELECT revision FROM history_state WHERE id = 1`).Scan(&rev)
		return it, false, rev, err
	}
	it.Revision++
	it.UpdatedMs = nowMs
	if _, err := tx.Exec(`UPDATE clip_items SET revision = ?, updated_ms = ?, expires_ms = ? WHERE seq = ?`,
		it.Revision, it.UpdatedMs, it.ExpiresMs, it.Seq); err != nil {
		return it, false, 0, err
	}
	if rev, err = bump(tx); err != nil {
		return it, false, 0, err
	}
	return it, true, rev, tx.Commit()
}

func sameStrings(a, b []string) bool {
	if len(a) != len(b) {
		return false
	}
	for i := range a {
		if a[i] != b[i] {
			return false
		}
	}
	return true
}

// SetSyncState records the aggregate and per-backend sync summary.
func (s *Store) SetSyncState(id, state string, backends map[string]BackendSync, nowMs int64) (rev uint64, err error) {
	tx, err := s.db.Begin()
	if err != nil {
		return 0, err
	}
	defer func() { _ = tx.Rollback() }()
	var seq int64
	if err := tx.QueryRow(`SELECT seq FROM clip_items WHERE id = ?`, id).Scan(&seq); errors.Is(err, sql.ErrNoRows) {
		return 0, ErrNotFound
	} else if err != nil {
		return 0, err
	}
	if _, err := tx.Exec(`DELETE FROM clip_item_sync WHERE item_seq = ?`, seq); err != nil {
		return 0, err
	}
	for b, st := range backends {
		if err := insertBackendSync(tx, seq, b, st); err != nil {
			return 0, err
		}
	}
	if _, err := tx.Exec(`UPDATE clip_items SET sync_state = ?, revision = revision + 1, updated_ms = ? WHERE seq = ?`, state, nowMs, seq); err != nil {
		return 0, err
	}
	if rev, err = bump(tx); err != nil {
		return 0, err
	}
	return rev, tx.Commit()
}

// DeleteHistory removes one item (expected 0 skips the revision check, for
// expiry and eviction) and returns the blobs it referenced.
func (s *Store) DeleteHistory(id string, expected uint64) (blobs []string, rev uint64, err error) {
	tx, err := s.db.Begin()
	if err != nil {
		return nil, 0, err
	}
	defer func() { _ = tx.Rollback() }()
	var seq int64
	var cur uint64
	if err := tx.QueryRow(`SELECT seq, revision FROM clip_items WHERE id = ?`, id).Scan(&seq, &cur); errors.Is(err, sql.ErrNoRows) {
		return nil, 0, ErrNotFound
	} else if err != nil {
		return nil, 0, err
	}
	if expected != 0 && cur != expected {
		return nil, 0, ErrConflict
	}
	if blobs, err = deleteSeqs(tx, []int64{seq}); err != nil {
		return nil, 0, err
	}
	if rev, err = bump(tx); err != nil {
		return nil, 0, err
	}
	return blobs, rev, tx.Commit()
}

// ExpiredHistory deletes every item with 0 < expires <= nowMs. It returns the
// IDs removed and the blobs they referenced (which may still be shared).
func (s *Store) ExpiredHistory(nowMs int64) (ids, blobs []string, rev uint64, err error) {
	tx, err := s.db.Begin()
	if err != nil {
		return nil, nil, 0, err
	}
	defer func() { _ = tx.Rollback() }()
	rows, err := tx.Query(`SELECT seq, id FROM clip_items WHERE expires_ms > 0 AND expires_ms <= ?`, nowMs)
	if err != nil {
		return nil, nil, 0, err
	}
	var seqs []int64
	for rows.Next() {
		var q int64
		var id string
		if err := rows.Scan(&q, &id); err != nil {
			_ = rows.Close()
			return nil, nil, 0, err
		}
		seqs, ids = append(seqs, q), append(ids, id)
	}
	if err := rows.Close(); err != nil {
		return nil, nil, 0, err
	}
	if len(seqs) == 0 {
		err = tx.QueryRow(`SELECT revision FROM history_state WHERE id = 1`).Scan(&rev)
		return nil, nil, rev, err
	}
	if blobs, err = deleteSeqs(tx, seqs); err != nil {
		return nil, nil, 0, err
	}
	if rev, err = bump(tx); err != nil {
		return nil, nil, 0, err
	}
	return ids, blobs, rev, tx.Commit()
}

func deleteSeqs(tx *sql.Tx, seqs []int64) (blobs []string, err error) {
	for _, q := range seqs {
		rows, err := tx.Query(`SELECT blob FROM clip_item_parts WHERE item_seq = ? AND blob <> ''`, q)
		if err != nil {
			return nil, err
		}
		for rows.Next() {
			var b string
			if err := rows.Scan(&b); err != nil {
				_ = rows.Close()
				return nil, err
			}
			blobs = append(blobs, b)
		}
		if err := rows.Close(); err != nil {
			return nil, err
		}
		if _, err := tx.Exec(`DELETE FROM clip_items WHERE seq = ?`, q); err != nil { // cascades
			return nil, err
		}
	}
	return blobs, nil
}

// HistoryTotals returns the item count and the bytes history itself stores
// (inline data and blobs). Files kept in place by reference cost nothing.
func (s *Store) HistoryTotals() (items int, bytes int64, err error) {
	if err = s.db.QueryRow(`SELECT COUNT(*) FROM clip_items`).Scan(&items); err != nil {
		return
	}
	err = s.db.QueryRow(`SELECT COALESCE(SUM(size), 0) FROM clip_item_parts WHERE kind <> 'file' OR ref = ''`).Scan(&bytes)
	return
}

// BlobInUse reports whether any item still references the blob.
func (s *Store) BlobInUse(name string) (bool, error) {
	var n int
	err := s.db.QueryRow(`SELECT COUNT(*) FROM clip_item_parts WHERE blob = ?`, name).Scan(&n)
	return n > 0, err
}

// ReferencedBlobs lists every blob name referenced, for orphan cleanup.
func (s *Store) ReferencedBlobs() (map[string]bool, error) {
	rows, err := s.db.Query(`SELECT DISTINCT blob FROM clip_item_parts WHERE blob <> ''`)
	if err != nil {
		return nil, err
	}
	defer func() { _ = rows.Close() }()
	out := map[string]bool{}
	for rows.Next() {
		var b string
		if err := rows.Scan(&b); err != nil {
			return nil, err
		}
		out[b] = true
	}
	return out, rows.Err()
}

func insertBackendSync(tx *sql.Tx, seq int64, backend string, st BackendSync) error {
	_, err := tx.Exec(`INSERT INTO clip_item_sync (item_seq, backend_id, state, disposition, error_code) VALUES (?, ?, ?, ?, ?)`,
		seq, backend, st.State, st.Disposition, st.ErrorCode)
	return err
}

// InterruptPending marks every sync still pending as interrupted. Called once
// at startup: nothing resumes or resends an unfinished transfer, so the
// summary must stop claiming it is in progress. It returns the IDs changed.
func (s *Store) InterruptPending(nowMs int64) (ids []string, err error) {
	tx, err := s.db.Begin()
	if err != nil {
		return nil, err
	}
	defer func() { _ = tx.Rollback() }()
	rows, err := tx.Query(`SELECT id FROM clip_items WHERE sync_state = 'pending'`)
	if err != nil {
		return nil, err
	}
	for rows.Next() {
		var id string
		if err := rows.Scan(&id); err != nil {
			_ = rows.Close()
			return nil, err
		}
		ids = append(ids, id)
	}
	if err := rows.Close(); err != nil {
		return nil, err
	}
	if len(ids) == 0 {
		return nil, nil
	}
	if _, err := tx.Exec(`UPDATE clip_item_sync SET state = 'interrupted' WHERE state = 'pending'`); err != nil {
		return nil, err
	}
	if _, err := tx.Exec(`UPDATE clip_items SET sync_state = 'interrupted', revision = revision + 1, updated_ms = ? WHERE sync_state = 'pending'`, nowMs); err != nil {
		return nil, err
	}
	if _, err := bump(tx); err != nil {
		return nil, err
	}
	return ids, tx.Commit()
}

// DedupKey names one received view of one transfer. Source must be the
// authenticated sender, never a value the peer merely claims.
type DedupKey struct{ Source, Transfer, View string }

// Dedup is a success record plus the bounds that apply when it is written.
type Dedup struct {
	Key   DedupKey
	NowMs int64
	TTLMs int64
	Max   int
}

func putDedup(tx *sql.Tx, d Dedup) error {
	if _, err := tx.Exec(`INSERT OR IGNORE INTO sync_dedup (source, transfer_id, view_hash, created_ms) VALUES (?, ?, ?, ?)`,
		d.Key.Source, d.Key.Transfer, d.Key.View, d.NowMs); err != nil {
		return err
	}
	if _, err := tx.Exec(`DELETE FROM sync_dedup WHERE created_ms <= ?`, d.NowMs-d.TTLMs); err != nil {
		return err
	}
	// Under capacity pressure the oldest records go first.
	_, err := tx.Exec(`DELETE FROM sync_dedup WHERE (source, transfer_id, view_hash) IN (
SELECT source, transfer_id, view_hash FROM sync_dedup ORDER BY created_ms DESC, source, transfer_id, view_hash LIMIT -1 OFFSET ?)`, d.Max)
	return err
}

// PutDedup records a successful receive that left no history record.
func (s *Store) PutDedup(d Dedup) error {
	tx, err := s.db.Begin()
	if err != nil {
		return err
	}
	defer func() { _ = tx.Rollback() }()
	if err := putDedup(tx, d); err != nil {
		return err
	}
	return tx.Commit()
}

// HasDedup reports whether a committed receive of this view is still remembered.
func (s *Store) HasDedup(k DedupKey, nowMs, ttlMs int64) (bool, error) {
	var n int
	err := s.db.QueryRow(`SELECT COUNT(*) FROM sync_dedup WHERE source = ? AND transfer_id = ? AND view_hash = ? AND created_ms > ?`,
		k.Source, k.Transfer, k.View, nowMs-ttlMs).Scan(&n)
	return n > 0, err
}

// DeleteDedup forgets a record (the receive it vouched for was rolled back).
func (s *Store) DeleteDedup(k DedupKey) error {
	_, err := s.db.Exec(`DELETE FROM sync_dedup WHERE source = ? AND transfer_id = ? AND view_hash = ?`, k.Source, k.Transfer, k.View)
	return err
}

// HistoryCleanupCandidates returns a bounded oldest-first batch. The Engine
// serializes policy and tag mutations with selection/deletion under hist.mu.
func (s *Store) HistoryCleanupCandidates(excluded []string, keep int64, limit int, storage string) ([]HistoryItem, error) {
	where, args := cleanupWhere(excluded)
	where += " AND seq <> ?"
	args = append(args, keep)
	switch storage {
	case "owned":
		where += " AND EXISTS (SELECT 1 FROM clip_item_parts p WHERE p.item_seq=clip_items.seq AND (p.kind<>'file' OR p.ref=''))"
	case "cache":
		where += " AND EXISTS (SELECT 1 FROM clip_item_parts p WHERE p.item_seq=clip_items.seq AND p.ref LIKE 'cache:%')"
	}
	args = append(args, limit)
	rows, err := s.db.Query(`SELECT `+itemCols+` FROM clip_items WHERE `+where+` ORDER BY created_ms,seq LIMIT ?`, args...)
	if err != nil {
		return nil, err
	}
	defer func() { _ = rows.Close() }()
	var out []HistoryItem
	for rows.Next() {
		it, err := scanItem(rows)
		if err != nil {
			return nil, err
		}
		out = append(out, it)
	}
	return out, rows.Err()
}

func cleanupWhere(excluded []string) (string, []any) {
	if len(excluded) == 0 {
		return "1=1", nil
	}
	marks := strings.TrimSuffix(strings.Repeat("?,", len(excluded)), ",")
	args := make([]any, len(excluded))
	for i, t := range excluded {
		args[i] = t
	}
	return `NOT EXISTS (SELECT 1 FROM clip_tags t WHERE t.item_seq=clip_items.seq AND t.tag IN (` + marks + `))`, args
}

func (s *Store) CleanupHistoryCount(excluded []string) (int, error) {
	where, args := cleanupWhere(excluded)
	var n int
	err := s.db.QueryRow(`SELECT COUNT(*) FROM clip_items WHERE `+where, args...).Scan(&n)
	return n, err
}

func (s *Store) CachedFileReferenced(id string) (bool, error) {
	var n int
	err := s.db.QueryRow(`SELECT COUNT(*) FROM clip_item_parts WHERE ref=?`, "cache:"+id).Scan(&n)
	return n > 0, err
}

// CachedFileProtected reports whether any history reference excludes this
// payload from automatic reclamation. The caller holds the history mutex.
func (s *Store) CachedFileProtected(id string, excluded []string) (bool, error) {
	if len(excluded) == 0 {
		return false, nil
	}
	marks := strings.TrimSuffix(strings.Repeat("?,", len(excluded)), ",")
	args := []any{"cache:" + id}
	for _, tag := range excluded {
		args = append(args, tag)
	}
	var protected bool
	err := s.db.QueryRow(`SELECT EXISTS (SELECT 1 FROM clip_item_parts p JOIN clip_tags t ON t.item_seq=p.item_seq WHERE p.ref=? AND t.tag IN (`+marks+`))`, args...).Scan(&protected)
	return protected, err
}
