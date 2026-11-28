package fixture

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"net"
	"net/http"
	"os"
	"os/exec"
	"path/filepath"
	"runtime"
	"strings"
	"time"
)

// ProfileOptions configures an isolated benchmark UNIC_HOME profile.
type ProfileOptions struct {
	HomeDir           string // Optional explicit path; if empty, creates a short temp dir.
	UnicBin           string // Path to unic CLI binary.
	UnicdBin          string // Path to unicd daemon binary.
	DesktopMode       string // "normal" (default) or "lightweight".
	StartInBackground bool
	Language          string // Default "en".
}

// Profile manages an isolated benchmark UNIC_HOME and any unicd process it starts.
type Profile struct {
	HomeDir      string
	DiscoveryDir string
	SocketPath   string
	ConfigPath   string
	UnicBin      string
	UnicdBin     string
	ownsDir      bool

	daemonCmd  *exec.Cmd
	daemonPID  int
	daemonDone chan error
}

// SeedResult summarizes the outcome of seeding a profile with a corpus manifest.
type SeedResult struct {
	SeededTexts   int   `json:"seeded_texts"`
	SeededImages  int   `json:"seeded_images"`
	SeededFiles   int   `json:"seeded_files"`
	HistoryCount  int   `json:"history_count"`
	RestartedPID  int   `json:"restarted_pid,omitempty"`
	DaemonStopped bool  `json:"daemon_stopped"`
	DurationMs    int64 `json:"duration_ms"`
}

// InitProfile creates an isolated UNIC_HOME, writes a benchmark config.toml
// with telemetry, sync, login, and hotkeys disabled, runs `unic migrate
// --initialize-empty --json`, and writes `desktop-setup.json` (`complete: true`).
func InitProfile(ctx context.Context, opts ProfileOptions) (*Profile, error) {
	if opts.UnicBin == "" {
		return nil, errors.New("UnicBin is required")
	}
	mode := opts.DesktopMode
	if mode == "" {
		mode = "normal"
	}
	if mode != "normal" && mode != "lightweight" {
		return nil, fmt.Errorf("invalid DesktopMode %q (want normal or lightweight)", mode)
	}
	lang := opts.Language
	if lang == "" {
		lang = "en"
	}

	homeDir := opts.HomeDir
	ownsDir := false
	if homeDir == "" {
		base := ""
		// macOS limits AF_UNIX socket paths to 104 bytes; prefer /tmp when available.
		if runtime.GOOS == "darwin" {
			if st, err := os.Stat("/tmp"); err == nil && st.IsDir() {
				base = "/tmp"
			}
		}
		dir, err := os.MkdirTemp(base, "ub-")
		if err != nil {
			return nil, fmt.Errorf("create temp UNIC_HOME: %w", err)
		}
		homeDir = dir
		ownsDir = true
	} else {
		if err := os.MkdirAll(homeDir, 0o700); err != nil {
			return nil, fmt.Errorf("create UNIC_HOME: %w", err)
		}
	}

	absHome, err := filepath.Abs(homeDir)
	if err != nil {
		if ownsDir {
			_ = os.RemoveAll(homeDir)
		}
		return nil, fmt.Errorf("resolve UNIC_HOME: %w", err)
	}
	if resolved, err := filepath.EvalSymlinks(absHome); err == nil {
		absHome = resolved
	}
	sockPath := filepath.Join(absHome, "unic.sock")
	if runtime.GOOS == "darwin" && len(sockPath) >= 100 {
		if ownsDir {
			_ = os.RemoveAll(absHome)
		}
		return nil, fmt.Errorf("socket path %q (%d bytes) risks exceeding macOS 104-byte AF_UNIX limit", sockPath, len(sockPath))
	}

	p := &Profile{
		HomeDir:      absHome,
		DiscoveryDir: absHome + "-disc",
		SocketPath:   sockPath,
		ConfigPath:   filepath.Join(absHome, "config.toml"),
		UnicBin:      opts.UnicBin,
		UnicdBin:     opts.UnicdBin,
		ownsDir:      ownsDir,
	}

	for _, sub := range []string{
		p.DiscoveryDir,
		filepath.Join(p.DiscoveryDir, "roaming"),
		filepath.Join(p.DiscoveryDir, "local"),
		filepath.Join(p.DiscoveryDir, "config"),
		filepath.Join(p.DiscoveryDir, "data"),
		filepath.Join(p.DiscoveryDir, "cache"),
	} {
		if err := os.MkdirAll(sub, 0o700); err != nil {
			_ = p.Cleanup()
			return nil, fmt.Errorf("create discovery dir %s: %w", sub, err)
		}
	}

	cfg := buildBenchmarkConfigTOML(mode, lang, opts.StartInBackground)
	if err := os.WriteFile(p.ConfigPath, []byte(cfg), 0o600); err != nil {
		_ = p.Cleanup()
		return nil, fmt.Errorf("write config.toml: %w", err)
	}

	if _, err := p.runUnic(ctx, nil, "migrate", "--initialize-empty", "--json"); err != nil {
		_ = p.Cleanup()
		return nil, fmt.Errorf("initialize empty profile: %w", err)
	}

	setupPath := filepath.Join(absHome, "desktop-setup.json")
	if err := os.WriteFile(setupPath, []byte("{\"complete\":true}\n"), 0o600); err != nil {
		_ = p.Cleanup()
		return nil, fmt.Errorf("write desktop-setup.json: %w", err)
	}
	return p, nil
}

func buildBenchmarkConfigTOML(mode, lang string, startInBackground bool) string {
	return fmt.Sprintf(`[telemetry]
enabled = false

[sync]
enabled = false
automatic = false
files = false

[sync.directories]
automatic = false

[sync.relay]
enabled = false

[history]
auto_cleanup = false

[desktop]
mode = %q
language = %q
launch_at_login = false
start_in_background = %t
quick_panel_shortcut = ""
quick_panel_double_tap = ""

[desktop.notifications]
enabled = false
`, mode, lang, startInBackground)
}

// Env returns an isolated process environment bound solely to this profile,
// isolating HOME/APPDATA/LOCALAPPDATA/XDG_* so v1 discovery never reads real user data.
func (p *Profile) Env() []string {
	var env []string
	for _, entry := range os.Environ() {
		if strings.HasPrefix(entry, "UNIC_HOME=") ||
			strings.HasPrefix(entry, "UNIC_SOCK=") ||
			strings.HasPrefix(entry, "UNIC_PROFILE=") ||
			strings.HasPrefix(entry, "HOME=") ||
			strings.HasPrefix(entry, "USERPROFILE=") ||
			strings.HasPrefix(entry, "APPDATA=") ||
			strings.HasPrefix(entry, "LOCALAPPDATA=") ||
			strings.HasPrefix(entry, "XDG_CONFIG_HOME=") ||
			strings.HasPrefix(entry, "XDG_DATA_HOME=") ||
			strings.HasPrefix(entry, "XDG_CACHE_HOME=") {
			continue
		}
		env = append(env, entry)
	}
	disc := p.DiscoveryDir
	if disc == "" {
		disc = p.HomeDir + "-disc"
	}
	return append(env,
		"UNIC_HOME="+p.HomeDir,
		"UNIC_SOCK="+p.SocketPath,
		"HOME="+disc,
		"USERPROFILE="+disc,
		"APPDATA="+filepath.Join(disc, "roaming"),
		"LOCALAPPDATA="+filepath.Join(disc, "local"),
		"XDG_CONFIG_HOME="+filepath.Join(disc, "config"),
		"XDG_DATA_HOME="+filepath.Join(disc, "data"),
		"XDG_CACHE_HOME="+filepath.Join(disc, "cache"),
	)
}

// StartDaemon launches unicd bound to this isolated profile and waits until /v1/service responds.
func (p *Profile) StartDaemon(ctx context.Context) (int, error) {
	if p.daemonCmd != nil && p.daemonPID > 0 {
		return p.daemonPID, nil
	}
	if p.UnicdBin == "" {
		return 0, errors.New("UnicdBin is required to start unicd")
	}
	logPath := filepath.Join(p.HomeDir, "bench-unicd.log")
	logFile, err := os.OpenFile(logPath, os.O_CREATE|os.O_WRONLY|os.O_APPEND, 0o600)
	if err != nil {
		return 0, fmt.Errorf("open unicd log: %w", err)
	}

	cmd := exec.Command(p.UnicdBin)
	cmd.Env = p.Env()
	cmd.Stdin = nil
	cmd.Stdout = logFile
	cmd.Stderr = logFile
	if err := cmd.Start(); err != nil {
		_ = logFile.Close()
		return 0, fmt.Errorf("start unicd: %w", err)
	}
	_ = logFile.Close()

	done := make(chan error, 1)
	go func() { done <- cmd.Wait() }()

	p.daemonCmd = cmd
	p.daemonPID = cmd.Process.Pid
	p.daemonDone = done

	waitCtx, cancel := context.WithTimeout(ctx, 25*time.Second)
	defer cancel()
	for {
		select {
		case exitErr := <-done:
			p.daemonCmd = nil
			p.daemonPID = 0
			p.daemonDone = nil
			return 0, fmt.Errorf("unicd exited prematurely (%v); see %s", exitErr, logPath)
		default:
		}
		if svc, err := p.probeService(waitCtx); err == nil && svc.PID == cmd.Process.Pid {
			return p.daemonPID, nil
		}
		select {
		case <-waitCtx.Done():
			_ = p.StopDaemon(context.Background())
			return 0, fmt.Errorf("wait for unicd readiness: %w; see %s", waitCtx.Err(), logPath)
		case <-time.After(100 * time.Millisecond):
		}
	}
}

// StopDaemon stops only the exact unicd process started by this Profile.
func (p *Profile) StopDaemon(ctx context.Context) error {
	if p.daemonCmd == nil || p.daemonCmd.Process == nil {
		return nil
	}
	cmd := p.daemonCmd
	done := p.daemonDone
	pid := p.daemonPID
	p.daemonCmd = nil
	p.daemonPID = 0
	p.daemonDone = nil

	// Ask unicd to stop gracefully via `unic daemon stop` first; fall back to
	// signaling only the recorded PID.
	stopCtx, cancel := context.WithTimeout(ctx, 10*time.Second)
	defer cancel()
	_, _ = p.runUnic(stopCtx, nil, "daemon", "stop", "--json")

	select {
	case err := <-done:
		_ = err
		return nil
	case <-time.After(3 * time.Second):
	}

	if err := signalTerminate(cmd.Process); err != nil && !errors.Is(err, os.ErrProcessDone) {
		_ = cmd.Process.Kill()
	}
	select {
	case err := <-done:
		_ = err
		return nil
	case <-time.After(5 * time.Second):
		if err := cmd.Process.Kill(); err != nil && !errors.Is(err, os.ErrProcessDone) {
			return fmt.Errorf("kill unicd pid %d: %w", pid, err)
		}
		select {
		case err := <-done:
			_ = err
		case <-time.After(2 * time.Second):
		}
		return nil
	}
}

// RestartDaemon stops the current unicd (if running) and launches a clean instance.
func (p *Profile) RestartDaemon(ctx context.Context) (int, error) {
	if err := p.StopDaemon(ctx); err != nil {
		return 0, err
	}
	return p.StartDaemon(ctx)
}

// Cleanup stops any daemon started by this profile and removes owned temporary directories.
func (p *Profile) Cleanup() error {
	var errs []error
	if err := p.StopDaemon(context.Background()); err != nil {
		errs = append(errs, err)
	}
	if p.ownsDir && p.HomeDir != "" {
		if err := os.RemoveAll(p.HomeDir); err != nil {
			errs = append(errs, err)
		}
	}
	if p.ownsDir && p.DiscoveryDir != "" {
		if err := os.RemoveAll(p.DiscoveryDir); err != nil {
			errs = append(errs, err)
		}
	}
	return errors.Join(errs...)
}

// SeedProfile populates the isolated profile with every asset from manifest using
// `unic send --to all` (no paired devices, local history commit only), verifies
// the history entry count, and restarts unicd (or stops it if keepDaemon is false)
// so seeding-time transient heap growth does not pollute steady-state runs.
func SeedProfile(ctx context.Context, p *Profile, fixtureDir string, manifest *Manifest, keepDaemon bool) (*SeedResult, error) {
	if p == nil || manifest == nil {
		return nil, errors.New("profile and manifest are required")
	}
	start := time.Now()
	if _, err := p.StartDaemon(ctx); err != nil {
		return nil, err
	}

	res := &SeedResult{}
	for _, t := range manifest.Texts {
		if err := p.sendExpectCommit(ctx, strings.NewReader(t.Content), "send", "--to", "all", "--json"); err != nil {
			return nil, fmt.Errorf("seed text %d: %w", t.Index, err)
		}
		res.SeededTexts++
	}
	for _, f := range manifest.Files {
		abs := filepath.Join(fixtureDir, filepath.FromSlash(f.RelativePath))
		if err := p.sendExpectCommit(ctx, nil, "send", "--to", "all", "-f", "--json", abs); err != nil {
			return nil, fmt.Errorf("seed file %d: %w", f.Index, err)
		}
		res.SeededFiles++
	}
	// Seed images last so newest history entries (@1..@N) include 2K PNG previews
	// for immediate Quick Panel and GUI selection benchmarks.
	for _, img := range manifest.Images {
		abs := filepath.Join(fixtureDir, filepath.FromSlash(img.RelativePath))
		if err := p.sendExpectCommit(ctx, nil, "send", "--to", "all", "--png", abs, "--json"); err != nil {
			return nil, fmt.Errorf("seed image %d: %w", img.Index, err)
		}
		res.SeededImages++
	}

	expected := manifest.ExpectedCount()
	count, err := p.HistoryCount(ctx, max(expected+50, 200))
	if err != nil {
		return nil, fmt.Errorf("verify seeded history count: %w", err)
	}
	res.HistoryCount = count
	if count != expected {
		return nil, fmt.Errorf("seeded history count mismatch: got %d, want %d", count, expected)
	}

	if keepDaemon {
		pid, err := p.RestartDaemon(ctx)
		if err != nil {
			return nil, fmt.Errorf("restart unicd after seed: %w", err)
		}
		res.RestartedPID = pid
	} else {
		if err := p.StopDaemon(ctx); err != nil {
			return nil, fmt.Errorf("stop unicd after seed: %w", err)
		}
		res.DaemonStopped = true
	}
	res.DurationMs = time.Since(start).Milliseconds()
	return res, nil
}

// HistoryCount queries `unic history list` and returns the number of stored entries.
func (p *Profile) HistoryCount(ctx context.Context, limit int) (int, error) {
	if limit < 1 {
		limit = 200
	}
	out, err := p.runUnic(ctx, nil, "history", "list", "-n", fmt.Sprintf("%d", limit), "--json")
	if err != nil {
		return 0, err
	}
	var items []json.RawMessage
	if err := json.Unmarshal(bytes.TrimSpace(out), &items); err != nil {
		return 0, fmt.Errorf("parse history list json: %w", err)
	}
	return len(items), nil
}

func (p *Profile) sendExpectCommit(ctx context.Context, stdin io.Reader, args ...string) error {
	out, exitCode, stderr, err := p.execUnic(ctx, stdin, args...)
	if err != nil && exitCode != 4 {
		return fmt.Errorf("exit=%d err=%v stderr=%s", exitCode, err, strings.TrimSpace(stderr))
	}
	var report struct {
		Submission struct {
			HistoryID string `json:"history_id"`
			Phase     string `json:"phase"`
		} `json:"submission"`
	}
	if uerr := json.Unmarshal(bytes.TrimSpace(out), &report); uerr != nil {
		return fmt.Errorf("decode send json (exit=%d): %w; raw=%s", exitCode, uerr, strings.TrimSpace(string(out)))
	}
	if report.Submission.HistoryID == "" {
		return fmt.Errorf("send did not produce history_id (exit=%d, phase=%q)", exitCode, report.Submission.Phase)
	}
	return nil
}

func (p *Profile) runUnic(ctx context.Context, stdin io.Reader, args ...string) ([]byte, error) {
	out, exitCode, stderr, err := p.execUnic(ctx, stdin, args...)
	if err != nil {
		return out, fmt.Errorf("unic %s failed (exit=%d): %w; stderr=%s", strings.Join(args, " "), exitCode, err, strings.TrimSpace(stderr))
	}
	return out, nil
}

func (p *Profile) execUnic(ctx context.Context, stdin io.Reader, args ...string) (stdout []byte, exitCode int, stderr string, err error) {
	cmd := exec.CommandContext(ctx, p.UnicBin, args...)
	cmd.Env = p.Env()
	cmd.Stdin = stdin
	var outBuf, errBuf bytes.Buffer
	cmd.Stdout = &outBuf
	cmd.Stderr = &errBuf
	runErr := cmd.Run()
	if runErr != nil {
		var ee *exec.ExitError
		if errors.As(runErr, &ee) {
			exitCode = ee.ExitCode()
		} else {
			exitCode = -1
		}
	}
	return outBuf.Bytes(), exitCode, errBuf.String(), runErr
}

type serviceInfo struct {
	PID       int    `json:"pid"`
	Directory string `json:"directory"`
	Mode      string `json:"mode"`
}

func (p *Profile) probeService(ctx context.Context) (serviceInfo, error) {
	reqCtx, cancel := context.WithTimeout(ctx, 2*time.Second)
	defer cancel()
	client := &http.Client{
		Transport: &http.Transport{
			DialContext: func(dctx context.Context, _, _ string) (net.Conn, error) {
				return (&net.Dialer{}).DialContext(dctx, "unix", p.SocketPath)
			},
		},
	}
	req, err := http.NewRequestWithContext(reqCtx, http.MethodGet, "http://local/v1/service", nil)
	if err != nil {
		return serviceInfo{}, err
	}
	resp, err := client.Do(req)
	if err != nil {
		return serviceInfo{}, err
	}
	defer func() { _ = resp.Body.Close() }()
	if resp.StatusCode != http.StatusOK {
		return serviceInfo{}, fmt.Errorf("http %d", resp.StatusCode)
	}
	var info serviceInfo
	if err := json.NewDecoder(resp.Body).Decode(&info); err != nil {
		return serviceInfo{}, err
	}
	return info, nil
}

func signalTerminate(proc *os.Process) error {
	if runtime.GOOS == "windows" {
		return proc.Kill()
	}
	return proc.Signal(os.Interrupt)
}
