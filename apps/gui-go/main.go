package main

import (
	"context"
	"embed"
	"fmt"
	"io/fs"
	"log"
	"os"
	"os/user"
	"path/filepath"
	"runtime"
	"strings"
	"time"

	"github.com/UniClipboard/UniClipboard/packages/desktop-host-go/apppaths"
	"github.com/UniClipboard/UniClipboard/packages/desktop-host-go/daemonclient"
	"github.com/UniClipboard/UniClipboard/packages/desktop-host-go/daemonlife"
	"github.com/UniClipboard/UniClipboard/packages/desktop-host-go/daemonproc"
	"github.com/wailsapp/wails/v3/pkg/application"
)

//go:embed all:frontend/dist
var assets embed.FS

type HostService struct {
	app    *application.App
	client *daemonclient.Client
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
func (h *HostService) OpenSecondary() {
	if w, ok := h.app.Window.GetByName("secondary"); ok {
		w.Show()
		w.Focus()
		return
	}
	h.app.Window.NewWithOptions(application.WebviewWindowOptions{Name: "secondary", Title: "UniClipboard · Go GUI · 第二窗口", URL: "/?window=secondary", Width: 760, Height: 600})
}
func (h *HostService) Quit() { h.app.Quit() }

func validateIsolation() error {
	if runtime.GOOS != "darwin" {
		return fmt.Errorf("this PoC currently permits only macOS isolation")
	}
	for _, key := range []string{"UNICLIPBOARD_DAEMON_BASE_URL", "UNICLIPBOARD_DAEMON_TOKEN_PATH", "UC_PORTABLE", "UC_DAEMON_RUN_MODE"} {
		if os.Getenv(key) != "" {
			return fmt.Errorf("isolated PoC refuses override %s", key)
		}
	}
	if strings.ContainsAny(os.Getenv("UC_PROFILE"), "/\\.") {
		return fmt.Errorf("invalid isolated profile")
	}
	if apppaths.IsPortable() {
		return fmt.Errorf("isolated PoC refuses portable mode")
	}

	u, err := user.Current()
	if err != nil {
		return err
	}
	home, err := filepath.EvalSymlinks(os.Getenv("HOME"))
	if err != nil {
		return err
	}
	real, err := filepath.EvalSymlinks(u.HomeDir)
	if err != nil {
		return err
	}
	if !strings.HasPrefix(filepath.Base(home), "uc-gui-go-") || home == real || strings.HasPrefix(home, real+string(os.PathSeparator)+"Library"+string(os.PathSeparator)) || !strings.HasPrefix(os.Getenv("UC_PROFILE"), "gui-go-") || os.Getenv("UC_GUI_GO_ISOLATED") != "1" || os.Getenv("UC_DISABLE_SYSTEM_CLIPBOARD") != "1" || os.Getenv("UNICLIPBOARD_ENV") != "development" {
		return fmt.Errorf("Go GUI PoC requires an isolated HOME, gui-go-* profile, UC_GUI_GO_ISOLATED=1, UC_DISABLE_SYSTEM_CLIPBOARD=1 and UNICLIPBOARD_ENV=development")
	}
	root, ok := apppaths.AppDataRoot()
	if !ok {
		return fmt.Errorf("data root unavailable")
	}
	ancestor := root
	for {
		_, err := os.Stat(ancestor)
		if err == nil {
			break
		}
		if !os.IsNotExist(err) {
			return err
		}
		parent := filepath.Dir(ancestor)
		if parent == ancestor {
			return fmt.Errorf("data root has no existing ancestor")
		}
		ancestor = parent
	}
	ancestor, err = filepath.EvalSymlinks(ancestor)
	if err != nil {
		return err
	}
	if ancestor != home && !strings.HasPrefix(ancestor, home+string(os.PathSeparator)) {
		return fmt.Errorf("data root escapes isolated HOME")
	}
	return nil
}
func main() {
	if err := validateIsolation(); err != nil {
		log.Fatal(err)
	}
	outcome, err := daemonlife.ProbeForReuse(daemonlife.StartupTimeout)
	if err != nil {
		log.Fatal(err)
	}
	switch outcome.Kind {
	case daemonlife.Incompatible:
		log.Fatal(daemonlife.IncompatibleError(outcome))
	case daemonlife.Absent:
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
	host := &HostService{client: client}
	services := []application.Service{application.NewService(host)}
	services = append(services, e2eServices(host)...)
	app := application.New(application.Options{Name: "UniClipboard Go GUI", Services: services, Assets: application.AssetOptions{Handler: application.BundledAssetFileServer(content)}, Mac: application.MacOptions{ApplicationShouldTerminateAfterLastWindowClosed: true}})
	host.app = app
	app.Window.NewWithOptions(application.WebviewWindowOptions{Name: "main", Title: "UniClipboard · Wails v3 Go GUI", URL: "/", Width: 860, Height: 650})
	if err := app.Run(); err != nil {
		log.Fatal(err)
	}
}
