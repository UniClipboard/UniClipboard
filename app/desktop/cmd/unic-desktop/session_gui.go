package main

import (
	"context"
	"sync"
	"time"

	"github.com/egoist/mygo"
	"github.com/egoist/mygo/ui"

	"github.com/uniclipboard/unic/app/desktop/internal/desktopprocess"
	"github.com/uniclipboard/unic/app/desktop/internal/desktopui"
	"github.com/uniclipboard/unic/app/desktop/internal/i18n"
	"github.com/uniclipboard/unic/app/desktop/internal/setupflow"
	"github.com/uniclipboard/unic/app/desktop/internal/setupui"
	"github.com/uniclipboard/unic/app/desktop/internal/uikit"
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
	pendingAction     string
	workers           sync.WaitGroup
}

func (s *desktopSession) showSetup(migrationOnly bool) {
	s.setup = setupui.New(s.flow, s.settings.Current().Desktop.Language, migrationOnly, s.start)
	s.g.win.SetMinimumSize(600, 480)
	s.g.win.SetContentSize(640, 560)
	s.g.win.Show()
	s.g.win.Focus()
	s.g.win.Invalidate()
}
func (s *desktopSession) view(c *ui.Context) {
	c.OnWindowShortcut(ui.Cmd, ui.KeyQ, mygo.App.Quit)
	if s.setup != nil {
		s.setup.View(c)
		return
	}
	if s.runtime != nil {
		if s.message == "" {
			s.runtime.screen.View(c)
		} else {
			k := uikit.For(c.Theme().Dark)
			ui.Column(c).Fill().Children(func() {
				k.InlineNotice(c, uikit.NoticeProps{Tone: uikit.ToneError, Title: i18n.Translate(s.settings.Current().Desktop.Language, s.message)})
				ui.Column(c).Grow(1).MinHeight(0).Children(func() { s.runtime.screen.View(c) })
			})
		}
		return
	}
	k := uikit.For(c.Theme().Dark)
	k.UiRoot(c, func() {
		ui.Column(c).Fill().Padding(40).Gap(12).Center().Children(func() {
			if s.message == "" {
				ui.Spinner(c)
				k.Text(c, i18n.Translate(s.settings.Current().Desktop.Language, "正在准备应用…"), uikit.TextProps{})
			} else {
				k.InlineNotice(c, uikit.NoticeProps{Tone: uikit.ToneError, Title: i18n.Translate(s.settings.Current().Desktop.Language, s.message)})
				if k.Button(c, i18n.Translate(s.settings.Current().Desktop.Language, "重试"), uikit.ButtonProps{Variant: uikit.Primary, Disabled: s.starting}).Clicked() {
					s.start()
				}
			}
		})
	})
}
func (s *desktopSession) start() {
	if s.starting || s.runtime != nil {
		return
	}
	s.starting, s.message = true, ""
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
			s.flow = flow
			s.setup = nil
			ctx, cancel := context.WithCancel(s.ctx)
			screen := desktopui.NewDesktop(ctx, s.g.c)
			screen.SetWindowToolbar(s.windowFrame, s.windowControls)
			r := &desktopRuntime{screen: screen, cancel: cancel}
			s.runtime = r
			s.navigate(s.pendingAction)
			screen.SetMigrationOpener(s.migrate)
			screen.SetQuickPanelOpener(func(string) {
				nativeAsync(func(ctx context.Context) error {
					return desktopprocess.Call(ctx, s.dir, desktopprocess.Tray, "open-quick-panel", nil, nil)
				})
			})
			screen.SetLoginStatus(func() (bool, string) { s.g.mu.Lock(); defer s.g.mu.Unlock(); return s.g.loginEnabled, s.g.loginError }, func() { s.g.applyLoginSettings(s.settings.Current().Desktop.LaunchAtLogin, true) })
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
			s.g.win.SetMinimumSize(600, 480)
			s.g.win.SetContentSize(desktopWindowWidth, desktopWindowHeight)
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
			s.g.mu.Lock()
			s.g.loginEnabled = state.LoginEnabled
			s.g.loginError = state.LoginError
			s.g.shortcut = state.Shortcut
			s.g.mu.Unlock()
			s.g.win.Invalidate()
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
	}
	s.g.win.Invalidate()
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
				s.message = "无法停止当前后台服务。请退出独立服务后重试迁移。"
				s.g.win.Invalidate()
				return
			}
			flow, flowErr := setupflow.Open(s.dir)
			if flowErr != nil {
				s.message = "无法读取引导状态。请重试。"
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
