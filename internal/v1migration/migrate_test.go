package v1migration

import (
	"bytes"
	"context"
	"database/sql"
	"encoding/json"
	"errors"
	"io"
	"os"
	"path/filepath"
	"testing"

	"github.com/uniclipboard/unic/engine"
)

type fixture struct {
	Generation                  string                     `json:"generation"`
	Passphrase                  string                     `json:"passphrase"`
	KEK                         rustBytes                  `json:"kek"`
	Vault                       map[string]json.RawMessage `json:"vault"`
	Text, HTML, Repaired, Image rustBytes
	FileBlob                    rustBytes `json:"file_blob"`
	ImageBlob                   rustBytes `json:"image_blob"`
	ImageHash                   string    `json:"image_hash"`
	FileHash                    string    `json:"file_hash"`
	FileOriginal                rustBytes `json:"file_original"`
	Tag, Assignment             rustBytes
	UnusedTag                   rustBytes `json:"unused_tag"`
}

func sourceFixture(t *testing.T) (string, string, fixture) {
	t.Helper()
	data, err := os.ReadFile("testdata/v3.json")
	if err != nil {
		t.Fatal(err)
	}
	var f fixture
	if json.Unmarshal(data, &f) != nil {
		t.Fatal("invalid Rust fixture")
	}
	root := t.TempDir()
	vault := filepath.Join(root, "vault")
	if err = os.MkdirAll(vault, 0o700); err != nil {
		t.Fatal(err)
	}
	for name, data := range f.Vault {
		if err = os.WriteFile(filepath.Join(vault, name), data, 0o600); err != nil {
			t.Fatal(err)
		}
	}
	dir := filepath.Join(root, "profile-data-generations", f.Generation, "v3-payloads")
	if err = os.MkdirAll(filepath.Join(dir, "blobs"), 0o700); err != nil {
		t.Fatal(err)
	}
	if err = os.WriteFile(filepath.Join(dir, "blobs", "blob-file"), f.FileBlob, 0o600); err != nil {
		t.Fatal(err)
	}
	if err = os.WriteFile(filepath.Join(dir, "blobs", "blob-image"), f.ImageBlob, 0o600); err != nil {
		t.Fatal(err)
	}
	dbpath := filepath.Join(dir, "profile.sqlite")
	db, err := sql.Open("sqlite", dbpath)
	if err != nil {
		t.Fatal(err)
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
		t.Fatal(err)
	}
	for _, q := range []struct {
		sql  string
		args []any
	}{
		{`INSERT INTO clipboard_snapshot_representation VALUES('rep-text','event-text','text/plain','text/plain',?,NULL,12)`, []any{[]byte(f.Text)}},
		{`INSERT INTO clipboard_snapshot_representation VALUES('rep-html','event-text','text/html','text/html',?,NULL,19)`, []any{[]byte(f.HTML)}},
		// Legacy conversion updated MIME/blob but retained the larger TIFF size.
		{`INSERT INTO clipboard_snapshot_representation VALUES('rep-image','event-image','image/png','public.png',NULL,'blob-image',?)`, []any{int64(lenMustRead(t, "testdata/pixel.png") + 1024)}},
		{`INSERT INTO entry_file_set VALUES('entry-file',0,'file','blob-file',14,?,?,NULL,NULL,'f',0)`, []any{f.FileHash, []byte(f.FileOriginal)}},
		{`INSERT INTO blob VALUES('blob-file',?,14,'unused')`, []any{f.FileHash}},
		{`INSERT INTO blob VALUES('blob-image',?,?,'unused')`, []any{f.ImageHash, lenMustRead(t, "testdata/pixel.png")}},
		{`INSERT INTO history_tag VALUES('tag-one',?),('tag-unused',?)`, []any{[]byte(f.Tag), []byte(f.UnusedTag)}},
		{`INSERT INTO history_tag_assignment VALUES('entry-text',?)`, []any{[]byte(f.Assignment)}},
	} {
		if _, err = db.Exec(q.sql, q.args...); err != nil {
			t.Fatal(err)
		}
	}
	return root, dbpath, f
}

func TestRustFixtureMigrationResumeAndSourceIndependence(t *testing.T) {
	ctx := context.Background()
	root, dbpath, f := sourceFixture(t)
	target := filepath.Join(t.TempDir(), "unic")
	open := func() *Source {
		s, err := Open(ctx, root, "fixture-service", []byte(f.Passphrase), nil)
		if err != nil {
			t.Fatal(err)
		}
		return s
	}
	s := open()
	report, err := s.Run(ctx, RunOptions{Target: target})
	_ = s.Close()
	if err != nil {
		t.Fatal(err)
	}
	if report.Status != "partial" || report.Imported != 4 || report.Failed != 1 || report.Verified != 4 {
		t.Fatalf("%+v", report)
	}
	// The missing row must remain retryable, while successful rows stay single.
	db, err := sql.Open("sqlite", dbpath)
	if err != nil {
		t.Fatal(err)
	}
	_, err = db.Exec(`INSERT INTO clipboard_snapshot_representation VALUES('rep-bad','event-bad','text/plain','text/plain',?,NULL,9)`, []byte(f.Repaired))
	_ = db.Close()
	if err != nil {
		t.Fatal(err)
	}
	s = open()
	report, err = s.Run(ctx, RunOptions{Target: target})
	sourceID := s.ID()
	_ = s.Close()
	if err != nil {
		t.Fatal(err)
	}
	if report.Status != "complete" || report.Imported != 1 || report.Skipped != 4 || report.Verified != 5 {
		t.Fatalf("%+v", report)
	}
	// Rename the entire old profile: neither a path-based dedup key nor target
	// content references may depend on its original location.
	moved := root + "-moved"
	if os.Rename(root, moved) != nil {
		t.Fatal("move fixture")
	}
	t.Cleanup(func() { _ = os.RemoveAll(moved) })
	s, err = Open(ctx, moved, "fixture-service", nil, func(_, account string) ([]byte, error) {
		if account == "kek:v1:profile:default" {
			return bytes.Clone(f.KEK), nil
		}
		return nil, ErrUnlock
	})
	if err != nil {
		t.Fatal(err)
	}
	if s.ID() != sourceID {
		t.Fatal("moving source changed its identity")
	}
	report, err = s.Run(ctx, RunOptions{Target: target})
	_ = s.Close()
	if err != nil || report.Imported != 0 || report.Skipped != 5 {
		t.Fatalf("%+v %v", report, err)
	}
	if err = os.RemoveAll(moved); err != nil {
		t.Fatal(err)
	}
	e, err := engine.New(ctx, engine.Options{DataDir: target})
	if err != nil {
		t.Fatal(err)
	}
	defer func() { _ = e.Close(ctx) }()
	page, err := e.ListHistory(ctx, engine.ListHistoryRequest{})
	if err != nil || len(page.Items) != 5 {
		t.Fatal("lost records", err)
	}
	tags, err := e.ListHistoryTags(ctx)
	if err != nil || len(tags) != 2 {
		t.Fatal("lost unassigned tags")
	}
	id, known, err := e.LookupHistoryImport(ctx, sourceID, "entry-file")
	if err != nil || !known {
		t.Fatal("lost import map")
	}
	h, err := e.RetainHistoryContent(ctx, id)
	if err != nil {
		t.Fatal(err)
	}
	defer func() { _ = e.ReleaseContent(ctx, h.LeaseID) }()
	rd, err := e.ReadContent(ctx, h.LeaseID, h.Content.Parts[0].ID)
	if err != nil {
		t.Fatal(err)
	}
	got, err := io.ReadAll(rd)
	_ = rd.Close()
	if err != nil || !bytes.Equal(got, []byte("binary\x00fixture")) {
		t.Fatal("file depends on old source")
	}
}

func lenMustRead(t *testing.T, path string) int {
	t.Helper()
	b, err := os.ReadFile(path)
	if err != nil {
		t.Fatal(err)
	}
	return len(b)
}

func TestRustFixtureAuthenticationAndTampering(t *testing.T) {
	ctx := context.Background()
	root, _, f := sourceFixture(t)
	if _, err := Open(ctx, root, "fixture", []byte("wrong"), nil); !errors.Is(err, ErrUnlock) {
		t.Fatal("wrong password accepted")
	}
	if _, err := Open(ctx, root, "fixture", nil, func(string, string) ([]byte, error) { return nil, ErrUnlock }); !errors.Is(err, ErrUnlock) {
		t.Fatal("missing credentials accepted")
	}
	s, err := Open(ctx, root, "fixture", []byte(f.Passphrase), nil)
	if err != nil {
		t.Fatal(err)
	}
	ct := bytes.Clone(f.Text)
	ct[len(ct)-1] ^= 1
	if _, err = s.crypto.open(ct, inlineAAD("event-text", "rep-text")); err == nil {
		t.Fatal("tampered content accepted")
	}
	if _, err = s.crypto.open(f.Text, inlineAAD("wrong", "rep-text")); err == nil {
		t.Fatal("wrong AAD accepted")
	}
	path := filepath.Join(s.crypto.blobRoot, "blob-file")
	blob := bytes.Clone(f.FileBlob)
	blob[len(blob)-1] ^= 1
	if os.WriteFile(path, blob, 0o600) != nil {
		t.Fatal("tamper fixture")
	}
	if _, err = s.blob("blob-file"); err == nil {
		t.Fatal("tampered blob accepted")
	}
	_ = s.Close()
}
