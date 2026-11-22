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

const archiveLimit = 128 << 20

var githubPath = regexp.MustCompile(`^/[A-Za-z0-9-]+/[A-Za-z0-9_.-]+$`)
var errAssetMissing = errors.New("GitHub asset not found")

type githubRepo struct {
	location string
	slug     string
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

func fetch(ctx context.Context, target string, limit int64) (io.ReadCloser, error) {
	req, err := http.NewRequestWithContext(ctx, http.MethodGet, target, nil)
	if err != nil {
		return nil, errors.New("invalid GitHub request")
	}
	req.Header.Set("Accept", "application/vnd.github+json")
	res, err := githubHTTP.Do(req)
	if err != nil {
		return nil, errors.New("GitHub request failed")
	}
	if res.StatusCode != http.StatusOK || res.ContentLength > limit {
		_ = res.Body.Close()
		if res.StatusCode == http.StatusNotFound {
			return nil, errAssetMissing
		}
		return nil, fmt.Errorf("GitHub response rejected (HTTP %d)", res.StatusCode)
	}
	return struct {
		io.Reader
		io.Closer
	}{io.LimitReader(res.Body, limit+1), res.Body}, nil
}

func (g githubRepo) commit(ctx context.Context, ref string) (string, error) {
	if ref == "" {
		ref = "HEAD"
	}
	if len(ref) > 256 || strings.ContainsAny(ref, "\x00\r\n") || ref == "." || ref == ".." {
		return "", errors.New("invalid GitHub ref")
	}
	const limit = 1 << 20
	body, err := fetch(ctx, "https://api.github.com/repos"+g.slug+"/commits/"+url.PathEscape(ref), limit)
	if err != nil {
		return "", fmt.Errorf("could not resolve GitHub ref: %w", err)
	}
	defer func() { _ = body.Close() }()
	raw, err := io.ReadAll(body)
	var commit struct{ SHA string }
	if err != nil || len(raw) > limit || json.Unmarshal(raw, &commit) != nil || !commitPattern.MatchString(commit.SHA) {
		return "", errors.New("invalid GitHub commit response")
	}
	if commitPattern.MatchString(ref) && commit.SHA != ref {
		return "", errors.New("GitHub returned a different commit")
	}
	return commit.SHA, nil
}

func (g githubRepo) source(ctx context.Context, commit, dest string) error {
	body, err := fetch(ctx, "https://codeload.github.com"+g.slug+"/tar.gz/"+commit, archiveLimit)
	if err != nil {
		return err
	}
	defer func() { _ = body.Close() }()
	return extractTarGzip(body, dest, true)
}

func extractTarGzip(body io.Reader, dest string, stripRoot bool) error {
	z, err := gzip.NewReader(body)
	if err != nil {
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
		h, err := r.Next()
		if errors.Is(err, io.EOF) {
			_, err := io.Copy(io.Discard, limited)
			if err != nil || limited.N == 0 || count == 0 || (stripRoot && archiveRoot == "") {
				return errors.New("invalid or oversized plugin archive")
			}
			return nil
		}
		if err != nil || count >= 10000 || h.Size < 0 || h.Size > archiveLimit || len(h.Name) > 1024 {
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
