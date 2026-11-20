package e2e

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"runtime"
	"time"
)

// Failure inventory is in docs/exec/desktop-process-failures.md. These commands
// drive native application windows with app-scoped input and a directory board.
// IPC activation does not establish tray clicks or global shortcut delivery.
func nativeProcessCommands() map[string]func(*desktopScenario, []string) error {
	return map[string]func(*desktopScenario, []string) error{
		"windows-frame": func(s *desktopScenario, args []string) error {
			if runtime.GOOS != "windows" || len(args) < 2 || len(args) > 3 || filepath.Base(args[0]) != args[0] {
				return errors.New("windows-frame name custom|system [action] requires Windows")
			}
			raw, err := s.roleCommand("gui", "probe")
			if err != nil {
				return err
			}
			var owner desktopRoleIdentity
			if err := json.Unmarshal(raw, &owner); err != nil {
				return err
			}
			action := "capture"
			if len(args) == 3 {
				action = args[2]
			}
			if action != "capture" && action != "maximized" && action != "restored" && action != "minimized" {
				return errors.New("physical input requires explicit owned-VM replay outside the isolated catalogue")
			}
			if action != "minimized" {
				if err := s.call(s.ctx, "idle", map[string]any{"window": s.window, "timeout": 2000}, nil); err != nil {
					return err
				}
			}
			cmd := exec.CommandContext(s.ctx, "powershell.exe", "-NoProfile", "-ExecutionPolicy", "Bypass", "-File", filepath.Join(s.root, "app", "desktop", "e2e", "windows_chrome.ps1"), "-OwnedProcessId", fmt.Sprint(owner.PID), "-OwnedBinary", owner.BinaryPath, "-Mode", args[1], "-OutputBase", filepath.Join(s.out, args[0]), "-Action", action)
			output, err := cmd.CombinedOutput()
			if err != nil {
				return fmt.Errorf("owned Windows frame: %w: %s", err, output)
			}
			nodes, err := s.tree()
			if err != nil {
				return err
			}
			var toolbar *desktopNode
			captions := map[string]desktopNode{}
			for i := range nodes {
				n := nodes[i]
				if n.Role == "toolbar" && (n.Label == "全部" || n.Label == "通用" || n.Label == "插件") {
					toolbar = &nodes[i]
				}
				if n.Role == "button" && (n.Label == "最小化" || n.Label == "最大化" || n.Label == "还原" || n.Label == "关闭") {
					captions[n.Label] = n
				}
			}
			if args[1] == "system" && len(captions) != 0 {
				return errors.New("system mode contains duplicate custom caption controls")
			}
			if args[1] == "custom" {
				if len(captions) != 3 {
					return fmt.Errorf("custom caption controls=%d, want 3", len(captions))
				}
				if toolbar != nil {
					b := toolbar.Bounds
					if b.Y != 0 || b.Height != 48 {
						return fmt.Errorf("extra native application caption row: %+v", b)
					}
					for label, n := range captions {
						r := n.Bounds
						if r.X < b.X || r.X+r.Width > b.X+b.Width+.1 || r.Y < b.Y || r.Y+r.Height > b.Y+b.Height+.1 {
							return fmt.Errorf("native caption %s outside shared toolbar: %+v", label, r)
						}
					}
					first := captions["最小化"].Bounds.X
					for _, n := range nodes {
						if n.Role != "search" && n.Label != "发送到设备" && n.Label != "切换置顶" && n.Label != "更多操作" {
							continue
						}
						r := n.Bounds
						if r.Y < b.Y+b.Height && (r.Width <= 0 || r.X+r.Width > first+.1) {
							return fmt.Errorf("native history command overlaps caption: %s %+v", n.Label, r)
						}
					}
				}
			}
			treeData, err := json.MarshalIndent(nodes, "", "  ")
			if err != nil {
				return err
			}
			if err := os.WriteFile(filepath.Join(s.out, args[0]+"-tree.json"), treeData, 0600); err != nil {
				return err
			}
			s.result.Screenshots = append(s.result.Screenshots, args[0]+".png")
			s.result.Assertions = append(s.result.Assertions, "OS frame geometry and exact-window capture: "+args[0]+"; action="+action)
			return nil
		},
		"role-checkpoint": func(s *desktopScenario, args []string) error {
			if len(args) != 1 || filepath.Base(args[0]) != args[0] {
				return errors.New("role-checkpoint name")
			}
			return s.nativeRoleCheckpoint(args[0])
		},
		"role-open-panel": func(s *desktopScenario, args []string) error {
			if len(args) != 0 {
				return errors.New("role-open-panel takes no arguments")
			}
			if _, err := s.roleCommand("tray", "open-quick-panel"); err != nil {
				return err
			}
			if err := s.selectNativeRole("quick-panel"); err != nil {
				return err
			}
			return s.nativeRoleCheckpoint("panel-open")
		},
		"role-panel-copy": func(s *desktopScenario, args []string) error {
			if len(args) != 0 || s.automation != filepath.Join(s.home, "automation-panel.sock") {
				return errors.New("role-panel-copy requires the independent panel window")
			}
			// Copy closes the panel. Pause recording before that known window gap.
			s.captureMu.Lock()
			s.capturePaused = true
			s.captureMu.Unlock()
			if err := s.call(s.ctx, "input", map[string]any{"window": s.window, "events": []any{map[string]any{"type": "key", "key": "CmdOrCtrl+C"}}}, nil); err != nil {
				return err
			}
			s.result.Assertions = append(s.result.Assertions, "app-scoped Copy shortcut was delivered to the independent native panel; exact board bytes are asserted separately")
			return nil
		},
		"role-select-gui": func(s *desktopScenario, args []string) error {
			if len(args) != 0 {
				return errors.New("role-select-gui takes no arguments")
			}
			if _, err := s.roleCommand("gui", "activate"); err != nil {
				return err
			}
			return s.selectNativeRole("gui")
		},
		"role-close-gui": func(s *desktopScenario, args []string) error {
			if len(args) > 1 || (len(args) == 1 && args[0] != "caption") {
				return errors.New("role-close-gui [caption]")
			}
			if s.nativeRoleBaseline == nil {
				return errors.New("role-checkpoint must precede GUI close")
			}
			raw, err := s.roleCommand("gui", "probe")
			if err != nil {
				return err
			}
			var current desktopRoleIdentity
			if err = json.Unmarshal(raw, &current); err != nil {
				return err
			}
			wait, release, err := observeProcessExit(current.PID)
			if err != nil {
				return err
			}
			defer release()
			s.captureMu.Lock()
			s.capturePaused = true
			s.captureMu.Unlock()
			if len(args) == 1 {
				node, err := s.find("button", "关闭")
				if err != nil {
					return err
				}
				err = s.call(s.ctx, "input", map[string]any{"window": s.window, "events": []any{map[string]any{"type": "click", "x": node.Bounds.X + node.Bounds.Width/2, "y": node.Bounds.Y + node.Bounds.Height/2}}}, nil)
				if err != nil {
					return err
				}
			} else if _, err := s.roleCommand("gui", "close"); err != nil {
				return err
			}
			ctx, cancel := context.WithTimeout(s.ctx, 10*time.Second)
			defer cancel()
			if err := wait(ctx); err != nil {
				return err
			}
			if _, err := s.roleCommand("gui", "probe"); err == nil {
				return errors.New("GUI role endpoint outlived its closed process")
			}
			if err := s.checkNativeBackground(); err != nil {
				return err
			}
			s.result.Assertions = append(s.result.Assertions, fmt.Sprintf("GUI PID %d exited after close %v; tray and service retained identities", current.PID, args))
			return s.nativeRoleCheckpoint("gui-closed")
		},
		"role-reopen-gui": func(s *desktopScenario, args []string) error {
			if len(args) != 0 {
				return errors.New("role-reopen-gui takes no arguments")
			}
			old, ok := s.nativeRoleBaseline["gui"]
			if !ok {
				return errors.New("GUI baseline identity is missing")
			}
			if _, err := s.roleCommand("tray", "open-gui"); err != nil {
				return err
			}
			if err := s.selectNativeRole("gui"); err != nil {
				return err
			}
			raw, err := s.roleCommand("gui", "probe")
			if err != nil {
				return err
			}
			var current desktopRoleIdentity
			if err = json.Unmarshal(raw, &current); err != nil {
				return err
			}
			if current.PID == old.PID {
				return errors.New("tray activation retained the closed GUI process")
			}
			if err = s.checkNativeBackground(); err != nil {
				return err
			}
			s.result.Assertions = append(s.result.Assertions, fmt.Sprintf("tray IPC recreated native GUI PID %d after old GUI PID %d exited", current.PID, old.PID))
			return s.nativeRoleCheckpoint("gui-reopened")
		},
	}
}

func (s *desktopScenario) selectNativeRole(role string) error {
	endpoint := filepath.Join(s.home, "automation.sock")
	if role == "quick-panel" {
		endpoint = filepath.Join(s.home, "automation-panel.sock")
	}
	return s.wait(func() bool {
		var out struct {
			Windows []struct {
				ID      int
				Kind    string
				Visible bool
			}
		}
		if s.callAt(s.ctx, endpoint, "windows", nil, &out) != nil {
			return false
		}
		for _, window := range out.Windows {
			if window.Kind == "content" && window.Visible {
				s.captureMu.Lock()
				s.automation = endpoint
				s.window = window.ID
				s.capturePaused = false
				if role == "gui" {
					s.mainWindow = window.ID
				}
				s.captureMu.Unlock()
				return true
			}
		}
		return false
	})
}

func (s *desktopScenario) checkNativeBackground() error {
	raw, err := s.roleCommand("tray", "probe")
	if err != nil {
		return err
	}
	var tray desktopRoleIdentity
	if err = json.Unmarshal(raw, &tray); err != nil {
		return err
	}
	if baseline, ok := s.nativeRoleBaseline["tray"]; !ok || tray.PID != baseline.PID || tray.StartedAt != baseline.StartedAt {
		return errors.New("GUI lifecycle changed the tray identity")
	}
	service, err := s.client.Service(s.ctx)
	if err != nil {
		return err
	}
	if service.ID != s.nativeServiceID {
		return errors.New("GUI lifecycle changed the service generation")
	}
	return nil
}

func (s *desktopScenario) nativeRoleCheckpoint(name string) error {
	if s.result.Mode != "native" || s.systemClipboard {
		return errors.New("native role checkpoint requires isolated native directory-board acceptance")
	}
	roles := map[string]desktopRoleIdentity{}
	for _, role := range []string{"tray", "gui", "quick-panel"} {
		raw, err := s.roleCommand(role, "probe")
		if err != nil {
			if role == "tray" {
				return err
			}
			continue
		}
		var identity desktopRoleIdentity
		if err = json.Unmarshal(raw, &identity); err != nil {
			return err
		}
		if identity.PID <= 0 || identity.Role != role || identity.Profile != s.home {
			return errors.New("native role identity did not match its profile")
		}
		roles[role] = identity
	}
	ctx, cancel := context.WithTimeout(s.ctx, 3*time.Second)
	defer cancel()
	service, err := s.client.Service(ctx)
	if err != nil {
		return err
	}
	seen := map[int]string{service.PID: "unicd"}
	for role, identity := range roles {
		if other, ok := seen[identity.PID]; ok {
			return fmt.Errorf("native roles %s and %s share PID %d", role, other, identity.PID)
		}
		seen[identity.PID] = role
	}
	if s.nativeRoleBaseline == nil {
		s.nativeRoleBaseline = roles
		s.nativeServiceID = service.ID
	}
	receipt := map[string]any{"at": time.Now().UTC(), "roles": roles, "service": service, "selected_automation_endpoint": s.automation, "selected_window_id": s.window, "activation": "private role IPC; not a tray click or global shortcut", "input": "application-scoped automation; not physical input", "clipboard": "directory fixture; not system clipboard", "role_start_time_semantics": "role-acquisition timestamp; not kernel creation time"}
	raw, err := json.MarshalIndent(receipt, "", "  ")
	if err != nil {
		return err
	}
	if err = os.WriteFile(filepath.Join(s.out, fmt.Sprintf("roles-%02d-%s.json", len(s.result.Steps), name)), raw, 0600); err != nil {
		return err
	}
	s.result.Assertions = append(s.result.Assertions, "native role checkpoint "+name+" verified distinct OS PIDs and private profile identity")
	return nil
}
