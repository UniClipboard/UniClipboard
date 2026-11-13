// Package appupdates adapts numbered FlareRelease builds to mygo's signed updater.
package appupdates

import (
	"archive/tar"
	"compress/gzip"
	"context"
	"crypto/ed25519"
	"crypto/sha256"
	"encoding/base64"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"net/http"
	"net/url"
	"os"
	"os/exec"
	"path/filepath"
	"runtime"
	"strconv"
	"strings"
	"sync"
	"time"
)

// PublicKey is supplied by the production packager. Development builds disable updates.
var PublicKey string
var FeedBase = "https://release.uniclipboard.app"

const maxArchive = 512 << 20

type Artifact struct {
	Platform    string `json:"platform"`
	Format      string `json:"format"`
	DownloadURL string `json:"downloadUrl"`
	Size        int64  `json:"size"`
	SHA256      string `json:"sha256"`
	Signature   string `json:"signature"`
	Updater     bool   `json:"updater"`
}

type Source struct {
	Base, Target, Key string
	Channel           func() string
	mu                sync.Mutex
	selected          Artifact
	version           string
	build             int
}

func allowedURL(value string) bool {
	u, e := url.Parse(value)
	return e == nil && u.User == nil && (u.Scheme == "https" || u.Scheme == "http" && (u.Hostname() == "127.0.0.1" || u.Hostname() == "::1"))
}

func get(ctx context.Context, address string) (io.ReadCloser, error) {
	if !allowedURL(address) {
		return nil, errors.New("update URL must use HTTPS")
	}
	req, e := http.NewRequestWithContext(ctx, http.MethodGet, address, nil)
	if e != nil {
		return nil, e
	}
	c := http.Client{Timeout: 2 * time.Minute, CheckRedirect: func(req *http.Request, via []*http.Request) error {
		if len(via) > 5 || !allowedURL(req.URL.String()) {
			return errors.New("unsafe update redirect")
		}
		return nil
	}}
	r, e := c.Do(req)
	if e != nil {
		return nil, e
	}
	if r.StatusCode != http.StatusOK {
		_ = r.Body.Close()
		return nil, fmt.Errorf("update HTTP status %d", r.StatusCode)
	}
	return r.Body, nil
}

func (s *Source) Manifest(ctx context.Context, _ bool) (io.ReadCloser, error) {
	channel := "stable"
	if s.Channel != nil {
		channel = s.Channel()
	}
	if channel != "stable" && channel != "alpha" {
		return nil, errors.New("invalid update channel")
	}
	body, e := get(ctx, strings.TrimRight(s.Base, "/")+"/v1/desktop/"+channel+".json")
	if e != nil {
		return nil, e
	}
	defer func() { _ = body.Close() }()
	data, e := io.ReadAll(io.LimitReader(body, (1<<20)+1))
	if e != nil || len(data) > 1<<20 {
		return nil, errors.New("update response exceeds limit")
	}
	var feed struct {
		Product, Version, BuildNumber, Notes string
		Artifacts                            []Artifact
	}
	if e = json.Unmarshal(data, &feed); e != nil {
		return nil, e
	}
	build, e := strconv.Atoi(feed.BuildNumber)
	if e != nil || build < 1 || build > 999999999 || feed.Product != "desktop" {
		return nil, errors.New("invalid update identity")
	}
	parts := strings.Split(feed.Version, ".")
	if len(parts) != 3 {
		return nil, errors.New("invalid update version")
	}
	for _, part := range parts {
		v, e := strconv.Atoi(part)
		if e != nil || v < 0 || strconv.Itoa(v) != part {
			return nil, errors.New("invalid update version")
		}
	}
	format := "update.tar.gz"
	if strings.HasPrefix(s.Target, "darwin-") {
		format = "app.tar.gz"
	}
	var selected Artifact
	count := 0
	for _, a := range feed.Artifacts {
		if a.Platform == s.Target && a.Updater {
			selected = a
			count++
		}
	}
	sha, e := hex.DecodeString(selected.SHA256)
	if count != 1 || selected.Format != format || selected.Size <= 0 || selected.Size > maxArchive || e != nil || len(sha) != 32 || !allowedURL(selected.DownloadURL) {
		return nil, errors.New("update target or format is unavailable")
	}
	s.mu.Lock()
	s.selected = selected
	s.version = feed.Version
	s.build = build
	s.mu.Unlock()
	m := map[string]any{"version": fmt.Sprintf("%s-build.%d", feed.Version, build), "notes": feed.Notes, "url": selected.DownloadURL, "size": selected.Size, "signature": selected.Signature}
	encoded, e := json.Marshal(m)
	if e != nil {
		return nil, e
	}
	return io.NopCloser(strings.NewReader(string(encoded))), nil
}

type temporaryFile struct {
	*os.File
	dir string
}

func (f *temporaryFile) Close() error {
	e := f.File.Close()
	if remove := os.RemoveAll(f.dir); e == nil {
		e = remove
	}
	return e
}

// Open verifies the signed archive and its embedded identity before mygo handles extraction/swap.
func (s *Source) Open(ctx context.Context, address string) (io.ReadCloser, error) {
	s.mu.Lock()
	a, version, build := s.selected, s.version, s.build
	s.mu.Unlock()
	if address != a.DownloadURL || a.Size <= 0 {
		return nil, errors.New("update archive was not selected")
	}
	key, e := base64.StdEncoding.DecodeString(s.Key)
	if e != nil || len(key) != ed25519.PublicKeySize {
		return nil, errors.New("invalid update key")
	}
	signature, e := base64.StdEncoding.DecodeString(a.Signature)
	if e != nil {
		return nil, errors.New("invalid update signature")
	}
	body, e := get(ctx, address)
	if e != nil {
		return nil, e
	}
	defer func() { _ = body.Close() }()
	dir, e := os.MkdirTemp("", "unic-signed-update-")
	if e != nil {
		return nil, e
	}
	ok := false
	defer func() {
		if !ok {
			_ = os.RemoveAll(dir)
		}
	}()
	path := filepath.Join(dir, "update.tar.gz")
	file, e := os.OpenFile(path, os.O_CREATE|os.O_EXCL|os.O_RDWR, 0600)
	if e != nil {
		return nil, e
	}
	hash := sha256.New()
	n, e := io.Copy(io.MultiWriter(file, hash), io.LimitReader(body, a.Size+1))
	if e != nil || n != a.Size || hex.EncodeToString(hash.Sum(nil)) != a.SHA256 || !ed25519.Verify(key, hash.Sum(nil), signature) {
		_ = file.Close()
		return nil, errors.New("update archive verification failed")
	}
	if _, e = file.Seek(0, io.SeekStart); e != nil {
		_ = file.Close()
		return nil, e
	}
	gz, e := gzip.NewReader(file)
	if e != nil {
		_ = file.Close()
		return nil, e
	}
	tarReader := tar.NewReader(io.LimitReader(gz, 2<<30))
	found := false
	for {
		header, readErr := tarReader.Next()
		if readErr == io.EOF {
			break
		}
		if readErr != nil {
			_ = gz.Close()
			_ = file.Close()
			return nil, readErr
		}
		if filepath.Base(header.Name) != "release-identity.json" {
			continue
		}
		if found || header.Typeflag != tar.TypeReg || header.Size > 16384 {
			_ = gz.Close()
			_ = file.Close()
			return nil, errors.New("invalid signed update identity")
		}
		var identity struct {
			Product, Version, Target string
			BuildNumber              int `json:"build_number"`
		}
		data, err := io.ReadAll(io.LimitReader(tarReader, 16385))
		if err != nil || json.Unmarshal(data, &identity) != nil || identity.Product != "desktop" || identity.Version != version || identity.BuildNumber != build || identity.Target != s.Target {
			_ = gz.Close()
			_ = file.Close()
			return nil, errors.New("signed update identity mismatch")
		}
		found = true
	}
	_ = gz.Close()
	if !found {
		_ = file.Close()
		return nil, errors.New("signed update lacks identity")
	}
	if _, e = file.Seek(0, io.SeekStart); e != nil {
		_ = file.Close()
		return nil, e
	}
	ok = true
	return &temporaryFile{File: file, dir: dir}, nil
}

// Owner identifies installations whose files must stay under their package manager.
func Owner(executable string) string {
	real, e := filepath.EvalSymlinks(executable)
	if e == nil {
		executable = real
	}
	dir := filepath.Dir(executable)
	if os.Getenv("SNAP") != "" {
		return "Snap"
	}
	if os.Getenv("FLATPAK_ID") != "" {
		return "Flatpak"
	}
	if os.Getenv("APPIMAGE") != "" {
		return "AppImage"
	}
	if b, e := os.ReadFile(filepath.Join(dir, "update-owner")); e == nil {
		v := strings.TrimSpace(string(b))
		if v != "" && v != "direct" {
			return v
		}
	}
	if _, e := os.Stat(filepath.Join(dir, "portable.dat")); e == nil {
		return "portable archive"
	}
	if _, e := os.Stat(filepath.Join(dir, "install.json")); e == nil {
		return "Scoop"
	}
	if runtime.GOOS == "linux" && (strings.HasPrefix(executable, "/usr/") || strings.HasPrefix(executable, "/nix/store/") || strings.HasPrefix(executable, "/snap/")) {
		return "system package manager"
	}
	if runtime.GOOS == "darwin" {
		ctx, cancel := context.WithTimeout(context.Background(), 2*time.Second)
		defer cancel()
		bundle := filepath.Dir(filepath.Dir(dir))
		if output, e := exec.CommandContext(ctx, "brew", "list", "--cask", "uniclipboard").Output(); e == nil {
			for _, line := range strings.Split(string(output), "\n") {
				if strings.TrimRight(line, "/") == bundle {
					return "Homebrew"
				}
			}
		}
	}
	return "direct"
}
