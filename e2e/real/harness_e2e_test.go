package main

import (
	"context"
	"encoding/csv"
	"encoding/json"
	"fmt"
	"io"
	"os"
	"os/exec"
	"path/filepath"
	"runtime"
	"strconv"
	"strings"
	"syscall"
	"testing"
	"time"

	api "github.com/uniclipboard/unic/internal/localapi"
)

// TestHarnessProcesses crosses executable/stdio/local-service boundaries. It
// does not replace the independent multi-host run or assert internal helpers.
func TestHarnessProcesses(t *testing.T) {
	if os.Getenv("UNIC_REAL_E2E") == "" {
		t.Skip("set UNIC_REAL_E2E=1")
	}
	ctx, cancel := context.WithTimeout(context.Background(), 4*time.Minute)
	t.Cleanup(cancel)
	root, err := filepath.Abs("../..")
	if err != nil {
		t.Fatal(err)
	}
	artifacts := os.Getenv("UNIC_REAL_E2E_ARTIFACTS")
	if artifacts == "" {
		artifacts = t.TempDir()
	}
	artifacts, err = filepath.Abs(artifacts)
	if err != nil {
		t.Fatal(err)
	}
	if err = os.MkdirAll(artifacts, 0700); err != nil {
		t.Fatal(err)
	}
	receipt := map[string]any{"status": "running", "scope": "local production process/CLI acceptance; not additional physical-machine evidence"}
	checks := []string{}
	defer func() {
		receipt["checks"] = checks
		receipt["status"] = "passed"
		if t.Failed() {
			receipt["status"] = "failed"
		}
		b, _ := json.MarshalIndent(receipt, "", "  ")
		if err := os.WriteFile(filepath.Join(artifacts, "manifest.json"), append(b, '\n'), 0600); err != nil {
			t.Error(err)
		}
	}()
	call := func(dir string, name string, args ...string) []byte {
		t.Helper()
		cmd := exec.CommandContext(ctx, name, args...)
		cmd.Dir = dir
		b, err := cmd.CombinedOutput()
		if err != nil {
			t.Fatalf("command %s failed: %v\n%s", name, err, b)
		}
		return b
	}
	tool := filepath.Join(artifacts, exe("unic-e2e"))
	call(root, "go", "build", "-o", tool, "./e2e/real")
	localLab := filepath.Join(artifacts, "local.toml")
	if err = os.WriteFile(localLab, []byte("version=1\n[nodes.local]\ntransport=\"local\"\n"), 0600); err != nil {
		t.Fatal(err)
	}
	doctor := filepath.Join(artifacts, "doctor")
	call(root, tool, "doctor", "--lab", localLab, "--out", doctor)
	var recorded manifest
	b, err := os.ReadFile(filepath.Join(doctor, "public", "manifest.json"))
	if err != nil {
		t.Fatal(err)
	}
	if err = json.Unmarshal(b, &recorded); err != nil {
		t.Fatal(err)
	}
	if recorded.Status != "passed" || recorded.MachineCount != 1 || !recorded.SnapshotUnchanged {
		t.Fatal("doctor did not verify the local worker")
	}
	if !recorded.PrivateRemoved || recorded.KeepPrivate {
		t.Fatal("successful doctor did not default to private cleanup")
	}
	if _, err = os.Stat(filepath.Join(doctor, "private")); !os.IsNotExist(err) {
		t.Fatal("successful doctor left its controller private directory")
	}
	for _, n := range recorded.Nodes {
		if n.Cleanup == nil || !n.Cleanup.Removed {
			t.Fatal("successful doctor left a node profile")
		}
	}
	receipt["source"] = recorded.Source
	checks = append(checks, "successful doctor removes node and controller private artifacts and retains its public receipt")
	doctor = filepath.Join(artifacts, "retained-doctor")
	call(root, tool, "doctor", "--lab", localLab, "--out", doctor, "--keep-private")
	retainedNodes, err := os.ReadFile(filepath.Join(doctor, "private", "nodes.json"))
	if err != nil {
		t.Fatal("explicit retention did not preserve the private inventory")
	}
	var retained map[string]struct{ Root string }
	if err = json.Unmarshal(retainedNodes, &retained); err != nil {
		t.Fatal(err)
	}
	for _, n := range retained {
		if _, err = os.Stat(n.Root); err != nil {
			t.Fatal("explicit retention did not preserve the node profile")
		}
	}
	t.Cleanup(func() {
		for _, n := range retained {
			_ = os.RemoveAll(n.Root)
		}
		_ = os.RemoveAll(filepath.Join(doctor, "private"))
	})
	checks = append(checks, "keep-private explicitly retains doctor profiles and controller snapshot")
	frozen := filepath.Join(doctor, "private", "source")
	bin := filepath.Join(artifacts, "programs")
	if err = os.MkdirAll(bin, 0700); err != nil {
		t.Fatal(err)
	}
	build := exec.CommandContext(ctx, "go", "build", "-trimpath", "-buildvcs=false", "-o", bin+string(filepath.Separator), "./app/service/cmd/unic", "./app/service/cmd/unicd")
	build.Dir = frozen
	build.Env = append(os.Environ(), "CGO_ENABLED=0", "GOWORK=off", "GOFLAGS=")
	if b, err = build.CombinedOutput(); err != nil {
		t.Fatalf("production build: %v %s", err, b)
	}
	workerPath := filepath.Join(doctor, "private", "artifacts", runtime.GOOS+"-"+runtime.GOARCH, exe("unic-e2e-worker"))
	receipt["worker_sha256"], _, err = fileSHA(workerPath)
	if err != nil {
		t.Fatal(err)
	}
	worker := exec.CommandContext(ctx, workerPath, "worker", "--run-id", recorded.RunID, "--node", "eof")
	input, err := worker.StdinPipe()
	if err != nil {
		t.Fatal(err)
	}
	output, err := worker.StdoutPipe()
	if err != nil {
		t.Fatal(err)
	}
	errLog, err := os.OpenFile(filepath.Join(artifacts, "eof-worker.log"), os.O_CREATE|os.O_WRONLY, 0600)
	if err != nil {
		t.Fatal(err)
	}
	defer func() { _ = errLog.Close() }()
	worker.Stderr = errLog
	if err = worker.Start(); err != nil {
		t.Fatal(err)
	}
	done := make(chan error, 1)
	go func() { done <- worker.Wait() }()
	enc, dec := json.NewEncoder(input), json.NewDecoder(output)
	sequence := uint64(0)
	rpc := func(method string, value, out any) {
		t.Helper()
		sequence++
		b, err := json.Marshal(value)
		if err != nil {
			t.Fatal(err)
		}
		if err = enc.Encode(request{ID: sequence, Method: method, Value: b}); err != nil {
			t.Fatal(err)
		}
		var r response
		if err = dec.Decode(&r); err != nil {
			t.Fatalf("worker response: %v (see private worker log)", err)
		}
		if r.ID != sequence || r.Error != "" {
			t.Fatalf("worker method %s failed: %s", method, r.Error)
		}
		if out != nil {
			if err = json.Unmarshal(r.Value, out); err != nil {
				t.Fatal(err)
			}
		}
	}
	var h hello
	rpc("hello", nil, &h)
	client := api.NewClient(h.Socket)
	var serviceID string
	servicePID := 0
	exited := false
	t.Cleanup(func() {
		if serviceID != "" {
			cleanup, stop := context.WithTimeout(context.Background(), 2*time.Second)
			_ = client.StopService(cleanup, serviceID)
			stop()
		}
		_ = input.Close()
		if !exited {
			select {
			case err := <-done:
				_ = err // Cleanup waits for termination, not a successful command.
				exited = true
			case <-time.After(10 * time.Second):
				_ = worker.Process.Kill()
			}
		}
		if exited && servicePID > 0 && !processExists(servicePID) {
			_ = os.RemoveAll(h.Root)
		}
	})
	hashes := map[string]string{}
	for _, name := range []string{exe("unic"), exe("unicd")} {
		from := filepath.Join(bin, name)
		data, err := os.ReadFile(from)
		if err != nil {
			t.Fatal(err)
		}
		if err = os.WriteFile(filepath.Join(h.Root, "bin", name), data, 0700); err != nil {
			t.Fatal(err)
		}
		hashes[name] = digest(data)
	}
	plugins := map[string][]byte{}
	for _, name := range []string{"sensitive", "emailtag"} {
		for _, file := range []string{"spec.toml", "main.lua"} {
			b, err := os.ReadFile(filepath.Join(frozen, "plugins", "examples", "lua", name, file))
			if err != nil {
				t.Fatal(err)
			}
			plugins[name+"/"+file] = b
		}
	}
	rpc("configure", installConfig{Name: "eof", Hashes: hashes, Plugins: plugins}, nil)
	rpc("start", nil, nil)
	instance, err := client.Service(ctx)
	if err != nil {
		t.Fatal(err)
	}
	serviceID = instance.ID
	servicePID = instance.PID
	var f fixture
	rpc("prepare", prepareRequest{Payload: payload{"text", 32}, Seed: "EOF-control-fixture"}, &f)
	var submitted submitResult
	rpc("submit", submitRequest{Fixture: f.ID, Key: "eof-control-commit"}, &submitted)
	if submitted.Result.HistoryID == "" || submitted.Result.SyncDisposition != "no_targets" {
		t.Fatal("production service did not commit before EOF")
	}
	status, err := client.Status(ctx)
	if err != nil {
		t.Fatal(err)
	}
	rpc("arm", armRequest{Source: status.ID, Fixture: f}, nil)
	sequence++
	waitBody, _ := json.Marshal(waitRequest{TimeoutSeconds: 1})
	started := time.Now()
	if err = enc.Encode(request{ID: sequence, Method: "wait", Value: waitBody}); err != nil {
		t.Fatal(err)
	}
	var timeoutReply response
	if err = dec.Decode(&timeoutReply); err != nil {
		t.Fatal(err)
	}
	elapsed := time.Since(started)
	if timeoutReply.ID != sequence || timeoutReply.Error != "receive_timeout" || elapsed < time.Second || elapsed > 5*time.Second {
		t.Fatal("configured receive deadline did not return the expected failure")
	}
	receipt["receive_timeout_ms"] = elapsed.Milliseconds()
	checks = append(checks, "one-second receive deadline fails through the real worker and running production service")
	if err = input.Close(); err != nil {
		t.Fatal(err)
	}
	select {
	case err = <-done:
		exited = true
		if err != nil {
			t.Fatalf("worker EOF exit: %v (see private worker log)", err)
		}
	case <-time.After(40 * time.Second):
		t.Fatal("worker did not finish after control EOF")
	}
	if _, err = os.Stat(h.Root); !os.IsNotExist(err) {
		t.Fatal("worker did not remove its profile after EOF")
	}
	if _, err = client.Status(ctx); err == nil {
		t.Fatal("owned service still accepts requests after EOF")
	}
	if processExists(instance.PID) {
		t.Fatal("owned service PID remains alive after worker EOF")
	}
	receipt["production_sha256"] = hashes
	checks = append(checks, "control EOF stops the actual service PID, removes the profile and closes the socket")
	duplicate := filepath.Join(artifacts, "duplicate.toml")
	if err = os.WriteFile(duplicate, []byte("version=1\n[nodes.a]\ntransport=\"local\"\n[nodes.b]\ntransport=\"local\"\n"), 0600); err != nil {
		t.Fatal(err)
	}
	rejected := filepath.Join(artifacts, "rejected")
	cmd := exec.CommandContext(ctx, tool, "doctor", "--lab", duplicate, "--out", rejected)
	cmd.Dir = root
	if _, err = cmd.CombinedOutput(); err == nil {
		t.Fatal("duplicate machine aliases were accepted")
	}
	var failure manifest
	b, err = os.ReadFile(filepath.Join(rejected, "public", "manifest.json"))
	if err != nil {
		t.Fatal(err)
	}
	if err = json.Unmarshal(b, &failure); err != nil {
		t.Fatal(err)
	}
	if failure.Status != "failed" || !strings.Contains(failure.Failure, "duplicate_machine_identity") || len(failure.CleanupErrors) > 0 {
		t.Fatal("duplicate-host failure was not retained correctly")
	}
	if !failure.PrivateRemoved || failure.KeepPrivate {
		t.Fatal("failed doctor did not default to controller private cleanup")
	}
	if _, err = os.Stat(filepath.Join(rejected, "private")); !os.IsNotExist(err) {
		t.Fatal("failed doctor left its controller private directory")
	}
	for _, n := range failure.Nodes {
		if n.Cleanup == nil || !n.Cleanup.Removed {
			t.Fatal("failed doctor left a node profile")
		}
	}
	checks = append(checks, "failed doctor removes every node profile and controller private directory while retaining its failure receipt")
	rejected = filepath.Join(artifacts, "retained-rejected")
	cmd = exec.CommandContext(ctx, tool, "doctor", "--lab", duplicate, "--out", rejected, "--keep-private")
	cmd.Dir = root
	if _, err = cmd.CombinedOutput(); err == nil {
		t.Fatal("retained duplicate-machine run unexpectedly passed")
	}
	var paths map[string]struct {
		Root string `json:"root"`
	}
	b, err = os.ReadFile(filepath.Join(rejected, "private", "nodes.json"))
	if err != nil {
		t.Fatal(err)
	}
	if err = json.Unmarshal(b, &paths); err != nil {
		t.Fatal(err)
	}
	for _, v := range paths {
		if _, err = os.Stat(v.Root); err != nil {
			t.Fatal("keep-private did not preserve failed node profiles")
		}
		if err = os.RemoveAll(v.Root); err != nil {
			t.Fatal(err)
		}
	}
	if err = os.RemoveAll(filepath.Join(rejected, "private")); err != nil {
		t.Fatal(err)
	}
	checks = append(checks, "keep-private explicitly retains failed node profiles and controller inventory")
	four := filepath.Join(artifacts, "four.toml")
	text := "version=1\n"
	for i := 0; i < 4; i++ {
		text += fmt.Sprintf("[nodes.n%d]\ntransport=\"ssh\"\nhost=\"not-a-test-host.invalid\"\n", i)
	}
	if err = os.WriteFile(four, []byte(text), 0600); err != nil {
		t.Fatal(err)
	}
	b = call(root, tool, "plan", "--lab", four)
	var plan struct {
		Links    int         `json:"directed_links"`
		Pairs    [][2]string `json:"pairing_edges"`
		Executes bool        `json:"executes_hosts"`
	}
	if err = json.Unmarshal(b, &plan); err != nil {
		t.Fatal(err)
	}
	if plan.Links != 12 || len(plan.Pairs) != 6 || plan.Executes {
		t.Fatal("four-node plan did not expand without contacting hosts")
	}
	checks = append(checks, "four-node CLI plan expands six pairs and twelve directions without SSH execution")
}
func processExists(pid int) bool {
	if runtime.GOOS == "windows" {
		b, err := exec.Command("tasklist", "/FI", fmt.Sprintf("PID eq %d", pid), "/FO", "CSV", "/NH").Output()
		if err != nil {
			return true
		}
		rows := csv.NewReader(strings.NewReader(string(b)))
		for {
			row, err := rows.Read()
			if err == io.EOF {
				break
			}
			if err != nil {
				return true
			}
			if len(row) > 1 && row[1] == strconv.Itoa(pid) {
				return true
			}
		}
		return false
	}
	p, err := os.FindProcess(pid)
	if err != nil {
		return false
	}
	return p.Signal(syscall.Signal(0)) == nil
}
