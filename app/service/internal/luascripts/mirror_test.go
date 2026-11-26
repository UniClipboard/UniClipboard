package luascripts

import (
	"archive/tar"
	"bytes"
	"compress/gzip"
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"fmt"
	"net/http"
	"net/http/httptest"
	"net/url"
	"os"
	"path/filepath"
	"runtime"
	"strings"
	"sync/atomic"
	"testing"

	api "github.com/uniclipboard/unic/internal/localapi"
)

func buildTestTarGz(t *testing.T, files map[string][]byte, withPAX bool) []byte {
	t.Helper()
	var buf bytes.Buffer
	gz := gzip.NewWriter(&buf)
	tw := tar.NewWriter(gz)
	if withPAX {
		if err := tw.WriteHeader(&tar.Header{
			Typeflag:   tar.TypeXGlobalHeader,
			Name:       "pax_global_header",
			PAXRecords: map[string]string{"comment": "synthetic archive"},
		}); err != nil {
			t.Fatal(err)
		}
	}
	for name, data := range files {
		if err := tw.WriteHeader(&tar.Header{Name: name, Mode: 0600, Size: int64(len(data))}); err != nil {
			t.Fatal(err)
		}
		if _, err := tw.Write(data); err != nil {
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

type routeTransport struct {
	target *url.URL
}

func (r routeTransport) RoundTrip(req *http.Request) (*http.Response, error) {
	clone := req.Clone(req.Context())
	host := clone.URL.Host
	clone.URL.Scheme = r.target.Scheme
	clone.URL.Host = r.target.Host
	clone.URL.Path = "/" + host + clone.URL.Path
	clone.URL.RawPath = ""
	clone.Host = r.target.Host
	return http.DefaultTransport.RoundTrip(clone)
}

func TestFetchMarketplaceWithMirrorAndEnvPrecedence(t *testing.T) {
	dir := t.TempDir()
	luaDir := filepath.Join(dir, "lua")
	if err := os.MkdirAll(luaDir, 0700); err != nil {
		t.Fatal(err)
	}

	makeCatalog := func(ver string) api.PluginMarketplace {
		return api.PluginMarketplace{
			Version:   1,
			UpdatedAt: "2026-11-26",
			Plugins: []api.MarketplaceEntry{
				{
					ID:           "unic.demo",
					Name:         "Demo",
					Author:       "UniClipboard",
					Category:     "text",
					Version:      ver,
					Summary:      "Demo plugin",
					Repository:   "https://github.com/UniClipboard/unic-plugin-demo",
					Ref:          strings.Repeat("a", 40),
					Capabilities: []string{"processor"},
				},
			},
		}
	}

	var mirrorHits atomic.Int32
	mirrorSrv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.URL.Path != "/prefix/registry.json" {
			http.NotFound(w, r)
			return
		}
		mirrorHits.Add(1)
		_ = json.NewEncoder(w).Encode(makeCatalog("1.0.0-mirror"))
	}))
	defer mirrorSrv.Close()

	envMirrorSrv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.URL.Path != "/registry.json" {
			http.NotFound(w, r)
			return
		}
		_ = json.NewEncoder(w).Encode(makeCatalog("1.0.0-env-mirror"))
	}))
	defer envMirrorSrv.Close()

	explicitRegSrv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		_ = json.NewEncoder(w).Encode(makeCatalog("1.0.0-explicit-reg"))
	}))
	defer explicitRegSrv.Close()

	ctx := context.Background()

	// 1. Configured mirror URL with subpath prefix serves /prefix/registry.json.
	got := FetchMarketplaceWithMirror(ctx, luaDir, true, mirrorSrv.URL+"/prefix/")
	if got.Source != "remote" || len(got.Plugins) != 1 || got.Plugins[0].Version != "1.0.0-mirror" {
		t.Fatalf("unexpected mirror catalog: %+v", got)
	}
	if mirrorHits.Load() != 1 {
		t.Fatalf("expected 1 mirror hit, got %d", mirrorHits.Load())
	}

	// 2. UNIC_PLUGIN_MIRROR_URL overrides configured mirrorURL.
	t.Setenv("UNIC_PLUGIN_MIRROR_URL", envMirrorSrv.URL)
	got = FetchMarketplaceWithMirror(ctx, luaDir, true, mirrorSrv.URL+"/prefix")
	if got.Source != "remote" || got.Plugins[0].Version != "1.0.0-env-mirror" {
		t.Fatalf("expected env mirror to win: %+v", got)
	}

	// 3. UNIC_PLUGIN_REGISTRY_URL overrides UNIC_PLUGIN_MIRROR_URL and configured mirror.
	t.Setenv("UNIC_PLUGIN_REGISTRY_URL", explicitRegSrv.URL)
	got = FetchMarketplaceWithMirror(ctx, luaDir, true, mirrorSrv.URL+"/prefix")
	if got.Source != "remote" || got.Plugins[0].Version != "1.0.0-explicit-reg" {
		t.Fatalf("expected explicit registry URL to win: %+v", got)
	}

	// 4. UNIC_PLUGIN_REGISTRY_URL=off stays completely offline even with mirror configured.
	t.Setenv("UNIC_PLUGIN_REGISTRY_URL", "off")
	got = FetchMarketplaceWithMirror(ctx, luaDir, true, mirrorSrv.URL+"/prefix")
	if got.Source != "bundled" || got.Warning != "" {
		t.Fatalf("expected offline bundled catalog: %+v", got)
	}
}

func TestInstallFromMirrorSkipsCommitAPIAndPreservesCanonicalLock(t *testing.T) {
	dir := t.TempDir()
	luaDir := filepath.Join(dir, "lua")
	commit := strings.Repeat("a", 40)
	manifest := `manifest_version = 1
id = "example.mirrorlua"
version = "1.0.0"
api_version = 1
kind = "lua"
entry = "main.lua"
capabilities = ["processor"]
`
	sourceTar := buildTestTarGz(t, map[string][]byte{
		"demo-root/spec.toml": []byte(manifest),
		"demo-root/main.lua":  []byte("return { id='example.mirrorlua', version='1.0.0', api_version=1, capabilities={'processor'}, init=function() end, start=function() end, process=function() end, close=function() end }"),
	}, true)

	var apiHits, codeloadHits atomic.Int32
	mirrorSrv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		switch r.URL.Path {
		case "/api.github.com/repos/example/demo/commits/" + commit:
			apiHits.Add(1)
			_, _ = fmt.Fprintf(w, `{"sha":%q}`, commit)
		case "/codeload.github.com/example/demo/tar.gz/" + commit:
			codeloadHits.Add(1)
			_, _ = w.Write(sourceTar)
		default:
			http.NotFound(w, r)
		}
	}))
	defer mirrorSrv.Close()

	id, err := Install(context.Background(), luaDir, InstallOptions{
		Repository: "https://github.com/Example/Demo.git",
		Ref:        commit,
		MirrorURL:  mirrorSrv.URL,
	})
	if err != nil {
		t.Fatalf("Install from mirror: %v", err)
	}
	if id != "example.mirrorlua" {
		t.Fatalf("id = %q, want example.mirrorlua", id)
	}
	if apiHits.Load() != 0 {
		t.Fatalf("expected 40-char commit ref to skip api.github.com, got %d hits", apiHits.Load())
	}
	if codeloadHits.Load() != 1 {
		t.Fatalf("expected 1 mirror codeload hit, got %d", codeloadHits.Load())
	}

	lock, err := readLock(installedDir(luaDir))
	if err != nil {
		t.Fatal(err)
	}
	pin, ok := lock[id]
	if !ok {
		t.Fatalf("missing lock entry for %s", id)
	}
	if pin.Repository != "https://github.com/example/demo" || pin.Commit != commit {
		t.Fatalf("lock must keep canonical GitHub identity, got %+v", pin)
	}
}

func TestInstallMirrorFallbackAndDigestIntegrityBoundary(t *testing.T) {
	dir := t.TempDir()
	luaDir := filepath.Join(dir, "lua")
	tagCommit := strings.Repeat("b", 40)
	corruptCommit := strings.Repeat("c", 40)

	luaManifest := `manifest_version = 1
id = "example.fallback"
version = "1.0.0"
api_version = 1
kind = "lua"
entry = "main.lua"
capabilities = ["processor"]
`
	luaSourceTar := buildTestTarGz(t, map[string][]byte{
		"repo-root/spec.toml": []byte(luaManifest),
		"repo-root/main.lua":  []byte("return { id='example.fallback', version='1.0.0', api_version=1, capabilities={'processor'}, init=function() end, start=function() end, process=function() end, close=function() end }"),
	}, true)

	goodAsset := buildTestTarGz(t, map[string][]byte{"demo": []byte("expected-bytes")}, false)
	goodAssetSum := sha256.Sum256(goodAsset)
	goManifest := fmt.Sprintf(`manifest_version = 1
id = "example.corrupt"
version = "1.0.0"
api_version = 1
kind = "go"
module = "example.com/corrupt"
package = "."
capabilities = ["processor"]
[[artifacts]]
goos = "%s"
goarch = "%s"
release = "v1.0.0"
asset = "demo.tar.gz"
executable = "demo"
sha256 = "%s"
`, runtime.GOOS, runtime.GOARCH, hex.EncodeToString(goodAssetSum[:]))
	goSourceTar := buildTestTarGz(t, map[string][]byte{
		"repo-root/spec.toml": []byte(goManifest),
	}, true)

	// Direct "GitHub" server intercepted via githubHTTP.Transport.
	var directAPIHits, directCodeloadHits, directAssetHits atomic.Int32
	directSrv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		switch r.URL.Path {
		case "/api.github.com/repos/example/unlisted/commits/v1.0.0":
			directAPIHits.Add(1)
			_, _ = fmt.Fprintf(w, `{"sha":%q}`, tagCommit)
		case "/codeload.github.com/example/unlisted/tar.gz/" + tagCommit:
			directCodeloadHits.Add(1)
			_, _ = w.Write(luaSourceTar)
		case "/github.com/example/corrupt/releases/download/v1.0.0/demo.tar.gz":
			directAssetHits.Add(1)
			_, _ = w.Write(goodAsset)
		default:
			http.NotFound(w, r)
		}
	}))
	defer directSrv.Close()

	prevTransport := githubHTTP.Transport
	directURL, err := url.Parse(directSrv.URL)
	if err != nil {
		t.Fatal(err)
	}
	githubHTTP.Transport = routeTransport{target: directURL}
	t.Cleanup(func() { githubHTTP.Transport = prevTransport })

	// Cross-origin redirect trap server (must never be reached from mirror).
	var evilHits atomic.Int32
	evilSrv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		evilHits.Add(1)
		_, _ = w.Write(luaSourceTar)
	}))
	defer evilSrv.Close()

	// Mirror returns 404 for unlisted repo's commit API, cross-origin redirect
	// for unlisted codeload, and corrupted bytes for corrupt repo's Go asset.
	mirrorSrv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		switch r.URL.Path {
		case "/api.github.com/repos/example/unlisted/commits/v1.0.0":
			http.NotFound(w, r)
		case "/codeload.github.com/example/unlisted/tar.gz/" + tagCommit:
			http.Redirect(w, r, evilSrv.URL+"/hijacked", http.StatusFound)
		case "/codeload.github.com/example/corrupt/tar.gz/" + corruptCommit:
			_, _ = w.Write(goSourceTar)
		case "/github.com/example/corrupt/releases/download/v1.0.0/demo.tar.gz":
			_, _ = w.Write([]byte("tampered-archive-bytes"))
		default:
			http.NotFound(w, r)
		}
	}))
	defer mirrorSrv.Close()

	// 1. Mirror 404 (API) and rejected cross-origin redirect (codeload) fall back cleanly to direct GitHub.
	id, err := Install(context.Background(), luaDir, InstallOptions{
		Repository: "https://github.com/example/unlisted",
		Ref:        "v1.0.0",
		MirrorURL:  mirrorSrv.URL,
	})
	if err != nil {
		t.Fatalf("expected direct GitHub fallback to succeed: %v", err)
	}
	if id != "example.fallback" {
		t.Fatalf("id = %q, want example.fallback", id)
	}
	if evilHits.Load() != 0 {
		t.Fatalf("mirror followed cross-origin redirect (%d hits)", evilHits.Load())
	}
	if directAPIHits.Load() != 1 || directCodeloadHits.Load() != 1 {
		t.Fatalf("expected direct fallback hits (api=%d codeload=%d)", directAPIHits.Load(), directCodeloadHits.Load())
	}

	// 2. Tampered asset bytes from mirror MUST fail SHA-256 verification and MUST NOT fall back to direct GitHub or --build.
	_, err = Install(context.Background(), luaDir, InstallOptions{
		Repository: "https://github.com/example/corrupt",
		Ref:        corruptCommit,
		MirrorURL:  mirrorSrv.URL,
		Build:      true,
	})
	if err == nil || !strings.Contains(err.Error(), "digest") {
		t.Fatalf("expected digest mismatch error, got %v", err)
	}
	if directAssetHits.Load() != 0 {
		t.Fatalf("digest mismatch must not trigger direct fallback (got %d hits)", directAssetHits.Load())
	}
}
