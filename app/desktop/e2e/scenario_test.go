package e2e

import (
	"bufio"
	"context"
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
	"strings"
	"sync"
	"syscall"
	"testing"
	"time"

	"github.com/rogpeppe/go-internal/testscript"

	"github.com/uniclipboard/unic/internal/clip"
	api "github.com/uniclipboard/unic/internal/localapi"
	"github.com/uniclipboard/unic/internal/testboard"
)

type desktopResult struct {
	RecordingStarted float64          `json:"recording_started_seconds,omitempty"`
	CaptureSeconds   []float64        `json:"capture_seconds,omitempty"`
	Status           string           `json:"status"`
	Mode             string           `json:"mode"`
	Failure          string           `json:"failure,omitempty"`
	Assertions       []string         `json:"assertions"`
	Steps            []map[string]any `json:"steps"`
	Screenshots      []string         `json:"screenshots"`
	Recording        string           `json:"recording,omitempty"`
	Frames           int              `json:"frames,omitempty"`
	Duration         float64          `json:"duration_seconds,omitempty"`
	Cleanup          string           `json:"cleanup"`
}
type desktopScenario struct {
	captureMu                      sync.Mutex // Do not overlap recording and checkpoint captures.
	ctx                            context.Context
	cancel                         context.CancelFunc
	name, root, private, out, home string
	app                            *exec.Cmd
	appDone                        chan error
	client                         *api.Client
	automation                     string
	window                         int
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
	env.Values["desktop"] = s
	env.Defer(func() { s.finish(env) })
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
				return errors.New("start takes no arguments")
			}
			return s.start()
		},
		"submit": func(s *desktopScenario, a []string) error {
			if len(a) != 2 {
				return errors.New("submit id text")
			}
			_, err := s.client.Submit(s.ctx, api.SubmitInput{RequestID: a[0], Snapshot: clip.Snapshot{Parts: []clip.Part{{Format: clip.Text, Data: []byte(a[1])}}}})
			return err
		},
		"ui-wait":   func(s *desktopScenario, a []string) error { return s.expect(a, false) },
		"ui-absent": func(s *desktopScenario, a []string) error { return s.expect(a, true) },
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
		"ui-click": func(s *desktopScenario, a []string) error {
			if len(a) != 2 {
				return errors.New("ui-click role label")
			}
			node, err := s.find(a[0], a[1])
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
func (s *desktopScenario) wait(check func() bool) error {
	timeout := time.NewTimer(20 * time.Second)
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
			return errors.New("condition did not become true within 20 seconds")
		case <-tick.C:
		}
	}
}
func (s *desktopScenario) start() error {
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
	config := "[telemetry]\nenabled=false\n[sync]\nenabled=false\n[desktop]\nmode='normal'\nlaunch_at_login=false\nstart_in_background=false\nquick_panel_shortcut=''\n"
	if err := os.WriteFile(filepath.Join(s.home, "config.toml"), []byte(config), 0600); err != nil {
		return err
	}
	s.automation = filepath.Join(s.home, "automation.sock")
	binary := filepath.Join(os.Getenv("UNIC_DESKTOP_E2E_BIN"), "unic-desktop")
	if os.PathSeparator == '\\' {
		binary += ".exe"
	}
	s.app = exec.Command(binary)
	for _, env := range os.Environ() {
		if !strings.HasPrefix(env, "UNIC_") && !strings.HasPrefix(env, "MYGO_AUTOMATION=") {
			s.app.Env = append(s.app.Env, env)
		}
	}
	s.app.Env = append(s.app.Env, "UNIC_HOME="+s.home, "UNIC_DESKTOP_E2E_PROFILE="+s.home, "MYGO_AUTOMATION="+s.automation, "UNIC_RENDEZVOUS_URL=http://127.0.0.1:1")
	log, err := os.Create(filepath.Join(s.private, "desktop.log"))
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
				return true
			}
		}
		return false
	})
	if err != nil {
		return fmt.Errorf("native window: %w", err)
	}
	if err = s.wait(func() bool { _, e := s.client.Status(s.ctx); return e == nil }); err != nil {
		return err
	}
	s.media = newDesktopRecording(s)
	s.result.Assertions = append(s.result.Assertions, "an isolated native content window and production service are running")
	return s.screenshot("started")
}

type desktopNode struct {
	ID                 string
	Role, Label, Value string
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
func (s *desktopScenario) expect(args []string, absent bool) error {
	if len(args) != 2 {
		return errors.New("ui assertion requires role and exact label")
	}
	match, err := desktopLabel(args[1])
	if err != nil {
		return err
	}
	stable := time.Time{}
	err = s.wait(func() bool {
		nodes, e := s.tree()
		if e != nil {
			return false
		}
		present := false
		for _, n := range nodes {
			present = present || (n.Role == args[0] && match(n.Label))
		}
		if present == absent {
			stable = time.Time{}
			return false
		}
		if stable.IsZero() {
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
	if err := ctx.Err(); err != nil {
		return err
	}
	started := time.Now()
	if method == "capture" {
		s.captureMu.Lock()
		defer s.captureMu.Unlock()
		started = time.Now()
		defer func() { s.result.CaptureSeconds = append(s.result.CaptureSeconds, time.Since(started).Seconds()) }()
	}
	connection, err := (&net.Dialer{Timeout: time.Second}).DialContext(ctx, "unix", s.automation)
	if err != nil {
		return err
	}
	defer func() { _ = connection.Close() }()
	if err = connection.SetDeadline(time.Now().Add(8 * time.Second)); err != nil {
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
			s.result.Failure = err.Error()
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
func (s *desktopScenario) stopApp() error {
	if s.appDone == nil {
		return nil
	}
	// App-scoped Quit crosses the actual view shortcut without physical input.
	_ = s.call(context.Background(), "input", map[string]any{"window": s.window, "events": []any{map[string]any{"type": "key", "key": "CmdOrCtrl+Q"}}}, nil)
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
	ctx, cancel := context.WithTimeout(context.Background(), 6*time.Second)
	defer cancel()
	for {
		_, err := s.client.Service(ctx)
		if err != nil {
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
	packages := map[string]string{"desktopui": "./app/desktop/internal/desktopui", "quickpanel": "./app/desktop/internal/quickpanel", "gui": "./app/desktop/cmd/unic-desktop", "startup": "./app/desktop/e2e"}
	pkg, ok := packages[group]
	if !ok {
		return errors.New("unknown acceptance package")
	}
	artifact := filepath.Join(s.private, "acceptance")
	if err := os.MkdirAll(artifact, 0700); err != nil {
		return err
	}
	receiptPath := filepath.Join(s.out, "scenario-receipt.json")
	command := exec.CommandContext(s.ctx, "go", "test", "-json", "-race", pkg, "-run", "^TestDesktopAcceptance$/^"+regexp.QuoteMeta(id)+"$", "-count=1", "-timeout=10m")
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
	var diagnostics []string
	for scanner.Scan() {
		var event struct{ Action, Test, Output string }
		if json.Unmarshal(scanner.Bytes(), &event) != nil {
			continue
		}
		if event.Output != "" {
			diagnostics = append(diagnostics, event.Output)
			if len(diagnostics) > 80 {
				diagnostics = diagnostics[1:]
			}
		}
		if event.Test != prefix && !strings.HasPrefix(event.Test, prefix+"/") {
			continue
		}
		if event.Action == "skip" || event.Action == "fail" {
			invalid = true
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
		return fmt.Errorf("acceptance scenario failed, skipped, or did not run (process=%v, stream=%v)\n%s", runErr, scanErr, strings.Join(diagnostics, ""))
	}
	if s.result.Mode == "view" && len(s.result.Screenshots) == 0 {
		return errors.New("view scenario emitted no screenshot")
	}
	return nil
}
