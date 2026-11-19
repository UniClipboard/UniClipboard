// Command tracecloud accepts synthetic traces from production CLI processes.
// Hosted upload is restricted to the existing authorized UniClipboard project.
package main

import (
	"bytes"
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"encoding/pem"
	"errors"
	"flag"
	"fmt"
	"io"
	"net/http"
	"net/http/httptest"
	"os"
	"os/exec"
	"path/filepath"
	"reflect"
	"regexp"
	"runtime"
	"sort"
	"strings"
	"sync"
	"syscall"
	"time"

	collector "go.opentelemetry.io/proto/otlp/collector/trace/v1"
	tracepb "go.opentelemetry.io/proto/otlp/trace/v1"
	"google.golang.org/protobuf/encoding/protojson"
	"google.golang.org/protobuf/proto"

	api "github.com/uniclipboard/unic/internal/localapi"
)

func main() {
	if err := run(); err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
}
func digest(b []byte) string { s := sha256.Sum256(b); return hex.EncodeToString(s[:]) }
func writeJSON(dir, name string, v any) error {
	b, e := json.MarshalIndent(v, "", "  ")
	if e != nil {
		return e
	}
	return os.WriteFile(filepath.Join(dir, name), append(b, '\n'), 0600)
}
func sourceHashes(root string) (map[string]string, error) {
	cmd := exec.Command("git", "-C", root, "ls-files", "--cached", "--others", "--exclude-standard", "--", "go.mod", "go.sum", "engine", "internal", "plugin", "app/service", "sourceapp", "e2e/tracecloud")
	b, e := cmd.Output()
	if e != nil {
		return nil, e
	}
	out := map[string]string{}
	for _, name := range strings.Split(strings.TrimSpace(string(b)), "\n") {
		if name == "" {
			continue
		}
		b, e = os.ReadFile(filepath.Join(root, name))
		if e != nil {
			return nil, e
		}
		out[name] = digest(b)
	}
	return out, nil
}
func cleanEnv() []string {
	var out []string
	for _, e := range os.Environ() {
		k, _, _ := strings.Cut(e, "=")
		if strings.HasPrefix(k, "UNIC_") || strings.HasPrefix(k, "OTEL_") || k == "SSL_CERT_FILE" || k == "HOME" || k == "USERPROFILE" || k == "XDG_DATA_HOME" || k == "LOCALAPPDATA" || k == "UC_PROFILE" {
			continue
		}
		out = append(out, e)
	}
	return out
}
func run() (result error) {
	local := flag.Bool("local", false, "use a private TLS receiver instead of Cloud")
	artifact := flag.String("artifact", ".cache/tracecloud/cloud", "sanitized artifact directory")
	flag.Parse()
	root, err := os.Getwd()
	if err != nil {
		return err
	}
	dir, err := os.MkdirTemp("", "unic-tracecloud-")
	if err != nil {
		return err
	}
	dir, err = filepath.EvalSymlinks(dir)
	if err != nil {
		return err
	}
	if err = os.MkdirAll(*artifact, 0700); err != nil {
		return err
	}
	ctx, cancel := context.WithTimeout(context.Background(), 3*time.Minute)
	defer cancel()
	var daemon *exec.Cmd
	var done chan error
	var daemonEnv []string
	completed := false
	defer func() {
		forced := false
		if daemon != nil {
			_ = daemon.Process.Signal(syscall.SIGTERM)
			select {
			case childErr := <-done:
				_ = childErr // An earlier failed check may have stopped the child.
			case <-time.After(5 * time.Second):
				forced = true
				_ = daemon.Process.Kill()
				childErr := <-done
				_ = childErr // Forced termination is separately recorded above.
			}
		}
		cleanupErr := os.RemoveAll(dir)
		_, statErr := os.Stat(dir)
		cleanup := map[string]any{"private_workspace_removed": os.IsNotExist(statErr), "process_reaped": true, "forced_termination": forced, "credentials_retained": false, "binary_retained": false, "passed": completed && result == nil && cleanupErr == nil && !forced}
		if e := writeJSON(*artifact, "cleanup.json", cleanup); e != nil {
			result = errors.Join(result, e)
		}
		if e := checksums(*artifact); e != nil {
			result = errors.Join(result, e)
		}
		result = errors.Join(result, cleanupErr)
	}()
	host, token, project := os.Getenv("UNIC_POSTHOG_HOST"), os.Getenv("UNIC_POSTHOG_PROJECT_TOKEN"), os.Getenv("UNIC_POSTHOG_PROJECT_ID")
	var mu sync.Mutex
	var requests []*collector.ExportTraceServiceRequest
	var receivedBytes int
	var receiverErrors []string
	version := fmt.Sprintf("traceacceptance.%d", time.Now().UnixNano())
	if *local {
		token = "phc_synthetic_local_trace_token"
		project = "local"
		server := httptest.NewTLSServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
			b, e := io.ReadAll(io.LimitReader(r.Body, 1<<20))
			_ = r.Body.Close()
			mu.Lock()
			defer mu.Unlock()
			if e != nil || r.URL.Path != "/i/v1/traces" || r.Header.Get("Authorization") != "Bearer "+token {
				receiverErrors = append(receiverErrors, "unexpected request")
				http.Error(w, "invalid", 400)
				return
			}
			var p collector.ExportTraceServiceRequest
			if e = proto.Unmarshal(b, &p); e != nil {
				receiverErrors = append(receiverErrors, "invalid protobuf")
				http.Error(w, "invalid", 400)
				return
			}
			decoded, _ := protojson.Marshal(&p)
			for _, canary := range []string{dir, "tracecloud-body-canary", "tracecloud-request-canary", "tracecloud-env-canary", token} {
				if bytes.Contains(decoded, []byte(canary)) {
					receiverErrors = append(receiverErrors, "private canary")
					http.Error(w, "invalid", 400)
					return
				}
			}
			receivedBytes += len(b)
			requests = append(requests, &p)
			w.Header().Set("Content-Type", "application/x-protobuf")
			w.WriteHeader(200)
		}))
		defer server.Close()
		host = server.URL
		cert := filepath.Join(dir, "root.pem")
		if err = os.WriteFile(cert, pem.EncodeToMemory(&pem.Block{Type: "CERTIFICATE", Bytes: server.Certificate().Raw}), 0600); err != nil {
			return err
		}
		daemonEnv = append(daemonEnv, "SSL_CERT_FILE="+cert)
	} else if project != "722858" || host != "https://us.i.posthog.com" || !regexp.MustCompile(`^phc_[A-Za-z0-9_-]+$`).MatchString(token) {
		return errors.New("select the existing authorized UniClipboard project 722858 and its verified US Cloud ingestion credential")
	}
	source, err := sourceHashes(root)
	if err != nil {
		return err
	}
	headCmd := exec.Command("git", "rev-parse", "HEAD")
	head, _ := headCmd.Output()
	flags := "-X github.com/uniclipboard/unic/internal/observability.PostHogHost=" + host + " -X github.com/uniclipboard/unic/internal/observability.PostHogToken=" + token + " -X github.com/uniclipboard/unic/internal/observability.ProductVersion=" + version + " -X github.com/uniclipboard/unic/internal/observability.traceSampleRatio=1"
	binaryHashes := map[string]string{}
	for _, name := range []string{"unic", "unicd"} {
		cmd := exec.CommandContext(ctx, "go", "build", "-race", "-trimpath", "-buildvcs=false", "-ldflags", flags, "-o", filepath.Join(dir, name), "./app/service/cmd/"+name)
		cmd.Stdout, cmd.Stderr = io.Discard, io.Discard
		if err = cmd.Run(); err != nil {
			return fmt.Errorf("production %s build failed", name)
		}
		b, e := os.ReadFile(filepath.Join(dir, name))
		if e != nil {
			return e
		}
		binaryHashes[name] = digest(b)
	}
	home := filepath.Join(dir, "profile")
	discovery := filepath.Join(dir, "discovery")
	if err = os.MkdirAll(discovery, 0700); err != nil {
		return err
	}
	sock := filepath.Join(dir, "service.sock")
	daemonEnv = append(cleanEnv(), append(daemonEnv, "UNIC_HOME="+home, "UNIC_SOCK="+sock, "HOME="+discovery, "USERPROFILE="+discovery, "XDG_DATA_HOME="+filepath.Join(discovery, "data"), "LOCALAPPDATA="+filepath.Join(discovery, "local"), "UC_PROFILE=", "GORACE=atexit_sleep_ms=0", "OTEL_RESOURCE_ATTRIBUTES=secret=tracecloud-env-canary")...)
	init := exec.CommandContext(ctx, filepath.Join(dir, "unic"), "migrate", "--initialize-empty", "--target", home, "--json")
	init.Env = daemonEnv
	init.Stdout, init.Stderr = io.Discard, io.Discard
	if err = init.Run(); err != nil {
		return errors.New("private production profile initialization failed")
	}
	config := "[sync]\nenabled=false\n[telemetry]\nenabled=true\nendpoint=\"" + host + "/i\"\n[telemetry.logs]\nenabled=false\n[telemetry.metrics]\nenabled=false\n[telemetry.analytics]\nenabled=false\n[telemetry.traces]\nenabled=true\n"
	if err = os.WriteFile(filepath.Join(home, "config.toml"), []byte(config), 0600); err != nil {
		return err
	}
	if err = os.WriteFile(filepath.Join(*artifact, "config.toml"), []byte(config), 0600); err != nil {
		return err
	}
	started := time.Now().UTC()
	receipt := map[string]any{"project_id": project, "ingestion_host": host, "service_version": version, "started_at": started, "source_sha": strings.TrimSpace(string(head)), "source_files_sha256": source, "binaries_sha256": binaryHashes, "go": runtime.Version(), "os": runtime.GOOS, "arch": runtime.GOARCH, "build_flags": []string{"-race", "-trimpath", "-buildvcs=false", "-ldflags [exact-bound ingestion credential redacted; traceSampleRatio=1]"}, "test_build_tags": []string{}, "expected_roots": 3, "expected_children": 3, "send_commands": 4, "read_back": "PENDING", "batch_period_seconds": 5}
	if err = writeJSON(*artifact, "upload-receipt.json", receipt); err != nil {
		return err
	}
	daemon = exec.CommandContext(ctx, filepath.Join(dir, "unicd"))
	daemon.Env = daemonEnv
	daemon.Stdout, daemon.Stderr = io.Discard, io.Discard
	if err = daemon.Start(); err != nil {
		daemon = nil
		return errors.New("production daemon start failed")
	}
	done = make(chan error, 1)
	go func() { done <- daemon.Wait(); close(done) }()
	client := api.NewClient(sock)
	ready := false
	for i := 0; i < 100; i++ {
		if _, e := client.Telemetry(ctx); e == nil {
			ready = true
			break
		}
		time.Sleep(100 * time.Millisecond)
	}
	if !ready {
		return errors.New("production daemon readiness failed")
	}
	for _, i := range []int{0, 1, 1, 2} {
		cmd := exec.CommandContext(ctx, filepath.Join(dir, "unic"), "send", "--to", "all", fmt.Sprintf("tracecloud-body-canary-%d", i), "--request-id", fmt.Sprintf("tracecloud-request-canary-%d", i))
		cmd.Env = daemonEnv
		cmd.Stdout, cmd.Stderr = io.Discard, io.Discard
		if err = cmd.Run(); err != nil {
			return errors.New("synthetic production CLI send failed")
		}
	}
	select {
	case <-ctx.Done():
		return errors.New("acceptance deadline")
	case <-time.After(11 * time.Second):
	}
	state, err := client.Telemetry(ctx)
	if err != nil {
		return errors.New("telemetry query failed")
	}
	active := state.Traces.State == "active" && state.Traces.Failures == 0 && state.Traces.Dropped == 0 && !state.Logs.Requested && !state.Metrics.Requested && !state.Analytics.Requested
	receipt["local_export_state"] = state
	receipt["all_exporters_expected"] = active
	_ = daemon.Process.Signal(syscall.SIGTERM)
	select {
	case err = <-done:
		if err != nil {
			return errors.New("daemon shutdown failed")
		}
	case <-time.After(5 * time.Second):
		return errors.New("daemon shutdown deadline")
	}
	daemon = nil
	after, err := sourceHashes(root)
	if err != nil {
		return err
	}
	stable := reflect.DeepEqual(source, after)
	receipt["source_unchanged"] = stable
	receipt["finished_at"] = time.Now().UTC()
	if *local {
		mu.Lock()
		spans := []*tracepb.Span{}
		decoded := []json.RawMessage{}
		resourcesOK := len(requests) > 0
		for _, p := range requests {
			b, _ := protojson.Marshal(p)
			decoded = append(decoded, json.RawMessage(b))
			for _, rs := range p.ResourceSpans {
				want := map[string]string{"service.name": "unicd", "service.version": version, "os.type": runtime.GOOS, "host.arch": runtime.GOARCH}
				if len(rs.Resource.GetAttributes()) != len(want) {
					resourcesOK = false
				}
				for _, a := range rs.Resource.GetAttributes() {
					if v, ok := want[a.Key]; !ok || a.Value.GetStringValue() != v {
						resourcesOK = false
					}
				}
				for _, ss := range rs.ScopeSpans {
					spans = append(spans, ss.Spans...)
				}
			}
		}
		trees := validateTrees(spans, 3)
		receipt["trace_requests"] = len(requests)
		receipt["otlp_bytes"] = receivedBytes
		receipt["receiver_errors"] = receiverErrors
		receipt["local_tree_assertion"] = trees
		receipt["local_resource_assertion"] = resourcesOK
		receipt["read_back"] = "LOCAL_TLS_VERIFIED"
		mu.Unlock()
		if len(receiverErrors) > 0 || !trees || !resourcesOK {
			_ = writeJSON(*artifact, "upload-receipt.json", receipt)
			return errors.New("local decoded trace acceptance failed")
		}
		if err = writeJSON(*artifact, "decoded-spans.json", decoded); err != nil {
			return err
		}
	}
	if err = writeJSON(*artifact, "upload-receipt.json", receipt); err != nil {
		return err
	}
	if !active {
		return errors.New("trace exporter degraded or unrelated signal enabled")
	}
	if !stable {
		return errors.New("source changed during acceptance; repeat against frozen source")
	}
	completed = true
	fmt.Println("Synthetic production trace upload completed; sanitized receipt retained.")
	return nil
}
func validateTrees(spans []*tracepb.Span, want int) bool {
	roots := map[string]*tracepb.Span{}
	children := map[string][]*tracepb.Span{}
	for _, s := range spans {
		if len(s.TraceId) != 16 || len(s.SpanId) != 8 || s.EndTimeUnixNano < s.StartTimeUnixNano {
			return false
		}
		if len(s.Events) > 0 || len(s.Links) > 0 || s.TraceState != "" || s.Status.GetMessage() != "" || len(s.Attributes) != 1 || s.Attributes[0].Key != "result" || s.Attributes[0].Value.GetStringValue() != "succeeded" {
			return false
		}
		if s.Name == "unic.submission" && len(s.ParentSpanId) == 0 {
			roots[string(s.TraceId)] = s
		} else if s.Name == "unic.commit" {
			children[string(s.TraceId)] = append(children[string(s.TraceId)], s)
		} else {
			return false
		}
	}
	if len(roots) != want || len(spans) != want*2 {
		return false
	}
	for id, r := range roots {
		cs := children[id]
		if len(cs) != 1 || !bytes.Equal(cs[0].ParentSpanId, r.SpanId) || cs[0].StartTimeUnixNano < r.StartTimeUnixNano || cs[0].EndTimeUnixNano > r.EndTimeUnixNano {
			return false
		}
	}
	return true
}
func checksums(dir string) error {
	entries, e := os.ReadDir(dir)
	if e != nil {
		return e
	}
	var lines []string
	for _, entry := range entries {
		if entry.IsDir() || entry.Name() == "SHA256SUMS" {
			continue
		}
		b, e := os.ReadFile(filepath.Join(dir, entry.Name()))
		if e != nil {
			return e
		}
		lines = append(lines, digest(b)+"  "+entry.Name())
	}
	sort.Strings(lines)
	return os.WriteFile(filepath.Join(dir, "SHA256SUMS"), []byte(strings.Join(lines, "\n")+"\n"), 0600)
}
