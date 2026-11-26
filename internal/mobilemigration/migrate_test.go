package mobilemigration

import (
	"bytes"
	"context"
	"crypto/sha256"
	"database/sql"
	"image"
	"image/color"
	"image/jpeg"
	"image/png"
	"io"
	"os"
	"path/filepath"
	"runtime"
	"slices"
	"testing"
	"time"

	"github.com/uniclipboard/unic/engine"
	"github.com/uniclipboard/unic/internal/sqldb"
	"github.com/uniclipboard/unic/internal/v1migration"
)

const legacySchemaV5 = `
CREATE TABLE clipboard_history (
  profileHash        TEXT PRIMARY KEY COLLATE NOCASE,
  type               TEXT NOT NULL,
  text               TEXT NOT NULL DEFAULT '',
  displayKind        TEXT,
  dataName           TEXT,
  size               INTEGER,
  fileUri            TEXT,
  hasData            INTEGER NOT NULL DEFAULT 0,
  hasRemoteData      INTEGER NOT NULL DEFAULT 0,
  localClipboardHash TEXT,
  timestamp          INTEGER NOT NULL DEFAULT 0,
  lastAccessed       INTEGER NOT NULL DEFAULT 0,
  lastModified       INTEGER NOT NULL DEFAULT 0,
  useCount           INTEGER NOT NULL DEFAULT 0,
  starred            INTEGER NOT NULL DEFAULT 0,
  pinned             INTEGER NOT NULL DEFAULT 0,
  isDeleted          INTEGER NOT NULL DEFAULT 0,
  isLocalFileReady   INTEGER NOT NULL DEFAULT 0,
  syncStatus         INTEGER NOT NULL DEFAULT 0,
  version            INTEGER NOT NULL DEFAULT 0,
  incarnationId      TEXT,
  "from"             TEXT,
  deviceName         TEXT,
  synced             INTEGER,
  contentId          TEXT,
  p2pEntryId         TEXT,
  p2pDeliveryState   TEXT,
  p2pDeliveryCounts  TEXT
);
PRAGMA user_version = 5;
`

func makePNGBytes(t *testing.T, r, g, b uint8) []byte {
	t.Helper()
	img := image.NewRGBA(image.Rect(0, 0, 4, 4))
	for y := 0; y < 4; y++ {
		for x := 0; x < 4; x++ {
			img.Set(x, y, color.RGBA{R: r, G: g, B: b, A: 255})
		}
	}
	var buf bytes.Buffer
	if err := png.Encode(&buf, img); err != nil {
		t.Fatalf("encode png: %v", err)
	}
	return buf.Bytes()
}

func makeJPEGBytes(t *testing.T) []byte {
	t.Helper()
	img := image.NewRGBA(image.Rect(0, 0, 8, 8))
	for y := 0; y < 8; y++ {
		for x := 0; x < 8; x++ {
			img.Set(x, y, color.RGBA{R: 120, G: 200, B: 80, A: 255})
		}
	}
	var buf bytes.Buffer
	if err := jpeg.Encode(&buf, img, &jpeg.Options{Quality: 90}); err != nil {
		t.Fatalf("encode jpeg: %v", err)
	}
	return buf.Bytes()
}

func fileSHA256(t *testing.T, path string) [32]byte {
	t.Helper()
	raw, err := os.ReadFile(path)
	if err != nil {
		t.Fatalf("read %s: %v", path, err)
	}
	return sha256.Sum256(raw)
}

func createLegacyDB(t *testing.T, dbPath string, seed func(db *sql.DB)) {
	t.Helper()
	if err := os.MkdirAll(filepath.Dir(dbPath), 0o700); err != nil {
		t.Fatalf("mkdir db dir: %v", err)
	}
	db, err := sql.Open(sqldb.Driver, dbPath)
	if err != nil {
		t.Fatalf("open sqlite: %v", err)
	}
	defer func() { _ = db.Close() }()
	if _, err := db.Exec(legacySchemaV5); err != nil {
		t.Fatalf("create schema: %v", err)
	}
	if seed != nil {
		seed(db)
	}
}

func TestIOSAppGroupMigrationCompletePartialRepairAndIdempotence(t *testing.T) {
	ctx := context.Background()
	root := t.TempDir()
	groupDir := filepath.Join(root, "app-group")
	dbPath := filepath.Join(groupDir, "Databases", DatabaseFilename)
	payloadsDir := filepath.Join(groupDir, "payloads")
	targetDir := filepath.Join(root, "unic-v2")
	if err := os.MkdirAll(payloadsDir, 0o700); err != nil {
		t.Fatalf("mkdir payloads: %v", err)
	}

	now := time.Now().UnixMilli()
	pngBytes := makePNGBytes(t, 12, 34, 56)
	jpgBytes := makeJPEGBytes(t)
	heicBytes := append([]byte{0x00, 0x00, 0x00, 0x18, 'f', 't', 'y', 'p', 'h', 'e', 'i', 'c'}, []byte("fake-heic-payload")...)
	zipBytes := []byte("PK\x03\x04fake-zip-archive-bytes")
	docBytes := []byte("hello legacy mobile attachment")

	// iOS App Group payloads are stored as payloads/{Type}-{profileHash} without file extension.
	if err := os.WriteFile(filepath.Join(payloadsDir, "Image-img-png-1"), pngBytes, 0o600); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(payloadsDir, "Image-img-jpg-2"), jpgBytes, 0o600); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(payloadsDir, "Image-img-heic-3"), heicBytes, 0o600); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(payloadsDir, "Group-grp-zip-4"), zipBytes, 0o600); err != nil {
		t.Fatal(err)
	}
	// Deliberately omit File-file-missing-5 on the first run to test partial + repair.

	createLegacyDB(t, dbPath, func(db *sql.DB) {
		rows := []struct {
			hash, typ, text, name, uri string
			size                       int64
			ts, mod                    int64
			star, pin, del             int
		}{
			{"Text-Hash-1", "Text", "https://uniclipboard.app/docs", "", "", 29, now - 5000, now - 4000, 1, 1, 0},
			{"img-png-1", "Image", "", "shot.png", "file:///var/mobile/Containers/Shared/AppGroup/OLD-UUID/payloads/Image-img-png-1", int64(len(pngBytes)), now - 4000, now - 4000, 0, 1, 0},
			{"img-jpg-2", "Image", "", "camera.jpg", "", int64(len(jpgBytes)), now - 3000, now - 3000, 1, 0, 0},
			{"img-heic-3", "Image", "", "photo.heic", "", int64(len(heicBytes)), now - 2500, now - 2500, 0, 0, 0},
			{"grp-zip-4", "Group", "", "bundle.zip", "", int64(len(zipBytes)), now - 2000, now - 2000, 0, 0, 0},
			{"file-missing-5", "File", "", "report.txt", "", int64(len(docBytes)), now - 1000, now - 1000, 0, 0, 0},
			{"deleted-6", "Text", "must not be imported", "", "", 20, now - 500, now - 500, 1, 1, 1},
		}
		for _, r := range rows {
			_, err := db.Exec(`INSERT INTO clipboard_history
				(profileHash, type, text, dataName, size, fileUri, timestamp, lastAccessed, lastModified, starred, pinned, isDeleted)
				VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)`,
				r.hash, r.typ, r.text, r.name, r.size, r.uri, r.ts, r.mod, r.mod, r.star, r.pin, r.del)
			if err != nil {
				t.Fatalf("insert %s: %v", r.hash, err)
			}
		}
	})

	dbDigestBefore := fileSHA256(t, dbPath)

	// 1. Inspect via App Group root directory.
	insp, err := Inspect(ctx, groupDir)
	if err != nil {
		t.Fatalf("Inspect: %v", err)
	}
	if insp.ActiveCount != 6 {
		t.Fatalf("ActiveCount = %d, want 6", insp.ActiveCount)
	}

	// 2. First run: 5 succeed, 1 missing attachment -> partial.
	src, err := Open(ctx, groupDir, OpenOptions{AttachmentRoots: []string{payloadsDir}})
	if err != nil {
		t.Fatalf("Open: %v", err)
	}
	var stages []string
	report, err := src.Run(ctx, v1migration.RunOptions{
		Target: targetDir,
		Progress: func(p v1migration.Progress) {
			if len(stages) == 0 || stages[len(stages)-1] != p.Stage {
				stages = append(stages, p.Stage)
			}
		},
	})
	_ = src.Close()
	if err != nil {
		t.Fatalf("first Run: %v", err)
	}
	if report.Status != "partial" || report.Total != 6 || report.Imported != 5 || report.Failed != 1 || report.MissingParts != 1 || report.Verified != 5 {
		t.Fatalf("unexpected partial report: %+v", report)
	}
	if !slices.Equal(stages, []string{"import", "verify"}) {
		t.Fatalf("stages = %v, want [import verify]", stages)
	}

	// 3. Restore missing file and re-run -> idempotent complete.
	if err := os.WriteFile(filepath.Join(payloadsDir, "File-file-missing-5"), docBytes, 0o600); err != nil {
		t.Fatal(err)
	}
	src2, err := Open(ctx, dbPath, OpenOptions{AttachmentRoots: []string{payloadsDir}})
	if err != nil {
		t.Fatalf("Open retry: %v", err)
	}
	report2, err := src2.Run(ctx, v1migration.RunOptions{Target: targetDir})
	_ = src2.Close()
	if err != nil {
		t.Fatalf("retry Run: %v", err)
	}
	if report2.Status != "complete" || report2.Imported != 1 || report2.Skipped != 5 || report2.Failed != 0 || report2.Verified != 6 {
		t.Fatalf("unexpected retry report: %+v", report2)
	}

	// 4. Third run -> completely skipped and verified.
	src3, err := Open(ctx, dbPath, OpenOptions{AttachmentRoots: []string{payloadsDir}})
	if err != nil {
		t.Fatalf("Open idempotent: %v", err)
	}
	report3, err := src3.Run(ctx, v1migration.RunOptions{Target: targetDir})
	_ = src3.Close()
	if err != nil {
		t.Fatalf("idempotent Run: %v", err)
	}
	if report3.Status != "complete" || report3.Imported != 0 || report3.Skipped != 6 || report3.Verified != 6 {
		t.Fatalf("unexpected idempotent report: %+v", report3)
	}

	// Source DB remained completely unmodified (read-only guarantee).
	if got := fileSHA256(t, dbPath); got != dbDigestBefore {
		t.Fatalf("source SQLite DB was modified during migration")
	}

	// 5. Reopen Engine in normal mode and verify all 6 migrated records.
	eng, err := engine.New(ctx, engine.Options{DataDir: targetDir})
	if err != nil {
		t.Fatalf("engine.New: %v", err)
	}
	defer func() { _ = eng.Close(ctx) }()
	page, err := eng.ListHistory(ctx, engine.ListHistoryRequest{Limit: 20})
	if err != nil {
		t.Fatalf("ListHistory: %v", err)
	}
	if len(page.Items) != 6 {
		t.Fatalf("ListHistory len = %d, want 6", len(page.Items))
	}

	// Verify text record has pinned+starred tags and exact body.
	textID, known, err := eng.LookupHistoryImport(ctx, SourceID, "text-hash-1")
	if err != nil || !known {
		t.Fatalf("lookup text-hash-1: known=%v err=%v", known, err)
	}
	textItem, err := eng.GetItem(ctx, textID)
	if err != nil {
		t.Fatalf("GetItem text: %v", err)
	}
	if !slices.Equal(textItem.Tags, []string{"pinned", "starred"}) {
		t.Fatalf("text tags = %v, want [pinned starred]", textItem.Tags)
	}
	if textItem.Origin.Kind != engine.OriginKindMigration {
		t.Fatalf("origin = %q, want migration", textItem.Origin.Kind)
	}

	// Verify JPEG was transcoded into image/png representation.
	jpgID, known, err := eng.LookupHistoryImport(ctx, SourceID, "img-jpg-2")
	if err != nil || !known {
		t.Fatalf("lookup img-jpg-2: known=%v err=%v", known, err)
	}
	jpgItem, err := eng.GetItem(ctx, jpgID)
	if err != nil {
		t.Fatalf("GetItem jpg: %v", err)
	}
	if jpgItem.Kind != engine.HistoryImage {
		t.Fatalf("jpg kind = %q, want image", jpgItem.Kind)
	}

	// Verify HEIC was preserved losslessly as a file part with exact bytes.
	heicID, known, err := eng.LookupHistoryImport(ctx, SourceID, "img-heic-3")
	if err != nil || !known {
		t.Fatalf("lookup img-heic-3: known=%v err=%v", known, err)
	}
	h, err := eng.RetainHistoryContent(ctx, heicID)
	if err != nil {
		t.Fatalf("RetainHistoryContent heic: %v", err)
	}
	defer func() { _ = eng.ReleaseContent(ctx, h.LeaseID) }()
	if len(h.Content.Parts) != 1 || h.Content.Parts[0].Name != "photo.heic" {
		t.Fatalf("unexpected heic parts: %+v", h.Content.Parts)
	}
	rc, err := eng.ReadContent(ctx, h.LeaseID, h.Content.Parts[0].ID)
	if err != nil {
		t.Fatalf("ReadContent heic: %v", err)
	}
	gotHeic, _ := io.ReadAll(rc)
	_ = rc.Close()
	if !bytes.Equal(gotHeic, heicBytes) {
		t.Fatalf("heic bytes mismatch")
	}
}

func TestAndroidLayoutRemappingAndTraversalProtection(t *testing.T) {
	ctx := context.Background()
	root := t.TempDir()
	filesDir := filepath.Join(root, "android-files")
	dbPath := filepath.Join(filesDir, "SQLite", DatabaseFilename)
	clipboardsDir := filepath.Join(filesDir, "clipboards")
	targetDir := filepath.Join(root, "unic-v2")

	// Create an outside secret file that must NEVER be readable via fileUri.
	outsideSecret := filepath.Join(root, "outside-secret.txt")
	if err := os.WriteFile(outsideSecret, []byte("top-secret"), 0o600); err != nil {
		t.Fatal(err)
	}

	// Android clipboards/history/{Type}-{profileHash}/{dataName}
	histFileDir := filepath.Join(clipboardsDir, "history", "File-android-doc-1")
	if err := os.MkdirAll(histFileDir, 0o700); err != nil {
		t.Fatal(err)
	}
	docBytes := []byte("android pdf bytes")
	if err := os.WriteFile(filepath.Join(histFileDir, "spec.pdf"), docBytes, 0o600); err != nil {
		t.Fatal(err)
	}

	// Legacy Android clipboards/images/{hash}.png
	imgDir := filepath.Join(clipboardsDir, "images")
	if err := os.MkdirAll(imgDir, 0o700); err != nil {
		t.Fatal(err)
	}
	pngBytes := makePNGBytes(t, 200, 100, 50)
	if err := os.WriteFile(filepath.Join(imgDir, "android-img-2.png"), pngBytes, 0o600); err != nil {
		t.Fatal(err)
	}

	// Legacy UniClip cache/temp_files/<name> referenced by fileUri
	tempFilesDir := filepath.Join(root, "android-cache", "temp_files")
	if err := os.MkdirAll(tempFilesDir, 0o700); err != nil {
		t.Fatal(err)
	}
	tempVideoBytes := []byte("legacy-temp-video-bytes")
	if err := os.WriteFile(filepath.Join(tempFilesDir, "clip-temp-4.mp4"), tempVideoBytes, 0o600); err != nil {
		t.Fatal(err)
	}

	now := time.Now().UnixMilli()
	createLegacyDB(t, dbPath, func(db *sql.DB) {
		_, err := db.Exec(`INSERT INTO clipboard_history (profileHash, type, text, dataName, size, fileUri, timestamp, isDeleted) VALUES
			('android-doc-1', 'File', '', 'spec.pdf', ?, 'file:///data/user/0/old/files/clipboards/history/File-android-doc-1/spec.pdf', ?, 0),
			('android-img-2', 'Image', '', 'pic.png', ?, '', ?, 0),
			('android-temp-4', 'File', '', 'video.mp4', ?, 'file:///data/user/0/app.uniclipboard.android.dev/cache/temp_files/clip-temp-4.mp4', ?, 0),
			('escape-3', 'File', '', 'secret.txt', 10, ?, ?, 0)`,
			len(docBytes), now-2000, len(pngBytes), now-1000, len(tempVideoBytes), now-800, "file://"+outsideSecret, now-500)
		if err != nil {
			t.Fatalf("insert: %v", err)
		}
	})

	src, err := Open(ctx, dbPath, OpenOptions{AttachmentRoots: []string{clipboardsDir, tempFilesDir}})
	if err != nil {
		t.Fatalf("Open: %v", err)
	}
	defer func() { _ = src.Close() }()

	report, err := src.Run(ctx, v1migration.RunOptions{Target: targetDir})
	if err != nil {
		t.Fatalf("Run: %v", err)
	}
	// escape-3 MUST be rejected as missing attachment (never read outside clipboardsDir/tempFilesDir).
	if report.Status != "partial" || report.Imported != 3 || report.Failed != 1 || report.MissingParts != 1 || report.Verified != 3 {
		t.Fatalf("unexpected android report: %+v", report)
	}
}

func TestEmptyDatabaseAndCancellationResume(t *testing.T) {
	ctx := context.Background()
	root := t.TempDir()

	// Empty DB (only deleted row) reports ActiveCount == 0.
	emptyDB := filepath.Join(root, "empty", DatabaseFilename)
	createLegacyDB(t, emptyDB, func(db *sql.DB) {
		_, _ = db.Exec(`INSERT INTO clipboard_history (profileHash, type, text, timestamp, isDeleted) VALUES ('del-1', 'Text', 'gone', 1000, 1)`)
	})
	insp, err := Inspect(ctx, emptyDB)
	if err != nil {
		t.Fatalf("Inspect empty: %v", err)
	}
	if insp.ActiveCount != 0 {
		t.Fatalf("empty ActiveCount = %d, want 0", insp.ActiveCount)
	}

	// Cancellation mid-run preserves committed rows and resumes cleanly.
	cancelDB := filepath.Join(root, "cancel", DatabaseFilename)
	targetDir := filepath.Join(root, "target")
	now := time.Now().UnixMilli()
	createLegacyDB(t, cancelDB, func(db *sql.DB) {
		for i := 0; i < 6; i++ {
			_, err := db.Exec(`INSERT INTO clipboard_history (profileHash, type, text, timestamp, isDeleted) VALUES (?, 'Text', ?, ?, 0)`,
				"rec-"+string(rune('a'+i)), "item text "+string(rune('a'+i)), now-int64((10-i)*1000))
			if err != nil {
				t.Fatal(err)
			}
		}
	})

	runCtx, cancel := context.WithCancel(ctx)
	src, err := Open(ctx, cancelDB, OpenOptions{})
	if err != nil {
		t.Fatal(err)
	}
	cancelReport, cancelErr := src.Run(runCtx, v1migration.RunOptions{
		Target: targetDir,
		Progress: func(p v1migration.Progress) {
			if p.Stage == "import" && p.Processed == 3 {
				cancel()
			}
		},
	})
	_ = src.Close()
	if cancelErr == nil || cancelReport.Status != "cancelled" {
		t.Fatalf("expected cancelled run, got status=%q err=%v", cancelReport.Status, cancelErr)
	}

	// Resume with uncancelled context completes and verifies all 6 rows.
	src2, err := Open(ctx, cancelDB, OpenOptions{})
	if err != nil {
		t.Fatal(err)
	}
	defer func() { _ = src2.Close() }()
	finalReport, err := src2.Run(ctx, v1migration.RunOptions{Target: targetDir})
	if err != nil {
		t.Fatalf("resume Run: %v", err)
	}
	if finalReport.Status != "complete" || finalReport.Imported+finalReport.Skipped != 6 || finalReport.Verified != 6 {
		t.Fatalf("unexpected resume report: %+v", finalReport)
	}
}

func TestReadOnlyDirectoryWALFallback(t *testing.T) {
	if runtime.GOOS == "windows" || os.Geteuid() == 0 {
		t.Skip("POSIX non-root directory permission test")
	}
	ctx := context.Background()
	root := t.TempDir()
	dbDir := filepath.Join(root, "ro-wal")
	dbPath := filepath.Join(dbDir, DatabaseFilename)
	now := time.Now().UnixMilli()

	createLegacyDB(t, dbPath, func(db *sql.DB) {
		if _, err := db.Exec(`PRAGMA journal_mode=WAL`); err != nil {
			t.Fatal(err)
		}
		if _, err := db.Exec(
			`INSERT INTO clipboard_history (profileHash, type, text, timestamp, isDeleted) VALUES ('ro-wal-1', 'Text', 'WAL read-only fallback', ?, 0)`,
			now,
		); err != nil {
			t.Fatal(err)
		}
	})
	// Cleanly closed WAL DB has no -wal sidecar; make its parent directory read-only so SQLite cannot create -shm.
	if err := os.Chmod(dbDir, 0o555); err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = os.Chmod(dbDir, 0o755) })

	insp, err := Inspect(ctx, dbPath)
	if err != nil {
		t.Fatalf("Inspect read-only WAL dir: %v", err)
	}
	if insp.ActiveCount != 1 {
		t.Fatalf("ActiveCount = %d, want 1", insp.ActiveCount)
	}

	src, err := Open(ctx, dbPath, OpenOptions{})
	if err != nil {
		t.Fatalf("Open read-only WAL dir: %v", err)
	}
	defer func() { _ = src.Close() }()
	targetDir := filepath.Join(root, "target")
	report, err := src.Run(ctx, v1migration.RunOptions{Target: targetDir})
	if err != nil {
		t.Fatalf("Run read-only WAL dir: %v", err)
	}
	if report.Status != "complete" || report.Imported != 1 || report.Verified != 1 {
		t.Fatalf("unexpected report: %+v", report)
	}
}
