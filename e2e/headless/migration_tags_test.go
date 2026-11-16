package headless

import (
	"bytes"
	"context"
	"crypto/sha256"
	"database/sql"
	"encoding/hex"
	"encoding/json"
	"os"
	"os/exec"
	"path/filepath"
	"reflect"
	"slices"
	"strings"
	"testing"
	"time"

	"github.com/uniclipboard/unic/engine"
	api "github.com/uniclipboard/unic/internal/localapi"
	"github.com/uniclipboard/unic/internal/v1migration"
	"github.com/uniclipboard/unic/internal/v1migration/v1fixture"
)

// Production migration, public Engine APIs, real daemon/socket filters and restart.
// All profiles are synthetic. No display, credentials, or system clipboard is used.
func TestProductionMigrationTags(t *testing.T) {
	if os.Getenv("UNIC_MIGRATION_TAGS_E2E") == "" {
		t.Skip("set UNIC_MIGRATION_TAGS_E2E=1")
	}
	ctx, cancel := context.WithTimeout(context.Background(), 4*time.Minute)
	defer cancel()
	root, err := filepath.Abs("../..")
	need(t, err)
	work, err := os.MkdirTemp("/private/tmp", "umt")
	if err != nil {
		work, err = os.MkdirTemp("", "umt")
	}
	need(t, err)
	artifacts := os.Getenv("UNIC_MIGRATION_TAGS_ARTIFACTS")
	if artifacts == "" {
		artifacts = t.TempDir()
	}
	need(t, os.MkdirAll(artifacts, 0700))
	receipt := map[string]any{"status": "running", "native_ui": "NOT_RUN", "v1_engine": "40c5ebf604a874647c46d4d0023dd2d795674a31"}
	checks := []string{}
	defer func() {
		cleanupErr := os.RemoveAll(work)
		if cleanupErr != nil {
			t.Error("temporary profile cleanup failed", cleanupErr)
		}
		receipt["temporary_profiles_removed"] = cleanupErr == nil
		if t.Failed() {
			receipt["status"] = "failed"
		} else {
			receipt["status"] = "passed"
		}
		receipt["checks"] = checks
		b, _ := json.MarshalIndent(receipt, "", "  ")
		need(t, os.WriteFile(filepath.Join(artifacts, "receipt.json"), b, 0600))
		sum := sha256.Sum256(b)
		need(t, os.WriteFile(filepath.Join(artifacts, "SHA256SUMS"), []byte(hex.EncodeToString(sum[:])+"  receipt.json\n"), 0600))
	}()
	sourceHashesBefore, err := sourceHashes(root)
	need(t, err)
	receipt["source_sha256"] = sourceHashesBefore
	head, err := exec.Command("git", "-C", root, "rev-parse", "HEAD").Output()
	need(t, err)
	receipt["source_commit"] = strings.TrimSpace(string(head))
	env := []string{}
	for _, v := range os.Environ() {
		if !strings.HasPrefix(v, "UNIC_") {
			env = append(env, v)
		}
	}
	binaries := map[string]string{}
	for _, name := range []string{"unic", "unicd"} {
		bin := filepath.Join(work, name)
		cmd := exec.CommandContext(ctx, "go", "build", "-race", "-o", bin, "./app/service/cmd/"+name)
		cmd.Dir = root
		out, err := cmd.CombinedOutput()
		if err != nil {
			t.Fatalf("build: %s %v", out, err)
		}
		b, err := os.ReadFile(bin)
		need(t, err)
		sum := sha256.Sum256(b)
		binaries[name] = hex.EncodeToString(sum[:])
	}
	receipt["binary_sha256"] = binaries
	source, dbpath, f, err := v1fixture.Build(filepath.Join(work, "source"))
	need(t, err)
	frozen, err := os.ReadFile(filepath.Join(root, "internal/v1migration/testdata/v3.json"))
	need(t, err)
	var extra struct {
		LinkText          v1fixture.Bytes `json:"link_text"`
		DirectoryRoot     v1fixture.Bytes `json:"directory_root"`
		DirectoryRelative v1fixture.Bytes `json:"directory_relative"`
		Collision         v1fixture.Bytes `json:"collision_tag"`
		Assignment        v1fixture.Bytes `json:"multi_assignment"`
	}
	need(t, json.Unmarshal(frozen, &extra))
	if len(extra.Collision) == 0 {
		t.Fatal("missing extended Rust fixture")
	}
	db, err := sql.Open("sqlite", dbpath)
	need(t, err)
	_, err = db.Exec(`DELETE FROM clipboard_entry WHERE entry_id='entry-bad'; INSERT INTO history_tag VALUES('tag-collision',?); CREATE TABLE search_entry_tag(profile_id TEXT NOT NULL,entry_id TEXT NOT NULL,tag_id TEXT NOT NULL,PRIMARY KEY(profile_id,entry_id,tag_id)); INSERT INTO search_entry_tag VALUES('default','entry-duplicate','link'),('default','entry-image','image'),('default','entry-file','directory'),('default','entry-text','code'),('other-profile','entry-text','link');`, []byte(extra.Collision))
	need(t, err)
	_, err = db.Exec("UPDATE history_tag_assignment SET tags_ct=? WHERE entry_id='entry-text'", []byte(extra.Assignment))
	need(t, err)
	_, err = db.Exec("UPDATE clipboard_entry SET event_id='event-link' WHERE entry_id='entry-duplicate'")
	need(t, err)
	_, err = db.Exec("INSERT INTO clipboard_snapshot_representation VALUES('rep-link','event-link','text/plain','text/plain',?,NULL,?)", []byte(extra.LinkText), len("https://example.test/migration"))
	need(t, err)
	_, err = db.Exec("UPDATE entry_file_set SET root_name_ct=?,relative_path_ct=? WHERE entry_id='entry-file'", []byte(extra.DirectoryRoot), []byte(extra.DirectoryRelative))
	need(t, err)
	need(t, db.Close())
	snapshot := func() map[string]string {
		out := map[string]string{}
		need(t, filepath.WalkDir(source, func(p string, d os.DirEntry, e error) error {
			if e != nil {
				return e
			}
			if !d.IsDir() {
				b, e := os.ReadFile(p)
				if e != nil {
					return e
				}
				s := sha256.Sum256(b)
				rel, _ := filepath.Rel(source, p)
				out[rel] = hex.EncodeToString(s[:])
			}
			return nil
		}))
		return out
	}
	before := snapshot()
	receipt["fixture_sha256"] = before
	target := filepath.Join(work, "target")
	migrate := func(target string, expected int, flags ...string) map[string]any {
		args := []string{"migrate", "--source", source, "--target", target, "--passphrase-stdin", "--json"}
		args = append(args, flags...)
		cmd := exec.CommandContext(ctx, filepath.Join(work, "unic"), args...)
		cmd.Env = env
		cmd.Stdin = strings.NewReader(f.Passphrase + "\n")
		var stderr bytes.Buffer
		cmd.Stderr = &stderr
		out, e := cmd.Output()
		code := 0
		if e != nil {
			if x, ok := e.(*exec.ExitError); ok {
				code = x.ExitCode()
			} else {
				t.Fatal(e)
			}
		}
		if code != expected {
			t.Fatalf("migration exit=%d want=%d stdout=%s stderr=%s", code, expected, out, stderr.String())
		}
		lines := strings.Split(strings.TrimSpace(string(out)), "\n")
		var report map[string]any
		need(t, json.Unmarshal([]byte(lines[len(lines)-1]), &report))
		return report
	}
	dryTarget := filepath.Join(work, "dry-target")
	dry := migrate(dryTarget, 0, "--dry-run", "--repair-tags")
	if dry["status"] != "ready" {
		t.Fatal(dry)
	}
	if _, err := os.Stat(dryTarget); !os.IsNotExist(err) {
		t.Fatal("dry run created target")
	}
	receipt["dry_run_report"] = dry
	first := migrate(target, 0)
	receipt["initial_report"] = first
	src, err := v1migration.Open(ctx, source, "fixture", []byte(f.Passphrase), nil)
	need(t, err)
	sourceID := src.ID()
	need(t, src.Close())
	expected := map[string][]string{"entry-text": {"Links", "builtin:code", "标签"}, "entry-duplicate": {"builtin:link"}, "entry-image": {"builtin:image"}, "entry-file": {"builtin:folder"}}
	read := func(target string) map[string]engine.HistoryItem {
		e, err := engine.New(ctx, engine.Options{DataDir: target, MigrationMode: true})
		need(t, err)
		defer e.Close(ctx)
		items := map[string]engine.HistoryItem{}
		for record := range expected {
			id, known, err := e.LookupHistoryImport(ctx, sourceID, record)
			need(t, err)
			if !known {
				t.Fatal("missing import mapping", record)
			}
			item, err := e.GetItem(ctx, id)
			need(t, err)
			items[record] = item
		}
		return items
	}
	initial := read(target)
	actual := map[string][]string{}
	for record, want := range expected {
		actual[record] = initial[record].Tags
		if !slices.Equal(initial[record].Tags, want) {
			t.Fatalf("%s tags=%v want=%v", record, initial[record].Tags, want)
		}
	}
	receipt["expected_tags"] = expected
	receipt["actual_tags"] = actual
	checks = append(checks, "custom associations, multiple tags, canonical built-ins and name collision")
	repeat := migrate(target, 0)
	if repeat["imported"] != float64(0) || repeat["skipped"] != float64(4) {
		t.Fatal(repeat)
	}
	receipt["repeat_report"] = repeat
	need(t, os.WriteFile(filepath.Join(target, "config.toml"), []byte("[telemetry]\nenabled=false\n[history]\nretention_days=0\n[sync]\nenabled=false\n"), 0600))
	for restart := 0; restart < 2; restart++ {
		p := start(t, filepath.Join(work, "unicd"), nil, append(env, "UNIC_HOME="+target, "UNIC_RENDEZVOUS_URL=http://127.0.0.1:1"), filepath.Join(work, "daemon.log"))
		defer p.stop()
		c := api.NewClient(filepath.Join(target, "unic.sock"))
		until(t, "daemon", func() bool { _, err := c.HistoryTags(ctx); return err == nil })
		tags, err := c.HistoryTags(ctx)
		need(t, err)
		if !slices.Contains(tags, "unused") {
			t.Fatal("unassigned tag lost")
		}
		for record, want := range expected {
			for _, tag := range want {
				page, err := c.ListHistory(ctx, "", tag)
				need(t, err)
				if len(page.Items) != 1 || page.Items[0].ID != string(initial[record].ID) {
					t.Fatalf("filter %s: %+v", tag, page)
				}
				search, err := c.SearchHistory(ctx, engine.HistorySearchRequest{Query: engine.HistorySearchQuery{Tags: []string{tag}}})
				need(t, err)
				if len(search.Hits) != 1 || search.Hits[0].Item.ID != string(initial[record].ID) {
					t.Fatalf("search filter %s: %+v", tag, search)
				}
			}
		}
		p.stop()
	}
	checks = append(checks, "real history/tag/search socket filters after two daemon starts")
	// Reproduce the old importer through the public offline import API.
	old := filepath.Join(work, "old")
	e, err := engine.New(ctx, engine.Options{DataDir: old, MigrationMode: true})
	need(t, err)
	unrelated, err := e.ImportHistory(ctx, engine.HistoryImportRequest{Source: "unrelated-source", RecordID: "unrelated-record", Tags: []string{"keep-existing"}, Content: engine.ContentInput{Representations: []engine.RepresentationInput{{MIME: "text/plain", Size: 8, Reader: strings.NewReader("existing")}}}})
	need(t, err)
	src, err = v1migration.Open(ctx, source, "fixture", []byte(f.Passphrase), nil)
	need(t, err)
	need(t, src.Walk(ctx, func(r *v1migration.Record, se error) error {
		if se != nil {
			return se
		}
		req := engine.HistoryImportRequest{Source: sourceID, RecordID: r.ID, CreatedAtMs: r.CreatedAtMs, UpdatedAtMs: r.UpdatedAtMs}
		for _, part := range r.Parts {
			reader, err := part.Open()
			if err != nil {
				return err
			}
			defer reader.Close()
			if part.Name == "" {
				req.Content.Representations = append(req.Content.Representations, engine.RepresentationInput{MIME: part.MIME, Size: part.Size, Reader: reader})
			} else {
				req.Content.Files = append(req.Content.Files, engine.FileInput{Name: part.Name, MIME: part.MIME, Size: part.Size, Reader: reader})
			}
		}
		_, err := e.ImportHistory(ctx, req)
		return err
	}))
	need(t, src.Close())
	editedID, _, err := e.LookupHistoryImport(ctx, sourceID, "entry-text")
	need(t, err)
	editedTags := []string{"target-edit"}
	_, err = e.UpdateItem(ctx, engine.UpdateItemRequest{ID: editedID, ExpectedRevision: 1, Tags: &editedTags})
	need(t, err)
	deletedID, _, err := e.LookupHistoryImport(ctx, sourceID, "entry-image")
	need(t, err)
	need(t, e.DeleteItem(ctx, engine.DeleteItemRequest{ID: deletedID, ExpectedRevision: 1}))
	need(t, e.Close(ctx))
	diagnosis := migrate(old, 3)
	receipt["diagnosis"] = diagnosis
	if diagnosis["tag_mismatches"] != float64(3) {
		t.Fatal(diagnosis)
	}
	repair := migrate(old, 3, "--repair-tags")
	receipt["repair"] = repair
	if repair["tags_repaired"] != float64(2) || repair["tag_repairs_skipped"] != float64(1) {
		t.Fatal(repair)
	}
	again := migrate(old, 3, "--repair-tags")
	if again["tags_repaired"] != float64(0) {
		t.Fatal(again)
	}
	e, err = engine.New(ctx, engine.Options{DataDir: old, MigrationMode: true})
	need(t, err)
	edited, err := e.GetItem(ctx, editedID)
	need(t, err)
	if !slices.Equal(edited.Tags, editedTags) {
		t.Fatal("target edit overwritten")
	}
	_, err = e.GetItem(ctx, deletedID)
	if err == nil {
		t.Fatal("deleted item resurrected")
	}
	for _, r := range []string{"entry-duplicate", "entry-file"} {
		id, _, err := e.LookupHistoryImport(ctx, sourceID, r)
		need(t, err)
		it, err := e.GetItem(ctx, id)
		need(t, err)
		if !slices.Equal(it.Tags, expected[r]) {
			t.Fatal("repair missing", r)
		}
	}
	unrelatedItem, err := e.GetItem(ctx, unrelated.HistoryID)
	need(t, err)
	if !slices.Equal(unrelatedItem.Tags, []string{"keep-existing"}) {
		t.Fatal("unrelated history changed")
	}
	page, err := e.ListHistory(ctx, engine.ListHistoryRequest{})
	need(t, err)
	if len(page.Items) != 4 {
		t.Fatalf("unexpected repaired history count %d", len(page.Items))
	}
	need(t, e.Close(ctx))
	checks = append(checks, "explicit repair preserves target edits and deletion and is idempotent")
	if !reflect.DeepEqual(before, snapshot()) {
		t.Fatal("source changed")
	}
	after, err := sourceHashes(root)
	need(t, err)
	if !reflect.DeepEqual(sourceHashesBefore, after) {
		t.Fatal("source tree changed")
	}
	// Each failure case uses an independent target and preserves its source snapshot.
	db, err = sql.Open("sqlite", dbpath)
	need(t, err)
	_, err = db.Exec("INSERT INTO search_entry_tag VALUES('default','entry-duplicate','favorited')")
	need(t, err)
	need(t, db.Close())
	variantBefore := snapshot()
	unsupported := migrate(filepath.Join(work, "unsupported"), 3)
	receipt["unsupported_report"] = unsupported
	if unsupported["unsupported_tags"] != float64(1) || unsupported["imported"] != float64(4) {
		t.Fatal(unsupported)
	}
	if !reflect.DeepEqual(variantBefore, snapshot()) {
		t.Fatal("unsupported source changed")
	}
	db, err = sql.Open("sqlite", dbpath)
	need(t, err)
	_, err = db.Exec("DELETE FROM search_entry_tag WHERE tag_id='favorited'; UPDATE history_tag_assignment SET tags_ct=X'0001'")
	need(t, err)
	need(t, db.Close())
	variantBefore = snapshot()
	damaged := migrate(filepath.Join(work, "damaged"), 3)
	receipt["damaged_assignment_report"] = damaged
	if damaged["tag_failures"] != float64(1) || damaged["failed"] != float64(1) || damaged["imported"] != float64(3) {
		t.Fatal(damaged)
	}
	if !reflect.DeepEqual(variantBefore, snapshot()) {
		t.Fatal("damaged source changed")
	}
	db, err = sql.Open("sqlite", dbpath)
	need(t, err)
	_, err = db.Exec("UPDATE history_tag_assignment SET tags_ct=?", []byte(extra.Assignment))
	need(t, err)
	_, err = db.Exec("DELETE FROM history_tag")
	need(t, err)
	need(t, db.Close())
	variantBefore = snapshot()
	orphaned := migrate(filepath.Join(work, "orphaned"), 3)
	receipt["orphaned_assignment_report"] = orphaned
	if orphaned["tag_failures"] != float64(1) || orphaned["failed"] != float64(1) {
		t.Fatal(orphaned)
	}
	if !reflect.DeepEqual(variantBefore, snapshot()) {
		t.Fatal("orphaned source changed")
	}
	checks = append(checks, "unsupported favorites, corrupted assignments and missing definitions report partial")
	receipt["source_unchanged"] = true
	checks = append(checks, "source profile and implementation checksums unchanged")
}
