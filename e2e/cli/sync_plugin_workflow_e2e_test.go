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
	"github.com/uniclipboard/unic/internal/rendezvous"
	"github.com/uniclipboard/unic/internal/rendezvous/rvtest"
	"github.com/uniclipboard/unic/internal/testboard"
)

// Failure inventory and retry limits are recorded before implementation in
// docs/exec/sync-plugin-workflow.md. No system clipboard or external network.
func TestSyncPluginWorkflow(t *testing.T) {
	for _, multi := range []bool{false, true} {
		name := "singleton"
		if multi {
			name = "instances"
		}
		t.Run(name, func(t *testing.T) { runSyncPluginWorkflow(t, multi) })
	}
}

func runSyncPluginWorkflow(t *testing.T, multi bool) {
	if !multi {
		t.Setenv("UNIC_E2E_ADAPTER_DELAY", "2s")
	}
	if os.Getenv("UNIC_E2E") == "" {
		t.Skip("set UNIC_E2E=1")
	}
	root, err := repoRoot()
	if err != nil {
		t.Fatal(err)
	}
	work := shortTemp(t)
	rv := rvtest.New()
	rv.TTL = 5 * time.Second
	defer rv.Close()
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
	schema, err := os.ReadFile(filepath.Join(root, "plugins/examples/loopback/settings.schema.json"))
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
settings_schema="settings.schema.json"
settings_sha256="%x"
transport_kind="peer_sessions"
[[artifacts]]
goos=%q
goarch=%q
release="v1.0.0"
asset="loopback.tar.gz"
executable="loopback"
sha256="%x"
`, sha256.Sum256(schema), runtime.GOOS, runtime.GOARCH, sha256.Sum256(asset))
	source := pluginArchive(t, map[string][]byte{"repo/spec.toml": []byte(manifest), "repo/settings.schema.json": schema})
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
			_ = os.WriteFile(filepath.Join(dest, "sync-plugin-workflow-"+map[bool]string{false: "singleton", true: "instances"}[multi]+".json"), data, 0600)
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
				// A successful production clipboard read is acknowledged only
				// after adapter baseline and ready have completed. Service
				// readiness alone does not permit automatic receive application.
				ctx, cancel := context.WithTimeout(context.Background(), time.Second)
				_, readyErr := api.NewClient(n.sock).Clipboard(ctx)
				cancel()
				if readyErr == nil {
					return
				}
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
		n := &node{rv: rv.URL, name: name, home: filepath.Join(work, name), sock: filepath.Join(work, name+".sock")}
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
		if multi {
			run(n, true, "plugin", "disable", "e2e.tcp")
			run(n, true, "plugin", "instance", "add", "e2e.tcp", "e2e."+name)
			run(n, true, "plugin", "instance", "add", "e2e.tcp", "e2e.second_"+name)
			for _, instance := range []string{"e2e." + name, "e2e.second_" + name} {
				file := filepath.Join(work, instance+".json")
				if err := os.WriteFile(file, []byte(`{"values":{"marker":"`+instance+`","credential":"synthetic-private-`+instance+`"}}`), 0600); err != nil {
					t.Fatal(err)
				}
				run(n, true, "plugin", "settings", "save", instance, file)
				out := run(n, true, "plugin", "settings", "show", instance)
				if strings.Contains(out, "synthetic-private-") || !strings.Contains(out, instance) {
					t.Fatal("instance settings leaked a secret or lost its marker")
				}
			}
			run(n, true, "plugin", "enable", "e2e."+name)
			run(n, true, "plugin", "enable", "e2e.second_"+name)
		}
		// Settings commands own temporary production services. Wait for their
		// documented release before starting this scenario's standalone owner.
		deadline := time.Now().Add(10 * time.Second)
		for n.listening() && time.Now().Before(deadline) {
			time.Sleep(25 * time.Millisecond)
		}
		if n.listening() {
			t.Fatal("temporary settings service did not release its owner")
		}
		start(n)
		if multi {
			for _, instance := range []string{"e2e." + name, "e2e.second_" + name} {
				data, err := os.ReadFile(filepath.Join(n.home, "plugin-data", instance, "marker"))
				if err != nil || string(data) != instance {
					t.Fatalf("runtime instance marker %s: %q (%v)", instance, data, err)
				}
			}
		}
	}
	backendA, backendB := "e2e.tcp", "e2e.tcp"
	if multi {
		backendA, backendB = "e2e.alpha", "e2e.beta"
	}
	a, b := nodes[0], nodes[1]
	t.Cleanup(func() {
		if !t.Failed() {
			return
		}
		for _, n := range nodes {
			ctx, cancel := context.WithTimeout(context.Background(), 3*time.Second)
			client := api.NewClient(n.sock)
			status, se := client.Status(ctx)
			capabilities, ce := client.Capabilities(ctx)
			history, he := client.QueryHistory(ctx, api.HistoryQuery{Limit: 20})
			received, re := client.Received(ctx)
			cancel()
			traces := map[string]string{}
			ids := []string{"e2e.tcp"}
			if multi {
				ids = []string{"e2e." + n.name, "e2e.second_" + n.name}
			}
			for _, id := range ids {
				raw, _ := os.ReadFile(filepath.Join(n.home, "plugin-data", id, "fixture-trace.json"))
				traces[id] = string(raw)
			}
			data, _ := json.MarshalIndent(map[string]any{"status": status, "capabilities": capabilities, "history": history, "received": received, "errors": fmt.Sprint(se, ce, he, re), "fixture_trace": traces}, "", "  ")
			t.Logf("failure diagnostic %s: %s", n.name, data)
			if dest := os.Getenv("UNIC_E2E_ARTIFACTS"); dest != "" {
				_ = os.WriteFile(filepath.Join(dest, "failure-"+map[bool]string{false: "singleton", true: "instances"}[multi]+"-"+n.name+".json"), data, 0600)
			}
		}
	})

	record("installed pinned external Go backend; both services started with tailcat disabled")
	if multi {
		record("per-instance Schema values, secret redaction and private runtime marker directories verified independently")
	}
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
	for i, n := range nodes {
		code, data := request(n, "GET", "/v1/sync-backends", nil)
		if code != 200 || !strings.Contains(string(data), `"id":"`+[]string{backendA, backendB}[i]+`"`) || !strings.Contains(string(data), `"pairing":true`) {
			t.Fatalf("capability projection: %d %s", code, data)
		}
	}
	run(a, false, "device", "addr", "--backend", "missing.backend")
	ticket := strings.TrimSpace(run(a, true, "device", "addr", "--backend", backendA))
	if multi {
		code, _ := request(b, "POST", "/v1/peers", map[string]string{"addr": ticket})
		if code == 200 {
			t.Fatal("implicit backend fallback accepted an alias ticket")
		}
		_, _ = request(a, "DELETE", "/v1/code", nil)
		code, _ = request(b, "POST", "/v1/peers", map[string]string{"addr": ticket, "backend_id": backendB})
		if code == 200 {
			t.Fatal("cancelled ticket was accepted")
		}
		secondaryText := strings.TrimSpace(run(a, true, "device", "addr", "--backend", "e2e.second_alpha"))
		secondary, ok := rendezvous.ParseText(secondaryText)
		if !ok {
			t.Fatal("secondary fixture ticket parse")
		}
		ticket = strings.TrimSpace(run(a, true, "device", "addr", "--backend", backendA))
		forged, ok := rendezvous.ParseText(ticket)
		if !ok {
			t.Fatal("primary fixture ticket parse")
		}
		forged.Access = secondary.Access
		code, _ = request(b, "POST", "/v1/peers", map[string]string{"addr": forged.Text(), "backend_id": backendB})
		if code == 200 {
			t.Fatal("ticket redeemed on a different issuer instance")
		}
		clientA := api.NewClient(a.sock)
		expired, err := clientA.NewCodeVia(context.Background(), backendA)
		if err != nil {
			t.Fatal(err)
		}
		expiredTicket, err := clientA.ExportTicketVia(context.Background(), backendA)
		if err != nil {
			t.Fatal(err)
		}
		time.Sleep(time.Until(time.UnixMilli(expired.ExpiresAtMs)) + 50*time.Millisecond)
		code, _ = request(b, "POST", "/v1/join", map[string]string{"code": expired.Code, "backend_id": backendB})
		if code == 200 {
			t.Fatal("expired code accepted")
		}
		code, _ = request(b, "POST", "/v1/peers", map[string]string{"addr": expiredTicket, "backend_id": backendB})
		if code == 200 {
			t.Fatal("expired direct ticket accepted")
		}
		fresh, err := clientA.NewCodeVia(context.Background(), backendA)
		if err != nil {
			t.Fatal(err)
		}
		code, _ = request(b, "POST", "/v1/join", map[string]string{"code": fresh.Code, "backend_id": backendB})
		if code != 200 {
			t.Fatalf("selected code join: %d", code)
		}
		record("selected local instances used real pairing codes; cancelled/expired tickets, expired codes and wrong issuer-instance redemption rejected")
	} else {
		code, _ := request(b, "POST", "/v1/peers", map[string]string{"addr": ticket, "backend_id": backendB})
		if code != 200 {
			t.Fatalf("pair: %d", code)
		}
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
		if !strings.Contains(out, `"backend_id":"`+[]string{backendA, backendB}[i]+`"`) || !strings.Contains(out, `"state":"sent"`) {
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

	if multi {
		secondTicket := strings.TrimSpace(run(a, true, "device", "addr", "--backend", "e2e.second_alpha"))
		code, _ := request(b, "POST", "/v1/peers", map[string]string{"addr": secondTicket, "backend_id": "e2e.second_beta"})
		if code != 200 {
			t.Fatalf("second path: %d", code)
		}
		for _, c := range clients {
			pending, err := c.Pending(context.Background())
			if err != nil || len(pending) != 1 || len(pending[0].Bindings) != 1 {
				t.Fatal("trusted device did not require new path approval")
			}
			p := pending[0]
			if _, err := c.Approve(context.Background(), p.ID, api.Approval{Fingerprint: p.Fingerprint, Revision: p.Revision - 1, Bindings: p.Bindings}); err == nil {
				t.Fatal("stale revision accepted")
			}
			if _, err := c.Approve(context.Background(), p.ID, api.Approval{Fingerprint: p.Fingerprint, Revision: p.Revision, Bindings: p.Bindings}); err != nil {
				t.Fatal(err)
			}
		}
		for _, n := range nodes {
			wait(func() bool {
				_, raw := request(n, "GET", "/v1/status", nil)
				var st struct {
					Peers []struct{ Bindings []struct{ Connection string } }
				}
				return json.Unmarshal(raw, &st) == nil && len(st.Peers) == 1 && len(st.Peers[0].Bindings) == 2 && st.Peers[0].Bindings[0].Connection == "connected" && st.Peers[0].Bindings[1].Connection == "connected"
			})
		}
		before, _ := clients[1].QueryHistory(context.Background(), api.HistoryQuery{Limit: 100})
		run(a, true, "send", "--to", "all", "two-path-dedup", "--json")
		wait(func() bool {
			after, err := clients[1].QueryHistory(context.Background(), api.HistoryQuery{Limit: 100})
			return err == nil && len(after.Items) == len(before.Items)+1
		})
		// Observe the completed two-path operation before asserting no second history row.
		out := run(a, true, "history", "list", "--json")
		if !strings.Contains(out, `"backend_id":"e2e.second_alpha"`) {
			t.Fatal("second result absent")
		}
		time.Sleep(250 * time.Millisecond)
		after, _ := clients[1].QueryHistory(context.Background(), api.HistoryQuery{Limit: 100})
		if len(after.Items) != len(before.Items)+1 {
			t.Fatal("duplicate delivery committed twice")
		}
		for _, fault := range []string{"fail-send", "crash-send"} {
			path := filepath.Join(a.home, "plugin-data", "e2e.second_alpha", fault)
			if err := os.WriteFile(path, []byte("owned fixture control"), 0600); err != nil {
				t.Fatal(err)
			}
			out := run(a, false, "send", "--to", "all", "partial-"+fault, "--json")
			if !strings.Contains(out, `"outcome":"partial"`) {
				t.Fatalf("partial failure was not retained: %s", out)
			}
			page, err := clients[0].QueryHistory(context.Background(), api.HistoryQuery{Limit: 1})
			if err != nil || len(page.Items) != 1 {
				t.Fatal("partial history absent")
			}
			states := map[string]string{}
			for _, backend := range page.Items[0].Backends {
				states[backend.BackendID] = backend.State
			}
			if states[backendA] != "sent" || states["e2e.second_alpha"] != "failed" {
				t.Fatalf("partial backend results: %v", states)
			}
			_ = os.Remove(path)
			for _, n := range nodes {
				wait(func() bool {
					status, err := api.NewClient(n.sock).Status(context.Background())
					if err != nil || len(status.Peers) != 1 {
						return false
					}
					for _, binding := range status.Peers[0].Bindings {
						if binding.Connection != "connected" {
							return false
						}
					}
					return true
				})
			}
			afterRecovery, _ := clients[1].QueryHistory(context.Background(), api.HistoryQuery{Limit: 100})
			time.Sleep(250 * time.Millisecond)
			later, _ := clients[1].QueryHistory(context.Background(), api.HistoryQuery{Limit: 100})
			if len(later.Items) != len(afterRecovery.Items) {
				t.Fatal("backend restart replayed uncertain content")
			}
		}
		record("per-instance transport failure and child crash preserve other-backend success; recovery does not replay uncertain content")
		run(a, true, "config", "set", `plugins.instances."e2e.second_alpha".sync.text`, "false")
		record("same implementation has two isolated instances; cross-instance pairing; new path approval; stale revision rejection; one device row and cross-backend dedup")
		status, _ := clients[0].Status(context.Background())
		var primary, second apiBindingSnapshot
		for _, path := range status.Peers[0].Bindings {
			if path.InstanceID == backendA {
				primary = apiBindingSnapshot{path.ID, path.Revision}
			} else {
				second = apiBindingSnapshot{path.ID, path.Revision}
			}
		}
		code, raw := request(a, "PATCH", "/v1/bindings/"+primary.ID, map[string]any{"revision": primary.Revision, "enabled": false})
		if code != 200 {
			t.Fatalf("pause path: %d", code)
		}
		var updated apiBindingSnapshot
		if json.Unmarshal(raw, &updated) != nil {
			t.Fatal("invalid path response")
		}
		code, _ = request(a, "PATCH", "/v1/bindings/"+primary.ID, map[string]any{"revision": primary.Revision, "enabled": true})
		if code != 409 {
			t.Fatal("stale path revision accepted")
		}
		code, _ = request(a, "PATCH", "/v1/bindings/"+primary.ID, map[string]any{"revision": updated.Revision, "enabled": true})
		if code != 200 {
			t.Fatal("path resume failed")
		}
		code, _ = request(a, "DELETE", fmt.Sprintf("/v1/bindings/%s?revision=%d", second.ID, second.Revision), nil)
		if code != 200 {
			t.Fatal("path removal failed")
		}
		status, _ = clients[0].Status(context.Background())
		if len(status.Peers) != 1 || len(status.Peers[0].Bindings) != 1 {
			t.Fatal("path removal removed the device or its other path")
		}
		record("path pause/resume/remove uses exact revisions and preserves the other device path")

	}
	run(a, true, "config", "set", `plugins.instances."`+backendA+`".sync.text`, "false")
	if out := run(a, true, "send", "--to", "all", "blocked-by-instance-policy", "--json"); !strings.Contains(out, `"sync_disposition":"policy_filtered"`) {
		t.Fatal("instance policy was not enforced")
	}
	run(a, true, "config", "set", `plugins.instances."`+backendA+`".sync.text`, "true")
	run(a, true, "config", "set", "sync.enabled", "false")
	if out := run(a, true, "send", "--to", "all", "blocked-globally", "--json"); !strings.Contains(out, `"sync_disposition":"disabled"`) {
		t.Fatal("global disable was not enforced")
	}
	record("instance policy and global disable enforced through CLI")
	if multi {
		file := filepath.Join(work, "changed-instance-settings.json")
		if err := os.WriteFile(file, []byte(`{"values":{"marker":"changed-runtime-context"}}`), 0600); err != nil {
			t.Fatal(err)
		}
		run(a, true, "plugin", "settings", "save", backendA, file)
		stop(a)
		start(a)
		status, err := clients[0].Status(context.Background())
		if err != nil {
			t.Fatal(err)
		}
		for _, peer := range status.Peers {
			for _, path := range peer.Bindings {
				if path.InstanceID == backendA && path.State == "authorized" {
					t.Fatal("changed instance config inherited old path authorization")
				}
			}
		}
		record("backend settings change atomically invalidated that instance's old path authorization on restart")
	}

	run(a, true, "device", "remove", approvals[0].ID)
	stop(a)
	start(a)
	s, err := clients[0].Status(context.Background())
	if err != nil || len(s.Peers) != 0 {
		t.Fatal("revoked device revived after restart")
	}
	record("device revoked; service restarted without restoring trust")
	run(a, true, "plugin", "disable", backendA)
	if multi {
		run(a, true, "plugin", "disable", "e2e.second_alpha")
	}
	stop(a)
	start(a)
	backends, err := clients[0].SyncBackends(context.Background())
	if err != nil || len(backends) != 0 {
		t.Fatal("disabled external backend was restored or Tailcat was started")
	}
	run(a, false, "device", "addr", "--backend", backendA)
	record("external backend disable persisted across restart; zero-backend mode and no fallback asserted")
	for _, n := range nodes {
		stop(n)
	}
	record("services and backend children stopped")
}

type apiBindingSnapshot struct {
	ID       string `json:"id"`
	Revision int64  `json:"revision"`
}
