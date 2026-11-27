package luascripts

import (
	"archive/tar"
	"bytes"
	"compress/gzip"
	"context"
	"errors"
	"io"
	"net/http"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"
)

type roundTripFunc func(*http.Request) (*http.Response, error)

func (f roundTripFunc) RoundTrip(r *http.Request) (*http.Response, error) { return f(r) }

func makeTestLuaArchive(t *testing.T, id string) []byte {
	t.Helper()
	var buf bytes.Buffer
	gz := gzip.NewWriter(&buf)
	tw := tar.NewWriter(gz)
	manifest := `manifest_version = 1
kind = "lua"
entry = "main.lua"
id = "` + id + `"
version = "1.0.0"
api_version = 1
capabilities = ["processor"]
`
	luaSrc := `return {
  id = "` + id + `",
  version = "1.0.0",
  api_version = 1,
  capabilities = { "processor" },
  process = function(ctx) end,
}
`
	files := map[string]string{
		"repo-root/spec.toml": manifest,
		"repo-root/main.lua":  luaSrc,
	}
	for name, body := range files {
		hdr := &tar.Header{Name: name, Mode: 0o600, Size: int64(len(body)), Typeflag: tar.TypeReg}
		if err := tw.WriteHeader(hdr); err != nil {
			t.Fatal(err)
		}
		if _, err := tw.Write([]byte(body)); err != nil {
			t.Fatal(err)
		}
	}
	if err := tw.Close(); err != nil {
		t.Fatal(err)
	}
	if err := gz.Close(); err != nil {
		t.Fatal(err)
	}
	return buf.Bytes()
}

func TestPrepareInstallCancelCleansStagingAndReturnsCanceled(t *testing.T) {
	origTransport := githubHTTP.Transport
	t.Cleanup(func() { githubHTTP.Transport = origTransport })

	const commit = "0123456789abcdef0123456789abcdef01234567"
	started := make(chan struct{}, 1)
	githubHTTP.Transport = roundTripFunc(func(req *http.Request) (*http.Response, error) {
		if req.URL.Host == "api.github.com" {
			return &http.Response{
				StatusCode: http.StatusOK,
				Body:       io.NopCloser(strings.NewReader(`{"sha":"` + commit + `"}`)),
				Header:     make(http.Header),
			}, nil
		}
		select {
		case started <- struct{}{}:
		default:
		}
		<-req.Context().Done()
		return nil, req.Context().Err()
	})

	root := filepath.Join(t.TempDir(), "plugins")
	ctx, cancel := context.WithCancel(context.Background())
	errCh := make(chan error, 1)
	go func() {
		_, err := PrepareInstall(ctx, root, InstallOptions{
			Repository: "https://github.com/example/unic-plugin-demo",
			Ref:        "main",
		})
		errCh <- err
	}()

	select {
	case <-started:
	case <-time.After(3 * time.Second):
		t.Fatal("PrepareInstall did not reach source download")
	}

	// Scan must stay non-blocking while PrepareInstall is downloading.
	if found, errs := Scan(root); len(found) != 0 || len(errs) != 0 {
		t.Fatalf("unexpected scan result during install: found=%v errs=%v", found, errs)
	}

	cancel()
	select {
	case err := <-errCh:
		if !errors.Is(err, context.Canceled) {
			t.Fatalf("expected context.Canceled, got %v", err)
		}
	case <-time.After(3 * time.Second):
		t.Fatal("PrepareInstall did not exit after cancellation")
	}

	// Verify staging directory was cleaned up and nothing was committed.
	entries, err := os.ReadDir(installedDir(root))
	if err != nil && !errors.Is(err, os.ErrNotExist) {
		t.Fatal(err)
	}
	for _, e := range entries {
		if strings.HasPrefix(e.Name(), ".stage-") || e.Name() == "installed.lock" {
			t.Fatalf("unexpected leftover entry after cancelled install: %s", e.Name())
		}
	}
}

func TestPrepareAndCommitInstallSucceedsAndRespectsPostPrepareCancel(t *testing.T) {
	origTransport := githubHTTP.Transport
	t.Cleanup(func() { githubHTTP.Transport = origTransport })

	const commit = "fedcba9876543210fedcba9876543210fedcba98"
	archive := makeTestLuaArchive(t, "example.demo")
	githubHTTP.Transport = roundTripFunc(func(req *http.Request) (*http.Response, error) {
		if req.URL.Host == "api.github.com" {
			return &http.Response{
				StatusCode: http.StatusOK,
				Body:       io.NopCloser(strings.NewReader(`{"sha":"` + commit + `"}`)),
				Header:     make(http.Header),
			}, nil
		}
		return &http.Response{
			StatusCode: http.StatusOK,
			Body:       io.NopCloser(bytes.NewReader(archive)),
			Header:     make(http.Header),
		}, nil
	})

	root := filepath.Join(t.TempDir(), "plugins")
	prep, err := PrepareInstall(context.Background(), root, InstallOptions{
		Repository: "https://github.com/example/unic-plugin-demo",
		Ref:        "main",
	})
	if err != nil {
		t.Fatalf("PrepareInstall: %v", err)
	}
	defer func() { _ = prep.Close() }()

	// A canceled context before Commit must not write installed.lock.
	canceledCtx, cancel := context.WithCancel(context.Background())
	cancel()
	if _, err := prep.Commit(canceledCtx, nil); !errors.Is(err, context.Canceled) {
		t.Fatalf("expected context.Canceled from Commit, got %v", err)
	}
	if found, _ := Scan(root); len(found) != 0 {
		t.Fatalf("canceled Commit leaked plugin: %+v", found)
	}

	// Normal Commit publishes the plugin cleanly.
	id, err := prep.Commit(context.Background(), nil)
	if err != nil || id != "example.demo" {
		t.Fatalf("Commit = %q, %v", id, err)
	}
	found, errs := Scan(root)
	if len(errs) != 0 || found["example.demo"].Manifest.Version != "1.0.0" {
		t.Fatalf("Scan after Commit: found=%+v errs=%+v", found, errs)
	}
}
