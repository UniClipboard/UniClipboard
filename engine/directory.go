package engine

import (
	"bytes"
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"hash"
	"io"
	"os"
	"path/filepath"
	"strconv"
	"syscall"

	"github.com/uniclipboard/unic/internal/directory"
)

// MimeDirectory identifies a structured tree manifest, not a text alternative.
const MimeDirectory = directory.MIME

func directoryReference(c *contentObj) bool {
	for _, p := range c.parts {
		if p.desc.MIME == directory.ReferenceMIME {
			return true
		}
	}
	return false
}
func directoryManifest(c *contentObj) (*directory.Manifest, *Error) {
	var data []byte
	var sizes []int64
	for _, p := range c.parts {
		if p.desc.Kind == PartFile {
			sizes = append(sizes, p.desc.Size)
		}
		if p.desc.MIME == directory.MIME {
			if data != nil {
				return nil, newErr(CodeInvalidArgument, "duplicate directory manifest")
			}
			data = p.data
		}
	}
	if data == nil {
		return nil, nil
	}
	m, err := directory.Decode(data, sizes)
	if err != nil {
		return nil, newErr(CodeInvalidArgument, "invalid directory manifest")
	}
	return m, nil
}

type snapshotReader struct {
	probe    *directoryProbe
	ctx      context.Context
	source   FileSource
	expected FileMeta
	reader   io.ReadCloser
	hash     hash.Hash
	done     bool
}

func (s *snapshotReader) Read(b []byte) (int, error) {
	if s.done {
		return 0, io.EOF
	}
	if err := s.ctx.Err(); err != nil {
		return 0, err
	}
	if s.reader == nil {
		r, meta, err := s.source.Open(s.ctx)
		if err != nil {
			return 0, err
		}
		if meta != s.expected {
			_ = r.Close()
			return 0, newErr(CodeSourceChanged, "source changed during preparation")
		}
		s.reader = r
		if err := s.probe.observe(s.ctx, "file"); err != nil {
			return 0, err
		}
	}
	n, err := s.reader.Read(b)
	if n > 0 {
		_, _ = s.hash.Write(b[:n])
	}
	if err == io.EOF {
		_ = s.reader.Close()
		s.reader = nil
		s.done = true
		r, meta, e := s.source.Open(s.ctx)
		if e != nil {
			return n, e
		}
		_ = r.Close()
		if meta != s.expected {
			return n, newErr(CodeSourceChanged, "source changed during preparation")
		}
	}
	return n, err
}
func (s *snapshotReader) Close() {
	if s.reader != nil {
		_ = s.reader.Close()
		s.reader = nil
	}
}

// prepareDirectory consumes no input reference and returns one new content
// reference. All payload bytes are reserved and copied by ImportContent.
func (e *Engine) prepareDirectory(ctx context.Context, c *contentObj, entry *submissionEntry) (*contentObj, *Error) {
	e.mu.Lock()
	settings := e.settings.Value.clone()
	e.mu.Unlock()
	if !settings.Sync.Enabled || !settings.Sync.Files {
		return nil, newErr(CodeSyncDisabled, "directory send is disabled")
	}
	allowed := false
	for _, b := range e.syncBackends {
		if e.policyOf(b.ID()).Files {
			allowed = true
		}
	}
	if !allowed {
		return nil, newErr(CodeSyncDisabled, "directory send is disabled by backend policy")
	}
	e.setSubmissionStage(entry, "scanning")
	if err := e.directoryProbe.observe(ctx, "scan"); err != nil {
		return nil, ctxErr(err)
	}
	var m directory.Manifest
	in := ContentInput{SourceApplication: c.source.Clone()}
	var dirs []DirectoryListing
	var readers []*snapshotReader
	var total int64
	defer func() {
		for _, r := range readers {
			r.Close()
		}
	}()
	add := func(root int, entry DirectoryEntry, expected *FileMeta) *Error {
		if len(m.Entries) >= directory.MaxEntries {
			return newErr(CodeLimitExceeded, "too many directory entries")
		}
		v := directory.Entry{Root: root, Path: entry.Path, Directory: entry.Directory, Size: entry.Size}
		if !entry.Directory {
			if len(in.Files) >= directory.MaxFiles || entry.Size > settings.Limits.MaxFileBytes || entry.Size < 0 {
				return newErr(CodeLimitExceeded, "directory file limit exceeded")
			}
			total += entry.Size
			if total > settings.Limits.MaxTotalBytes {
				return newErr(CodeLimitExceeded, "directory total limit exceeded")
			}
			rc, meta, err := entry.Source.Open(ctx)
			if err != nil {
				return sourceErr(ctx, err)
			}
			_ = rc.Close()
			if meta.Size != entry.Size || (expected != nil && meta != *expected) {
				return newErr(CodeSourceChanged, "source changed during preparation")
			}
			reader := &snapshotReader{probe: &e.directoryProbe, ctx: ctx, source: entry.Source, expected: meta, hash: sha256.New()}
			readers = append(readers, reader)
			v.File = len(in.Files)
			v.SHA256 = hex.EncodeToString(make([]byte, 32))
			in.Files = append(in.Files, FileInput{Name: filepath.Base(entry.Path), Size: entry.Size, Reader: reader})
			if entry.Path == "" {
				in.Files[len(in.Files)-1].Name = m.Roots[root].Name
			}
		}
		m.Entries = append(m.Entries, v)
		return nil
	}
	if len(c.parts) > directory.MaxRoots {
		return nil, newErr(CodeLimitExceeded, "too many selected roots")
	}
	for _, p := range c.parts {
		if p.desc.Kind != PartFile {
			return nil, newErr(CodeUnsupported, "directory selections cannot contain alternate formats")
		}
		root := len(m.Roots)
		ds, isDir := p.src.(DirectorySource)
		if p.desc.MIME == directory.ReferenceMIME && !isDir {
			return nil, newErr(CodeUnsupported, "directory source adapter unavailable")
		}
		m.Roots = append(m.Roots, directory.Root{Name: p.desc.Name, Directory: isDir})
		if isDir {
			listing, err := ds.ReadDirectory(ctx)
			if err != nil {
				return nil, sourceErr(ctx, err)
			}
			dirs = append(dirs, listing)
			for _, entry := range listing.Entries {
				if err := add(root, entry, nil); err != nil {
					return nil, err
				}
			}
		} else {
			if p.src == nil {
				return nil, newErr(CodeUnsupported, "selection source is unavailable")
			}
			if err := add(root, DirectoryEntry{Size: p.desc.Size, Source: p.src}, &p.meta); err != nil {
				return nil, err
			}
		}
	}
	sizes := make([]int64, len(in.Files))
	for i, f := range in.Files {
		sizes[i] = f.Size
	}
	if err := m.Validate(sizes); err != nil {
		return nil, newErr(CodeInvalidArgument, "invalid selected directory structure")
	}
	// Reserve a bounded metadata payload together with snapshot bytes. Empty
	// trees use this representation as their nonempty content.
	initial, _ := json.Marshal(m)
	if len(initial) > directory.MaxManifest {
		return nil, newErr(CodeLimitExceeded, "directory manifest limit exceeded")
	}
	in.Representations = []RepresentationInput{{MIME: directory.MIME, Size: int64(len(initial)), Reader: bytesReader(initial)}}
	e.setSubmissionStage(entry, "preparing")
	if err := e.directoryProbe.observe(ctx, "snapshot"); err != nil {
		return nil, ctxErr(err)
	}
	h, err := e.ImportContent(ctx, in)
	if err != nil {
		return nil, asError(err)
	}
	success := false
	defer func() {
		if !success {
			_ = e.ReleaseContent(context.Background(), h.LeaseID)
		}
	}()
	for _, ds := range dirs {
		if ds.Verify == nil {
			return nil, newErr(CodeInvalidArgument, "invalid directory listing")
		}
		if err := ds.Verify(ctx); err != nil {
			return nil, sourceErr(ctx, err)
		}
	}
	for i := range m.Entries {
		if !m.Entries[i].Directory {
			m.Entries[i].SHA256 = hex.EncodeToString(readers[m.Entries[i].File].hash.Sum(nil))
		}
	}
	data, _ := json.Marshal(m)
	e.cs.mu.Lock()
	rec := e.cs.leases[h.LeaseID]
	prepared := rec.c
	prepared.parts[0].data = data
	prepared.refs++
	e.cs.mu.Unlock()
	_ = e.ReleaseContent(context.Background(), h.LeaseID)
	success = true
	return prepared, nil
}
func bytesReader(b []byte) io.Reader { return bytes.NewReader(b) }

// DirectoryPaths materializes a leased immutable tree for a platform clipboard.
// Hard links reuse retained payload bytes; cross-filesystem copies are charged.
func (e *Engine) DirectoryPaths(ctx context.Context, lease LeaseID) ([]string, error) {
	release, gateErr := e.enter(ctx)
	if gateErr != nil {
		return nil, gateErr
	}
	defer release()
	e.cs.mu.Lock()
	l := e.cs.leases[lease]
	if l == nil {
		e.cs.mu.Unlock()
		return nil, newErr(CodeResourceExpired, "directory lease expired")
	}
	c := l.c
	c.refs++
	e.cs.mu.Unlock()
	defer e.cs.unref(c)
	c.treeMu.Lock()
	defer c.treeMu.Unlock()
	if c.treePaths != nil {
		return append([]string{}, c.treePaths...), nil
	}
	m, failure := directoryManifest(c)
	if failure != nil {
		return nil, failure
	}
	if m == nil {
		return nil, newErr(CodeUnsupported, "content is not a directory snapshot")
	}
	var err error
	if c.dir == "" {
		if err := os.MkdirAll(e.opts.InboxDir, 0700); err != nil {
			return nil, newErr(CodeUnavailable, "cannot materialize directory")
		}
		c.dir, err = os.MkdirTemp(e.opts.InboxDir, contentDirPrefix)
		if err != nil {
			return nil, newErr(CodeUnavailable, "cannot materialize directory")
		}
	}
	view := filepath.Join(c.dir, "view")
	_ = os.RemoveAll(view)
	if err := os.Mkdir(view, 0700); err != nil {
		return nil, newErr(CodeUnavailable, "cannot materialize directory")
	}
	ok := false
	var extra int64
	defer func() {
		if !ok {
			if extra > 0 {
				e.cs.mu.Lock()
				e.cs.stored -= extra
				e.cs.mu.Unlock()
			}
			_ = os.RemoveAll(view)
		}
	}()
	root, err := os.OpenRoot(view)
	if err != nil {
		return nil, newErr(CodeUnavailable, "cannot materialize directory")
	}
	defer func() { _ = root.Close() }()
	ownedRoot, openErr := os.OpenRoot(e.opts.DataDir)
	if openErr != nil {
		return nil, newErr(CodeUnavailable, "directory storage unavailable")
	}
	defer func() { _ = ownedRoot.Close() }()
	paths := make([]string, len(m.Roots))
	for i, r := range m.Roots {
		sub := fmtRoot(i)
		if err := root.Mkdir(sub, 0700); err != nil {
			return nil, newErr(CodeUnavailable, "cannot materialize directory")
		}
		paths[i] = filepath.Join(view, sub, r.Name)
	}
	var files []*partRec
	for _, p := range c.parts {
		if p.desc.Kind == PartFile {
			files = append(files, p)
		}
	}
	for _, entry := range m.Entries {
		if ctx.Err() != nil {
			return nil, ctxErr(ctx.Err())
		}
		rel := filepath.Join(fmtRoot(entry.Root), m.Roots[entry.Root].Name, filepath.FromSlash(entry.Path))
		if entry.Directory {
			if err := root.Mkdir(rel, 0700); err != nil {
				return nil, newErr(CodeUnavailable, "cannot materialize directory")
			}
			continue
		}
		p := files[entry.File]
		source := p.path
		if source == "" {
			return nil, newErr(CodeUnsupported, "host tree paths require materialization")
		}
		info, err := os.Lstat(source)
		if err != nil || !info.Mode().IsRegular() || info.Size() != entry.Size {
			return nil, newErr(CodeResourceExpired, "directory payload unavailable")
		}
		sourceRel, sourceErr := filepath.Rel(e.opts.DataDir, source)
		targetRel, targetErr := filepath.Rel(e.opts.DataDir, filepath.Join(view, rel))
		linkErr := error(syscall.EXDEV)
		if sourceErr == nil && targetErr == nil && filepath.IsLocal(sourceRel) && filepath.IsLocal(targetRel) {
			linkErr = ownedRoot.Link(sourceRel, targetRel)
		}
		if err := linkErr; err != nil {
			if !errors.Is(err, syscall.EXDEV) && !errors.Is(err, syscall.EOPNOTSUPP) && !errors.Is(err, syscall.EPERM) && !errors.Is(err, syscall.ENOSYS) {
				return nil, newErr(CodeUnavailable, "cannot materialize directory payload")
			}
			if !e.reserveStored(entry.Size, false) {
				return nil, newErr(CodeResourceExhausted, "not enough directory view storage")
			}
			extra += entry.Size
			r, err := e.openPart(ctx, p)
			if err != nil {
				return nil, err
			}
			copyErr := copyDirectoryView(ctx, root, rel, r, entry.Size)
			_ = r.Close()
			if copyErr != nil {
				return nil, copyErr
			}
		}
	}
	e.cs.mu.Lock()
	c.owned += extra
	e.cs.mu.Unlock()
	c.treePaths = paths
	ok = true
	return append([]string{}, paths...), nil
}
func fmtRoot(i int) string { return "r" + strconv.Itoa(i) }

// verifyDirectory checks all member hashes before any receive publication.
func (e *Engine) verifyDirectory(ctx context.Context, c *contentObj) *Error {
	m, err := directoryManifest(c)
	if err != nil {
		return err
	}
	if m == nil {
		return nil
	}
	var files []*partRec
	for _, p := range c.parts {
		if p.desc.Kind == PartFile {
			files = append(files, p)
		}
	}
	for _, entry := range m.Entries {
		if entry.Directory {
			continue
		}
		p := files[entry.File]
		r, err := e.openPart(ctx, p)
		if err != nil {
			return err
		}
		h := sha256.New()
		n, readErr := io.Copy(h, ctxReader{ctx, r})
		_ = r.Close()
		if readErr != nil || n != entry.Size || hex.EncodeToString(h.Sum(nil)) != entry.SHA256 {
			return newErr(CodeInvalidArgument, "directory payload integrity failed")
		}
	}
	return nil
}

func (e *Engine) storeReceivedDirectory(ctx context.Context, c *contentObj, m *directory.Manifest) (failure *Error) {
	defer func() {
		if recover() != nil {
			failure = newErr(CodeUnavailable, "custom directory storage failed")
		}
	}()
	if e.opts.StoreReceivedDirectory == nil {
		return newErr(CodeUnsupported, "host cannot store directories")
	}
	var parts []*partRec
	var manifest *partRec
	for _, p := range c.parts {
		if p.desc.Kind == PartFile {
			parts = append(parts, p)
		}
		if p.desc.MIME == directory.MIME {
			manifest = p
		}
	}
	stored, err := e.opts.StoreReceivedDirectory(ctx, ReceivedDirectory{Manifest: *m, OpenFile: func(i int) (io.ReadCloser, error) {
		if i < 0 || i >= len(parts) {
			return nil, newErr(CodeInvalidArgument, "invalid directory member")
		}
		r, err := e.openPart(ctx, parts[i])
		if err != nil {
			return nil, err
		}
		return r, nil
	}})
	if err != nil || len(stored.Files) != len(parts) || len(stored.Roots) != len(m.Roots) {
		return newErr(CodeUnavailable, "custom directory storage failed")
	}
	rootsBytes, _ := json.Marshal(stored.Roots)
	if len(rootsBytes) > 65536 {
		return newErr(CodeLimitExceeded, "custom directory roots exceed limit")
	}
	metas := make([]FileMeta, len(parts))
	for i, src := range stored.Files {
		if src == nil || src.PersistentRef() == "" {
			return newErr(CodeUnavailable, "custom directory storage failed")
		}
		r, meta, err := src.Open(ctx)
		if err != nil {
			return newErr(CodeUnavailable, "custom directory storage failed")
		}
		_ = r.Close()
		if meta.Size != parts[i].desc.Size {
			return newErr(CodeSourceChanged, "custom directory member changed")
		}
		metas[i] = meta
	}
	for i, p := range parts {
		p.src = stored.Files[i]
		p.meta = metas[i]
		p.path = ""
		p.desc.Referenced = true
		c.owned -= p.desc.Size
	}
	manifest.treeRoots = append([]string{}, stored.Roots...)
	return nil
}

// DirectoryReferences returns only opaque host roots through an explicit lease call.
func (e *Engine) DirectoryReferences(ctx context.Context, l LeaseID) ([]string, error) {
	if err := ctx.Err(); err != nil {
		return nil, ctxErr(err)
	}
	e.cs.mu.Lock()
	defer e.cs.mu.Unlock()
	rec := e.cs.leases[l]
	if rec == nil {
		return nil, newErr(CodeResourceExpired, "directory lease expired")
	}
	for _, p := range rec.c.parts {
		if len(p.treeRoots) > 0 {
			return append([]string{}, p.treeRoots...), nil
		}
	}
	return nil, nil
}

func copyDirectoryView(ctx context.Context, root *os.Root, name string, r io.Reader, size int64) *Error {
	out, err := root.OpenFile(name, os.O_CREATE|os.O_EXCL|os.O_WRONLY, 0600)
	if err != nil {
		return newErr(CodeUnavailable, "cannot materialize directory payload")
	}
	n, err := io.CopyN(out, ctxReader{ctx, r}, size)
	closeErr := out.Close()
	if err != nil || n != size {
		return inputErr(ctx, err)
	}
	if closeErr != nil {
		return newErr(CodeUnavailable, "cannot materialize directory payload")
	}
	return expectEOF(ctx, r)
}

// FolderEntry is one direct child of a listed folder.
type FolderEntry struct {
	Name      string
	Directory bool
	Size      int64
}

// FolderLister is implemented by a host directory source that can list one
// level of the live folder. It never recurses.
type FolderLister interface {
	ListFolder(ctx context.Context, limit int) (entries []FolderEntry, truncated bool, err error)
}

// ListHistoryFolder lists the direct children of a directory-reference record
// as they are now. The read is bounded by limit and never follows links.
func (e *Engine) ListHistoryFolder(ctx context.Context, id HistoryID, limit int) ([]FolderEntry, bool, error) {
	release, gerr := e.historyGate(ctx, false, true)
	if gerr != nil {
		return nil, false, gerr
	}
	defer release()
	c, lerr := e.loadHistoryContent(id)
	if lerr != nil {
		return nil, false, lerr
	}
	defer e.cs.unref(c)
	for _, p := range c.parts {
		if p.desc.MIME != directory.ReferenceMIME {
			continue
		}
		l, ok := p.src.(FolderLister)
		if !ok {
			return nil, false, newErr(CodeSourceChanged, "the folder is unavailable")
		}
		return l.ListFolder(ctx, limit)
	}
	return nil, false, newErr(CodeInvalidArgument, "the record is not a folder reference")
}
