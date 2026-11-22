package e2e

import (
	"bytes"
	"context"
	"crypto/hmac"
	"crypto/sha256"
	"encoding/base64"
	"encoding/hex"
	"encoding/json"
	"fmt"
	"io"
	"net"
	"net/http"
	"net/http/httptest"
	"os"
	"os/exec"
	"path/filepath"
	"runtime"
	"strings"
	"syscall"
	"testing"
	"time"

	"github.com/BurntSushi/toml"

	api "github.com/uniclipboard/unic/internal/localapi"
)

func TestSyncClipboardExternalProcess(t *testing.T) {
	pluginDir := os.Getenv("UNIC_E2E_SYNCCLIPBOARD")
	if os.Getenv("UNIC_E2E") == "" || pluginDir == "" {
		t.Skip("set UNIC_E2E=1 and UNIC_E2E_SYNCCLIPBOARD to the independent plugin checkout")
	}
	root, err := repoRoot()
	if err != nil {
		t.Fatal(err)
	}
	ctx, cancel := context.WithTimeout(context.Background(), 90*time.Second)
	defer cancel()
	work := shortTemp(t)
	binaryPath := filepath.Join(work, "syncclipboard")
	// Pin the fixture to the host SDK under test while keeping the plugin go.mod untouched.
	workspace := filepath.Join(work, "go.work")
	writeFile := func(name string, data []byte) {
		t.Helper()
		if err := os.WriteFile(name, data, 0600); err != nil {
			t.Fatal(err)
		}
	}
	writeFile(workspace, []byte(fmt.Sprintf("go 1.27.3\nuse (\n%q\n%q\n)\n", root, pluginDir)))
	build := exec.CommandContext(ctx, "go", "build", "-race", "-o", binaryPath, "./cmd/unic-plugin-syncclipboard")
	build.Dir = pluginDir
	build.Env = append(os.Environ(), "GOWORK="+workspace)
	if out, err := build.CombinedOutput(); err != nil {
		t.Fatalf("build standalone plugin: %v\n%s", err, out)
	}
	readFile := func(path string) []byte {
		t.Helper()
		data, err := os.ReadFile(path)
		if err != nil {
			t.Fatal(err)
		}
		return data
	}
	binary := readFile(binaryPath)
	schema := readFile(filepath.Join(pluginDir, "settings.schema.json"))
	asset := pluginArchive(t, map[string][]byte{"syncclipboard": binary})
	var manifest map[string]any
	if _, err := toml.Decode(string(readFile(filepath.Join(pluginDir, "spec.toml"))), &manifest); err != nil {
		t.Fatalf("parse plugin manifest: %v", err)
	}
	// This fixture serves the executable built above, including when the
	// checkout already declares published artifacts for the same platform.
	manifest["artifacts"] = []map[string]string{{
		"goos":       runtime.GOOS,
		"goarch":     runtime.GOARCH,
		"release":    "v0.1.0",
		"asset":      "plugin.tar.gz",
		"executable": "syncclipboard",
		"sha256":     fmt.Sprintf("%x", sha256.Sum256(asset)),
	}}
	var encodedManifest bytes.Buffer
	if err := toml.NewEncoder(&encodedManifest).Encode(manifest); err != nil {
		t.Fatalf("encode plugin manifest: %v", err)
	}
	source := pluginArchive(t, map[string][]byte{"repo/spec.toml": encodedManifest.Bytes(), "repo/settings.schema.json": schema})
	github := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		switch {
		case strings.Contains(r.URL.Path, "/commits/"):
			_, _ = fmt.Fprintf(w, `{"sha":%q}`, strings.Repeat("a", 40))
		case strings.Contains(r.URL.Path, "/tar.gz/"):
			_, _ = w.Write(source)
		case strings.Contains(r.URL.Path, "/releases/download/"):
			_, _ = w.Write(asset)
		default:
			http.NotFound(w, r)
		}
	}))
	defer github.Close()
	t.Setenv("UNIC_E2E_GITHUB", github.URL)
	n := &node{home: filepath.Join(work, "home"), sock: filepath.Join(work, "u.sock")}
	if err := os.MkdirAll(n.home, 0700); err != nil {
		t.Fatal(err)
	}
	if err := initializeFixtureProfile(filepath.Join(binDir, "unic"), n.home, n.env()); err != nil {
		t.Fatal(err)
	}
	writeFile(filepath.Join(n.home, "config.toml"), []byte("[telemetry]\nenabled=false\n[plugins.instances.\"builtin.tailcat\"]\nenabled=false\n"))
	run := func(args ...string) string {
		t.Helper()
		out, stderr, err := n.unic(args...)
		if err != nil {
			t.Fatalf("%s: %v %s", strings.Join(args[:min(len(args), 2)], " "), err, stderr)
		}
		return out
	}
	run("plugin", "install", "https://github.com/uniclipboard/unic-plugin-syncclipboard")
	listener, err := net.Listen("tcp", "127.0.0.1:0")
	if err != nil {
		t.Fatal(err)
	}
	address := listener.Addr().String()
	_ = listener.Close()
	key := sha256.Sum256([]byte("isolated synthetic acceptance fixture"))
	settings := filepath.Join(work, "settings.json")
	writeFile(settings, fmt.Appendf(nil, `{"values":{"listen":%q,"credential_key":"%x"}}`, address, key))
	run("plugin", "settings", "save", "unic.syncclipboard", settings)
	run("plugin", "enable", "unic.syncclipboard")
	n.cmd = exec.CommandContext(ctx, filepath.Join(binDir, "unicd"))
	n.cmd.Env = n.env()
	if err := n.cmd.Start(); err != nil {
		t.Fatal(err)
	}
	n.done = make(chan struct{})
	go func() { _ = n.cmd.Wait(); close(n.done) }()
	t.Cleanup(func() {
		n.kill(syscall.SIGKILL)
		<-n.done
	})
	wait := func(label string, ready func() bool) {
		t.Helper()
		for deadline := time.Now().Add(10 * time.Second); time.Now().Before(deadline); time.Sleep(30 * time.Millisecond) {
			if ready() {
				return
			}
		}
		t.Fatalf("%s did not become ready", label)
	}
	wait("service", n.listening)
	parseChannel := func(s string) (c api.PeerStatus) {
		t.Helper()
		if err := json.Unmarshal([]byte(s), &c); err != nil || c.ID == "" || c.Revision <= 0 {
			t.Fatalf("invalid channel response: %v", err)
		}
		return c
	}
	a := parseChannel(run("device", "channel-add", "unic.syncclipboard", "Client A"))
	b := parseChannel(run("device", "channel-add", "unic.syncclipboard", "Client B"))
	password := func(id string) string {
		mac := hmac.New(sha256.New, key[:])
		_, _ = io.WriteString(mac, "unic.syncclipboard credential v1\x00"+id)
		return hex.EncodeToString(mac.Sum(nil))
	}
	httpClient := &http.Client{Timeout: time.Second}
	requestPath := func(method, path, id, secret, body string) (int, string) {
		t.Helper()
		req, err := http.NewRequestWithContext(ctx, method, "http://"+address+path, strings.NewReader(body))
		if err != nil {
			t.Fatal(err)
		}
		req.SetBasicAuth(id, secret)
		resp, err := httpClient.Do(req)
		if err != nil {
			return 0, ""
		}
		result, err := io.ReadAll(resp.Body)
		closeErr := resp.Body.Close()
		if err != nil || closeErr != nil {
			return 0, ""
		}
		return resp.StatusCode, string(result)
	}
	request := func(method, id, secret, body string) (int, string) {
		return requestPath(method, "/SyncClipboard.json", id, secret, body)
	}
	expect := func(label, method, id, secret, body string, want int) {
		t.Helper()
		if code, _ := request(method, id, secret, body); code != want {
			t.Fatalf("%s: HTTP %d, want %d", label, code, want)
		}
	}
	wait("plugin listener", func() bool { code, _ := request("GET", a.ID, "invalid", ""); return code == 401 })
	expect("credential isolation", "GET", b.ID, password(a.ID), "", 401)

	run("send", "--to", "all", "local synthetic text")
	wait("outbound publication", func() bool {
		code, doc := request("GET", a.ID, password(a.ID), "")
		return code == 200 && strings.Contains(doc, "local synthetic text")
	})
	expect("invalid document", "PUT", a.ID, password(a.ID), `{"type":"Text"}`, 400)
	expect("unsupported document", "PUT", a.ID, password(a.ID), `{"type":"Group","text":"fixture","hasData":true}`, 415)
	expect("inbound", "PUT", a.ID, password(a.ID), `{"type":"Text","text":"remote synthetic text","hasData":false}`, 204)
	client := api.NewClient(n.sock)
	wait("inbound history", func() bool {
		page, err := client.ListHistory(ctx, "", "")
		if err != nil {
			return false
		}
		for _, item := range page.Items {
			text, err := client.HistoryPreview(ctx, item)
			if err == nil && text == "remote synthetic text" {
				return true
			}
		}
		return false
	})
	if code, doc := request("GET", b.ID, password(b.ID), ""); code != 200 || !strings.Contains(doc, "local synthetic text") {
		t.Fatal("upload changed another channel's publication")
	}
	const pixel = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+aDskAAAAASUVORK5CYII="
	image, err := base64.StdEncoding.DecodeString(pixel)
	if err != nil {
		t.Fatal(err)
	}
	if code, _ := requestPath("PUT", "/file/upload.png", a.ID, password(a.ID), string(image)); code != 204 {
		t.Fatalf("stage an image: HTTP %d", code)
	}
	expect("cross-channel staging", "PUT", b.ID, password(b.ID), `{"type":"Image","text":"upload.png","hasData":true,"dataName":"upload.png"}`, 409)
	expect("image inbound", "PUT", a.ID, password(a.ID), fmt.Sprintf(`{"type":"Image","text":"upload.png","hasData":true,"dataName":"upload.png","transferDataHash":"%X"}`, sha256.Sum256(image)), 204)
	imagePath := filepath.Join(work, "pixel.png")
	writeFile(imagePath, image)
	run("send", "--to", "all", "--png", imagePath)
	var imageName string
	wait("image publication", func() bool {
		code, body := request("GET", b.ID, password(b.ID), "")
		var meta struct {
			Type             string
			DataName         string
			TransferDataHash string
		}
		if code != 200 || json.Unmarshal([]byte(body), &meta) != nil || meta.Type != "Image" || meta.DataName == "" || meta.TransferDataHash != fmt.Sprintf("%X", sha256.Sum256(image)) {
			return false
		}
		imageName = meta.DataName
		return true
	})
	if code, body := requestPath("GET", "/file/"+imageName, b.ID, password(b.ID), ""); code != 200 || body != string(image) {
		t.Fatalf("image file bytes differ: HTTP %d", code)
	}
	a = parseChannel(run("device", "channel", "pause", a.ID, fmt.Sprint(a.Revision)))
	expect("paused read", "GET", a.ID, password(a.ID), "", 403)
	expect("paused write", "PUT", a.ID, password(a.ID), `{"type":"Text","text":"blocked","hasData":false}`, 403)
	run("device", "channel", "remove", a.ID, fmt.Sprint(a.Revision))
	expect("removed read", "GET", a.ID, password(a.ID), "", 403)
	run("config", "set", "sync.enabled", "false")
	wait("global disable", func() bool { code, _ := request("GET", b.ID, password(b.ID), ""); return code == 403 })
	n.kill(syscall.SIGTERM)
	select {
	case <-n.done:
	case <-time.After(8 * time.Second):
		t.Fatal("daemon shutdown timed out")
	}
	expect("listener stopped", "GET", b.ID, password(b.ID), "", 0)
	if dest := os.Getenv("UNIC_E2E_ARTIFACTS"); dest != "" {
		hash := func(data []byte) string { return fmt.Sprintf("%x", sha256.Sum256(data)) }
		receipt, err := json.MarshalIndent(map[string]any{
			"passed": true, "goos": runtime.GOOS, "goarch": runtime.GOARCH,
			"program_sha256": map[string]string{"plugin": hash(binary), "unic": hash(readFile(filepath.Join(binDir, "unic"))), "unicd": hash(readFile(filepath.Join(binDir, "unicd")))},
			"assertions":     []string{"installation", "credentials", "outbound text", "inbound history", "image stage and download SHA-256", "invalid and unsupported documents", "channel isolation", "pause", "removal", "global disable", "shutdown"},
		}, "", "  ")
		if err != nil {
			t.Fatal(err)
		}
		if err := os.MkdirAll(dest, 0700); err != nil {
			t.Fatal(err)
		}
		writeFile(filepath.Join(dest, "syncclipboard-external.json"), append(receipt, '\n'))
	}
}
