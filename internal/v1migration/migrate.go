package v1migration

import (
	"bytes"
	"context"
	"crypto/sha256"
	"errors"
	"hash"
	"io"
	"os"
	"path/filepath"
	"slices"
	"strings"

	"github.com/zeebo/blake3"

	"github.com/uniclipboard/unic/engine"
)

type Report struct {
	Status             string `json:"status"`
	Total              int    `json:"total"`
	Imported           int    `json:"imported"`
	Recoverable        int    `json:"recoverable"`
	Skipped            int    `json:"skipped"`
	Failed             int    `json:"failed"`
	MissingParts       int    `json:"missing_parts"`
	Verified           int    `json:"verified"`
	VerificationFailed int    `json:"verification_failed"`
	Tags               int    `json:"tags"`
}
type Progress struct {
	Stage     string `json:"stage"`
	Processed int    `json:"processed"`
	Total     int    `json:"total"`
}
type RunOptions struct {
	Target   string
	DryRun   bool
	Progress func(Progress)
}

// Run owns the complete offline migration. A non-complete report must never
// be presented as successful full migration by its caller.
func (s *Source) Run(ctx context.Context, opts RunOptions) (report Report, runErr error) {
	report.Status = "failed"
	report.Total, runErr = s.Count()
	if runErr != nil {
		return
	}
	report.Tags = len(s.tags)
	notify := func(stage string, n int) {
		if opts.Progress != nil {
			opts.Progress(Progress{stage, n, report.Total})
		}
	}
	var target *engine.Engine
	var dst string // the resolved target; every reopen must use it, never the caller's spelling
	if !opts.DryRun {
		if opts.Target == "" {
			return report, errors.New("v1 migration: target directory is required")
		}
		// Opening the wrong directory must not create a target within the source.
		src, e1 := canonicalDirectory(s.root)
		var e2 error
		dst, e2 = canonicalDirectory(opts.Target)
		if e1 != nil || e2 != nil {
			return report, ErrFormat
		}
		if containsDirectory(src, dst) || containsDirectory(dst, src) {
			return report, errors.New("v1 migration: source and target must be separate directories")
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
		if runErr = target.ImportHistoryTags(ctx, s.TagNames()); runErr != nil {
			return
		}
	}
	processed := 0
	runErr = s.Walk(ctx, func(r *Record, sourceErr error) error {
		processed++
		notify("import", processed)
		report.MissingParts += r.MissingParts
		if sourceErr != nil {
			report.Failed++
			return nil
		}
		if opts.DryRun {
			for _, part := range r.Parts {
				reader, err := part.Open()
				if err != nil {
					report.Failed++
					return nil
				}
				_, err = io.Copy(io.Discard, &contextReader{ctx, reader})
				_ = reader.Close()
				if err != nil {
					report.Failed++
					return nil
				}
			}
			report.Recoverable++
			return nil
		}
		_, known, err := target.LookupHistoryImport(ctx, s.ID(), r.ID)
		if err != nil {
			return err
		}
		if known {
			report.Skipped++
			return nil
		}
		request, closers, err := r.importRequest(s.ID())
		if err != nil {
			closeReaders(closers)
			report.Failed++
			return nil
		}
		result, err := target.ImportHistory(ctx, request)
		closeReaders(closers)
		if err != nil {
			if ctx.Err() != nil {
				return ctx.Err()
			}
			var ee *engine.Error
			// A storage failure is session-wide; continuing could repeatedly
			// write against a full disk. Content failures remain retryable rows.
			if errors.As(err, &ee) && (ee.Code == engine.CodeUnavailable || ee.Code == engine.CodeResourceExhausted) {
				return err
			}
			report.Failed++
			return nil
		}
		if result.AlreadyImported {
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
		runErr = s.Walk(ctx, func(r *Record, sourceErr error) error {
			processed++
			notify("verify", processed)
			id, known, err := target.LookupHistoryImport(ctx, s.ID(), r.ID)
			if err != nil {
				return err
			}
			if !known {
				return nil
			}
			if sourceErr != nil {
				report.VerificationFailed++
				return nil
			}
			if err = verifyRecord(ctx, target, id, r); err != nil {
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

func canonicalDirectory(name string) (string, error) {
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
func containsDirectory(parent, child string) bool {
	rel, err := filepath.Rel(parent, child)
	return err == nil && (rel == "." || rel != ".." && !strings.HasPrefix(rel, ".."+string(filepath.Separator)))
}

func (r *Record) importRequest(source string) (engine.HistoryImportRequest, []io.Closer, error) {
	req := engine.HistoryImportRequest{Source: source, RecordID: r.ID, CreatedAtMs: r.CreatedAtMs, UpdatedAtMs: r.UpdatedAtMs, Tags: r.Tags}
	var closers []io.Closer
	for _, p := range r.Parts {
		reader, err := p.Open()
		if err != nil {
			return req, closers, err
		}
		closers = append(closers, reader)
		if p.Name == "" {
			req.Content.Representations = append(req.Content.Representations, engine.RepresentationInput{MIME: p.MIME, Size: p.Size, Reader: reader})
		} else {
			req.Content.Files = append(req.Content.Files, engine.FileInput{Name: p.Name, MIME: p.MIME, Size: p.Size, Reader: reader})
		}
	}
	return req, closers, nil
}
func closeReaders(readers []io.Closer) {
	for _, r := range readers {
		_ = r.Close()
	}
}

type checkedFile struct {
	file     *os.File
	initial  os.FileInfo
	digest   hash.Hash
	expected string
	done     bool
}

func (p Part) Open() (io.ReadCloser, error) {
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
	return &checkedFile{file: f, initial: st, digest: blake3.New(), expected: p.Hash}, nil
}
func (f *checkedFile) Close() error { return f.file.Close() }
func (f *checkedFile) Read(b []byte) (int, error) {
	n, err := f.file.Read(b)
	if n > 0 {
		_, _ = f.digest.Write(b[:n])
	}
	if err == io.EOF && !f.done {
		f.done = true
		st, e := f.file.Stat()
		if e != nil || st.Size() != f.initial.Size() || !st.ModTime().Equal(f.initial.ModTime()) ||
			f.expected != "" && !matchesContentHash(f.expected, f.digest.Sum(nil)) {
			return n, ErrContent
		}
	}
	return n, err
}

func verifyRecord(ctx context.Context, e *engine.Engine, id engine.HistoryID, r *Record) error {
	item, err := e.GetItem(ctx, id)
	if err != nil {
		return err
	}
	tags := slices.Clone(r.Tags)
	slices.Sort(tags)
	tags = slices.Compact(tags)
	if item.CreatedAtMs != r.CreatedAtMs || item.UpdatedAtMs != r.UpdatedAtMs || !slices.Equal(item.Tags, tags) {
		return ErrContent
	}
	h, err := e.RetainHistoryContent(ctx, id)
	if err != nil {
		return err
	}
	defer func() { _ = e.ReleaseContent(context.Background(), h.LeaseID) }()
	// Engine groups representations before files.
	parts := slices.Clone(r.Parts)
	slices.SortStableFunc(parts, func(a, b Part) int {
		if (a.Name == "") == (b.Name == "") {
			return 0
		}
		if a.Name == "" {
			return -1
		}
		return 1
	})
	if len(h.Content.Parts) != len(parts) {
		return ErrContent
	}
	buf := make([]byte, 64<<10)
	defer clear(buf)
	for i, p := range parts {
		desc := h.Content.Parts[i]
		if desc.Size != p.Size || desc.MIME != p.MIME {
			return ErrContent
		}
		want, err := p.Open()
		if err != nil {
			return err
		}
		sumWant := sha256.New()
		_, err = io.CopyBuffer(sumWant, &contextReader{ctx, want}, buf)
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

type contextReader struct {
	ctx context.Context
	r   io.Reader
}

func (r *contextReader) Read(b []byte) (int, error) {
	if err := r.ctx.Err(); err != nil {
		return 0, err
	}
	return r.r.Read(b)
}
