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
	app           *application.App
	client        *daemonclient.Client
	effects       *visualEffects
	panel         panelState
	exit          exitIntent
	prompts       promptStore
	stopScheduler context.CancelFunc
	stopTray      context.CancelFunc
	notifier      *notifications.NotificationService
	helper        *quickpanelhelper.Supervisor // nil when the WebView quick panel is in use
	updates       updater
	tray          *trayMenu
	files         knownFiles

	quitting atomic.Bool

	shortcutsMu       sync.Mutex
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
		log.Fatal(err)
	}
	spawnedDaemon := false // whether this launch started the daemon (a cold start) or attached to one
	outcome, err := daemonlife.ProbeForReuse(daemonlife.StartupTimeout)
	if err != nil {
		log.Fatal(err)
	}
	switch outcome.Kind {
	case daemonlife.Incompatible:
		log.Fatal(daemonlife.IncompatibleError(outcome))
	case daemonlife.Absent:
		spawnedDaemon = true
		if err := daemonproc.SpawnDetachedDaemon("gui"); err != nil {
			log.Fatal(err)
		}
		if err := daemonlife.WaitHealthy(daemonlife.StartupTimeout, ""); err != nil {
			log.Fatal(err)
		}
	case daemonlife.Compatible:
		if outcome.Health.Residency == daemonlife.ResidencyOneshot {
			log.Fatal("PoC requires a persistent daemon; refusing to replace an existing oneshot daemon")
		}
	}
	client, err := daemonclient.FromEnv()
	if err != nil {
		log.Fatal(err)
	}
	content, err := fs.Sub(assets, "frontend/dist")
	if err != nil {
		log.Fatal(err)
	}
	host := &HostService{client: client, effects: newVisualEffects(), notifier: notifications.New()}
	services := []application.Service{application.NewService(host), application.NewService(host.notifier)}
	services = append(services, e2eServices(host)...)
	app := application.New(application.Options{Name: "UniClipboard Go GUI", Services: services, Mac: application.MacOptions{ActivationPolicy: activationPolicy()}, Assets: application.AssetOptions{Handler: application.BundledAssetFileServer(content), Middleware: host.fileMiddleware},
		ShouldQuit: func() bool { host.quitting.Store(true); return true },
		OnShutdown: host.shutdown})
	host.app = app
	startup, _ := host.loadStartupSettings()
	// A Silent or Lightweight launch does not build the window at boot (and so never pays the WebView
	// cost); it is created the first time something asks to show it.
	if !startup.hidden() || forceMainWindow() {
		host.openMainWindow()
	}
	host.initQuickPanel()
	go host.reconcileAutoStart()
	host.initTray()
	host.watchNotificationClicks()
	// The launch sequence waits for the event loop: it may show windows, notify, or quit the app.
	app.Event.OnApplicationEvent(events.Common.ApplicationStarted, func(*application.ApplicationEvent) {
		if startup.silent() {
			// Silent mode leaves no window to see, so tell the user the app is running.
			if err := host.notify("silent-start", "UniClipboard", backgroundNotice); err != nil {
				log.Printf("failed to show the silent-start notification: %v", err)
			}
		}
		go host.coldLaunch(startup, spawnedDaemon)
	})
	schedulerCtx, stopScheduler := context.WithCancel(context.Background())
	host.stopScheduler = stopScheduler
	go host.runUpdateScheduler(schedulerCtx)
	if err := app.Run(); err != nil {
		log.Fatal(err)
	}
}
