package main

import (
	"context"
	"encoding/json"
	"errors"
	"os"
	"path/filepath"
	"runtime"
	"strconv"
	"time"

	"github.com/egoist/mygo"
	"github.com/egoist/mygo/plugins/updater"
	"github.com/egoist/mygo/plugins/updater/native"

	"github.com/uniclipboard/unic/app/desktop/internal/appupdates"
	"github.com/uniclipboard/unic/app/desktop/internal/desktopprocess"
	"github.com/uniclipboard/unic/app/desktop/internal/desktopui"
	"github.com/uniclipboard/unic/app/desktop/internal/lifecycle"
	"github.com/uniclipboard/unic/app/desktop/internal/quickpanel"
	api "github.com/uniclipboard/unic/internal/localapi"
	config "github.com/uniclipboard/unic/internal/localconfig"
	paths "github.com/uniclipboard/unic/internal/localpaths"
	"github.com/uniclipboard/unic/internal/releaseinfo"
)

var desktopLaunchMode = "normal"

func configureNativeRole(dir string, accessory bool) {
	mygo.App.SetName(desktopAppName)
	mygo.App.SetVersion(releaseinfo.Version + "-build." + releaseinfo.BuildNumber)
	mygo.App.SetPath(mygo.PathUserData, dir)
	if accessory && runtime.GOOS == "darwin" {
		mygo.App.SetActivationPolicy(mygo.ActivationPolicyAccessory)
	}
}
func roleCall(ctx context.Context, dir string, role desktopprocess.Role, command string, payload any, result any) error {
	return desktopprocess.Call(ctx, dir, role, command, payload, result)
}
func nativeAsync(work func(context.Context) error) {
	go func() {
		ctx, cancel := context.WithTimeout(context.Background(), 45*time.Second)
		defer cancel()
		if work(ctx) != nil {
			logHostFailure()
		}
	}()
}
func runNativeRole(role desktopprocess.Role, settings *config.Manager) error {
	switch role {
	case desktopprocess.Tray:
		return runTray(settings)
	case desktopprocess.GUI:
		return runGUI(settings)
	case desktopprocess.QuickPanel:
		return runPanel(settings)
	default:
		return errors.New("unknown desktop role")
	}
}
func runTray(settings *config.Manager) error {
	ctx, cancel := roleContext()
	defer cancel()
	r, err := newTrayRuntime(ctx, cancel, settings)
	if err != nil {
		return err
	}
	r.quit = mygo.App.Quit
	owner, err := desktopprocess.Acquire(ctx, r.dir, desktopprocess.Tray, r.handler)
	if errors.Is(err, desktopprocess.ErrRunning) {
		return nil
	}
	if err != nil {
		return err
	}
	defer func() { _ = owner.Close() }()
	defer r.close()
	configureNativeRole(r.dir, true)
	program, err := os.Executable()
	if err != nil {
		return err
	}
	if appupdates.PublicKey != "" && appupdates.Owner(program) == "direct" {
		mygo.Updater.SetSource(&desktopUpdateSource{runtime: r, Source: &appupdates.Source{Base: appupdates.FeedBase, Target: runtime.GOOS + "-" + runtime.GOARCH, Key: appupdates.PublicKey, Channel: func() string { return settings.Current().Desktop.UpdateChannel }}})
		mygo.Use(native.Plugin)
		checkDesktopUpdates = updater.CheckForUpdates
	}
	mygo.App.OnWindowAllClosed(func() {})
	mygo.App.OnWillQuit(func(*mygo.QuitEvent) { cancel() })
	mygo.App.OnQuit(func() { r.close(); _ = owner.Close() })
	showDesktop = func() { nativeAsync(r.openGUI) }
	showQuickPanel = func() { nativeAsync(r.openPanel) }
	mygo.App.WhenReady(func() {
		nativePlatform := quickpanel.NewNative()
		shortcut := quickpanel.NewShortcut(mygo.GlobalShortcut.Register, mygo.GlobalShortcut.Unregister, showQuickPanel)
		r.capture = func() panelActivation {
			result := make(chan panelActivation, 1)
			mygo.RunOnMain(func() {
				target, err := nativePlatform.Capture()
				a := panelActivation{Target: target}
				if err != nil {
					a.Error = quickpanel.FailureCode(err)
				}
				result <- a
			})
			select {
			case a := <-result:
				return a
			case <-ctx.Done():
				return panelActivation{Error: "target"}
			}
		}
		registration := lifecycle.LoginRegistration{Path: filepath.Join(r.dir, "desktop-login.json"), Enabled: mygo.App.OpenAtLogin, SetEnabled: func(enabled bool) error {
			if enabled && os.Getenv("UNIC_DESKTOP_PROFILE_KIND") != "default" {
				return errors.New("login startup requires the default profile")
			}
			return mygo.App.SetOpenAtLogin(enabled)
		}}
		r.shortcut = func(retry bool) shortcutStatus {
			done := make(chan shortcutStatus, 1)
			mygo.RunOnMain(func() {
				requested := settings.Current().Desktop.QuickPanelShortcut
				key := requested
				r.mu.Lock()
				paused := r.paused
				r.mu.Unlock()
				if paused {
					key = ""
				}
				message := ""
				if err := shortcut.Set(key); err != nil {
					message = "快捷键注册失败。原快捷键保持有效。请检查冲突后重试。"
				}
				done <- shortcutStatus{Requested: requested, Effective: shortcut.Effective, Error: message}
			})
			select {
			case state := <-done:
				return state
			case <-ctx.Done():
				return shortcutStatus{}
			}
		}
		r.login = func(retry bool) (bool, string) {
			type result struct {
				enabled bool
				message string
			}
			done := make(chan result, 1)
			mygo.RunOnMain(func() {
				enabled, err := registration.Apply(settings.Current().Desktop.LaunchAtLogin, retry)
				message := ""
				if err != nil {
					message = "登录启动设置未生效。请检查系统登录项权限。"
				}
				done <- result{enabled, message}
			})
			select {
			case state := <-done:
				return state.enabled, state.message
			case <-ctx.Done():
				return false, ""
			}
		}
		close(r.nativeReady)
		updateTray := setupTray(ctx, settings.Current().Desktop)
		mygo.App.OnWillQuit(func(*mygo.QuitEvent) {
			shortcut.Close()
			if closer, ok := nativePlatform.(interface{ Close() }); ok {
				closer.Close()
			}
		})
		apply := func() {
			s := r.shortcut(false)
			enabled, message := r.login(false)
			r.mu.Lock()
			r.state.Shortcut = s
			r.state.LoginEnabled = enabled
			r.state.LoginError = message
			r.mu.Unlock()
		}
		settings.OnChange = func(_, current config.Config, err error) {
			if err == nil {
				mygo.RunOnMain(func() { updateTray(current.Desktop) })
				go apply()
			}
		}
		go apply()
		go func() { _ = settings.Watch(ctx) }()
		go notifyPending(ctx, r.socket, func() string { return settings.Current().Desktop.Language })
		// A directly restarted tray (including updater relaunch) applies the same
		// saved mode and setup gate as the no-role launcher.
		go func() {
			_, err := r.handler(ctx, "startup", json.RawMessage(strconv.Quote(desktopLaunchMode)))
			if err != nil {
				logHostFailure()
			}
		}()

		go r.supervise()
		go func() { <-ctx.Done(); mygo.App.Quit() }()
	})
	return mygo.App.Run()
}
func (r *trayRuntime) supervise() {
	ticker := time.NewTicker(2 * time.Second)
	defer ticker.Stop()
	for {
		select {
		case <-r.ctx.Done():
			return
		case <-ticker.C:
			r.runtimeMu.Lock()
			service := r.service
			active := r.adapterCancel != nil
			wanted := r.wanted
			r.runtimeMu.Unlock()
			if !wanted {
				continue
			}
			if !active {
				ctx, cancel := context.WithTimeout(r.ctx, 35*time.Second)
				_ = r.start(ctx, false)
				cancel()
				continue
			}
			failed := false
			if service != nil {
				select {
				case <-service.Done():
					failed = true
				default:
				}
			} else {
				ctx, cancel := context.WithTimeout(r.ctx, time.Second)
				_, err := api.NewClient(r.socket).Service(ctx)
				cancel()
				failed = err != nil
			}
			if failed {
				_ = r.pause(false)
				ctx, cancel := context.WithTimeout(r.ctx, 35*time.Second)
				_ = r.start(ctx, false)
				cancel()
			}
		}
	}
}
func runPanel(settings *config.Manager) error {
	dir, err := paths.Dir()
	if err != nil {
		return err
	}
	ctx, cancel := roleContext()
	defer cancel()
	var panel *desktopui.ProcessPanel
	ready := make(chan struct{})
	handler := func(ctx context.Context, command string, raw json.RawMessage) (any, error) {
		select {
		case <-ready:
		case <-ctx.Done():
			return nil, ctx.Err()
		}
		switch command {
		case "status":
			result := make(chan roleStatus, 1)
			mygo.RunOnMain(func() { result <- roleStatus{Ready: panel != nil, Visible: panel != nil && panel.Visible()} })
			select {
			case state := <-result:
				return state, nil
			case <-ctx.Done():
				return nil, ctx.Err()
			}
		case "activate":
			var activation panelActivation
			if err := json.Unmarshal(raw, &activation); err != nil {
				return nil, err
			}
			mygo.RunOnMain(func() {
				if panel != nil {
					panel.Open(activation.Target, activation.Error)
				}
			})
			return nil, nil
		case "close":
			mygo.RunOnMain(func() {
				if panel != nil {
					panel.Hide()
				}
			})
			return nil, nil
		case "quit":
			return desktopprocess.ReplyAfter(nil, mygo.App.Quit), nil
		default:
			return nil, errors.New("unknown panel command")
		}
	}
	owner, err := desktopprocess.Acquire(ctx, dir, desktopprocess.QuickPanel, handler)
	if errors.Is(err, desktopprocess.ErrRunning) {
		return nil
	}
	if err != nil {
		return err
	}
	defer func() { _ = owner.Close() }()
	configureNativeRole(dir, true)
	c, err := client()
	if err != nil {
		return err
	}
	mygo.App.OnWindowAllClosed(func() {})
	mygo.App.OnWillQuit(func(*mygo.QuitEvent) {
		cancel()
		if panel != nil {
			panel.Close()
		}
	})
	mygo.App.WhenReady(func() {
		panel = desktopui.NewProcessPanel(ctx, c, remoteClipboard{dir}, func(action string) {
			nativeAsync(func(ctx context.Context) error {
				return roleCall(ctx, dir, desktopprocess.Tray, "open-gui", action, nil)
			})
		})
		close(ready)
		go func() { <-ctx.Done(); mygo.App.Quit() }()
	})
	return mygo.App.Run()
}
