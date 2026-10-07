//go:build linux

package main

import (
	"context"
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"runtime"
	"slices"
	"sync"
	"syscall"
	"time"

	"github.com/godbus/dbus/v5"
)

// pacRunnerName is the session-bus name glib-networking's GNOME proxy resolver calls to evaluate a proxy auto-configuration
// (PAC) script (proxy/gnome/gproxyresolvergnome.c). Without a service behind it the resolver logs "Proxy autoconfiguration will
// not work" and every lookup fails (the 17c12 stage-4 `pac-nohelper-*` runs). The resolver addresses the well-known name, not a
// unique connection, so a restarted helper is picked up without any reconnect.
const pacRunnerName = "org.gtk.GLib.PACRunner"

// pacBusAddress is the bus address the supervisor dials; empty means the user's session bus. Only the E2E build sets it
// (pacrunner_e2e_linux.go), so an end-to-end run can make the supervisor's own bus unusable (silent, missing) while the rest
// of the application keeps its normal bus. The application itself cannot start on such a bus, which is why the supervisor's
// budget cannot be isolated any other way.
var pacBusAddress = func() string { return "" }

func dialPacBus(ctx context.Context) (*dbus.Conn, error) {
	addr := pacBusAddress()
	if addr == "" {
		return dbus.ConnectSessionBus(dbus.WithContext(ctx))
	}
	conn, err := dbus.Dial(addr, dbus.WithContext(ctx))
	if err != nil {
		return nil, err
	}
	if err := conn.Auth(nil); err != nil {
		conn.Close()
		return nil, err
	}
	if err := conn.Hello(); err != nil {
		conn.Close()
		return nil, err
	}
	return conn, nil
}

const (
	// pacBudget bounds every step of the start path (bus connection, each bus call, the wait for the helper to own the name) and
	// is also how long init waits in total: a stuck bus can delay the launch by at most this long.
	pacBudget = 3 * time.Second
	// pacRestartLimit bounds how often a vanished helper is restarted inside pacRestartWindow before the supervisor gives up.
	pacRestartLimit  = 5
	pacRestartWindow = time.Minute
)

// init makes the AppImage's own glib-pacrunner available, but only when nothing else can serve the name, and never in place of
// something that can:
//   - a bus that can activate the name (a host service file) is asked to start it once; if that works the host's helper serves and
//     nothing of ours runs, if the service is listed but cannot start (missing program, crash) the bundled helper takes over;
//   - a name that already has an owner (a running host helper, another instance's helper) is left alone, nothing is started
//     and nothing is replaced (no replacement flags, the helper owns the name with the default flags);
//   - with no session bus nothing is started and PAC stays unavailable, which the resolver reports as a failed request (never a
//     direct connection): this is not self-contained PAC support, GNOME's PAC path needs the session bus.
//
// A supervisor goroutine keeps watching the name (NameOwnerChanged): when the owner disappears and nothing can activate it, the
// bundled helper is (re)started, which is also how a killed helper or an exited first instance is recovered. init does not
// wait for the bus beyond pacBudget; it runs before the WebView exists because the resolver contacts the helper the first time a
// PAC configuration is read.
func init() {
	appDir := os.Getenv("APPDIR")
	if appDir == "" {
		return
	}
	helper := filepath.Join(appDir, "usr", "libexec", "glib-pacrunner")
	if _, err := os.Stat(helper); err != nil {
		return
	}
	settled := make(chan struct{})
	go superviseBundledPacRunner(helper, settled)
	select {
	case <-settled:
	case <-time.After(pacBudget):
		fmt.Fprintln(os.Stderr, "pacrunner: the session bus did not settle within", pacBudget, "- continuing without waiting")
	}
}

func superviseBundledPacRunner(helper string, settled chan<- struct{}) {
	// Pdeathsig is delivered when the THREAD that created the child exits, not when the process does. Every child is created by this
	// goroutine, which keeps its OS thread for its whole life (never unlocked, never returning while the process runs).
	runtime.LockOSThread()
	started := time.Now()
	fmt.Fprintln(os.Stderr, "pacrunner: supervisor started at", started.Format(time.RFC3339Nano))
	var once sync.Once
	settle := func() { once.Do(func() { close(settled) }) }
	defer settle()

	// godbus ties the connection's lifetime to the context given to the dial (cancelling it closes the connection), so the dial gets a
	// context that lives as long as the supervisor and the budget is enforced around it: past pacBudget the dial is cancelled.
	connCtx, closeConn := context.WithCancel(context.Background())
	defer closeConn()
	type dialResult struct {
		conn *dbus.Conn
		err  error
	}
	dialed := make(chan dialResult, 1)
	go func() {
		c, err := dialPacBus(connCtx)
		dialed <- dialResult{c, err}
	}()
	var conn *dbus.Conn
	var err error
	select {
	case d := <-dialed:
		conn, err = d.conn, d.err
	case <-time.After(pacBudget):
		err = fmt.Errorf("the session bus did not answer within %v", pacBudget)
	}
	if err != nil {
		fmt.Fprintln(os.Stderr, "pacrunner: no session bus, PAC configurations cannot be evaluated:", err, "(supervisor gave up after", time.Since(started).Round(time.Millisecond), "at", time.Now().Format(time.RFC3339Nano)+")")
		return
	}
	defer conn.Close()

	var activatable []string
	ctx, cancel := context.WithTimeout(context.Background(), pacBudget)
	err = conn.BusObject().CallWithContext(ctx, "org.freedesktop.DBus.ListActivatableNames", 0).Store(&activatable)
	cancel()
	hostService := err == nil && slices.Contains(activatable, pacRunnerName)

	signals := make(chan *dbus.Signal, 16)
	conn.Signal(signals)
	if err := conn.AddMatchSignal(dbus.WithMatchSender("org.freedesktop.DBus"), dbus.WithMatchInterface("org.freedesktop.DBus"),
		dbus.WithMatchMember("NameOwnerChanged"), dbus.WithMatchArg(0, pacRunnerName)); err != nil {
		fmt.Fprintln(os.Stderr, "pacrunner: cannot watch the name owner:", err)
	}

	exited := make(chan struct{}, 1)
	var starts []time.Time
	owned := func() bool {
		ctx, cancel := context.WithTimeout(context.Background(), pacBudget)
		defer cancel()
		var has bool
		return conn.BusObject().CallWithContext(ctx, "org.freedesktop.DBus.NameHasOwner", 0, pacRunnerName).Store(&has) == nil && has
	}
	// startHostService asks the bus to activate the host's service now (what the resolver would do on first use) and reports whether it
	// came up: a service FILE that is listed but whose program is missing or crashes is not a service (the stage-5 fixture that only
	// renamed the binary), so activation failing hands over to the bundled helper instead of ruling PAC out.
	startHostService := func() bool {
		ctx, cancel := context.WithTimeout(context.Background(), pacBudget)
		defer cancel()
		var reply uint32
		err := conn.BusObject().CallWithContext(ctx, "org.freedesktop.DBus.StartServiceByName", 0, pacRunnerName, uint32(0)).Store(&reply)
		if err != nil {
			fmt.Fprintln(os.Stderr, "pacrunner: the host PAC service is listed but cannot be started:", err)
			return false
		}
		return reply == 1 || reply == 2 // DBUS_START_REPLY_SUCCESS / DBUS_START_REPLY_ALREADY_RUNNING
	}
	ensure := func() {
		if owned() {
			return // someone serves the name: no second process, no replacement
		}
		now := time.Now()
		starts = slices.DeleteFunc(starts, func(t time.Time) bool { return now.Sub(t) > pacRestartWindow })
		if len(starts) >= pacRestartLimit {
			fmt.Fprintln(os.Stderr, "pacrunner: the bundled helper keeps exiting, giving up")
			return
		}
		starts = append(starts, now)
		if hostService && startHostService() {
			return // the host's helper serves the name now; the bus restarts it on demand
		}
		cmd := exec.Command(helper)
		cmd.SysProcAttr = &syscall.SysProcAttr{Pdeathsig: syscall.SIGTERM, Setpgid: true}
		if err := cmd.Start(); err != nil {
			fmt.Fprintln(os.Stderr, "pacrunner: cannot start the bundled helper:", err)
			return
		}
		go func() {
			_ = cmd.Wait()
			select {
			case exited <- struct{}{}:
			default:
			}
		}()
		deadline := time.Now().Add(pacBudget)
		for time.Now().Before(deadline) && !owned() {
			time.Sleep(50 * time.Millisecond)
		}
	}
	ensure()
	settle()
	for {
		select {
		case sig := <-signals:
			if sig != nil && len(sig.Body) == 3 {
				if newOwner, _ := sig.Body[2].(string); newOwner == "" {
					ensure() // the owner went away and nothing can activate the name
				}
			}
		case <-exited:
			ensure()
		}
	}
}
