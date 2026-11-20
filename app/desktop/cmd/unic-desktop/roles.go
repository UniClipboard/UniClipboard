package main

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"os"
	"os/signal"
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

// panelActivation is ephemeral. Never write it to a file or a log.
type panelActivation struct {
	Target quickpanel.Target `json:"target"`
	Error  string            `json:"error,omitempty"`
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
}
type shortcutStatus struct {
	Requested string `json:"requested"`
	Effective string `json:"effective"`
	Error     string `json:"error"`
}

// trayRuntime owns the only clipboard adapter and only service-lifetime pipe.
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
	return &trayRuntime{paused: flow.Required(), ctx: ctx, cancel: cancel, dir: dir, socket: socket, executable: executable, settings: settings, nativeReady: make(chan struct{}), clipboard: quickpanel.NewClipboard(newDesktopBoard())}, nil
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
	adapterCtx, cancel := context.WithCancel(r.ctx)
	r.service = service
	r.adapterCancel = cancel
	r.adapterDone = make(chan struct{})
	done := r.adapterDone
	go func() {
		defer close(done)
		_ = clipboardintegration.RunRemote(adapterCtx, api.NewClient(r.socket), r.clipboard, nil)
	}()
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
	case "clipboard-apply":
		var in clipboardRequest
		if err := json.Unmarshal(raw, &in); err != nil {
			return nil, err
		}
		mark, err := r.clipboard.Apply(ctx, api.NewClient(r.socket), in.ID, in.Plain)
		return clipboardReply{Mark: mark, Failure: quickpanel.FailureCode(err)}, nil
	case "clipboard-matches":
		var in clipboardRequest
		if err := json.Unmarshal(raw, &in); err != nil {
			return nil, err
		}
		return r.clipboard.Matches(in.Mark), nil
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
	_ = r.closeViews(ctx)
	_ = r.pause(false)
}
func roleContext() (context.Context, context.CancelFunc) {
	return signal.NotifyContext(context.Background(), os.Interrupt, syscall.SIGTERM)
}

// remoteClipboard preserves the tracked write receipt in the adapter owner.
// UI processes neither acquire an adapter session nor access the native board.
type remoteClipboard struct{ dir string }

func (r remoteClipboard) Apply(ctx context.Context, _ *api.Client, id string, plain bool) (string, error) {
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
	var matches bool
	return desktopprocess.Call(ctx, r.dir, desktopprocess.Tray, "clipboard-matches", clipboardRequest{Mark: mark}, &matches) == nil && matches
}

// closeViews is a shutdown barrier. No PID-only kill is permitted.
func (r *trayRuntime) closeViews(ctx context.Context) error {
	roles := []desktopprocess.Role{desktopprocess.GUI, desktopprocess.QuickPanel}
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
