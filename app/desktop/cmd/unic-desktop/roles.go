package main

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"os"
	"os/signal"
	"runtime"
	"sync"
	"syscall"
	"time"

	"github.com/uniclipboard/unic/app/desktop/internal/desktopprocess"
	"github.com/uniclipboard/unic/app/desktop/internal/lifecycle"
	"github.com/uniclipboard/unic/app/desktop/internal/quickpanel"
	"github.com/uniclipboard/unic/app/desktop/internal/setupflow"
	"github.com/uniclipboard/unic/internal/clipboardintegration"
	api "github.com/uniclipboard/unic/internal/localapi"
	config "github.com/uniclipboard/unic/internal/localconfig"
	paths "github.com/uniclipboard/unic/internal/localpaths"
)

// Each role enters this function in a separate operating-system process.
var runRole = runNativeRole
var useDaemonClipboard = runtime.GOOS == "darwin"

// panelActivation is ephemeral. Never write it to a file or a log.
type panelActivation struct {
	Target    quickpanel.Target `json:"target"`
	Error     string            `json:"error,omitempty"`
	Favorites bool              `json:"favorites,omitempty"`
}
type clipboardReply struct {
	Mark    string `json:"mark"`
	Failure string `json:"failure"`
}
type clipboardRequest struct {
	ID    string `json:"id"`
	Plain bool   `json:"plain"`
	Mark  string `json:"mark"`
}
type roleStatus struct {
	Ready         bool           `json:"ready"`
	SetupRequired bool           `json:"setup_required"`
	Visible       bool           `json:"visible"`
	ServicePID    int            `json:"service_pid"`
	Shortcut      shortcutStatus `json:"shortcut"`
	LoginEnabled  bool           `json:"login_enabled"`
	LoginError    string         `json:"login_error"`
	Updates       updateStatus   `json:"updates"`
}
type updateStatus struct {
	Supported          bool   `json:"supported"`
	Owner              string `json:"owner"`
	AutoCheckUpdate    bool   `json:"auto_check_update"`
	AutoDownloadUpdate bool   `json:"auto_download_update"`
	AvailableVersion   string `json:"available_version,omitempty"`
}
type updatePrefsRequest struct {
	AutoCheckUpdate    *bool `json:"auto_check_update,omitempty"`
	AutoDownloadUpdate *bool `json:"auto_download_update,omitempty"`
}
type shortcutStatus struct {
	Requested string `json:"requested"`
	Effective string `json:"effective"`
	Error     string `json:"error"`
}

// trayRuntime owns the service-lifetime pipe and the adapter on hosts where the service does not provide one.
// runtimeMu serializes service transitions, including migration and shutdown.
type trayRuntime struct {
	nativeReady             chan struct{}
	wanted                  bool
	paused                  bool
	retiring                bool
	ctx                     context.Context
	cancel                  context.CancelFunc
	dir, socket, executable string
	settings                *config.Manager
	runtimeMu               sync.Mutex
	service                 *lifecycle.Process
	adapterCancel           context.CancelFunc
	adapterDone             chan struct{}
	clipboard               *quickpanel.Clipboard
	mu                      sync.Mutex
	state                   roleStatus
	capture                 func() panelActivation
	shortcut                func(bool) shortcutStatus
	login                   func(bool) (bool, string)
	checkUpdates            func()
	updatePrefs             func(updatePrefsRequest) updateStatus
	quit                    func()
	opening                 sync.Mutex
}

func newTrayRuntime(ctx context.Context, cancel context.CancelFunc, settings *config.Manager) (*trayRuntime, error) {
	dir, err := paths.Dir()
	if err != nil {
		return nil, err
	}
	socket, err := paths.Socket()
	if err != nil {
		return nil, err
	}
	executable, err := lifecycle.Sibling()
	if err != nil {
		return nil, err
	}
	flow, err := setupflow.Open(dir)
	if err != nil {
		return nil, err
	}
	r := &trayRuntime{paused: flow.Required(), ctx: ctx, cancel: cancel, dir: dir, socket: socket, executable: executable, settings: settings, nativeReady: make(chan struct{})}
	if !useDaemonClipboard {
		r.clipboard = quickpanel.NewClipboard(newDesktopBoard())
	}
	return r, nil
}
func (r *trayRuntime) status() roleStatus {
	r.mu.Lock()
	state := r.state
	r.mu.Unlock()
	if flow, err := setupflow.Open(r.dir); err == nil {
		state.SetupRequired = flow.Required()
	}
	ctx, cancel := context.WithTimeout(r.ctx, time.Second)
	defer cancel()
	if v, err := api.NewClient(r.socket).Service(ctx); err == nil && v.Directory == r.dir {
		state.ServicePID = v.PID
	}
	return state
}
func (r *trayRuntime) isRetiring() bool {
	r.mu.Lock()
	defer r.mu.Unlock()
	return r.retiring || r.ctx.Err() != nil
}
func (r *trayRuntime) start(ctx context.Context, entering bool) error {
	if r.isRetiring() {
		return desktopprocess.ErrUnavailable
	}
	select {
	case <-r.nativeReady:
	case <-ctx.Done():
		return ctx.Err()
	}
	r.runtimeMu.Lock()
	defer r.runtimeMu.Unlock()
	flow, err := setupflow.Open(r.dir)
	if err != nil {
		return err
	}
	if flow.Required() && !entering {
		return errors.New("desktop setup is required")
	}
	r.wanted = true
	if r.adapterCancel != nil {
		return nil
	}
	service, err := lifecycle.AcquireService(ctx, r.dir, r.socket, r.executable)
	if err != nil {
		return err
	}
	if entering {
		if err = flow.Complete(); err != nil {
			if service != nil {
				_ = service.Stop()
			}
			return err
		}
	}
	if useDaemonClipboard {
		if err := api.NewClient(r.socket).EnableDesktopClipboard(ctx); err != nil {
			if service != nil {
				_ = service.Stop()
			}
			return err
		}
	}
	adapterCtx, cancel := context.WithCancel(r.ctx)
	r.service = service
	r.adapterCancel = cancel
	r.adapterDone = make(chan struct{})
	if useDaemonClipboard {
		enabled, err := api.NewClient(r.socket).EnableDesktopNotifications(ctx)
		if err == nil && !enabled {
			go notifyPending(adapterCtx, r.socket, func() string { return r.settings.Current().Desktop.Language })
		}
		close(r.adapterDone)
	} else {
		done := r.adapterDone
		go func() {
			defer close(done)
			_ = clipboardintegration.RunRemote(adapterCtx, api.NewClient(r.socket), r.clipboard, nil)
		}()
	}
	r.mu.Lock()
	r.state.Ready = true
	r.state.SetupRequired = false
	r.paused = false
	r.mu.Unlock()
	if r.shortcut != nil {
		go func() { state := r.shortcut(false); r.mu.Lock(); r.state.Shortcut = state; r.mu.Unlock() }()
	}
	return nil
}
func (r *trayRuntime) pause(migration bool) error {
	r.runtimeMu.Lock()
	defer r.runtimeMu.Unlock()
	if migration && r.service == nil {
		ctx, cancel := context.WithTimeout(r.ctx, time.Second)
		defer cancel()
		if _, err := api.NewClient(r.socket).Service(ctx); err == nil {
			return errors.New("independent service blocks offline migration")
		}
	}
	r.wanted = false
	if r.adapterCancel != nil {
		r.adapterCancel()
		<-r.adapterDone
		r.adapterCancel = nil
	}
	var err error
	if r.service != nil {
		err = r.service.Stop()
		r.service = nil
	}
	r.mu.Lock()
	r.state.Ready = false
	r.mu.Unlock()
	return err
}
func (r *trayRuntime) openGUI(ctx context.Context) error { return r.openGUIAction(ctx, "") }
func (r *trayRuntime) openGUIAction(ctx context.Context, action string) error {
	r.opening.Lock()
	defer r.opening.Unlock()
	return r.openGUILocked(ctx, action)
}
func (r *trayRuntime) openGUILocked(ctx context.Context, action string) error {
	if r.isRetiring() {
		return desktopprocess.ErrUnavailable
	}
	info, err := launchDesktopRole(ctx, r.dir, r.socket, desktopprocess.GUI)
	if err != nil {
		return err
	}
	quickpanel.AllowForeground(info.PID)
	return desktopprocess.Call(ctx, r.dir, desktopprocess.GUI, "activate", action, nil)
}
func (r *trayRuntime) openPanel(ctx context.Context) error {
	return r.openPanelFiltered(ctx, false)
}

func (r *trayRuntime) openPanelFiltered(ctx context.Context, favorites bool) error {
	select {
	case <-r.nativeReady:
	case <-ctx.Done():
		return ctx.Err()
	}
	// Serialize capture and activation. Never capture a target behind an already
	// visible panel. Its existing session must handle repeated shortcuts.
	r.opening.Lock()
	defer r.opening.Unlock()
	r.mu.Lock()
	paused, retiring := r.paused, r.retiring
	r.mu.Unlock()
	if retiring || r.ctx.Err() != nil {
		return desktopprocess.ErrUnavailable
	}
	if paused {
		return errors.New("desktop integration is paused for migration")
	}
	if flow, err := setupflow.Open(r.dir); err != nil {
		return err
	} else if flow.Required() {
		return r.openGUILocked(ctx, "")
	}
	var state roleStatus
	if err := desktopprocess.Call(ctx, r.dir, desktopprocess.QuickPanel, "status", nil, &state); err == nil && state.Visible {
		if favorites {
			return desktopprocess.Call(ctx, r.dir, desktopprocess.QuickPanel, "favorites", nil, nil)
		}
		return desktopprocess.Call(ctx, r.dir, desktopprocess.QuickPanel, "close", nil, nil)
	}
	activation := panelActivation{Error: "target"}
	if r.capture != nil {
		activation = r.capture()
	}
	if activation.Target.PID != 0 {
		for _, role := range []desktopprocess.Role{desktopprocess.GUI, desktopprocess.QuickPanel} {
			if info, err := desktopprocess.Probe(ctx, r.dir, role); err == nil && info.PID == activation.Target.PID {
				activation = panelActivation{Error: "target"}
			}
		}
	}
	activation.Favorites = favorites
	info, err := launchDesktopRole(ctx, r.dir, r.socket, desktopprocess.QuickPanel)
	if err != nil {
		return err
	}
	quickpanel.AllowForeground(info.PID)
	return desktopprocess.Call(ctx, r.dir, desktopprocess.QuickPanel, "activate", activation, nil)
}
func (r *trayRuntime) handler(ctx context.Context, command string, raw json.RawMessage) (any, error) {
	switch command {
	case "status":
		return r.status(), nil
	case "startup", "background":
		flow, err := setupflow.Open(r.dir)
		if err != nil {
			return nil, err
		}
		if flow.Required() {
			return nil, r.openGUI(ctx)
		}
		if err = r.start(ctx, false); err != nil {
			return nil, err
		}
		if command == "background" {
			return nil, nil
		}
		return nil, r.openGUI(ctx)
	case "gui-closed":
		r.opening.Lock()
		defer r.opening.Unlock()
		// Reload imports an immediate save. A malformed or missing file keeps
		// the manager's last valid configuration, including its close policy.
		_ = r.settings.Reload()
		if r.settings.Current().Desktop.Mode != "lightweight" {
			return nil, nil
		}
		if _, err := desktopprocess.Probe(ctx, r.dir, desktopprocess.GUI); err == nil {
			// A later user activation already created another GUI.
			return nil, nil
		} else if !errors.Is(err, desktopprocess.ErrUnavailable) {
			return nil, err
		}
		r.runtimeMu.Lock()
		if r.service != nil {
			if err := r.service.Detach(ctx); err != nil {
				r.runtimeMu.Unlock()
				return nil, err
			}
			r.service = nil
		}
		r.runtimeMu.Unlock()
		r.mu.Lock()
		r.retiring = true
		r.mu.Unlock()
		return desktopprocess.ReplyAfter(nil, func() {
			r.cancel()
			if r.quit != nil {
				r.quit()
			}
		}), nil
	case "open-gui":
		var action string
		_ = json.Unmarshal(raw, &action)
		return nil, r.openGUIAction(ctx, action)
	case "open-quick-panel":
		return nil, r.openPanel(ctx)
	case "start-runtime":
		return nil, r.start(ctx, true)
	case "pause-runtime":
		// Block every panel entry before stopping integration and saving pending intent.
		r.opening.Lock()
		defer r.opening.Unlock()
		r.mu.Lock()
		r.paused = true
		r.mu.Unlock()
		if r.shortcut != nil {
			state := r.shortcut(false)
			r.mu.Lock()
			r.state.Shortcut = state
			r.mu.Unlock()
		}
		_ = desktopprocess.Call(ctx, r.dir, desktopprocess.QuickPanel, "quit", nil, nil)
		err := r.pause(true)
		if err == nil {
			flow, e := setupflow.Open(r.dir)
			if e == nil {
				e = flow.BeginMigration()
			}
			err = e
		}
		if err != nil {
			r.mu.Lock()
			r.paused = false
			r.mu.Unlock()
			if r.shortcut != nil {
				state := r.shortcut(false)
				r.mu.Lock()
				r.state.Shortcut = state
				r.mu.Unlock()
			}
			_ = r.start(ctx, false)
		}
		return nil, err
	case "retry-shortcut":
		select {
		case <-r.nativeReady:
		case <-ctx.Done():
			return nil, ctx.Err()
		}
		if r.shortcut != nil {
			state := r.shortcut(true)
			r.mu.Lock()
			r.state.Shortcut = state
			r.mu.Unlock()
		}
		return nil, nil
	case "retry-login":
		select {
		case <-r.nativeReady:
		case <-ctx.Done():
			return nil, ctx.Err()
		}
		if r.login != nil {
			enabled, message := r.login(true)
			r.mu.Lock()
			r.state.LoginEnabled = enabled
			r.state.LoginError = message
			r.mu.Unlock()
		}
		return nil, nil
	case "check-updates":
		select {
		case <-r.nativeReady:
		case <-ctx.Done():
			return nil, ctx.Err()
		}
		// Reload config so a just-saved update_channel is seen before checking.
		_ = r.settings.Reload()
		if r.checkUpdates != nil {
			r.checkUpdates()
		}
		return nil, nil
	case "set-update-prefs":
		select {
		case <-r.nativeReady:
		case <-ctx.Done():
			return nil, ctx.Err()
		}
		var in updatePrefsRequest
		if err := json.Unmarshal(raw, &in); err != nil {
			return nil, err
		}
		if r.updatePrefs != nil {
			st := r.updatePrefs(in)
			r.mu.Lock()
			r.state.Updates = st
			r.mu.Unlock()
			return st, nil
		}
		r.mu.Lock()
		st := r.state.Updates
		r.mu.Unlock()
		return st, nil
	case "clipboard-apply":
		var in clipboardRequest
		if err := json.Unmarshal(raw, &in); err != nil {
			return nil, err
		}
		if r.clipboard == nil {
			return nil, desktopprocess.ErrUnavailable
		}
		mark, err := r.clipboard.Apply(ctx, api.NewClient(r.socket), in.ID, in.Plain)
		return clipboardReply{Mark: mark, Failure: quickpanel.FailureCode(err)}, nil
	case "clipboard-matches":
		var in clipboardRequest
		if err := json.Unmarshal(raw, &in); err != nil {
			return nil, err
		}
		return r.clipboard != nil && r.clipboard.Matches(in.Mark), nil
	case "restart", "restart-gui":
		var action string
		_ = json.Unmarshal(raw, &action)
		return nil, r.restart(command == "restart", action)
	case "quit":
		r.opening.Lock()
		r.mu.Lock()
		r.retiring = true
		r.mu.Unlock()
		r.opening.Unlock()
		return desktopprocess.ReplyAfter(nil, func() {
			r.cancel()
			if r.quit != nil {
				r.quit()
			}
		}), nil

	default:
		return nil, fmt.Errorf("unknown tray command")
	}
}
func (r *trayRuntime) close() {
	ctx, cancel := context.WithTimeout(context.Background(), 8*time.Second)
	defer cancel()
	_ = r.closeViews(ctx, desktopprocess.GUI, desktopprocess.QuickPanel)
	_ = r.pause(false)
}

func (r *trayRuntime) restart(restartService bool, action string) error {
	if !r.opening.TryLock() {
		return errors.New("another desktop operation is in progress")
	}
	defer r.opening.Unlock()
	r.mu.Lock()
	retiring, paused := r.retiring, r.paused
	r.mu.Unlock()
	if retiring || r.ctx.Err() != nil {
		return desktopprocess.ErrUnavailable
	}
	if paused {
		return errors.New("desktop integration is paused for migration")
	}
	// Finish the close phase within the GUI client's request deadline. Close the
	// panel first so a panel failure leaves the requesting GUI available.
	closeCtx, cancelClose := context.WithTimeout(r.ctx, 8*time.Second)
	defer cancelClose()
	if restartService {
		if err := r.closeViews(closeCtx, desktopprocess.QuickPanel); err != nil {
			return err
		}
	}
	if err := r.closeViews(closeCtx, desktopprocess.GUI); err != nil {
		return err
	}
	if err := closeCtx.Err(); err != nil {
		return err
	}
	if r.isRetiring() {
		return desktopprocess.ErrUnavailable
	}
	// The closing GUI cancels its IPC call. Use the tray lifetime to complete
	// the restart even after the requesting process has exited.
	ctx, cancel := context.WithTimeout(r.ctx, 45*time.Second)
	defer cancel()
	var startErr error
	if restartService {
		if r.settings != nil {
			_ = r.settings.Reload()
		}
		if err := r.pause(false); err != nil {
			logHostFailure()
		}
		if r.isRetiring() {
			return desktopprocess.ErrUnavailable
		}
		startErr = r.start(ctx, false)
	}
	openErr := r.openGUILocked(ctx, action)
	if startErr != nil || openErr != nil {
		logHostFailure()
	}
	if startErr != nil {
		return startErr
	}
	return openErr
}
func roleContext() (context.Context, context.CancelFunc) {
	return signal.NotifyContext(context.Background(), os.Interrupt, syscall.SIGTERM)
}

// remoteClipboard preserves the tracked write receipt in the adapter owner.
// UI processes neither acquire an adapter session nor access the native board.
type remoteClipboard struct {
	dir    string
	daemon *api.Client
}

func (r remoteClipboard) Apply(ctx context.Context, _ *api.Client, id string, plain bool) (string, error) {
	if r.daemon != nil {
		out, err := r.daemon.ApplyDesktopClipboard(ctx, id, plain)
		if err == nil {
			err = quickpanel.FailureFromCode(out.Failure)
		}
		return out.Mark, err
	}
	var reply clipboardReply
	err := desktopprocess.Call(ctx, r.dir, desktopprocess.Tray, "clipboard-apply", clipboardRequest{ID: id, Plain: plain}, &reply)
	if err == nil {
		err = quickpanel.FailureFromCode(reply.Failure)
	}
	return reply.Mark, err
}
func (r remoteClipboard) Matches(mark string) bool {
	ctx, cancel := context.WithTimeout(context.Background(), time.Second)
	defer cancel()
	if r.daemon != nil {
		matches, err := r.daemon.MatchDesktopClipboard(ctx, mark)
		return err == nil && matches
	}
	var matches bool
	return desktopprocess.Call(ctx, r.dir, desktopprocess.Tray, "clipboard-matches", clipboardRequest{Mark: mark}, &matches) == nil && matches
}

// closeViews is a shutdown barrier. No PID-only kill is permitted.
func (r *trayRuntime) closeViews(ctx context.Context, roles ...desktopprocess.Role) error {
	for _, role := range roles {
		err := desktopprocess.Call(ctx, r.dir, role, "quit", nil, nil)
		if err != nil && !errors.Is(err, desktopprocess.ErrUnavailable) {
			return err
		}
	}
	for _, role := range roles {
		for {
			_, err := desktopprocess.Probe(ctx, r.dir, role)
			if errors.Is(err, desktopprocess.ErrUnavailable) {
				break
			}
			if err != nil {
				return err
			}
			select {
			case <-ctx.Done():
				return ctx.Err()
			case <-time.After(50 * time.Millisecond):
			}
		}
	}
	return nil
}

var roleEnvironment = func(_ desktopprocess.Role, env []string) []string { return env }

func launchDesktopRole(ctx context.Context, dir, socket string, role desktopprocess.Role, args ...string) (desktopprocess.Info, error) {
	return desktopprocess.LaunchEnv(ctx, dir, socket, role, roleEnvironment(role, os.Environ()), args...)
}

// A user close reaches this barrier only after GUI resources and role ownership
// have been released. A concurrent reopen is admitted by the tray before it
// decides whether the lightweight session can retire.
func notifyGUIClosed(dir string) {
	ctx, cancel := context.WithTimeout(context.Background(), 35*time.Second)
	defer cancel()
	if err := desktopprocess.Call(ctx, dir, desktopprocess.Tray, "gui-closed", nil, nil); err != nil && !errors.Is(err, desktopprocess.ErrUnavailable) {
		logHostFailure()
	}
}
