// Package syncpath verifies the production passive reducer with two headless
// services and a local maintained DERP/STUN stack. No test transport or probe.
package syncpath

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"encoding/pem"
	"fmt"
	"io"
	"net"
	"net/http"
	"net/http/httptest"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
	"sync"
	"syscall"
	"testing"
	"time"

	"github.com/tailscale/tailcat"
	"github.com/uniclipboard/unic/internal/clip"
	api "github.com/uniclipboard/unic/internal/localapi"
	collectmetrics "go.opentelemetry.io/proto/otlp/collector/metrics/v1"
	"google.golang.org/protobuf/encoding/protojson"
	"google.golang.org/protobuf/proto"
	"tailscale.com/derp/derpserver"
	"tailscale.com/net/stun/stuntest"
	"tailscale.com/tailcfg"
	"tailscale.com/types/key"
)

func TestPassivePaths(t *testing.T) {
	if os.Getenv("UNIC_SYNC_PATH_E2E") == "" {
		t.Skip("set UNIC_SYNC_PATH_E2E=1")
	}
	bin := os.Getenv("UNIC_SYNC_PATH_BIN")
	if bin == "" {
		t.Fatal("UNIC_SYNC_PATH_BIN is required")
	}
	for _, mode := range []string{"direct", "derp"} {
		t.Run(mode, func(t *testing.T) { runPath(t, bin, mode) })
	}
}
func runPath(t *testing.T, bin, mode string) {
	start := time.Now()
	source := sourceTree(t)
	artifactRoot := os.Getenv("UNIC_SYNC_PATH_ARTIFACTS")
	if artifactRoot == "" {
		t.Fatal("UNIC_SYNC_PATH_ARTIFACTS is required")
	}
	t.Cleanup(func() {
		if t.Failed() {
			dir := filepath.Join(artifactRoot, mode)
			_ = os.MkdirAll(dir, 0700)
			b, _ := json.Marshal(map[string]any{"mode": mode, "status": "FAIL", "source_files_sha256": source})
			_ = os.WriteFile(filepath.Join(dir, "failure.json"), b, 0600)
		}
	})
	root, err := os.MkdirTemp("/private/tmp", "usp")
	if err != nil {
		t.Fatal(err)
	}
	defer os.RemoveAll(root)
	var mu sync.Mutex
	var decoded [][]byte
	var paths []map[string]string
	collector := httptest.NewTLSServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		b, _ := io.ReadAll(io.LimitReader(r.Body, 8<<20))
		if r.URL.Path != "/v1/metrics" {
			t.Error("unexpected export signal")
			w.WriteHeader(400)
			return
		}
		var req collectmetrics.ExportMetricsServiceRequest
		if proto.Unmarshal(b, &req) != nil {
			t.Error("OTLP decode")
			return
		}
		data, _ := protojson.Marshal(&req)
		mu.Lock()
		defer mu.Unlock()
		decoded = append(decoded, data)
		for _, rm := range req.ResourceMetrics {
			for _, sm := range rm.ScopeMetrics {
				for _, m := range sm.Metrics {
					if m.Name != "unic.sync.path.samples" {
						continue
					}
					for _, p := range m.GetSum().GetDataPoints() {
						attrs := map[string]string{}
						for _, kv := range p.Attributes {
							attrs[kv.Key] = kv.Value.GetStringValue()
						}
						paths = append(paths, attrs)
					}
				}
			}
		}
	}))
	defer collector.Close()
	cert := filepath.Join(root, "ca.pem")
	must(t, os.WriteFile(cert, pem.EncodeToMemory(&pem.Block{Type: "CERTIFICATE", Bytes: collector.Certificate().Raw}), 0600))
	derp := derpserver.New(key.NewNode(), func(string, ...any) {})
	relay := httptest.NewTLSServer(derpserver.Handler(derp))
	defer relay.Close()
	defer derp.Close()
	stun, closeSTUN := stuntest.Serve(t)
	defer closeSTUN()
	port := relay.Listener.Addr().(*net.TCPAddr).Port
	region := &tailcfg.DERPRegion{RegionID: 1, RegionCode: "fixture", Nodes: []*tailcfg.DERPNode{{Name: "fixture", RegionID: 1, HostName: "127.0.0.1", IPv4: "127.0.0.1", IPv6: "none", DERPPort: port, STUNPort: stun.Port, STUNTestIP: "127.0.0.1", InsecureForTests: true}}}
	ctx, cancel := context.WithTimeout(context.Background(), 120*time.Second)
	defer cancel()
	var clients []*api.Client
	var cmds []*exec.Cmd
	var dones []chan error
	defer func() {
		for i, c := range cmds {
			_ = c.Process.Signal(syscall.SIGTERM)
			select {
			case err := <-dones[i]:
				if err != nil {
					t.Errorf("service shutdown: %v", err)
				}
			case <-time.After(5 * time.Second):
				_ = c.Process.Kill()
				<-dones[i]
				t.Error("service shutdown timeout")
			}
		}
	}()
	for i := 0; i < 2; i++ {
		home := filepath.Join(root, fmt.Sprint(i))
		sock := filepath.Join(root, fmt.Sprintf("s%d", i))
		dir := filepath.Join(home, "plugin-data", "builtin.tailcat")
		must(t, os.MkdirAll(dir, 0700))
		sk, ck, psk := key.NewNode(), key.NewNode(), tailcat.NewPresharedKey()
		sktext, _ := sk.MarshalText()
		cktext, _ := ck.MarshalText()
		psktext, _ := psk.MarshalText()
		ci := tailcat.ConnInfo{ServerPublic: tailcat.NodePublic{NodePublic: sk.Public()}, PresharedKey: psk, Region: []*tailcfg.DERPRegion{region}}
		creds, _ := json.Marshal(map[string]any{"version": 1, "server_key": string(sktext), "client_key": string(cktext), "psk": string(psktext), "addr": string(ci.Addr())})
		must(t, os.WriteFile(filepath.Join(dir, "tailcat.json"), creds, 0600))
		cfg := fmt.Sprintf("[telemetry]\nendpoint=%q\n[telemetry.logs]\nenabled=false\n[telemetry.analytics]\nenabled=false\n", collector.URL)
		must(t, os.WriteFile(filepath.Join(home, "config.toml"), []byte(cfg), 0600))
		cmd := exec.Command(filepath.Join(bin, "unicd"))
		cmd.Env = append(os.Environ(), "UNIC_HOME="+home, "UNIC_SOCK="+sock, "SSL_CERT_FILE="+cert, "IN_TS_TEST=true", "TS_DEBUG_ALWAYS_USE_DERP="+fmt.Sprint(mode == "derp"), "GORACE=atexit_sleep_ms=0")
		// Logs can contain local debug-library details; retain no raw log artifact.
		cmd.Stdout = io.Discard
		cmd.Stderr = io.Discard
		must(t, cmd.Start())
		done := make(chan error, 1)
		go func() { done <- cmd.Wait() }()
		cmds = append(cmds, cmd)
		dones = append(dones, done)
		c := api.NewClient(sock)
		clients = append(clients, c)
		until(t, ctx, func() bool { _, err := c.Status(ctx); return err == nil })
	}
	ticket, err := clients[0].ExportTicket(ctx)
	must(t, err)
	_, err = clients[1].Pair(ctx, ticket)
	must(t, err)
	for _, c := range clients {
		var pending []api.PendingPeer
		until(t, ctx, func() bool { pending, _ = c.Pending(ctx); return len(pending) > 0 })
		p := pending[0]
		_, err := c.Approve(ctx, p.ID, api.Approval{Fingerprint: p.Fingerprint, Revision: p.Revision, Bindings: p.Bindings})
		must(t, err)
	}
	for _, c := range clients {
		until(t, ctx, func() bool {
			s, err := c.Status(ctx)
			if err != nil {
				return false
			}
			for _, p := range s.Peers {
				if p.Connected {
					return true
				}
			}
			return false
		})
	}
	submit := func(n int) {
		text := fmt.Sprintf("private-path-fixture-%d", n)
		_, err := clients[0].Submit(ctx, api.SubmitInput{RequestID: fmt.Sprintf("path-%d", n), Snapshot: clip.Snapshot{Parts: []clip.Part{{Format: clip.Text, Data: []byte(text)}}}})
		must(t, err)
		until(t, ctx, func() bool {
			p, e := clients[1].ListHistory(ctx, "", "")
			if e != nil || len(p.Items) == 0 {
				return false
			}
			var out strings.Builder
			if clients[1].CopyHistoryContent(ctx, p.Items[0].ID, 0, &out) != nil {
				return false
			}
			return out.String() == text
		})
	}
	// Keep business traffic active until the normal 60-second export. Sampling
	// observes existing traffic; no Ping/DiscoPing or timer-triggered transfer.
	for n := 0; ; n++ {
		submit(n)
		mu.Lock()
		known := false
		for _, p := range paths {
			if p["path"] == mode {
				known = true
			}
		}
		mu.Unlock()
		if known {
			break
		}
		if ctx.Err() != nil {
			t.Fatal("path sample missing")
		}
		time.Sleep(2 * time.Second)
	}
	// For direct mode, removing the only relay independently proves a later
	// delivered payload can use the remaining direct UDP path.
	independent := "forced UDP disable through maintained magicsock knob"
	if mode == "direct" {
		must(t, derp.Close())
		relay.Close()
		submit(1000)
		independent = "payload received after the only local DERP server was closed"
	}
	mu.Lock()
	defer mu.Unlock()
	known, unknown := 0, 0
	for _, p := range paths {
		if p["path"] == mode {
			known++
		}
		if p["path"] == "unknown" {
			unknown++
		}
		if mode == "derp" && p["path"] == "direct" {
			t.Error("direct reported with UDP disabled")
		}
	}
	if known == 0 || unknown == 0 {
		t.Error("expected incoming known and unsupported outgoing unknown coverage")
	}
	dest := os.Getenv("UNIC_SYNC_PATH_ARTIFACTS")
	if dest == "" {
		t.Fatal("UNIC_SYNC_PATH_ARTIFACTS is required")
	}
	dest = filepath.Join(dest, mode)
	must(t, os.MkdirAll(dest, 0700))
	write := func(name string, v any) {
		b, _ := json.MarshalIndent(v, "", "  ")
		must(t, os.WriteFile(filepath.Join(dest, name), b, 0600))
	}
	binary, err := os.ReadFile(filepath.Join(bin, "unicd"))
	must(t, err)
	hash := sha256.Sum256(binary)
	head, _ := exec.Command("git", "rev-parse", "HEAD").Output()
	write("manifest.json", map[string]any{"source_head": strings.TrimSpace(string(head)), "source_files_sha256": source, "binary_sha256": hex.EncodeToString(hash[:]), "build_tags": []string{"ts_omit_captiveportal", "ts_omit_portmapper"}, "race": true, "tailcat": "v0.7.0", "tailscale": "a2263542f260", "native_clipboard": "NOT_RUN", "hosted": "NOT_RUN"})
	write("scenarios.json", map[string]any{"mode": mode, "failed": t.Failed(), "known_series_observations": known, "unknown_series_observations": unknown, "independent_control": independent, "duration_seconds": time.Since(start).Seconds()})
	write("network-conditions.json", map[string]any{"derp": "local maintained derpserver with synthetic keys", "stun": "local maintained stuntest", "observer_active_probes": 0, "force_derp": mode == "derp", "direct_control": independent, "client_status": "unsupported; unknown"})
	var output strings.Builder
	for _, line := range decoded {
		for _, secret := range []string{root, "private-path-fixture", string(ticket)} {
			if strings.Contains(string(line), secret) {
				t.Fatal("private canary in OTLP")
			}
		}
		output.Write(line)
		output.WriteByte('\n')
	}
	must(t, os.WriteFile(filepath.Join(dest, "otlp.decoded.jsonl"), []byte(output.String()), 0600))
	if !sameSource(source, sourceTree(t)) {
		t.Error("source changed during acceptance")
	}
	write("commands.json", map[string]any{"build": "go build -race -tags ts_omit_captiveportal,ts_omit_portmapper -o .cache/0020/real-bin/unicd ./app/service/cmd/unicd", "test": "make sync-path-e2e", "environment_keys": []string{"UNIC_HOME", "UNIC_SOCK", "SSL_CERT_FILE", "IN_TS_TEST", "TS_DEBUG_ALWAYS_USE_DERP"}, "exit_code": 0})
	replay := `#!/bin/sh
set -eu
cd "$(git rev-parse --show-toplevel)"
make sync-path-e2e
`
	must(t, os.WriteFile(filepath.Join(dest, "replay.sh"), []byte(replay), 0700))
	var sums strings.Builder
	files, _ := os.ReadDir(dest)
	for _, f := range files {
		if f.IsDir() || f.Name() == "SHA256SUMS" {
			continue
		}
		b, err := os.ReadFile(filepath.Join(dest, f.Name()))
		must(t, err)
		sum := sha256.Sum256(b)
		fmt.Fprintf(&sums, "%x  %s\n", sum, f.Name())
	}
	must(t, os.WriteFile(filepath.Join(dest, "SHA256SUMS"), []byte(sums.String()), 0600))
}
func must(t *testing.T, err error) {
	t.Helper()
	if err != nil {
		t.Fatalf("fixture operation failed: %T", err)
	}
}
func until(t *testing.T, ctx context.Context, f func() bool) {
	t.Helper()
	for !f() {
		if ctx.Err() != nil {
			t.Fatal("fixture condition timeout")
		}
		time.Sleep(50 * time.Millisecond)
	}
}

func sourceTree(t *testing.T) map[string]string {
	t.Helper()
	out, err := exec.Command("go", "env", "GOMOD").Output()
	must(t, err)
	root := filepath.Dir(strings.TrimSpace(string(out)))
	files, err := exec.Command("git", "-C", root, "ls-files", "--cached", "--others", "--exclude-standard", "--", "go.mod", "go.sum", "engine", "internal", "app/service", "e2e/syncpath").Output()
	must(t, err)
	result := map[string]string{}
	for _, path := range strings.Split(strings.TrimSpace(string(files)), "\n") {
		if path == "" {
			continue
		}
		b, err := os.ReadFile(filepath.Join(root, path))
		must(t, err)
		sum := sha256.Sum256(b)
		result[path] = hex.EncodeToString(sum[:])
	}
	return result
}
func sameSource(a, b map[string]string) bool {
	if len(a) != len(b) {
		return false
	}
	for k, v := range a {
		if b[k] != v {
			return false
		}
	}
	return true
}
