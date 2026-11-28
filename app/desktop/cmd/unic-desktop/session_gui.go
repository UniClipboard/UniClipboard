package main

import (
	"context"
	"errors"
	"runtime"
	"sync"
	"time"

	"github.com/egoist/mygo"
	"github.com/egoist/mygo/ui"

	"github.com/uniclipboard/unic/app/desktop/internal/desktopkit"
	"github.com/uniclipboard/unic/app/desktop/internal/desktopprocess"
	"github.com/uniclipboard/unic/app/desktop/internal/desktopui"
	"github.com/uniclipboard/unic/app/desktop/internal/i18n"
	"github.com/uniclipboard/unic/app/desktop/internal/setupflow"
	"github.com/uniclipboard/unic/app/desktop/internal/setupui"
	"github.com/uniclipboard/unic/internal/clipboardintegration"
	config "github.com/uniclipboard/unic/internal/localconfig"
)

type desktopRuntime struct {
	screen *desktopui.Desktop
	cancel context.CancelFunc
}

// desktopSession serializes setup and ordinary runtime ownership on the view
// thread. Only the startup/shutdown worker touches a service in transition.
type desktopSession struct {
	windowFrame       bool
	windowControls    func(*ui.Context)
	g                 *gui
	ctx               context.Context
	settings          *config.Manager
	flow              *setupflow.Flow
	dir               string
	setup             *setupui.Screen
	runtime           *desktopRuntime
	starting, closing bool
	message           string
	toasts            desktopkit.ToastStore
	pendingAction     string
	workers           sync.WaitGroup
}

const (
	setupWindowWidth  = 780
	setupWindowHeight = 540
)

func (s *desktopSession) showSetup(migrationOnly bool) {
	s.setup = setupui.New(s.flow, s.settings.Current().Desktop.Language, migrationOnly, s.start)
	s.g.win.SetMinimumSize(600, 480)
	resizeContentCentered(s.g.win, setupWindowWidth, setupWindowHeight)
	s.g.win.Show()
	s.g.win.Focus()
	s.g.win.Invalidate()
}
func (s *desktopSession) view(c *ui.Context) {
	c.OnWindowShortcut(ui.Cmd, ui.KeyQ, mygo.App.Quit)
	if s.setup != nil {
		s.setup.View(c, s.windowControls)
		return
	}
	if s.runtime != nil {
		s.runtime.screen.View(c)
		return
	}
	lang := s.settings.Current().Desktop.Language
	k := desktopkit.For(c.Theme().Dark)
	k.UiRoot(c, func() {
		ui.Column(c).Fill().Padding(40).Gap(12).Center().Children(func() {
			if s.message == "" {
				ui.Spinner(c)
				k.Text(c, i18n.Translate(lang, "正在准备应用…"), desktopkit.TextProps{})
			} else {
				k.Text(c, i18n.Translate(lang, s.message), desktopkit.TextProps{Tone: desktopkit.ToneSecondary})
				if k.Button(c, i18n.Translate(lang, "重试"), desktopkit.ButtonProps{Variant: desktopkit.Primary, Disabled: s.starting}).Clicked() {
					s.start()
				}
			}
		})
		k.Toaster(c, &s.toasts, desktopkit.ToasterProps{
			Position:   desktopkit.ToastTopCenter,
			Label:      i18n.Translate(lang, "通知"),
			CloseLabel: i18n.Translate(lang, "关闭提示"),
		})
	})
}
func (s *desktopSession) start() {
	if s.starting || s.runtime != nil {
		return
	}
	s.starting, s.message = true, ""
	s.toasts.Dismiss("startup-error")
	s.workers.Add(1)
	go func() {
		defer s.workers.Done()
		err := desktopprocess.Call(s.ctx, s.dir, desktopprocess.Tray, "start-runtime", nil, nil)
		mygo.RunOnMain(func() {
			s.starting = false
			if s.closing || s.ctx.Err() != nil {
				mygo.App.Quit()
				return
			}
			if err != nil {
				s.startFailed()
				return
			}
			flow, flowErr := setupflow.Open(s.dir)
			if flowErr != nil {
				s.startFailed()
				return
			}
			wasSetup := s.setup != nil
			s.flow = flow
			s.setup = nil
			ctx, cancel := context.WithCancel(s.ctx)
			screen := desktopui.NewDesktop(ctx, s.g.c)
			bindStorageActions(screen, s.dir)
			bindDesktopDiagnostics(screen, s.dir, s.g.c, func() string { return s.settings.Current().Desktop.Language })
			bindBackupActions(screen, func() string { return s.settings.Current().Desktop.Language })
			bindDesktopShortcutPermissions(screen)
			bindDesktopNotifications(screen, s.dir)
			screen.SetPendingCountChanged(func(n int) { mygo.App.SetBadgeCount(n) })
			screen.SetWindowToolbar(s.windowFrame, s.windowControls)
			r := &desktopRuntime{screen: screen, cancel: cancel}
			s.runtime = r
			s.navigate(s.pendingAction)
			screen.SetMigrationOpener(s.migrate)
			screen.SetRestarter(s.restart)
			screen.SetDismissedAnnouncements(flow.State().DismissedAnnouncements, s.dismissRemoteAnnouncement)
			screen.SetQuickPanelOpener(func(string) {
				nativeAsync(func(ctx context.Context) error {
					return desktopprocess.Call(ctx, s.dir, desktopprocess.Tray, "open-quick-panel", nil, nil)
				})
			})
			screen.SetLoginStatus(func() (bool, string) { s.g.mu.Lock(); defer s.g.mu.Unlock(); return s.g.loginEnabled, s.g.loginError }, func() { s.g.applyLoginSettings(s.settings.Current().Desktop.LaunchAtLogin, true) })
			if runtime.GOOS == "linux" && useDaemonClipboard {
				fallback := clipboardintegration.NewPanelClipboard(s.dir, newDesktopBoard())
				screen.SetClipboardFallback(func() bool {
					s.g.mu.Lock()
					defer s.g.mu.Unlock()
					return s.g.clipboardUnavailable
				}, func(ctx context.Context, id string) error {
					_, err := fallback.Apply(ctx, s.g.c, id, false)
					return err
				}, fallback.Close)
			}
			screen.SetUpdater(
				func() desktopui.UpdateStatus {
					s.g.mu.Lock()
					defer s.g.mu.Unlock()
					return s.g.updates
				},
				func() {
					nativeAsync(func(ctx context.Context) error {
						return desktopprocess.Call(ctx, s.dir, desktopprocess.Tray, "check-updates", nil, nil)
					})
				},
				func(autoCheck, autoDownload *bool) {
					// Apply optimistically so the UI responds immediately, then reconcile with tray reply.
					s.g.mu.Lock()
					if autoCheck != nil {
						s.g.updates.AutoCheckUpdate = *autoCheck
					}
					if autoDownload != nil {
						s.g.updates.AutoDownloadUpdate = *autoDownload
					}
					s.g.mu.Unlock()
					req := updatePrefsRequest{AutoCheckUpdate: autoCheck, AutoDownloadUpdate: autoDownload}
					nativeAsync(func(ctx context.Context) error {
						var st updateStatus
						err := desktopprocess.Call(ctx, s.dir, desktopprocess.Tray, "set-update-prefs", req, &st)
						if err == nil {
							s.g.mu.Lock()
							s.g.updates = desktopui.UpdateStatus{
								Supported:          st.Supported,
								Owner:              st.Owner,
								AutoCheckUpdate:    st.AutoCheckUpdate,
								AutoDownloadUpdate: st.AutoDownloadUpdate,
								AvailableVersion:   st.AvailableVersion,
							}
							s.g.mu.Unlock()
							if s.g.win != nil {
								s.g.win.Invalidate()
							}
						}
						return err
					})
				},
			)
			screen.SetExternalShortcut(func() (string, string) {
				s.g.mu.Lock()
				defer s.g.mu.Unlock()
				return s.g.shortcut.Effective, s.g.shortcut.Error
			}, func() {
				nativeAsync(func(ctx context.Context) error {
					return desktopprocess.Call(ctx, s.dir, desktopprocess.Tray, "retry-shortcut", nil, nil)
				})
			})
			go s.followHost(ctx)
			if wasSetup {
				s.g.win.SetMinimumSize(600, 480)
				resizeContentCentered(s.g.win, desktopWindowWidth, desktopWindowHeight)
			}
			s.g.win.Invalidate()
		})
	}()
}
func (s *desktopSession) followHost(ctx context.Context) {
	ticker := time.NewTicker(2 * time.Second)
	defer ticker.Stop()
	for {
		var state roleStatus
		probe, cancel := context.WithTimeout(ctx, time.Second)
		err := desktopprocess.Call(probe, s.dir, desktopprocess.Tray, "status", nil, &state)
		cancel()
		if err == nil {
			nextUpdates := desktopui.UpdateStatus{
				Supported:          state.Updates.Supported,
				Owner:              state.Updates.Owner,
				AutoCheckUpdate:    state.Updates.AutoCheckUpdate,
				AutoDownloadUpdate: state.Updates.AutoDownloadUpdate,
				AvailableVersion:   state.Updates.AvailableVersion,
			}
			s.g.mu.Lock()
			changed := s.g.loginEnabled != state.LoginEnabled ||
				s.g.loginError != state.LoginError ||
				s.g.clipboardUnavailable != state.ClipboardUnavailable ||
				s.g.shortcut != state.Shortcut ||
				s.g.updates != nextUpdates
			if changed {
				s.g.loginEnabled = state.LoginEnabled
				s.g.loginError = state.LoginError
				s.g.clipboardUnavailable = state.ClipboardUnavailable
				s.g.shortcut = state.Shortcut
				s.g.updates = nextUpdates
			}
			s.g.mu.Unlock()
			if changed {
				s.g.win.Invalidate()
			}
		}
		select {
		case <-ctx.Done():
			return
		case <-ticker.C:
		}
	}
}
func (s *desktopSession) startFailed() {
	s.message = "无法启动应用。请检查数据目录和后台服务，然后重试。"
	if s.setup != nil {
		s.setup.EnterFailed(s.message)
	} else {
		lang := s.settings.Current().Desktop.Language
		s.toasts.Error(i18n.Translate(lang, s.message), desktopkit.ToastOptions{
			ID:       "startup-error",
			Duration: desktopkit.ToastPersistent,
		})
	}
	s.g.win.Invalidate()
}
func (s *desktopSession) restart(restartService bool, done func(error)) {
	if s.starting || s.runtime == nil {
		if done != nil {
			done(errors.New("desktop session is not ready to restart"))
		}
		return
	}
	command := "restart-gui"
	if restartService {
		command = "restart"
	}
	s.workers.Add(1)
	go func() {
		defer s.workers.Done()
		ctx, cancel := context.WithTimeout(s.ctx, 12*time.Second)
		defer cancel()
		err := desktopprocess.Call(ctx, s.dir, desktopprocess.Tray, command, nil, nil)
		if done != nil && s.ctx.Err() == nil {
			done(err)
		}
	}()
}

func (s *desktopSession) dismissRemoteAnnouncement(id string) {
	s.workers.Add(1)
	go func() {
		defer s.workers.Done()
		flow, err := setupflow.Open(s.dir)
		if err != nil {
			return
		}
		_ = flow.DismissAnnouncement(id)
	}()
}

func (s *desktopSession) migrate() {
	if s.starting || s.runtime == nil {
		return
	}
	s.starting = true
	s.workers.Add(1)
	go func() {
		defer s.workers.Done()
		err := desktopprocess.Call(s.ctx, s.dir, desktopprocess.Tray, "pause-runtime", nil, nil)
		mygo.RunOnMain(func() {
			s.starting = false
			if s.closing {
				mygo.App.Quit()
				return
			}
			if err != nil {
				msg := "无法停止当前后台服务。请退出独立服务后重试迁移。"
				s.runtime.screen.ShowErrorToast(i18n.Translate(s.settings.Current().Desktop.Language, msg))
				s.g.win.Invalidate()
				return
			}
			flow, flowErr := setupflow.Open(s.dir)
			if flowErr != nil {
				msg := "无法读取引导状态。请重试。"
				s.runtime.screen.ShowErrorToast(i18n.Translate(s.settings.Current().Desktop.Language, msg))
				s.g.win.Invalidate()
				return
			}
			s.flow = flow
			s.runtime.screen.Close()
			s.runtime.cancel()
			s.runtime = nil
			s.showSetup(true)
		})
	}()
}

// requestQuit is used by both window close during setup and application quit.
func (s *desktopSession) requestQuit(e *mygo.QuitEvent) {
	if s.starting {
		e.PreventDefault()
		s.closing = true
		return
	}
	if s.setup != nil && !s.setup.RequestClose(func() { s.closing = false; mygo.App.Quit() }) {
		e.PreventDefault()
		s.closing = true
	}

}
func (s *desktopSession) close() {
	s.workers.Wait()
	if r := s.runtime; r != nil {
		r.screen.Close()
		r.cancel()
	}
}

func (s *desktopSession) navigate(action string) {
	s.pendingAction = action
	if s.runtime != nil {
		s.runtime.screen.OpenFromPanel(action)
		s.pendingAction = ""
	}
}

type resizableWindow interface {
	IsMaximized() bool
	IsFullScreen() bool
	Bounds() mygo.Rectangle
	ContentBounds() mygo.Rectangle
	SetContentBounds(mygo.Rectangle)
	SetContentSize(width, height int)
}

func resizeContentCentered(win *mygo.Window, width, height int) {
	if win == nil {
		return
	}
	resizeContentCenteredWith(win, width, height, displayWorkAreaFor)
}

func displayWorkAreaFor(outer mygo.Rectangle) mygo.Rectangle {
	if outer.Width > 0 && outer.Height > 0 {
		if wa := mygo.Screen.DisplayMatching(outer).WorkArea; wa.Width > 0 && wa.Height > 0 {
			return wa
		}
	}
	return mygo.Screen.PrimaryDisplay().WorkArea
}

func resizeContentCenteredWith(win resizableWindow, width, height int, workAreaFor func(mygo.Rectangle) mygo.Rectangle) {
	if win == nil || win.IsMaximized() || win.IsFullScreen() {
		return
	}
	content := win.ContentBounds()
	if content.Width == width && content.Height == height {
		return
	}
	if content.Width <= 0 || content.Height <= 0 {
		win.SetContentSize(width, height)
		return
	}
	outer := win.Bounds()
	var workArea mygo.Rectangle
	if workAreaFor != nil {
		workArea = workAreaFor(outer)
	}
	win.SetContentBounds(centeredContentBounds(outer, content, width, height, workArea))
}

// centeredContentBounds computes target content bounds of size (width, height)
// anchored on the window's current center, clamping the resulting outer frame
// inside workArea when available.
func centeredContentBounds(outer, content mygo.Rectangle, width, height int, workArea mygo.Rectangle) mygo.Rectangle {
	insetLeft := content.X - outer.X
	insetTop := content.Y - outer.Y
	insetRight := (outer.X + outer.Width) - (content.X + content.Width)
	insetBottom := (outer.Y + outer.Height) - (content.Y + content.Height)
	if outer.Width <= 0 || outer.Height <= 0 || insetLeft < 0 || insetTop < 0 || insetRight < 0 || insetBottom < 0 {
		insetLeft, insetTop, insetRight, insetBottom = 0, 0, 0, 0
	}

	target := mygo.Rectangle{
		X:      content.X + (content.Width-width)/2,
		Y:      content.Y + (content.Height-height)/2,
		Width:  width,
		Height: height,
	}

	if workArea.Width > 0 && workArea.Height > 0 {
		targetOuter := mygo.Rectangle{
			X:      target.X - insetLeft,
			Y:      target.Y - insetTop,
			Width:  width + insetLeft + insetRight,
			Height: height + insetTop + insetBottom,
		}
		targetOuter.X = max(min(targetOuter.X, workArea.X+workArea.Width-targetOuter.Width), workArea.X)
		targetOuter.Y = max(min(targetOuter.Y, workArea.Y+workArea.Height-targetOuter.Height), workArea.Y)
		target.X = targetOuter.X + insetLeft
		target.Y = targetOuter.Y + insetTop
	}

	return target
}
