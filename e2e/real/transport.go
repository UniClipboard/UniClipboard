package main

import (
	"bytes"
	"context"
	"encoding/base64"
	"encoding/binary"
	"encoding/json"
	"fmt"
	"io"
	"os"
	"os/exec"
	"path/filepath"
	"runtime"
	"strings"
	"sync"
	"time"
	"unicode/utf16"
)

type controlMessage struct {
	reply response
	err   error
}

type platform struct{ OS, Arch, Bootstrap string }
type node struct {
	name     string
	config   nodeConfig
	platform platform
	hello    hello
	hashes   map[string]string
	cmd      *exec.Cmd
	input    io.WriteCloser
	replies  chan controlMessage
	done     chan error
	mutex    sync.Mutex
	sequence uint64
	broken   bool
	private  string
	cleanup  *logsResult
	running  bool
	deviceID string
}

func ps(script string) string {
	script = "[Console]::OutputEncoding=[Text.UTF8Encoding]::new($false); $OutputEncoding=[Console]::OutputEncoding; " + script
	codes := utf16.Encode([]rune(script))
	b := make([]byte, len(codes)*2)
	for i, v := range codes {
		binary.LittleEndian.PutUint16(b[i*2:], v)
	}
	return "powershell -NoProfile -NonInteractive -EncodedCommand " + base64.StdEncoding.EncodeToString(b)
}
func quotePS(s string) string { return "'" + strings.ReplaceAll(s, "'", "''") + "'" }
func quoteSH(s string) string { return "'" + strings.ReplaceAll(s, "'", "'\"'\"'") + "'" }
func sshArgs(host, cmd string) []string {
	return []string{"-T", "-o", "BatchMode=yes", "-o", "ConnectTimeout=10", "-o", "ServerAliveInterval=5", "-o", "ServerAliveCountMax=3", "--", host, cmd}
}
func remote(ctx context.Context, host, cmd string) ([]byte, error) {
	process := exec.CommandContext(ctx, "ssh", sshArgs(host, cmd)...)
	var output bytes.Buffer
	process.Stdout = &output
	if err := process.Run(); err != nil {
		return nil, fmt.Errorf("ssh_command_failed")
	}
	return output.Bytes(), nil
}
func detect(ctx context.Context, c nodeConfig) (platform, error) {
	if c.Transport == "local" {
		return platform{OS: runtime.GOOS, Arch: runtime.GOARCH}, nil
	}
	probe, cancel := context.WithTimeout(ctx, 20*time.Second)
	defer cancel()
	b, err := remote(probe, c.Host, "uname -s -m")
	if err == nil {
		parts := strings.Fields(string(b))
		if len(parts) != 2 {
			return platform{}, fmt.Errorf("unexpected_uname_result")
		}
		osName := map[string]string{"Darwin": "darwin", "Linux": "linux"}[parts[0]]
		arch := map[string]string{"arm64": "arm64", "aarch64": "arm64", "x86_64": "amd64", "amd64": "amd64"}[parts[1]]
		if osName == "" || arch == "" {
			return platform{}, fmt.Errorf("unsupported_platform")
		}
		return platform{OS: osName, Arch: arch}, nil
	}
	b, err = remote(probe, c.Host, ps("[Console]::WriteLine([System.Runtime.InteropServices.RuntimeInformation]::OSArchitecture.ToString())"))
	if err != nil {
		return platform{}, fmt.Errorf("ssh_platform_probe_failed")
	}
	arch := map[string]string{"X64": "amd64", "Arm64": "arm64"}[strings.TrimSpace(string(b))]
	if arch == "" {
		return platform{}, fmt.Errorf("unsupported_windows_architecture")
	}
	return platform{OS: "windows", Arch: arch}, nil
}
func (n *node) bootstrap(ctx context.Context, worker, runID string) error {
	var cmd *exec.Cmd
	if n.config.Transport == "local" {
		cmd = exec.Command(worker, "worker", "--run-id", runID, "--node", n.name)
	} else {
		var b []byte
		var err error
		if n.platform.OS == "windows" {
			script := "$p=Join-Path ([IO.Path]::GetTempPath()) ('unic-e2e-'+[Guid]::NewGuid().ToString('N')); [IO.Directory]::CreateDirectory($p) | Out-Null; [IO.File]::WriteAllText((Join-Path $p 'owner')," + quotePS(runID) + "); [Console]::WriteLine($p)"
			b, err = remote(ctx, n.config.Host, ps(script))
		} else {
			b, err = remote(ctx, n.config.Host, "p=$(mktemp -d \"${TMPDIR:-/tmp}/unic-e2e-XXXXXXXX\") && printf %s "+quoteSH(runID)+" > \"$p/owner\" && printf '%s\\n' \"$p\"")
		}
		if err != nil {
			return fmt.Errorf("bootstrap_directory_failed")
		}
		n.platform.Bootstrap = strings.TrimSpace(string(b))
		if n.platform.Bootstrap == "" || strings.ContainsAny(n.platform.Bootstrap, "\r\n") {
			return fmt.Errorf("invalid_bootstrap_directory")
		}
		filename := "unic-e2e-worker"
		if n.platform.OS == "windows" {
			filename += ".exe"
		}
		path := strings.ReplaceAll(n.platform.Bootstrap, "\\", "/") + "/" + filename
		if err = n.copy(ctx, worker, path); err != nil {
			return err
		}
		var invoke string
		if n.platform.OS == "windows" {
			invoke = "\"" + path + "\" worker --run-id " + runID + " --node " + n.name
		} else {
			if _, err = remote(ctx, n.config.Host, "chmod 700 "+quoteSH(path)); err != nil {
				return fmt.Errorf("worker_chmod_failed")
			}
			invoke = quoteSH(path) + " worker --run-id " + runID + " --node " + n.name
		}
		cmd = exec.Command("ssh", sshArgs(n.config.Host, invoke)...)
	}
	var err error
	n.input, err = cmd.StdinPipe()
	if err != nil {
		return err
	}
	stdout, err := cmd.StdoutPipe()
	if err != nil {
		return err
	}
	stderr, err := os.OpenFile(n.private, os.O_CREATE|os.O_APPEND|os.O_WRONLY, 0600)
	if err != nil {
		return err
	}
	cmd.Stderr = stderr
	if err = cmd.Start(); err != nil {
		_ = stderr.Close()
		return fmt.Errorf("worker_start_failed")
	}
	n.cmd = cmd
	n.done = make(chan error, 1)
	readFinished := make(chan struct{})
	n.replies = make(chan controlMessage, 2)
	go func() {
		<-readFinished
		err := cmd.Wait()
		_ = stderr.Close()
		n.done <- err
	}()
	go func() {
		defer close(readFinished)
		dec := json.NewDecoder(stdout)
		for {
			var r response
			if err := dec.Decode(&r); err != nil {
				n.replies <- controlMessage{err: err}
				return
			}
			n.replies <- controlMessage{reply: r}
		}
	}()
	return n.call(ctx, "hello", nil, &n.hello)
}
func (n *node) copy(ctx context.Context, source, dest string) error {
	if n.config.Transport == "local" {
		b, err := os.ReadFile(source)
		if err != nil {
			return err
		}
		if err = os.MkdirAll(filepath.Dir(dest), 0700); err != nil {
			return err
		}
		return os.WriteFile(dest, b, 0700)
	}
	_, err := command(ctx, "", nil, "scp", "-q", "-o", "BatchMode=yes", "-o", "ConnectTimeout=10", "--", source, n.config.Host+":"+strings.ReplaceAll(dest, "\\", "/"))
	if err != nil {
		return fmt.Errorf("artifact_copy_failed")
	}
	return nil
}
func (n *node) call(ctx context.Context, method string, value, out any) error {
	n.mutex.Lock()
	defer n.mutex.Unlock()
	if ctx.Err() != nil {
		return fmt.Errorf("%s: call_cancelled_before_send", n.name)
	}
	limit := 5 * time.Minute
	if method == "wait" {
		if v, ok := value.(waitRequest); ok {
			limit = time.Duration(v.TimeoutSeconds+30) * time.Second
		}
	}
	ctx, cancel := context.WithTimeout(ctx, limit)
	defer cancel()
	if n.broken || n.input == nil {
		return fmt.Errorf("%s: worker_disconnected", n.name)
	}
	n.sequence++
	data, err := json.Marshal(value)
	if err != nil {
		return err
	}
	req := request{ID: n.sequence, Method: method, Value: data}
	if err = json.NewEncoder(n.input).Encode(req); err != nil {
		n.broken = true
		return fmt.Errorf("%s: control_write_failed", n.name)
	}
	select {
	case message := <-n.replies:
		if message.err != nil {
			n.broken = true
			return fmt.Errorf("%s: worker_connection_closed", n.name)
		}
		r := message.reply
		if r.ID != req.ID {
			n.broken = true
			return fmt.Errorf("%s: control_sequence_mismatch", n.name)
		}
		if r.Error != "" {
			return fmt.Errorf("%s: %s: %s", n.name, method, r.Error)
		}
		if out != nil {
			if err = json.Unmarshal(r.Value, out); err != nil {
				return fmt.Errorf("%s: invalid_worker_response", n.name)
			}
		}
		return nil
	case <-ctx.Done():
		n.broken = true
		return fmt.Errorf("%s: control_call_cancelled", n.name)
	}
}
func (n *node) action(ctx context.Context, method string, value any) error {
	return n.call(ctx, method, value, nil)
}
func (n *node) cli(ctx context.Context, args ...string) (cliResult, error) {
	var result cliResult
	err := n.call(ctx, "cli", cliRequest{Args: args}, &result)
	if err == nil && result.Code != 0 {
		err = fmt.Errorf("%s: cli_%s_failed", n.name, args[0])
	}
	return result, err
}
func (n *node) stop(ctx context.Context) error { return n.action(ctx, "stop", nil) }
func (n *node) close(runID string, keep bool) error {
	if n.cmd == nil {
		if keep {
			return nil
		}
		return n.removeBootstrap(runID)
	}
	ctx, cancel := context.WithTimeout(context.Background(), 35*time.Second)
	defer cancel()
	var result logsResult
	err := n.call(ctx, "close", keep, &result)
	_ = n.input.Close()
	select {
	case waitErr := <-n.done:
		if waitErr != nil && err == nil {
			err = fmt.Errorf("%s: worker_exit_failed", n.name)
		}
	case <-ctx.Done():
		_ = n.cmd.Process.Kill()
		if err == nil {
			err = fmt.Errorf("%s: worker_exit_timeout", n.name)
		}
	}
	if err == nil {
		n.cleanup = &result
		if result.ContainsSecret {
			return fmt.Errorf("%s: secret_in_daemon_log", n.name)
		}
		if !keep && !result.Removed {
			return fmt.Errorf("%s: cleanup_incomplete", n.name)
		}
	}
	if err != nil {
		return err
	}
	if !keep {
		return n.removeBootstrap(runID)
	}
	return nil
}
func (n *node) removeBootstrap(runID string) error {
	if n.platform.Bootstrap == "" {
		return nil
	}
	ctx, cancel := context.WithTimeout(context.Background(), 20*time.Second)
	defer cancel()
	dir := n.platform.Bootstrap
	var script string
	if n.platform.OS == "windows" {
		script = ps("$p=" + quotePS(dir) + "; if ([IO.File]::ReadAllText((Join-Path $p 'owner')) -ne " + quotePS(runID) + ") { exit 1 }; Remove-Item -LiteralPath $p -Recurse -Force")
	} else {
		script = "test \"$(cat " + quoteSH(dir+"/owner") + ")\" = " + quoteSH(runID) + " && rm -rf -- " + quoteSH(dir)
	}
	if _, err := remote(ctx, n.config.Host, script); err != nil {
		return fmt.Errorf("%s: bootstrap_cleanup_failed", n.name)
	}
	return nil
}
