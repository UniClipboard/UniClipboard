package luascripts

import (
	"archive/tar"
	"compress/gzip"
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"net/http"
	"net/url"
	"os"
	"path"
	"path/filepath"
	"regexp"
	"strings"
	"time"
)

const (
	archiveLimit = 128 << 20
	// DefaultMirrorURL is the official static plugin marketplace mirror.
	DefaultMirrorURL = "https://plugins.uniclipboard.app"
)

var githubPath = regexp.MustCompile(`^/[A-Za-z0-9-]+/[A-Za-z0-9_.-]+$`)
var errAssetMissing = errors.New("GitHub asset not found")

type githubRepo struct {
	location string
	slug     string
	mirror   *url.URL
}

func openGitHub(location string) (githubRepo, error) {
	u, err := url.Parse(location)
	if err != nil || u.Scheme != "https" || !strings.EqualFold(u.Host, "github.com") || u.User != nil ||
		u.RawPath != "" || u.RawQuery != "" || u.ForceQuery || u.Fragment != "" || !githubPath.MatchString(u.Path) {
		return githubRepo{}, errors.New("expected a public GitHub repository URL")
	}
	slug := strings.ToLower(strings.TrimSuffix(u.Path, ".git"))
	if name := path.Base(slug); name == "." || name == ".." || name == "/" {
		return githubRepo{}, errors.New("invalid GitHub repository name")
	}
	return githubRepo{location: "https://github.com" + slug, slug: slug}, nil
}

func isLoopbackHost(host string) bool {
	return host == "127.0.0.1" || host == "localhost" || host == "::1"
}

func parseMirrorURL(raw string) (*url.URL, error) {
	trimmed := strings.TrimSpace(raw)
	if trimmed == "" || strings.EqualFold(trimmed, "off") || strings.EqualFold(trimmed, "none") {
		return nil, nil
	}
	if len(trimmed) > 256 || strings.ContainsAny(trimmed, " \t\r\n\x00") {
		return nil, errors.New("invalid plugin mirror URL")
	}
	u, err := url.Parse(trimmed)
	if err != nil || u.Host == "" || u.User != nil || u.RawQuery != "" || u.ForceQuery || u.Fragment != "" {
		return nil, errors.New("invalid plugin mirror URL")
	}
	if u.Scheme != "https" && (u.Scheme != "http" || !isLoopbackHost(u.Hostname())) {
		return nil, errors.New("plugin mirror URL must use https")
	}
	u.Path = strings.TrimRight(u.Path, "/")
	u.RawPath = ""
	return u, nil
}

func resolveMirrorURL(configured string) (*url.URL, error) {
	if env, ok := os.LookupEnv("UNIC_PLUGIN_MIRROR_URL"); ok {
		return parseMirrorURL(env)
	}
	return parseMirrorURL(configured)
}

var githubHTTP = &http.Client{
	Timeout: 2 * time.Minute,
	CheckRedirect: func(req *http.Request, via []*http.Request) error {
		host := req.URL.Host
		if len(via) >= 5 || req.URL.Scheme != "https" || req.URL.User != nil ||
			(host != "api.github.com" && host != "github.com" && host != "codeload.github.com" && host != "release-assets.githubusercontent.com" && host != "objects.githubusercontent.com") {
			return errors.New("unexpected GitHub redirect")
		}
		return nil
	},
}

func (g githubRepo) mirrorClient() *http.Client {
	mirror := g.mirror
	return &http.Client{
		Timeout: 2 * time.Minute,
		CheckRedirect: func(req *http.Request, via []*http.Request) error {
			if len(via) >= 5 || req.URL.User != nil || req.URL.Scheme != mirror.Scheme || !strings.EqualFold(req.URL.Host, mirror.Host) {
				return errors.New("unexpected mirror redirect")
			}
			return nil
		},
	}
}

func (g githubRepo) mirrorTarget(githubURL string) (string, bool) {
	if g.mirror == nil {
		return "", false
	}
	u, err := url.Parse(githubURL)
	if err != nil || u.Scheme != "https" {
		return "", false
	}
	host := strings.ToLower(u.Host)
	if host != "api.github.com" && host != "codeload.github.com" && host != "github.com" {
		return "", false
	}
	return g.mirror.Scheme + "://" + g.mirror.Host + g.mirror.Path + "/" + host + u.EscapedPath(), true
}

func shouldFallbackFromMirror(status int) bool {
	return status == 0 || status == http.StatusNotFound || status == http.StatusTooManyRequests || status >= 500
}

func fetchWithClient(ctx context.Context, client *http.Client, target string, limit int64, githubAccept bool) (io.ReadCloser, int, error) {
	req, err := http.NewRequestWithContext(ctx, http.MethodGet, target, nil)
	if err != nil {
		return nil, 0, errors.New("invalid GitHub request")
	}
	if githubAccept {
		req.Header.Set("Accept", "application/vnd.github+json")
	}
	res, err := client.Do(req)
	if err != nil {
		if ctx.Err() != nil {
			return nil, 0, ctx.Err()
		}
		return nil, 0, errors.New("GitHub request failed")
	}
	if res.StatusCode != http.StatusOK || res.ContentLength > limit {
		status := res.StatusCode
		_ = res.Body.Close()
		if status == http.StatusNotFound {
			return nil, status, errAssetMissing
		}
		return nil, status, fmt.Errorf("GitHub response rejected (HTTP %d)", status)
	}
	return struct {
		io.Reader
		io.Closer
	}{io.LimitReader(res.Body, limit+1), res.Body}, http.StatusOK, nil
}

func (g githubRepo) fetch(ctx context.Context, target string, limit int64) (io.ReadCloser, error) {
	if mirrorURL, ok := g.mirrorTarget(target); ok {
		body, status, err := fetchWithClient(ctx, g.mirrorClient(), mirrorURL, limit, false)
		if err == nil {
			return body, nil
		}
		if ctx.Err() != nil || !shouldFallbackFromMirror(status) {
			return nil, err
		}
	}
	body, _, err := fetchWithClient(ctx, githubHTTP, target, limit, true)
	return body, err
}

func (g githubRepo) commit(ctx context.Context, ref string) (string, error) {
	if ref == "" {
		ref = "HEAD"
	}
	if len(ref) > 256 || strings.ContainsAny(ref, "\x00\r\n") || ref == "." || ref == ".." {
		return "", errors.New("invalid GitHub ref")
	}
	if commitPattern.MatchString(ref) {
		return ref, nil
	}
	const limit = 1 << 20
	body, err := g.fetch(ctx, "https://api.github.com/repos"+g.slug+"/commits/"+url.PathEscape(ref), limit)
	if err != nil {
		return "", fmt.Errorf("could not resolve GitHub ref: %w", err)
	}
	defer func() { _ = body.Close() }()
	raw, err := io.ReadAll(body)
	if err != nil && ctx.Err() != nil {
		return "", ctx.Err()
	}
	var commit struct{ SHA string }
	if err != nil || len(raw) > limit || json.Unmarshal(raw, &commit) != nil || !commitPattern.MatchString(commit.SHA) {
		return "", errors.New("invalid GitHub commit response")
	}
	return commit.SHA, nil
}

func (g githubRepo) source(ctx context.Context, commit, dest string) error {
	body, err := g.fetch(ctx, "https://codeload.github.com"+g.slug+"/tar.gz/"+commit, archiveLimit)
	if err != nil {
		return err
	}
	defer func() { _ = body.Close() }()
	return extractTarGzip(ctx, body, dest, true)
}

func extractTarGzip(ctx context.Context, body io.Reader, dest string, stripRoot bool) error {
	if err := ctx.Err(); err != nil {
		return err
	}
	z, err := gzip.NewReader(body)
	if err != nil {
		if ctx.Err() != nil {
			return ctx.Err()
		}
		return errors.New("invalid plugin gzip archive")
	}
	defer func() { _ = z.Close() }()
	limited := &io.LimitedReader{R: z, N: archiveLimit + 1}
	r := tar.NewReader(limited)
	if err := os.MkdirAll(dest, 0700); err != nil {
		return err
	}
	var archiveRoot string
	for count := 0; ; count++ {
		if err := ctx.Err(); err != nil {
			return err
		}
		h, err := r.Next()
		if errors.Is(err, io.EOF) {
			_, err := io.Copy(io.Discard, limited)
			if err != nil && ctx.Err() != nil {
				return ctx.Err()
			}
			if err != nil || limited.N == 0 || count == 0 || (stripRoot && archiveRoot == "") {
				return errors.New("invalid or oversized plugin archive")
			}
			return nil
		}
		if err != nil {
			if ctx.Err() != nil {
				return ctx.Err()
			}
			return errors.New("invalid or oversized plugin archive")
		}
		if count >= 10000 || h.Size < 0 || h.Size > archiveLimit || len(h.Name) > 1024 {
			return errors.New("invalid or oversized plugin archive")
		}
		// GitHub codeload emits a global PAX metadata entry before its source
		// directory. It is not a filesystem entry and must not select the root.
		if h.Typeflag == tar.TypeXGlobalHeader {
			continue
		}
		name := strings.TrimSuffix(h.Name, "/")
		if !localPath(name) {
			return errors.New("unsafe plugin archive path")
		}
		if stripRoot {
			first, rest, hasRest := strings.Cut(name, "/")
			if archiveRoot == "" {
				archiveRoot = first
			}
			if first != archiveRoot {
				return errors.New("unexpected GitHub archive root")
			}
			if !hasRest && h.Typeflag == tar.TypeDir {
				continue
			}
			name = rest
		}
		if !localPath(name) {
			return errors.New("unsafe plugin archive path")
		}
		target := filepath.Join(dest, name)
		switch h.Typeflag {
		case tar.TypeDir:
			if err := os.MkdirAll(target, 0700); err != nil {
				return err
			}
		case tar.TypeSymlink:
			link := h.Linkname
			target := path.Clean(path.Join(path.Dir(name), link))
			if !stripRoot || link == "" || path.IsAbs(link) || !localPath(target) || strings.ContainsAny(link, "\\\x00:") {
				return errors.New("unsafe plugin archive link")
			}
			// Source bundles may contain documentation links. Never materialize a link.
		case tar.TypeReg:
			if err := os.MkdirAll(filepath.Dir(target), 0700); err != nil {
				return err
			}
			f, err := os.OpenFile(target, os.O_CREATE|os.O_EXCL|os.O_WRONLY, 0600)
			if err != nil {
				return err
			}
			_, err = io.CopyN(f, r, h.Size)
			if err := errors.Join(err, f.Close()); err != nil {
				return err
			}
		default:
			return errors.New("unsupported plugin archive entry")
		}
	}
}

func copyPluginFile(source, target string, mode os.FileMode) error {
	in, err := os.Open(source)
	if err != nil {
		return err
	}
	defer func() { _ = in.Close() }()
	if err := os.MkdirAll(filepath.Dir(target), 0700); err != nil {
		return err
	}
	out, err := os.OpenFile(target, os.O_CREATE|os.O_EXCL|os.O_WRONLY, mode)
	if err != nil {
		return err
	}
	_, err = io.Copy(out, in)
	return errors.Join(err, out.Close())
}
