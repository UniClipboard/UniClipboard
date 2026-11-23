package headless

import (
	"archive/tar"
	"bytes"
	"compress/gzip"
	"context"
	"crypto/ed25519"
	"crypto/rand"
	"crypto/sha256"
	"encoding/base64"
	"encoding/json"
	"fmt"
	"io"
	"net/http"
	"net/http/httptest"
	"os"
	"os/exec"
	"path/filepath"
	"runtime"
	"strings"
	"sync/atomic"
	"testing"
	"time"

	"github.com/uniclipboard/unic/internal/desktopupdate"
	api "github.com/uniclipboard/unic/internal/localapi"
)

func TestMacOSDaemonUpdate(t *testing.T) {
	artifacts := os.Getenv("UNIC_MACOS_UPDATE_ARTIFACTS")
	if artifacts == "" {
		t.Skip("set UNIC_MACOS_UPDATE_ARTIFACTS to a receipt directory")
	}
	need(t, os.MkdirAll(artifacts, 0700))
	root, err := filepath.Abs("../..")
	need(t, err)
	sourceHashes, err := sourceHashes(root)
	need(t, err)
	ctx, cancel := context.WithTimeout(context.Background(), 90*time.Second)
	defer cancel()
	profile, err := os.MkdirTemp("/private/tmp", "uc-update-")
	need(t, err)
	t.Cleanup(func() { _ = os.RemoveAll(profile) })
	binary := filepath.Join(profile, "unicd")
	build := exec.CommandContext(ctx, "go", "build", "-tags", "unic_e2e", "-o", binary, "./app/service/cmd/unicd")
	build.Dir = root
	if output, err := build.CombinedOutput(); err != nil {
		t.Fatalf("build unicd: %v\n%s", err, output)
	}
	executable, err := os.ReadFile(binary)
	need(t, err)
	checks := []string{}
	defer func() {
		data, err := json.MarshalIndent(map[string]any{
			"passed": !t.Failed(), "checks": checks, "source": sourceHashes,
			"binary_sha256": fmt.Sprintf("%x", sha256.Sum256(executable)),
		}, "", "  ")
		need(t, err)
		need(t, os.WriteFile(filepath.Join(artifacts, "receipt.json"), data, 0600))
	}()
	target := "darwin-" + runtime.GOARCH
	identity := fmt.Appendf(nil, `{"product":"desktop","version":"2.0.0","build_number":7,"target":%q}`, target)
	var compressed bytes.Buffer
	gz := gzip.NewWriter(&compressed)
	archive := tar.NewWriter(gz)
	need(t, archive.WriteHeader(&tar.Header{Name: "UniClipboard.app/Contents/Resources/release-identity.json", Mode: 0600, Size: int64(len(identity))}))
	_, err = archive.Write(identity)
	need(t, err)
	need(t, archive.Close())
	need(t, gz.Close())
	validBytes := compressed.Bytes()
	corruptBytes := bytes.Clone(validBytes)
	corruptBytes[len(corruptBytes)/2] ^= 1
	public, private, err := ed25519.GenerateKey(rand.Reader)
	need(t, err)
	digest := sha256.Sum256(validBytes)
	signature := base64.StdEncoding.EncodeToString(ed25519.Sign(private, digest[:]))
	key := base64.StdEncoding.EncodeToString(public)
	const desktopVersion = "2.0.0-build.6"
	var signedSelection atomic.Bool
	var corrupt atomic.Bool
	feed := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		switch r.URL.Path {
		case "/v1/desktop/alpha.json":
			signedSelection.Store(r.URL.Query().Get("from") == desktopVersion)
			_ = json.NewEncoder(w).Encode(map[string]any{
				"product": "desktop", "version": "2.0.0", "buildNumber": "7", "notes": "English\n<!-- zh -->\n中文",
				"artifacts": []desktopupdate.Artifact{{Platform: target, Format: "app.tar.gz", DownloadURL: "http://" + r.Host + "/archive",
					Size: int64(len(validBytes)), SHA256: fmt.Sprintf("%x", digest), Signature: signature, Updater: true}},
			})
		case "/archive":
			if corrupt.Load() {
				_, _ = w.Write(corruptBytes)
			} else {
				_, _ = w.Write(validBytes)
			}
		default:
			http.NotFound(w, r)
		}
	}))
	t.Cleanup(feed.Close)
	need(t, os.WriteFile(filepath.Join(profile, "config.toml"),
		[]byte("[sync]\nenabled = false\n[telemetry]\nenabled = false\n[desktop]\nupdate_channel = \"alpha\"\nlanguage = \"zh-CN\"\n"), 0600))
	socket := filepath.Join(profile, "u.sock")
	proc := start(t, binary, nil, append(os.Environ(), "UNIC_HOME="+profile, "UNIC_SOCK="+socket,
		"UNIC_E2E_DESKTOP_UPDATE_FEED="+feed.URL), filepath.Join(artifacts, "daemon.log"))
	client := api.NewClient(socket)
	until(t, "daemon readiness", func() bool {
		service, err := client.Service(ctx)
		return err == nil && proc.cmd.Process.Pid == service.PID
	})
	source := &desktopupdate.DaemonSource{Client: client, Key: key, CurrentVersion: desktopVersion}
	if _, err := client.OpenDesktopUpdate(ctx, "unknown"); err == nil {
		t.Fatal("an unselected archive was served")
	}
	checks = append(checks, "unselected archive refused")
	manifest, err := source.Manifest(ctx, false)
	need(t, err)
	manifestBytes, err := io.ReadAll(manifest)
	need(t, err)
	need(t, manifest.Close())
	if !signedSelection.Load() || !strings.Contains(string(manifestBytes), "中文") {
		t.Fatal("daemon did not use the configured channel, locale and Desktop version")
	}
	checks = append(checks, "daemon selects feed and localized notes")
	if _, err := source.Open(ctx, feed.URL+"/other"); err == nil {
		t.Fatal("desktop was allowed to replace the selected archive URL")
	}
	corrupt.Store(true)
	if _, err := source.Open(ctx, feed.URL+"/archive"); err == nil {
		t.Fatal("corrupt archive passed daemon verification")
	}
	checks = append(checks, "unselected URL and tampered archive refused")
	corrupt.Store(false)
	download, err := source.Open(ctx, feed.URL+"/archive")
	need(t, err)
	defer func() { _ = download.Close() }()
	proc.stop()
	actual, err := io.ReadAll(download)
	need(t, err)
	if !bytes.Equal(actual, validBytes) {
		t.Fatal("verified desktop archive changed after daemon retirement")
	}
	checks = append(checks, "verified archive remains readable after daemon retirement")
}
