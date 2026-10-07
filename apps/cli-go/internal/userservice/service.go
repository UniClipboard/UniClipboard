// Package userservice manages only uniclip-owned user service definitions.
package userservice

import (
	"bytes"
	"context"
	"crypto/sha256"
	"encoding/xml"
	"errors"
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"runtime"
	"sort"
	"strconv"
	"strings"
	"time"

	"github.com/UniClipboard/UniClipboard/packages/desktop-host-go/apppaths"
	"github.com/UniClipboard/UniClipboard/packages/desktop-host-go/daemonproc"
)

const marker = "Managed by uniclip user service v1"

type Service struct{ Name, Path, Home, Domain string }
type State struct {
	Name      string `json:"name"`
	Installed bool   `json:"installed"`
	Loaded    bool   `json:"loaded"`
	Running   bool   `json:"running"`
	PID       int    `json:"pid,omitempty"`
}

func New() (*Service, error) {
	if runtime.GOOS != "darwin" && runtime.GOOS != "linux" {
		return nil, fmt.Errorf("user services are supported only on macOS launchd and Linux systemd (current platform: %s)", runtime.GOOS)
	}
	if os.Getuid() == 0 {
		return nil, errors.New("run service commands as your login user, without sudo")
	}
	if err := apppaths.PortableError(); err != nil {
		return nil, err
	}
	if apppaths.IsPortable() {
		return nil, errors.New("user services do not support portable installations; install uniclip and uniclipd in a stable directory")
	}
	home, err := os.UserHomeDir()
	if err != nil || !filepath.IsAbs(home) {
		return nil, errors.New("service requires an absolute HOME")
	}
	profile := os.Getenv("UC_PROFILE")
	if profile != "" {
		for _, c := range profile {
			if !(c >= 'a' && c <= 'z' || c >= 'A' && c <= 'Z' || c >= '0' && c <= '9' || c == '_' || c == '-') {
				return nil, errors.New("profile must contain only ASCII letters, digits, hyphens or underscores")
			}
		}
	}
	// Include HOME so isolated homes cannot address a real user's service.
	sum := sha256.Sum256([]byte(home + "\x00" + profile))
	name := fmt.Sprintf("app.uniclipboard.cli.%x", sum[:12])
	s := &Service{Name: name, Home: home, Domain: fmt.Sprintf("gui/%d", os.Getuid())}
	if runtime.GOOS == "darwin" {
		s.Path = filepath.Join(home, "Library", "LaunchAgents", name+".plist")
	} else {
		config := os.Getenv("XDG_CONFIG_HOME")
		if !filepath.IsAbs(config) {
			config = filepath.Join(home, ".config")
		}
		s.Path = filepath.Join(config, "systemd", "user", name+".service")
	}
	return s, nil
}

func command(name string, args ...string) (string, error) {
	ctx, cancel := context.WithTimeout(context.Background(), 70*time.Second)
	defer cancel()
	cmd := exec.CommandContext(ctx, name, args...)
	// Native managers can block; keep CLI operations bounded without leaving helpers behind.
	cmd.WaitDelay = 5 * time.Second
	data, err := cmd.CombinedOutput()
	if err != nil {
		return string(data), fmt.Errorf("%s %s: %w: %s", name, strings.Join(args, " "), err, strings.TrimSpace(string(data)))
	}
	return string(data), nil
}
func (s *Service) ctl(args ...string) (string, error) {
	if runtime.GOOS == "darwin" {
		return command("/bin/launchctl", args...)
	}
	return command("systemctl", append([]string{"--user"}, args...)...)
}
func (s *Service) target() string { return s.Domain + "/" + s.Name }
func (s *Service) unit() string   { return s.Name + ".service" }

func (s *Service) definition() ([]byte, error) {
	data, err := os.ReadFile(s.Path)
	if errors.Is(err, os.ErrNotExist) {
		return nil, nil
	}
	if err != nil {
		return nil, err
	}
	info, err := os.Lstat(s.Path)
	if err != nil {
		return nil, err
	}
	if !info.Mode().IsRegular() || !bytes.Contains(data, []byte(marker)) {
		return nil, fmt.Errorf("refusing unowned service definition: %s", s.Path)
	}
	return data, nil
}

func (s *Service) Status() (State, error) {
	state := State{Name: s.Name}
	data, err := s.definition()
	if err != nil {
		return state, err
	}
	state.Installed = data != nil
	if runtime.GOOS == "darwin" {
		out, err := s.ctl("print", s.target())
		if err != nil {
			if strings.Contains(out, "Could not find service") {
				return state, nil
			}
			return state, err
		}
		if !state.Installed {
			return state, errors.New("service label is loaded without our definition; refusing ownership")
		}
		if !strings.Contains(out, "path = "+s.Path+"\n") {
			return state, errors.New("loaded service uses another definition; refusing ownership")
		}
		state.Loaded = true
		for _, line := range strings.Split(out, "\n") {
			if !strings.HasPrefix(line, "\t") || strings.HasPrefix(line, "\t\t") {
				continue
			}
			key, value, ok := strings.Cut(strings.TrimSpace(line), " = ")
			if ok && key == "pid" {
				state.PID, _ = strconv.Atoi(value)
			}
			if ok && key == "state" {
				state.Running = value == "running"
			}
		}
	} else {
		out, err := s.ctl("show", s.unit(), "--property=LoadState,ActiveState,MainPID,FragmentPath")
		if err != nil {
			return state, err
		}
		for _, line := range strings.Split(out, "\n") {
			key, value, _ := strings.Cut(line, "=")
			switch key {
			case "LoadState":
				state.Loaded = value == "loaded"
			case "ActiveState":
				state.Running = value == "active"
			case "FragmentPath":
				if value != "" && value != s.Path {
					return state, errors.New("loaded service uses another definition; refusing ownership")
				}
			case "MainPID":
				state.PID, _ = strconv.Atoi(value)
			}
		}
		if state.Loaded && !state.Installed {
			return state, errors.New("service unit exists outside our definition; refusing ownership")
		}
	}
	return state, nil
}

func stableExecutable(path string) (string, error) {
	absolute, err := filepath.Abs(path)
	if err != nil {
		return "", err
	}
	absolute, err = filepath.EvalSymlinks(absolute)
	if err != nil {
		return "", err
	}
	info, err := os.Stat(absolute)
	if err != nil {
		return "", err
	}
	if !info.Mode().IsRegular() || info.Mode()&0111 == 0 {
		return "", fmt.Errorf("not executable: %s", absolute)
	}
	if os.Getenv("UNICLIPBOARD_ENV") == "development" {
		return absolute, nil
	}
	temp := filepath.Clean(os.TempDir())
	if strings.HasPrefix(absolute, temp+string(os.PathSeparator)) || strings.HasPrefix(absolute, "/private/tmp/") || strings.HasPrefix(absolute, "/tmp/") {
		return "", errors.New("production service cannot reference a temporary binary; install a release first")
	}
	for dir := filepath.Dir(absolute); ; dir = filepath.Dir(dir) {
		if _, err := os.Stat(filepath.Join(dir, ".git")); err == nil {
			return "", errors.New("production service cannot reference a repository/worktree binary; install a release first")
		}
		if dir == filepath.Dir(dir) {
			break
		}
	}
	return absolute, nil
}

func xmlString(value string) string {
	var b bytes.Buffer
	xml.EscapeText(&b, []byte(value))
	return "<string>" + b.String() + "</string>"
}

// systemd uses C-style quoting, percent specifiers, and dollar expansion in ExecStart.
func unitQuote(value string) string { return strconv.Quote(strings.ReplaceAll(value, "%", "%%")) }

func (s *Service) render(server bool) ([]byte, error) {
	self, err := os.Executable()
	if err != nil {
		return nil, err
	}
	self, err = stableExecutable(self)
	if err != nil {
		return nil, err
	}
	daemon, err := daemonproc.ResolveDaemonExe()
	if err != nil {
		return nil, err
	}
	daemon, err = stableExecutable(daemon)
	if err != nil {
		return nil, err
	}
	// run resolves its sibling before PATH: insist on a stable sibling, rather than persisting an arbitrary shell PATH.
	if filepath.Dir(daemon) != filepath.Dir(self) {
		return nil, errors.New("service requires uniclipd installed beside uniclip")
	}
	env := map[string]string{"HOME": s.Home, "UC_PROFILE": os.Getenv("UC_PROFILE"), "UNICLIPBOARD_ENV": "production", "UC_DAEMON_SPAWN_ORIGIN": "cli", "PATH": filepath.Dir(self) + ":/usr/bin:/bin:/usr/sbin:/sbin"}
	if os.Getenv("UNICLIPBOARD_ENV") == "development" {
		env["UNICLIPBOARD_ENV"] = "development"
	}
	for _, key := range []string{"XDG_DATA_HOME", "XDG_CACHE_HOME", "XDG_STATE_HOME", "UC_DISABLE_SYSTEM_CLIPBOARD"} {
		if value := os.Getenv(key); value != "" {
			env[key] = value
		}
	}
	keys := make([]string, 0, len(env))
	for key := range env {
		keys = append(keys, key)
	}
	sort.Strings(keys)
	args := []string{self, "run"}
	if server {
		args = append(args, "--server")
	}
	if runtime.GOOS == "darwin" {
		logs, ok := apppaths.AppLogDir()
		if !ok {
			return nil, errors.New("could not resolve log directory")
		}
		if err := os.MkdirAll(logs, 0700); err != nil {
			return nil, err
		}
		var b strings.Builder
		b.WriteString("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!-- " + marker + " -->\n<plist version=\"1.0\"><dict><key>Label</key>" + xmlString(s.Name) + "<key>ProgramArguments</key><array>")
		for _, arg := range args {
			b.WriteString(xmlString(arg))
		}
		b.WriteString("</array><key>EnvironmentVariables</key><dict>")
		for _, key := range keys {
			b.WriteString("<key>" + key + "</key>" + xmlString(env[key]))
		}
		b.WriteString("</dict><key>RunAtLoad</key><true/><key>KeepAlive</key><false/><key>StandardOutPath</key>" + xmlString(filepath.Join(logs, "service.stdout.log")) + "<key>StandardErrorPath</key>" + xmlString(filepath.Join(logs, "service.stderr.log")) + "</dict></plist>\n")
		return []byte(b.String()), nil
	}
	var b strings.Builder
	b.WriteString("# " + marker + "\n[Unit]\nDescription=UniClipboard user daemon\n[Service]\nType=simple\nExecStart=:")
	for i, arg := range args {
		if i > 0 {
			b.WriteByte(' ')
		}
		b.WriteString(unitQuote(arg))
	}
	b.WriteString("\nTimeoutStopSec=65\n")
	for _, key := range keys {
		b.WriteString("Environment=" + unitQuote(key+"="+env[key]) + "\n")
	}
	b.WriteString("\n[Install]\nWantedBy=default.target\n")
	return []byte(b.String()), nil
}

func (s *Service) Start(server bool) error {
	desired, err := s.render(server)
	if err != nil {
		return err
	}
	current, err := s.definition()
	if err != nil {
		return err
	}
	state, err := s.Status()
	if err != nil {
		return err
	}
	if state.Running {
		if !bytes.Equal(current, desired) {
			return errors.New("running service configuration differs; service stop, then service start with the desired options")
		}
		return nil
	}
	if state.Loaded && runtime.GOOS == "darwin" {
		if _, err := s.ctl("bootout", s.target()); err != nil {
			return err
		}
	}
	if !bytes.Equal(current, desired) {
		if err := os.MkdirAll(filepath.Dir(s.Path), 0700); err != nil {
			return err
		}
		// Exclusive temporary file and rename keep a complete owned definition on disk.
		f, err := os.CreateTemp(filepath.Dir(s.Path), ".uniclip-service-*")
		if err != nil {
			return err
		}
		defer os.Remove(f.Name())
		if _, err = f.Write(desired); err != nil {
			f.Close()
			return err
		}
		if err = f.Close(); err != nil {
			return err
		}
		if err = os.Rename(f.Name(), s.Path); err != nil {
			return err
		}
	}
	if runtime.GOOS == "darwin" {
		if _, err := s.ctl("enable", s.target()); err != nil {
			return err
		}
		_, err = s.ctl("bootstrap", s.Domain, s.Path)
	} else {
		if _, err := s.ctl("daemon-reload"); err != nil {
			return err
		}
		_, err = s.ctl("enable", "--now", s.unit())
	}
	return err
}

func (s *Service) Stop() error {
	state, err := s.Status()
	if err != nil {
		return err
	}
	if !state.Installed {
		return nil
	}
	if runtime.GOOS == "darwin" {
		if _, err := s.ctl("disable", s.target()); err != nil {
			return err
		}
		if state.Loaded {
			_, err = s.ctl("bootout", s.target())
		}
	} else {
		_, err = s.ctl("disable", "--now", s.unit())
	}
	if err != nil {
		return err
	}
	return waitStopped(state.PID)
}

func waitStopped(pid int) error {
	deadline := time.Now().Add(65 * time.Second)
	for pid > 0 && daemonproc.IsActiveDaemon(uint32(pid)) {
		if !time.Now().Before(deadline) {
			return errors.New("service daemon did not exit; refusing another start")
		}
		time.Sleep(200 * time.Millisecond)
	}
	return nil
}

func (s *Service) Restart() error {
	state, err := s.Status()
	if err != nil {
		return err
	}
	if !state.Installed {
		return errors.New("service is not installed; use service start first")
	}
	if runtime.GOOS == "darwin" {
		if state.Loaded {
			if _, err := s.ctl("bootout", s.target()); err != nil {
				return err
			}
			if err := waitStopped(state.PID); err != nil {
				return err
			}
		}
		if _, err := s.ctl("enable", s.target()); err != nil {
			return err
		}
		_, err = s.ctl("bootstrap", s.Domain, s.Path)
	} else {
		if _, err := s.ctl("enable", s.unit()); err != nil {
			return err
		}
		_, err = s.ctl("restart", s.unit())
	}
	return err
}
