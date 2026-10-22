package main

import (
	"context"
	"fmt"
	"log"
	"net/http"
	"os"
	"os/exec"
	"strconv"
	"sync/atomic"
	"time"

	"github.com/UniClipboard/UniClipboard/packages/desktop-host-go/daemonclient"
	"github.com/UniClipboard/UniClipboard/packages/desktop-host-go/daemonlife"
	"github.com/UniClipboard/UniClipboard/packages/desktop-host-go/daemonproc"
)

const daemonStopTimeout = 10 * time.Second

// exitIntent records what happens to the daemon when the GUI process exits.
// A plain quit (tray Quit, Cmd-Q) stops the daemon. Restart keeps it running.
// Lightweight mode stops only the UI child on every platform.
type exitIntent struct{ keepDaemon atomic.Bool }

func (h *HostService) quit(keepDaemon bool) {
	if uiChild && !keepDaemon {
		go func() {
			ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
			defer cancel()
			_ = h.ownerCall(ctx, "quit-app", nil, nil)
		}()
		return
	}
	h.exit.keepDaemon.Store(keepDaemon)
	h.quitting.Store(true)
	h.app.Quit()
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
	if h.stopTray != nil {
		h.stopTray()
	}
	if h.helper != nil {
		h.helper.Shutdown()
	}
	h.modifierMonitor().Shutdown() // release the keyboard poll before the process exits
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

// stopDaemonOnExit terminates the connected daemon and waits for it to exit.
func (h *HostService) stopDaemonOnExit() {
	if h.exit.keepDaemon.Load() {
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
	deadline := time.Now().Add(daemonStopTimeout)
	for time.Now().Before(deadline) {
		if outcome, err := daemonlife.Probe(); err == nil && outcome.Kind == daemonlife.Absent {
			return nil
		}
		time.Sleep(200 * time.Millisecond)
	}
	return fmt.Errorf("the daemon (pid %d) was still answering %s after the stop", pid, daemonStopTimeout)
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
	exe, err := restartExecutable()
	if err != nil {
		return err
	}
	cmd := exec.Command(exe)
	cmd.Env = append(os.Environ(), restartParentEnv+"="+strconv.Itoa(os.Getpid()))
	if err := cmd.Start(); err != nil {
		return err
	}
	go func() { _ = cmd.Wait() }()
	h.quit(true)
	return nil
}

// restartDaemon replaces the daemon process; the frontend reconnects by itself.
func restartDaemon() error {
	if err := stopDaemon(); err != nil {
		return err
	}
	if err := daemonproc.SpawnDetachedDaemon("gui"); err != nil {
		return err
	}
	return daemonlife.WaitHealthy(daemonlife.StartupTimeout, "")
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
	outcome, err := daemonlife.Probe()
	if err != nil {
		log.Printf("after the daemon stop the daemon state is unknown: %v", err)
		return
	}
	if outcome.Kind == daemonlife.Absent && startAbsent {
		if err := daemonproc.SpawnDetachedDaemon("gui"); err != nil {
			log.Printf("failed to start the daemon after the stop: %v", err)
			return
		}
		if err := daemonlife.WaitHealthy(daemonlife.StartupTimeout, ""); err != nil {
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
