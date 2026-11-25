package store

import (
	"database/sql"
	"errors"
	"slices"
)

const annotationSchema = `
CREATE TABLE IF NOT EXISTS history_enrichments (
  item_seq       INTEGER NOT NULL REFERENCES clip_items (seq) ON DELETE CASCADE,
  plugin_id      TEXT NOT NULL,
  plugin_version TEXT NOT NULL,
  status         TEXT NOT NULL, -- 'ok' | 'skipped' | 'failed'
  updated_ms     INTEGER NOT NULL,
  PRIMARY KEY (item_seq, plugin_id)
) WITHOUT ROWID;
CREATE TABLE IF NOT EXISTS history_annotations (
  id             INTEGER PRIMARY KEY AUTOINCREMENT,
  item_seq       INTEGER NOT NULL REFERENCES clip_items (seq) ON DELETE CASCADE,
  plugin_id      TEXT NOT NULL,
  ann_type       TEXT NOT NULL,
  plugin_version TEXT NOT NULL,
  payload_json   BLOB NOT NULL,
  byte_size      INTEGER NOT NULL,
  updated_ms     INTEGER NOT NULL,
  UNIQUE (item_seq, plugin_id, ann_type)
);
CREATE INDEX IF NOT EXISTS history_annotations_item ON history_annotations (item_seq);
CREATE TABLE IF NOT EXISTS history_annotation_assets (
  annotation_id INTEGER NOT NULL REFERENCES history_annotations (id) ON DELETE CASCADE,
  asset_key     TEXT NOT NULL,
  mime          TEXT NOT NULL,
  blob          TEXT NOT NULL,
  byte_size     INTEGER NOT NULL,
  PRIMARY KEY (annotation_id, asset_key)
) WITHOUT ROWID;
CREATE INDEX IF NOT EXISTS history_annotation_assets_blob ON history_annotation_assets (blob) WHERE blob <> '';
`

type EnrichmentRecord struct {
	PluginID      string
	PluginVersion string
	Status        string
	UpdatedMs     int64
}

type AnnotationAsset struct {
	Key      string
	MIME     string
	Blob     string
	ByteSize int64
}

type Annotation struct {
	PluginID      string
	Type          string
	PluginVersion string
	PayloadJSON   []byte
	Assets        []AnnotationAsset
	UpdatedMs     int64
}

type SaveEnrichmentInput struct {
	ItemID        string
	PluginID      string
	PluginVersion string
	Status        string
	AddTags       []string
	Annotations   []Annotation
	SearchDocs    []SearchDocument
	SearchBudget  int64
	MaxTags       int
	NowMs         int64
}

// EnrichmentStatus returns the last recorded status for (itemID, pluginID).
func (s *Store) EnrichmentStatus(itemID, pluginID string) (EnrichmentRecord, bool, error) {
	var rec EnrichmentRecord
	err := s.db.QueryRow(`
SELECT e.plugin_id, e.plugin_version, e.status, e.updated_ms
FROM history_enrichments e
JOIN clip_items i ON i.seq = e.item_seq
WHERE i.id = ? AND e.plugin_id = ?`, itemID, pluginID).Scan(&rec.PluginID, &rec.PluginVersion, &rec.Status, &rec.UpdatedMs)
	if errors.Is(err, sql.ErrNoRows) {
		return EnrichmentRecord{}, false, nil
	}
	return rec, err == nil, err
}

// SaveEnrichment atomically persists enrichment status, optional added tags,
// versioned annotations/assets and source-isolated derived search documents.
func (s *Store) SaveEnrichment(in SaveEnrichmentInput) (rev uint64, itemRev uint64, oldBlobs []string, changed bool, err error) {
	tx, err := s.db.Begin()
	if err != nil {
		return 0, 0, nil, false, err
	}
	defer func() { _ = tx.Rollback() }()

	var seq, expires int64
	err = tx.QueryRow(`SELECT seq, revision, expires_ms FROM clip_items WHERE id = ?`, in.ItemID).Scan(&seq, &itemRev, &expires)
	if errors.Is(err, sql.ErrNoRows) || (err == nil && expires > 0 && in.NowMs >= expires) {
		return 0, 0, nil, false, ErrNotFound
	}
	if err != nil {
		return 0, 0, nil, false, err
	}

	if _, err := tx.Exec(`
INSERT INTO history_enrichments (item_seq, plugin_id, plugin_version, status, updated_ms)
VALUES (?, ?, ?, ?, ?)
ON CONFLICT (item_seq, plugin_id) DO UPDATE SET
  plugin_version = excluded.plugin_version,
  status = excluded.status,
  updated_ms = excluded.updated_ms`,
		seq, in.PluginID, in.PluginVersion, in.Status, in.NowMs); err != nil {
		return 0, 0, nil, false, err
	}

	if in.Status != "ok" {
		return 0, itemRev, nil, false, tx.Commit()
	}

	// Collect any previously stored blobs for this (item, plugin) before replacing.
	rows, err := tx.Query(`
SELECT a.blob
FROM history_annotation_assets a
JOIN history_annotations h ON h.id = a.annotation_id
WHERE h.item_seq = ? AND h.plugin_id = ? AND a.blob <> ''`, seq, in.PluginID)
	if err != nil {
		return 0, 0, nil, false, err
	}
	for rows.Next() {
		var b string
		if err := rows.Scan(&b); err != nil {
			_ = rows.Close()
			return 0, 0, nil, false, err
		}
		oldBlobs = append(oldBlobs, b)
	}
	if err := rows.Close(); err != nil {
		return 0, 0, nil, false, err
	}

	resDel, err := tx.Exec(`DELETE FROM history_annotations WHERE item_seq = ? AND plugin_id = ?`, seq, in.PluginID)
	if err != nil {
		return 0, 0, nil, false, err
	}
	if n, _ := resDel.RowsAffected(); n > 0 {
		changed = true
	}

	for _, ann := range in.Annotations {
		resIns, err := tx.Exec(`
INSERT INTO history_annotations (item_seq, plugin_id, ann_type, plugin_version, payload_json, byte_size, updated_ms)
VALUES (?, ?, ?, ?, ?, ?, ?)`,
			seq, in.PluginID, ann.Type, in.PluginVersion, ann.PayloadJSON, int64(len(ann.PayloadJSON)), in.NowMs)
		if err != nil {
			return 0, 0, nil, false, err
		}
		annID, err := resIns.LastInsertId()
		if err != nil {
			return 0, 0, nil, false, err
		}
		for _, asset := range ann.Assets {
			if _, err := tx.Exec(`
INSERT INTO history_annotation_assets (annotation_id, asset_key, mime, blob, byte_size)
VALUES (?, ?, ?, ?, ?)`, annID, asset.Key, asset.MIME, asset.Blob, asset.ByteSize); err != nil {
				return 0, 0, nil, false, err
			}
		}
		changed = true
	}

	if len(in.AddTags) > 0 {
		existing, err := loadTags(tx, seq)
		if err != nil {
			return 0, 0, nil, false, err
		}
		maxTags := in.MaxTags
		if maxTags <= 0 {
			maxTags = 1000
		}
		for _, t := range in.AddTags {
			if slices.Contains(existing, t) {
				continue
			}
			if len(existing) >= maxTags {
				break
			}
			if _, err := tx.Exec(`INSERT OR IGNORE INTO clip_tags (item_seq, tag) VALUES (?, ?)`, seq, t); err != nil {
				return 0, 0, nil, false, err
			}
			existing = append(existing, t)
			changed = true
		}
	}

	searchChanged, err := replaceEnrichmentSearch(tx, seq, in.PluginID, in.SearchDocs, in.SearchBudget)
	if err != nil {
		return 0, 0, nil, false, err
	}
	if searchChanged {
		changed = true
	}

	if changed {
		itemRev++
		if _, err := tx.Exec(`UPDATE clip_items SET revision = ?, updated_ms = ? WHERE seq = ?`, itemRev, in.NowMs, seq); err != nil {
			return 0, 0, nil, false, err
		}
		if rev, err = bump(tx); err != nil {
			return 0, 0, nil, false, err
		}
	}
	return rev, itemRev, oldBlobs, changed, tx.Commit()
}

func loadTags(tx *sql.Tx, seq int64) ([]string, error) {
	rows, err := tx.Query(`SELECT tag FROM clip_tags WHERE item_seq = ?`, seq)
	if err != nil {
		return nil, err
	}
	defer func() { _ = rows.Close() }()
	var tags []string
	for rows.Next() {
		var t string
		if err := rows.Scan(&t); err != nil {
			return nil, err
		}
		tags = append(tags, t)
	}
	return tags, rows.Err()
}

// ListAnnotations returns all persisted annotations for an existing, non-expired item.
func (s *Store) ListAnnotations(itemID string, nowMs int64) ([]Annotation, error) {
	var seq, expires int64
	err := s.db.QueryRow(`SELECT seq, expires_ms FROM clip_items WHERE id = ?`, itemID).Scan(&seq, &expires)
	if errors.Is(err, sql.ErrNoRows) || (err == nil && expires > 0 && nowMs >= expires) {
		return nil, ErrNotFound
	}
	if err != nil {
		return nil, err
	}
	rows, err := s.db.Query(`
SELECT id, plugin_id, ann_type, plugin_version, payload_json, updated_ms
FROM history_annotations
WHERE item_seq = ?
ORDER BY id`, seq)
	if err != nil {
		return nil, err
	}
	type row struct {
		id  int64
		ann Annotation
	}
	var list []row
	for rows.Next() {
		var r row
		if err := rows.Scan(&r.id, &r.ann.PluginID, &r.ann.Type, &r.ann.PluginVersion, &r.ann.PayloadJSON, &r.ann.UpdatedMs); err != nil {
			_ = rows.Close()
			return nil, err
		}
		list = append(list, r)
	}
	if err := rows.Close(); err != nil {
		return nil, err
	}
	out := make([]Annotation, 0, len(list))
	for _, r := range list {
		arows, err := s.db.Query(`
SELECT asset_key, mime, blob, byte_size
FROM history_annotation_assets
WHERE annotation_id = ?
ORDER BY asset_key`, r.id)
		if err != nil {
			return nil, err
		}
		for arows.Next() {
			var a AnnotationAsset
			if err := arows.Scan(&a.Key, &a.MIME, &a.Blob, &a.ByteSize); err != nil {
				_ = arows.Close()
				return nil, err
			}
			r.ann.Assets = append(r.ann.Assets, a)
		}
		if err := arows.Close(); err != nil {
			return nil, err
		}
		out = append(out, r.ann)
	}
	return out, nil
}

// GetAnnotationAsset returns metadata and blob name for one persisted annotation asset.
func (s *Store) GetAnnotationAsset(itemID, pluginID, annType, assetKey string, nowMs int64) (AnnotationAsset, error) {
	var expires int64
	var a AnnotationAsset
	err := s.db.QueryRow(`
SELECT i.expires_ms, a.asset_key, a.mime, a.blob, a.byte_size
FROM clip_items i
JOIN history_annotations h ON h.item_seq = i.seq
JOIN history_annotation_assets a ON a.annotation_id = h.id
WHERE i.id = ? AND h.plugin_id = ? AND h.ann_type = ? AND a.asset_key = ?`,
		itemID, pluginID, annType, assetKey).Scan(&expires, &a.Key, &a.MIME, &a.Blob, &a.ByteSize)
	if errors.Is(err, sql.ErrNoRows) || (err == nil && expires > 0 && nowMs >= expires) {
		return AnnotationAsset{}, ErrNotFound
	}
	return a, err
}

// DeletePluginEnrichments removes all annotations, status rows, and derived search
// documents owned by pluginID and returns any unreferenced asset blobs.
func (s *Store) DeletePluginEnrichments(pluginID string) (blobs []string, err error) {
	tx, err := s.db.Begin()
	if err != nil {
		return nil, err
	}
	defer func() { _ = tx.Rollback() }()

	rows, err := tx.Query(`
SELECT a.blob
FROM history_annotation_assets a
JOIN history_annotations h ON h.id = a.annotation_id
WHERE h.plugin_id = ? AND a.blob <> ''`, pluginID)
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
	if _, err := tx.Exec(`DELETE FROM history_annotations WHERE plugin_id = ?`, pluginID); err != nil {
		return nil, err
	}
	if _, err := tx.Exec(`DELETE FROM history_enrichments WHERE plugin_id = ?`, pluginID); err != nil {
		return nil, err
	}
	if _, err := tx.Exec(`DELETE FROM history_search_docs WHERE source = ?`, "enricher:"+pluginID); err != nil {
		return nil, err
	}
	if _, err := tx.Exec(`DELETE FROM history_enrichment_search_state WHERE plugin_id = ?`, pluginID); err != nil {
		return nil, err
	}
	if _, err := bump(tx); err != nil {
		return nil, err
	}
	return blobs, tx.Commit()
}
