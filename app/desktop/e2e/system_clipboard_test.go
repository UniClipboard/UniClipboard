package e2e

import (
	"bytes"
	"context"
	"crypto/rand"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"image"
	"image/color"
	"image/png"
	"io"
	"net/url"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
	"time"

	"github.com/BurntSushi/toml"

	"github.com/uniclipboard/unic/internal/clip"
	api "github.com/uniclipboard/unic/internal/localapi"
	config "github.com/uniclipboard/unic/internal/localconfig"
)

// Failure inventory before implementation: wrong host, active SPICE sharing,
// baseline broadcast, missed capture, directory-board substitution, failed
// system application, echo history, a hung reader, and clipboard-owner residue.
func systemClipboardCommands() map[string]func(*desktopScenario, []string) error {
	return map[string]func(*desktopScenario, []string) error{
		"system-offline-backlog": func(s *desktopScenario, a []string) error {
			if !s.systemClipboard || s.policyPeer == nil || len(a) != 0 {
				return errors.New("system-offline-backlog requires a native guest and connected peer")
			}
			return s.systemOfflineBacklog()
		},
		"system-file-cleanup": func(s *desktopScenario, a []string) error {
			if !s.systemClipboard || s.policyPeer == nil || len(a) != 0 {
				return errors.New("system-file-cleanup requires a native guest and connected production peer")
			}
			return s.systemFileCleanup()
		},
		"system-peer-text": func(s *desktopScenario, a []string) error {
			if !s.systemClipboard || s.policyPeer == nil || len(a) != 1 {
				return errors.New("system-peer-text requires a native guest, connected production peer and synthetic text")
			}
			before, err := s.policyPeer.client.QueryHistory(s.ctx, api.HistoryQuery{})
			if err != nil {
				return err
			}
			if _, err = s.policyPeer.client.Submit(s.ctx, api.SubmitInput{RequestID: "native-peer-text", Snapshot: clip.Snapshot{Parts: []clip.Part{{Format: clip.Text, Data: []byte(a[0])}}}}); err != nil {
				return err
			}
			if err = s.systemBoardEquals(a[0]); err != nil {
				return fmt.Errorf("remote text did not apply to real system clipboard: %w", err)
			}
			if err = s.wait(func() bool {
				page, e := s.client.QueryHistory(s.ctx, api.HistoryQuery{Origins: []string{"remote_device"}})
				if e != nil {
					return false
				}
				for _, item := range page.Items {
					var b bytes.Buffer
					if s.client.CopyHistoryContent(s.ctx, item.ID, 0, &b) == nil && b.String() == a[0] {
						return true
					}
				}
				return false
			}); err != nil {
				return err
			}
			timer := time.NewTimer(2 * time.Second)
			defer timer.Stop()
			select {
			case <-timer.C:
			case <-s.ctx.Done():
				return s.ctx.Err()
			}
			after, err := s.policyPeer.client.QueryHistory(s.ctx, api.HistoryQuery{})
			if err != nil {
				return err
			}
			if len(after.Items) != len(before.Items)+1 {
				return errors.New("received system text echoed back to the sender")
			}
			s.result.Assertions = append(s.result.Assertions, "headless production peer delivered exact text to real guest clipboard and remote history without sender echo; this is one graphical endpoint")
			return nil
		},
		"system-second-launch": func(s *desktopScenario, a []string) error {
			if !s.systemClipboard || len(a) != 0 || s.app == nil {
				return errors.New("system-second-launch requires an owned guest desktop")
			}
			before, err := s.client.Service(s.ctx)
			if err != nil {
				return err
			}
			ctx, cancel := context.WithTimeout(s.ctx, 12*time.Second)
			defer cancel()
			second := exec.CommandContext(ctx, s.app.Path)
			for _, entry := range s.app.Env {
				if !strings.HasPrefix(entry, "MYGO_AUTOMATION=") {
					second.Env = append(second.Env, entry)
				}
			}
			second.Env = append(second.Env, "MYGO_AUTOMATION=")
			if b, e := second.CombinedOutput(); e != nil {
				if w := os.WriteFile(filepath.Join(s.out, "second-launch-failure.log"), b, 0600); w != nil {
					return errors.Join(e, w)
				}
				return fmt.Errorf("second launch failed: %w", e)
			}
			after, err := s.client.Service(s.ctx)
			if err != nil {
				return err
			}
			if after.ID != before.ID {
				return errors.New("second launch replaced the owned service generation")
			}
			var reopened desktopRoleIdentity
			raw, err := s.roleCommand("gui", "probe")
			if err != nil {
				return err
			}
			if err = json.Unmarshal(raw, &reopened); err != nil {
				return err
			}
			if reopened.PID == s.app.Process.Pid {
				return errors.New("second launch retained the closed GUI process")
			}
			s.result.Assertions = append(s.result.Assertions, fmt.Sprintf("second production launch recreated independent GUI PID %d and preserved the tray-owned service generation", reopened.PID))
			return nil
		},
		"system-window": func(s *desktopScenario, a []string) error {
			if !s.systemClipboard || len(a) != 2 || (a[1] != "visible" && a[1] != "hidden") {
				return errors.New("system-window title visible|hidden requires a guest")
			}
			if a[0] == "unic" && a[1] == "hidden" {
				return s.wait(func() bool {
					// Main-window close must terminate the GUI OS process.
					select {
					case err := <-s.appDone:
						s.appDone <- err
						if err != nil {
							return false
						}
					default:
						return false
					}
					if _, err := s.roleCommand("gui", "probe"); err == nil {
						return false
					}
					if _, err := s.roleCommand("tray", "probe"); err != nil {
						return false
					}
					_, err := s.client.Service(s.ctx)
					return err == nil
				})
			}
			endpoint := filepath.Join(s.home, "automation.sock")
			if a[0] == "unic · 快捷面板" {
				endpoint = filepath.Join(s.home, "automation-panel.sock")
			}
			return s.wait(func() bool {
				var out struct {
					Windows []struct {
						ID      int
						Title   string
						Visible bool
					}
				}
				if s.callAt(s.ctx, endpoint, "windows", nil, &out) != nil {
					return false
				}
				found := 0
				for _, w := range out.Windows {
					if w.Title == a[0] && w.Visible == (a[1] == "visible") {
						found++
						if w.Visible {
							s.captureMu.Lock()
							s.window = w.ID
							s.automation = endpoint
							s.capturePaused = false
							if a[0] == "unic" {
								s.mainWindow = w.ID
							}
							s.captureMu.Unlock()
						}
					}
				}
				return found == 1
			})
		},
		"system-format-roundtrip": func(s *desktopScenario, a []string) error {
			if !s.systemClipboard || len(a) != 1 {
				return errors.New("system-format-roundtrip requires a disposable guest and html, png or file")
			}
			return s.systemFormatRoundtrip(a[0])
		},
		"system-paste-target": func(s *desktopScenario, a []string) error {
			if !s.systemClipboard || len(a) != 2 || filepath.Base(a[1]) != a[1] || s.pasteTarget != nil {
				return errors.New("system-paste-target requires inserted text, a saved-byte fixture and a disposable guest")
			}
			saved, err := os.ReadFile(filepath.Join(s.workdir, a[1]))
			if err != nil {
				return err
			}
			return s.pasteIntoEditor(a[0], saved)
		},
		"system-key": func(s *desktopScenario, a []string) error {
			if len(a) != 1 || (a[0] != "Enter" && a[0] != "Escape" && a[0] != "Ctrl+C" && a[0] != "Ctrl+F" && a[0] != "Alt+F4") {
				return errors.New("system-key currently accepts Enter, Escape, Ctrl+C, Ctrl+F or Alt+F4")
			}
			var windows struct {
				Windows []struct {
					ID      int
					Focused bool
				}
			}
			if err := s.wait(func() bool {
				if s.call(s.ctx, "windows", nil, &windows) != nil {
					return false
				}
				for _, w := range windows.Windows {
					if w.ID == s.window && w.Focused {
						return true
					}
				}
				return false
			}); err != nil {
				return errors.New("owned native window does not have OS focus; no key sent")
			}
			if a[0] == "Alt+F4" {
				s.captureMu.Lock()
				s.capturePaused = true
				s.captureMu.Unlock()
			}
			return s.systemInput("key", a[0])
		},
		"system-board-write": func(s *desktopScenario, a []string) error {
			if !s.systemClipboard || os.Getenv("UNIC_DESKTOP_E2E_GUEST") == "" || len(a) != 1 {
				return errors.New("system-board-write requires an explicit disposable guest and one synthetic text")
			}
			cmd := exec.CommandContext(s.ctx, "wl-copy", "--foreground", "--type", "text/plain;charset=utf-8")
			cmd.Stdin = strings.NewReader(a[0])
			if err := cmd.Start(); err != nil {
				return err
			}
			s.clipboardOwners = append(s.clipboardOwners, cmd)
			return s.systemBoardEquals(a[0])
		},
		"system-board-equals": func(s *desktopScenario, a []string) error {
			if !s.systemClipboard || os.Getenv("UNIC_DESKTOP_E2E_GUEST") == "" || len(a) != 1 {
				return errors.New("system-board-equals requires an explicit disposable guest and one synthetic text")
			}
			return s.systemBoardEquals(a[0])
		},
	}
}

// Failure inventory: service ownership replacement, old receipt auto-application,
// baseline broadcast, discarded persistent history, failed new receive, or leaks.
func (s *desktopScenario) systemOfflineBacklog() (result error) {
	s.captureMu.Lock()
	locked := true
	defer func() {
		if locked {
			s.captureMu.Unlock()
		}
	}()
	if err := s.stopApp(); err != nil {
		return err
	}
	environment := s.app.Env
	s.app, s.appDone = nil, nil
	standalone := exec.Command(filepath.Join(os.Getenv("UNIC_DESKTOP_E2E_BIN"), "unicd"))
	standalone.Env = environment
	log, err := os.Create(filepath.Join(s.private, "backlog-service.log"))
	if err != nil {
		return err
	}
	standalone.Stdout, standalone.Stderr = log, log
	if err = standalone.Start(); err != nil {
		_ = log.Close()
		return err
	}
	done := make(chan error, 1)
	go func() { err := standalone.Wait(); _ = log.Close(); done <- err }()
	reaped := false
	defer func() {
		if reaped {
			return
		}
		_ = standalone.Process.Kill()
		select {
		case exitErr := <-done:
			var exit *exec.ExitError
			if exitErr != nil && !errors.As(exitErr, &exit) {
				result = errors.Join(result, exitErr)
			}
		case <-time.After(5 * time.Second):
			result = errors.Join(result, errors.New("owned backlog service did not exit"))
		}
	}()
	if err = s.wait(func() bool { _, e := s.client.Service(s.ctx); return e == nil }); err != nil {
		return err
	}
	// The private test disables rendezvous. Refresh the peer's endpoint after
	// service restart through its real ticket and ordinary approval path.
	ticket, err := s.policyPeer.client.ExportTicket(s.ctx)
	if err != nil {
		return err
	}
	if _, err = s.client.Pair(s.ctx, ticket); err != nil {
		return err
	}
	for _, client := range []*api.Client{s.client, s.policyPeer.client} {
		pending, e := client.Pending(s.ctx)
		if e != nil {
			return e
		}
		if len(pending) > 1 {
			return errors.New("unexpected pending peers after owned service restart")
		}
		for _, req := range pending {
			if _, e = client.Approve(s.ctx, req.ID, api.Approval{Fingerprint: req.Fingerprint, Revision: req.Revision, Bindings: req.Bindings}); e != nil {
				return e
			}
		}
	}
	if err = s.wait(func() bool {
		st, e := s.client.Status(s.ctx)
		return e == nil && len(st.Peers) == 1 && st.Peers[0].Connected
	}); err != nil {
		return fmt.Errorf("restarted standalone service is not connected: %w", err)
	}
	backlog := "UNIC-D received while desktop offline 中文"
	if _, err = s.policyPeer.client.Submit(s.ctx, api.SubmitInput{RequestID: "native-offline-backlog", Snapshot: clip.Snapshot{Parts: []clip.Part{{Format: clip.Text, Data: []byte(backlog)}}}}); err != nil {
		return err
	}
	if err = s.wait(func() bool {
		page, e := s.client.QueryHistory(s.ctx, api.HistoryQuery{Origins: []string{"remote_device"}})
		if e != nil {
			return false
		}
		for _, item := range page.Items {
			var b bytes.Buffer
			if s.client.CopyHistoryContent(s.ctx, item.ID, 0, &b) == nil && b.String() == backlog {
				return true
			}
		}
		return false
	}); err != nil {
		return fmt.Errorf("headless backlog not delivered: %w", err)
	}
	if err = s.systemBoardEquals("UNIC-D offline startup baseline"); err != nil {
		return err
	}
	if err = s.launchNative(); err != nil {
		return err
	}
	instance, err := s.client.Service(s.ctx)
	if err != nil {
		return err
	}
	if instance.PID != standalone.Process.Pid {
		return errors.New("normal Desktop replaced the independently started service")
	}
	select {
	case exitErr := <-done:
		reaped = true
		return errors.Join(errors.New("independent standalone service exited during Desktop startup"), exitErr)
	default:
	}
	s.captureMu.Unlock()
	locked = false
	ticket, err = s.policyPeer.client.ExportTicket(s.ctx)
	if err != nil {
		return err
	}
	if _, err = s.client.Pair(s.ctx, ticket); err != nil {
		return err
	}
	if err = s.wait(func() bool {
		st, e := s.client.Status(s.ctx)
		return e == nil && len(st.Peers) == 1 && st.Peers[0].Connected
	}); err != nil {
		return err
	}
	if err = s.systemBoardEquals("UNIC-D offline startup baseline"); err != nil {
		return err
	}
	timer := time.NewTimer(2 * time.Second)
	defer timer.Stop()
	select {
	case <-timer.C:
	case <-s.ctx.Done():
		return s.ctx.Err()
	}
	if err = s.systemBoardEquals("UNIC-D offline startup baseline"); err != nil {
		return err
	}
	page, err := s.client.QueryHistory(s.ctx, api.HistoryQuery{Origins: []string{"remote_device"}})
	if err != nil {
		return err
	}
	retained := false
	for _, item := range page.Items {
		var b bytes.Buffer
		if s.client.CopyHistoryContent(s.ctx, item.ID, 0, &b) == nil && b.String() == backlog {
			retained = true
		}
	}
	if !retained {
		return errors.New("reopening lost persisted offline history")
	}
	s.result.Assertions = append(s.result.Assertions, "real peer delivered backlog while only standalone service ran; normal Desktop reused that independent service, retained history and left the native baseline unchanged")
	return systemClipboardCommands()["system-peer-text"](s, []string{"UNIC-D fresh receive after offline backlog 中文"})
}

// Failure inventory: excluded remote history removed, pinned native file freed,
// exclusion removal ignored, released file leaked, or user source deleted.
func (s *desktopScenario) systemFileCleanup() error {
	payload := []byte("UNIC-D managed cleanup 中文\n")
	source := filepath.Join(s.private, "peer-user-owned.txt")
	if err := os.WriteFile(source, payload, 0600); err != nil {
		return err
	}
	if _, err := s.policyPeer.client.Submit(s.ctx, api.SubmitInput{RequestID: "native-managed-file", Snapshot: clip.Snapshot{Files: []clip.File{{Name: filepath.Base(source), Path: source, Size: int64(len(payload))}}}}); err != nil {
		return err
	}
	var received api.HistoryItem
	if err := s.wait(func() bool {
		page, e := s.client.QueryHistory(s.ctx, api.HistoryQuery{Origins: []string{"remote_device"}, Kinds: []string{"files"}})
		if e != nil || len(page.Items) != 1 {
			return false
		}
		received = page.Items[0]
		return true
	}); err != nil {
		return fmt.Errorf("managed file was not received: %w", err)
	}
	managed := ""
	if err := s.wait(func() bool {
		b, e := exec.CommandContext(s.ctx, "wl-paste", "--no-newline", "--type", "text/uri-list").Output()
		if e != nil {
			return false
		}
		u, e := url.Parse(strings.TrimSpace(string(b)))
		if e != nil || u.Scheme != "file" || u.Host != "" {
			return false
		}
		rel, e := filepath.Rel(s.home, u.Path)
		if e != nil || rel == ".." || strings.HasPrefix(rel, ".."+string(os.PathSeparator)) {
			return false
		}
		actual, e := os.ReadFile(u.Path)
		if e != nil || !bytes.Equal(actual, payload) {
			return false
		}
		managed = u.Path
		return true
	}); err != nil {
		return fmt.Errorf("received file not applied as exact managed native URI: %w", err)
	}
	tags := []string{"keep-native"}
	if _, err := s.client.UpdateHistory(s.ctx, received.ID, api.HistoryUpdate{Revision: received.Revision, Tags: &tags}); err != nil {
		return err
	}
	policy := func(excluded []string) error {
		st, err := s.client.Config(s.ctx)
		if err != nil {
			return err
		}
		st.Config.History = config.History{AutoCleanup: true, MaxItems: 1, RetentionDays: 0, MaxBytes: st.Config.History.MaxBytes, ExcludedTags: excluded}
		var b bytes.Buffer
		if err = toml.NewEncoder(&b).Encode(st.Config); err != nil {
			return err
		}
		out, err := s.client.SaveConfig(s.ctx, st.Revision, b.Bytes())
		if err != nil {
			return err
		}
		if out.Error != "" || out.ApplyError != "" {
			return errors.New("native cleanup policy did not apply")
		}
		return nil
	}
	if err := policy(tags); err != nil {
		return err
	}
	if _, err := s.client.Submit(s.ctx, api.SubmitInput{RequestID: "native-cleanup-newer", Snapshot: clip.Snapshot{Parts: []clip.Part{{Format: clip.Text, Data: []byte("UNIC-D newer cleanup history")}}}}); err != nil {
		return err
	}
	timer := time.NewTimer(2 * time.Second)
	defer timer.Stop()
	select {
	case <-timer.C:
	case <-s.ctx.Done():
		return s.ctx.Err()
	}
	if _, err := s.client.GetHistory(s.ctx, received.ID); err != nil {
		return fmt.Errorf("excluded history removed: %w", err)
	}
	if b, err := os.ReadFile(managed); err != nil || !bytes.Equal(b, payload) {
		return errors.New("excluded managed file removed or changed")
	}
	s.result.Assertions = append(s.result.Assertions, "excluded tag protects received history and exact native clipboard managed file under active count cleanup")
	if err := policy([]string{}); err != nil {
		return err
	}
	if err := s.wait(func() bool {
		_, e := s.client.GetHistory(s.ctx, received.ID)
		return api.ReasonOf(e) == "not_found"
	}); err != nil {
		page, pageErr := s.client.QueryHistory(s.ctx, api.HistoryQuery{})
		st, stateErr := s.client.Config(s.ctx)
		diagnostic, e := json.MarshalIndent(map[string]any{"history": page, "history_error": fmt.Sprint(pageErr), "cleanup_policy": st.Config.History, "apply_error": st.ApplyError, "config_error": fmt.Sprint(stateErr)}, "", "  ")
		if e != nil {
			return errors.Join(err, e)
		}
		if e = os.WriteFile(filepath.Join(s.out, "cleanup-failure.json"), diagnostic, 0600); e != nil {
			return errors.Join(err, e)
		}
		return fmt.Errorf("eligible history not removed after exclusion removal: %w", err)
	}
	if b, err := os.ReadFile(managed); err != nil || !bytes.Equal(b, payload) {
		return errors.New("native clipboard holder lost its managed file")
	}
	s.result.Assertions = append(s.result.Assertions, "removing exclusion deletes eligible history while native clipboard holder retains exact file bytes")
	owner := exec.CommandContext(s.ctx, "wl-copy", "--foreground", "--type", "text/plain;charset=utf-8")
	owner.Stdin = strings.NewReader("UNIC-D native holder released")
	if err := owner.Start(); err != nil {
		return err
	}
	s.clipboardOwners = append(s.clipboardOwners, owner)
	if err := s.systemBoardEquals("UNIC-D native holder released"); err != nil {
		return err
	}
	if err := s.wait(func() bool { _, e := os.Stat(managed); return errors.Is(e, os.ErrNotExist) }); err != nil {
		return fmt.Errorf("released managed file not reclaimed: %w", err)
	}
	if b, err := os.ReadFile(source); err != nil || !bytes.Equal(b, payload) {
		return errors.New("user-owned source changed during managed cleanup")
	}
	s.result.Assertions = append(s.result.Assertions, fmt.Sprintf("replacing native clipboard releases holder and reclaims managed file; user source preserved, SHA256 %x", sha256.Sum256(payload)))
	return nil
}

func (s *desktopScenario) pasteIntoEditor(want string, saved []byte) error {
	document := filepath.Join(s.private, "target-document.txt")
	if err := os.WriteFile(document, nil, 0600); err != nil {
		return err
	}
	s.pasteTarget = exec.CommandContext(s.ctx, "gnome-text-editor", "--standalone", document)
	s.pasteTarget.Env = append(os.Environ(), "XDG_CONFIG_HOME="+filepath.Join(s.private, "target-config"),
		"XDG_DATA_HOME="+filepath.Join(s.private, "target-data"), "XDG_STATE_HOME="+filepath.Join(s.private, "target-state"),
		"XDG_CACHE_HOME="+filepath.Join(s.private, "target-cache"), "GSETTINGS_BACKEND=memory")
	log, err := os.Create(filepath.Join(s.private, "target.log"))
	if err != nil {
		return err
	}
	s.pasteTarget.Stdout, s.pasteTarget.Stderr = log, log
	if err = s.pasteTarget.Start(); err != nil {
		_ = log.Close()
		s.pasteTarget = nil
		return err
	}
	s.pasteTargetDone = make(chan error, 1)
	go func() { err := s.pasteTarget.Wait(); _ = log.Close(); s.pasteTargetDone <- err }()
	inspect := func(mode string) (string, error) {
		ctx, cancel := context.WithTimeout(s.ctx, 2*time.Second)
		defer cancel()
		b, err := exec.CommandContext(ctx, "python3", filepath.Join(s.root, "app/desktop/e2e/target_access.py"), fmt.Sprint(s.pasteTarget.Process.Pid), mode).Output()
		return string(b), err
	}
	if err = s.wait(func() bool { _, e := inspect("active"); return e == nil }); err != nil {
		return errors.New("isolated target editor does not own OS focus; no paste sent")
	}
	s.result.Assertions = append(s.result.Assertions, "exact standalone GNOME Text Editor process owns OS focus")
	if err = s.systemInput("key", "Ctrl+V"); err != nil {
		return err
	}
	if err = s.wait(func() bool { text, e := inspect("text"); return e == nil && text == want }); err != nil {
		return errors.New("real target editor did not insert the expected clipboard text")
	}
	s.result.Assertions = append(s.result.Assertions, "target editor accessible text equals the inserted synthetic clipboard bytes")
	if err = s.systemInput("key", "Ctrl+S"); err != nil {
		return err
	}
	var actual []byte
	if err = s.wait(func() bool {
		var e error
		actual, e = os.ReadFile(document)
		return e == nil && string(actual) == string(saved)
	}); err != nil {
		if writeErr := os.WriteFile(filepath.Join(s.out, "target-saved-failure.txt"), actual, 0600); writeErr != nil {
			return errors.Join(err, writeErr)
		}
		return fmt.Errorf("target document saved %d bytes; expected %d fixture bytes", len(actual), len(saved))
	}
	if err = os.WriteFile(filepath.Join(s.out, "target-content.txt"), actual, 0600); err != nil {
		return err
	}
	version, err := exec.CommandContext(s.ctx, "gnome-text-editor", "--version").Output()
	if err != nil {
		return err
	}
	metadata, err := json.MarshalIndent(map[string]any{"application": strings.TrimSpace(string(version)),
		"pid": s.pasteTarget.Process.Pid, "content_sha256": fmt.Sprintf("%x", sha256.Sum256(actual)),
		"inserted_text_sha256": fmt.Sprintf("%x", sha256.Sum256([]byte(want))), "save_rule": "GtkSourceView implicit trailing newline",
		"physical_paste": true, "accessible_text_equal": true, "saved_bytes_equal": true}, "", "  ")
	if err != nil {
		return err
	}
	if err = os.WriteFile(filepath.Join(s.out, "target-receipt.json"), metadata, 0600); err != nil {
		return err
	}
	captureCtx, captureCancel := context.WithTimeout(s.ctx, 20*time.Second)
	defer captureCancel()
	picture := filepath.Join(s.out, "target-native.png")
	capture := exec.CommandContext(captureCtx, "spectacle", "--background", "--new-instance", "--activewindow", "--nonotify", "--output", picture)
	capture.Env = append(os.Environ(), "XDG_CONFIG_HOME="+filepath.Join(s.private, "capture-config"),
		"XDG_DATA_HOME="+filepath.Join(s.private, "capture-data"), "XDG_CACHE_HOME="+filepath.Join(s.private, "capture-cache"))
	if _, err = capture.CombinedOutput(); err != nil {
		return fmt.Errorf("native target capture failed: %w", err)
	}
	file, err := os.Open(picture)
	if err != nil {
		return err
	}
	geometry, decodeErr := png.DecodeConfig(file)
	if err = errors.Join(decodeErr, file.Close()); err != nil {
		return err
	}
	if geometry.Width == 0 || geometry.Height == 0 {
		return errors.New("native target screenshot is empty")
	}
	if _, err = inspect("active"); err != nil {
		return errors.New("target lost OS focus during native screenshot")
	}
	s.result.Screenshots = append(s.result.Screenshots, "target-native.png")
	s.result.Assertions = append(s.result.Assertions, "KDE Spectacle captured the focused target's real guest window")
	s.result.Assertions = append(s.result.Assertions, "GNOME Text Editor inserted exact synthetic text through physical Ctrl+V and saved it through physical Ctrl+S")
	return nil
}

// Failures specified before execution: lost MIME/data, source-file damage,
// false capture, restore without a real write, echo, and surviving owners.
func (s *desktopScenario) systemFormatRoundtrip(kind string) error {
	var payload []byte
	mime := ""
	switch kind {
	case "html":
		mime, payload = "text/html", []byte("<p><b>UNIC-D rich 中文</b></p>")
	case "png":
		mime = "image/png"
		img := image.NewNRGBA(image.Rect(0, 0, 17, 13))
		img.Set(3, 4, color.NRGBA{R: 250, G: 31, B: 79, A: 255})
		var b bytes.Buffer
		if err := png.Encode(&b, img); err != nil {
			return err
		}
		payload = b.Bytes()
	case "file":
		mime = "text/uri-list"
		payload = []byte("UNIC-D user-owned file 中文\n")
	default:
		return errors.New("expected html, png or file")
	}
	before, err := s.client.QueryHistory(s.ctx, api.HistoryQuery{})
	if err != nil {
		return err
	}
	known := map[string]bool{}
	for _, item := range before.Items {
		known[item.ID] = true
	}
	offer := payload
	source := filepath.Join(s.private, "user-owned.txt")
	if kind == "file" {
		if err = os.WriteFile(source, payload, 0600); err != nil {
			return err
		}
		u := url.URL{Scheme: "file", Path: source}
		offer = []byte(u.String() + "\r\n")
	}
	owner := exec.CommandContext(s.ctx, "wl-copy", "--foreground", "--type", mime)
	owner.Stdin = bytes.NewReader(offer)
	if err = owner.Start(); err != nil {
		return err
	}
	s.clipboardOwners = append(s.clipboardOwners, owner)
	var captured api.HistoryItem
	if err = s.wait(func() bool {
		page, e := s.client.QueryHistory(s.ctx, api.HistoryQuery{})
		if e != nil {
			return false
		}
		for _, item := range page.Items {
			if !known[item.ID] && item.Origin == "local_system" {
				captured = item
				return true
			}
		}
		return false
	}); err != nil {
		return fmt.Errorf("%s native capture missing: %w", kind, err)
	}
	part := -1
	for i, p := range captured.Parts {
		if (kind == "file" && p.Kind == "file") || p.MIME == mime {
			part = i
		}
	}
	if part < 0 {
		return fmt.Errorf("%s required representation missing: %+v", kind, captured.Parts)
	}
	if len(captured.Parts) != 1 && (kind != "html" || len(captured.Parts) != 2) {
		return fmt.Errorf("%s captured unexpected parts: %+v", kind, captured.Parts)
	}
	// KDE can advertise a text alternative for an HTML-only owner. Bind it to
	// the external OS offer rather than assuming that the source owns all MIME types.
	for i, p := range captured.Parts {
		if i == part {
			continue
		}
		if p.MIME != "text/plain" {
			return fmt.Errorf("unexpected alternative: %+v", p)
		}
		offered, e := exec.CommandContext(s.ctx, "wl-paste", "--no-newline", "--type", p.MIME).Output()
		if e != nil {
			return e
		}
		c, e := s.client.HistoryContent(s.ctx, captured.ID, i)
		if e != nil {
			return e
		}
		b, e := io.ReadAll(c)
		if e = errors.Join(e, c.Close()); e != nil {
			return e
		}
		if !bytes.Equal(b, offered) {
			return errors.New("native text alternative differs from captured bytes")
		}
		s.result.Assertions = append(s.result.Assertions, fmt.Sprintf("native HTML text alternative matches external offer SHA256 %x", sha256.Sum256(b)))
	}
	content, err := s.client.HistoryContent(s.ctx, captured.ID, part)
	if err != nil {
		return err
	}
	stored, readErr := io.ReadAll(content)
	if err = errors.Join(readErr, content.Close()); err != nil {
		return err
	}
	if !bytes.Equal(stored, payload) {
		return fmt.Errorf("%s captured bytes differ", kind)
	}
	if kind != "file" && captured.Parts[part].MIME != mime {
		return errors.New("captured MIME differs")
	}
	// Replace the OS offer before restore, so an unchanged source cannot pass.
	barrier := exec.CommandContext(s.ctx, "wl-copy", "--foreground", "--type", "application/x-unic-d-barrier")
	barrier.Stdin = strings.NewReader("UNIC-D barrier")
	if err = barrier.Start(); err != nil {
		return err
	}
	s.clipboardOwners = append(s.clipboardOwners, barrier)
	if err = s.wait(func() bool {
		b, e := exec.CommandContext(s.ctx, "wl-paste", "--no-newline", "--type", "application/x-unic-d-barrier").Output()
		return e == nil && string(b) == "UNIC-D barrier"
	}); err != nil {
		return err
	}
	label := string(payload)
	if kind == "png" {
		label = "图片"
	}
	if kind == "file" {
		label = "user-owned.txt"
	}
	var selected desktopNode
	if err = s.wait(func() bool { var e error; selected, e = s.find("listItem", label); return e == nil }); err != nil {
		return err
	}
	if err = s.call(s.ctx, "input", map[string]any{"window": s.window, "events": []any{map[string]any{"type": "click", "x": selected.Bounds.X + selected.Bounds.Width/2, "y": selected.Bounds.Y + selected.Bounds.Height/2}}}, nil); err != nil {
		return err
	}
	if err = systemClipboardCommands()["system-key"](s, []string{"Enter"}); err != nil {
		return err
	}
	var observed []byte
	var observeError string
	if err = s.wait(func() bool {
		ctx, cancel := context.WithTimeout(s.ctx, time.Second)
		defer cancel()
		b, e := exec.CommandContext(ctx, "wl-paste", "--no-newline", "--type", mime).Output()
		observed = append(observed[:0], b...)
		observeError = fmt.Sprint(e)
		if e != nil {
			return false
		}
		if kind == "file" {
			u, e := url.Parse(strings.TrimSpace(string(b)))
			if e != nil || u.Scheme != "file" || u.Host != "" {
				return false
			}
			b, e = os.ReadFile(u.Path)
			if e != nil {
				return false
			}
		}
		return bytes.Equal(b, payload)
	}); err != nil {
		types, _ := exec.CommandContext(s.ctx, "wl-paste", "--list-types").Output()
		diagnostic, e := json.MarshalIndent(map[string]any{"kind": kind, "parts": captured.Parts, "offered_types": string(types), "read_error": observeError, "actual": string(observed), "expected_sha256": fmt.Sprintf("%x", sha256.Sum256(payload))}, "", "  ")
		if e != nil {
			return errors.Join(err, e)
		}
		if e = os.WriteFile(filepath.Join(s.out, "format-failure.json"), diagnostic, 0600); e != nil {
			return errors.Join(err, e)
		}
		return fmt.Errorf("%s real clipboard restore differs: %w", kind, err)
	}
	if kind == "file" {
		b, e := os.ReadFile(source)
		if e != nil || !bytes.Equal(b, payload) {
			return errors.New("user-owned source file changed")
		}
	}
	timer := time.NewTimer(2 * time.Second)
	defer timer.Stop()
	select {
	case <-timer.C:
	case <-s.ctx.Done():
		return s.ctx.Err()
	}
	after, err := s.client.QueryHistory(s.ctx, api.HistoryQuery{})
	if err != nil {
		return err
	}
	if len(after.Items) != len(before.Items)+1 {
		return fmt.Errorf("%s capture/restore created extra history", kind)
	}
	s.result.Assertions = append(s.result.Assertions, fmt.Sprintf("%s real guest capture, exact stored bytes, replaced-offer restore and no echo; payload SHA256 %x", kind, sha256.Sum256(payload)))
	return nil
}

// One request has one identity. A missing acknowledgement never replays input.
func (s *desktopScenario) systemInput(action string, value any) error {
	dir := os.Getenv("UNIC_DESKTOP_E2E_CONTROL")
	guest := os.Getenv("UNIC_DESKTOP_E2E_GUEST")
	if !s.systemClipboard || guest == "" || !filepath.IsAbs(dir) {
		return errors.New("physical input requires its explicit disposable guest controller")
	}
	var nonce [16]byte
	if _, err := rand.Read(nonce[:]); err != nil {
		return err
	}
	id := hex.EncodeToString(nonce[:])
	data, err := json.Marshal(map[string]any{"id": id, "guest": guest, "action": action, "value": value})
	if err != nil {
		return err
	}
	if err = os.WriteFile(filepath.Join(dir, "request.tmp"), data, 0600); err != nil {
		return err
	}
	if err = os.Rename(filepath.Join(dir, "request.tmp"), filepath.Join(dir, "request.json")); err != nil {
		return err
	}
	var reply struct {
		ID string
		OK bool
	}
	if err = s.wait(func() bool {
		b, e := os.ReadFile(filepath.Join(dir, "ack.json"))
		return e == nil && json.Unmarshal(b, &reply) == nil && reply.ID == id
	}); err != nil {
		return fmt.Errorf("physical input outcome unknown; do not replay: %w", err)
	}
	if !reply.OK {
		return errors.New("guest controller refused physical input")
	}
	s.result.Assertions = append(s.result.Assertions, "one physical guest input request acknowledged without retry")
	return nil
}

func (s *desktopScenario) systemBoardEquals(want string) error {
	err := s.wait(func() bool {
		ctx, cancel := context.WithTimeout(s.ctx, time.Second)
		defer cancel()
		out, err := exec.CommandContext(ctx, "wl-paste", "--no-newline", "--type", "text/plain").Output()
		return err == nil && string(out) == want
	})
	if err == nil {
		s.result.Assertions = append(s.result.Assertions, "real guest clipboard text equals the synthetic expected bytes")
	}
	return err
}
