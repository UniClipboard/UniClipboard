// Package desktopprocess provides private, profile-scoped Desktop role IPC.
// Engine and service code do not import this package.
package desktopprocess

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"net"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
	"sync"
	"time"

	"github.com/uniclipboard/unic/internal/servicelaunch"
)

type Role string

const (
	Tray       Role = "tray"
	GUI        Role = "gui"
	QuickPanel Role = "quick-panel"
)

var ErrRunning = errors.New("desktop role already running")
var ErrUnavailable = errors.New("desktop role unavailable")
var ErrStaleBuild = errors.New("desktop role belongs to a previous build")

type Info struct {
	Role         Role      `json:"role"`
	PID          int       `json:"pid"`
	StartedAt    time.Time `json:"started_at"`
	Profile      string    `json:"profile"`
	BinaryPath   string    `json:"binary_path"`
	BinarySHA256 string    `json:"binary_sha256"`
}
type Handler func(context.Context, string, json.RawMessage) (any, error)

type afterReply struct {
	value any
	after func()
}

// ReplyAfter schedules after only when the response has been encoded and fully
// written to the connection. Encoding or write failure does not run after.
// The callback runs in a separate goroutine so it may close the role owner.
// A completed write acknowledges the command; it does not acknowledge that the
// callback has finished. Callers that need completion must probe the role.
func ReplyAfter(value any, after func()) any { return afterReply{value: value, after: after} }

type request struct {
	Identity Info            `json:"identity"`
	Command  string          `json:"command"`
	Payload  json.RawMessage `json:"payload"`
}
type response struct {
	Identity  Info            `json:"identity"`
	Result    json.RawMessage `json:"result,omitempty"`
	Error     string          `json:"error,omitempty"`
	ErrorCode string          `json:"error_code,omitempty"`
}
type Owner struct {
	info     Info
	listener net.Listener
	unlock   func()
	cancel   context.CancelFunc
	once     sync.Once
	workers  sync.WaitGroup
}

func validRole(role Role) bool { return role == Tray || role == GUI || role == QuickPanel }

func identity(dir string, role Role) (Info, error) {
	if !validRole(role) {
		return Info{}, errors.New("invalid desktop role")
	}
	profile, err := filepath.Abs(dir)
	if err != nil {
		return Info{}, err
	}
	profile, err = filepath.EvalSymlinks(profile)
	if err != nil {
		return Info{}, err
	}
	program, hash, err := selfExecutable()
	if err != nil {
		return Info{}, err
	}
	return Info{Role: role, PID: os.Getpid(), StartedAt: time.Now().UTC(), Profile: profile, BinaryPath: program, BinarySHA256: hash}, nil
}

var self struct {
	once       sync.Once
	path, hash string
	err        error
}

func fileHash(path string) (string, error) {
	f, err := os.Open(path)
	if err != nil {
		return "", err
	}
	sum := sha256.New()
	_, copyErr := io.Copy(sum, f)
	closeErr := f.Close()
	if err := errors.Join(copyErr, closeErr); err != nil {
		return "", err
	}
	return hex.EncodeToString(sum.Sum(nil)), nil
}
func selfExecutable() (string, string, error) {
	self.once.Do(func() {
		self.path, self.err = os.Executable()
		if self.err != nil {
			return
		}
		self.path, self.err = filepath.EvalSymlinks(self.path)
		if self.err != nil {
			return
		}
		self.hash, self.err = fileHash(self.path)
	})
	if self.err != nil {
		return "", "", self.err
	}
	hash, err := fileHash(self.path)
	if err != nil {
		return "", "", err
	}
	if hash != self.hash {
		return "", "", errors.New("desktop executable changed; restart the application")
	}
	return self.path, self.hash, nil
}
func verifyPeer(conn net.Conn, server bool, expected Info) (int, error) {
	rawPath, pid, err := peerExecutable(conn, server)
	if err != nil {
		return 0, errors.New("desktop IPC peer executable unavailable")
	}
	path, err := filepath.EvalSymlinks(rawPath)
	if err != nil || !samePath(path, expected.BinaryPath) {
		if !server && isReplacedExecutable(rawPath, expected.BinaryPath) {
			return 0, fmt.Errorf("%w: desktop IPC peer executable replaced", ErrStaleBuild)
		}
		return 0, errors.New("desktop IPC peer executable mismatch")
	}
	hash, err := fileHash(path)
	if err != nil || hash != expected.BinarySHA256 {
		if !server {
			return 0, fmt.Errorf("%w: desktop IPC peer build mismatch", ErrStaleBuild)
		}
		return 0, errors.New("desktop IPC peer build mismatch")
	}
	return pid, nil
}

func canonicalPeerPath(path string) string {
	if resolved, err := filepath.EvalSymlinks(path); err == nil {
		return resolved
	}
	dir := filepath.Dir(path)
	if resolvedDir, err := filepath.EvalSymlinks(dir); err == nil {
		return filepath.Join(resolvedDir, filepath.Base(path))
	}
	return filepath.Clean(path)
}

func isMovedAsideName(peerBase, expectedBase string) bool {
	prefix := "." + expectedBase + ".old-"
	if len(peerBase) <= len(prefix) || !samePath(peerBase[:len(prefix)], prefix) {
		return false
	}
	for i := len(prefix); i < len(peerBase); i++ {
		if peerBase[i] < '0' || peerBase[i] > '9' {
			return false
		}
	}
	return true
}

// isReplacedExecutable reports whether peerPath is a previous build of
// expectedPath that an in-place or staged updater moved aside or unlinked.
// Unrecognized executables in the same directory return false.
func isReplacedExecutable(peerPath, expectedPath string) bool {
	if peerPath == "" || expectedPath == "" {
		return false
	}
	expected := canonicalPeerPath(expectedPath)
	if trimmed, ok := strings.CutSuffix(peerPath, " (deleted)"); ok {
		if samePath(canonicalPeerPath(trimmed), expected) {
			return true
		}
	}
	peer := canonicalPeerPath(peerPath)
	if samePath(filepath.Dir(peer), filepath.Dir(expected)) {
		if isMovedAsideName(filepath.Base(peer), filepath.Base(expected)) {
			return true
		}
	}
	// macOS staged updates move <App>.app into .<App>.app.update-<id>/old
	// beside the bundle while old child roles may still be running.
	if macOSBundleOldExecutable(peer, expected) {
		return true
	}
	return false
}

func macOSBundleOldExecutable(peer, expected string) bool {
	if !samePath(filepath.Base(peer), filepath.Base(expected)) {
		return false
	}
	// Expected: <parent>/<Name>.app/Contents/MacOS/<bin>
	macOSDir := filepath.Dir(expected)
	contentsDir := filepath.Dir(macOSDir)
	bundleDir := filepath.Dir(contentsDir)
	if filepath.Base(macOSDir) != "MacOS" || filepath.Base(contentsDir) != "Contents" || !strings.HasSuffix(bundleDir, ".app") {
		return false
	}
	peerMacOS := filepath.Dir(peer)
	peerContents := filepath.Dir(peerMacOS)
	peerOld := filepath.Dir(peerContents)
	peerWork := filepath.Dir(peerOld)
	if filepath.Base(peerMacOS) != "MacOS" || filepath.Base(peerContents) != "Contents" || filepath.Base(peerOld) != "old" {
		return false
	}
	if !samePath(filepath.Dir(peerWork), filepath.Dir(bundleDir)) {
		return false
	}
	prefix := "." + filepath.Base(bundleDir) + ".update-"
	return strings.HasPrefix(filepath.Base(peerWork), prefix) && len(filepath.Base(peerWork)) > len(prefix)
}
func sameIdentity(a, b Info) bool {
	return a.Profile == b.Profile && a.Role == b.Role && samePath(a.BinaryPath, b.BinaryPath) && a.BinarySHA256 == b.BinarySHA256
}

func runtimeDir(profile string) (string, error) {
	path := filepath.Join(profile, "desktop-runtime")
	if err := privateDir(path); err != nil {
		return "", err
	}
	return path, nil
}

// Acquire holds a kernel lock for the entire role lifetime. It never trusts a PID file.
// ErrRunning is returned only after the existing endpoint passes identity checks.
func Acquire(ctx context.Context, dir string, role Role, handler Handler) (*Owner, error) {
	info, err := identity(dir, role)
	if err != nil {
		return nil, err
	}
	root, err := runtimeDir(info.Profile)
	if err != nil {
		return nil, err
	}
	unlock, err := lock(filepath.Join(root, string(role)+".lock"))
	if err != nil {
		if errors.Is(err, ErrRunning) {
			// The winning launcher may still be opening its endpoint.
			ready, cancel := context.WithTimeout(ctx, 5*time.Second)
			defer cancel()
			probeErr := servicelaunch.Wait(ready, func() (bool, error) {
				_, err := Probe(ready, dir, role)
				if errors.Is(err, ErrUnavailable) {
					return false, nil
				}
				return err == nil, err
			})
			if probeErr != nil {
				return nil, probeErr
			}
		}
		return nil, err
	}
	ln, err := listen(root, role)
	if err != nil {
		unlock()
		return nil, err
	}
	ownerCtx, cancel := context.WithCancel(ctx)
	owner := &Owner{info: info, listener: ln, unlock: unlock, cancel: cancel}
	owner.workers.Add(1)
	go func() {
		defer owner.workers.Done()
		limit := make(chan struct{}, 32)
		for {
			conn, err := ln.Accept()
			if err != nil {
				return
			}
			select {
			case limit <- struct{}{}:
			default:
				_ = conn.Close()
				continue
			}
			owner.workers.Add(1)
			go func() {
				defer owner.workers.Done()
				defer func() { <-limit }()
				defer func() { _ = conn.Close() }()
				stop := context.AfterFunc(ownerCtx, func() { _ = conn.Close() })
				defer stop()
				_ = conn.SetDeadline(time.Now().Add(30 * time.Second))
				peerPID, peerErr := verifyPeer(conn, true, info)
				if peerErr != nil {
					return
				}
				decoder := json.NewDecoder(io.LimitReader(conn, 1<<20))
				var req request
				if err := decoder.Decode(&req); err != nil {
					return
				}
				reply := response{Identity: info}
				var after func()
				if !sameIdentity(req.Identity, info) || req.Identity.PID != peerPID {
					reply.Error = "desktop IPC identity mismatch"
				} else if req.Command == "__probe" {
					reply.Result, _ = json.Marshal(info)
				} else if handler == nil {
					reply.Error = "desktop role command unavailable"
				} else {
					value, err := handler(ownerCtx, req.Command, req.Payload)
					if err != nil {
						reply.Error = err.Error()
						if errors.Is(err, ErrUnavailable) {
							reply.ErrorCode = "unavailable"
						}
					} else {
						if deferred, ok := value.(afterReply); ok {
							value, after = deferred.value, deferred.after
						}
						reply.Result, err = json.Marshal(value)
						if err != nil {
							reply.Error = "desktop IPC result encoding failed"
						}
					}
				}
				if err := json.NewEncoder(conn).Encode(reply); err == nil && reply.Error == "" && after != nil {
					go after()
				}
			}()
		}
	}()
	return owner, nil
}

// Info identifies this role owner. StartedAt is the role acquisition timestamp.
func (o *Owner) Info() Info { return o.info }
func (o *Owner) Close() error {
	var err error
	o.once.Do(func() { o.cancel(); err = o.listener.Close(); o.workers.Wait(); o.unlock() })
	return err
}

// Call sends one bounded command. The receiver and sender must belong to the same
// profile and exact executable build. The private endpoint is the access boundary.
func Call(ctx context.Context, dir string, role Role, command string, payload, result any) error {
	info, err := identity(dir, role)
	if err != nil {
		return err
	}
	root, err := runtimeDir(info.Profile)
	if err != nil {
		return err
	}
	conn, err := dial(ctx, root, role)
	if err != nil {
		return fmt.Errorf("%w: %v", ErrUnavailable, err)
	}
	defer func() { _ = conn.Close() }()
	deadline := time.Now().Add(30 * time.Second)
	if d, ok := ctx.Deadline(); ok && d.Before(deadline) {
		deadline = d
	}
	_ = conn.SetDeadline(deadline)
	stop := context.AfterFunc(ctx, func() { _ = conn.Close() })
	defer stop()
	peerPID, err := verifyPeer(conn, false, info)
	if err != nil {
		return err
	}
	raw, err := json.Marshal(payload)
	if err != nil {
		return err
	}
	if len(raw) > 1<<19 {
		return errors.New("desktop IPC payload too large")
	}
	if err := json.NewEncoder(conn).Encode(request{Identity: info, Command: command, Payload: raw}); err != nil {
		return err
	}
	var reply response
	if err := json.NewDecoder(io.LimitReader(conn, 1<<20)).Decode(&reply); err != nil {
		return err
	}
	if !sameIdentity(info, reply.Identity) || reply.Identity.PID != peerPID {
		if reply.Identity.Profile == info.Profile && reply.Identity.Role == info.Role && samePath(reply.Identity.BinaryPath, info.BinaryPath) && reply.Identity.BinarySHA256 != info.BinarySHA256 && reply.Identity.PID == peerPID {
			return fmt.Errorf("%w: desktop IPC owner build mismatch", ErrStaleBuild)
		}
		return errors.New("desktop IPC owner identity mismatch")
	}
	if reply.Error != "" {
		if reply.ErrorCode == "unavailable" {
			return fmt.Errorf("%w: %s", ErrUnavailable, reply.Error)
		}
		return errors.New(reply.Error)
	}
	if result != nil && len(reply.Result) != 0 {
		return json.Unmarshal(reply.Result, result)
	}
	return nil
}
func Probe(ctx context.Context, dir string, role Role) (Info, error) {
	var result Info
	err := Call(ctx, dir, role, "__probe", nil, &result)
	return result, err
}

// RetireStale terminates an orphaned previous-build view role that still owns
// this profile's private IPC endpoint after an executable replacement. It
// refuses unrecognized executables and never touches a live current-build role.
func RetireStale(ctx context.Context, dir string, role Role) (bool, error) {
	if role != GUI && role != QuickPanel {
		return false, errors.New("stale retirement is limited to view roles")
	}
	info, err := identity(dir, role)
	if err != nil {
		return false, err
	}
	root, err := runtimeDir(info.Profile)
	if err != nil {
		return false, err
	}
	conn, err := dial(ctx, root, role)
	if err != nil {
		return false, nil
	}
	retired, err := retireStalePeer(ctx, conn, root, info)
	_ = conn.Close()
	if err != nil || !retired {
		return retired, err
	}
	waitCtx, cancel := context.WithTimeout(ctx, 5*time.Second)
	defer cancel()
	err = servicelaunch.Wait(waitCtx, func() (bool, error) {
		_, probeErr := Probe(waitCtx, dir, role)
		if errors.Is(probeErr, ErrUnavailable) {
			return true, nil
		}
		return false, nil
	})
	return err == nil, err
}

// Launch starts the same installed executable in a detached role. Concurrent
// launches converge on the role lock. It never activates or toggles an existing role.
func Launch(ctx context.Context, dir, socket string, role Role, args ...string) (Info, error) {
	return LaunchEnv(ctx, dir, socket, role, os.Environ(), args...)
}

// LaunchEnv starts a role with the caller's environment. The selected profile
// and service socket always override inherited values.
func LaunchEnv(ctx context.Context, dir, socket string, role Role, environment []string, args ...string) (Info, error) {
	if err := ctx.Err(); err != nil {
		return Info{}, err
	}
	if existing, err := Probe(ctx, dir, role); err == nil {
		return existing, nil
	} else if !errors.Is(err, ErrUnavailable) {
		return Info{}, err
	}
	program, err := os.Executable()
	if err != nil {
		return Info{}, err
	}
	cmd := exec.Command(program, append([]string{"--role", string(role)}, args...)...)
	configureRoleProcess(cmd)
	env := make([]string, 0, len(environment)+2)
	for _, entry := range environment {
		if !strings.HasPrefix(entry, "UNIC_HOME=") && !strings.HasPrefix(entry, "UNIC_SOCK=") {
			env = append(env, entry)
		}
	}
	cmd.Env = append(env, "UNIC_HOME="+dir, "UNIC_SOCK="+socket)
	root, err := runtimeDir(dir)
	if err != nil {
		return Info{}, err
	}
	log, err := os.OpenFile(filepath.Join(root, string(role)+".log"), os.O_CREATE|os.O_APPEND|os.O_WRONLY, 0600)
	if err != nil {
		return Info{}, err
	}
	cmd.Stdout, cmd.Stderr = log, log
	if err := ctx.Err(); err != nil {
		_ = log.Close()
		return Info{}, err
	}
	err = cmd.Start()
	_ = log.Close()
	if err != nil {
		return Info{}, err
	}
	done := make(chan error, 1)
	go func() { done <- cmd.Wait() }()
	var info Info
	startup, cancel := context.WithTimeout(ctx, 30*time.Second)
	defer cancel()
	err = servicelaunch.Wait(startup, func() (bool, error) {
		var probeErr error
		info, probeErr = Probe(startup, dir, role)
		if probeErr == nil {
			return true, nil
		}
		if !errors.Is(probeErr, ErrUnavailable) {
			return false, probeErr
		}
		select {
		case exit := <-done:
			if exit != nil {
				return false, fmt.Errorf("desktop role exited before readiness: %w", exit)
			}
			return false, errors.New("desktop role exited before readiness")
		default:
		}
		return false, nil
	})
	if err != nil {
		_ = cmd.Process.Kill()
	}
	return info, err
}
