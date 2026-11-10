// Package v1fixture builds a synthetic UniClipboard v1 profile for migration tests.
// It is test support. No release program imports it.
package v1fixture

import (
	"database/sql"
	"encoding/json"
	"errors"
	"os"
	"path/filepath"
	"runtime"

	_ "modernc.org/sqlite"
)

var testdata = func() string {
	_, file, _, _ := runtime.Caller(0)
	return filepath.Join(filepath.Dir(file), "..", "testdata")
}()

// Bytes decodes Rust Vec<u8>, which serializes as an integer array.
type Bytes []byte

func (b *Bytes) UnmarshalJSON(data []byte) error {
	var ints []uint16
	if err := json.Unmarshal(data, &ints); err != nil {
		return err
	}
	*b = make([]byte, len(ints))
	for i, v := range ints {
		if v > 255 {
			return errors.New("byte out of range")
		}
		(*b)[i] = byte(v)
	}
	return nil
}

type Fixture struct {
	Generation                  string                     `json:"generation"`
	Passphrase                  string                     `json:"passphrase"`
	KEK                         Bytes                      `json:"kek"`
	Vault                       map[string]json.RawMessage `json:"vault"`
	Text, HTML, Repaired, Image Bytes
	FileBlob                    Bytes  `json:"file_blob"`
	ImageBlob                   Bytes  `json:"image_blob"`
	ImageHash                   string `json:"image_hash"`
	FileHash                    string `json:"file_hash"`
	FileOriginal                Bytes  `json:"file_original"`
	Tag, Assignment             Bytes
	UnusedTag                   Bytes `json:"unused_tag"`
}

func Build(root string) (string, string, Fixture, error) {
	data, err := os.ReadFile(filepath.Join(testdata, "v3.json"))
	if err != nil {
		return "", "", Fixture{}, err
	}
	var f Fixture
	if json.Unmarshal(data, &f) != nil {
		return "", "", Fixture{}, errors.New("invalid Rust fixture")
	}
	vault := filepath.Join(root, "vault")
	if err = os.MkdirAll(vault, 0o700); err != nil {
		return "", "", Fixture{}, err
	}
	for name, data := range f.Vault {
		if err = os.WriteFile(filepath.Join(vault, name), data, 0o600); err != nil {
			return "", "", Fixture{}, err
		}
	}
	dir := filepath.Join(root, "profile-data-generations", f.Generation, "v3-payloads")
	if err = os.MkdirAll(filepath.Join(dir, "blobs"), 0o700); err != nil {
		return "", "", Fixture{}, err
	}
	if err = os.WriteFile(filepath.Join(dir, "blobs", "blob-file"), f.FileBlob, 0o600); err != nil {
		return "", "", Fixture{}, err
	}
	if err = os.WriteFile(filepath.Join(dir, "blobs", "blob-image"), f.ImageBlob, 0o600); err != nil {
		return "", "", Fixture{}, err
	}
	pixelData, err := os.ReadFile(filepath.Join(testdata, "pixel.png"))
	if err != nil {
		return "", "", Fixture{}, err
	}
	pixel := len(pixelData)
	dbpath := filepath.Join(dir, "profile.sqlite")
	db, err := sql.Open("sqlite", dbpath)
	if err != nil {
		return "", "", Fixture{}, err
	}
	defer func() { _ = db.Close() }()
	schema := `
CREATE TABLE clipboard_entry(entry_id TEXT PRIMARY KEY,event_id TEXT,created_at_ms INTEGER,active_time_ms INTEGER,content_category TEXT,deleted_at_ms INTEGER);
CREATE TABLE clipboard_snapshot_representation(id TEXT PRIMARY KEY,event_id TEXT,mime_type TEXT,format_id TEXT,inline_data BLOB,blob_id TEXT,size_bytes INTEGER);
CREATE TABLE entry_file_set(entry_id TEXT,line_index INTEGER,kind TEXT,blob_id TEXT,size_bytes INTEGER,content_hash TEXT,original_text_ct BLOB,relative_path_ct BLOB,root_name_ct BLOB,kind_tag TEXT,root_index INTEGER);
CREATE TABLE blob(blob_id TEXT PRIMARY KEY,content_hash TEXT,size_bytes INTEGER,storage_path TEXT);
CREATE TABLE history_tag(tag_id TEXT PRIMARY KEY,payload_ct BLOB);
CREATE TABLE history_tag_assignment(entry_id TEXT PRIMARY KEY,tags_ct BLOB);
INSERT INTO clipboard_entry VALUES('entry-text','event-text',1000,1100,'text',NULL),
 ('entry-duplicate','event-text',2000,2100,'text',NULL),('entry-file','event-file',3000,3100,'file',NULL),
 ('entry-bad','event-bad',4000,4100,'text',NULL),('entry-deleted','event-text',5000,5000,'text',5100),
 ('entry-image','event-image',3500,3600,'image',NULL);
`
	if _, err = db.Exec(schema); err != nil {
		return "", "", Fixture{}, err
	}
	for _, q := range []struct {
		sql  string
		args []any
	}{
		{`INSERT INTO clipboard_snapshot_representation VALUES('rep-text','event-text','text/plain','text/plain',?,NULL,12)`, []any{[]byte(f.Text)}},
		{`INSERT INTO clipboard_snapshot_representation VALUES('rep-html','event-text','text/html','text/html',?,NULL,19)`, []any{[]byte(f.HTML)}},
		// Legacy conversion updated MIME/blob but retained the larger TIFF size.
		{`INSERT INTO clipboard_snapshot_representation VALUES('rep-image','event-image','image/png','public.png',NULL,'blob-image',?)`, []any{int64(pixel + 1024)}},
		{`INSERT INTO entry_file_set VALUES('entry-file',0,'file','blob-file',14,?,?,NULL,NULL,'f',0)`, []any{f.FileHash, []byte(f.FileOriginal)}},
		{`INSERT INTO blob VALUES('blob-file',?,14,'unused')`, []any{f.FileHash}},
		{`INSERT INTO blob VALUES('blob-image',?,?,'unused')`, []any{f.ImageHash, pixel}},
		{`INSERT INTO history_tag VALUES('tag-one',?),('tag-unused',?)`, []any{[]byte(f.Tag), []byte(f.UnusedTag)}},
		{`INSERT INTO history_tag_assignment VALUES('entry-text',?)`, []any{[]byte(f.Assignment)}},
	} {
		if _, err = db.Exec(q.sql, q.args...); err != nil {
			return "", "", Fixture{}, err
		}
	}

	return root, dbpath, f, nil
}
