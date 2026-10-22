// A process/API reproduction driver. Run with go run ./e2e/startup.
package main

import (
	"context"
	"encoding/json"
	"fmt"
	"net"
	"net/http"
	"os"
	"os/exec"
	"path/filepath"
	"time"

	"github.com/UniClipboard/UniClipboard/packages/desktop-host-go/daemonclient"
	"github.com/UniClipboard/UniClipboard/packages/desktop-host-go/daemonlife"
	"github.com/UniClipboard/UniClipboard/packages/desktop-host-go/daemonproc"
)

func must(err error) {
	if err != nil {
		panic(err)
	}
}
func write(path string, value any) {
	data, err := json.Marshal(value)
	must(err)
	must(os.MkdirAll(filepath.Dir(path), 0700))
	must(os.WriteFile(path, data, 0600))
}
func root() string { r, err := daemonproc.AppDataRoot(); must(err); return r }
func fixture() {
	// Bound fixture lifetime even if the driver fails before explicit cleanup.
	time.AfterFunc(15*time.Second, func() { os.Exit(2) })
	mode := os.Getenv("STARTUP_FIXTURE")
	listener, err := net.Listen("tcp4", "127.0.0.1:0")
	must(err)
	id := os.Getenv("UC_DAEMON_STARTUP_ID")
	if id == "" {
		id = "fixture-current-run"
	}
	conn := daemonproc.ConnFile{Format: 1, Host: "127.0.0.1", Port: uint16(listener.Addr().(*net.TCPAddr).Port), Token: "isolated-e2e-token", PID: uint32(os.Getpid()), StartedAtMs: 1}
	// Keep identical timestamps to disprove timestamp-only ownership.
	data, _ := json.Marshal(conn)
	var record map[string]any
	must(json.Unmarshal(data, &record))
	record["instanceId"] = id
	if mode == "missing-run" {
		delete(record, "instanceId")
	}
	if mode == "dead-pid" {
		record["pid"] = uint32(2147483647)
	}
	if mode == "delayed-child" {
		time.Sleep(350 * time.Millisecond)
	}
	write(filepath.Join(root(), "daemon-startup.conn"), record)
	mux := http.NewServeMux()
	mux.HandleFunc("/health", func(w http.ResponseWriter, r *http.Request) { w.WriteHeader(503) })
	mux.HandleFunc("/startup", func(w http.ResponseWriter, r *http.Request) {
		if r.Header.Get("Authorization") != "Bearer "+conn.Token {
			w.WriteHeader(401)
			return
		}
		responseID := id
		if mode == "wrong-run" {
			responseID = "predecessor-run"
		}
		if mode == "changed-record" {
			record["instanceId"] = "replacement-run"
			write(filepath.Join(root(), "daemon-startup.conn"), record)
		}
		failure := map[string]any{"instanceId": responseID, "pid": os.Getpid(), "service_failed": true, "progress": map[string]any{"state": "failed", "failure": map[string]any{"reason": "startup_failed", "retryable": true}}}
		must(json.NewEncoder(w).Encode(failure))
	})
	mux.HandleFunc("/startup/exit", func(w http.ResponseWriter, r *http.Request) {
		if mode == "still-starting" {
			w.WriteHeader(409)
			return
		}
		if r.Method != "POST" || r.Header.Get("Authorization") != "Bearer "+conn.Token || r.Header.Get("X-UniClipboard-Startup-Instance") != id {
			w.WriteHeader(403)
			return
		}
		w.WriteHeader(202)
		go func() { time.Sleep(50 * time.Millisecond); os.Exit(0) }()
	})
	must(http.Serve(listener, mux))
}

func waitRecord() {
	deadline := time.Now().Add(5 * time.Second)
	for {
		if _, err := os.Stat(filepath.Join(root(), "daemon-startup.conn")); err == nil {
			return
		}
		if time.Now().After(deadline) {
			panic("fixture startup timeout")
		}
		time.Sleep(10 * time.Millisecond)
	}
}

func main() {
	if os.Getenv("STARTUP_FIXTURE") != "" {
		fixture()
		return
	}
	sandbox, err := os.MkdirTemp("", "uc-startup-e2e-")
	must(err)
	// Keep artifacts in this isolated sandbox for inspection.
	must(os.Setenv("HOME", sandbox))
	must(os.Setenv("XDG_DATA_HOME", filepath.Join(sandbox, "data")))
	must(os.Setenv("UC_PROFILE", "gui-go-startup-e2e"))
	must(os.Setenv("UC_PORTABLE", "0"))
	var results []map[string]any
	exe, err := os.Executable()
	must(err)
	for _, mode := range []string{"wrong-run", "dead-pid", "changed-record", "valid", "missing-run"} {
		must(os.RemoveAll(root()))
		child := exec.Command(exe)
		child.Env = append(os.Environ(), "STARTUP_FIXTURE="+mode)
		must(child.Start())
		path := filepath.Join(root(), "daemon-startup.conn")
		deadline := time.Now().Add(5 * time.Second)
		for {
			if _, err := os.Stat(path); err == nil {
				break
			}
			if time.Now().After(deadline) {
				panic("fixture startup timeout")
			}
			time.Sleep(10 * time.Millisecond)
		}
		ctx, cancel := context.WithTimeout(context.Background(), time.Second)
		body, err := daemonclient.ReadStartupStatus(ctx)
		cancel()
		passed := err == nil && ((mode == "valid" && len(body) > 0) || (mode != "valid" && len(body) == 0))
		// Reap only this fixture child, never a discovered process.
		must(child.Process.Kill())
		_ = child.Wait()
		results = append(results, map[string]any{"case": mode, "passed": passed, "snapshotAccepted": len(body) > 0, "error": fmt.Sprint(err)})
	}
	// Confirm the live startup service prevents a competing spawn.
	must(os.RemoveAll(root()))
	child := exec.Command(exe)
	child.Env = append(os.Environ(), "STARTUP_FIXTURE=valid")
	must(child.Start())
	deadline := time.Now().Add(5 * time.Second)
	for {
		if _, err := os.Stat(filepath.Join(root(), "daemon-startup.conn")); err == nil {
			break
		}
		if time.Now().After(deadline) {
			panic("fixture timeout")
		}
		time.Sleep(10 * time.Millisecond)
	}
	_, err = daemonlife.ProbeForReuse(300 * time.Millisecond)
	results = append(results, map[string]any{"case": "live-startup-owner", "passed": err != nil, "error": fmt.Sprint(err)})
	must(child.Process.Kill())
	_ = child.Wait()

	// Exercise the public spawn handle, exact-run wait and authenticated retry.
	// The sibling uniclipd is a protocol fixture, never an installed daemon.
	for _, mode := range []string{"valid", "still-starting", "delayed-child"} {
		must(os.RemoveAll(root()))
		var predecessor *exec.Cmd
		if mode == "delayed-child" {
			predecessor = exec.Command(exe)
			predecessor.Env = append(os.Environ(), "STARTUP_FIXTURE=valid")
			must(predecessor.Start())
			waitRecord()
		}
		must(os.Setenv("STARTUP_FIXTURE", mode))
		attempt, err := daemonproc.SpawnDetachedDaemonForStartup("e2e")
		must(err)
		must(os.Unsetenv("STARTUP_FIXTURE"))
		waitStart := time.Now()
		err = daemonlife.WaitHealthyForAttempt(2*time.Second, "", attempt)
		elapsed := time.Since(waitStart)
		passed := daemonlife.RetryableStartupError(err)
		if mode == "delayed-child" {
			passed = passed && elapsed >= 300*time.Millisecond
		}
		results = append(results, map[string]any{"case": "owned-" + mode, "passed": passed, "elapsedMs": elapsed.Milliseconds(), "error": fmt.Sprint(err)})
		// Try a replaced discovery record first; the owned child must survive.
		conn, readErr := daemonproc.ReadStartupConnFile()
		must(readErr)
		original := *conn
		conn.InstanceID = "different-run"
		write(filepath.Join(root(), "daemon-startup.conn"), conn)
		ctx, cancel := context.WithTimeout(context.Background(), time.Second)
		releaseErr := daemonproc.ReleaseFailedStartup(ctx, attempt)
		cancel()
		results = append(results, map[string]any{"case": "refuse-other-run-" + mode, "passed": releaseErr != nil && !attempt.Exited()})
		write(filepath.Join(root(), "daemon-startup.conn"), original)
		ctx, cancel = context.WithTimeout(context.Background(), 2*time.Second)
		releaseErr = daemonproc.ReleaseFailedStartup(ctx, attempt)
		cancel()
		if mode == "still-starting" {
			results = append(results, map[string]any{"case": "refuse-starting", "passed": releaseErr != nil && !attempt.Exited()})
			// Fixture-only endpoint mode has no live Engine; use the known child PID for cleanup.
			process, e := os.FindProcess(int(attempt.PID()))
			must(e)
			must(process.Kill())
			ctx, cancel = context.WithTimeout(context.Background(), time.Second)
			must(attempt.Wait(ctx))
			cancel()
		} else {
			results = append(results, map[string]any{"case": "release-owned-" + mode, "passed": releaseErr == nil && attempt.Exited()})
		}
		if predecessor != nil {
			must(predecessor.Process.Kill())
			_ = predecessor.Wait()
		}
	}

	if service := os.Getenv("UC_STARTUP_E2E_REAL_SERVICE"); service != "" {
		binary, err := os.ReadFile(service)
		must(err)
		must(os.WriteFile(filepath.Join(filepath.Dir(exe), "uniclipd"), binary, 0700))
		for _, mode := range []string{"interrupted", "still-starting"} {
			must(os.RemoveAll(root()))
			must(os.Setenv("UC_STARTUP_E2E_ROOT", root()))
			must(os.Setenv("STARTUP_FIXTURE", mode))
			attempt, err := daemonproc.SpawnDetachedDaemonForStartup("e2e")
			must(err)
			must(os.Unsetenv("STARTUP_FIXTURE"))
			waitRecord()
			conn, err := daemonproc.ReadStartupConnFile()
			must(err)
			ctx, cancel := context.WithTimeout(context.Background(), time.Second)
			body, err := daemonclient.ReadStartupStatusAt(ctx, *conn)
			cancel()
			results = append(results, map[string]any{"case": "rust-service-identity-" + mode, "passed": err == nil && len(body) > 0 && conn.InstanceID == attempt.InstanceID() && conn.PID == attempt.PID()})
			request, err := http.NewRequest(http.MethodPost, conn.BaseURL()+"/startup/exit", nil)
			must(err)
			request.Header.Set("Authorization", "Bearer "+conn.Token)
			request.Header.Set("X-UniClipboard-Startup-Instance", "predecessor-run")
			client := daemonclient.NewLocalHTTPClient(time.Second)
			response, err := client.Do(request)
			must(err)
			response.Body.Close()
			client.CloseIdleConnections()
			results = append(results, map[string]any{"case": "rust-service-refuse-other-run-" + mode, "passed": response.StatusCode == 401 && !attempt.Exited()})
			err = daemonlife.WaitHealthyForAttempt(300*time.Millisecond, "", attempt)
			results = append(results, map[string]any{"case": "rust-service-recovery-" + mode, "passed": daemonlife.RetryableStartupError(err), "error": fmt.Sprint(err)})
			ctx, cancel = context.WithTimeout(context.Background(), 2*time.Second)
			releaseErr := daemonproc.ReleaseFailedStartup(ctx, attempt)
			cancel()
			if mode == "interrupted" {
				_, statErr := os.Stat(filepath.Join(root(), "daemon-startup.conn"))
				results = append(results, map[string]any{"case": "rust-service-exit-interrupted", "passed": releaseErr == nil && attempt.Exited() && os.IsNotExist(statErr)})
			} else {
				results = append(results, map[string]any{"case": "rust-service-refuse-starting", "passed": releaseErr != nil && !attempt.Exited()})
				process, err := os.FindProcess(int(attempt.PID()))
				must(err)
				must(process.Kill())
				ctx, cancel = context.WithTimeout(context.Background(), time.Second)
				must(attempt.Wait(ctx))
				cancel()
			}
		}
	}
	report := map[string]any{"sandbox": sandbox, "cases": results}
	write(filepath.Join(sandbox, "result.json"), report)
	must(json.NewEncoder(os.Stdout).Encode(report))
	for _, r := range results {
		if r["passed"] != true {
			os.Exit(1)
		}
	}
}
