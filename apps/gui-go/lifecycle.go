package main

import (
	"context"
	"fmt"
	"log"
	"net/http"
	"os"
	"os/exec"
	"strconv"
	"time"

	"github.com/UniClipboard/UniClipboard/packages/desktop-host-go/daemonclient"
	"github.com/UniClipboard/UniClipboard/packages/desktop-host-go/daemonlife"
	"github.com/UniClipboard/UniClipboard/packages/desktop-host-go/daemonproc"
)

const daemonStopTimeout = 10 * time.Second

func (h *HostService) quit(keepDaemon bool) {
	if uiChild && !keepDaemon {
		go func() {
			ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
			defer cancel()
			_ = h.ownerCall(ctx, "quit-app", nil, nil)
		}()
		return
	}
	if h.beginQuit(keepDaemon) {
		h.app.Quit()
	}
}

// beginQuit makes shutdown idempotent: a later child exit cannot replace a
// full-quit decision with keep-daemon, or overwrite a successful restart.
func (h *HostService) beginQuit(keepDaemon bool) bool {
	return h.exitIntent.begin(keepDaemon)
}

// shutdown stops background work, then the daemon unless the exit keeps it.
func (h *HostService) shutdown() {
	if uiChild {
		h.persistMainWindowSize()
		if h.owner != nil {
			h.owner.close()
		}
		return
	}
	if h.ui != nil {
		h.ui.stop()
	}
	if h.stopDesktopTheme != nil {
		h.stopDesktopTheme()
	}

	h.settingsWatch.Pause() // close its daemon connection before the daemon may be stopped below
	if h.stopSettings != nil {
		h.stopSettings()
	}
	if h.stopWake != nil {
		h.stopWake() // unsubscribe before the scheduler stops so no wake targets a finished loop
	}
	if h.stopActivity != nil {
		h.stopActivity() // likewise: stop the system callback before the loop it feeds
	}
	if h.stopScheduler != nil {
		h.stopScheduler()
	}
	h.updates.close()
	if h.stopTray != nil {
		h.stopTray()
	}
	if h.helper != nil {
		h.helper.Shutdown()
	}
	if h.modifier != nil {
		h.modifier.Shutdown()
	}
	h.stopDaemonOnExit()
}

// pageDisconnectGrace is how long the page gets to close its daemon WebSocket after `app://shutting-down`.
const pageDisconnectGrace = 300 * time.Millisecond

// announceDaemonStop tells the pages to close their daemon WebSocket before the daemon stops. A WebView that is
// destroyed sends no close frame, so the daemon would otherwise wait for its heartbeat timeout before it exits.
func (h *HostService) announceDaemonStop() {
	h.persistMainWindowSize()
	h.settingsWatch.Pause() // the host's own connection would hold the daemon the same way; RestartDaemon resumes it
	h.emit(appShuttingDownEvent, nil)
	time.Sleep(pageDisconnectGrace)
}

// stopDaemonOnExit requests an orderly stop for a starting daemon or stops the connected daemon.
func (h *HostService) stopDaemonOnExit() {
	if h.keepDaemon.Load() {
		return
	}
	if h.daemon() == nil {
		if attempt := h.startupAttempt.Load(); attempt != nil && !attempt.Exited() && daemonproc.IsActiveDaemon(attempt.PID()) {
			ctx, cancel := context.WithTimeout(context.Background(), daemonStopTimeout)
			defer cancel()
			if err := daemonproc.StopStartingDaemon(ctx, attempt); err != nil {
				log.Printf("failed to stop starting daemon on exit: %v", err)
			}
		}
		return
	}
	h.announceDaemonStop()
	if err := stopDaemon(); err != nil {
		log.Printf("failed to stop the daemon on exit: %v", err)
	}
}

// stopDaemon terminates the running daemon and waits until it is gone. It mirrors `stop_daemon_before_update`: a
// missing, stale or in-process (owned by another GUI shell) daemon is nothing to stop, and a daemon that survives is
// an error the caller decides on (the Windows update install must abort, because the live daemon image blocks the
// installer).
func stopDaemon() error {
	pid, ok, err := runningDaemonPID()
	if err != nil || !ok {
		return err
	}
	if err := daemonproc.StopDaemon(pid, requestDaemonGracefulStop, daemonStopTimeout); err != nil {
		return err
	}
	if err := daemonlife.WaitAbsent(daemonStopTimeout); err != nil {
		return fmt.Errorf("the daemon (pid %d) was still present %s after the stop: %w", pid, daemonStopTimeout, err)
	}
	return nil
}

// requestDaemonGracefulStop asks the daemon to shut down in an orderly way. The route marks the run marker clean
// before it notifies the shutdown task, so the next start does not report an abnormal exit.
func requestDaemonGracefulStop(ctx context.Context) error {
	client, err := daemonclient.FromEnv()
	if err != nil {
		return err
	}
	resp, err := client.Send(ctx, daemonclient.Request{Method: http.MethodPost, Path: "/lifecycle/graceful-stop"})
	if err != nil {
		return err
	}
	defer resp.Body.Close()
	if resp.StatusCode != http.StatusAccepted {
		return fmt.Errorf("graceful stop: unexpected status %d", resp.StatusCode)
	}
	return nil
}

// runningDaemonPID returns the pid of the live, GUI-owned daemon. The pid file carries the run mode; the connection
// file is the fallback for a daemon that predates it.
func runningDaemonPID() (uint32, bool, error) {
	meta, err := daemonproc.ReadPidMetadata()
	if err != nil {
		return 0, false, err
	}
	if meta != nil {
		if meta.Mode == "in_process" || !daemonproc.IsActiveDaemon(meta.PID) {
			return 0, false, nil
		}
		return meta.PID, true, nil
	}
	conn, err := daemonproc.ReadConnFile()
	if err != nil || conn == nil || !daemonproc.IsActiveDaemon(conn.PID) {
		return 0, false, err
	}
	return conn.PID, true, nil
}

// restartGUI starts a fresh copy of this executable and exits, keeping the daemon.
func (h *HostService) restartGUI() error {
	if uiChild {
		ctx, cancel := context.WithTimeout(context.Background(), 15*time.Second)
		defer cancel()
		return h.ownerCall(ctx, "restart-gui", nil, nil)
	}
	return h.restartShell(false)
}

const backgroundRestartArg = "--background-restart"
const residentRestartArg = "--resident-restart"

func launchAfterHostExit(background, resident bool) error {
	exe, err := restartExecutable()
	if err != nil {
		return err
	}
	args := []string{}
	if resident {
		args = append(args, residentRestartArg)
	}
	if background {
		args = append(args, backgroundRestartArg)
	}
	cmd := exec.Command(exe, args...)
	cmd.Env = append(os.Environ(), restartParentEnv+"="+strconv.Itoa(os.Getpid()))
	if err := cmd.Start(); err != nil {
		return err
	}
	go func() { _ = cmd.Wait() }()
	return nil
}

func (h *HostService) restartShell(background bool) error {
	// Keep child activation and the restart commitment under the same lock.
	// A later open observes quitting and forwards itself to the replacement.
	if h.ui != nil {
		h.ui.mu.Lock()
		if background {
			state := h.ui.stateLocked()
			_, unavailable := state["stateError"]
			background = !unavailable && state["mainExists"] != true && h.ui.pendingOpen.Load() == 0
		}
	}
	// Serialize a bootstrap handoff with an explicit activation held for readiness.
	// Once the exit is committed, later activations are forwarded instead.
	h.readyMu.Lock()
	started, err := h.exitIntent.restart(func() error {
		return launchAfterHostExit(background && !h.showWhenReady, !h.lightweight.Load())
	})
	h.readyMu.Unlock()
	if h.ui != nil {
		h.ui.mu.Unlock()
	}
	if started {
		h.app.Quit()
	}
	return err
}

// forwardActivation carries an explicit launch across an idle exit or mode
// restart. The new process waits for this host, then uses the same instance lock.
func (h *HostService) forwardActivation() {
	if !h.keepDaemon.Load() || !h.activationForwarded.CompareAndSwap(false, true) {
		return
	}
	if err := launchAfterHostExit(false, !h.lightweight.Load()); err != nil {
		h.activationForwarded.Store(false)
		log.Printf("forward activation after host exit failed: %v", err)
	}
}

// restartDaemon replaces the daemon process; the frontend reconnects by itself.
func restartDaemon() error {
	if err := stopDaemon(); err != nil {
		return err
	}
	return daemonlife.SpawnAndWait("gui", daemonlife.StartupTimeout, "")
}

// fullRestart replaces the daemon first, then the GUI. A failed daemon restart
// does not stop the GUI restart: the new GUI's bootstrap retries recovery.
func (h *HostService) fullRestart() {
	if uiChild {
		ctx, cancel := context.WithTimeout(context.Background(), 15*time.Second)
		defer cancel()
		if err := h.ownerCall(ctx, "restart-app", nil, nil); err != nil {
			log.Printf("app restart request failed: %v", err)
		}
		return
	}
	h.announceDaemonStop() // the windows close their sockets and the watcher pauses, so no connection holds the stop
	if err := restartDaemon(); err != nil {
		log.Printf("daemon restart failed, restarting the GUI anyway: %v", err)
	}
	if err := h.restartGUI(); err != nil {
		h.resumeAfterStop(false)
		h.app.Dialog.Error().SetTitle("UniClipboard").SetMessage(err.Error()).Show()
	}
}

// resumeAfterStop ends a host-initiated daemon stop after which this process keeps running: a daemon restart, a
// failed GUI relaunch, a failed update install. announceDaemonStop made the windows close their daemon sockets and
// paused the watcher. Start a daemon when startAbsent is set and none runs; when a compatible daemon answers, store
// its client and tell the windows to reconnect. The watcher resumes in every case and retries against whatever runs.
func (h *HostService) resumeAfterStop(startAbsent bool) {
	defer h.settingsWatch.Resume()
	outcome, err := daemonlife.ProbeForReuse(daemonlife.StartupTimeout)
	if err != nil {
		log.Printf("after the daemon stop the daemon state is unknown: %v", err)
		return
	}
	if outcome.Kind == daemonlife.Absent && startAbsent {
		if err := daemonlife.SpawnAndWait("gui", daemonlife.StartupTimeout, ""); err != nil {
			log.Printf("the daemon did not become healthy after the stop: %v", err)
			return
		}
	} else if outcome.Kind != daemonlife.Compatible {
		log.Printf("no compatible daemon after the stop (%v); the windows stay disconnected", outcome.Kind)
		return
	}
	// A new process has its own connection file and token: re-read them, then tell the windows to reconnect.
	client, err := daemonclient.FromEnv()
	if err != nil {
		log.Printf("failed to reconnect after the daemon stop: %v", err)
		return
	}
	h.client.Store(client)
	h.emit(daemonConnectionChanged, nil)
}
