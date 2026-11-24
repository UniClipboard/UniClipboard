package main

import (
	"bytes"
	"context"
	"crypto/sha256"
	"database/sql"
	"encoding/hex"
	"encoding/json"
	"flag"
	"fmt"
	"io"
	"os"
	"path/filepath"
	"runtime"
	"slices"
	"time"

	_ "modernc.org/sqlite"

	"github.com/uniclipboard/unic/engine"
	"github.com/uniclipboard/unic/internal/v1migration/v1fixture"
)

type generateOutput struct {
	Variant        string   `json:"variant"`
	SourceDir      string   `json:"sourceDir"`
	DBPath         string   `json:"dbPath,omitempty"`
	SourceDBSha256 string   `json:"sourceDbSha256,omitempty"`
	Passphrase     string   `json:"passphrase,omitempty"`
	ExpectedCount  int      `json:"expectedCount"`
	ExpectedFailed int      `json:"expectedFailed"`
	ExpectedTags   []string `json:"expectedTags,omitempty"`
}

func sha256File(path string) (string, error) {
	raw, err := os.ReadFile(path)
	if err != nil {
		return "", err
	}
	sum := sha256.Sum256(raw)
	return hex.EncodeToString(sum[:]), nil
}

func repoRoot() string {
	_, file, _, _ := runtime.Caller(0)
	return filepath.Clean(filepath.Join(filepath.Dir(file), "..", "..", "..", ".."))
}

func fatal(format string, args ...any) {
	fmt.Fprintf(os.Stderr, format+"\n", args...)
	os.Exit(1)
}

func runGenerate(variant, outDir string) {
	if outDir == "" {
		fatal("-out is required")
	}
	if err := os.RemoveAll(outDir); err != nil {
		fatal("clean outDir: %v", err)
	}
	if err := os.MkdirAll(outDir, 0o700); err != nil {
		fatal("mkdir outDir: %v", err)
	}
	absOut, err := filepath.EvalSymlinks(outDir)
	if err != nil {
		absOut, _ = filepath.Abs(outDir)
	}

	if variant == "empty" {
		b, _ := json.Marshal(generateOutput{
			Variant:       variant,
			SourceDir:     absOut,
			ExpectedCount: 0,
		})
		fmt.Println(string(b))
		return
	}

	if variant != "complete" && variant != "partial" {
		fatal("unsupported variant %q (want complete, partial, or empty)", variant)
	}

	root, dbPath, f, err := v1fixture.Build(absOut)
	if err != nil {
		fatal("v1fixture.Build: %v", err)
	}

	db, err := sql.Open("sqlite", dbPath)
	if err != nil {
		fatal("open sqlite: %v", err)
	}
	if variant == "complete" {
		if _, err := db.Exec(
			`INSERT INTO clipboard_snapshot_representation VALUES('rep-bad','event-bad','text/plain','text/plain',?,NULL,9)`,
			[]byte(f.Repaired),
		); err != nil {
			_ = db.Close()
			fatal("insert rep-bad: %v", err)
		}
	}

	now := time.Now().UnixMilli()
	entries := []string{"entry-text", "entry-duplicate", "entry-file", "entry-image", "entry-bad", "entry-deleted"}
	for idx, id := range entries {
		ts := now - int64(len(entries)-idx)*60_000
		if _, err := db.Exec(
			`UPDATE clipboard_entry SET created_at_ms = ?, active_time_ms = ? WHERE entry_id = ?`,
			ts, ts+100, id,
		); err != nil {
			_ = db.Close()
			fatal("update timestamps for %s: %v", id, err)
		}
	}
	if err := db.Close(); err != nil {
		fatal("close sqlite: %v", err)
	}

	sum, err := sha256File(dbPath)
	if err != nil {
		fatal("hash db: %v", err)
	}

	expectedCount := 4
	expectedFailed := 1
	if variant == "complete" {
		expectedCount = 5
		expectedFailed = 0
	}
	out := generateOutput{
		Variant:        variant,
		SourceDir:      root,
		DBPath:         dbPath,
		SourceDBSha256: sum,
		Passphrase:     f.Passphrase,
		ExpectedCount:  expectedCount,
		ExpectedFailed: expectedFailed,
		ExpectedTags:   []string{"标签", "unused"},
	}
	b, _ := json.Marshal(out)
	fmt.Println(string(b))
}

func runVerify(sourceDir, sourceDB, sourceSHA, targetDir string, expectedCount int, deleteSource bool) {
	if targetDir == "" {
		fatal("-target is required")
	}

	markerRaw, err := os.ReadFile(filepath.Join(targetDir, "mobile-setup.json"))
	if err != nil {
		fatal("read mobile-setup.json: %v", err)
	}
	var marker struct {
		Complete bool   `json:"complete"`
		Source   string `json:"source,omitempty"`
	}
	if err := json.Unmarshal(markerRaw, &marker); err != nil {
		fatal("parse mobile-setup.json: %v (%s)", err, string(markerRaw))
	}
	if !marker.Complete {
		fatal("mobile-setup.json complete=false, expected true after entering history: %s", string(markerRaw))
	}

	if sourceDB != "" && sourceSHA != "" {
		afterSHA, err := sha256File(sourceDB)
		if err != nil {
			fatal("read source DB for read-only check: %v", err)
		}
		if afterSHA != sourceSHA {
			fatal("source DB modified during migration: before=%s after=%s", sourceSHA, afterSHA)
		}
	}

	if deleteSource && sourceDir != "" {
		if err := os.RemoveAll(sourceDir); err != nil {
			fatal("delete source dir: %v", err)
		}
	}

	ctx := context.Background()
	settings := engine.DefaultSettings()
	settings.History.AutoCleanup = false
	settings.Sync.Enabled = false

	eng, err := engine.New(ctx, engine.Options{
		DataDir:       targetDir,
		MigrationMode: true,
		Settings:      settings,
	})
	if err != nil {
		fatal("engine.New verify target: %v", err)
	}
	defer func() { _ = eng.Close(ctx) }()

	page, err := eng.ListHistory(ctx, engine.ListHistoryRequest{Limit: 50})
	if err != nil {
		fatal("ListHistory: %v", err)
	}
	if len(page.Items) != expectedCount {
		fatal("history count mismatch: got %d, want %d", len(page.Items), expectedCount)
	}

	if expectedCount > 0 {
		tags, err := eng.ListHistoryTags(ctx)
		if err != nil {
			fatal("ListHistoryTags: %v", err)
		}
		if !slices.Contains(tags, "标签") || !slices.Contains(tags, "unused") {
			fatal("missing expected migrated tags in %v", tags)
		}

		wantPixel, err := os.ReadFile(filepath.Join(repoRoot(), "internal", "v1migration", "testdata", "pixel.png"))
		if err != nil {
			fatal("read pixel.png: %v", err)
		}
		wantFile := []byte("binary\x00fixture")

		type partData struct {
			mime string
			data []byte
		}
		readAllParts := func(id engine.HistoryID) []partData {
			h, err := eng.RetainHistoryContent(ctx, id)
			if err != nil {
				fatal("RetainHistoryContent(%s): %v", id, err)
			}
			defer func() { _ = eng.ReleaseContent(ctx, h.LeaseID) }()
			if len(h.Content.Parts) == 0 {
				fatal("no content parts for %s", id)
			}
			parts := make([]partData, 0, len(h.Content.Parts))
			for _, part := range h.Content.Parts {
				rd, err := eng.ReadContent(ctx, h.LeaseID, part.ID)
				if err != nil {
					fatal("ReadContent(%s/%s): %v", id, part.ID, err)
				}
				data, rerr := io.ReadAll(rd)
				_ = rd.Close()
				if rerr != nil {
					fatal("ReadAll(%s/%s): %v", id, part.ID, rerr)
				}
				parts = append(parts, partData{mime: part.MIME, data: data})
			}
			return parts
		}

		fixtureTextCount := 0
		htmlTextCount := 0
		taggedTextCount := 0
		recoveredCount := 0
		imageCount := 0
		fileCount := 0

		for _, item := range page.Items {
			if item.Origin.Kind != engine.OriginKindMigration {
				fatal("unexpected origin kind on %s: %+v", item.ID, item.Origin)
			}
			parts := readAllParts(item.ID)
			switch item.Kind {
			case engine.HistoryText:
				var plain []byte
				var hasPlain bool
				for _, p := range parts {
					switch p.mime {
					case "text/plain":
						plain = p.data
						hasPlain = true
					case "text/html":
						if string(p.data) != "<b>fixture text</b>" {
							fatal("unexpected migrated html %q on %s", string(p.data), item.ID)
						}
						htmlTextCount++
					default:
						fatal("unexpected text part mime %q on %s", p.mime, item.ID)
					}
				}
				if !hasPlain {
					fatal("missing text/plain part on %s", item.ID)
				}
				switch string(plain) {
				case "fixture text":
					fixtureTextCount++
					if slices.Contains(item.Tags, "标签") {
						taggedTextCount++
					}
				case "recovered":
					recoveredCount++
				default:
					fatal("unexpected migrated text %q on %s", string(plain), item.ID)
				}
			case engine.HistoryImage:
				imageCount++
				if !bytes.Equal(parts[0].data, wantPixel) {
					fatal("image bytes mismatch: got %d bytes, want %d bytes", len(parts[0].data), len(wantPixel))
				}
			case engine.HistoryFiles:
				fileCount++
				if !bytes.Equal(parts[0].data, wantFile) {
					fatal("file bytes mismatch: got %x, want %x", parts[0].data, wantFile)
				}
			default:
				fatal("unexpected migrated item kind %q (%+v)", item.Kind, item)
			}
		}

		wantRecovered := 0
		if expectedCount == 5 {
			wantRecovered = 1
		}
		if fixtureTextCount != 2 || htmlTextCount != 2 || taggedTextCount != 1 || recoveredCount != wantRecovered || imageCount != 1 || fileCount != 1 {
			fatal("item breakdown mismatch: fixtureText=%d html=%d tagged=%d recovered=%d (want %d) image=%d file=%d",
				fixtureTextCount, htmlTextCount, taggedTextCount, recoveredCount, wantRecovered, imageCount, fileCount)
		}
	}

	res, _ := json.Marshal(map[string]any{
		"ok":            true,
		"verifiedCount": len(page.Items),
		"sourceDeleted": deleteSource,
	})
	fmt.Println(string(res))
}

func main() {
	mode := flag.String("mode", "generate", "generate or verify")
	variant := flag.String("variant", "complete", "complete, partial, or empty")
	outDir := flag.String("out", "", "output fixture directory")
	sourceDir := flag.String("source", "", "v1 source directory")
	sourceDB := flag.String("source-db", "", "v1 profile.sqlite path")
	sourceSHA := flag.String("source-sha", "", "expected sha256 of v1 profile.sqlite")
	targetDir := flag.String("target", "", "migrated v2 target data directory")
	expectedCount := flag.Int("expected-count", 0, "expected migrated history record count")
	deleteSource := flag.Bool("delete-source", false, "delete source directory before opening v2 Engine")
	flag.Parse()

	switch *mode {
	case "generate":
		runGenerate(*variant, *outDir)
	case "verify":
		runVerify(*sourceDir, *sourceDB, *sourceSHA, *targetDir, *expectedCount, *deleteSource)
	default:
		fatal("unknown -mode %q", *mode)
	}
}
