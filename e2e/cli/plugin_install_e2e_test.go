package e2e

import (
	"archive/tar"
	"bytes"
	"compress/gzip"
	"crypto/sha256"
	"database/sql"
	"encoding/json"
	"fmt"
	"net/http"
	"net/http/httptest"
	"os"
	"os/exec"
	"path/filepath"
	"runtime"
	"strings"
	"sync/atomic"
	"testing"

	"github.com/uniclipboard/unic/internal/v1migration/v1fixture"
)

func TestGitHubPluginInstallation(t *testing.T) {
	if os.Getenv("UNIC_E2E") == "" {
		t.Skip("set UNIC_E2E=1")
	}
	root, err := repoRoot()
	if err != nil {
		t.Fatal(err)
	}
	work := shortTemp(t)
	pluginBinary := filepath.Join(work, "settingsdemo")
	build := exec.Command("go", "build", "-o", pluginBinary, "./plugins/examples/settingsdemo/cmd/plugin")
	build.Dir = root
	if out, err := build.CombinedOutput(); err != nil {
		t.Fatalf("build Go plugin fixture: %v\n%s", err, out)
	}
	binary, err := os.ReadFile(pluginBinary)
	if err != nil {
		t.Fatal(err)
	}
	schema, err := os.ReadFile(filepath.Join(root, "plugins/examples/settingsdemo/settings.schema.json"))
	if err != nil {
		t.Fatal(err)
	}
	asset := pluginArchive(t, map[string][]byte{"settingsdemo": binary})
	good := strings.Repeat("a", 40)
	bad := strings.Repeat("b", 40)
	source := strings.Repeat("c", 40)
	upgrade := strings.Repeat("d", 40)
	rateLimited := strings.Repeat("e", 40)
	manifest := fmt.Sprintf(`manifest_version = 1
id = "example.settingsdemo"
version = "1.0.0"
api_version = 1
kind = "go"
module = "example.com/settingsdemo"
package = "."
capabilities = ["processor"]
settings_schema = "settings.schema.json"
settings_sha256 = "%x"
[[artifacts]]
goos = "%s"
goarch = "%s"
release = "v1.0.0"
asset = "settingsdemo.tar.gz"
executable = "settingsdemo"
sha256 = "%x"
`, sha256.Sum256(schema), runtime.GOOS, runtime.GOARCH, sha256.Sum256(asset))
	makeSource := func(manifest string) []byte {
		return pluginArchive(t, map[string][]byte{
			"repo/plugins/demo/spec.toml":            []byte(manifest),
			"repo/plugins/demo/settings.schema.json": schema,
			"repo/plugins/demo/go.mod":               []byte("module example.com/settingsdemo\n\ngo 1.23.0\n"),
			"repo/plugins/demo/go.sum":               {},
			"repo/plugins/demo/main.go":              []byte("package main\nfunc main() {}\n"),
		})
	}
	archives := map[string][]byte{
		good:        makeSource(manifest),
		upgrade:     makeSource(manifest),
		rateLimited: makeSource(strings.Replace(manifest, "v1.0.0", "rate-limited", 1)),
		bad:         makeSource(strings.Replace(manifest, "v1.0.0", "bad", 1)),
		source: makeSource(strings.ReplaceAll(
			strings.Replace(manifest, "v1.0.0", "missing", 1),
			"example.settingsdemo", "example.source-demo",
		)),
	}
	var requests atomic.Int64
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		requests.Add(1)
		switch {
		case strings.HasPrefix(r.URL.Path, "/api.github.com/repos/example/repo/commits/"):
			ref := strings.TrimPrefix(r.URL.Path, "/api.github.com/repos/example/repo/commits/")
			commit := map[string]string{"HEAD": good, "v1.0.0": good, good: good, "bad": bad, "source": source, "upgrade": upgrade, "rate-limited": rateLimited}[ref]
			if commit == "" {
				http.NotFound(w, r)
				return
			}
			_, _ = fmt.Fprintf(w, "{\"sha\":%q}", commit)
		case strings.HasPrefix(r.URL.Path, "/codeload.github.com/example/repo/tar.gz/"):
			commit := strings.TrimPrefix(r.URL.Path, "/codeload.github.com/example/repo/tar.gz/")
			if archive := archives[commit]; archive != nil {
				_, _ = w.Write(archive)
				return
			}
			http.NotFound(w, r)
		case strings.Contains(r.URL.Path, "/releases/download/v1.0.0/"):
			_, _ = w.Write(asset)
		case strings.Contains(r.URL.Path, "/releases/download/rate-limited/"):
			http.Error(w, "limited", http.StatusTooManyRequests)
		case strings.Contains(r.URL.Path, "/releases/download/bad/"):
			_, _ = w.Write([]byte("corrupt asset"))
		default:
			http.NotFound(w, r)
		}
	}))
	defer server.Close()
	t.Setenv("UNIC_E2E_GITHUB", server.URL)
	n := &node{home: filepath.Join(work, "home"), sock: filepath.Join(work, "node.sock")}
	// Use a synthetic source and the production migration path. Never probe or
	// import the developer's default history when preparing this isolated profile.
	sourceDir, dbpath, fixture, err := v1fixture.Build(filepath.Join(work, "synthetic-source"))
	if err != nil {
		t.Fatal(err)
	}
	db, err := sql.Open("sqlite", dbpath)
	if err != nil {
		t.Fatal(err)
	}
	_, err = db.Exec(`INSERT INTO clipboard_snapshot_representation VALUES('rep-bad','event-bad','text/plain','text/plain',?,NULL,9)`, []byte(fixture.Repaired))
	if err != nil {
		t.Fatal(err)
	}
	if err = db.Close(); err != nil {
		t.Fatal(err)
	}
	initialize := exec.Command(filepath.Join(binDir, "unic"), "migrate", "--source", sourceDir, "--target", n.home, "--passphrase-stdin", "--json")
	initialize.Env = n.env()
	initialize.Stdin = strings.NewReader(fixture.Passphrase + "\n")
	if out, err := initialize.CombinedOutput(); err != nil {
		t.Fatalf("initialize synthetic plugin profile: %v\n%s", err, out)
	}

	install := []string{"plugin", "install", "https://github.com/Example/Repo.git", "--path", "plugins/demo"}
	run := func(ok bool, args ...string) string {
		t.Helper()
		out, stderr, err := n.unic(args...)
		if ok != (err == nil) {
			t.Fatalf("%v: %v\n%s\n%s", args, err, out, stderr)
		}
		return out + stderr
	}
	run(true, install...)
	run(true, append(append([]string{}, install...), "--ref", "v1.0.0", "--build")...)
	if out := run(false, append(append([]string{}, install...), "--ref", "bad", "--replace", "--build")...); !strings.Contains(out, "digest") {
		t.Fatalf("corrupt release did not report digest failure: %s", out)
	}
	lockPath := filepath.Join(n.home, "plugins", "installed", "installed.lock")
	readLock := func() []byte {
		t.Helper()
		data, err := os.ReadFile(lockPath)
		if err != nil {
			t.Fatal(err)
		}
		return data
	}
	if out := run(false, append(append([]string{}, install...), "--ref", "rate-limited", "--replace", "--build")...); !strings.Contains(out, "429") {
		t.Fatalf("release limit was not preserved: %s", out)
	}
	if data := readLock(); !bytes.Contains(data, []byte(good)) || bytes.Contains(data, []byte(bad)) {
		t.Fatalf("failed upgrade changed the installation: %s", data)
	}
	if out := run(false, append(append([]string{}, install...), "--ref", "source")...); !strings.Contains(out, "--build") {
		t.Fatalf("missing asset did not suggest opt-in build: %s", out)
	}
	run(true, append(append([]string{}, install...), "--ref", "source", "--build")...)
	run(true, append(append([]string{}, install...), "--ref", "upgrade", "--replace")...)
	run(true, "plugin", "rollback", "example.settingsdemo")
	var pins map[string]struct{ Commit string }
	if err := json.Unmarshal(readLock(), &pins); err != nil || pins["example.settingsdemo"].Commit != good {
		t.Fatalf("rollback did not restore the previous commit: %v", err)
	}
	run(true, "plugin", "enable", "example.settingsdemo")
	out := run(true, "plugin", "settings", "show", "example.settingsdemo")
	if !strings.Contains(out, "\"schema\"") {
		t.Fatalf("Go plugin startup and Schema discovery failed: %s", out)
	}
	run(true, "plugin", "uninstall", "example.settingsdemo")
	data := readLock()
	if bytes.Contains(data, []byte("example.settingsdemo")) || !bytes.Contains(data, []byte(source)) {
		t.Fatalf("unexpected installations after uninstall: %s", data)
	}
	run(true, "status")
	if _, err := os.Stat(filepath.Join(n.home, "plugins", "installed", "example.settingsdemo", good)); !os.IsNotExist(err) {
		t.Fatalf("retired plugin was not pruned: %v", err)
	}
	if dest := os.Getenv("UNIC_E2E_ARTIFACTS"); dest != "" {
		if err := os.MkdirAll(dest, 0700); err != nil {
			t.Fatal(err)
		}
		receipt, err := json.MarshalIndent(map[string]any{"passed": true, "global_pax_metadata": true, "github_requests": requests.Load(),
			"prebuilt_commit": good, "source_commit": source, "corrupt_commit": bad, "rollback_from": upgrade, "rate_limited_commit": rateLimited, "goos": runtime.GOOS, "goarch": runtime.GOARCH}, "", "  ")
		if err != nil {
			t.Fatal(err)
		}
		if err := os.WriteFile(filepath.Join(dest, "github-plugin-install.json"), receipt, 0600); err != nil {
			t.Fatal(err)
		}
	}
}

func pluginArchive(t *testing.T, files map[string][]byte) []byte {
	t.Helper()
	var buf bytes.Buffer
	zip := gzip.NewWriter(&buf)
	tarball := tar.NewWriter(zip)
	// Match the real GitHub codeload archive: metadata precedes the source root.
	if err := tarball.WriteHeader(&tar.Header{Typeflag: tar.TypeXGlobalHeader, Name: "pax_global_header", PAXRecords: map[string]string{"comment": "synthetic GitHub source metadata"}}); err != nil {
		t.Fatal(err)
	}
	for name, data := range files {
		if err := tarball.WriteHeader(&tar.Header{Name: name, Mode: 0600, Size: int64(len(data))}); err != nil {
			t.Fatal(err)
		}
		if _, err := tarball.Write(data); err != nil {
			t.Fatal(err)
		}
	}
	if err := tarball.Close(); err != nil {
		t.Fatal(err)
	}
	if err := zip.Close(); err != nil {
		t.Fatal(err)
	}
	return buf.Bytes()
}
