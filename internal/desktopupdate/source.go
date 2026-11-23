// Package desktopupdate checks numbered release feeds and verifies update archives.
package desktopupdate

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
	"path/filepath"
	"strconv"
	"strings"
	"sync"
	"time"

	"github.com/uniclipboard/unic/internal/releaseinfo"
)

const FeedBase = "https://release.uniclipboard.app"

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
	CurrentVersion    string
	Channel           func() string
	Language          func() string
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
	req.Header.Set("User-Agent", "UniClipboard/"+releaseinfo.Version)
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
	address := strings.TrimRight(s.Base, "/") + "/v1/desktop/" + channel + ".json"
	if s.CurrentVersion != "" {
		address += "?from=" + url.QueryEscape(s.CurrentVersion)
	}
	body, e := get(ctx, address)
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
	language := "en"
	if s.Language != nil {
		language = s.Language()
	}
	m := map[string]any{"version": fmt.Sprintf("%s-build.%d", feed.Version, build), "notes": localizedNotes(feed.Notes, language), "url": selected.DownloadURL, "size": selected.Size, "signature": selected.Signature, "sha256": selected.SHA256}
	encoded, e := json.Marshal(m)
	if e != nil {
		return nil, e
	}
	return io.NopCloser(strings.NewReader(string(encoded))), nil
}

func localizedNotes(notes, language string) string {
	english, chinese, _ := strings.Cut(notes, "<!-- zh -->")
	primary, _, _ := strings.Cut(language, "-")
	if strings.EqualFold(primary, "zh") && strings.TrimSpace(chinese) != "" {
		return strings.TrimSpace(chinese)
	}
	return strings.TrimSpace(english)
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
	body, e := get(ctx, address)
	if e != nil {
		return nil, e
	}
	defer func() { _ = body.Close() }()
	file, e := copyVerified(body, a, s.Key)
	if e != nil {
		return nil, e
	}
	ok := false
	defer func() {
		if !ok {
			_ = file.Close()
		}
	}()
	gz, e := gzip.NewReader(file)
	if e != nil {
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
			return nil, readErr
		}
		if filepath.Base(header.Name) != "release-identity.json" {
			continue
		}
		if found || header.Typeflag != tar.TypeReg || header.Size > 16384 {
			_ = gz.Close()
			return nil, errors.New("invalid signed update identity")
		}
		var identity struct {
			Product, Version, Target string
			BuildNumber              int `json:"build_number"`
		}
		data, err := io.ReadAll(io.LimitReader(tarReader, 16385))
		if err != nil || json.Unmarshal(data, &identity) != nil || identity.Product != "desktop" || identity.Version != version || identity.BuildNumber != build || identity.Target != s.Target {
			_ = gz.Close()
			return nil, errors.New("signed update identity mismatch")
		}
		found = true
	}
	_ = gz.Close()
	if !found {
		return nil, errors.New("signed update lacks identity")
	}
	if _, e = file.Seek(0, io.SeekStart); e != nil {
		return nil, e
	}
	ok = true
	return file, nil
}

// copyVerified stages the stream and verifies its size, digest and signature.
func copyVerified(body io.Reader, a Artifact, publicKey string) (*temporaryFile, error) {
	if a.Size <= 0 || a.Size > maxArchive {
		return nil, errors.New("invalid update archive size")
	}
	key, err := base64.StdEncoding.DecodeString(publicKey)
	if err != nil || len(key) != ed25519.PublicKeySize {
		return nil, errors.New("invalid update key")
	}
	signature, err := base64.StdEncoding.DecodeString(a.Signature)
	if err != nil || len(signature) != ed25519.SignatureSize {
		return nil, errors.New("invalid update signature")
	}
	dir, err := os.MkdirTemp("", "unic-signed-update-")
	if err != nil {
		return nil, err
	}
	file, err := os.OpenFile(filepath.Join(dir, "update.tar.gz"), os.O_CREATE|os.O_EXCL|os.O_RDWR, 0600)
	if err != nil {
		_ = os.RemoveAll(dir)
		return nil, err
	}
	staged := &temporaryFile{File: file, dir: dir}
	hash := sha256.New()
	n, err := io.Copy(io.MultiWriter(file, hash), io.LimitReader(body, a.Size+1))
	if err != nil || n != a.Size || hex.EncodeToString(hash.Sum(nil)) != a.SHA256 || !ed25519.Verify(key, hash.Sum(nil), signature) {
		_ = staged.Close()
		return nil, errors.New("update archive verification failed")
	}
	if _, err := file.Seek(0, io.SeekStart); err != nil {
		_ = staged.Close()
		return nil, err
	}
	return staged, nil
}
