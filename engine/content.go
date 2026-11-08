package engine

import (
	"bytes"
	"context"
	"errors"
	"fmt"
	"image/png"
	"io"
	"io/fs"
	"os"
	"path/filepath"
	"strings"
	"sync"
	"syscall"
	"unicode/utf8"

	"github.com/uniclipboard/unic/internal/wire"
)

const (
	contentDirPrefix = "unic-content-"
	tombCap          = 8192
)

// tomb remembers the most recent IDs that were released or freed, so that a
// repeated release is a success and a late use says "expired", not "unknown".
type tomb struct {
	set   map[string]struct{}
	queue []string
}

func (t *tomb) add(id string) {
	if t.set == nil {
		t.set = map[string]struct{}{}
	}
	if _, ok := t.set[id]; ok {
		return
	}
	t.set[id] = struct{}{}
	t.queue = append(t.queue, id)
	if len(t.queue) > tombCap {
		delete(t.set, t.queue[0])
		t.queue = t.queue[1:]
	}
}

func (t *tomb) has(id string) bool { _, ok := t.set[id]; return ok }

type partRec struct {
	desc PartDescriptor
	data []byte     // representation
	path string     // file the engine owns
	src  FileSource // file the host keeps
	meta FileMeta   // what src reported at import
}

// contentObj is stored content. It lives while anything references it: a
// lease, an open reader (and, later, operations and received items).
type contentObj struct {
	id    ContentID
	parts []*partRec
	total int64
	owned int64 // bytes charged to the storage quota
	dir   string
	refs  int
	// blobs are history blobs this content reads in place; they stay on disk
	// until the content is freed even if their record is deleted.
	blobs []string
	hist  *blobTracker
}

type leaseRec struct {
	c *contentObj
}

type contentStore struct {
	mu       sync.Mutex
	contents map[ContentID]*contentObj
	leases   map[LeaseID]*leaseRec
	gone     tomb
	stored   int64
	readers  map[*ContentReader]struct{}
}

func newContentStore() *contentStore {
	return &contentStore{contents: map[ContentID]*contentObj{}, leases: map[LeaseID]*leaseRec{}, readers: map[*ContentReader]struct{}{}}
}

// unrefLocked drops one reference; at zero the content is forgotten and its
// owned directory is returned for the caller to delete outside the lock.
func (cs *contentStore) unrefLocked(c *contentObj) (dir string) {
	c.refs--
	if c.refs > 0 {
		return ""
	}
	delete(cs.contents, c.id)
	cs.gone.add(string(c.id))
	cs.stored -= c.owned
	if len(c.blobs) > 0 {
		c.hist.release(c.blobs) // leaf lock; may unlink doomed blobs
	}
	return c.dir
}

func (cs *contentStore) unref(c *contentObj) {
	cs.mu.Lock()
	dir := cs.unrefLocked(c)
	cs.mu.Unlock()
	if dir != "" {
		_ = os.RemoveAll(dir)
	}
}

func (cs *contentStore) descriptor(c *contentObj) ContentDescriptor {
	d := ContentDescriptor{ID: c.id, TotalSize: c.total, Parts: make([]PartDescriptor, len(c.parts))}
	for i, p := range c.parts {
		d.Parts[i] = p.desc
	}
	return d
}

// ctxReader makes reads stop when ctx ends.
type ctxReader struct {
	ctx context.Context
	r   io.Reader
}

func (c ctxReader) Read(p []byte) (int, error) {
	if err := c.ctx.Err(); err != nil {
		return 0, err
	}
	return c.r.Read(p)
}

// expectEOF checks that r has nothing more to give.
func expectEOF(ctx context.Context, r io.Reader) *Error {
	var b [1]byte
	for i := 0; i < 100; i++ {
		n, err := ctxReader{ctx, r}.Read(b[:])
		if n > 0 {
			return newErr(CodeInvalidArgument, "the input is longer than its declared size")
		}
		if err == io.EOF {
			return nil
		}
		if err != nil {
			return inputErr(ctx, err)
		}
	}
	return newErr(CodeInvalidArgument, "the input does not end")
}

func inputErr(ctx context.Context, err error) *Error {
	if ctx.Err() != nil {
		return ctxErr(ctx.Err())
	}
	if errors.Is(err, io.EOF) || errors.Is(err, io.ErrUnexpectedEOF) {
		return newErr(CodeInvalidArgument, "the input is shorter than its declared size")
	}
	return newErr(CodeInvalidArgument, "the input could not be read")
}

// sourceErr classifies a FileSource failure without forwarding its text.
func sourceErr(ctx context.Context, err error) *Error {
	var ee *Error
	switch {
	case errors.As(err, &ee):
		return ee
	case ctx.Err() != nil:
		return ctxErr(ctx.Err())
	case errors.Is(err, fs.ErrNotExist):
		return newErr(CodeSourceChanged, "the file is gone")
	case errors.Is(err, fs.ErrPermission):
		return newErr(CodeSourceChanged, "the file can no longer be opened (permission)")
	}
	return newErr(CodeUnavailable, "cannot open the file")
}

type writeErr struct {
	w   io.Writer
	err error
}

func (w *writeErr) Write(p []byte) (int, error) {
	n, err := w.w.Write(p)
	if err != nil {
		w.err = err
	}
	return n, err
}

func (e *Engine) newID() string { return string(e.instance) + "-" + randHex(8) }

// foreign reports whether id was made by another instance.
func (e *Engine) foreign(id string) bool { return !strings.HasPrefix(id, string(e.instance)+"-") }

// ImportContent stores content so it can be sent. Representations are copied
// into memory; files are copied (Reader) or kept where they are (Source). It
// returns a lease the caller must release. On any error nothing is left
// behind.
func (e *Engine) ImportContent(ctx context.Context, in ContentInput) (ContentHandle, error) {
	release, gerr := e.enter(ctx)
	if gerr != nil {
		return ContentHandle{}, gerr
	}
	defer release()
	e.mu.Lock()
	lim := e.settings.Value.Limits
	e.mu.Unlock()

	if len(in.Representations)+len(in.Files) == 0 {
		return ContentHandle{}, newErr(CodeInvalidArgument, "the content is empty")
	}
	if len(in.Representations)+len(in.Files) > wire.MaxParts {
		return ContentHandle{}, newErr(CodeLimitExceeded, "too many parts")
	}
	var total, owned int64
	seen := map[string]bool{}
	mimes := make([]string, len(in.Representations))
	for i, r := range in.Representations {
		m := strings.ToLower(strings.TrimSpace(r.MIME))
		switch {
		case !contains(coreFormats, m):
			return ContentHandle{}, newErr(CodeUnsupported, "unsupported format")
		case seen[m]:
			return ContentHandle{}, newErr(CodeInvalidArgument, "a format appears twice")
		case r.Reader == nil || r.Size <= 0:
			return ContentHandle{}, newErr(CodeInvalidArgument, "a representation is empty")
		case r.Size > wire.MaxPartBytes:
			return ContentHandle{}, newErr(CodeLimitExceeded, "a representation is over the size limit")
		}
		seen[m], mimes[i] = true, m
		total += r.Size
		owned += r.Size
	}
	names := make([]string, len(in.Files))
	for i, f := range in.Files {
		switch {
		case strings.TrimSpace(f.Name) == "" || len(f.Name) > 255:
			return ContentHandle{}, newErr(CodeInvalidArgument, "a file needs a name of at most 255 bytes")
		case (f.Reader == nil) == (f.Source == nil):
			return ContentHandle{}, newErr(CodeInvalidArgument, "give a file either a Reader or a Source")
		case f.Size < 0:
			return ContentHandle{}, newErr(CodeInvalidArgument, "negative file size")
		case f.Size > wire.MaxFileBytes || f.Size > lim.MaxFileBytes:
			return ContentHandle{}, newErr(CodeLimitExceeded, "a file is over the size limit")
		}
		names[i] = wire.SafeName(f.Name)
		total += f.Size
		if f.Reader != nil {
			owned += f.Size
		}
	}
	if total > wire.MaxTotal || total > lim.MaxTotalBytes {
		return ContentHandle{}, newErr(CodeLimitExceeded, "the content is over the size limit")
	}

	cs := e.cs
	if !e.reserveStored(owned, false) { // history may give way, received entries are not touched
		return ContentHandle{}, retryable(newErr(CodeResourceExhausted, "not enough storage for this content"))
	}

	c := &contentObj{id: ContentID(e.newID()), total: total, owned: owned, refs: 1}
	ok := false
	defer func() {
		if !ok {
			cs.mu.Lock()
			cs.stored -= owned
			cs.mu.Unlock()
			if c.dir != "" {
				_ = os.RemoveAll(c.dir)
			}
		}
	}()

	n := 0
	nextPart := func() PartID { n++; return PartID(fmt.Sprintf("p%d", n)) }
	for i, r := range in.Representations {
		buf := make([]byte, r.Size)
		if _, err := io.ReadFull(ctxReader{ctx, r.Reader}, buf); err != nil {
			return ContentHandle{}, inputErr(ctx, err)
		}
		if eerr := expectEOF(ctx, r.Reader); eerr != nil {
			return ContentHandle{}, eerr
		}
		switch mimes[i] {
		case MimeText, MimeHTML:
			if !utf8.Valid(buf) {
				return ContentHandle{}, newErr(CodeInvalidArgument, "text is not valid UTF-8")
			}
		case MimePNG:
			if _, err := png.DecodeConfig(bytes.NewReader(buf)); err != nil {
				return ContentHandle{}, newErr(CodeInvalidArgument, "the image is not a valid PNG")
			}
		}
		c.parts = append(c.parts, &partRec{data: buf, desc: PartDescriptor{ID: nextPart(), Kind: PartRepresentation, MIME: mimes[i], Size: r.Size}})
	}
	for i, f := range in.Files {
		mime := strings.ToLower(strings.TrimSpace(f.MIME))
		if mime == "" {
			mime = "application/octet-stream"
		}
		p := &partRec{desc: PartDescriptor{ID: nextPart(), Kind: PartFile, MIME: mime, Name: names[i], Size: f.Size}}
		if f.Source != nil {
			rc, meta, err := f.Source.Open(ctx)
			if err != nil {
				return ContentHandle{}, sourceErr(ctx, err)
			}
			_ = rc.Close()
			if meta.Size != f.Size {
				return ContentHandle{}, newErr(CodeInvalidArgument, "a file's size differs from the declared size")
			}
			p.src, p.meta, p.desc.Referenced = f.Source, meta, true
		} else {
			if c.dir == "" {
				if err := os.MkdirAll(e.opts.InboxDir, 0o700); err != nil {
					return ContentHandle{}, newErr(CodeUnavailable, "cannot create the storage directory")
				}
				c.dir = filepath.Join(e.opts.InboxDir, contentDirPrefix+randHex(8))
				if err := os.Mkdir(c.dir, 0o700); err != nil {
					c.dir = ""
					return ContentHandle{}, newErr(CodeUnavailable, "cannot create the storage directory")
				}
			}
			sub := filepath.Join(c.dir, fmt.Sprintf("f%d", i))
			if err := os.Mkdir(sub, 0o700); err != nil {
				return ContentHandle{}, newErr(CodeUnavailable, "cannot store the file")
			}
			path := filepath.Join(sub, names[i])
			if eerr := e.copyIn(ctx, path, f.Reader, f.Size); eerr != nil {
				return ContentHandle{}, eerr
			}
			p.path = path
		}
		c.parts = append(c.parts, p)
	}

	cs.mu.Lock()
	lease := LeaseID(e.newID())
	cs.contents[c.id] = c
	cs.leases[lease] = &leaseRec{c: c}
	h := ContentHandle{Content: cs.descriptor(c), LeaseID: lease}
	cs.mu.Unlock()
	ok = true
	return h, nil
}

// createNewFile makes a file that must not exist yet. It is a variable so that
// tests can make the disk fail.
var createNewFile = func(path string) (io.WriteCloser, error) {
	return os.OpenFile(path, os.O_CREATE|os.O_EXCL|os.O_WRONLY, 0o600)
}

// copyIn writes exactly size bytes of r to a new file at path.
func (e *Engine) copyIn(ctx context.Context, path string, r io.Reader, size int64) *Error {
	f, err := createNewFile(path)
	if err != nil {
		return newErr(CodeUnavailable, "cannot store the file")
	}
	w := &writeErr{w: f}
	_, cerr := io.CopyN(w, ctxReader{ctx, r}, size)
	if clerr := f.Close(); cerr == nil && w.err == nil {
		w.err = clerr
	}
	switch {
	case w.err != nil && errors.Is(w.err, syscall.ENOSPC):
		return retryable(newErr(CodeResourceExhausted, "the disk is full"))
	case w.err != nil:
		return newErr(CodeUnavailable, "cannot store the file")
	case cerr != nil:
		return inputErr(ctx, cerr)
	}
	return expectEOF(ctx, r)
}

// RetainContent gives the caller a new lease on content that still exists, for
// example one held by the received list.
func (e *Engine) RetainContent(ctx context.Context, id ContentID) (ContentHandle, error) {
	release, gerr := e.enter(ctx)
	if gerr != nil {
		return ContentHandle{}, gerr
	}
	defer release()
	cs := e.cs
	cs.mu.Lock()
	defer cs.mu.Unlock()
	c := cs.contents[id]
	if c == nil {
		if e.foreign(string(id)) || cs.gone.has(string(id)) {
			return ContentHandle{}, newErr(CodeResourceExpired, "the content is no longer available")
		}
		return ContentHandle{}, newErr(CodeNotFound, "no such content")
	}
	c.refs++
	lease := LeaseID(e.newID())
	cs.leases[lease] = &leaseRec{c: c}
	return ContentHandle{Content: cs.descriptor(c), LeaseID: lease}, nil
}

// ReleaseContent gives up one lease. It is idempotent per lease. Content is
// deleted when nothing references it any more; transfers and open readers keep
// it alive until they finish.
func (e *Engine) ReleaseContent(ctx context.Context, lease LeaseID) error {
	release, gerr := e.enter(ctx)
	if gerr != nil {
		return gerr
	}
	defer release()
	cs := e.cs
	cs.mu.Lock()
	l := cs.leases[lease]
	if l == nil {
		defer cs.mu.Unlock()
		switch {
		case e.foreign(string(lease)):
			return newErr(CodeResourceExpired, "the lease belongs to an earlier instance")
		case cs.gone.has(string(lease)):
			return nil
		}
		return newErr(CodeNotFound, "no such lease")
	}
	delete(cs.leases, lease)
	cs.gone.add(string(lease))
	dir := cs.unrefLocked(l.c)
	cs.mu.Unlock()
	if dir != "" {
		_ = os.RemoveAll(dir)
	}
	return nil
}

// leased finds the part of a leased content. cs.mu must be held.
func (e *Engine) leasedLocked(lease LeaseID, part PartID) (*contentObj, *partRec, *Error) {
	l := e.cs.leases[lease]
	if l == nil {
		switch {
		case e.foreign(string(lease)) || e.cs.gone.has(string(lease)):
			return nil, nil, newErr(CodeResourceExpired, "the lease is no longer valid")
		}
		return nil, nil, newErr(CodeNotFound, "no such lease")
	}
	for _, p := range l.c.parts {
		if p.desc.ID == part {
			return l.c, p, nil
		}
	}
	return nil, nil, newErr(CodeNotFound, "no such part")
}

// ContentReader streams the bytes of one part. Close it.
type ContentReader struct {
	e    *Engine
	c    *contentObj
	mu   sync.Mutex
	rc   io.ReadCloser
	done bool
	dead bool // the engine closed
}

func (r *ContentReader) Read(p []byte) (int, error) {
	r.mu.Lock()
	dead, done := r.dead, r.done
	r.mu.Unlock()
	switch {
	case dead:
		return 0, errClosed
	case done:
		return 0, io.ErrClosedPipe
	}
	return r.rc.Read(p)
}

// Close releases the reader's hold on the content. It is idempotent.
func (r *ContentReader) Close() error {
	r.mu.Lock()
	if r.done {
		r.mu.Unlock()
		return nil
	}
	r.done = true
	r.mu.Unlock()
	err := r.rc.Close()
	r.e.cs.mu.Lock()
	delete(r.e.cs.readers, r)
	dir := r.e.cs.unrefLocked(r.c)
	r.e.cs.mu.Unlock()
	if dir != "" {
		_ = os.RemoveAll(dir)
	}
	return err
}

func (r *ContentReader) abort() {
	r.mu.Lock()
	r.dead = true
	r.mu.Unlock()
	_ = r.rc.Close()
}

// ReadContent opens one part of leased content for reading. The reader holds
// its own reference, so releasing the lease does not cut a read short. For a
// referenced file the host's file is opened again and checked against what it
// was at import (source_changed if it differs).
func (e *Engine) ReadContent(ctx context.Context, lease LeaseID, part PartID) (*ContentReader, error) {
	release, gerr := e.enter(ctx)
	if gerr != nil {
		return nil, gerr
	}
	defer release()
	cs := e.cs
	cs.mu.Lock()
	c, p, perr := e.leasedLocked(lease, part)
	if perr != nil {
		cs.mu.Unlock()
		return nil, perr
	}
	c.refs++
	cs.mu.Unlock()

	rc, oerr := e.openPart(ctx, p)
	if oerr != nil {
		cs.unref(c)
		return nil, oerr
	}
	r := &ContentReader{e: e, c: c, rc: rc}
	cs.mu.Lock()
	cs.readers[r] = struct{}{}
	cs.mu.Unlock()
	return r, nil
}

func (e *Engine) openPart(ctx context.Context, p *partRec) (io.ReadCloser, *Error) {
	switch {
	case p.data != nil:
		return io.NopCloser(bytes.NewReader(p.data)), nil
	case p.src != nil:
		rc, meta, err := p.src.Open(ctx)
		if err != nil {
			return nil, sourceErr(ctx, err)
		}
		if meta != p.meta {
			_ = rc.Close()
			return nil, newErr(CodeSourceChanged, "the file changed after it was added")
		}
		return rc, nil
	}
	f, err := os.Open(p.path)
	if err != nil {
		return nil, newErr(CodeResourceExpired, "the stored file is gone")
	}
	return f, nil
}

// LocalFilePath returns the absolute path of a part the engine stores itself
// (a received file), so a host can hand it to the system without copying. The
// path is valid while the lease is held; it is host-local data and never
// appears in a DTO, event or log. Referenced files and in-memory parts return
// unsupported: the host already has the former, and reads the latter.
func (e *Engine) LocalFilePath(ctx context.Context, lease LeaseID, part PartID) (string, error) {
	release, gerr := e.enter(ctx)
	if gerr != nil {
		return "", gerr
	}
	defer release()
	e.cs.mu.Lock()
	c, p, perr := e.leasedLocked(lease, part)
	if perr != nil {
		e.cs.mu.Unlock()
		return "", perr
	}
	path, src := p.path, p.src
	if src != nil && path == "" && len(c.blobs) == 0 && p.desc.Referenced {
		// A file history kept by reference: hand the host its own reference
		// back (a path on the desktop), after checking the file is unchanged.
		if ps, ok := src.(PersistentFileSource); ok {
			path = ps.PersistentRef()
		}
	}
	e.cs.mu.Unlock()
	if path == "" {
		return "", newErr(CodeUnsupported, "this part is not a file the engine stores")
	}
	if src != nil {
		rc, oerr := e.openPart(ctx, p)
		if oerr != nil {
			return "", oerr
		}
		_ = rc.Close()
	}
	return path, nil
}

// cleanContentDirs removes what an earlier instance of this data directory
// left in the inbox. Only directories the engine names itself are touched, and
// the directory lock proves nobody is using them.
func cleanContentDirs(inbox string) {
	old, _ := filepath.Glob(filepath.Join(inbox, contentDirPrefix+"*"))
	for _, d := range old {
		_ = os.RemoveAll(d)
	}
	_ = os.RemoveAll(filepath.Join(inbox, recvDirName))
}

// dropContent ends every read and deletes everything the engine stores. Leases
// do not survive Close.
func (e *Engine) dropContent() {
	e.cs.mu.Lock()
	rs := make([]*ContentReader, 0, len(e.cs.readers))
	for r := range e.cs.readers {
		rs = append(rs, r)
	}
	var dirs []string
	for _, c := range e.cs.contents {
		if c.dir != "" {
			dirs = append(dirs, c.dir)
		}
	}
	e.cs.mu.Unlock()
	for _, r := range rs {
		r.abort()
	}
	for _, d := range dirs {
		_ = os.RemoveAll(d)
	}
}
