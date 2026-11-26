package main

import (
	"bufio"
	"context"
	"encoding/json"
	"errors"
	"flag"
	"fmt"
	"io"
	"os"
	"os/exec"
	"path/filepath"
	"strconv"
	"strings"
	"syscall"
	"time"

	"github.com/uniclipboard/unic/internal/climigration"
	api "github.com/uniclipboard/unic/internal/localapi"
	paths "github.com/uniclipboard/unic/internal/localpaths"
	"github.com/uniclipboard/unic/internal/servicelaunch"
	"github.com/uniclipboard/unic/internal/servicelifecycle"
)

const daemonUsage = `Manage the background unicd service for the active profile.

Usage:
  unic daemon <command> [flags]

Commands:
  start [--foreground] [--json]   Start unicd in the background (or foreground)
  stop [--json]                   Stop a running standalone unicd
  restart [--json]                Restart standalone unicd
  status [--json]                 Show daemon state without starting a service
  logs [-n LINES] [-f|--follow]   Show daemon log output

Examples:
  unic daemon start
  unic daemon status
  unic daemon restart
  unic --profile work daemon start
`

func runDaemonCmd(ctx context.Context, sub string, args []string, asJSONDefault bool) error {
	if wantsHelp(args) {
		printDaemonSubHelp(sub)
		return nil
	}
	switch sub {
	case "help":
		fmt.Print(daemonUsage)
		return nil
	case "start":
		return runDaemonStart(ctx, args, asJSONDefault)
	case "stop":
		return runDaemonStop(ctx, args, asJSONDefault)
	case "restart":
		return runDaemonRestart(ctx, args, asJSONDefault)
	case "status":
		return runDaemonStatus(ctx, args, asJSONDefault)
	case "logs":
		return runDaemonLogs(ctx, args)
	default:
		return usageError(strings.TrimRight(daemonUsage, "\n"))
	}
}

func printDaemonSubHelp(sub string) {
	switch sub {
	case "start":
		fmt.Print("Usage: unic daemon start [--foreground] [--json]\n\nStart the background unicd service for the active profile.\nUse --foreground to keep unicd attached to the current terminal.\n")
	case "stop":
		fmt.Print("Usage: unic daemon stop [--json]\n\nStop the running standalone unicd service for the active profile.\n")
	case "restart":
		fmt.Print("Usage: unic daemon restart [--json]\n\nRestart the standalone unicd service (applies config or plugin changes).\n")
	case "status":
		fmt.Print("Usage: unic daemon status [--json]\n\nInspect daemon state without starting a temporary service.\n")
	case "logs":
		fmt.Print("Usage: unic daemon logs [-n LINES] [-f|--follow]\n\nPrint the tail of the daemon log file.\n")
	default:
		fmt.Print(daemonUsage)
	}
}

func runDaemonStart(ctx context.Context, args []string, asJSONDefault bool) error {
	fs := flag.NewFlagSet("daemon start", flag.ContinueOnError)
	fs.SetOutput(io.Discard)
	fg := fs.Bool("foreground", false, "run unicd in the foreground")
	asJSON := fs.Bool("json", asJSONDefault, "output JSON")
	pos, err := parse(fs, args)
	if err != nil || len(pos) != 0 {
		return usageError("usage: unic daemon start [--foreground] [--json]")
	}
	if *fg && *asJSON {
		return usageError("--foreground cannot be combined with --json")
	}
	dir, err := paths.Dir()
	if err != nil {
		return err
	}
	if err := climigration.Required(dir); err != nil {
		return err
	}
	socket, err := paths.Socket()
	if err != nil {
		return err
	}
	if *fg {
		return runDaemonForeground(ctx, dir, socket)
	}
	inst, started, err := startStandaloneDaemon(ctx, dir, socket)
	if err != nil {
		return err
	}
	profileName, _ := paths.ActiveProfile()
	if *asJSON {
		return json.NewEncoder(os.Stdout).Encode(map[string]any{
			"running":   true,
			"started":   started,
			"profile":   profileName,
			"directory": dir,
			"socket":    socket,
			"instance":  inst,
		})
	}
	if started {
		fmt.Printf("%s daemon started (pid %d, profile %s)\n", style(outTTY, sOK, "✓"), inst.PID, profileName)
		note("logs: %s", filepath.Join(dir, "daemon.log"))
	} else {
		fmt.Printf("daemon is already running (mode %s, pid %d, profile %s)\n", inst.Mode, inst.PID, profileName)
	}
	return nil
}

func runDaemonForeground(ctx context.Context, dir, socket string) error {
	exe, err := servicelaunch.Sibling()
	if err != nil {
		return err
	}
	if err := servicelaunch.CheckExecutable(exe); err != nil {
		return err
	}
	raw := api.NewClient(socket)
	probe, cancel := context.WithTimeout(ctx, probeTimeout)
	inst, probeErr := raw.Service(probe)
	cancel()
	if probeErr == nil {
		if inst.Directory != dir {
			return errors.New("socket belongs to a different state directory")
		}
		return fmt.Errorf("daemon is already running (mode %s, pid %d)", inst.Mode, inst.PID)
	}
	cmd := exec.CommandContext(ctx, exe)
	cmd.Env = daemonEnv(dir, socket)
	cmd.Stdin = nil
	cmd.Stdout = os.Stdout
	cmd.Stderr = os.Stderr
	if err := cmd.Start(); err != nil {
		return err
	}
	var waitErr error
	done := make(chan struct{})
	go func() {
		waitErr = cmd.Wait()
		close(done)
	}()
	select {
	case <-done:
		return waitErr
	case <-ctx.Done():
		_ = cmd.Process.Signal(syscall.SIGTERM)
		select {
		case <-done:
		case <-time.After(gracefulBudget):
			_ = cmd.Process.Kill()
			<-done
		}
		return nil
	}
}

func daemonEnv(dir, socket string) []string {
	var env []string
	for _, entry := range os.Environ() {
		if strings.HasPrefix(entry, "UNIC_HOME=") || strings.HasPrefix(entry, "UNIC_SOCK=") || strings.HasPrefix(entry, "UNIC_PROFILE=") {
			continue
		}
		env = append(env, entry)
	}
	return append(env, "UNIC_HOME="+dir, "UNIC_SOCK="+socket)
}

func startStandaloneDaemon(ctx context.Context, dir, socket string) (servicelifecycle.Instance, bool, error) {
	raw := api.NewClient(socket)
	acquireCtx, cancelAcquire := context.WithTimeout(ctx, acquireBudget)
	defer cancelAcquire()

	for {
		unlock, err := servicelaunch.LockContext(acquireCtx, dir)
		if err != nil {
			return servicelifecycle.Instance{}, false, err
		}
		probe, cancelProbe := context.WithTimeout(acquireCtx, probeTimeout)
		inst, err := raw.Service(probe)
		cancelProbe()
		switch {
		case err == nil:
			if inst.Directory != dir {
				unlock()
				return servicelifecycle.Instance{}, false, errors.New("socket belongs to a different state directory")
			}
			if inst.Retiring {
				unlock()
				if err := waitServiceGone(acquireCtx, raw, inst.ID, nil); err != nil {
					return servicelifecycle.Instance{}, false, fmt.Errorf("wait for retiring unicd: %w", err)
				}
				continue
			}
			if inst.Mode == servicelifecycle.ModeCLITemporary {
				unlock()
				// Wait briefly for an active CLI-temporary command to finish rather than colliding.
				if err := waitServiceGone(acquireCtx, raw, inst.ID, nil); err != nil {
					return servicelifecycle.Instance{}, false, errors.New("a temporary CLI service is currently active; retry in a moment")
				}
				continue
			}
			unlock()
			return inst, false, nil

		case errors.Is(err, api.ErrNoDaemon):
			inst, startErr := spawnStandalone(acquireCtx, raw, dir, socket)
			unlock()
			return inst, startErr == nil, startErr

		case closedByPeer(err):
			unlock()
			select {
			case <-acquireCtx.Done():
				return servicelifecycle.Instance{}, false, fmt.Errorf("wait for previous unicd shutdown: %w", acquireCtx.Err())
			case <-time.After(50 * time.Millisecond):
			}
			continue

		default:
			unlock()
			return servicelifecycle.Instance{}, false, fmt.Errorf("inspect existing unicd: %w", err)
		}
	}
}

func spawnStandalone(ctx context.Context, raw *api.Client, dir, socket string) (servicelifecycle.Instance, error) {
	exe, err := servicelaunch.Sibling()
	if err != nil {
		return servicelifecycle.Instance{}, err
	}
	if err := servicelaunch.CheckExecutable(exe); err != nil {
		return servicelifecycle.Instance{}, err
	}
	startCtx, cancel := context.WithTimeout(ctx, startBudget)
	defer cancel()
	logPath := filepath.Join(dir, "daemon.log")

	for {
		logFile, err := os.OpenFile(logPath, os.O_CREATE|os.O_WRONLY|os.O_APPEND, 0o600)
		if err != nil {
			return servicelifecycle.Instance{}, err
		}
		cmd := exec.Command(exe)
		servicelaunch.ConfigureProcess(cmd)
		cmd.Env = daemonEnv(dir, socket)
		cmd.Stdin = nil
		cmd.Stdout = logFile
		cmd.Stderr = logFile
		err = cmd.Start()
		_ = logFile.Close()
		if err != nil {
			return servicelifecycle.Instance{}, err
		}
		var childWaitErr error
		done := make(chan struct{})
		go func() {
			childWaitErr = cmd.Wait()
			close(done)
		}()

		var inst servicelifecycle.Instance
		waitErr := servicelaunch.Wait(startCtx, func() (bool, error) {
			select {
			case <-done:
				if childWaitErr != nil {
					return false, childWaitErr
				}
				return false, errors.New("unicd exited before readiness; see daemon.log")
			default:
			}
			probe, cancelProbe := context.WithTimeout(startCtx, probeTimeout)
			defer cancelProbe()
			v, perr := raw.Service(probe)
			if perr != nil {
				return false, nil
			}
			inst = v
			return v.PID == cmd.Process.Pid && v.Directory == dir && v.Mode == servicelifecycle.ModeStandalone, nil
		})
		if waitErr == nil {
			return inst, nil
		}
		var exit *exec.ExitError
		if errors.As(waitErr, &exit) && exit.ExitCode() == servicelifecycle.ExitDirectoryInUse {
			timer := time.NewTimer(100 * time.Millisecond)
			select {
			case <-startCtx.Done():
				timer.Stop()
				return servicelifecycle.Instance{}, fmt.Errorf("wait for state directory: %w", startCtx.Err())
			case <-timer.C:
			}
			continue
		}
		if errors.As(waitErr, &exit) {
			return servicelifecycle.Instance{}, fmt.Errorf("unicd exited before readiness: %w; see %s", waitErr, logPath)
		}
		// Readiness timed out or cancelled: stop the child we just started.
		_ = cmd.Process.Signal(syscall.SIGTERM)
		select {
		case <-done:
		case <-time.After(3 * time.Second):
			_ = cmd.Process.Kill()
		}
		return servicelifecycle.Instance{}, fmt.Errorf("wait for unicd readiness: %w; see %s", waitErr, logPath)
	}
}

func runDaemonStop(ctx context.Context, args []string, asJSONDefault bool) error {
	fs := flag.NewFlagSet("daemon stop", flag.ContinueOnError)
	fs.SetOutput(io.Discard)
	asJSON := fs.Bool("json", asJSONDefault, "output JSON")
	pos, err := parse(fs, args)
	if err != nil || len(pos) != 0 {
		return usageError("usage: unic daemon stop [--json]")
	}
	dir, err := paths.CanonicalDir()
	if err != nil {
		return err
	}
	socket, err := paths.ResolveSocket()
	if err != nil {
		return err
	}
	profileName, _ := paths.ActiveProfile()
	inst, stopped, err := stopStandaloneDaemon(ctx, dir, socket)
	if err != nil {
		return err
	}
	if *asJSON {
		out := map[string]any{
			"running":   false,
			"stopped":   stopped,
			"profile":   profileName,
			"directory": dir,
		}
		if stopped {
			out["previous_pid"] = inst.PID
		}
		return json.NewEncoder(os.Stdout).Encode(out)
	}
	if stopped {
		fmt.Printf("%s daemon stopped (pid %d, profile %s)\n", style(outTTY, sOK, "✓"), inst.PID, profileName)
	} else {
		fmt.Printf("daemon is not running (profile %s)\n", profileName)
	}
	return nil
}

func stopStandaloneDaemon(ctx context.Context, dir, socket string) (servicelifecycle.Instance, bool, error) {
	raw := api.NewClient(socket)
	probe, cancelProbe := context.WithTimeout(ctx, probeTimeout)
	inst, err := raw.Service(probe)
	cancelProbe()
	if errors.Is(err, api.ErrNoDaemon) {
		return servicelifecycle.Instance{}, false, nil
	}
	if err != nil {
		return servicelifecycle.Instance{}, false, fmt.Errorf("inspect existing unicd: %w", err)
	}
	if inst.Directory != dir {
		return servicelifecycle.Instance{}, false, errors.New("socket belongs to a different state directory")
	}
	switch inst.Mode {
	case servicelifecycle.ModeDesktop:
		return inst, false, errors.New("daemon is managed by unic-desktop; quit the Desktop app to stop it")
	case servicelifecycle.ModeCLITemporary:
		return inst, false, errors.New("daemon is a temporary CLI session; wait for the active unic command to finish")
	}
	stopCtx, cancelStop := context.WithTimeout(ctx, 5*time.Second)
	err = raw.StopService(stopCtx, inst.ID)
	cancelStop()
	if err != nil && !errors.Is(err, api.ErrNoDaemon) && !closedByPeer(err) {
		return inst, false, fmt.Errorf("stop unicd: %w", err)
	}
	waitCtx, cancelWait := context.WithTimeout(ctx, gracefulBudget)
	defer cancelWait()
	if err := waitServiceGone(waitCtx, raw, inst.ID, nil); err != nil {
		return inst, false, fmt.Errorf("wait for unicd to stop: %w", err)
	}
	return inst, true, nil
}

func runDaemonRestart(ctx context.Context, args []string, asJSONDefault bool) error {
	fs := flag.NewFlagSet("daemon restart", flag.ContinueOnError)
	fs.SetOutput(io.Discard)
	asJSON := fs.Bool("json", asJSONDefault, "output JSON")
	pos, err := parse(fs, args)
	if err != nil || len(pos) != 0 {
		return usageError("usage: unic daemon restart [--json]")
	}
	dir, err := paths.Dir()
	if err != nil {
		return err
	}
	if err := climigration.Required(dir); err != nil {
		return err
	}
	socket, err := paths.Socket()
	if err != nil {
		return err
	}
	prev, wasRunning, err := stopStandaloneDaemon(ctx, dir, socket)
	if err != nil {
		return err
	}
	inst, _, err := startStandaloneDaemon(ctx, dir, socket)
	if err != nil {
		return err
	}
	profileName, _ := paths.ActiveProfile()
	if *asJSON {
		out := map[string]any{
			"running":   true,
			"restarted": true,
			"profile":   profileName,
			"directory": dir,
			"socket":    socket,
			"instance":  inst,
		}
		if wasRunning {
			out["previous_pid"] = prev.PID
		}
		return json.NewEncoder(os.Stdout).Encode(out)
	}
	fmt.Printf("%s daemon restarted (pid %d, profile %s)\n", style(outTTY, sOK, "✓"), inst.PID, profileName)
	return nil
}

func runDaemonStatus(ctx context.Context, args []string, asJSONDefault bool) error {
	fs := flag.NewFlagSet("daemon status", flag.ContinueOnError)
	fs.SetOutput(io.Discard)
	asJSON := fs.Bool("json", asJSONDefault, "output JSON")
	pos, err := parse(fs, args)
	if err != nil || len(pos) != 0 {
		return usageError("usage: unic daemon status [--json]")
	}
	dir, err := paths.CanonicalDir()
	if err != nil {
		return err
	}
	socket, err := paths.ResolveSocket()
	if err != nil {
		return err
	}
	profileName, _ := paths.ActiveProfile()
	raw := api.NewClient(socket)
	probe, cancel := context.WithTimeout(ctx, probeTimeout)
	inst, err := raw.Service(probe)
	cancel()

	running := false
	var caps *capabilitiesSummary
	switch {
	case err == nil:
		if inst.Directory != dir {
			return errors.New("socket belongs to a different state directory")
		}
		running = true
		cctx, ccancel := context.WithTimeout(ctx, probeTimeout)
		if c, cerr := raw.Capabilities(cctx); cerr == nil {
			v := capabilitiesView(c)
			caps = &v
		}
		ccancel()
	case errors.Is(err, api.ErrNoDaemon):
		running = false
	default:
		return fmt.Errorf("inspect existing unicd: %w", err)
	}

	if *asJSON {
		payload := map[string]any{
			"profile":   profileName,
			"directory": dir,
			"socket":    socket,
			"running":   running,
		}
		if running {
			payload["instance"] = inst
		}
		if caps != nil {
			payload["clipboard"] = caps
		}
		return json.NewEncoder(os.Stdout).Encode(payload)
	}

	fmt.Printf("profile    %s\n", profileName)
	fmt.Printf("directory  %s\n", dir)
	fmt.Printf("socket     %s\n", socket)
	if !running {
		fmt.Printf("status     stopped\n")
		note("run `unic daemon start` to start the background service")
		return nil
	}
	uptime := time.Since(time.UnixMilli(inst.StartedAtMs)).Round(time.Second)
	fmt.Printf("status     running (%s, pid %d, up %s)\n", inst.Mode, inst.PID, uptime)
	fmt.Printf("leases     %d\n", inst.Leases)
	if caps != nil {
		fmt.Printf("clipboard  %s\n", caps.Summary)
	}
	return nil
}

func runDaemonLogs(ctx context.Context, args []string) error {
	fs := flag.NewFlagSet("daemon logs", flag.ContinueOnError)
	fs.SetOutput(io.Discard)
	lines := fs.Int("n", 50, "number of lines")
	fs.IntVar(lines, "lines", 50, "number of lines")
	follow := fs.Bool("f", false, "follow log output")
	fs.BoolVar(follow, "follow", false, "follow log output")
	pos, err := parse(fs, args)
	if err != nil || len(pos) != 0 {
		return usageError("usage: unic daemon logs [-n LINES] [-f|--follow]")
	}
	if *lines < 1 {
		return usageError("-n/--lines must be at least 1")
	}
	dir, err := paths.CanonicalDir()
	if err != nil {
		return err
	}
	logPath := filepath.Join(dir, "daemon.log")
	if _, err := os.Stat(logPath); errors.Is(err, os.ErrNotExist) {
		alt := filepath.Join(dir, "cli-daemon.log")
		if _, err2 := os.Stat(alt); err2 == nil {
			logPath = alt
		} else {
			return fmt.Errorf("no daemon log found at %s (start with `unic daemon start`)", logPath)
		}
	}
	f, err := os.Open(logPath)
	if err != nil {
		return err
	}
	defer func() { _ = f.Close() }()

	if err := printLastLines(f, os.Stdout, *lines); err != nil {
		return err
	}
	if !*follow {
		return nil
	}
	reader := bufio.NewReader(f)
	for {
		line, err := reader.ReadString('\n')
		if len(line) > 0 {
			_, _ = io.WriteString(os.Stdout, line)
		}
		if err == nil {
			continue
		}
		if !errors.Is(err, io.EOF) {
			return err
		}
		select {
		case <-ctx.Done():
			return nil
		case <-time.After(250 * time.Millisecond):
		}
	}
}

func printLastLines(r io.Reader, w io.Writer, n int) error {
	sc := bufio.NewScanner(r)
	sc.Buffer(make([]byte, 64*1024), 1024*1024)
	ring := make([]string, 0, n)
	for sc.Scan() {
		if len(ring) < n {
			ring = append(ring, sc.Text())
		} else {
			copy(ring, ring[1:])
			ring[n-1] = sc.Text()
		}
	}
	if err := sc.Err(); err != nil {
		return err
	}
	for _, line := range ring {
		if _, err := fmt.Fprintln(w, line); err != nil {
			return err
		}
	}
	return nil
}

func waitServiceGone(ctx context.Context, raw *api.Client, id string, done <-chan struct{}) error {
	return servicelaunch.Wait(ctx, func() (bool, error) {
		if done != nil {
			select {
			case <-done:
				return true, nil
			default:
			}
		}
		probe, cancel := context.WithTimeout(ctx, probeTimeout)
		defer cancel()
		v, err := raw.Service(probe)
		if errors.Is(err, api.ErrNoDaemon) || (err == nil && v.ID != id) {
			return true, nil
		}
		return false, nil
	})
}

func formatDurationShort(d time.Duration) string {
	if d < time.Minute {
		return strconv.Itoa(int(d.Seconds())) + "s"
	}
	return d.Round(time.Second).String()
}
