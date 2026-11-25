package e2e

import (
	"bufio"
	"context"
	"database/sql"
	"encoding/base64"
	"encoding/json"
	"errors"
	"fmt"
	"image/png"
	"io"
	"net"
	"os"
	"os/exec"
	"os/signal"
	"path/filepath"
	"regexp"
	"runtime"
	"slices"
	"strconv"
	"strings"
	"sync"
	"syscall"
	"testing"
	"time"

	"github.com/godbus/dbus/v5"
	"github.com/rogpeppe/go-internal/testscript"

	"github.com/uniclipboard/unic/app/desktop/internal/setupflow"
	"github.com/uniclipboard/unic/engine"
	"github.com/uniclipboard/unic/internal/clip"
	api "github.com/uniclipboard/unic/internal/localapi"
	"github.com/uniclipboard/unic/internal/testboard"
	"github.com/uniclipboard/unic/internal/v1migration/v1fixture"
)

type desktopResult struct {
	ElapsedMillis    int64            `json:"elapsed_ms"`
	RecordingStarted float64          `json:"recording_started_seconds,omitempty"`
	CaptureSeconds   []float64        `json:"capture_seconds,omitempty"`
	Status           string           `json:"status"`
	Mode             string           `json:"mode"`
	Failure          string           `json:"failure,omitempty"`
	Assertions       []string         `json:"assertions"`
	Steps            []map[string]any `json:"steps"`
	Screenshots      []string         `json:"screenshots"`
	Recording        string           `json:"recording,omitempty"`
	RecordingEncoder string           `json:"recording_encoder,omitempty"`
	Frames           int              `json:"frames,omitempty"`
	Duration         float64          `json:"duration_seconds,omitempty"`
	Cleanup          string           `json:"cleanup"`
}
type desktopScenario struct {
	externalBackend                bool
	systemClipboard                bool
	clipboardOwners                []*exec.Cmd
	pasteTarget                    *exec.Cmd
	pasteTargetDone                chan error
	idleInhibitor                  *dbus.Conn
	policyPeer                     *pluginPolicyPeer
	captureMu                      sync.Mutex // Do not overlap recording and checkpoint captures.
	capturePaused                  bool       // The GUI process is deliberately absent.
	ctx                            context.Context
	cancel                         context.CancelFunc
	name, root, private, out, home string
	workdir                        string
	windowWidth, windowHeight      int
	windowTheme                    string
	windowSkin                     string
	app                            *exec.Cmd
	appDone                        chan error
	standaloneDaemon               *exec.Cmd
	standaloneDaemonDone           chan error
	client                         *api.Client
	automation                     string
	window                         int
	mainWindow                     int
	nativeRoleBaseline             map[string]desktopRoleIdentity
	nativeServiceID                string
	media                          *desktopRecording
	result                         desktopResult
	started                        time.Time
}

func desktopWorkdir(t *testing.T) string {
	base := os.TempDir()
	if os.PathSeparator == '/' {
		base = "/tmp"
	}
	path, err := os.MkdirTemp(base, "des-")
	if err != nil {
		t.Fatal(err)
	}
	path, err = filepath.EvalSymlinks(path)
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() {
		if err := os.RemoveAll(path); err != nil {
			t.Error(err)
		}
	})
	return path
}
func setupDesktopScenario(env *testscript.Env) error {
	name := strings.TrimPrefix(filepath.Base(env.WorkDir), "script-")
	root, err := filepath.Abs("../../..")
	if err != nil {
		return err
	}
	data, err := os.ReadFile(filepath.Join(root, "app/desktop/e2e/cases", name+".txtar"))
	if err != nil {
		return err
	}
	mode := "process"
	for _, candidate := range []string{"native", "view"} {
		if strings.Contains(string(data), "# Mode: "+candidate+"\n") {
			mode = candidate
		}
	}
	ctx, cancel := signal.NotifyContext(context.Background(), os.Interrupt, syscall.SIGTERM)
	private := filepath.Join(os.Getenv("UNIC_DESKTOP_E2E_PRIVATE"), name)
	out := filepath.Join(os.Getenv("UNIC_DESKTOP_E2E_ARTIFACTS"), name)
	if !filepath.IsAbs(private) || !filepath.IsAbs(out) {
		cancel()
		return errors.New("run through make desktop-e2e")
	}
	if err = os.MkdirAll(private, 0700); err != nil {
		cancel()
		return err
	}
	if err = os.MkdirAll(out, 0700); err != nil {
		cancel()
		return err
	}
	s := &desktopScenario{ctx: ctx, cancel: cancel, name: name, root: root, private: private, out: out, started: time.Now(), result: desktopResult{Status: "FAIL", Mode: mode, Cleanup: "NOT_RUN"}}
	s.workdir = env.WorkDir
	s.systemClipboard = strings.Contains(string(data), "# Environment: disposable-guest\n")
	env.Values["desktop"] = s
	env.Defer(func() { s.finish(env) })
	if mode == "native" && os.Getenv("DBUS_SESSION_BUS_ADDRESS") != "" {
		conn, err := dbus.ConnectSessionBus()
		if err == nil {
			object := conn.Object("org.freedesktop.ScreenSaver", "/ScreenSaver")
			var locked bool
			probe := object.CallWithContext(ctx, "org.freedesktop.ScreenSaver.GetActive", 0).Store(&locked)
			if probe == nil && locked {
				_ = conn.Close()
				return errors.New("native session is locked; no unlock is attempted")
			}
			var cookie uint32
			if probe == nil && object.CallWithContext(ctx, "org.freedesktop.ScreenSaver.Inhibit", 0, "unic Desktop E2E", "Native acceptance in progress").Store(&cookie) == nil {
				s.idleInhibitor = conn
				s.result.Assertions = append(s.result.Assertions, "temporary idle inhibition held only for this native case")
			} else {
				_ = conn.Close()
			}
		}
	}
	if s.systemClipboard && s.idleInhibitor == nil {
		s.result.Failure = "guest acceptance requires an unlocked session with temporary idle inhibition"
		cancel()
		return errors.New(s.result.Failure)
	}
	return nil
}
func desktopState(ts *testscript.TestScript) *desktopScenario {
	return ts.Value("desktop").(*desktopScenario)
}
func (s *desktopScenario) step(name string, args []string, err error) {
	status := "PASS"
	if err != nil {
		status = "FAIL"
		s.result.Failure = err.Error()
	}
	s.result.Steps = append(s.result.Steps, map[string]any{"at_ms": time.Since(s.started).Milliseconds(), "command": name, "args": args, "status": status})
}
func desktopCommands() map[string]func(*testscript.TestScript, bool, []string) {
	handlers := map[string]func(*desktopScenario, []string) error{
		"start": func(s *desktopScenario, a []string) error {
			if len(a) != 0 {
				if len(a) != 3 && len(a) != 4 {
					return errors.New("start [width height light|dark [skin]]")
				}
				var err error
				s.windowWidth, err = strconv.Atoi(a[0])
				if err != nil || s.windowWidth < 600 {
					return errors.New("invalid window width")
				}
				s.windowHeight, err = strconv.Atoi(a[1])
				if err != nil || s.windowHeight < 480 {
					return errors.New("invalid window height")
				}
				if a[2] != "light" && a[2] != "dark" {
					return errors.New("invalid window theme")
				}
				s.windowTheme = a[2]
				s.windowSkin = "macos"
				if len(a) == 4 {
					skin := a[3]
					if skin == "native" {
						skin = runtime.GOOS
						if skin == "darwin" {
							skin = "macos"
						}
					}
					if skin != "windows" && skin != "macos" && skin != "linux" {
						return errors.New("invalid window skin")
					}
					s.windowSkin = skin
				}
			}
			return s.start()
		},
		"start-error": func(s *desktopScenario, a []string) error {
			if len(a) != 0 {
				return errors.New("start-error takes no arguments")
			}
			return s.startNative("startup-error")
		},
		"start-setup": func(s *desktopScenario, a []string) error {
			if len(a) != 1 || (a[0] != "complete" && a[0] != "partial") {
				return errors.New("start-setup complete|partial")
			}
			return s.startNative(a[0])
		},
		"observe": func(s *desktopScenario, a []string) error {
			if s.result.Mode != "native" || len(a) != 1 {
				return errors.New("observe seconds requires a native case")
			}
			seconds, err := strconv.Atoi(a[0])
			if err != nil || seconds < 1 || seconds > 5 {
				return errors.New("observe requires 1 to 5 seconds")
			}
			timer := time.NewTimer(time.Duration(seconds) * time.Second)
			defer timer.Stop()
			select {
			case <-timer.C:
				return nil
			case <-s.ctx.Done():
				return s.ctx.Err()
			}
		},
		"service-absent": func(s *desktopScenario, a []string) error {
			if len(a) != 0 {
				return errors.New("service-absent takes no arguments")
			}
			_, err := s.client.Service(s.ctx)
			if !errors.Is(err, api.ErrNoDaemon) {
				return fmt.Errorf("service exists before setup entry: %v", err)
			}
			s.result.Assertions = append(s.result.Assertions, "no service runs before setup entry")
			return nil
		},
		"history-count": func(s *desktopScenario, a []string) error {
			if len(a) != 1 {
				return errors.New("history-count expected")
			}
			expected, err := strconv.Atoi(a[0])
			if err != nil {
				return err
			}
			var actual uint64
			err = s.wait(func() bool {
				counts, e := s.client.CountHistorySearches(s.ctx, engine.HistorySearchCountsRequest{Queries: []engine.HistorySearchQuery{{}}})
				if e != nil || len(counts.Counts) != 1 {
					return false
				}
				actual = counts.Counts[0]
				return actual == uint64(expected)
			})
			if err != nil {
				return fmt.Errorf("history count: got %d, expected %d: %w", actual, expected, err)
			}
			s.result.Assertions = append(s.result.Assertions, fmt.Sprintf("production history contains exactly %d migrated records", expected))
			return nil
		},
		"submit": func(s *desktopScenario, a []string) error {
			if len(a) != 2 {
				return errors.New("submit id text")
			}
			_, err := s.client.Submit(s.ctx, api.SubmitInput{RequestID: a[0], Snapshot: clip.Snapshot{Parts: []clip.Part{{Format: clip.Text, Data: []byte(a[1])}}}})
			return err
		},
		"ui-toolbar-contained": func(s *desktopScenario, a []string) error {
			if len(a) != 0 {
				return errors.New("ui-toolbar-contained takes no arguments")
			}
			nodes, err := s.tree()
			if err != nil {
				return err
			}
			var bar *desktopNode
			for i := range nodes {
				if nodes[i].Role == "toolbar" && nodes[i].Label == "全部" {
					bar = &nodes[i]
					break
				}
			}
			if bar == nil {
				return errors.New("history toolbar missing")
			}
			if s.windowWidth > 0 && bar.Bounds.X+bar.Bounds.Width != float64(s.windowWidth) {
				return fmt.Errorf("actual window width=%v, want %d", bar.Bounds.X+bar.Bounds.Width, s.windowWidth)
			}
			if bar.Bounds.Height != 52 {
				return fmt.Errorf("toolbar height=%v, want 52", bar.Bounds.Height)
			}
			var controls []desktopNode
			takeover := false
			for _, n := range nodes {
				if n.Role == "button" && n.Label == "收起搜索" {
					takeover = true
				}
				if n.Label != "显示方式" && n.Label != "发送到设备" && n.Label != "切换收藏" && n.Label != "动作" && n.Label != "更多操作" && n.Label != "收起搜索" && n.Label != "最小化" && n.Label != "最大化" && n.Label != "还原" && n.Label != "关闭" && n.Role != "search" {
					continue
				}
				b := n.Bounds
				if b.Y >= bar.Bounds.Y+bar.Bounds.Height {
					continue
				}
				if b.X < bar.Bounds.X || b.X+b.Width > bar.Bounds.X+bar.Bounds.Width+.1 || b.Y < bar.Bounds.Y+8 || b.Y+b.Height > bar.Bounds.Y+bar.Bounds.Height-8+.1 {
					return fmt.Errorf("%s outside toolbar insets: control=%+v toolbar=%+v", n.Label, b, bar.Bounds)
				}
				controls = append(controls, n)
			}
			slices.SortFunc(controls, func(a, b desktopNode) int {
				if a.Bounds.X < b.Bounds.X {
					return -1
				}
				if a.Bounds.X > b.Bounds.X {
					return 1
				}
				return 0
			})
			for i := 1; i < len(controls); i++ {
				prev, cur := controls[i-1], controls[i]
				if prev.Bounds.X+prev.Bounds.Width > cur.Bounds.X+.1 {
					return fmt.Errorf("toolbar controls overlap: %s (%+v) overlaps %s (%+v)", prev.Label, prev.Bounds, cur.Label, cur.Bounds)
				}
			}
			if takeover {
				hasSearch, hasClose := false, false
				for _, n := range controls {
					if n.Role == "search" {
						hasSearch = true
					}
					if n.Label == "收起搜索" {
						hasClose = true
					}
				}
				if !hasSearch || !hasClose {
					return fmt.Errorf("narrow search takeover missing controls: search=%v close=%v", hasSearch, hasClose)
				}
			} else {
				core := 0
				for _, n := range controls {
					if n.Label == "显示方式" || n.Label == "发送到设备" || n.Label == "切换收藏" || n.Label == "更多操作" || n.Role == "search" {
						core++
					}
				}
				if core != 5 {
					return fmt.Errorf("toolbar controls=%d, want 5", core)
				}
			}
			data, err := json.MarshalIndent(nodes, "", "  ")
			if err != nil {
				return err
			}
			if err := os.WriteFile(filepath.Join(s.out, fmt.Sprintf("toolbar-%02d.json", len(s.result.Steps))), append(data, '\n'), 0644); err != nil {
				return err
			}
			s.result.Assertions = append(s.result.Assertions, fmt.Sprintf("all toolbar controls fit without overlap in the 52-point band at %dx%d, %s", s.windowWidth, s.windowHeight, s.windowTheme))
			return nil
		},
		"ui-idle": func(s *desktopScenario, a []string) error {
			if len(a) != 0 {
				return errors.New("ui-idle takes no arguments")
			}
			if err := s.call(s.ctx, "idle", map[string]any{"window": s.window, "timeout": 2000}, nil); err != nil {
				return err
			}
			s.result.Assertions = append(s.result.Assertions, "native UI settles without perpetual frame requests")
			return nil
		},
		"ui-wait":   func(s *desktopScenario, a []string) error { return s.expect(a, false) },
		"ui-absent": func(s *desktopScenario, a []string) error { return s.expect(a, true) },
		"ui-invalid": func(s *desktopScenario, a []string) error {
			if len(a) != 2 {
				return errors.New("ui-invalid role label")
			}
			err := s.wait(func() bool {
				node, err := s.find(a[0], a[1])
				return err == nil && slices.Contains(node.States, "invalid")
			})
			if err == nil {
				s.result.Assertions = append(s.result.Assertions, fmt.Sprintf("%s %q is invalid", a[0], a[1]))
			}
			return err
		},
		"ui-disabled": func(s *desktopScenario, a []string) error {
			if len(a) != 2 {
				return errors.New("ui-disabled role label")
			}
			stable := time.Time{}
			err := s.wait(func() bool {
				node, err := s.find(a[0], a[1])
				if err == nil {
					for _, state := range node.States {
						if state == "disabled" {
							if stable.IsZero() {
								stable = time.Now()
							}
							return time.Since(stable) >= 350*time.Millisecond
						}
					}
				}
				stable = time.Time{}
				return false
			})
			if err == nil {
				s.result.Assertions = append(s.result.Assertions, fmt.Sprintf("%s %q is disabled", a[0], a[1]))
			}
			return err
		},
		"ui-enabled": func(s *desktopScenario, a []string) error {
			if len(a) != 2 {
				return errors.New("ui-enabled role label")
			}
			var previous desktopNode
			stable := time.Time{}
			err := s.wait(func() bool {
				node, err := s.find(a[0], a[1])
				if err != nil {
					stable = time.Time{}
					return false
				}
				for _, state := range node.States {
					if state == "disabled" {
						stable = time.Time{}
						return false
					}
				}
				if stable.IsZero() || node.ID != previous.ID || node.Bounds != previous.Bounds {
					stable, previous = time.Now(), node
				}
				return time.Since(stable) >= 350*time.Millisecond
			})
			if err == nil {
				s.result.Assertions = append(s.result.Assertions, fmt.Sprintf("%s %q is enabled", a[0], a[1]))
			}
			return err
		},
		"ui-set": func(s *desktopScenario, a []string) error {
			if len(a) != 3 {
				return errors.New("ui-set role label text")
			}
			node, err := s.find(a[0], a[1])
			if err != nil {
				return err
			}
			return s.call(s.ctx, "act", map[string]any{"window": s.window, "id": node.ID, "action": "setValue", "text": a[2]}, nil)
		},
		"ui-reveal": func(s *desktopScenario, a []string) error {
			if len(a) != 2 {
				return errors.New("ui-reveal role label")
			}
			node, err := s.find(a[0], a[1])
			if err != nil {
				return err
			}
			if err := s.call(s.ctx, "act", map[string]any{"window": s.window, "id": node.ID, "action": "scrollIntoView"}, nil); err != nil {
				return err
			}
			return s.wait(func() bool {
				node, err := s.find(a[0], a[1])
				if err != nil {
					return false
				}
				return node.Bounds.Y >= 0 && (s.windowHeight == 0 || node.Bounds.Y+node.Bounds.Height <= float64(s.windowHeight))
			})
		},
		"ui-click": func(s *desktopScenario, a []string) error {
			if len(a) != 2 {
				return errors.New("ui-click role label")
			}
			// Serialize against background recording captures so a slow software
			// CapturePage() cannot stall between sampling bounds and delivering
			// the click, and wait for the target element's bounds to settle when
			// an enter/layout transition (such as a Sheet scale-in) is animating.
			s.captureMu.Lock()
			defer s.captureMu.Unlock()
			node, err := s.findForClick(a[0], a[1])
			if err != nil {
				return err
			}
			return s.call(s.ctx, "input", map[string]any{"window": s.window, "events": []any{map[string]any{"type": "click", "x": node.Bounds.X + node.Bounds.Width/2, "y": node.Bounds.Y + node.Bounds.Height/2}}}, nil)
		},
		"ui-key": func(s *desktopScenario, a []string) error {
			if len(a) != 1 {
				return errors.New("ui-key accelerator")
			}
			return s.call(s.ctx, "input", map[string]any{"window": s.window, "events": []any{map[string]any{"type": "key", "key": a[0]}}}, nil)
		},
		"screenshot": func(s *desktopScenario, a []string) error {
			if len(a) != 1 || filepath.Base(a[0]) != a[0] {
				return errors.New("screenshot needs one simple name")
			}
			return s.screenshot(a[0])
		},
		"board-equals": func(s *desktopScenario, a []string) error {
			if len(a) != 1 {
				return errors.New("board-equals text")
			}
			err := s.wait(func() bool {
				v, e := testboard.Dir(filepath.Join(s.home, "board")).Read()
				return e == nil && len(v.Parts) == 1 && v.Parts[0].Format == clip.Text && string(v.Parts[0].Data) == a[0]
			})
			if err == nil {
				s.result.Assertions = append(s.result.Assertions, "directory clipboard equals the selected history bytes")
			}
			return err
		},
		"acceptance": func(s *desktopScenario, a []string) error {
			if len(a) != 2 {
				return errors.New("acceptance package scenario-id")
			}
			return s.acceptance(a[0], a[1])
		},
	}
	for name, handler := range nativeProcessCommands() {
		handlers[name] = handler
	}
	for name, handler := range builtinPluginCommands() {
		handlers[name] = handler
	}
	for name, handler := range systemClipboardCommands() {
		handlers[name] = handler
	}
	result := map[string]func(*testscript.TestScript, bool, []string){}
	for name, handler := range handlers {
		result[name] = func(ts *testscript.TestScript, neg bool, args []string) {
			s := desktopState(ts)
			var err error
			if neg {
				err = errors.New("use ui-absent for negative assertions")
			} else {
				err = handler(s, args)
			}
			s.step(name, args, err)
			if err != nil {
				ts.Fatalf("%s: %v", name, err)
			}
		}
	}
	return result
}
func (s *desktopScenario) wait(check func() bool) error { return s.waitFor(20*time.Second, check) }
func (s *desktopScenario) waitFor(limit time.Duration, check func() bool) error {
	timeout := time.NewTimer(limit)
	defer timeout.Stop()
	tick := time.NewTicker(60 * time.Millisecond)
	defer tick.Stop()
	for {
		if check() {
			return nil
		}
		select {
		case <-s.ctx.Done():
			return s.ctx.Err()
		case <-timeout.C:
			return fmt.Errorf("condition did not become true within %s", limit)
		case <-tick.C:
		}
	}
}
func (s *desktopScenario) start() error { return s.startNative("") }
func (s *desktopScenario) startNative(migration string) error {
	if s.result.Mode != "native" {
		return errors.New("only native cases may start a desktop window")
	}
	if s.app != nil {
		return errors.New("desktop already started")
	}
	s.home = filepath.Join(s.private, "home")
	if err := os.MkdirAll(s.home, 0700); err != nil {
		return err
	}
	if err := os.WriteFile(filepath.Join(s.home, "e2e-owner"), []byte("desktop-e2e"), 0600); err != nil {
		return err
	}
	config := "[history]\nauto_cleanup=false\n[telemetry]\nenabled=false\n[sync]\nenabled=false\n[desktop]\nmode='normal'\nlaunch_at_login=false\nstart_in_background=false\nquick_panel_shortcut=''\n"
	if s.externalBackend {
		config += "[plugins.instances.\"builtin.tailcat\"]\nenabled=false\n"
		if err := s.seedExternalPlugin(s.home); err != nil {
			return err
		}
	}
	if err := os.WriteFile(filepath.Join(s.home, "config.toml"), []byte(config), 0600); err != nil {
		return err
	}
	if migration == "" || migration == "startup-error" {
		if s.systemClipboard {
			initialize := exec.CommandContext(s.ctx, filepath.Join(os.Getenv("UNIC_DESKTOP_E2E_BIN"), "unic"), "migrate", "--initialize-empty", "--json")
			initialize.Env = append(os.Environ(), "UNIC_HOME="+s.home, "XDG_DATA_HOME="+filepath.Join(s.private, "discovery"), "UC_PROFILE=")
			if output, err := initialize.CombinedOutput(); err != nil {
				return fmt.Errorf("initialize task guest profile: %w (%s)", err, output)
			}
			s.result.Assertions = append(s.result.Assertions, "task guest profile explicitly initialized through production CLI")
		}
		flow, err := setupflow.Open(s.home)
		if err != nil {
			return err
		}
		if err = flow.Complete(); err != nil {
			return err
		}
	} else {
		_, dbpath, fixture, err := v1fixture.Build(filepath.Join(s.private, "v1-source"))
		if err != nil {
			return err
		}
		db, err := sql.Open("sqlite", dbpath)
		if err != nil {
			return err
		}
		defer func() { _ = db.Close() }()
		if migration == "complete" {
			_, err = db.Exec("INSERT INTO clipboard_snapshot_representation VALUES('rep-bad','event-bad','text/plain','text/plain',?,NULL,9)", []byte(fixture.Repaired))
			if err != nil {
				return err
			}
		}
		tx, err := db.Begin()
		if err != nil {
			return err
		}
		// Rollback is best-effort cleanup after early failure; after Commit it
		// returns sql.ErrTxDone because the transaction has already finished.
		defer func() { _ = tx.Rollback() }()
		for i := 0; i < 1500; i++ {
			_, err = tx.Exec("INSERT INTO clipboard_entry VALUES(?, 'event-text', ?, ?, 'text', NULL)", fmt.Sprintf("native-%04d", i), int64(10000+i), int64(10000+i))
			if err != nil {
				return err
			}
		}
		if err = tx.Commit(); err != nil {
			return err
		}
		s.result.Assertions = append(s.result.Assertions, "isolated encrypted v1 fixture with 1505 records; synthetic system credentials unavailable")
	}

	if migration == "startup-error" {
		if err := os.Mkdir(filepath.Join(s.home, "unic.db"), 0700); err != nil {
			return err
		}
		s.result.Assertions = append(s.result.Assertions, "owned database-path directory forces a real service startup failure without personal data")
	}
	if err := s.launchNative(); err != nil {
		return err
	}
	if migration == "" {
		if err := s.wait(func() bool { _, e := s.client.Status(s.ctx); return e == nil }); err != nil {
			return err
		}
		s.result.Assertions = append(s.result.Assertions, "an isolated native content window and production service are running")
	} else {
		s.result.Assertions = append(s.result.Assertions, "a real native setup window is running before service startup")
	}
	s.media = newDesktopRecording(s)
	return s.screenshot("started")
}

// launchNative reopens the existing private profile without rewriting configuration.
func (s *desktopScenario) launchNative() error {
	s.automation = filepath.Join(s.home, "automation.sock")
	binary := filepath.Join(os.Getenv("UNIC_DESKTOP_E2E_BIN"), "unic-desktop")
	if os.PathSeparator == '\\' {
		binary += ".exe"
	}
	s.app = exec.Command(binary, "--role", "gui")
	for _, env := range os.Environ() {
		if !strings.HasPrefix(env, "UNIC_") && !strings.HasPrefix(env, "MYGO_AUTOMATION=") {
			s.app.Env = append(s.app.Env, env)
		}
	}
	if s.windowWidth > 0 {
		s.app.Env = append(s.app.Env, fmt.Sprintf("UNIC_DESKTOP_E2E_WINDOW=%dx%d", s.windowWidth, s.windowHeight), "UNIC_DESKTOP_E2E_THEME="+s.windowTheme)
	}
	s.app.Env = append(s.app.Env, "UNIC_HOME="+s.home, "UNIC_DESKTOP_E2E_PROFILE="+s.home, "MYGO_AUTOMATION="+s.automation, "UNIC_RENDEZVOUS_URL=http://127.0.0.1:1")
	if s.systemClipboard {
		s.app.Env = append(s.app.Env, "UNIC_DESKTOP_E2E_GUEST="+os.Getenv("UNIC_DESKTOP_E2E_GUEST"))
	}
	log, err := os.OpenFile(filepath.Join(s.private, "desktop.log"), os.O_CREATE|os.O_WRONLY|os.O_APPEND, 0600)
	if err != nil {
		return err
	}
	s.app.Stdout = log
	s.app.Stderr = log
	if err = s.app.Start(); err != nil {
		_ = log.Close()
		return err
	}
	s.appDone = make(chan error, 1)
	go func() { err := s.app.Wait(); _ = log.Close(); s.appDone <- err }()
	s.client = api.NewClient(filepath.Join(s.home, "unic.sock"))
	err = s.wait(func() bool {
		var result struct {
			Windows []struct {
				ID      int
				Kind    string
				Visible bool
			}
		}
		if s.call(s.ctx, "windows", nil, &result) != nil {
			return false
		}
		for _, w := range result.Windows {
			if w.Visible && w.Kind == "content" {
				s.window = w.ID
				s.mainWindow = w.ID
				return true
			}
		}
		return false
	})
	if err != nil {
		return fmt.Errorf("native window: %w", err)
	}
	return nil
}

type desktopNode struct {
	ID                 string
	Role, Label, Value string
	States             []string
	Bounds             struct{ X, Y, Width, Height float64 }
}

func (s *desktopScenario) tree() ([]desktopNode, error) {
	var out struct{ Nodes []desktopNode }
	err := s.call(s.ctx, "tree", map[string]any{"window": s.window}, &out)
	return out.Nodes, err
}
func (s *desktopScenario) find(role, label string) (desktopNode, error) {
	match, err := desktopLabel(label)
	if err != nil {
		return desktopNode{}, err
	}
	nodes, err := s.tree()
	if err != nil {
		return desktopNode{}, err
	}
	var found []desktopNode
	for _, n := range nodes {
		if n.Role == role && match(n.Label) {
			found = append(found, n)
		}
	}
	if len(found) != 1 {
		return desktopNode{}, fmt.Errorf("expected one %s named %q; found %d", role, label, len(found))
	}
	return found[0], nil
}
func (s *desktopScenario) findForClick(role, label string) (desktopNode, error) {
	if s.call(s.ctx, "idle", map[string]any{"window": s.window, "timeout": 15}, nil) == nil {
		return s.find(role, label)
	}
	var prev, target desktopNode
	stable := time.Time{}
	err := s.waitFor(3*time.Second, func() bool {
		if s.call(s.ctx, "idle", map[string]any{"window": s.window, "timeout": 15}, nil) == nil {
			node, err := s.find(role, label)
			if err != nil {
				stable = time.Time{}
				return false
			}
			target = node
			return true
		}
		node, err := s.find(role, label)
		if err != nil {
			stable = time.Time{}
			return false
		}
		if stable.IsZero() || node.ID != prev.ID || node.Bounds != prev.Bounds {
			prev, stable = node, time.Now()
			return false
		}
		if time.Since(stable) >= 400*time.Millisecond {
			target = node
			return true
		}
		return false
	})
	if err != nil {
		return s.find(role, label)
	}
	return target, nil
}
func (s *desktopScenario) expect(args []string, absent bool) error {
	if len(args) < 2 || len(args) > 3 {
		return errors.New("ui assertion requires role, exact label and optional timeout seconds")
	}
	limit := 20 * time.Second
	if len(args) == 3 {
		seconds, err := strconv.Atoi(args[2])
		if err != nil || seconds < 1 || seconds > 120 {
			return errors.New("timeout must be 1 to 120 seconds")
		}
		limit = time.Duration(seconds) * time.Second
	}
	match, err := desktopLabel(args[1])
	if err != nil {
		return err
	}
	type nodeSnap struct {
		ID     string
		Bounds struct{ X, Y, Width, Height float64 }
	}
	var previous []nodeSnap
	stable := time.Time{}
	err = s.waitFor(limit, func() bool {
		nodes, e := s.tree()
		if e != nil {
			return false
		}
		var matched []nodeSnap
		for _, n := range nodes {
			if n.Role == args[0] && match(n.Label) {
				matched = append(matched, nodeSnap{ID: n.ID, Bounds: n.Bounds})
			}
		}
		present := len(matched) > 0
		if present == absent {
			previous = nil
			stable = time.Time{}
			return false
		}
		if !absent {
			if stable.IsZero() || !slices.Equal(matched, previous) {
				previous = matched
				stable = time.Now()
				return false
			}
		} else if stable.IsZero() {
			stable = time.Now()
		}
		return time.Since(stable) >= 350*time.Millisecond
	})
	if err == nil {
		s.result.Assertions = append(s.result.Assertions, fmt.Sprintf("%s %q present=%t", args[0], args[1], !absent))
	}
	return err
}
func (s *desktopScenario) call(ctx context.Context, method string, params any, out any) error {
	return s.callAt(ctx, s.automation, method, params, out)
}
func (s *desktopScenario) callAt(ctx context.Context, endpoint, method string, params any, out any) error {
	if err := ctx.Err(); err != nil {
		return err
	}
	started := time.Now()
	deadline := 8 * time.Second
	if method == "capture" {
		started = time.Now()
		// Bound raster capture separately from semantic UI commands.
		deadline = 30 * time.Second
		defer func() { s.result.CaptureSeconds = append(s.result.CaptureSeconds, time.Since(started).Seconds()) }()
	}
	connection, err := (&net.Dialer{Timeout: time.Second}).DialContext(ctx, "unix", endpoint)
	if err != nil {
		return err
	}
	defer func() { _ = connection.Close() }()
	if err = connection.SetDeadline(time.Now().Add(deadline)); err != nil {
		return err
	}
	if err = json.NewEncoder(connection).Encode(map[string]any{"id": 1, "method": method, "params": params}); err != nil {
		return err
	}
	var response struct {
		Result json.RawMessage
		Error  *struct{ Code, Message string }
	}
	if err = json.NewDecoder(connection).Decode(&response); err != nil {
		return err
	}
	if response.Error != nil {
		return fmt.Errorf("automation %s: %s", response.Error.Code, response.Error.Message)
	}
	if out != nil {
		return json.Unmarshal(response.Result, out)
	}
	return nil
}
func (s *desktopScenario) capture() (data []byte, err error) {
	s.captureMu.Lock()
	defer s.captureMu.Unlock()
	if s.capturePaused {
		return nil, errCapturePaused
	}
	var out struct{ PNG string }
	if err = s.call(s.ctx, "capture", map[string]any{"window": s.window}, &out); err != nil {
		return nil, err
	}
	data, err = base64.StdEncoding.DecodeString(out.PNG)
	if err != nil {
		return nil, err
	}
	config, err := png.DecodeConfig(strings.NewReader(string(data)))
	if err != nil || config.Width < 100 || config.Height < 100 {
		return nil, errors.New("invalid window screenshot")
	}
	return data, nil
}
func (s *desktopScenario) screenshot(name string) error {
	data, err := s.capture()
	if err != nil {
		return err
	}
	file := fmt.Sprintf("%02d-%s.png", len(s.result.Screenshots)+1, name)
	if err = os.WriteFile(filepath.Join(s.out, file), data, 0600); err != nil {
		return err
	}
	s.result.Screenshots = append(s.result.Screenshots, file)
	if s.windowWidth > 0 {
		config, err := png.DecodeConfig(strings.NewReader(string(data)))
		if err != nil {
			return err
		}
		metadata, err := json.MarshalIndent(map[string]any{"content_width": s.windowWidth, "content_height": s.windowHeight, "theme": s.windowTheme, "skin": s.windowSkin, "pixel_width": config.Width, "pixel_height": config.Height, "capture_scale": float64(config.Width) / float64(s.windowWidth)}, "", "  ")
		if err != nil {
			return err
		}
		if err := os.WriteFile(filepath.Join(s.out, file+".json"), append(metadata, '\n'), 0600); err != nil {
			return err
		}
	}
	return nil
}
func (s *desktopScenario) finish(env *testscript.Env) {
	failed := true
	if t, ok := env.T().(interface{ Failed() bool }); ok {
		failed = t.Failed()
	}
	if failed && s.window != 0 {
		_ = s.screenshot("failure")
		if nodes, err := s.tree(); err == nil {
			b, _ := json.MarshalIndent(nodes, "", "  ")
			_ = os.WriteFile(filepath.Join(s.out, "failure-tree.json"), b, 0600)
		}
	}
	if s.media != nil {
		if err := s.media.stop(); err != nil {
			failed = true
			if s.result.Failure != "" {
				s.result.Failure += "\nrecording: " + err.Error()
			} else {
				s.result.Failure = err.Error()
			}
		}
	}
	if s.result.Mode == "native" && (s.window == 0 || len(s.result.Screenshots) == 0 || s.result.Recording == "" || len(s.result.Assertions) == 0) {
		failed = true
		if s.result.Failure == "" {
			s.result.Failure = "required native evidence or assertions missing"
		}
	}
	s.cancel()
	cleanupErr := s.stopApp()
	if s.pasteTarget != nil && s.pasteTarget.Process != nil {
		if err := s.pasteTarget.Process.Kill(); err != nil && !errors.Is(err, os.ErrProcessDone) {
			cleanupErr = errors.Join(cleanupErr, err)
		}
		select {
		case err := <-s.pasteTargetDone:
			var exit *exec.ExitError
			if err != nil && !errors.As(err, &exit) {
				cleanupErr = errors.Join(cleanupErr, err)
			}
		case <-time.After(5 * time.Second):
			cleanupErr = errors.Join(cleanupErr, errors.New("owned paste target did not exit"))
		}
	}
	for _, owner := range s.clipboardOwners {
		_ = owner.Process.Kill()
		_ = owner.Wait()
	}
	if s.policyPeer != nil {
		cleanupErr = errors.Join(cleanupErr, s.policyPeer.stop())
	}
	if s.idleInhibitor != nil {
		cleanupErr = errors.Join(cleanupErr, s.idleInhibitor.Close())
	}
	if cleanupErr != nil {
		failed = true
		if s.result.Failure == "" {
			s.result.Failure = cleanupErr.Error()
		} else {
			s.result.Failure += "\ncleanup: " + cleanupErr.Error()
		}
		s.result.Cleanup = "FAIL"
	} else {
		s.result.Cleanup = "PASS"
	}
	if !failed {
		s.result.Status = "PASS"
	}
	s.result.ElapsedMillis = time.Since(s.started).Milliseconds()
	data, err := json.MarshalIndent(s.result, "", "  ")
	if err == nil {
		err = os.WriteFile(filepath.Join(s.out, "result.json"), data, 0600)
	}
	if err != nil {
		env.T().Fatal("cannot write Desktop result: ", err)
	}
	if failed {
		env.T().Fatal("Desktop scenario failed: ", s.result.Failure)
	}
}
func (s *desktopScenario) roleCommand(role, command string) ([]byte, error) {
	binPath := ""
	var baseEnv []string
	if s.app != nil {
		binPath = s.app.Path
		baseEnv = s.app.Env
	} else if s.standaloneDaemon != nil {
		binPath = filepath.Join(os.Getenv("UNIC_DESKTOP_E2E_BIN"), "unic-desktop")
		if os.PathSeparator == '\\' {
			binPath += ".exe"
		}
		baseEnv = s.standaloneDaemon.Env
	} else {
		return nil, errors.New("Desktop has not started")
	}
	ctx, cancel := context.WithTimeout(context.Background(), 15*time.Second)
	defer cancel()
	cmd := exec.CommandContext(ctx, binPath, "--role", role, "--command", command)
	for _, entry := range baseEnv {
		if !strings.HasPrefix(entry, "MYGO_AUTOMATION=") {
			cmd.Env = append(cmd.Env, entry)
		}
	}
	cmd.Env = append(cmd.Env, "MYGO_AUTOMATION=")
	return cmd.CombinedOutput()
}
func (s *desktopScenario) stopApp() error {
	if s.appDone == nil && s.standaloneDaemonDone == nil {
		return nil
	}
	if s.appDone == nil && s.standaloneDaemonDone != nil {
		var cleanupErr error
		_, _ = s.roleCommand("gui", "quit")
		_, _ = s.roleCommand("quick-panel", "quit")
		_, _ = s.roleCommand("tray", "quit")
		if s.standaloneDaemon != nil && s.standaloneDaemon.Process != nil {
			_ = s.standaloneDaemon.Process.Signal(syscall.SIGTERM)
			select {
			case err := <-s.standaloneDaemonDone:
				_ = err
			case <-time.After(8 * time.Second):
				_ = s.standaloneDaemon.Process.Kill()
				select {
				case err := <-s.standaloneDaemonDone:
					_ = err
				case <-time.After(3 * time.Second):
					cleanupErr = errors.Join(cleanupErr, errors.New("standalone unicd did not exit"))
				}
			}
		}
		return cleanupErr
	}
	type observedRole struct {
		role string
		pid  int
		wait func(context.Context) error
	}
	var observed []observedRole
	for _, role := range []string{"gui", "quick-panel", "tray"} {
		raw, err := s.roleCommand(role, "probe")
		if err != nil {
			continue
		}
		var identity desktopRoleIdentity
		if err = json.Unmarshal(raw, &identity); err != nil {
			return err
		}
		if identity.PID <= 0 || identity.Role != role || identity.Profile != s.home {
			return errors.New("cannot bind cleanup to an unverified role identity")
		}
		wait, release, err := observeProcessExit(identity.PID)
		if err != nil {
			return err
		}
		defer release()
		observed = append(observed, observedRole{role: role, pid: identity.PID, wait: wait})
	}
	// App-scoped Quit crosses the actual view shortcut without physical input.
	_ = s.callAt(context.Background(), filepath.Join(s.home, "automation.sock"), "input", map[string]any{"window": s.mainWindow, "events": []any{map[string]any{"type": "key", "key": "CmdOrCtrl+Q"}}}, nil)
	// The GUI may already have exited through its window close action. Close the
	// persistent tray explicitly so its adapter and owned service also retire.
	_, _ = s.roleCommand("tray", "quit")
	select {
	case err := <-s.appDone:
		if err != nil {
			return fmt.Errorf("desktop exited unexpectedly: %w", err)
		}
	case <-time.After(8 * time.Second):
		_ = s.app.Process.Kill()
		select {
		case err := <-s.appDone:
			if err != nil {
				return fmt.Errorf("desktop required a forced stop: %w", err)
			}
		case <-time.After(3 * time.Second):
			return errors.New("owned desktop did not exit")
		}
		return errors.New("owned desktop required a forced stop")
	}
	ctx, cancel := context.WithTimeout(context.Background(), 15*time.Second)
	defer cancel()
	for {
		_, err := s.client.Service(ctx)
		if err != nil {
			for _, role := range observed {
				if err = role.wait(ctx); err != nil {
					return err
				}
				if _, err = s.roleCommand(role.role, "probe"); err == nil {
					return fmt.Errorf("owned %s endpoint remained after process %d exited", role.role, role.pid)
				}
				s.result.Assertions = append(s.result.Assertions, fmt.Sprintf("cleanup observed %s PID %d exit and endpoint removal", role.role, role.pid))
			}
			return nil
		}
		select {
		case <-ctx.Done():
			return errors.New("owned desktop service remained alive")
		case <-time.After(100 * time.Millisecond):
		}
	}
}
func desktopLabel(pattern string) (func(string) bool, error) {
	if expression, ok := strings.CutPrefix(pattern, "~"); ok {
		compiled, err := regexp.Compile(expression)
		if err != nil {
			return nil, err
		}
		return compiled.MatchString, nil
	}
	return func(label string) bool { return label == pattern }, nil
}

// acceptance runs the package-local scenario registry, never an old Test entry.
func (s *desktopScenario) acceptance(group, id string) error {
	if s.result.Mode == "native" || id != s.name {
		return errors.New("invalid acceptance scenario identity")
	}
	packages := map[string]string{"desktopui": "./app/desktop/internal/desktopui", "quickpanel": "./app/desktop/internal/quickpanel", "gui": "./app/desktop/cmd/unic-desktop", "startup": "./app/desktop/e2e", "boardui": "./app/desktop/boardui/e2e", "desktopkit": "./app/desktop/internal/desktopkit"}
	pkg, ok := packages[group]
	if !ok {
		return errors.New("unknown acceptance package")
	}
	artifact := filepath.Join(s.private, "acceptance")
	if err := os.MkdirAll(artifact, 0700); err != nil {
		return err
	}
	receiptPath := filepath.Join(s.out, "scenario-receipt.json")
	args := []string{"test", "-json"}
	if runtime.GOOS != "windows" || runtime.GOARCH != "arm64" {
		args = append(args, "-race")
	}
	args = append(args, pkg, "-run", "^TestDesktopAcceptance$/^"+regexp.QuoteMeta(id)+"$", "-count=1", "-timeout=10m")
	command := exec.CommandContext(s.ctx, "go", args...)
	command.Dir = s.root
	for _, v := range os.Environ() {
		if !strings.HasPrefix(v, "UNIC_") {
			command.Env = append(command.Env, v)
		}
	}
	command.Env = append(command.Env, "UNIC_DESKTOP_E2E=1", "UNIC_DESKTOP_E2E_SOURCE_HEAD="+os.Getenv("UNIC_DESKTOP_E2E_SOURCE_HEAD"), "UNIC_DESKTOP_E2E_CASE_RECEIPT="+receiptPath, "UNIC_DESKTOP_E2E_CASE_ARTIFACTS="+s.out, "UNIC_DESKTOP_E2E_CASE_PRIVATE="+artifact)
	log, err := os.Create(filepath.Join(s.private, "acceptance.log"))
	if err != nil {
		return err
	}
	defer func() { _ = log.Close() }()
	pipe, err := command.StdoutPipe()
	if err != nil {
		return err
	}
	command.Stderr = log
	if err = command.Start(); err != nil {
		return err
	}
	scanner := bufio.NewScanner(io.TeeReader(pipe, log))
	scanner.Buffer(make([]byte, 65536), 4*1024*1024)
	prefix := "TestDesktopAcceptance/" + id
	passed := false
	var invalid bool
	// Failure mode: a long successful tail can evict the first failed
	// subtest's assertion before the package exits. Preserve its recent
	// bounded context once; retain neither the full private log nor source.
	var diagnostics, firstFailureContext []string
	var firstFailureTest, firstFailureAction string
	appendDiagnostic := func(output string) {
		const maxEntryBytes, maxContextBytes = 4096, 12288
		if len(output) > maxEntryBytes {
			marker := "\n[diagnostic entry truncated]\n"
			half := (maxEntryBytes - len(marker)) / 2
			output = output[:half] + marker + output[len(output)-half:]
		}
		diagnostics = append(diagnostics, output)
		bytes := 0
		for _, line := range diagnostics {
			bytes += len(line)
		}
		for len(diagnostics) > 80 || bytes > maxContextBytes {
			bytes -= len(diagnostics[0])
			diagnostics = diagnostics[1:]
		}
	}
	retainFirstFailure := func(test, action string) {
		if firstFailureTest == "" {
			firstFailureTest, firstFailureAction = test, action
			firstFailureContext = append([]string(nil), diagnostics...)
		}
	}
	for scanner.Scan() {
		var event struct{ Action, Test, Output string }
		if json.Unmarshal(scanner.Bytes(), &event) != nil {
			continue
		}
		if event.Output != "" {
			appendDiagnostic(event.Output)
		}
		if event.Action == "fail" && event.Test == "" {
			retainFirstFailure("package "+pkg, event.Action)
		}
		if event.Test != prefix && !strings.HasPrefix(event.Test, prefix+"/") {
			continue
		}
		if event.Action == "skip" || event.Action == "fail" {
			invalid = true
			retainFirstFailure(event.Test, event.Action)
		}
		if event.Action == "pass" {
			s.result.Assertions = append(s.result.Assertions, event.Test)
			if event.Test == prefix {
				passed = true
			}
		}
	}
	scanErr := scanner.Err()
	runErr := command.Wait()
	// Retain the detailed startup receipt, but never its logs or binaries.
	if group == "startup" {
		if raw, e := os.ReadFile(filepath.Join(artifact, "manifest.json")); e == nil {
			if e = os.WriteFile(receiptPath, raw, 0600); e != nil {
				return e
			}
		}
	}
	err = filepath.WalkDir(s.out, func(path string, entry os.DirEntry, err error) error {
		if err != nil {
			return err
		}
		if entry.IsDir() || !strings.HasSuffix(path, ".png") {
			return nil
		}
		f, err := os.Open(path)
		if err != nil {
			return err
		}
		_, decodeErr := png.Decode(f)
		_ = f.Close()
		if decodeErr != nil {
			return decodeErr
		}
		relative, err := filepath.Rel(s.out, path)
		if err == nil {
			s.result.Screenshots = append(s.result.Screenshots, filepath.ToSlash(relative))
		}
		return err
	})
	if err != nil {
		return err
	}
	if runErr != nil || scanErr != nil || !passed || invalid {
		context := "Recent diagnostic tail:\n" + strings.Join(diagnostics, "")
		if firstFailureTest != "" {
			context = "First failed/skipped test: " + firstFailureTest + " (" + firstFailureAction + ")\n" + strings.Join(firstFailureContext, "") + "\n" + context
		}
		// The report links this small receipt; the full log stays private and
		// is cleaned with the frozen build and synthetic profile.
		receipt, marshalErr := json.MarshalIndent(map[string]any{
			"first_test": firstFailureTest, "first_action": firstFailureAction,
			"first_context": firstFailureContext, "tail": diagnostics,
			"context_limit_bytes": 12288, "entry_limit_bytes": 4096,
		}, "", "  ")
		if marshalErr == nil {
			if writeErr := os.WriteFile(filepath.Join(s.out, "acceptance-diagnostics.json"), append(receipt, '\n'), 0600); writeErr != nil {
				context += "\nDiagnostic receipt write failed: " + writeErr.Error()
			}
		}
		return fmt.Errorf("acceptance scenario failed, skipped, or did not run (process=%v, stream=%v)\n%s", runErr, scanErr, context)
	}
	if s.result.Mode == "view" && len(s.result.Screenshots) == 0 {
		return errors.New("view scenario emitted no screenshot")
	}
	return nil
}
