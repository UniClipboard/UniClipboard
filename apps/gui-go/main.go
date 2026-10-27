package main

import (
	"context"
	"embed"
	"fmt"
	"io/fs"
	"log"
	"os"
	"runtime"
	"sync"
	"sync/atomic"
	"time"

	"github.com/UniClipboard/UniClipboard/packages/desktop-host-go/buildinfo"
	"github.com/UniClipboard/UniClipboard/packages/desktop-host-go/daemonclient"
	"github.com/UniClipboard/UniClipboard/packages/desktop-host-go/daemonlife"
	"github.com/UniClipboard/UniClipboard/packages/desktop-host-go/daemonproc"
	"github.com/UniClipboard/UniClipboard/packages/desktop-host-go/globalshortcut"
	"github.com/UniClipboard/UniClipboard/packages/desktop-host-go/hostlog"
	"github.com/UniClipboard/UniClipboard/packages/desktop-host-go/quickpanelhelper"
	"github.com/UniClipboard/UniClipboard/packages/desktop-host-go/settings"
	"github.com/wailsapp/wails/v3/pkg/application"
	"github.com/wailsapp/wails/v3/pkg/events"
	"github.com/wailsapp/wails/v3/pkg/services/notifications"
)

//go:embed all:frontend/dist
var assets embed.FS

type HostService struct {
	ui               *uiProcess
	owner            *uiPeer
	app              *application.App
	client           atomic.Pointer[daemonclient.Client] // replaced when the daemon is restarted; read through daemon()
	effects          *visualEffects
	desktopTheme     *desktopThemeState
	stopDesktopTheme context.CancelFunc
	panel            panelState
	exitIntent
	prompts             promptStore
	stopScheduler       context.CancelFunc
	stopTray            context.CancelFunc
	settingsWatch       *settings.Watcher // keeps applySettings current from the daemon, with or without a window
	trayLanguageApplied string            // the tray language applySettings last applied; read and written on the watcher goroutine only
	stopSettings        context.CancelFunc
	notifier            *notifications.NotificationService
	helper              *quickpanelhelper.Supervisor // nil when the WebView quick panel is in use
	updates             updater
	lastCheck           lastCheckAt
	wake                chan string // pending wake source (system resume, background activity) for the update scheduler, capacity 1
	analytics           analyticsQueue
	stopWake            func()
	stopActivity        func() // invalidates the macOS background activity
	tray                *trayMenu
	singleInstanceID    string
	readyMu             sync.Mutex
	ready               bool // the daemon bootstrap finished
	showWhenReady       bool // a second launch asked for the main window before that
	files               knownFiles

	lightweight         atomic.Bool // settings-owned residency; transitions serialize with ui.mu
	modeRestarting      atomic.Bool
	activationForwarded atomic.Bool
	automaticLaunch     bool // captured before bootstrap
	bootstrapping       atomic.Bool
	startupAttempt      atomic.Pointer[daemonproc.StartupAttempt]

	shortcutsMu       sync.Mutex
	panelShortcut     *globalshortcut.Applier // the quick panel shortcut's desired and active binding; set by initQuickPanel
	appliedPanel      appliedPanelSettings    // what reconcilePanel last applied; guarded by shortcutsMu
	osShortcuts       []string                // the shortcuts the WebView executor registered with the OS; written by panelShortcut only
	binder            *wailsShortcutBinder
	modifierOnce      sync.Once
	modifier          *modifierMonitor     // the WebView panel's modifier double-tap trigger; created by modifierMonitor()
	windowSize        *mainWindowSizeState // accessed only on the Wails main thread
	navMu             sync.Mutex
	pendingNavigation string
}

// emit broadcasts an event to every window, matching Tauri's app-wide emit.
// daemon returns the client of the current daemon. A restart replaces it (the new process has a new token), so
// callers fetch it per use instead of keeping it.
func (h *HostService) daemon() *daemonclient.Client { return h.client.Load() }

func (h *HostService) emit(name string, payload any) {
	h.app.Event.Emit(name, payload)
	if h.ui != nil {
		h.ui.event(name, payload)
	}
}

// openMainWindow creates the main window in the UI child. Closing it exits that
// child unless a visible fallback panel still needs it. Normal mode keeps its
// host; lightweight mode releases the temporary host while retaining the daemon.
func (h *HostService) openMainWindow() *application.WebviewWindow {
	minWidth := 900
	if runtime.GOOS == "linux" {
		minWidth = 640
	}
	size, maximized := h.loadMainWindowSize()
	size.Width, size.Height = max(size.Width, minWidth), max(size.Height, 600)
	w := h.app.Window.NewWithOptions(quietOptions(withWindowMaterial(application.WebviewWindowOptions{
		Name: "main", Title: "UniClipboard", URL: "/", Width: size.Width, Height: size.Height, MinWidth: minWidth, MinHeight: 600,
		JS: windowControlsScript(),
		// Match the default Windows custom title bar at creation so Wails sets
		// up DWM decorations on the first activation, before the page loads.
		Frameless: runtime.GOOS == "windows",
		Mac:       application.MacWindow{TitleBar: application.MacTitleBarHiddenInset},
	})))
	// Use the runtime frame dimensions: macOS construction sizes the content
	// rectangle, whereas Size and SetSize use the outer window frame.
	w.SetSize(size.Width, size.Height)
	h.rememberMainWindowSize(w, size, maximized)
	if maximized {
		w.Maximise()
	}
	suppressKeyboardMenu(w)
	w.RegisterHook(events.Common.WindowClosing, func(e *application.WindowEvent) {
		if h.quitting.Load() {
			return
		}
		if panel, ok := h.app.Window.GetByName(quickPanelWindowName); ok && panel.IsVisible() {
			// Let Wails destroy this window while the fallback panel keeps its
			// own WebView. Flushing first avoids touching a destroyed window.
			h.persistMainWindowSize()
			application.InvokeSync(func() { h.windowSize = nil })
			h.setDockIconVisible(false)
			return
		}
		e.Cancel()
		go h.closeUI()
	})
	return w
}

func main() {
	// Persistent role log for the diagnostic export; failing to open it leaves console logging only.
	hostLog := hostlog.OpenDefault("uniclipboard-gui")
	hostLog.RedirectStandardLog()
	hostLog.Info("gui host starting version=%s ui_child=%t", buildinfo.PackageVersion, hasArg(os.Args[1:], uiChildArg))
	defer hostLog.Close()
	if err := validateEnvironment(); err != nil {
		if releaseBuild {
			failStartup(err)
		}
		log.Fatal(err)
	}
	if err := validateUIRole(); err != nil {
		log.Fatal(err)
	}
	if !uiChild {
		waitForRestartParent()
	}
	ensureAppScope()
	withTray := !uiChild && preflightTrayHost()
	content, err := fs.Sub(assets, "frontend/dist")
	if err != nil {
		log.Fatal(err)
	}
	host := &HostService{effects: newVisualEffects(), notifier: notifications.New(), wake: make(chan string, 1)}
	// Follow, not daemon(): after a restart the host did not record (it reported a failure, or the GUI relaunch
	// failed), the watcher must reach the daemon that actually runs, and every other caller then uses it too.
	host.settingsWatch = settings.NewWatcher(func() *daemonclient.Client { return daemonclient.Follow(&host.client) }, host.applySettings)
	if uiChild {
		host.owner = newUIPeer(uiInput, uiOutput, host.handleUIRequest)
	}
	if !uiChild {
		host.desktopTheme = newDesktopThemeState()
	}
	host.lastCheck.recordNow()
	services := []application.Service{hostService(host)}
	if !uiChild {
		services = append(services, notifierServices(host)...)
	}
	services = append(services, e2eServices(host)...)
	uniqueID, err := singleInstanceID()
	if err != nil {
		log.Fatal(err)
	}
	if uiChild {
		uniqueID += ".ui"
	}
	host.singleInstanceID = uniqueID
	if uiProcessEnabled() {
		host.ui = &uiProcess{host: host}
	}
	// application.New takes the single-instance lock before anything else touches shared state: a second GUI of the
	// same scope hands its arguments to the first one and exits inside this call.
	// Preflight only read existing settings; daemon creation, panel and login-item
	// side effects happen after this lock. The daemon client is attached afterwards.
	app := application.New(application.Options{Name: "UniClipboard Go GUI", Services: services, Mac: application.MacOptions{ActivationPolicy: initialActivationPolicy(), ApplicationShouldTerminateAfterLastWindowClosed: false}, Linux: application.LinuxOptions{ApplicationID: uniqueID, ProgramName: bundleID, DisableQuitOnLastWindowClosed: true}, Windows: application.WindowsOptions{DisableQuitOnLastWindowClosed: true, AdditionalBrowserArgs: e2eBrowserArgs(), WebviewUserDataPath: webviewDataPath()}, Assets: application.AssetOptions{Handler: application.BundledAssetFileServer(content), Middleware: host.fileMiddleware},
		// Wails discards its default logger in production, including fatal startup errors.
		ErrorHandler:   func(err error) { log.Printf("Wails application error: %v", err) },
		KeyBindings:    devToolsKeyBindings(),
		SingleInstance: host.singleInstanceOptions(uniqueID),
		ShouldQuit: func() bool {
			if uiChild && !host.quitting.Load() {
				if host.owner == nil {
					host.quitting.Store(true)
					return true
				}
				go func() {
					ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
					defer cancel()
					if err := host.ownerCall(ctx, "quit-app", nil, nil); err != nil {
						log.Printf("full quit request failed: %v", err)
					}
				}()
				return false
			}
			host.beginQuit(false)
			return true
		},
		OnShutdown: host.shutdown})
	host.app = app
	if withTray {
		host.initTray()
	}
	host.installLaunchOriginObserver()
	host.routeDockReopen()
	if !uiChild {
		e2eLaunch(host)
	}
	if hasArg(os.Args[1:], quickPanelLaunchArg) {
		// Tauri contract (validate_primary_launch): the panel can only be requested from a running GUI.
		log.Print("UniClipboard GUI is not running; cannot show the quick panel")
		os.Exit(1)
	}
	// Everything that needs the daemon runs once the event loop is up (ApplicationStarted). Wails registers its
	// second-instance observer when the loop starts, so the seconds a cold daemon start takes must not precede it:
	// a launch arriving meanwhile would be lost. Activations that arrive before bootstrap finishes are held
	// (see onSecondInstance) and carried out afterwards.
	app.Event.OnApplicationEvent(events.Common.ApplicationStarted, func(*application.ApplicationEvent) {
		host.automaticLaunch = hasArg(os.Args[1:], autostartLaunchArg) || platformAutomaticLaunch()
		if host.ui == nil {
			host.startVisualEffects()
		} else {
			host.startDesktopTheme()
		}
		if uiChild {
			go host.bootstrapUI()
		} else {
			go host.bootstrap()
		}
	})
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

// bootstrap attaches to (or starts) the daemon and prepares the shell: windows, tray activation, quick panel, login item
// reconcile, the cold-launch sequence and the update scheduler.
func (h *HostService) bootstrap() { h.bootstrapWithRetry(false) }

func (h *HostService) bootstrapWithRetry(retry bool) {
	if !h.bootstrapping.CompareAndSwap(false, true) {
		return
	}
	defer h.bootstrapping.Store(false)
	spawnedDaemon, err := h.attachStartupDaemon(retry)
	if err != nil {
		h.startupFailed(err)
		return
	}
	client, err := daemonclient.FromEnv()
	if err != nil {
		h.fatal(err)
	}
	h.client.Store(client)
	startup, _ := h.loadStartupSettings()
	h.lightweight.Store(startup.lightweight())
	show := shouldOpenAtLaunch(startup.BackgroundStartup, h.automaticLaunch,
		restartedGUI && hasArg(os.Args[1:], backgroundRestartArg), forceMainWindow())
	withTray := !startup.lightweight() || show || h.heldShow()
	if withTray && h.tray == nil {
		// A background launch may discover that it needs a tray after app.Run.
		// Finish daemon recovery, then hand off to a configured host so the
		// Linux tray is constructed with its menu before the native event loop.
		h.coldLaunch(startup, spawnedDaemon)
		h.reconcileAutoStart()
		if err := h.restartShell(!show && !h.heldShow()); err != nil {
			h.fatal(err)
		}
		return
	}
	if !withTray && h.tray != nil {
		h.tray.tray.Destroy()
		h.tray = nil
	}
	if show {
		h.deferShowUntilReady()
	} else {
		h.setDockIconVisible(false) // no window yet: run as a menu bar app until one is shown
	}
	h.initQuickPanel()
	h.initPanelShortcuts()
	// Finish reconciliation before a windowless lightweight launch may exit.
	h.reconcileAutoStart()
	h.watchNotificationClicks()
	if !show && !startup.lightweight() {
		// Automatic background startup leaves no window to see.
		if err := h.notify("silent-start", "UniClipboard", backgroundNotices[resolveTrayLanguage(startup.Language)]); err != nil {
			log.Printf("failed to show the silent-start notification: %v", err)
		}
	}
	if !h.quitting.Load() {
		schedulerCtx, stopScheduler := context.WithCancel(context.Background())
		h.stopScheduler = stopScheduler
		timing := schedulerTimingOverride(defaultSchedulerTiming)
		h.stopActivity = startBackgroundActivity(timing.activityInterval, func() { h.signalWake(wakeBackgroundActivity) })
		go h.runUpdateScheduler(schedulerCtx, timing)
	}
	go func() {
		h.coldLaunch(startup, spawnedDaemon)
		if h.quitting.Load() {
			return
		}
		h.startSettingsWatch()
		if h.tray != nil {
			h.startTray()
		}
		h.finishBootstrap()
	}()
}

// attachStartupDaemon keeps the resident host as the sole daemon owner.
// A retry may release only this host's exact child after a retryable failure.
func (h *HostService) attachStartupDaemon(retry bool) (bool, error) {
	outcome, err := daemonlife.ProbeForReuse(daemonlife.StartupTimeout)
	if err != nil && retry && daemonlife.RetryableStartupError(err) && h.startupAttempt.Load() != nil {
		ctx, cancel := context.WithTimeout(context.Background(), daemonStopTimeout)
		releaseErr := daemonproc.ReleaseFailedStartup(ctx, h.startupAttempt.Load())
		cancel()
		if releaseErr != nil {
			return false, &daemonlife.Error{Kind: daemonlife.ErrStartupFailed, Retryable: true, Details: fmt.Sprintf("startup retry could not release the owned run: %v", releaseErr)}
		}
		h.startupAttempt.Store(nil)
		outcome, err = daemonlife.ProbeForReuse(daemonlife.StartupTimeout)
	}
	if err != nil {
		return false, err
	}
	switch outcome.Kind {
	case daemonlife.Incompatible:
		log.Print(daemonlife.IncompatibleError(outcome))
		return false, incompatibleDaemonError(outcome)
	case daemonlife.Absent:
		attempt, err := daemonproc.SpawnDetachedDaemonForStartup("gui")
		if err != nil {
			return false, err
		}
		h.startupAttempt.Store(attempt)
		return true, daemonlife.WaitHealthyForAttempt(daemonlife.StartupTimeout, "", attempt)
	case daemonlife.Compatible:
		if outcome.Health.Residency == daemonlife.ResidencyOneshot {
			return true, daemonlife.PromoteOneshot(daemonlife.ResidencyStandalone, "gui")
		}
	}
	return false, nil
}

// Keep a usable recovery path instead of exiting a retryable startup failure.
// This native dialog works before any business endpoint or UI child exists.
func (h *HostService) startupFailed(err error) {
	if !daemonlife.RetryableStartupError(err) {
		h.releaseOwnedFailedStartup()
		h.fatal(err)
		return
	}
	log.Print(err)
	message := err.Error()
	if h.startupAttempt.Load() == nil {
		message += "\nAn existing daemon owns this profile. Retry checks it again without stopping it."
	}
	dialog := h.app.Dialog.Error().SetTitle("UniClipboard").SetMessage(message)
	dialog.AddButton("Retry").OnClick(func() { go h.bootstrapWithRetry(true) })
	dialog.AddButton("Quit").OnClick(func() {
		// An unready or shared daemon must never be stopped by GUI shutdown.
		go func() { h.releaseOwnedFailedStartup(); h.quit(true) }()
	})
	dialog.Show()
}

func (h *HostService) releaseOwnedFailedStartup() {
	if attempt := h.startupAttempt.Load(); attempt != nil {
		ctx, cancel := context.WithTimeout(context.Background(), daemonStopTimeout)
		defer cancel()
		if err := daemonproc.ReleaseFailedStartup(ctx, attempt); err != nil {
			log.Printf("could not release owned failed startup: %v", err)
		}
	}
}
