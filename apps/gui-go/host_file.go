package main

import (
	"context"
	"mime"
	"net/http"
	"net/url"
	"os"
	"path/filepath"
	"strconv"
	"strings"
	"sync"
	"time"
)

const (
	hostFileRoute       = "/host-file"
	historyPageSize     = 1000
	historyMaxPages     = 20
	allowedPathsTTL     = 5 * time.Second
	allowedPathsMissTTL = time.Second
	maxPreviewBytes     = 64 << 20
)

// previewTypes are the only file kinds the preview route serves, so it can never
// become a general local-file reader.
var previewTypes = map[string]string{
	".png": "image/png", ".jpg": "image/jpeg", ".jpeg": "image/jpeg", ".gif": "image/gif",
	".webp": "image/webp", ".bmp": "image/bmp", ".avif": "image/avif", ".svg": "image/svg+xml",
}

// knownFiles is the set of local paths the daemon's history refers to. It is the
// authority for what the WebView may preview: the shared frontend turns a history
// entry's file path into a `/host-file` URL, and the host serves it only if the
// daemon itself reported that path (and only while content is unlocked).
type knownFiles struct {
	mu         sync.Mutex
	paths      map[string]struct{}
	builtAt    time.Time
	lastMiss   time.Time
	refreshing bool
}

func (h *HostService) pathKnown(ctx context.Context, path string) (bool, error) {
	k := &h.files
	k.mu.Lock()
	if k.paths != nil && time.Since(k.builtAt) < allowedPathsTTL {
		_, ok := k.paths[path]
		if ok || time.Since(k.lastMiss) < allowedPathsMissTTL {
			k.mu.Unlock()
			return ok, nil
		}
	}
	k.mu.Unlock()

	paths, err := h.loadHistoryFilePaths(ctx)
	if err != nil {
		return false, err
	}
	k.mu.Lock()
	defer k.mu.Unlock()
	k.paths, k.builtAt, k.lastMiss = paths, time.Now(), time.Now()
	_, ok := paths[path]
	return ok, nil
}

// loadHistoryFilePaths lists history entries and collects their `file://` paths.
// A locked profile makes the daemon answer 423, which surfaces as an error here.
func (h *HostService) loadHistoryFilePaths(ctx context.Context) (map[string]struct{}, error) {
	paths := map[string]struct{}{}
	for page := 0; page < historyMaxPages; page++ {
		var entries []struct {
			Preview string `json:"preview"`
		}
		query := "/clipboard/entries?limit=" + strconv.Itoa(historyPageSize) + "&offset=" + strconv.Itoa(page*historyPageSize)
		if err := h.daemon().Get(ctx, query, &entries); err != nil {
			return nil, err
		}
		for _, entry := range entries {
			for _, line := range strings.Split(entry.Preview, "\n") {
				if path, ok := fileURIToPath(line); ok {
					paths[path] = struct{}{}
				}
			}
		}
		if len(entries) < historyPageSize {
			break
		}
	}
	return paths, nil
}

// fileURIToPath decodes a `file://` URI the way the frontend does
// (`fileUriToLocalPath`): percent-decoded path, drive letter without the leading slash.
func fileURIToPath(uri string) (string, bool) {
	uri = strings.TrimSpace(uri)
	if len(uri) < 7 || !strings.EqualFold(uri[:7], "file://") {
		return "", false
	}
	parsed, err := url.Parse(uri)
	if err != nil || parsed.Path == "" {
		return "", false
	}
	path := parsed.Path
	if len(path) >= 3 && path[0] == '/' && path[2] == ':' {
		path = path[1:]
	}
	return path, true
}

// fileMiddleware serves `/host-file?path=…` and passes everything else through.
func (h *HostService) fileMiddleware(next http.Handler) http.Handler {
	return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.URL.Path != hostFileRoute {
			next.ServeHTTP(w, r)
			return
		}
		h.serveFile(w, r)
	})
}

func (h *HostService) serveFile(w http.ResponseWriter, r *http.Request) {
	if r.Method != http.MethodGet && r.Method != http.MethodHead {
		http.Error(w, "method not allowed", http.StatusMethodNotAllowed)
		return
	}
	path := r.URL.Query().Get("path")
	contentType, allowedType := previewTypes[strings.ToLower(filepath.Ext(path))]
	if path == "" || !filepath.IsAbs(path) || filepath.Clean(path) != path || !allowedType {
		http.Error(w, "not found", http.StatusNotFound)
		return
	}
	ctx, cancel := context.WithTimeout(r.Context(), 15*time.Second)
	defer cancel()
	known, err := h.pathKnown(ctx, path)
	if err != nil {
		// Locked or unavailable history must not reveal files.
		http.Error(w, "unavailable", http.StatusServiceUnavailable)
		return
	}
	if !known {
		http.Error(w, "not found", http.StatusNotFound)
		return
	}
	file, err := os.Open(path)
	if err != nil {
		http.Error(w, "not found", http.StatusNotFound)
		return
	}
	defer file.Close()
	info, err := file.Stat()
	if err != nil || !info.Mode().IsRegular() {
		http.Error(w, "not found", http.StatusNotFound)
		return
	}
	if info.Size() > maxPreviewBytes {
		http.Error(w, "too large", http.StatusRequestEntityTooLarge)
		return
	}
	w.Header().Set("Content-Type", mime.FormatMediaType(contentType, nil))
	w.Header().Set("X-Content-Type-Options", "nosniff")
	// An SVG opened as a document must not run script or load anything.
	w.Header().Set("Content-Security-Policy", "default-src 'none'; sandbox")
	w.Header().Set("Cache-Control", "private, max-age=60")
	http.ServeContent(w, r, "", info.ModTime(), file)
}
