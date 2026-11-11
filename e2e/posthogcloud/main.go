// Command posthogcloud drives two production services with synthetic data.
// It requires explicitly selected build-time Cloud metadata and never reads a
// user's live profile. Query read-back is recorded separately by the operator.
package main

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"flag"
	"fmt"
	"io"
	"os"
	"os/exec"
	"os/signal"
	"path/filepath"
	"regexp"
	"strings"
	"syscall"
	"time"

	api "github.com/uniclipboard/unic/internal/localapi"
)

type node struct {
	cmd    *exec.Cmd
	done   chan error
	client *api.Client
	home   string
}

func (n *node) close() {
	if n.cmd == nil {
		return
	}
	_ = n.cmd.Process.Signal(syscall.SIGTERM)
	select {
	case err := <-n.done:
		_ = err // Shutdown may terminate the child with a signal.
	case <-time.After(5 * time.Second):
		_ = n.cmd.Process.Kill()
		err := <-n.done
		_ = err // The forced child termination is expected.
	}
	n.cmd = nil
}
func main() {
	if err := run(); err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
}
func run() error {
	artifact := flag.String("artifact", ".cache/0019/cloud", "sanitized acceptance directory")
	flag.Parse()
	host, token, project := os.Getenv("UNIC_POSTHOG_HOST"), os.Getenv("UNIC_POSTHOG_PROJECT_TOKEN"), os.Getenv("UNIC_POSTHOG_PROJECT_ID")
	if host != "https://us.i.posthog.com" && host != "https://eu.i.posthog.com" {
		return errors.New("select the Cloud ingestion host explicitly")
	}
	if !regexp.MustCompile(`^phc_[A-Za-z0-9_-]+$`).MatchString(token) || !regexp.MustCompile(`^[1-9][0-9]*$`).MatchString(project) {
		return errors.New("supply an ingestion-only project token and the selected project ID")
	}
	root, err := os.Getwd()
	if err != nil {
		return err
	}
	dir, err := os.MkdirTemp("", "unic-posthog-cloud-")
	if err != nil {
		return err
	}
	defer func() { _ = os.RemoveAll(dir) }()
	dir, err = filepath.EvalSymlinks(dir)
	if err != nil {
		return err
	}
	if err = os.MkdirAll(*artifact, 0700); err != nil {
		return err
	}
	ctx, cancel := signal.NotifyContext(context.Background(), os.Interrupt, syscall.SIGTERM)
	defer cancel()
	ctx, deadline := context.WithTimeout(ctx, 3*time.Minute)
	defer deadline()
	write := func(name string, value any) error {
		data, err := json.MarshalIndent(value, "", "  ")
		if err != nil {
			return err
		}
		return os.WriteFile(filepath.Join(*artifact, name), data, 0600)
	}
	source, err := sourceHashes(root)
	if err != nil {
		return err
	}
	flags := "-X github.com/uniclipboard/unic/internal/observability.PostHogHost=" + host + " -X github.com/uniclipboard/unic/internal/observability.PostHogToken=" + token + " -X github.com/uniclipboard/unic/internal/observability.ProductVersion=dev"
	binaries := map[string]string{}
	for _, name := range []string{"unic", "unicd"} {
		binary := filepath.Join(dir, name)
		cmd := exec.CommandContext(ctx, "go", "build", "-race", "-trimpath", "-buildvcs=false", "-ldflags", flags, "-o", binary, "./app/service/cmd/"+name)
		// Build output never includes invocation arguments or deployment values.
		cmd.Stdout, cmd.Stderr = io.Discard, io.Discard
		if err = cmd.Run(); err != nil {
			return fmt.Errorf("production %s build failed", name)
		}
		b, err := os.ReadFile(binary)
		if err != nil {
			return err
		}
		sum := sha256.Sum256(b)
		binaries[name] = hex.EncodeToString(sum[:])
	}
	started := time.Now().UTC()
	receipt := map[string]any{"project_id": project, "ingestion_host": host, "started_at": started, "expected_operations": 10, "expected_startups": 2, "expected_person_profiles": 0, "source_files_sha256": source, "binaries_sha256": binaries, "build_flags": []string{"-race", "-trimpath", "-buildvcs=false", "-ldflags [ingestion metadata redacted]"}, "test_build_tags": []string{}, "read_back": "PENDING"}
	if err = write("upload-receipt.json", receipt); err != nil {
		return err
	}
	nodes := []*node{}
	defer func() {
		for _, n := range nodes {
			n.close()
		}
	}()
	for i, count := range []int{3, 7} {
		home := filepath.Join(dir, fmt.Sprintf("profile-%d", i))
		if err = os.MkdirAll(home, 0700); err != nil {
			return err
		}
		if err = os.WriteFile(filepath.Join(home, "config.toml"), []byte("[sync]\nenabled=false\n"), 0600); err != nil {
			return err
		}
		sock := filepath.Join(dir, fmt.Sprintf("%d.sock", i))
		log, err := os.Create(filepath.Join(dir, fmt.Sprintf("%d.log", i)))
		if err != nil {
			return err
		}
		n := &node{cmd: exec.Command(filepath.Join(dir, "unicd")), done: make(chan error, 1), client: api.NewClient(sock), home: home}
		n.cmd.Env = append(os.Environ(), "UNIC_HOME="+home, "UNIC_SOCK="+sock, "GORACE=atexit_sleep_ms=0")
		n.cmd.Stdout, n.cmd.Stderr = log, log
		if err = n.cmd.Start(); err != nil {
			_ = log.Close()
			return errors.New("production daemon startup failed")
		}
		nodes = append(nodes, n)
		go func() { err := n.cmd.Wait(); _ = log.Close(); n.done <- err }()
		ready := false
		for tries := 0; tries < 100; tries++ {
			if _, err := n.client.Telemetry(ctx); err == nil {
				ready = true
				break
			}
			select {
			case <-ctx.Done():
				return ctx.Err()
			case <-time.After(100 * time.Millisecond):
			}
		}
		if !ready {
			return errors.New("production daemon did not become ready")
		}
		for j := 0; j < count; j++ {
			cmd := exec.CommandContext(ctx, filepath.Join(dir, "unic"), "send", "--to", "all", fmt.Sprintf("Synthetic Cloud acceptance %d %d", i, j), "--request-id", fmt.Sprintf("acceptance-%d-%d", i, j))
			cmd.Env = n.cmd.Env
			cmd.Stdout, cmd.Stderr = io.Discard, io.Discard
			if err = cmd.Run(); err != nil {
				return errors.New("synthetic submission failed")
			}
		}
		// An invalid save retains the valid configuration and produces a safe
		// config.rejected warning at the existing remote warn threshold.
		time.Sleep(200 * time.Millisecond)
		if err = os.WriteFile(filepath.Join(home, "config.toml"), []byte("[telemetry]\nenabled=\"invalid\"\n"), 0600); err != nil {
			return err
		}
	}
	fmt.Println("Two isolated production services submitted 3 + 7 synthetic operations; waiting for the production metrics interval.")
	select {
	case <-ctx.Done():
		return ctx.Err()
	case <-time.After(65 * time.Second):
	}
	states := []any{}
	allActive := true
	for _, n := range nodes {
		state, err := n.client.Telemetry(ctx)
		if err != nil {
			return errors.New("diagnostics query failed")
		}
		states = append(states, state)
		if state.Logs.Failures > 0 || state.Metrics.Failures > 0 || state.Analytics.Failures > 0 || state.Logs.State != "active" || state.Metrics.State != "active" || state.Analytics.State != "active" {
			allActive = false
		}
		n.close()
	}
	after, err := sourceHashes(root)
	if err != nil {
		return err
	}
	sourceStable := len(source) == len(after)
	for k, v := range source {
		if after[k] != v {
			sourceStable = false
		}
	}
	receipt["finished_at"] = time.Now().UTC()
	receipt["local_export_states"] = states
	receipt["all_exporters_active"] = allActive
	receipt["source_unchanged"] = sourceStable
	if err = write("upload-receipt.json", receipt); err != nil {
		return err
	}
	if !sourceStable {
		return errors.New("source changed during Cloud acceptance")
	}
	if !allActive {
		return errors.New("one or more Cloud exporters degraded; inspect the sanitized upload receipt")
	}
	fmt.Println("Production exporters completed without local export failures. Cloud query read-back is still required.")
	return nil
}
func sourceHashes(root string) (map[string]string, error) {
	cmd := exec.Command("git", "-C", root, "ls-files", "--cached", "--others", "--exclude-standard", "--", "go.mod", "go.sum", "engine", "internal", "app/service", "tools/package", "e2e/posthogcloud")
	data, err := cmd.Output()
	if err != nil {
		return nil, err
	}
	out := map[string]string{}
	for _, name := range strings.Split(strings.TrimSpace(string(data)), "\n") {
		if name == "" {
			continue
		}
		b, err := os.ReadFile(filepath.Join(root, name))
		if err != nil {
			return nil, err
		}
		sum := sha256.Sum256(b)
		out[name] = hex.EncodeToString(sum[:])
	}
	return out, nil
}
