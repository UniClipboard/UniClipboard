//go:build desktop_e2e

package main

import (
	"context"
	"encoding/json"
	"errors"
	"os"
	"path/filepath"
	"sync"
	"sync/atomic"

	"github.com/uniclipboard/unic/app/desktop/internal/desktopprocess"
	"github.com/uniclipboard/unic/app/desktop/internal/quickpanel"
	"github.com/uniclipboard/unic/app/desktop/internal/setupflow"
	"github.com/uniclipboard/unic/internal/clip"
	"github.com/uniclipboard/unic/internal/clipboardintegration"
	api "github.com/uniclipboard/unic/internal/localapi"
	config "github.com/uniclipboard/unic/internal/localconfig"
	paths "github.com/uniclipboard/unic/internal/localpaths"
)

type clipboardReply struct {
	Mark    string `json:"mark"`
	Failure string `json:"failure"`
}

type clipboardRequest struct {
	ID                string `json:"id"`
	Plain             bool   `json:"plain"`
	Mark              string `json:"mark"`
	InterruptComplete bool   `json:"interrupt_complete,omitempty"`
}

// Only the canonical E2E build can replace native event loops. Role IPC,
// processes, service ownership, adapter protocol and persistence stay real.
func init() {
	if os.Getenv("UNIC_DESKTOP_E2E_PROCESS_FIXTURE") == "1" {
		runRole = runProcessFixture
		capturePublicTarget = func() panelActivation { return panelActivation{Error: "target"} }
	}
}
func runProcessFixture(role desktopprocess.Role, settings *config.Manager) error {
	ctx, cancel := roleContext()
	defer cancel()
	dir, err := paths.Dir()
	if err != nil {
		return err
	}
	socket, err := paths.Socket()
	if err != nil {
		return err
	}
	if role == desktopprocess.Tray {
		r, err := newTrayRuntime(ctx, cancel, settings)
		if err != nil {
			return err
		}
		close(r.nativeReady)
		owner, err := desktopprocess.Acquire(ctx, dir, role, r.handler)
		if errors.Is(err, desktopprocess.ErrRunning) {
			return nil
		}
		if err != nil {
			return err
		}
		defer func() { _ = owner.Close() }()
		defer r.close()
		r.mu.Lock()
		r.state.Shortcut = shortcutStatus{Requested: settings.Current().Desktop.QuickPanelShortcut}
		r.mu.Unlock()
		flow, err := setupflow.Open(dir)
		if err != nil {
			return err
		}
		if !flow.Required() {
			if err = r.start(ctx, false); err != nil {
				return err
			}
		}
		go r.supervise()
		<-ctx.Done()
		return nil
	}
	if role != desktopprocess.GUI && role != desktopprocess.QuickPanel {
		return errors.New("unknown desktop role")
	}
	var mu sync.Mutex
	visible := role == desktopprocess.GUI
	userClosed := false
	var panelBoard *clipboardintegration.PanelClipboard
	var panelWrites atomic.Int64
	if role == desktopprocess.QuickPanel {
		panelBoard = clipboardintegration.NewPanelClipboard(dir, &countingFixtureBoard{base: newDesktopBoard(), writes: &panelWrites})
		defer panelBoard.Close()
	}
	handler := func(ctx context.Context, command string, raw json.RawMessage) (any, error) {
		if role == desktopprocess.QuickPanel {
			switch command {
			case "panel-write-count":
				return panelWrites.Load(), nil
			case "panel-apply":
				var req clipboardRequest
				if err := json.Unmarshal(raw, &req); err != nil {
					return nil, err
				}
				if req.InterruptComplete {
					panelBoard.BeforeComplete = func() error { return errors.New("fixture interrupted completion") }
					defer func() { panelBoard.BeforeComplete = nil }()
				}
				mark, err := panelBoard.Apply(ctx, api.NewClient(socket), req.ID, req.Plain)
				failure := ""
				if err != nil {
					if errors.Is(err, clipboardintegration.ErrUncertain) {
						failure = "uncertain"
					} else {
						failure = quickpanel.FailureCode(err)
					}
				}
				return clipboardReply{Mark: mark, Failure: failure}, nil
			case "panel-open-main":
				var action string
				_ = json.Unmarshal(raw, &action)
				return nil, openMainFromPanel(ctx, dir, socket, action)
			}
		}
		mu.Lock()
		defer mu.Unlock()
		flow, err := setupflow.Open(dir)
		if err != nil {
			return nil, err
		}
		switch command {
		case "status":
			return roleStatus{Ready: !flow.Required(), SetupRequired: flow.Required(), Visible: visible}, nil
		case "activate":
			var act panelActivation
			_ = json.Unmarshal(raw, &act)
			if role == desktopprocess.QuickPanel && visible && act.Action == panelActionToggle {
				visible = false
				return nil, nil
			}
			visible = true
			return nil, nil
		case "focus", "favorites":
			return nil, nil
		case "close":
			visible = false
			if role == desktopprocess.GUI {
				userClosed = true
				return desktopprocess.ReplyAfter(nil, cancel), nil
			}
			return nil, nil
		case "quit":
			if role == desktopprocess.GUI {
				if _, err := os.Stat(filepath.Join(dir, "fixture-reject-gui-quit")); err == nil {
					return nil, errors.New("fixture rejected GUI quit")
				}
			}
			return desktopprocess.ReplyAfter(nil, cancel), nil
		case "complete-setup":
			return nil, desktopprocess.Call(ctx, dir, desktopprocess.Tray, "start-runtime", nil, nil)
		default:
			return nil, errors.New("unknown fixture role command")
		}
	}
	owner, err := desktopprocess.Acquire(ctx, dir, role, handler)
	if errors.Is(err, desktopprocess.ErrRunning) {
		return nil
	}
	if err != nil {
		return err
	}
	defer func() {
		_ = owner.Close()
		mu.Lock()
		closed := userClosed
		mu.Unlock()
		if closed {
			notifyGUIClosed(dir)
		}
	}()
	<-ctx.Done()
	return nil
}

type countingFixtureBoard struct {
	base   clipboardintegration.Board
	writes *atomic.Int64
}

func (b *countingFixtureBoard) Read() (clip.Snapshot, error) { return b.base.Read() }
func (b *countingFixtureBoard) Write(s clip.Snapshot) error {
	b.writes.Add(1)
	return b.base.Write(s)
}
