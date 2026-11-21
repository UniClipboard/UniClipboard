package e2e

import (
	"bytes"
	"context"
	"crypto/sha256"
	"encoding/json"
	"fmt"
	"image"
	"image/png"
	"net"
	"net/http"
	"net/http/httptest"
	"os"
	"os/exec"
	"path/filepath"
	"runtime"
	"strconv"
	"strings"
	"syscall"
	"testing"
	"time"

	"github.com/uniclipboard/unic/internal/clip"
	api "github.com/uniclipboard/unic/internal/localapi"
	"github.com/uniclipboard/unic/internal/testboard"
)

// Failure inventory and retry limits are recorded before implementation in
// docs/exec/sync-plugin-workflow.md. No system clipboard or external network.
func TestSyncPluginWorkflow(t *testing.T) {
	if os.Getenv("UNIC_E2E") == "" {
		t.Skip("set UNIC_E2E=1")
	}
	root, err := repoRoot()
	if err != nil {
		t.Fatal(err)
	}
	work := shortTemp(t)
	binaryPath := filepath.Join(work, "loopback")
	build := exec.Command("go", "build", "-race", "-o", binaryPath, "./plugins/examples/loopback/cmd/plugin")
	build.Dir = root
	if out, err := build.CombinedOutput(); err != nil {
		t.Fatalf("build fixture: %v\n%s", err, out)
	}
	binary, err := os.ReadFile(binaryPath)
	if err != nil {
		t.Fatal(err)
	}
	asset := pluginArchive(t, map[string][]byte{"loopback": binary})
	manifest := fmt.Sprintf(`manifest_version=1
id="e2e.tcp"
version="1.0.0"
api_version=1
kind="go"
module="example.com/loopback"
package="."
capabilities=["sync_backend"]
transport_kind="peer_sessions"
[[artifacts]]
goos=%q
goarch=%q
release="v1.0.0"
asset="loopback.tar.gz"
executable="loopback"
sha256="%x"
`, runtime.GOOS, runtime.GOARCH, sha256.Sum256(asset))
	source := pluginArchive(t, map[string][]byte{"repo/spec.toml": []byte(manifest)})
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
	programHashes := map[string]string{"plugin": fmt.Sprintf("%x", sha256.Sum256(binary))}
	for _, name := range []string{"unic", "unicd"} {
		data, err := os.ReadFile(filepath.Join(binDir, name))
		if err != nil {
			t.Fatal(err)
		}
		programHashes[name] = fmt.Sprintf("%x", sha256.Sum256(data))
	}
	var steps []string
	record := func(s string) { steps = append(steps, s); t.Log(s) }
	defer func() {
		if dest := os.Getenv("UNIC_E2E_ARTIFACTS"); dest != "" {
			_ = os.MkdirAll(dest, 0700)
			data, _ := json.MarshalIndent(map[string]any{"passed": !t.Failed(), "steps": steps, "plugin_sha256": fmt.Sprintf("%x", sha256.Sum256(binary)), "scope": "real CLI, service and external Go RPC processes; private loopback transport", "program_sha256": programHashes}, "", "  ")
			_ = os.WriteFile(filepath.Join(dest, "sync-plugin-workflow.json"), data, 0600)
		}
	}()
	run := func(n *node, ok bool, args ...string) string {
		t.Helper()
		out, stderr, e := n.unic(args...)
		if (e == nil) != ok {
			t.Fatalf("CLI %s: %v %s", args[0], e, stderr)
		}
		return out
	}
	start := func(n *node) {
		t.Helper()
		log, err := os.OpenFile(filepath.Join(n.home, "service.log"), os.O_CREATE|os.O_APPEND|os.O_WRONLY, 0600)
		if err != nil {
			t.Fatal(err)
		}
		n.cmd = exec.Command(filepath.Join(binDir, "unicd"))
		n.cmd.Env = n.env()
		n.cmd.Stdout = log
		n.cmd.Stderr = log
		if err := n.cmd.Start(); err != nil {
			t.Fatal(err)
		}
		n.done = make(chan struct{})
		cmd := n.cmd
		done := n.done
		go func() { _ = cmd.Wait(); _ = log.Close(); close(done) }()
		end := time.Now().Add(15 * time.Second)
		for time.Now().Before(end) {
			if n.listening() {
				return
			}
			select {
			case <-done:
				t.Fatal("service exited at startup")
			default:
			}
			time.Sleep(50 * time.Millisecond)
		}
		t.Fatal("service did not become ready")
	}
	stop := func(n *node) {
		t.Helper()
		children, _ := exec.Command("pgrep", "-P", strconv.Itoa(n.cmd.Process.Pid)).Output()
		n.kill(syscall.SIGTERM)
		select {
		case <-n.done:
		case <-time.After(10 * time.Second):
			n.kill(syscall.SIGKILL)
			t.Fatal("service cleanup timed out")
		}
		for _, child := range strings.Fields(string(children)) {
			pid, err := strconv.Atoi(child)
			if err != nil {
				t.Fatal(err)
			}
			end := time.Now().Add(3 * time.Second)
			for syscall.Kill(pid, 0) == nil && time.Now().Before(end) {
				time.Sleep(20 * time.Millisecond)
			}
			if syscall.Kill(pid, 0) == nil {
				t.Fatal("backend child survived service shutdown")
			}
		}
	}
	nodes := []*node{}
	for _, name := range []string{"alpha", "beta"} {
		n := &node{name: name, home: filepath.Join(work, name), sock: filepath.Join(work, name+".sock")}
		nodes = append(nodes, n)
		if err := os.MkdirAll(n.home, 0700); err != nil {
			t.Fatal(err)
		}
		if err := initializeFixtureProfile(filepath.Join(binDir, "unic"), n.home, n.env()); err != nil {
			t.Fatal(err)
		}
		cfg := fmt.Sprintf("device_name=%q\n[telemetry]\nenabled=false\n[plugins.instances.\"builtin.tailcat\"]\nenabled=false\n", name)
		if err := os.WriteFile(filepath.Join(n.home, "config.toml"), []byte(cfg), 0600); err != nil {
			t.Fatal(err)
		}
		run(n, true, "plugin", "install", "https://github.com/example/loopback", "--enable")
		t.Cleanup(func() {
			if n.done != nil {
				n.kill(syscall.SIGKILL)
				<-n.done
			}
		})
		start(n)
	}
	a, b := nodes[0], nodes[1]
	record("installed pinned external Go backend; both services started with tailcat disabled")
	request := func(n *node, method, path string, body any) (int, []byte) {
		t.Helper()
		raw, _ := json.Marshal(body)
		tr := &http.Transport{DialContext: func(ctx context.Context, _, _ string) (net.Conn, error) {
			return (&net.Dialer{}).DialContext(ctx, "unix", n.sock)
		}}
		defer tr.CloseIdleConnections()
		client := &http.Client{Transport: tr, Timeout: 10 * time.Second}
		req, _ := http.NewRequest(method, "http://unic"+path, strings.NewReader(string(raw)))
		resp, err := client.Do(req)
		if err != nil {
			t.Fatal(err)
		}
		defer func() { _ = resp.Body.Close() }()
		var out json.RawMessage
		if err := json.NewDecoder(resp.Body).Decode(&out); err != nil {
			t.Fatal(err)
		}
		return resp.StatusCode, out
	}
	for _, n := range nodes {
		code, data := request(n, "GET", "/v1/sync-backends", nil)
		if code != 200 || !strings.Contains(string(data), `"id":"e2e.tcp"`) || !strings.Contains(string(data), `"pairing":true`) {
			t.Fatalf("capability projection: %d %s", code, data)
		}
	}
	run(a, false, "device", "addr", "--backend", "missing.backend")
	ticket := strings.TrimSpace(run(a, true, "device", "addr", "--backend", "e2e.tcp"))
	code, _ := request(b, "POST", "/v1/peers", map[string]string{"addr": ticket})
	if code != 200 {
		t.Fatalf("pair: %d", code)
	}
	clients := []*api.Client{api.NewClient(a.sock), api.NewClient(b.sock)}
	var approvals []api.PendingPeer
	for _, c := range clients {
		pending, err := c.Pending(context.Background())
		if err != nil || len(pending) != 1 {
			t.Fatalf("pending approval missing: %v", err)
		}
		approvals = append(approvals, pending[0])
	}
	if approvals[0].Fingerprint != approvals[1].Fingerprint {
		t.Fatal("fixture fingerprints differ")
	}
	for i, c := range clients {
		p := approvals[i]
		bad := api.Approval{Fingerprint: "wrong", Revision: p.Revision, Bindings: p.Bindings}
		if _, err := c.Approve(context.Background(), p.ID, bad); err == nil {
			t.Fatal("wrong fingerprint accepted")
		}
		if _, err := c.Approve(context.Background(), p.ID, api.Approval{Fingerprint: p.Fingerprint, Revision: p.Revision, Bindings: p.Bindings}); err != nil {
			t.Fatal(err)
		}
	}
	record("paired through non-tailcat ticket; compared both fixture fingerprints; rejected wrong fingerprint; explicitly approved both sides")
	wait := func(check func() bool) {
		t.Helper()
		end := time.Now().Add(15 * time.Second)
		for time.Now().Before(end) {
			if check() {
				return
			}
			time.Sleep(50 * time.Millisecond)
		}
		t.Fatal("assertion deadline exceeded")
	}
	for _, c := range clients {
		wait(func() bool {
			s, err := c.Status(context.Background())
			return err == nil && len(s.Peers) == 1 && s.Peers[0].Connected
		})
	}
	for i, n := range nodes {
		other := clients[1-i]
		text := fmt.Sprintf("external-backend-%d", i)
		run(n, true, "send", "--to", "all", text, "--json")
		wait(func() bool { h, err := other.Clipboard(context.Background()); return err == nil && h.Text == text })
		out := run(n, true, "history", "list", "--json")
		if !strings.Contains(out, `"backend_id":"e2e.tcp"`) || !strings.Contains(out, `"state":"sent"`) {
			t.Fatal("missing per-backend sent result")
		}
	}
	record("bidirectional text through external backend; per-backend history results asserted")
	var pngData bytes.Buffer
	if err := png.Encode(&pngData, image.NewRGBA(image.Rect(0, 0, 3, 3))); err != nil {
		t.Fatal(err)
	}
	for i, n := range nodes {
		for _, kind := range []string{"image", "file"} {
			data := pngData.Bytes()
			snapshot := clip.Snapshot{Parts: []clip.Part{{Format: clip.PNG, Data: data}}}
			if kind == "file" {
				data = bytes.Repeat([]byte{byte(33 + i)}, 8192)
				file := filepath.Join(n.home, "synthetic.bin")
				if err := os.WriteFile(file, data, 0600); err != nil {
					t.Fatal(err)
				}
				snapshot = clip.Snapshot{Files: []clip.File{{Name: "synthetic.bin", Path: file, Size: int64(len(data))}}}
			}
			result, err := clients[i].Submit(context.Background(), api.SubmitInput{RequestID: fmt.Sprintf("payload-%d-%s", i, kind), Snapshot: snapshot})
			if err != nil || string(result.SyncDisposition) != "queued" {
				t.Fatalf("payload submit: %v", err)
			}
			other := nodes[1-i]
			wait(func() bool {
				got, err := testboard.Dir(filepath.Join(other.home, "board")).Read()
				if err != nil {
					return false
				}
				if kind == "image" {
					return len(got.Parts) == 1 && bytes.Equal(got.Parts[0].Data, data)
				}
				if len(got.Files) != 1 {
					return false
				}
				body, e := os.ReadFile(got.Files[0].Path)
				return e == nil && bytes.Equal(body, data)
			})
		}
	}
	record("bidirectional PNG and file payloads verified byte for byte through production API and directory adapter")

	run(a, true, "config", "set", `plugins.instances."e2e.tcp".sync.text`, "false")
	if out := run(a, true, "send", "--to", "all", "blocked-by-instance-policy", "--json"); !strings.Contains(out, `"sync_disposition":"policy_filtered"`) {
		t.Fatal("instance policy was not enforced")
	}
	run(a, true, "config", "set", `plugins.instances."e2e.tcp".sync.text`, "true")
	run(a, true, "config", "set", "sync.enabled", "false")
	if out := run(a, true, "send", "--to", "all", "blocked-globally", "--json"); !strings.Contains(out, `"sync_disposition":"disabled"`) {
		t.Fatal("global disable was not enforced")
	}
	record("instance policy and global disable enforced through CLI")
	run(a, true, "device", "remove", approvals[0].ID)
	stop(a)
	start(a)
	s, err := clients[0].Status(context.Background())
	if err != nil || len(s.Peers) != 0 {
		t.Fatal("revoked device revived after restart")
	}
	record("device revoked; service restarted without restoring trust")
	run(a, true, "plugin", "disable", "e2e.tcp")
	stop(a)
	start(a)
	backends, err := clients[0].SyncBackends(context.Background())
	if err != nil || len(backends) != 0 {
		t.Fatal("disabled external backend was restored or Tailcat was started")
	}
	run(a, false, "device", "addr", "--backend", "e2e.tcp")
	record("external backend disable persisted across restart; zero-backend mode and no fallback asserted")
	for _, n := range nodes {
		stop(n)
	}
	record("services and backend children stopped")
}
