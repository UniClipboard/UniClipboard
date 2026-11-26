// Command legacy-mobile-migration verifies offline legacy Expo SQLite migration
// (including WAL journal mode, clipboards/, payloads/, cache/temp_files/,
// partial->repair->idempotent retries, and read-only source preservation)
// across two separate processes through the mobile binding.
package main

import (
	"bytes"
	"crypto/sha256"
	"database/sql"
	"encoding/hex"
	"encoding/json"
	"fmt"
	"image"
	"image/color"
	"image/jpeg"
	"image/png"
	"os"
	"os/exec"
	"path/filepath"
	"slices"
	"time"

	"github.com/uniclipboard/unic/bindings/mobile"
	"github.com/uniclipboard/unic/engine"
	"github.com/uniclipboard/unic/internal/sqldb"
)

const legacySchemaV5 = `
PRAGMA journal_mode = WAL;
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

type migrationReport struct {
	Status       string `json:"status"`
	Total        int    `json:"total"`
	Imported     int    `json:"imported"`
	Skipped      int    `json:"skipped"`
	Failed       int    `json:"failed"`
	MissingParts int    `json:"missing_parts"`
	Verified     int    `json:"verified"`
	Tags         int    `json:"tags"`
}

func must[T any](v T, err error) T {
	check(err)
	return v
}

func check(err error) {
	if err != nil {
		panic(err)
	}
}

func require(cond bool, msg string) {
	if !cond {
		panic(msg)
	}
}

func encode(v any) string {
	return string(must(json.Marshal(v)))
}

func decode[T any](raw string) T {
	var out T
	check(json.Unmarshal([]byte(raw), &out))
	return out
}

func shaHex(b []byte) string {
	s := sha256.Sum256(b)
	return hex.EncodeToString(s[:])
}

func fileSHA(path string) string {
	return shaHex(must(os.ReadFile(path)))
}

func makePNG() []byte {
	img := image.NewRGBA(image.Rect(0, 0, 8, 8))
	for y := 0; y < 8; y++ {
		for x := 0; x < 8; x++ {
			img.Set(x, y, color.RGBA{R: 30, G: 144, B: 255, A: 255})
		}
	}
	var buf bytes.Buffer
	check(png.Encode(&buf, img))
	return buf.Bytes()
}

func makeJPEG() []byte {
	img := image.NewRGBA(image.Rect(0, 0, 8, 8))
	for y := 0; y < 8; y++ {
		for x := 0; x < 8; x++ {
			img.Set(x, y, color.RGBA{R: 255, G: 120, B: 40, A: 255})
		}
	}
	var buf bytes.Buffer
	check(jpeg.Encode(&buf, img, &jpeg.Options{Quality: 90}))
	return buf.Bytes()
}

func stageAndMigrate(root string) {
	targetDir := filepath.Join(root, "unic-v2")
	legacyRoot := filepath.Join(root, "legacy")
	sqlDir := filepath.Join(legacyRoot, "SQLite")
	clipDir := filepath.Join(legacyRoot, "clipboards")
	histDir := filepath.Join(clipDir, "history")
	tempDir := filepath.Join(legacyRoot, "cache", "temp_files")
	check(os.MkdirAll(sqlDir, 0o700))
	check(os.MkdirAll(histDir, 0o700))
	check(os.MkdirAll(tempDir, 0o700))

	pngBytes := makePNG()
	jpgBytes := makeJPEG()
	heicBytes := append([]byte{0x00, 0x00, 0x00, 0x18, 'f', 't', 'y', 'p', 'h', 'e', 'i', 'c'}, []byte("e2e-heic-bytes")...)
	docBytes := []byte("legacy mobile document fixture\n")
	zipBytes := []byte("PK\x03\x04e2e-group-zip-bytes")
	tempBytes := []byte("legacy cache/temp_files attachment bytes\n")
	repairBytes := []byte("repaired missing attachment bytes\n")

	writeHist := func(sub, name string, data []byte) {
		dir := filepath.Join(histDir, sub)
		check(os.MkdirAll(dir, 0o700))
		check(os.WriteFile(filepath.Join(dir, name), data, 0o600))
	}
	writeHist("Image-mob-png-1", "shot.png", pngBytes)
	writeHist("Image-mob-jpg-2", "cam.jpg", jpgBytes)
	writeHist("Image-mob-heic-3", "live.heic", heicBytes)
	writeHist("File-mob-doc-4", "notes.txt", docBytes)
	writeHist("Group-mob-grp-5", "bundle.zip", zipBytes)
	check(os.WriteFile(filepath.Join(tempDir, "clip-temp-6.txt"), tempBytes, 0o600))

	dbPath := filepath.Join(sqlDir, "uniclipboard.db")
	db := must(sql.Open(sqldb.Driver, dbPath))
	_, err := db.Exec(legacySchemaV5)
	check(err)

	now := time.Now().UnixMilli()
	rows := []struct {
		hash, typ, text, name, uri string
		size                       int64
		ts                         int64
		star, pin, del             int
	}{
		{"mob-txt-0", "Text", "Legacy mobile text fixture", "", "", 26, now - 8000, 1, 1, 0},
		{"mob-png-1", "Image", "", "shot.png", "file:///var/mobile/Containers/Data/Application/OLD-UUID/Documents/clipboards/history/Image-mob-png-1/shot.png", int64(len(pngBytes)), now - 7000, 0, 1, 0},
		{"mob-jpg-2", "Image", "", "cam.jpg", "", int64(len(jpgBytes)), now - 6000, 1, 0, 0},
		{"mob-heic-3", "Image", "", "live.heic", "", int64(len(heicBytes)), now - 5000, 0, 0, 0},
		{"mob-doc-4", "File", "", "notes.txt", "", int64(len(docBytes)), now - 4000, 0, 0, 0},
		{"mob-grp-5", "Group", "", "bundle.zip", "", int64(len(zipBytes)), now - 3000, 0, 0, 0},
		{"mob-temp-6", "File", "", "clip-temp-6.txt", "file:///data/user/0/app.uniclipboard.android/cache/temp_files/clip-temp-6.txt", int64(len(tempBytes)), now - 2000, 0, 0, 0},
		{"mob-missing-7", "File", "", "deferred.txt", "", int64(len(repairBytes)), now - 1000, 0, 0, 0},
		{"mob-deleted-8", "Text", "soft-deleted must stay excluded", "", "", 31, now - 500, 1, 1, 1},
	}
	for _, r := range rows {
		_, err := db.Exec(`INSERT INTO clipboard_history
			(profileHash, type, text, dataName, size, fileUri, timestamp, lastAccessed, lastModified, starred, pinned, isDeleted)
			VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)`,
			r.hash, r.typ, r.text, r.name, r.size, r.uri, r.ts, r.ts, r.ts, r.star, r.pin, r.del)
		check(err)
	}
	check(db.Close())
	dbDigestBefore := fileSHA(dbPath)

	sess := must(mobile.OpenSetupFlow(targetDir, filepath.Join(root, "absent-v1")))
	check(sess.ConfigureLegacyMobile(encode(map[string]any{
		"database_candidates": []string{dbPath},
		"attachment_roots":    []string{clipDir, tempDir},
	})))

	det := decode[struct {
		Status         string `json:"status"`
		SourceKind     string `json:"source_kind"`
		RequiresUnlock bool   `json:"requires_unlock"`
	}](must(sess.DetectSource("")))
	require(det.Status == "found" && det.SourceKind == "mobile_sqlite" && !det.RequiresUnlock,
		fmt.Sprintf("unexpected detect result: %+v", det))

	// First run: 7 recoverable, 1 missing -> partial.
	rep1 := decode[migrationReport](must(sess.Run(nil, nil, nil, nil)))
	require(rep1.Status == "partial" && rep1.Total == 8 && rep1.Imported == 7 && rep1.Failed == 1 && rep1.MissingParts == 1 && rep1.Verified == 7,
		fmt.Sprintf("unexpected first run report: %+v", rep1))

	// Restore missing attachment and retry -> complete.
	writeHist("File-mob-missing-7", "deferred.txt", repairBytes)
	rep2 := decode[migrationReport](must(sess.Run(nil, nil, nil, nil)))
	require(rep2.Status == "complete" && rep2.Imported == 1 && rep2.Skipped == 7 && rep2.Failed == 0 && rep2.Verified == 8,
		fmt.Sprintf("unexpected retry report: %+v", rep2))

	// Third run -> idempotent skip.
	rep3 := decode[migrationReport](must(sess.Run(nil, nil, nil, nil)))
	require(rep3.Status == "complete" && rep3.Imported == 0 && rep3.Skipped == 8 && rep3.Verified == 8,
		fmt.Sprintf("unexpected idempotent report: %+v", rep3))

	require(fileSHA(dbPath) == dbDigestBefore, "source uniclipboard.db was modified during read-only migration")
	check(sess.MarkComplete())

	check(os.WriteFile(filepath.Join(root, "expected.json"), []byte(encode(map[string]string{
		"heic_sha256":   shaHex(heicBytes),
		"doc_sha256":    shaHex(docBytes),
		"zip_sha256":    shaHex(zipBytes),
		"temp_sha256":   shaHex(tempBytes),
		"repair_sha256": shaHex(repairBytes),
	})), 0o600))
}

func readAndVerify(root string) {
	targetDir := filepath.Join(root, "unic-v2")
	expected := decode[map[string]string](string(must(os.ReadFile(filepath.Join(root, "expected.json")))))

	sess := must(mobile.OpenSetupFlow(targetDir, filepath.Join(root, "absent-v1")))
	st := decode[struct {
		Complete bool `json:"complete"`
	}](must(sess.GetState()))
	require(st.Complete, "reopened setup state must remain complete")

	m := must(mobile.NewEngine(encode(map[string]string{"data_dir": targetDir})))
	check(m.Start(nil))
	defer func() { _ = m.Close(nil) }()

	page := decode[engine.HistoryPage](must(m.ListHistory(`{"limit":20}`)))
	require(len(page.Items) == 8, fmt.Sprintf("expected 8 migrated items after reopen, got %d", len(page.Items)))

	byKind := map[engine.HistoryKind]int{}
	seenFiles := map[string]string{}
	sawPinnedStarredText := false
	for _, it := range page.Items {
		require(it.Origin.Kind == engine.OriginKindMigration, "unexpected origin: "+string(it.Origin.Kind))
		byKind[it.Kind]++
		if it.Kind == engine.HistoryText {
			preview := decode[struct {
				Text string `json:"text"`
			}](must(m.ReadHistoryTextPreview(string(it.ID), 512)))
			if preview.Text == "Legacy mobile text fixture" {
				require(slices.Equal(it.Tags, []string{"pinned", "starred"}), fmt.Sprintf("unexpected text tags: %v", it.Tags))
				sawPinnedStarredText = true
			}
		}
		if it.Kind == engine.HistoryFiles {
			staged := decode[struct {
				Directory string `json:"directory"`
				Files     []struct {
					Path string `json:"path"`
					Name string `json:"name"`
				} `json:"files"`
			}](must(m.StageHistoryExport(string(it.ID), false)))
			require(len(staged.Files) == 1, fmt.Sprintf("expected 1 file part, got %d", len(staged.Files)))
			seenFiles[staged.Files[0].Name] = fileSHA(staged.Files[0].Path)
			_ = os.RemoveAll(staged.Directory)
		}
	}
	require(sawPinnedStarredText, "missing migrated pinned+starred text record")
	require(byKind[engine.HistoryText] == 1 && byKind[engine.HistoryImage] == 2 && byKind[engine.HistoryFiles] == 5,
		fmt.Sprintf("unexpected kind distribution: %+v", byKind))
	require(seenFiles["live.heic"] == expected["heic_sha256"], "heic sha mismatch")
	require(seenFiles["notes.txt"] == expected["doc_sha256"], "doc sha mismatch")
	require(seenFiles["bundle.zip"] == expected["zip_sha256"], "zip sha mismatch")
	require(seenFiles["clip-temp-6.txt"] == expected["temp_sha256"], "temp_files sha mismatch")
	require(seenFiles["deferred.txt"] == expected["repair_sha256"], "repaired file sha mismatch")

	fmt.Println(encode(map[string]any{
		"status": "passed",
		"checks": []string{
			"legacy_mobile_detect_no_unlock",
			"wal_readonly_preservation",
			"partial_repair_idempotence",
			"cache_temp_files_remapping",
			"second_process_reopen_and_sha256_readback",
		},
		"items": len(page.Items),
	}))
}

func main() {
	if len(os.Args) == 3 && os.Args[1] == "write" {
		stageAndMigrate(os.Args[2])
		return
	}
	if len(os.Args) == 3 && os.Args[1] == "read" {
		readAndVerify(os.Args[2])
		return
	}
	dir := must(os.MkdirTemp("", "unic-legacy-mobile-e2e-"))
	defer func() { _ = os.RemoveAll(dir) }()
	dir = must(filepath.EvalSymlinks(dir))
	bin := filepath.Join(dir, "legacy-mobile-migration")
	build := exec.Command("go", "build", "-o", bin, "./e2e/legacy-mobile-migration")
	build.Stdout = os.Stdout
	build.Stderr = os.Stderr
	check(build.Run())
	for _, stage := range []string{"write", "read"} {
		cmd := exec.Command(bin, stage, dir)
		cmd.Stdout = os.Stdout
		cmd.Stderr = os.Stderr
		check(cmd.Run())
	}
}
