package mobilemigration

import (
	"bytes"
	"context"
	"crypto/sha256"
	"database/sql"
	"errors"
	"image"
	_ "image/gif"
	_ "image/jpeg"
	"image/png"
	"io"
	"mime"
	"net/http"
	"net/url"
	"os"
	"path/filepath"
	"runtime"
	"slices"
	"strings"
	"unicode/utf8"

	_ "golang.org/x/image/tiff"
	_ "golang.org/x/image/webp"

	"github.com/uniclipboard/unic/engine"
	"github.com/uniclipboard/unic/internal/sqldb"
	"github.com/uniclipboard/unic/internal/v1migration"
	"github.com/uniclipboard/unic/internal/wire"
)

const (
	// DatabaseFilename is the Expo SQLite filename used by legacy UniClip mobile releases (v1.3–v2.4).
	DatabaseFilename = "uniclipboard.db"

	// SourceID is the stable opaque import source identifier for in-place legacy mobile SQLite migration.
	// Using a fixed namespace keyed by each record's lowercased profileHash keeps retries idempotent
	// even if the iOS sandbox container UUID or staged backup directory path changes.
	SourceID = "mobile-expo-sqlite:v1"

	maxPayloadBytes = 500 << 20
	maxImagePixels  = 32 << 20
	walkPageSize    = 128
)

var (
	ErrFormat  = v1migration.ErrFormat
	ErrContent = v1migration.ErrContent
)

// Inspection describes a probed legacy mobile Expo SQLite source.
type Inspection struct {
	DatabasePath string
	ActiveCount  int
}

// OpenOptions configures read-only access to a legacy mobile Expo SQLite database.
type OpenOptions struct {
	// AttachmentRoots lists directories that may contain legacy mobile media/file payloads,
	// such as the iOS App Group payloads/ directory or Documents/clipboards/ (iOS/Android).
	AttachmentRoots []string
}

// Source holds a consistent read-only SQLite snapshot of a legacy mobile clipboard_history database.
type Source struct {
	dbPath          string
	sourceDir       string
	attachmentRoots []string
	db              *sql.DB
	tx              *sql.Tx
	hasLocalHash    bool
}

type legacyRow struct {
	profileHash  string
	itemType     string
	text         string
	dataName     string
	fileUri      string
	localHash    string
	size         int64
	sizeValid    bool
	timestamp    int64
	lastAccessed int64
	lastModified int64
	starred      bool
	pinned       bool
}

type recordPart struct {
	MIME string
	Name string // empty for in-memory/representation parts, non-empty for file parts
	Data []byte
	Path string
	Size int64
}

type record struct {
	ID           string
	CreatedAtMs  int64
	UpdatedAtMs  int64
	Tags         []string
	Parts        []recordPart
	MissingParts int
}

func (r *record) close() {
	for _, p := range r.Parts {
		clear(p.Data)
	}
	r.Parts = nil
}

// ResolveDatabasePath resolves path (either a uniclipboard.db file or a directory containing it
// directly, under Databases/, or under SQLite/) to a canonical file path.
func ResolveDatabasePath(path string) (string, error) {
	trimmed := strings.TrimSpace(path)
	if trimmed == "" {
		return "", ErrFormat
	}
	abs, err := filepath.Abs(trimmed)
	if err != nil {
		return "", ErrFormat
	}
	if resolved, rerr := filepath.EvalSymlinks(abs); rerr == nil {
		abs = resolved
	}
	st, err := os.Stat(abs)
	if err != nil {
		if os.IsNotExist(err) {
			return "", os.ErrNotExist
		}
		return "", err
	}
	if st.Mode().IsRegular() {
		if filepath.Base(abs) != DatabaseFilename {
			return "", ErrFormat
		}
		return abs, nil
	}
	if !st.IsDir() {
		return "", ErrFormat
	}
	candidates := []string{
		filepath.Join(abs, DatabaseFilename),
		filepath.Join(abs, "Databases", DatabaseFilename),
		filepath.Join(abs, "SQLite", DatabaseFilename),
	}
	for _, cand := range candidates {
		fst, ferr := os.Stat(cand)
		if ferr == nil && fst.Mode().IsRegular() {
			if resolved, rerr := filepath.EvalSymlinks(cand); rerr == nil {
				return resolved, nil
			}
			return cand, nil
		}
	}
	return "", ErrFormat
}

// Inspect validates a legacy mobile Expo SQLite database read-only and counts active (non-deleted) rows.
func Inspect(ctx context.Context, path string) (Inspection, error) {
	dbPath, err := ResolveDatabasePath(path)
	if err != nil {
		return Inspection{}, err
	}
	db, tx, _, err := openReadOnlySnapshot(ctx, dbPath)
	if err != nil {
		return Inspection{}, err
	}
	defer func() {
		_ = tx.Rollback()
		_ = db.Close()
	}()
	var count int
	if err := tx.QueryRowContext(ctx, `SELECT count(*) FROM clipboard_history WHERE IFNULL(isDeleted, 0) = 0`).Scan(&count); err != nil {
		if ctx.Err() != nil {
			return Inspection{}, ctx.Err()
		}
		return Inspection{}, ErrFormat
	}
	return Inspection{DatabasePath: dbPath, ActiveCount: count}, nil
}

// Open opens a legacy mobile Expo SQLite database in read-only snapshot mode without modifying any source files.
func Open(ctx context.Context, path string, opts OpenOptions) (*Source, error) {
	if err := ctx.Err(); err != nil {
		return nil, err
	}
	dbPath, err := ResolveDatabasePath(path)
	if err != nil {
		return nil, err
	}
	db, tx, hasLocalHash, err := openReadOnlySnapshot(ctx, dbPath)
	if err != nil {
		return nil, err
	}
	roots := collectAttachmentRoots(dbPath, opts.AttachmentRoots)
	return &Source{
		dbPath:          dbPath,
		sourceDir:       filepath.Dir(dbPath),
		attachmentRoots: roots,
		db:              db,
		tx:              tx,
		hasLocalHash:    hasLocalHash,
	}, nil
}

func (s *Source) Close() error {
	if s == nil {
		return nil
	}
	_ = s.tx.Rollback()
	return s.db.Close()
}

func (s *Source) ID() string {
	return SourceID
}

func (s *Source) Count() (int, error) {
	var n int
	if s.tx.QueryRow(`SELECT count(*) FROM clipboard_history WHERE IFNULL(isDeleted, 0) = 0`).Scan(&n) != nil {
		return 0, ErrFormat
	}
	return n, nil
}

func openReadOnlySnapshot(ctx context.Context, dbPath string) (*sql.DB, *sql.Tx, bool, error) {
	databasePath := filepath.ToSlash(dbPath)
	if runtime.GOOS == "windows" && !strings.HasPrefix(databasePath, "/") {
		databasePath = "/" + databasePath
	}
	u := url.URL{Scheme: "file", Path: databasePath}
	dsn := u.String() + sqldb.MigrationOptions

	db, tx, hasLocalHash, err := tryOpenReadOnlySnapshot(ctx, dsn)
	if err == nil {
		return db, tx, hasLocalHash, nil
	}
	if ctx.Err() != nil {
		return nil, nil, false, ctx.Err()
	}
	// A cleanly closed WAL-mode database in a read-only directory (or on a filesystem where SQLite
	// cannot create/mmap -shm, such as Android external storage FUSE) fails unless opened with
	// immutable=1. Only use immutable=1 when no non-empty -wal file exists alongside the database,
	// so live uncheckpointed WAL frames are never silently skipped.
	if !hasNonEmptyWAL(dbPath) {
		if db2, tx2, hasLocalHash2, err2 := tryOpenReadOnlySnapshot(ctx, dsn+"&immutable=1"); err2 == nil {
			return db2, tx2, hasLocalHash2, nil
		}
	}
	return nil, nil, false, err
}

func hasNonEmptyWAL(dbPath string) bool {
	st, err := os.Stat(dbPath + "-wal")
	if errors.Is(err, os.ErrNotExist) {
		return false
	}
	if err != nil {
		return true
	}
	return st.Size() > 0
}

func tryOpenReadOnlySnapshot(ctx context.Context, dsn string) (*sql.DB, *sql.Tx, bool, error) {
	db, err := sql.Open(sqldb.Driver, dsn)
	if err != nil {
		return nil, nil, false, ErrFormat
	}
	db.SetMaxOpenConns(1)
	tx, err := db.BeginTx(ctx, &sql.TxOptions{ReadOnly: true})
	if err != nil {
		_ = db.Close()
		if ctx.Err() != nil {
			return nil, nil, false, ctx.Err()
		}
		return nil, nil, false, ErrFormat
	}
	var tableExists int
	if err := tx.QueryRowContext(ctx, `SELECT count(*) FROM sqlite_master WHERE type='table' AND name='clipboard_history'`).Scan(&tableExists); err != nil || tableExists == 0 {
		_ = tx.Rollback()
		_ = db.Close()
		if ctx.Err() != nil {
			return nil, nil, false, ctx.Err()
		}
		return nil, nil, false, ErrFormat
	}
	cols, err := tableColumns(ctx, tx, "clipboard_history")
	if err != nil {
		_ = tx.Rollback()
		_ = db.Close()
		return nil, nil, false, err
	}
	required := []string{"profileHash", "type", "text", "timestamp", "isDeleted"}
	for _, col := range required {
		if !cols[strings.ToLower(col)] {
			_ = tx.Rollback()
			_ = db.Close()
			return nil, nil, false, ErrFormat
		}
	}
	return db, tx, cols["localclipboardhash"], nil
}

func tableColumns(ctx context.Context, tx *sql.Tx, table string) (map[string]bool, error) {
	rows, err := tx.QueryContext(ctx, "PRAGMA table_info("+table+")")
	if err != nil {
		if ctx.Err() != nil {
			return nil, ctx.Err()
		}
		return nil, ErrFormat
	}
	defer func() { _ = rows.Close() }()
	cols := map[string]bool{}
	for rows.Next() {
		var cid int
		var name, colType string
		var notNull, pk int
		var dflt sql.NullString
		if rows.Scan(&cid, &name, &colType, &notNull, &dflt, &pk) != nil {
			return nil, ErrFormat
		}
		cols[strings.ToLower(name)] = true
	}
	if rows.Err() != nil {
		return nil, ErrFormat
	}
	return cols, nil
}

func collectAttachmentRoots(dbPath string, explicit []string) []string {
	seen := map[string]bool{}
	var roots []string
	add := func(dir string) {
		trimmed := strings.TrimSpace(dir)
		if trimmed == "" {
			return
		}
		abs, err := filepath.Abs(trimmed)
		if err != nil {
			return
		}
		if resolved, rerr := filepath.EvalSymlinks(abs); rerr == nil {
			abs = resolved
		}
		st, err := os.Stat(abs)
		if err != nil || !st.IsDir() {
			return
		}
		if !seen[abs] {
			seen[abs] = true
			roots = append(roots, abs)
		}
	}
	for _, dir := range explicit {
		add(dir)
	}
	dbDir := filepath.Dir(dbPath)
	parent := filepath.Dir(dbDir)
	// Sibling payload / clipboard folders for App Group containers, Android filesDir, or manual backups.
	add(filepath.Join(parent, "payloads"))
	add(filepath.Join(parent, "clipboards"))
	add(filepath.Join(dbDir, "payloads"))
	add(filepath.Join(dbDir, "clipboards"))
	add(dbDir)
	return roots
}

// Run imports all active legacy mobile clipboard records into targetDir and verifies every imported
// record after reopening the target Engine.
func (s *Source) Run(ctx context.Context, opts v1migration.RunOptions) (report v1migration.Report, runErr error) {
	report.Status = "failed"
	report.Total, runErr = s.Count()
	if runErr != nil {
		return
	}
	notify := func(stage string, n int) {
		if opts.Progress != nil {
			opts.Progress(v1migration.Progress{Stage: stage, Processed: n, Total: report.Total})
		}
	}

	var target *engine.Engine
	var dst string
	if !opts.DryRun {
		if runErr = v1migration.ValidateDirectories(s.sourceDir, opts.Target); runErr != nil {
			return
		}
		dst, runErr = canonicalDir(opts.Target)
		if runErr != nil {
			return
		}
		target, runErr = engine.New(ctx, engine.Options{DataDir: dst, MigrationMode: true})
		if runErr != nil {
			return
		}
		defer func() {
			if target != nil {
				_ = target.Close(context.Background())
			}
		}()
	}

	tagSet := map[string]bool{}
	processed := 0
	runErr = s.walk(ctx, func(row legacyRow) error {
		processed++
		notify("import", processed)
		rec, recErr := s.buildRecord(row)
		defer rec.close()
		for _, tag := range rec.Tags {
			tagSet[tag] = true
		}
		report.MissingParts += rec.MissingParts
		if recErr != nil {
			report.Failed++
			return nil
		}
		if opts.DryRun {
			for _, part := range rec.Parts {
				r, err := part.open()
				if err != nil {
					report.Failed++
					return nil
				}
				_, err = io.Copy(io.Discard, &ctxReader{ctx, r})
				_ = r.Close()
				if err != nil {
					if ctx.Err() != nil {
						return ctx.Err()
					}
					report.Failed++
					return nil
				}
			}
			report.Recoverable++
			return nil
		}

		id, known, err := target.LookupHistoryImport(ctx, s.ID(), rec.ID)
		if err != nil {
			return err
		}
		if known {
			report.Skipped++
			if err := repairMissingTags(ctx, target, id, rec.Tags, &report); err != nil {
				return err
			}
			return nil
		}

		req, closers, err := rec.importRequest(s.ID())
		if err != nil {
			closeAll(closers)
			report.Failed++
			return nil
		}
		res, err := target.ImportHistory(ctx, req)
		closeAll(closers)
		if err != nil {
			if ctx.Err() != nil {
				return ctx.Err()
			}
			var ee *engine.Error
			if errors.As(err, &ee) && (ee.Code == engine.CodeUnavailable || ee.Code == engine.CodeResourceExhausted) {
				return err
			}
			report.Failed++
			return nil
		}
		if res.AlreadyImported {
			report.Skipped++
		} else {
			report.Imported++
		}
		return nil
	})
	if runErr != nil {
		if ctx.Err() != nil {
			report.Status = "cancelled"
		}
		return
	}
	report.Tags = len(tagSet)

	if target != nil {
		runErr = target.Close(ctx)
		target = nil
		if runErr != nil {
			return
		}
		target, runErr = engine.New(ctx, engine.Options{DataDir: dst, MigrationMode: true})
		if runErr != nil {
			return
		}
		processed = 0
		runErr = s.walk(ctx, func(row legacyRow) error {
			processed++
			notify("verify", processed)
			recID := normalizeRecordID(row.profileHash)
			id, known, err := target.LookupHistoryImport(ctx, s.ID(), recID)
			if err != nil {
				return err
			}
			if !known {
				return nil
			}
			rec, recErr := s.buildRecord(row)
			defer rec.close()
			if recErr != nil {
				report.VerificationFailed++
				return nil
			}
			item, itemErr := target.GetItem(ctx, id)
			if itemErr == nil && !slices.Equal(item.Tags, rec.Tags) {
				report.TagMismatches++
			}
			if err := verifyRecord(ctx, target, id, &rec); err != nil {
				if ctx.Err() != nil {
					return ctx.Err()
				}
				report.VerificationFailed++
			} else {
				report.Verified++
			}
			return nil
		})
		if runErr != nil {
			if ctx.Err() != nil {
				report.Status = "cancelled"
			}
			return
		}
	}

	report.Status = "complete"
	if report.Failed > 0 || report.MissingParts > 0 || report.VerificationFailed > 0 {
		report.Status = "partial"
	}
	if opts.DryRun && report.Status == "complete" {
		report.Status = "ready"
	}
	return
}

func (s *Source) walk(ctx context.Context, fn func(legacyRow) error) error {
	localHashExpr := "NULL"
	if s.hasLocalHash {
		localHashExpr = "localClipboardHash"
	}
	query := `SELECT profileHash, type, IFNULL(text, ''), dataName, size, fileUri, ` + localHashExpr + `,
IFNULL(timestamp, 0), IFNULL(lastAccessed, 0), IFNULL(lastModified, 0), IFNULL(starred, 0), IFNULL(pinned, 0)
FROM clipboard_history
WHERE IFNULL(isDeleted, 0) = 0
ORDER BY IFNULL(timestamp, 0) ASC, profileHash ASC
LIMIT ? OFFSET ?`

	for offset := 0; ; offset += walkPageSize {
		if err := ctx.Err(); err != nil {
			return err
		}
		rows, err := s.tx.QueryContext(ctx, query, walkPageSize, offset)
		if err != nil {
			if ctx.Err() != nil {
				return ctx.Err()
			}
			return ErrFormat
		}
		var page []legacyRow
		for rows.Next() {
			var r legacyRow
			var dataName, fileUri, localHash sql.NullString
			var size sql.NullInt64
			var starred, pinned int
			if err := rows.Scan(
				&r.profileHash, &r.itemType, &r.text, &dataName, &size, &fileUri, &localHash,
				&r.timestamp, &r.lastAccessed, &r.lastModified, &starred, &pinned,
			); err != nil {
				_ = rows.Close()
				return ErrFormat
			}
			r.dataName = dataName.String
			r.fileUri = fileUri.String
			r.localHash = localHash.String
			r.size = size.Int64
			r.sizeValid = size.Valid
			r.starred = starred != 0
			r.pinned = pinned != 0
			page = append(page, r)
		}
		rowErr := rows.Err()
		_ = rows.Close()
		if rowErr != nil {
			return ErrFormat
		}
		if len(page) == 0 {
			return nil
		}
		for _, r := range page {
			if err := ctx.Err(); err != nil {
				return err
			}
			if err := fn(r); err != nil {
				return err
			}
		}
		if len(page) < walkPageSize {
			return nil
		}
	}
}

func normalizeRecordID(profileHash string) string {
	trimmed := strings.ToLower(strings.TrimSpace(profileHash))
	if trimmed == "" {
		return ""
	}
	if len(trimmed) <= 256 {
		return trimmed
	}
	sum := sha256.Sum256([]byte(trimmed))
	return "sha256:" + fmtHex(sum[:])
}

func fmtHex(b []byte) string {
	const digits = "0123456789abcdef"
	out := make([]byte, len(b)*2)
	for i, v := range b {
		out[i*2] = digits[v>>4]
		out[i*2+1] = digits[v&0x0f]
	}
	return string(out)
}

func (s *Source) buildRecord(row legacyRow) (record, error) {
	created := row.timestamp
	if created <= 0 {
		created = max(row.lastModified, row.lastAccessed)
	}
	if created < 0 {
		created = 0
	}
	updated := max(created, max(row.lastModified, row.lastAccessed))
	var tags []string
	if row.pinned {
		tags = append(tags, "pinned")
	}
	if row.starred {
		tags = append(tags, "starred")
	}
	slices.Sort(tags)

	rec := record{
		ID:          normalizeRecordID(row.profileHash),
		CreatedAtMs: created,
		UpdatedAtMs: updated,
		Tags:        tags,
	}
	if rec.ID == "" {
		return rec, ErrFormat
	}

	kind := strings.ToLower(strings.TrimSpace(row.itemType))
	switch kind {
	case "text":
		text := row.text
		if text == "" || !utf8.ValidString(text) || int64(len(text)) > wire.MaxPartBytes {
			rec.MissingParts++
			return rec, ErrContent
		}
		data := []byte(text)
		rec.Parts = []recordPart{{
			MIME: engine.MimeText,
			Data: data,
			Size: int64(len(data)),
		}}
		return rec, nil

	case "image":
		path, st, ok := s.resolveAttachment(row)
		if !ok {
			rec.MissingParts++
			return rec, ErrContent
		}
		part, err := buildImagePart(path, st, row.dataName)
		if err != nil {
			rec.MissingParts++
			return rec, err
		}
		rec.Parts = []recordPart{part}
		return rec, nil

	case "file", "group":
		path, st, ok := s.resolveAttachment(row)
		if !ok {
			rec.MissingParts++
			return rec, ErrContent
		}
		if st.Size() < 0 || st.Size() > wire.MaxFileBytes {
			rec.MissingParts++
			return rec, ErrContent
		}
		detectedMIME := sniffFileMIME(path, row.dataName, kind == "group")
		name := resolveFileName(row.dataName, path, detectedMIME, kind == "group")
		rec.Parts = []recordPart{{
			MIME: detectedMIME,
			Name: name,
			Path: path,
			Size: st.Size(),
		}}
		return rec, nil

	default:
		// Unknown legacy item type: fall back to text if non-empty, otherwise mark unrecoverable.
		if row.text != "" && utf8.ValidString(row.text) && int64(len(row.text)) <= wire.MaxPartBytes {
			data := []byte(row.text)
			rec.Parts = []recordPart{{
				MIME: engine.MimeText,
				Data: data,
				Size: int64(len(data)),
			}}
			return rec, nil
		}
		rec.MissingParts++
		return rec, ErrContent
	}
}

func buildImagePart(path string, st os.FileInfo, dataName string) (recordPart, error) {
	if st.Size() <= 0 || st.Size() > wire.MaxFileBytes {
		return recordPart{}, ErrContent
	}
	// If small enough to inspect/decode into PNG representation, prefer native image/png representation.
	if st.Size() <= maxPayloadBytes && st.Size() <= wire.MaxPartBytes {
		raw, err := readBounded(path, min(maxPayloadBytes, wire.MaxPartBytes))
		if err == nil {
			sniffed := http.DetectContentType(raw)
			if sniffed == engine.MimePNG {
				if cfg, _, cerr := image.DecodeConfig(bytes.NewReader(raw)); cerr == nil && imageWithinBudget(cfg) {
					return recordPart{
						MIME: engine.MimePNG,
						Data: raw,
						Size: int64(len(raw)),
					}, nil
				}
			}
			if pngBytes, ok := transcodeToPNG(raw); ok && int64(len(pngBytes)) <= wire.MaxPartBytes {
				clear(raw)
				return recordPart{
					MIME: engine.MimePNG,
					Data: pngBytes,
					Size: int64(len(pngBytes)),
				}, nil
			}
			clear(raw)
		}
	}
	// Preserve undecodable formats (e.g. HEIC/HEIF) or oversized images as a file part so no bytes are lost.
	detectedMIME := sniffFileMIME(path, dataName, false)
	name := resolveFileName(dataName, path, detectedMIME, false)
	return recordPart{
		MIME: detectedMIME,
		Name: name,
		Path: path,
		Size: st.Size(),
	}, nil
}

func transcodeToPNG(raw []byte) ([]byte, bool) {
	cfg, _, err := image.DecodeConfig(bytes.NewReader(raw))
	if err != nil || !imageWithinBudget(cfg) {
		return nil, false
	}
	img, _, err := image.Decode(bytes.NewReader(raw))
	if err != nil {
		return nil, false
	}
	var out bytes.Buffer
	if png.Encode(&out, img) != nil {
		return nil, false
	}
	return out.Bytes(), true
}

func imageWithinBudget(cfg image.Config) bool {
	return cfg.Width > 0 && cfg.Height > 0 && int64(cfg.Width) <= maxImagePixels/int64(cfg.Height)
}

func (s *Source) resolveAttachment(row legacyRow) (string, os.FileInfo, bool) {
	hashVariants := uniqueNonEmpty(row.profileHash, strings.ToLower(row.profileHash), strings.ToUpper(row.profileHash))
	typeVariants := uniqueNonEmpty(row.itemType, titleCaseType(row.itemType))

	var candidates []string
	// 1. iOS App Group payloads/{Type}-{profileHash} and Android clipboards/history/{Type}-{profileHash}/{dataName}.
	for _, root := range s.attachmentRoots {
		for _, typ := range typeVariants {
			for _, h := range hashVariants {
				profileID := typ + "-" + h
				if isSafeSegment(profileID) {
					candidates = append(candidates,
						filepath.Join(root, profileID),
						filepath.Join(root, "payloads", profileID),
					)
					histDir := filepath.Join(root, "history", profileID)
					if row.dataName != "" && isSafeSegment(row.dataName) {
						candidates = append(candidates, filepath.Join(histDir, row.dataName))
					}
					if single := singleRegularFile(histDir); single != "" {
						candidates = append(candidates, single)
					}
					// Direct history subfolder if root is already clipboards/history.
					directHistDir := filepath.Join(root, profileID)
					if row.dataName != "" && isSafeSegment(row.dataName) {
						candidates = append(candidates, filepath.Join(directHistDir, row.dataName))
					}
					if single := singleRegularFile(directHistDir); single != "" {
						candidates = append(candidates, single)
					}
				}
			}
		}
		// Legacy Android clipboards/images/{hash}* and clipboards/files/{hash}*.
		subdir := "files"
		if strings.EqualFold(row.itemType, "image") {
			subdir = "images"
		}
		ext := filepath.Ext(row.dataName)
		for _, h := range uniqueNonEmpty(row.profileHash, strings.ToLower(row.profileHash), row.localHash, strings.ToLower(row.localHash)) {
			if !isSafeSegment(h) {
				continue
			}
			if ext != "" && isSafeSegment(h+ext) {
				candidates = append(candidates, filepath.Join(root, subdir, h+ext))
			}
			candidates = append(candidates, filepath.Join(root, subdir, h))
		}
	}

	// 2. Stored fileUri (including container-UUID remapping for iOS upgrades).
	if uriPath := parseFileURIPath(row.fileUri); uriPath != "" {
		candidates = append(candidates, uriPath)
		candidates = append(candidates, s.remapStoredURIPath(uriPath)...)
	}

	for _, cand := range candidates {
		resolved, st, ok := s.validateCandidateFile(cand)
		if ok {
			return resolved, st, true
		}
	}
	return "", nil, false
}

func (s *Source) remapStoredURIPath(uriPath string) []string {
	slashed := filepath.ToSlash(uriPath)
	var out []string
	markers := []string{"/payloads/", "/clipboards/", "/temp_files/"}
	for _, marker := range markers {
		idx := strings.LastIndex(slashed, marker)
		if idx < 0 {
			continue
		}
		rel := strings.TrimPrefix(slashed[idx+len(marker):], "/")
		if rel == "" || strings.Contains(rel, "..") {
			continue
		}
		relOS := filepath.FromSlash(rel)
		for _, root := range s.attachmentRoots {
			out = append(out, filepath.Join(root, relOS))
		}
	}
	return out
}

func (s *Source) validateCandidateFile(path string) (string, os.FileInfo, bool) {
	if strings.TrimSpace(path) == "" {
		return "", nil, false
	}
	resolved, err := filepath.EvalSymlinks(path)
	if err != nil {
		return "", nil, false
	}
	st, err := os.Stat(resolved)
	if err != nil || !st.Mode().IsRegular() {
		return "", nil, false
	}
	for _, root := range s.attachmentRoots {
		if containsDir(root, resolved) {
			return resolved, st, true
		}
	}
	return "", nil, false
}

func singleRegularFile(dir string) string {
	entries, err := os.ReadDir(dir)
	if err != nil {
		return ""
	}
	var found string
	for _, e := range entries {
		if e.IsDir() {
			continue
		}
		if found != "" {
			// Ambiguous multiple files; avoid guessing.
			return ""
		}
		found = filepath.Join(dir, e.Name())
	}
	return found
}

func parseFileURIPath(raw string) string {
	trimmed := strings.TrimSpace(raw)
	if trimmed == "" {
		return ""
	}
	if strings.HasPrefix(strings.ToLower(trimmed), "file://") {
		u, err := url.Parse(trimmed)
		if err != nil {
			return ""
		}
		return u.Path
	}
	if filepath.IsAbs(trimmed) {
		return trimmed
	}
	return ""
}

func sniffFileMIME(path string, dataName string, isGroup bool) string {
	if ext := strings.ToLower(filepath.Ext(dataName)); ext != "" {
		if byExt := mime.TypeByExtension(ext); byExt != "" {
			if base, _, err := mime.ParseMediaType(byExt); err == nil && base != "" {
				return base
			}
		}
		switch ext {
		case ".heic":
			return "image/heic"
		case ".heif":
			return "image/heif"
		case ".webp":
			return "image/webp"
		case ".zip":
			return "application/zip"
		}
	}
	f, err := os.Open(path)
	if err == nil {
		defer func() { _ = f.Close() }()
		var buf [512]byte
		n, _ := f.Read(buf[:])
		if n > 0 {
			if n >= 12 && string(buf[4:8]) == "ftyp" {
				brand := string(buf[8:12])
				if brand == "heic" || brand == "heix" || brand == "hevc" || brand == "hevx" || brand == "mif1" || brand == "msf1" {
					return "image/heic"
				}
			}
			sniffed := http.DetectContentType(buf[:n])
			if base, _, perr := mime.ParseMediaType(sniffed); perr == nil && base != "" && base != "application/octet-stream" {
				return base
			}
		}
	}
	if isGroup {
		return "application/zip"
	}
	return "application/octet-stream"
}

func resolveFileName(dataName string, path string, mimeType string, isGroup bool) string {
	cleaned := strings.TrimSpace(dataName)
	if cleaned != "" {
		base := wire.SafeName(filepath.Base(cleaned))
		if base != "" && base != "." && base != ".." {
			return base
		}
	}
	base := wire.SafeName(filepath.Base(path))
	if ext := filepath.Ext(base); ext != "" && !strings.HasPrefix(base, "Image-") && !strings.HasPrefix(base, "File-") && !strings.HasPrefix(base, "Group-") {
		return base
	}
	if isGroup || mimeType == "application/zip" {
		return "archive.zip"
	}
	ext := extensionForMIME(mimeType)
	if strings.HasPrefix(mimeType, "image/") {
		return "image" + ext
	}
	if ext != "" {
		return "file" + ext
	}
	return "file.bin"
}

func extensionForMIME(mimeType string) string {
	switch mimeType {
	case "image/png":
		return ".png"
	case "image/jpeg":
		return ".jpg"
	case "image/gif":
		return ".gif"
	case "image/webp":
		return ".webp"
	case "image/heic":
		return ".heic"
	case "image/heif":
		return ".heif"
	case "application/pdf":
		return ".pdf"
	case "application/zip":
		return ".zip"
	case "text/plain":
		return ".txt"
	default:
		if exts, err := mime.ExtensionsByType(mimeType); err == nil && len(exts) > 0 {
			return exts[0]
		}
		return ""
	}
}

func titleCaseType(t string) string {
	switch strings.ToLower(strings.TrimSpace(t)) {
	case "text":
		return "Text"
	case "image":
		return "Image"
	case "file":
		return "File"
	case "group":
		return "Group"
	default:
		return t
	}
}

func uniqueNonEmpty(values ...string) []string {
	var out []string
	for _, v := range values {
		v = strings.TrimSpace(v)
		if v == "" || slices.Contains(out, v) {
			continue
		}
		out = append(out, v)
	}
	return out
}

func isSafeSegment(name string) bool {
	return name != "" && name != "." && name != ".." && !strings.ContainsAny(name, `/\`+"\x00")
}

func readBounded(path string, limit int64) ([]byte, error) {
	f, err := os.Open(path)
	if err != nil {
		return nil, ErrContent
	}
	defer func() { _ = f.Close() }()
	data, err := io.ReadAll(io.LimitReader(f, limit+1))
	if err != nil || int64(len(data)) > limit {
		clear(data)
		return nil, ErrContent
	}
	return data, nil
}

func (p recordPart) open() (io.ReadCloser, error) {
	if p.Path == "" {
		return io.NopCloser(bytes.NewReader(p.Data)), nil
	}
	f, err := os.Open(p.Path)
	if err != nil {
		return nil, ErrContent
	}
	st, err := f.Stat()
	if err != nil || !st.Mode().IsRegular() || st.Size() != p.Size {
		_ = f.Close()
		return nil, ErrContent
	}
	return f, nil
}

func (r *record) importRequest(source string) (engine.HistoryImportRequest, []io.Closer, error) {
	req := engine.HistoryImportRequest{
		Source:      source,
		RecordID:    r.ID,
		CreatedAtMs: r.CreatedAtMs,
		UpdatedAtMs: r.UpdatedAtMs,
		Tags:        r.Tags,
	}
	var closers []io.Closer
	for _, p := range r.Parts {
		reader, err := p.open()
		if err != nil {
			return req, closers, err
		}
		closers = append(closers, reader)
		if p.Name == "" {
			req.Content.Representations = append(req.Content.Representations, engine.RepresentationInput{
				MIME:   p.MIME,
				Size:   p.Size,
				Reader: reader,
			})
		} else {
			req.Content.Files = append(req.Content.Files, engine.FileInput{
				Name:   p.Name,
				MIME:   p.MIME,
				Size:   p.Size,
				Reader: reader,
			})
		}
	}
	return req, closers, nil
}

func repairMissingTags(ctx context.Context, target *engine.Engine, id engine.HistoryID, wantTags []string, report *v1migration.Report) error {
	if len(wantTags) == 0 {
		return nil
	}
	item, err := target.GetItem(ctx, id)
	var ee *engine.Error
	if errors.As(err, &ee) && ee.Code == engine.CodeNotFound {
		return nil
	}
	if err != nil {
		return err
	}
	missing := false
	for _, tag := range wantTags {
		if !slices.Contains(item.Tags, tag) {
			missing = true
			break
		}
	}
	if !missing {
		return nil
	}
	if item.Revision != 1 {
		report.TagRepairsSkipped++
		return nil
	}
	merged := append(slices.Clone(item.Tags), wantTags...)
	slices.Sort(merged)
	merged = slices.Compact(merged)
	if _, err := target.UpdateItem(ctx, engine.UpdateItemRequest{
		ID:               id,
		ExpectedRevision: item.Revision,
		Tags:             &merged,
	}); err != nil {
		return err
	}
	report.TagsRepaired++
	return nil
}

func verifyRecord(ctx context.Context, e *engine.Engine, id engine.HistoryID, r *record) error {
	item, err := e.GetItem(ctx, id)
	if err != nil {
		return err
	}
	tags := slices.Clone(r.Tags)
	slices.Sort(tags)
	tags = slices.Compact(tags)
	if item.CreatedAtMs != r.CreatedAtMs || (item.Revision == 1 && item.UpdatedAtMs != r.UpdatedAtMs) || !slices.Equal(item.Tags, tags) {
		return ErrContent
	}
	h, err := e.RetainHistoryContent(ctx, id)
	if err != nil {
		return err
	}
	defer func() { _ = e.ReleaseContent(context.Background(), h.LeaseID) }()
	if len(h.Content.Parts) != len(r.Parts) {
		return ErrContent
	}
	buf := make([]byte, 64<<10)
	defer clear(buf)
	for i, p := range r.Parts {
		desc := h.Content.Parts[i]
		if desc.Size != p.Size || desc.MIME != p.MIME {
			return ErrContent
		}
		want, err := p.open()
		if err != nil {
			return err
		}
		sumWant := sha256.New()
		_, err = io.CopyBuffer(sumWant, &ctxReader{ctx, want}, buf)
		_ = want.Close()
		if err != nil {
			return ErrContent
		}
		got, err := e.ReadContent(ctx, h.LeaseID, desc.ID)
		if err != nil {
			return err
		}
		sumGot := sha256.New()
		_, err = io.CopyBuffer(sumGot, got, buf)
		_ = got.Close()
		if err != nil || !bytes.Equal(sumGot.Sum(nil), sumWant.Sum(nil)) {
			return ErrContent
		}
	}
	return nil
}

func closeAll(closers []io.Closer) {
	for _, c := range closers {
		_ = c.Close()
	}
}

func canonicalDir(name string) (string, error) {
	abs, err := filepath.Abs(name)
	if err != nil {
		return "", ErrFormat
	}
	var missing []string
	for {
		resolved, err := filepath.EvalSymlinks(abs)
		if err == nil {
			for i := len(missing) - 1; i >= 0; i-- {
				resolved = filepath.Join(resolved, missing[i])
			}
			return resolved, nil
		}
		if !os.IsNotExist(err) {
			return "", ErrFormat
		}
		parent := filepath.Dir(abs)
		if parent == abs {
			return "", ErrFormat
		}
		missing = append(missing, filepath.Base(abs))
		abs = parent
	}
}

func containsDir(parent, child string) bool {
	rel, err := filepath.Rel(parent, child)
	return err == nil && (rel == "." || rel != ".." && !strings.HasPrefix(rel, ".."+string(filepath.Separator)))
}

type ctxReader struct {
	ctx context.Context
	r   io.Reader
}

func (r *ctxReader) Read(b []byte) (int, error) {
	if err := r.ctx.Err(); err != nil {
		return 0, err
	}
	return r.r.Read(b)
}
