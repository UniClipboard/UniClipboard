package mobile

import (
	"context"
	"errors"
	"io"
	"mime"
	"os"
	"path/filepath"
	"sort"
	"strings"
	"time"

	"github.com/uniclipboard/unic/engine"
	"github.com/uniclipboard/unic/internal/directory"
	"github.com/uniclipboard/unic/internal/wire"
)

const (
	previewMaxFiles = 120
	previewMaxBytes = 100 << 20
	exportMaxAge    = 24 * time.Hour
)

func (m *Engine) GetFileCacheState() (string, error) {
	state, err := m.e.GetFileCacheState(context.Background())
	if err != nil {
		return "", wrapErr(err)
	}
	return marshalJSON(map[string]int64{
		"stored_bytes": state.StoredBytes, "reserved_bytes": state.ReservedBytes,
	})
}

func (m *Engine) HistoryFileMetadata(historyID string) (string, error) {
	ctx := context.Background()
	handle, err := m.e.RetainHistoryContent(ctx, engine.HistoryID(historyID))
	if err != nil {
		return "", wrapErr(err)
	}
	defer m.e.ReleaseContent(ctx, handle.LeaseID)
	var files []engine.PartDescriptor
	for _, part := range handle.Content.Parts {
		if part.Kind == engine.PartFile {
			files = append(files, part)
		}
	}
	return marshalJSON(files)
}

type mobileFolderEntry struct {
	Path      string `json:"path"`
	Directory bool   `json:"directory"`
	Size      int64  `json:"size"`
}

func (m *Engine) ListHistoryFolder(historyID string, limit int) (string, error) {
	ctx := context.Background()
	id := engine.HistoryID(historyID)
	limit = min(max(limit, 1), 200)
	item, err := m.e.GetItem(ctx, id)
	if err != nil {
		return "", wrapErr(err)
	}
	entries := []mobileFolderEntry{}
	truncated := false
	if item.Kind != engine.HistoryDirectory && item.Kind != engine.HistoryDirectoryReference {
		return "", wrapErr(&engine.Error{Code: engine.CodeInvalidArgument, Message: "not a directory record"})
	}
	if item.Kind == engine.HistoryDirectoryReference {
		children, more, err := m.e.ListHistoryFolder(ctx, id, limit)
		if err != nil {
			return "", wrapErr(err)
		}
		truncated = more
		for _, child := range children {
			entries = append(entries, mobileFolderEntry{child.Name, child.Directory, child.Size})
		}
	} else {
		handle, err := m.e.RetainHistoryContent(ctx, id)
		if err != nil {
			return "", wrapErr(err)
		}
		defer m.e.ReleaseContent(ctx, handle.LeaseID)
		for _, part := range handle.Content.Parts {
			if part.MIME != engine.MimeDirectory {
				continue
			}
			reader, err := m.e.ReadContent(ctx, handle.LeaseID, part.ID)
			if err != nil {
				return "", wrapErr(err)
			}
			defer reader.Close()
			data, err := io.ReadAll(io.LimitReader(reader, directory.MaxManifest+1))
			if err != nil {
				return "", wrapErr(err)
			}
			var sizes []int64
			for _, file := range handle.Content.Parts {
				if file.Kind == engine.PartFile {
					sizes = append(sizes, file.Size)
				}
			}
			manifest, err := directory.Decode(data, sizes)
			if err != nil {
				return "", wrapErr(err)
			}
			for _, entry := range manifest.Entries {
				if len(entries) == limit {
					truncated = true
					break
				}
				path := manifest.Roots[entry.Root].Name
				if entry.Path != "" {
					path += "/" + entry.Path
				}
				entries = append(entries, mobileFolderEntry{path, entry.Directory, entry.Size})
			}
			break
		}
	}
	return marshalJSON(map[string]any{"entries": entries, "truncated": truncated})
}

type mobileExportFile struct {
	Path string `json:"path"`
	Name string `json:"name"`
	MIME string `json:"mime"`
}

func exportExtension(mediaType string) string {
	switch mediaType {
	case "image/jpeg":
		return ".jpg"
	case "image/png":
		return ".png"
	case "image/heic", "image/heif", "image/gif", "image/webp", "image/avif", "image/tiff", "image/bmp":
		return "." + strings.TrimPrefix(mediaType, "image/")
	}
	if extensions, _ := mime.ExtensionsByType(mediaType); len(extensions) != 0 {
		return extensions[0]
	}
	return ".bin"
}

// StageHistoryExport writes the original parts to a private, short-lived staging directory.
func (m *Engine) StageHistoryExport(historyID string, imagesOnly bool) (string, error) {
	ctx := context.Background()
	handle, err := m.e.RetainHistoryContent(ctx, engine.HistoryID(historyID))
	if err != nil {
		return "", wrapErr(err)
	}
	defer m.e.ReleaseContent(ctx, handle.LeaseID)
	root := filepath.Join(m.cacheDir, "exports")
	if err := os.MkdirAll(root, 0700); err != nil {
		return "", wrapErr(err)
	}
	pruneExpiredExports(root)
	dir, err := os.MkdirTemp(root, "share-")
	if err != nil {
		return "", wrapErr(err)
	}
	keep := false
	defer func() {
		if !keep {
			_ = os.RemoveAll(dir)
		}
	}()
	var files []mobileExportFile
	for _, part := range handle.Content.Parts {
		image := strings.HasPrefix(part.MIME, "image/")
		if (imagesOnly && !image) || (!image && part.Kind != engine.PartFile) {
			continue
		}
		name := "UniClipboard" + exportExtension(part.MIME)
		if part.Name != "" {
			name = wire.SafeName(part.Name)
		}
		if filepath.Ext(name) == "" && image {
			name += exportExtension(part.MIME)
		}
		partDir, err := os.MkdirTemp(dir, "part-")
		if err != nil {
			return "", wrapErr(err)
		}
		path := filepath.Join(partDir, name)
		if _, err := m.ExportPartToFile(string(handle.LeaseID), string(part.ID), path); err != nil {
			return "", err
		}
		files = append(files, mobileExportFile{path, name, part.MIME})
	}
	if len(files) == 0 {
		return "", wrapErr(&engine.Error{Code: engine.CodeUnsupported, Message: "no exportable files"})
	}
	out, err := marshalJSON(map[string]any{"directory": dir, "files": files})
	keep = err == nil
	return out, err
}

func pruneExpiredExports(root string) {
	entries, _ := os.ReadDir(root)
	for _, entry := range entries {
		name := entry.Name()
		if !strings.HasPrefix(name, "share-") && !strings.HasPrefix(name, "diagnostics-") {
			continue
		}
		if info, err := entry.Info(); err == nil && time.Since(info.ModTime()) > exportMaxAge {
			_ = os.RemoveAll(filepath.Join(root, name))
		}
	}
}

func (m *Engine) ExportHistoryImagePreview(historyID string) (string, error) {
	m.previewMu.Lock()
	defer m.previewMu.Unlock()
	ctx := context.Background()
	handle, err := m.e.RetainHistoryContent(ctx, engine.HistoryID(historyID))
	if err != nil {
		return "", wrapErr(err)
	}
	defer m.e.ReleaseContent(ctx, handle.LeaseID)
	for _, part := range handle.Content.Parts {
		if !strings.HasPrefix(part.MIME, "image/") {
			continue
		}
		if part.Size > previewMaxBytes {
			return "", wrapErr(&engine.Error{Code: engine.CodeLimitExceeded, Message: "image exceeds preview cache size"})
		}
		dir := filepath.Join(m.cacheDir, "previews")
		path := filepath.Join(dir, historyID+exportExtension(part.MIME))
		if info, err := os.Stat(path); err != nil || info.Size() != part.Size {
			if err := m.prunePreviews(1, part.Size, false); err != nil {
				return "", wrapErr(err)
			}
			if _, err := m.ExportPartToFile(string(handle.LeaseID), string(part.ID), path); err != nil {
				return "", err
			}
		}
		now := time.Now()
		_ = os.Chtimes(path, now, now)
		return marshalJSON(map[string]any{"part": part, "path": path})
	}
	return "", wrapErr(&engine.Error{Code: engine.CodeUnsupported, Message: "no image part"})
}

// ReconcileHistoryPreviews removes orphaned previews and stale export staging.
func (m *Engine) ReconcileHistoryPreviews() error {
	m.previewMu.Lock()
	defer m.previewMu.Unlock()
	pruneExpiredExports(filepath.Join(m.cacheDir, "exports"))
	return wrapErr(m.prunePreviews(0, 0, true))
}

func (m *Engine) ClearHistoryPreviews() error {
	m.previewMu.Lock()
	defer m.previewMu.Unlock()
	return wrapErr(os.RemoveAll(filepath.Join(m.cacheDir, "previews")))
}

func (m *Engine) prunePreviews(reserved int, bytes int64, reconcile bool) error {
	dir := filepath.Join(m.cacheDir, "previews")
	entries, err := os.ReadDir(dir)
	if errors.Is(err, os.ErrNotExist) {
		return nil
	}
	if err != nil {
		return err
	}
	var files []os.FileInfo
	for _, entry := range entries {
		info, err := entry.Info()
		if err != nil || !info.Mode().IsRegular() {
			continue
		}
		name := info.Name()
		if reconcile {
			ext := filepath.Ext(name)
			id := strings.TrimSuffix(name, ext)
			_, err := m.e.GetItem(context.Background(), engine.HistoryID(id))
			var engineErr *engine.Error
			if ext == ".img" || strings.HasPrefix(name, ".unic-part-") ||
				(errors.As(err, &engineErr) && engineErr.Code == engine.CodeNotFound) {
				if err := os.Remove(filepath.Join(dir, name)); err != nil {
					return err
				}
				continue
			}
		}
		files = append(files, info)
		bytes += info.Size()
	}
	sort.Slice(files, func(i, j int) bool { return files[i].ModTime().Before(files[j].ModTime()) })
	count := len(files) + reserved
	for _, info := range files {
		if count <= previewMaxFiles && bytes <= previewMaxBytes {
			break
		}
		if err := os.Remove(filepath.Join(dir, info.Name())); err != nil {
			return err
		}
		count--
		bytes -= info.Size()
	}
	return nil
}
