package main

import (
	"os"
	"os/exec"
	"sync/atomic"
	"time"

	"github.com/UniClipboard/UniClipboard/packages/desktop-host-go/daemonlife"
	"github.com/UniClipboard/UniClipboard/packages/desktop-host-go/daemonproc"
)

const daemonStopTimeout = 10 * time.Second

// exitIntent records what happens to the daemon when the GUI process exits.
// A plain quit (tray Quit, Cmd-Q) stops the daemon; lightweight mode and
// restart leave it running for the next GUI.
type exitIntent struct{ keepDaemon atomic.Bool }

func (h *HostService) quit(keepDaemon bool) {
	h.exit.keepDaemon.Store(keepDaemon)
	h.quitting.Store(true)
	h.app.Quit()
}

// shutdown stops background work, then the daemon unless the exit keeps it.
func (h *HostService) shutdown() {
	if h.stopWake != nil {
		h.stopWake() // unsubscribe before the scheduler stops so no wake targets a finished loop
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
	h.stopDaemonOnExit()
}

// stopDaemonOnExit terminates the connected daemon and waits for it to exit.
func (h *HostService) stopDaemonOnExit() {
	if h.exit.keepDaemon.Load() {
		return
	}
	stopDaemon()
}

func stopDaemon() {
	conn, err := daemonproc.ReadConnFile()
	if err != nil || conn == nil {
		return
	}
	if !daemonproc.Terminate(conn.PID) {
		return
	}
	deadline := time.Now().Add(daemonStopTimeout)
	for time.Now().Before(deadline) {
		if outcome, err := daemonlife.Probe(); err == nil && outcome.Kind == daemonlife.Absent {
			return
		}
		time.Sleep(200 * time.Millisecond)
	}
}

// restartGUI starts a fresh copy of this executable and exits, keeping the daemon.
func (h *HostService) restartGUI() error {
	exe, err := os.Executable()
	if err != nil {
		return err
	}
	cmd := exec.Command(exe)
	cmd.Env = os.Environ()
	if err := cmd.Start(); err != nil {
		return err
	}
	go func() { _ = cmd.Wait() }()
	h.quit(true)
	return nil
}

// restartDaemon replaces the daemon process; the frontend reconnects by itself.
func restartDaemon() error {
	stopDaemon()
	if err := daemonproc.SpawnDetachedDaemon("gui"); err != nil {
		return err
	}
	return daemonlife.WaitHealthy(daemonlife.StartupTimeout, "")
}

// fullRestart replaces the daemon first, then the GUI. A failed daemon restart
// does not stop the GUI restart: the new GUI's bootstrap retries recovery.
func (h *HostService) fullRestart() {
	_ = restartDaemon()
	if err := h.restartGUI(); err != nil {
		h.app.Dialog.Error().SetTitle("UniClipboard").SetMessage(err.Error()).Show()
	}
}
