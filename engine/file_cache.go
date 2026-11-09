package engine

import (
	"context"
	"fmt"
	"io"
	"os"
	"path/filepath"
	"sort"
	"strings"
	"sync"
	"sync/atomic"
	"time"

	"github.com/uniclipboard/unic/internal/store"
	"github.com/uniclipboard/unic/internal/wire"
)

// fileCache is the only authority for deleting retained received file bytes.
// It never opens or enumerates a custom receive directory for cleanup.
type fileCache struct {
	bytes        atomic.Int64
	mu           sync.Mutex
	root         *os.Root
	dir          string
	identity     os.FileInfo
	st           *store.Store
	files        map[string]store.CachedFile
	held         map[string]int
	startupHeld  map[string]bool
	reserved     int64
	deferStartup bool
}

func openFileCache(directory string, st *store.Store) (*fileCache, error) {
	dir, err := filepath.Abs(directory)
	if err != nil {
		return nil, err
	}
	if info, err := os.Lstat(dir); err == nil && info.Mode()&os.ModeSymlink != 0 {
		return nil, fmt.Errorf("default cache must not use symlinks")
	}
	if err := os.MkdirAll(dir, 0700); err != nil {
		return nil, err
	}
	resolved, err := filepath.EvalSymlinks(dir)
	if err != nil {
		return nil, err
	}
	abs, err := filepath.Abs(dir)
	if err != nil || resolved != abs {
		return nil, fmt.Errorf("default cache must not use symlinks")
	}
	root, err := os.OpenRoot(dir)
	if err != nil {
		return nil, err
	}
	info, err := os.Stat(dir)
	if err != nil {
		_ = root.Close()
		return nil, err
	}
	c := &fileCache{root: root, dir: dir, identity: info, st: st, files: map[string]store.CachedFile{}, held: map[string]int{}, startupHeld: map[string]bool{}}
	rows, err := st.CachedFiles()
	if err != nil {
		_ = root.Close()
		return nil, err
	}
	for _, f := range rows {
		if len(f.ID) != 32 || strings.Trim(f.ID, "0123456789abcdef") != "" || f.Name != wire.SafeName(f.Name) {
			_ = root.Close()
			return nil, fmt.Errorf("invalid cache identity")
		}
		c.files[f.ID] = f
		c.bytes.Add(f.Size)
	}
	return c, nil
}
func (c *fileCache) path(f store.CachedFile) string { return filepath.Join(c.dir, f.ID, f.Name) }
func (c *fileCache) safe() bool {
	info, err := os.Lstat(c.dir)
	return err == nil && info.IsDir() && info.Mode()&os.ModeSymlink == 0 && os.SameFile(info, c.identity)
}
func (c *fileCache) hold(id string) (string, bool) {
	c.mu.Lock()
	defer c.mu.Unlock()
	f, ok := c.files[id]
	if !ok || !f.Available || !c.safe() {
		return "", false
	}
	info, err := c.root.Stat(filepath.Join(id, f.Name))
	if err != nil || !info.Mode().IsRegular() || info.Size() != f.Size {
		return "", false
	}
	c.held[id]++
	return c.path(f), true
}
func (c *fileCache) release(ids []string) {
	c.mu.Lock()
	defer c.mu.Unlock()
	for _, id := range ids {
		c.held[id]--
		if c.held[id] <= 0 {
			delete(c.held, id)
		}
	}
}
func (c *fileCache) available(id string) bool {
	c.mu.Lock()
	defer c.mu.Unlock()
	f, ok := c.files[id]
	if !ok || !f.Available || !c.safe() {
		return false
	}
	info, err := c.root.Stat(filepath.Join(id, f.Name))
	return err == nil && info.Mode().IsRegular() && info.Size() == f.Size
}

// reserve counts each retained byte and each pending byte once. Unlink failures stay charged.
func (c *fileCache) reserve(n int64, policy FileCacheSettings, expiry bool) (bool, []string) {
	c.mu.Lock()
	defer c.mu.Unlock()
	if policy.MaxBytes == 0 {
		policy = DefaultSettings().FileCache
	}
	if n > policy.MaxBytes || !c.safe() || (n > 0 && c.deferStartup) {
		return false, nil
	}
	rows, err := c.st.CachedFiles()
	if err != nil {
		return false, nil
	}
	used := c.reserved
	for _, f := range rows {
		used += f.Size
	}
	var changed []string
	now := time.Now().UnixMilli()
	for _, f := range rows {
		old := expiry && policy.AutoCleanup && now-f.CompletedMs >= policy.RetentionHours*int64(time.Hour/time.Millisecond)
		if !old && f.Available && used+n <= policy.MaxBytes {
			continue
		}
		if c.held[f.ID] > 0 || c.startupHeld[f.ID] {
			continue
		}
		ids, err := c.st.MarkCachedFileUnavailable(f.ID)
		if err != nil {
			continue
		}
		changed = append(changed, ids...)
		f.Available = false
		c.files[f.ID] = f
		// Root.RemoveAll confines symlink resolution to the already-open default root.
		if !c.safe() {
			continue
		}
		if err = c.root.RemoveAll(f.ID); err != nil {
			continue
		}
		if err = c.st.ForgetCachedFile(f.ID); err != nil {
			continue
		}
		delete(c.files, f.ID)
		c.bytes.Add(-f.Size)
		used -= f.Size
	}
	if used+n > policy.MaxBytes {
		return false, changed
	}
	c.reserved += n
	return true, changed
}
func (c *fileCache) cancel(n int64) { c.mu.Lock(); c.reserved -= n; c.mu.Unlock() }

// promote takes a verified received file into the default cache and pins it.
func (c *fileCache) promote(source, name string, size int64) (string, string, error) {
	c.mu.Lock()
	defer c.mu.Unlock()
	if !c.safe() {
		return "", "", fmt.Errorf("default cache identity changed")
	}
	id := randHex(16)
	name = wire.SafeName(name)
	f := store.CachedFile{ID: id, Name: name, Size: size, CompletedMs: time.Now().UnixMilli(), Available: false}
	// Journal ownership before creating a path. A crash leaves a charged tombstone, not an untracked directory.
	if err := c.st.PutCachedFile(f); err != nil {
		return "", "", err
	}
	c.files[id] = f
	c.bytes.Add(size)
	c.reserved -= size
	fail := func(err error) (string, string, error) {
		if c.safe() && c.root.RemoveAll(id) == nil && c.st.ForgetCachedFile(id) == nil {
			delete(c.files, id)
			c.bytes.Add(-size)
		}
		return id, "", err
	}
	if err := c.root.Mkdir(id, 0700); err != nil {
		return fail(err)
	}
	out, err := c.root.OpenFile(filepath.Join(id, name), os.O_CREATE|os.O_EXCL|os.O_WRONLY, 0600)
	if err != nil {
		return fail(err)
	}
	in, err := os.Open(source)
	if err == nil {
		_, err = io.CopyN(out, in, size)
		_ = in.Close()
	}
	if err == nil {
		err = out.Sync()
	}
	if cerr := out.Close(); err == nil {
		err = cerr
	}
	if err != nil {
		return fail(err)
	}
	for _, path := range []string{id, "."} {
		if !c.safe() {
			return fail(fmt.Errorf("default cache identity changed"))
		}
		if err := syncImportDirectory(filepath.Join(c.dir, path)); err != nil {
			return fail(err)
		}
	}
	f.CompletedMs = time.Now().UnixMilli()
	if err = c.st.PublishCachedFile(id, f.CompletedMs); err != nil {
		return fail(err)
	}
	f.Available = true
	c.files[id] = f
	c.held[id]++
	return id, c.path(f), nil
}

// finalizeReceivedFiles retains Engine-owned bytes or adopts a host-owned source.
// Host callbacks run without Engine state, history, or content locks.
func (e *Engine) finalizeReceivedFiles(ctx context.Context, c *contentObj) *Error {
	for _, p := range c.parts {
		if p.desc.Kind != PartFile {
			continue
		}
		size := p.desc.Size
		if e.opts.StoreReceivedFile == nil {
			id, path, err := e.cache.promote(p.path, p.desc.Name, size)
			p.cache = id
			if err != nil {
				return newErr(CodeUnavailable, "cannot retain received file")
			}
			p.path = path
			c.cached = append(c.cached, id)
		} else {
			source, meta, err := e.storeReceivedFile(ctx, p)
			if err != nil {
				return err
			}
			p.src, p.meta, p.path, p.desc.Referenced = source, meta, "", true
		}
		c.owned -= size
	}
	return nil
}

func (e *Engine) storeReceivedFile(ctx context.Context, p *partRec) (source PersistentFileSource, meta FileMeta, failure *Error) {
	// Host code can fail; no host error text, reference, path, or panic escapes.
	defer func() {
		if recover() != nil {
			source = nil
			failure = newErr(CodeUnavailable, "host file storage failed")
		}
	}()
	reader, err := e.openPart(ctx, p)
	if err != nil {
		return nil, meta, err
	}
	defer func() { _ = reader.Close() }()
	limited := &io.LimitedReader{R: reader, N: p.desc.Size}
	stored, hostErr := e.opts.StoreReceivedFile(ctx, ReceivedFile{Name: p.desc.Name, MIME: p.desc.MIME, Size: p.desc.Size, Reader: limited})
	if hostErr == nil && limited.N != 0 {
		return nil, meta, newErr(CodeInvalidArgument, "host file storage did not consume the received file")
	}
	if hostErr != nil || stored == nil {
		return nil, meta, newErr(CodeUnavailable, "host file storage failed")
	}
	ref := stored.PersistentRef()
	if ref == "" || len(ref) > 65536 || strings.HasPrefix(ref, "cache:") {
		return nil, meta, newErr(CodeInvalidArgument, "host file storage returned an invalid reference")
	}
	opened, meta, hostErr := stored.Open(ctx)
	if hostErr != nil || opened == nil {
		return nil, meta, newErr(CodeUnavailable, "host file storage returned unreadable content")
	}
	_ = opened.Close()
	if meta.Size != p.desc.Size {
		return nil, meta, newErr(CodeInvalidArgument, "host file storage returned the wrong size")
	}
	if ctx.Err() != nil {
		return nil, meta, ctxErr(ctx.Err())
	}
	return stored, meta, nil
}

func (e *Engine) cleanupFileCache(expiry bool) {
	e.mu.Lock()
	policy := e.settings.Value.FileCache
	e.mu.Unlock()
	_, ids := e.cache.reserve(0, policy, expiry)
	if len(ids) > 0 {
		e.historyChanged(ids...)
	}
}

// FileCacheState contains no paths or filenames. Custom received files are outside this ledger.
type FileCacheState struct {
	Scope         string           `json:"scope"`
	StoredBytes   int64            `json:"stored_bytes"`
	ReservedBytes int64            `json:"reserved_bytes"`
	Files         []CacheFileState `json:"files"`
}
type CacheFileState struct {
	ID          string `json:"id"`
	Size        int64  `json:"size"`
	CompletedMs int64  `json:"completed_ms"`
	Available   bool   `json:"available"`
	Held        bool   `json:"held"`
}

func (e *Engine) GetFileCacheState(ctx context.Context) (FileCacheState, error) {
	release, err := e.enter(ctx)
	if err != nil {
		return FileCacheState{}, err
	}
	defer release()
	c := e.cache
	c.mu.Lock()
	defer c.mu.Unlock()
	out := FileCacheState{Scope: "default_directory_only", StoredBytes: c.bytes.Load(), ReservedBytes: c.reserved, Files: []CacheFileState{}}
	for id, f := range c.files {
		out.Files = append(out.Files, CacheFileState{ID: id, Size: f.Size, CompletedMs: f.CompletedMs, Available: f.Available, Held: c.held[id] > 0 || c.startupHeld[id]})
	}
	sort.Slice(out.Files, func(i, j int) bool { return out.Files[i].ID < out.Files[j].ID })
	return out, nil
}

// ReconcileClipboardFiles pins only default-cache paths currently on the clipboard.
// A host must call this after each successful clipboard read, before reporting its observation.
func (e *Engine) ReconcileClipboardFiles(ctx context.Context, paths []string) error {
	release, err := e.enter(ctx)
	if err != nil {
		return err
	}
	defer release()
	c := e.cache
	if c == nil {
		return nil
	}
	c.mu.Lock()
	clear(c.startupHeld)
	for id, f := range c.files {
		for _, p := range paths {
			if p == c.path(f) {
				c.startupHeld[id] = true
			}
		}
	}
	startup := c.deferStartup
	c.deferStartup = false
	c.mu.Unlock()
	if startup {
		e.mu.Lock()
		policy := e.settings.Value.FileCache
		e.mu.Unlock()
		_, ids := c.reserve(0, policy, true)
		if len(ids) > 0 {
			e.historyChanged(ids...)
		}
	}
	return nil
}

func (e *Engine) cachedBytes() int64 {
	if e.cache == nil {
		return 0
	}
	return e.cache.bytes.Load()
}

func (c *fileCache) pinPath(path string) (string, bool) {
	c.mu.Lock()
	defer c.mu.Unlock()
	for id, f := range c.files {
		if c.path(f) == path {
			if !f.Available || !c.safe() {
				return "", false
			}
			c.held[id]++
			return id, true
		}
	}
	return "", false
}
