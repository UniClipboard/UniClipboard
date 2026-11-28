package scenarios

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"net"
	"net/http"
	"os"
	"os/exec"
	"path/filepath"
	"runtime"
	"strconv"
	"strings"
	"time"

	"github.com/uniclipboard/unic/tools/bench/internal/fixture"
	"github.com/uniclipboard/unic/tools/bench/internal/sampler"
)

type roleProbeInfo struct {
	Role         string `json:"role"`
	PID          int    `json:"pid"`
	Profile      string `json:"profile"`
	BinaryPath   string `json:"binary_path"`
	BinarySHA256 string `json:"binary_sha256"`
}

type roleStatusInfo struct {
	Ready                bool `json:"ready"`
	SetupRequired        bool `json:"setup_required"`
	Paused               bool `json:"paused"`
	Visible              bool `json:"visible"`
	Focused              bool `json:"focused"`
	ServicePID           int  `json:"service_pid"`
	ClipboardUnavailable bool `json:"clipboard_unavailable"`
}

type serviceStatus struct {
	PID       int    `json:"pid"`
	Directory string `json:"directory"`
	Mode      string `json:"mode"`
	Retiring  bool   `json:"retiring"`
}

type historyItemSummary struct {
	ID   string `json:"id"`
	Kind string `json:"kind"`
}

// benchSession manages an isolated benchmark profile and strictly tracks
// every spawned PID + executable path so teardown never touches unrelated user processes.
type benchSession struct {
	profile     *fixture.Profile
	desktopBin  string
	interactive bool
	metricsDir  string
	trackedPIDs map[int]string // pid -> expected executable path
}

func newBenchSession(ctx context.Context, unicBin, unicdBin, desktopBin string, mode string, interactive bool, metricsDir string) (*benchSession, error) {
	prof, err := fixture.InitProfile(ctx, fixture.ProfileOptions{
		UnicBin:           unicBin,
		UnicdBin:          unicdBin,
		DesktopMode:       mode,
		StartInBackground: false,
		Language:          "en",
	})
	if err != nil {
		return nil, err
	}
	if interactive {
		if err := os.WriteFile(filepath.Join(prof.HomeDir, "e2e-owner"), []byte("desktop-e2e"), 0o600); err != nil {
			_ = prof.Cleanup()
			return nil, err
		}
		if err := os.MkdirAll(filepath.Join(prof.HomeDir, "board"), 0o700); err != nil {
			_ = prof.Cleanup()
			return nil, err
		}
		if metricsDir != "" {
			if err := os.MkdirAll(metricsDir, 0o700); err != nil {
				_ = prof.Cleanup()
				return nil, err
			}
		}
	}
	absDesktop, err := filepath.Abs(desktopBin)
	if err == nil {
		if resolved, rerr := filepath.EvalSymlinks(absDesktop); rerr == nil {
			absDesktop = resolved
		}
	}
	return &benchSession{
		profile:     prof,
		desktopBin:  absDesktop,
		interactive: interactive,
		metricsDir:  metricsDir,
		trackedPIDs: make(map[int]string),
	}, nil
}

func (s *benchSession) env(forLauncher bool) []string {
	base := s.profile.Env()
	out := make([]string, 0, len(base)+8)
	for _, entry := range base {
		if strings.HasPrefix(entry, "MYGO_") ||
			strings.HasPrefix(entry, "UNIC_DESKTOP_E2E_") ||
			strings.HasPrefix(entry, "UNIC_BENCH_METRICS_DIR=") ||
			strings.HasPrefix(entry, "UNIC_RENDEZVOUS_URL=") {
			continue
		}
		out = append(out, entry)
	}
	out = append(out, "UNIC_RENDEZVOUS_URL=http://127.0.0.1:1")
	if s.interactive {
		out = append(out,
			"UNIC_DESKTOP_E2E_PROFILE="+s.profile.HomeDir,
			"UNIC_DESKTOP_E2E_WINDOW=1100x720",
			"UNIC_DESKTOP_E2E_THEME=light",
			"MYGO_AUTOMATION=",
		)
		if s.metricsDir != "" {
			out = append(out, "UNIC_BENCH_METRICS_DIR="+s.metricsDir)
		}
	}
	_ = forLauncher
	return out
}

func (s *benchSession) seed(ctx context.Context, fixtureDir string, manifest *fixture.Manifest) error {
	if manifest == nil || manifest.ExpectedCount() == 0 {
		return nil
	}
	// SeedProfile starts unicd, seeds all items, verifies history count, and
	// stops unicd cleanly so seeding-time heap does not pollute steady-state runs (F8).
	_, err := fixture.SeedProfile(ctx, s.profile, fixtureDir, manifest, false)
	return err
}

func (s *benchSession) trackPID(pid int, exePath string) {
	if pid <= 0 {
		return
	}
	canon := exePath
	if resolved, err := filepath.EvalSymlinks(exePath); err == nil {
		canon = resolved
	}
	s.trackedPIDs[pid] = canon
}

func (s *benchSession) startDaemon(ctx context.Context) (int, error) {
	// Set metrics dir on process env if interactive bench_probe is active.
	if s.interactive && s.metricsDir != "" {
		// Launch unicd with interactive env so UNIC_BENCH_METRICS_DIR is inherited.
		logPath := filepath.Join(s.profile.HomeDir, "bench-unicd.log")
		f, err := os.OpenFile(logPath, os.O_CREATE|os.O_WRONLY|os.O_APPEND, 0o600)
		if err != nil {
			return 0, err
		}
		cmd := exec.Command(s.profile.UnicdBin)
		cmd.Env = s.env(true)
		cmd.Stdout = f
		cmd.Stderr = f
		if err := cmd.Start(); err != nil {
			_ = f.Close()
			return 0, err
		}
		_ = f.Close()
		done := make(chan error, 1)
		go func() { done <- cmd.Wait() }()
		pid := cmd.Process.Pid
		s.trackPID(pid, s.profile.UnicdBin)

		wctx, cancel := context.WithTimeout(ctx, 25*time.Second)
		defer cancel()
		for {
			select {
			case exitErr := <-done:
				return 0, fmt.Errorf("unicd exited early: %v", exitErr)
			default:
			}
			if svc, err := s.service(wctx); err == nil && svc.PID == pid {
				return pid, nil
			}
			select {
			case <-wctx.Done():
				return 0, wctx.Err()
			case <-time.After(100 * time.Millisecond):
			}
		}
	}
	pid, err := s.profile.StartDaemon(ctx)
	if err != nil {
		return 0, err
	}
	s.trackPID(pid, s.profile.UnicdBin)
	return pid, nil
}

func (s *benchSession) runDesktop(ctx context.Context, args ...string) ([]byte, error) {
	cmd := exec.CommandContext(ctx, s.desktopBin, args...)
	cmd.Env = s.env(true)
	var outBuf, errBuf bytes.Buffer
	cmd.Stdout = &outBuf
	cmd.Stderr = &errBuf
	if err := cmd.Run(); err != nil {
		return outBuf.Bytes(), fmt.Errorf("unic-desktop %s: %w; stderr=%s", strings.Join(args, " "), err, strings.TrimSpace(errBuf.String()))
	}
	return outBuf.Bytes(), nil
}

func (s *benchSession) roleProbe(ctx context.Context, role string) (roleProbeInfo, error) {
	raw, err := s.runDesktop(ctx, "--role", role, "--command", "probe")
	if err != nil {
		return roleProbeInfo{}, err
	}
	var info roleProbeInfo
	if err := json.Unmarshal(bytes.TrimSpace(raw), &info); err != nil {
		return roleProbeInfo{}, fmt.Errorf("decode probe %s: %w", role, err)
	}
	if err := verifyRoleOwnership(info, s.profile.HomeDir, s.desktopBin); err != nil {
		return roleProbeInfo{}, err
	}
	s.trackPID(info.PID, s.desktopBin)
	return info, nil
}

func (s *benchSession) roleStatus(ctx context.Context, role string) (roleStatusInfo, error) {
	raw, err := s.runDesktop(ctx, "--role", role, "--command", "status")
	if err != nil {
		return roleStatusInfo{}, err
	}
	var st roleStatusInfo
	if err := json.Unmarshal(bytes.TrimSpace(raw), &st); err != nil {
		return roleStatusInfo{}, fmt.Errorf("decode status %s: %w", role, err)
	}
	return st, nil
}

func (s *benchSession) roleCommand(ctx context.Context, role, command string) error {
	_, err := s.runDesktop(ctx, "--role", role, "--command", command)
	return err
}

func (s *benchSession) service(ctx context.Context) (serviceStatus, error) {
	reqCtx, cancel := context.WithTimeout(ctx, 2*time.Second)
	defer cancel()
	client := &http.Client{
		Transport: &http.Transport{
			DialContext: func(dctx context.Context, _, _ string) (net.Conn, error) {
				return (&net.Dialer{}).DialContext(dctx, "unix", s.profile.SocketPath)
			},
		},
	}
	req, err := http.NewRequestWithContext(reqCtx, http.MethodGet, "http://local/v1/service", nil)
	if err != nil {
		return serviceStatus{}, err
	}
	resp, err := client.Do(req)
	if err != nil {
		return serviceStatus{}, err
	}
	defer func() { _ = resp.Body.Close() }()
	if resp.StatusCode != http.StatusOK {
		return serviceStatus{}, fmt.Errorf("http %d", resp.StatusCode)
	}
	var svc serviceStatus
	if err := json.NewDecoder(resp.Body).Decode(&svc); err != nil {
		return serviceStatus{}, err
	}
	return svc, nil
}

func (s *benchSession) listHistory(ctx context.Context, limit int) ([]historyItemSummary, error) {
	reqCtx, cancel := context.WithTimeout(ctx, 5*time.Second)
	defer cancel()
	client := &http.Client{
		Transport: &http.Transport{
			DialContext: func(dctx context.Context, _, _ string) (net.Conn, error) {
				return (&net.Dialer{}).DialContext(dctx, "unix", s.profile.SocketPath)
			},
		},
	}
	req, err := http.NewRequestWithContext(reqCtx, http.MethodGet, fmt.Sprintf("http://local/v1/history?limit=%d", limit), nil)
	if err != nil {
		return nil, err
	}
	resp, err := client.Do(req)
	if err != nil {
		return nil, err
	}
	defer func() { _ = resp.Body.Close() }()
	if resp.StatusCode != http.StatusOK {
		return nil, fmt.Errorf("http %d", resp.StatusCode)
	}
	var page struct {
		Items []historyItemSummary `json:"items"`
	}
	if err := json.NewDecoder(resp.Body).Decode(&page); err != nil {
		return nil, err
	}
	return page.Items, nil
}

func (s *benchSession) waitRoleReady(ctx context.Context, role string, requireVisible bool) (roleProbeInfo, roleStatusInfo, error) {
	wctx, cancel := context.WithTimeout(ctx, 25*time.Second)
	defer cancel()
	for {
		info, perr := s.roleProbe(wctx, role)
		if perr == nil {
			st, serr := s.roleStatus(wctx, role)
			if serr == nil && (!requireVisible || st.Visible) {
				if st.ServicePID > 0 {
					s.trackPID(st.ServicePID, s.profile.UnicdBin)
				}
				return info, st, nil
			}
		}
		select {
		case <-wctx.Done():
			return roleProbeInfo{}, roleStatusInfo{}, fmt.Errorf("wait role %s ready (visible=%t): %w", role, requireVisible, wctx.Err())
		case <-time.After(100 * time.Millisecond):
		}
	}
}

func (s *benchSession) waitRoleAbsent(ctx context.Context, role string) error {
	wctx, cancel := context.WithTimeout(ctx, 15*time.Second)
	defer cancel()
	for {
		if _, err := s.roleProbe(wctx, role); err != nil {
			return nil
		}
		select {
		case <-wctx.Done():
			return fmt.Errorf("role %s did not exit within timeout", role)
		case <-time.After(100 * time.Millisecond):
		}
	}
}

func (s *benchSession) startNormalBackground(ctx context.Context) (map[string]int, error) {
	if _, err := s.runDesktop(ctx, "--background", "--mode", "normal"); err != nil {
		return nil, err
	}
	trayInfo, traySt, err := s.waitRoleReady(ctx, "tray", false)
	if err != nil {
		return nil, err
	}
	if traySt.ServicePID <= 0 {
		svc, err := s.service(ctx)
		if err != nil {
			return nil, fmt.Errorf("unicd service missing after normal background start: %w", err)
		}
		traySt.ServicePID = svc.PID
	}
	s.trackPID(traySt.ServicePID, s.profile.UnicdBin)

	qpInfo, _, err := s.waitRoleReady(ctx, "quick-panel", false)
	if err != nil {
		return nil, err
	}
	if _, err := s.roleProbe(ctx, "gui"); err == nil {
		return nil, errors.New("unexpected gui process running during normal background idle")
	}
	return map[string]int{
		"unicd":       traySt.ServicePID,
		"tray":        trayInfo.PID,
		"quick-panel": qpInfo.PID,
	}, nil
}

func (s *benchSession) startLightweightBackground(ctx context.Context) (map[string]int, error) {
	if _, err := s.runDesktop(ctx, "--background", "--mode", "lightweight"); err != nil {
		return nil, err
	}
	wctx, cancel := context.WithTimeout(ctx, 20*time.Second)
	defer cancel()
	var svc serviceStatus
	for {
		v, err := s.service(wctx)
		if err == nil && v.PID > 0 {
			svc = v
			break
		}
		select {
		case <-wctx.Done():
			return nil, fmt.Errorf("lightweight unicd unavailable: %w", wctx.Err())
		case <-time.After(100 * time.Millisecond):
		}
	}
	if err := verifyServiceOwnership(svc, s.profile.HomeDir); err != nil {
		return nil, err
	}
	s.trackPID(svc.PID, s.profile.UnicdBin)
	for _, forbidden := range []string{"tray", "gui", "quick-panel"} {
		if _, err := s.roleProbe(ctx, forbidden); err == nil {
			return nil, fmt.Errorf("lightweight background unexpectedly has %s running", forbidden)
		}
	}
	return map[string]int{"unicd": svc.PID}, nil
}

func (s *benchSession) openGUI(ctx context.Context) (int, float64, error) {
	started := time.Now()
	if err := s.roleCommand(ctx, "tray", "open-gui"); err != nil {
		return 0, 0, err
	}
	info, _, err := s.waitRoleReady(ctx, "gui", true)
	if err != nil {
		return 0, 0, err
	}
	if runtime.GOOS == "windows" {
		if err := waitVisibleWindowsHWND(ctx, info.PID); err != nil {
			return 0, 0, err
		}
	}
	return info.PID, float64(time.Since(started).Microseconds()) / 1000.0, nil
}

func (s *benchSession) closeGUI(ctx context.Context) error {
	_ = s.roleCommand(ctx, "gui", "close")
	return s.waitRoleAbsent(ctx, "gui")
}

func (s *benchSession) openQuickPanelWarm(ctx context.Context) (int, float64, error) {
	started := time.Now()
	if err := s.roleCommand(ctx, "tray", "open-quick-panel"); err != nil {
		return 0, 0, err
	}
	info, _, err := s.waitRoleReady(ctx, "quick-panel", true)
	if err != nil {
		return 0, 0, err
	}
	if runtime.GOOS == "windows" {
		if err := waitVisibleWindowsHWND(ctx, info.PID); err != nil {
			return 0, 0, err
		}
	}
	return info.PID, float64(time.Since(started).Microseconds()) / 1000.0, nil
}

func (s *benchSession) closeQuickPanel(ctx context.Context) error {
	if err := s.roleCommand(ctx, "quick-panel", "close"); err != nil {
		return err
	}
	wctx, cancel := context.WithTimeout(ctx, 10*time.Second)
	defer cancel()
	for {
		st, err := s.roleStatus(wctx, "quick-panel")
		if err == nil && !st.Visible {
			return nil
		}
		select {
		case <-wctx.Done():
			return fmt.Errorf("quick-panel did not hide: %w", wctx.Err())
		case <-time.After(50 * time.Millisecond):
		}
	}
}

func waitVisibleWindowsHWND(ctx context.Context, pid int) error {
	wctx, cancel := context.WithTimeout(ctx, 10*time.Second)
	defer cancel()
	for {
		ps, err := sampler.SampleProcess(pid)
		if err == nil && ps.VisibleWindows >= 1 {
			return nil
		}
		select {
		case <-wctx.Done():
			return fmt.Errorf("windows EnumWindows visible HWND >= 1 timeout for pid %d", pid)
		case <-time.After(50 * time.Millisecond):
		}
	}
}

func (s *benchSession) teardown(keepProfile bool) error {
	ctx, cancel := context.WithTimeout(context.Background(), 15*time.Second)
	defer cancel()

	// Graceful shutdown via role IPC and daemon stop.
	_ = s.roleCommand(ctx, "gui", "quit")
	_ = s.roleCommand(ctx, "quick-panel", "quit")
	_ = s.roleCommand(ctx, "tray", "quit")

	// Also ask standalone unicd to stop if running.
	if svc, err := s.service(ctx); err == nil && svc.PID > 0 {
		s.trackPID(svc.PID, s.profile.UnicdBin)
		stopCmd := exec.CommandContext(ctx, s.profile.UnicBin, "daemon", "stop", "--json")
		stopCmd.Env = s.profile.Env()
		_, _ = stopCmd.CombinedOutput()
	}

	// Wait for every tracked PID to exit; only kill verified tracked PIDs.
	var errs []error
	deadline := time.Now().Add(8 * time.Second)
	for pid, expectedExe := range s.trackedPIDs {
		for time.Now().Before(deadline) {
			if _, err := sampler.SampleProcess(pid); err != nil {
				break
			}
			time.Sleep(100 * time.Millisecond)
		}
		if _, err := sampler.SampleProcess(pid); err == nil {
			if matchesExecutable(pid, expectedExe) {
				if proc, ferr := os.FindProcess(pid); ferr == nil {
					_ = proc.Kill()
					killDeadline := time.Now().Add(2 * time.Second)
					for time.Now().Before(killDeadline) {
						if _, err := sampler.SampleProcess(pid); err != nil {
							break
						}
						time.Sleep(50 * time.Millisecond)
					}
				}
			}
			if _, stillErr := sampler.SampleProcess(pid); stillErr == nil {
				errs = append(errs, fmt.Errorf("tracked pid %d (%s) still alive after cleanup", pid, expectedExe))
			}
		}
	}

	if err := s.profile.StopDaemon(ctx); err != nil {
		errs = append(errs, err)
	}
	if !keepProfile {
		if err := s.profile.Cleanup(); err != nil {
			errs = append(errs, err)
		}
	}
	return errors.Join(errs...)
}

func verifyRoleOwnership(info roleProbeInfo, profileDir, desktopBin string) error {
	wantProfile := canonicalPath(profileDir)
	gotProfile := canonicalPath(info.Profile)
	if wantProfile == "" || gotProfile != wantProfile {
		return fmt.Errorf("role %s profile mismatch: got %q, want %q", info.Role, info.Profile, profileDir)
	}
	wantBin := canonicalPath(desktopBin)
	gotBin := canonicalPath(info.BinaryPath)
	if wantBin == "" || !samePathFold(gotBin, wantBin) {
		return fmt.Errorf("role %s binary mismatch: got %q, want %q", info.Role, info.BinaryPath, desktopBin)
	}
	if info.PID <= 0 {
		return fmt.Errorf("role %s returned invalid pid %d", info.Role, info.PID)
	}
	return nil
}

func verifyServiceOwnership(svc serviceStatus, profileDir string) error {
	want := canonicalPath(profileDir)
	got := canonicalPath(svc.Directory)
	if want == "" || !samePathFold(got, want) {
		return fmt.Errorf("service directory mismatch: got %q, want %q", svc.Directory, profileDir)
	}
	return nil
}

func matchesExecutable(pid int, expectedExe string) bool {
	if pid <= 0 || expectedExe == "" {
		return false
	}
	want := canonicalPath(expectedExe)
	switch runtime.GOOS {
	case "darwin":
		out, err := exec.Command("/bin/ps", "-p", strconv.Itoa(pid), "-o", "comm=").Output()
		if err != nil {
			return false
		}
		got := canonicalPath(strings.TrimSpace(string(out)))
		return samePathFold(got, want)
	case "windows":
		out, err := exec.Command("powershell", "-NoProfile", "-Command",
			fmt.Sprintf("(Get-Process -Id %d -ErrorAction SilentlyContinue).Path", pid)).Output()
		if err != nil {
			return false
		}
		got := canonicalPath(strings.TrimSpace(string(out)))
		return got != "" && samePathFold(got, want)
	default:
		return false
	}
}

func canonicalPath(p string) string {
	if p == "" {
		return ""
	}
	abs, err := filepath.Abs(p)
	if err == nil {
		p = abs
	}
	if resolved, err := filepath.EvalSymlinks(p); err == nil {
		p = resolved
	}
	return filepath.Clean(p)
}

func samePathFold(a, b string) bool {
	if runtime.GOOS == "windows" || runtime.GOOS == "darwin" {
		return strings.EqualFold(a, b)
	}
	return a == b
}

// waitDaemonConverged waits for unicd to exit its initial startup memory plateau
// (e.g. ~30-35s post-start Go heap settle on macOS) before measuring baselines.
func waitDaemonConverged(ctx context.Context, unicdPID int, minWait, maxWait time.Duration) error {
	if unicdPID <= 0 {
		return fmt.Errorf("invalid unicd pid %d", unicdPID)
	}
	if maxWait < minWait {
		maxWait = minWait
	}
	started := time.Now()
	wctx, cancel := context.WithTimeout(ctx, maxWait)
	defer cancel()

	var samples []uint64
	ticker := time.NewTicker(time.Second)
	defer ticker.Stop()

	for {
		ps, err := sampler.SampleProcess(unicdPID)
		if err != nil {
			return fmt.Errorf("unicd exited while converging (pid=%d): %w", unicdPID, err)
		}
		samples = append(samples, ps.PrimaryMemoryBytes)
		if daemonHasConverged(samples, time.Since(started), minWait) {
			return nil
		}
		select {
		case <-wctx.Done():
			if ctx.Err() != nil {
				return ctx.Err()
			}
			// Reached bounded maxWait cleanly; proceed with current daemon state.
			return nil
		case <-ticker.C:
		}
	}
}

func daemonHasConverged(samples []uint64, elapsed, minWait time.Duration) bool {
	const stableWindow = 5
	const jitterTolerance = uint64(2 << 20) // 2 MiB
	if len(samples) < stableWindow {
		return false
	}
	var peak uint64
	for _, s := range samples {
		if s > peak {
			peak = s
		}
	}
	tail := samples[len(samples)-stableWindow:]
	minTail, maxTail := tail[0], tail[0]
	for _, s := range tail[1:] {
		if s < minTail {
			minTail = s
		}
		if s > maxTail {
			maxTail = s
		}
	}
	stable := (maxTail - minTail) <= jitterTolerance
	if !stable {
		return false
	}
	last := tail[len(tail)-1]
	// Early exit once an explicit >=30% drop from startup peak has stabilized after >=25s,
	// or once a flat profile is already settled <= 35 MiB past minWait (e.g. Windows/empty).
	if peak > 0 && elapsed >= 25*time.Second && last <= uint64(float64(peak)*0.70) {
		return true
	}
	if elapsed >= minWait && last <= (35<<20) {
		return true
	}
	return false
}
