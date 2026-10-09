package main

import (
	"context"
	"embed"
	"fmt"
	"io/fs"
	"log"
	"os"
	"sync"
	"sync/atomic"
	"time"

	"github.com/UniClipboard/UniClipboard/packages/desktop-host-go/buildinfo"
	"github.com/UniClipboard/UniClipboard/packages/desktop-host-go/daemonclient"
	"github.com/UniClipboard/UniClipboard/packages/desktop-host-go/daemonlife"
	"github.com/UniClipboard/UniClipboard/packages/desktop-host-go/daemonproc"
	"github.com/UniClipboard/UniClipboard/packages/desktop-host-go/quickpanelhelper"
	"github.com/wailsapp/wails/v3/pkg/application"
	"github.com/wailsapp/wails/v3/pkg/events"
	"github.com/wailsapp/wails/v3/pkg/services/notifications"
)

//go:embed all:frontend/dist
var assets embed.FS

type HostService struct {
	app              *application.App
	client           *daemonclient.Client
	effects          *visualEffects
	panel            panelState
	exit             exitIntent
	prompts          promptStore
	stopScheduler    context.CancelFunc
	stopTray         context.CancelFunc
	notifier         *notifications.NotificationService
	helper           *quickpanelhelper.Supervisor // nil when the WebView quick panel is in use
	updates          updater
	lastCheck        lastCheckAt
	wake             chan string // pending wake source (system resume, background activity) for the update scheduler, capacity 1
	analytics        analyticsQueue
	stopWake         func()
	stopActivity     func() // invalidates the macOS background activity
	tray             *trayMenu
	singleInstanceID string
	readyMu          sync.Mutex
	ready            bool // the daemon bootstrap finished
	showWhenReady    bool // a second launch asked for the main window before that
	files            knownFiles

	quitting atomic.Bool

	shortcutsMu       sync.Mutex
	osShortcuts       []string // the shortcuts currently registered with the OS; guarded by shortcutsMu
	binder            *wailsShortcutBinder
	modifierOnce      sync.Once
	modifier          *modifierMonitor // the WebView panel's modifier double-tap trigger; created by modifierMonitor()
	mainMu            sync.Mutex
	navMu             sync.Mutex
	pendingNavigation string
}

// emit broadcasts an event to every window, matching Tauri's app-wide emit.
func (h *HostService) emit(name string, payload any) { h.app.Event.Emit(name, payload) }

func (h *HostService) takePendingNavigation() any {
	h.navMu.Lock()
	defer h.navMu.Unlock()
	if h.pendingNavigation == "" {
		return nil
	}
	route := h.pendingNavigation
	h.pendingNavigation = ""
	return route
}

type Connection struct {
	BaseURL string `json:"baseUrl"`
	WSURL   string `json:"wsUrl"`
	Profile string `json:"profile"`
	PID     uint32 `json:"pid"`
}

func (h *HostService) Connection() (Connection, error) {
	c, err := daemonproc.ReadConnFile()
	if err != nil {
		return Connection{}, err
	}
	if c == nil {
		return Connection{}, fmt.Errorf("daemon connection unavailable")
	}
	return Connection{h.client.BaseURL, h.client.WSURL, os.Getenv("UC_PROFILE"), c.PID}, nil
}
func (h *HostService) Session() (daemonclient.Session, error) {
	ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
	defer cancel()
	return h.client.ExchangeSession(ctx, "gui")
}

// openMainWindow creates the main window. Closing it hides it so the process,
// daemon connection and window state stay alive until an explicit quit; the
// dock/reopen handler in Wails shows it again.
func (h *HostService) openMainWindow() {
	w := h.app.Window.NewWithOptions(quietOptions(application.WebviewWindowOptions{
		Name: "main", Title: "UniClipboard", URL: "/", Width: 1100, Height: 720, MinWidth: 900, MinHeight: 600,
		Mac: application.MacWindow{TitleBar: application.MacTitleBarHiddenInset},
	}))
	w.RegisterHook(events.Common.WindowClosing, func(e *application.WindowEvent) {
		if h.quitting.Load() {
			return
		}
		e.Cancel()
		w.Hide()
	})
}

func main() {
	if err := validateEnvironment(); err != nil {
		if releaseBuild {
			failStartup(err)
		}
		log.Fatal(err)
	}
	waitForRestartParent()
	content, err := fs.Sub(assets, "frontend/dist")
	if err != nil {
		log.Fatal(err)
	}
	host := &HostService{effects: newVisualEffects(), notifier: notifications.New(), wake: make(chan string, 1)}
	host.lastCheck.recordNow()
	services := append([]application.Service{application.NewService(host)}, notifierServices(host)...)
	services = append(services, e2eServices(host)...)
	uniqueID, err := singleInstanceID()
	if err != nil {
		log.Fatal(err)
	}
	host.singleInstanceID = uniqueID
	// application.New takes the single-instance lock before anything else touches shared state: a second GUI of the
	// same scope hands its arguments to the first one and exits inside this call, so it never probes or spawns a
	// daemon, starts the quick panel helper or reconciles the login item. The daemon client is attached afterwards.
	app := application.New(application.Options{Name: "UniClipboard Go GUI", Services: services, Mac: application.MacOptions{ActivationPolicy: activationPolicy()}, Assets: application.AssetOptions{Handler: application.BundledAssetFileServer(content), Middleware: host.fileMiddleware},
		SingleInstance: host.singleInstanceOptions(uniqueID),
		ShouldQuit:     func() bool { host.quitting.Store(true); return true },
		OnShutdown:     host.shutdown})
	host.app = app
	e2eLaunch(host)
	if hasArg(os.Args[1:], quickPanelLaunchArg) {
		// Tauri contract (validate_primary_launch): the panel can only be requested from a running GUI.
		log.Print("UniClipboard GUI is not running; cannot show the quick panel")
		os.Exit(1)
	}
	// Everything that needs the daemon runs once the event loop is up (ApplicationStarted). Wails registers its
	// second-instance observer when the loop starts, so the seconds a cold daemon start takes must not precede it:
	// a launch arriving meanwhile would be lost. Activations that arrive before bootstrap finishes are held
	// (see onSecondInstance) and carried out afterwards.
	app.Event.OnApplicationEvent(events.Common.ApplicationStarted, func(*application.ApplicationEvent) { go host.bootstrap() })
	host.stopWake = app.Event.OnApplicationEvent(events.Common.SystemDidWake, func(*application.ApplicationEvent) { host.signalWake(wakeSystemResume) })
	if err := app.Run(); err != nil {
		log.Fatal(err)
	}
}

// fatal ends the process when the shell cannot get a usable daemon. A launcher or a login item has no terminal, so the release form
// tells the user in a dialog first (exactly like failStartup does before the event loop runs); other forms keep the log line and the
// immediate exit that scripts and E2E runs rely on. It does not return: the dialog's OK button ends the process.
func (h *HostService) fatal(err error) {
	log.Print(err)
	if !releaseBuild {
		os.Exit(1)
	}
	dialog := h.app.Dialog.Error().SetTitle("UniClipboard").SetMessage(err.Error())
	dialog.AddButton("OK").OnClick(func() { os.Exit(1) })
	dialog.Show()
	select {}
}

// incompatibleDaemonError is the message for a daemon of another version still serving this profile. The usual cause is an in-place
// upgrade while the previous build was running: its processes survive the file replacement, and the new build refuses to act against them.
func incompatibleDaemonError(outcome daemonlife.Outcome) error {
	observed := "of another version"
	if outcome.ObservedVersion != nil {
		observed = *outcome.ObservedVersion
	}
	return fmt.Errorf("UniClipboard %s cannot start because UniClipboard %s is still running. Quit it completely from its tray menu, or log out and back in, then start UniClipboard again.", buildinfo.PackageVersion, observed)
}

// bootstrap attaches to (or starts) the daemon and builds the shell around it: windows, tray, quick panel, login item
// reconcile, the cold-launch sequence and the update scheduler.
func (h *HostService) bootstrap() {
	spawnedDaemon := false // whether this launch started the daemon (a cold start) or attached to one
	outcome, err := daemonlife.ProbeForReuse(daemonlife.StartupTimeout)
	if err != nil {
		h.fatal(err)
	}
	switch outcome.Kind {
	case daemonlife.Incompatible:
		log.Print(daemonlife.IncompatibleError(outcome))
		h.fatal(incompatibleDaemonError(outcome))
	case daemonlife.Absent:
		spawnedDaemon = true
		if err := daemonproc.SpawnDetachedDaemon("gui"); err != nil {
			h.fatal(err)
		}
		if err := daemonlife.WaitHealthy(daemonlife.StartupTimeout, ""); err != nil {
			h.fatal(err)
		}
	case daemonlife.Compatible:
		if outcome.Health.Residency == daemonlife.ResidencyOneshot {
			// A command-line `space init` (or another one-shot client) is still winding down: promote it to the
			// persistent daemon this shell needs, like `uniclip start` does.
			spawnedDaemon = true
			if err := daemonlife.PromoteOneshot(daemonlife.ResidencyStandalone, "gui"); err != nil {
				h.fatal(err)
			}
		}
	}
	client, err := daemonclient.FromEnv()
	if err != nil {
		h.fatal(err)
	}
	h.client = client
	startup, _ := h.loadStartupSettings()
	// A Silent or Lightweight launch does not build the window at boot (and so never pays the WebView
	// cost); it is created the first time something asks to show it.
	if !startup.hidden() || forceMainWindow() {
		h.openMainWindow()
	}
	h.initQuickPanel()
	h.initPanelShortcuts()
	go h.reconcileAutoStart()
	h.initTray()
	h.watchNotificationClicks()
	if startup.silent() {
		// Silent mode leaves no window to see, so tell the user the app is running.
		if err := h.notify("silent-start", "UniClipboard", backgroundNotice); err != nil {
			log.Printf("failed to show the silent-start notification: %v", err)
		}
	}
	go func() {
		h.coldLaunch(startup, spawnedDaemon)
		h.finishBootstrap()
	}()
	if !h.quitting.Load() {
		schedulerCtx, stopScheduler := context.WithCancel(context.Background())
		h.stopScheduler = stopScheduler
		timing := schedulerTimingOverride(defaultSchedulerTiming)
		h.stopActivity = startBackgroundActivity(timing.activityInterval, func() { h.signalWake(wakeBackgroundActivity) })
		go h.runUpdateScheduler(schedulerCtx, timing)
	}
}
