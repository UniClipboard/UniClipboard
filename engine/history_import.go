package engine

import (
	"context"
	"io"
	"os"
	"path/filepath"
	"runtime"
	"strings"

	"github.com/uniclipboard/unic/internal/store"
	"github.com/uniclipboard/unic/internal/wire"
)

// HistoryImportRequest is one immutable record from a local archive. Source
// and RecordID must be opaque identifiers, never paths or user content.
// Import is offline, durable, idempotent, and never invokes processors or sync.
type HistoryImportRequest struct {
	Source, RecordID         string
	CreatedAtMs, UpdatedAtMs int64
	Tags                     []string
	Content                  ContentInput
}
type HistoryImportResult struct {
	HistoryID       HistoryID `json:"history_id"`
	AlreadyImported bool      `json:"already_imported"`
}

func (e *Engine) LookupHistoryImport(ctx context.Context, source, record string) (HistoryID, bool, error) {
	release, err := e.historyGate(ctx, false, false)
	if err != nil {
		return "", false, err
	}
	defer release()
	id, known, serr := e.st.LookupHistoryImport(source, record)
	if serr != nil {
		return "", false, historyStoreErr(serr)
	}
	return HistoryID(id), known, nil
}

func (e *Engine) ImportHistory(ctx context.Context, req HistoryImportRequest) (HistoryImportResult, error) {
	release, err := e.historyGate(ctx, true, false)
	if err != nil {
		return HistoryImportResult{}, err
	}
	defer release()
	if !e.opts.MigrationMode {
		return HistoryImportResult{}, newErr(CodeInvalidState, "history import requires an offline migration session")
	}
	if req.Source == "" || len(req.Source) > 256 || req.RecordID == "" || len(req.RecordID) > 256 || req.CreatedAtMs < 0 || req.UpdatedAtMs < 0 {
		return HistoryImportResult{}, newErr(CodeInvalidArgument, "invalid history import metadata")
	}
	h := e.hist
	h.mu.Lock()
	defer h.mu.Unlock()
	old, known, lookupErr := e.st.LookupHistoryImport(req.Source, req.RecordID)
	if lookupErr != nil {
		return HistoryImportResult{}, historyStoreErr(lookupErr)
	}
	if known {
		return HistoryImportResult{HistoryID: HistoryID(old), AlreadyImported: true}, nil
	}
	tags, err := normalizeTags(req.Tags)
	if err != nil {
		return HistoryImportResult{}, err
	}
	count := len(req.Content.Representations) + len(req.Content.Files)
	if count == 0 || count > wire.MaxParts {
		return HistoryImportResult{}, newErr(CodeLimitExceeded, "invalid number of imported parts")
	}
	it := store.HistoryItem{
		ID: randHex(16), Kind: string(HistoryText), OriginKind: string(OriginKindMigration),
		CreatedMs: req.CreatedAtMs, UpdatedMs: max(req.CreatedAtMs, req.UpdatedAtMs), Tags: tags,
		SyncState: string(HistorySyncNotRequested), ImportSource: req.Source, ImportRecord: req.RecordID,
	}
	type input struct {
		part   store.HistoryPart
		reader io.Reader
	}
	var inputs []input
	seen := map[string]bool{}
	for _, rep := range req.Content.Representations {
		m := strings.ToLower(strings.TrimSpace(rep.MIME))
		if !contains(coreFormats, m) || seen[m] || rep.Size < 1 || rep.Size > wire.MaxPartBytes || rep.Reader == nil {
			return HistoryImportResult{}, newErr(CodeInvalidArgument, "invalid imported representation")
		}
		seen[m] = true
		if m == MimePNG {
			it.Kind = string(HistoryImage)
		}
		inputs = append(inputs, input{store.HistoryPart{Kind: "rep", MIME: m, Size: rep.Size}, rep.Reader})
	}
	for _, file := range req.Content.Files {
		if file.Source != nil || file.Reader == nil || file.Name == "" || len(file.Name) > 255 || file.Size < 0 || file.Size > wire.MaxFileBytes {
			return HistoryImportResult{}, newErr(CodeInvalidArgument, "invalid imported file")
		}
		it.Kind = string(HistoryFiles)
		inputs = append(inputs, input{store.HistoryPart{Kind: "file", Name: wire.SafeName(file.Name), MIME: nonEmptyMIME(file.MIME), Size: file.Size}, file.Reader})
	}
	for _, in := range inputs {
		it.TotalBytes += in.part.Size
	}
	if it.TotalBytes > wire.MaxTotal {
		return HistoryImportResult{}, newErr(CodeLimitExceeded, "imported history is too large")
	}
	var written []string
	published := false
	defer func() {
		if !published {
			for _, b := range written {
				_ = os.RemoveAll(filepath.Join(h.dir, b))
			}
		}
	}()
	for _, in := range inputs {
		if err := ctx.Err(); err != nil {
			return HistoryImportResult{}, ctxErr(err)
		}
		p := in.part
		if p.Kind == "rep" && p.Size <= historyInlineMax {
			b := make([]byte, p.Size)
			if _, readErr := io.ReadFull(ctxReader{ctx, in.reader}, b); readErr != nil {
				return HistoryImportResult{}, inputErr(ctx, readErr)
			}
			if err := expectEOF(ctx, in.reader); err != nil {
				return HistoryImportResult{}, err
			}
			p.Inline = b
		} else {
			name := randHex(16)
			dir := filepath.Join(h.dir, name)
			if os.Mkdir(dir, 0o700) != nil {
				return HistoryImportResult{}, newErr(CodeUnavailable, "cannot prepare imported content")
			}
			written = append(written, name)
			fname := "data"
			if p.Kind == "file" {
				fname = p.Name
			}
			path := filepath.Join(dir, fname)
			if err := e.copyIn(ctx, path, in.reader, p.Size); err != nil {
				return HistoryImportResult{}, err
			}
			f, openErr := os.OpenFile(path, os.O_RDWR, 0)
			if openErr != nil {
				return HistoryImportResult{}, newErr(CodeUnavailable, "cannot persist imported content")
			}
			syncErr := f.Sync()
			closeErr := f.Close()
			if syncErr != nil || closeErr != nil {
				return HistoryImportResult{}, newErr(CodeUnavailable, "cannot persist imported content")
			}
			if syncImportDirectory(dir) != nil {
				return HistoryImportResult{}, newErr(CodeUnavailable, "cannot persist imported content")
			}
			p.Blob = name
		}
		it.Parts = append(it.Parts, p)
	}
	if syncImportDirectory(h.dir) != nil {
		return HistoryImportResult{}, newErr(CodeUnavailable, "cannot persist imported content")
	}
	if ctx.Err() != nil {
		return HistoryImportResult{}, ctxErr(ctx.Err())
	}
	if _, _, serr := e.st.InsertHistory(it); serr != nil {
		return HistoryImportResult{}, historyStoreErr(serr)
	}
	published = true
	h.refresh(e.st)
	e.historyChanged(it.ID)
	return HistoryImportResult{HistoryID: HistoryID(it.ID)}, nil
}

func nonEmptyMIME(s string) string {
	if s == "" {
		return "application/octet-stream"
	}
	return s
}
func syncImportDirectory(path string) error {
	if runtime.GOOS == "windows" {
		return nil
	}
	f, err := os.Open(path)
	if err != nil {
		return err
	}
	defer func() { _ = f.Close() }()
	return f.Sync()
}

// ImportHistoryTags preserves definitions even when no recoverable history
// currently refers to them. Existing associations are never changed.
func (e *Engine) ImportHistoryTags(ctx context.Context, names []string) error {
	release, err := e.historyGate(ctx, true, false)
	if err != nil {
		return err
	}
	defer release()
	if !e.opts.MigrationMode {
		return newErr(CodeInvalidState, "tag import requires an offline migration session")
	}
	tags, err := normalizeTags(names)
	if err != nil {
		return err
	}
	e.hist.mu.Lock()
	defer e.hist.mu.Unlock()
	if err := e.st.ImportHistoryTags(tags); err != nil {
		return historyStoreErr(err)
	}
	return nil
}
func (e *Engine) ListHistoryTags(ctx context.Context) ([]string, error) {
	release, err := e.historyGate(ctx, false, false)
	if err != nil {
		return nil, err
	}
	defer release()
	tags, serr := e.st.ListHistoryTags()
	if serr != nil {
		return nil, historyStoreErr(serr)
	}
	return tags, nil
}
