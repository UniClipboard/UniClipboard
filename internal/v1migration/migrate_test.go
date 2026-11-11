package v1migration

import (
	"bytes"
	"context"
	"database/sql"
	"errors"
	"io"
	"os"
	"path/filepath"
	"testing"

	"github.com/uniclipboard/unic/engine"
	"github.com/uniclipboard/unic/internal/v1migration/v1fixture"
)

func sourceFixture(t *testing.T) (string, string, fixture) {
	t.Helper()
	root, dbpath, f, err := v1fixture.Build(t.TempDir())
	if err != nil {
		t.Fatal(err)
	}
	return root, dbpath, f
}

type fixture = v1fixture.Fixture

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
	// This fixture dates from 1970. Test source independence without age cleanup.
	settings := engine.DefaultSettings()
	settings.History.RetentionDays = 0
	e, err := engine.New(ctx, engine.Options{DataDir: target, Settings: settings})
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
