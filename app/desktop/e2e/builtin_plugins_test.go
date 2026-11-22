package e2e

import (
	"bytes"
	"encoding/json"
	"errors"
	"fmt"
	"image"
	"image/png"
	"os"
	"path/filepath"
	"slices"
	"strconv"
	"strings"
	"time"

	"github.com/uniclipboard/unic/internal/clip"
	api "github.com/uniclipboard/unic/internal/localapi"
	config "github.com/uniclipboard/unic/internal/localconfig"
)

// These commands observe the production process; all setting writes happen via UI.
func builtinPluginCommands() map[string]func(*desktopScenario, []string) error {
	return map[string]func(*desktopScenario, []string) error{
		"start-external": func(s *desktopScenario, a []string) error {
			if len(a) != 0 {
				return errors.New("start-external takes no arguments")
			}
			s.externalBackend = true
			return s.start()
		},
		"external-roundtrip": func(s *desktopScenario, a []string) error {
			if len(a) != 0 {
				return errors.New("external-roundtrip takes no arguments")
			}
			return s.externalRoundtrip()
		},
		"external-policy-check": func(s *desktopScenario, a []string) error {
			result, err := s.client.Submit(s.ctx, api.SubmitInput{RequestID: "external-filtered", Snapshot: clip.Snapshot{Parts: []clip.Part{{Format: clip.Text, Data: []byte("external-filtered")}}}})
			if err != nil {
				return err
			}
			if string(result.SyncDisposition) != "policy_filtered" {
				return fmt.Errorf("external policy disposition: %s", result.SyncDisposition)
			}
			s.result.Assertions = append(s.result.Assertions, "external instance text policy enforced by production Engine")
			return nil
		},
		"plugin-back-in-toolbar": func(s *desktopScenario, a []string) error {
			if len(a) != 0 {
				return errors.New("plugin-back-in-toolbar takes no arguments")
			}
			err := s.wait(func() bool {
				button, e := s.find("button", "返回插件列表")
				if e != nil {
					return false
				}
				bar, e := s.find("toolbar", "插件")
				if e != nil {
					return false
				}
				b, t := button.Bounds, bar.Bounds
				if b.Width <= 0 || b.Width > 44 || b.Height > 44 || b.X < t.X || b.Y < t.Y || b.Y+b.Height > t.Y+t.Height {
					return false
				}
				nodes, e := s.tree()
				if e != nil {
					return false
				}
				for _, n := range nodes {
					if n.Role == "text" && n.Label == "插件" && n.Bounds.Y >= t.Y && n.Bounds.Y+n.Bounds.Height <= t.Y+t.Height && n.Bounds.X >= t.X {
						return b.X+b.Width <= n.Bounds.X
					}
				}
				return false
			})
			if err == nil {
				s.result.Assertions = append(s.result.Assertions, "compact accessible plugin back icon is inside the toolbar, before its title")
			}
			return err
		},
		"connect-policy-peer": func(s *desktopScenario, a []string) error { return s.connectPolicyPeer(a) },
		"start-pairing-peer": func(s *desktopScenario, a []string) error {
			if len(a) != 0 || s.policyPeer != nil {
				return errors.New("start-pairing-peer takes no arguments and creates one peer")
			}
			_, _, err := s.spawnPeer()
			return err
		},
		"peer-pair-desktop": func(s *desktopScenario, a []string) error {
			if len(a) != 0 || s.policyPeer == nil {
				return errors.New("peer-pair-desktop requires an owned peer")
			}
			var ticket string
			var err error
			if s.externalBackend {
				ticket, err = s.client.ExportTicketVia(s.ctx, "e2e.desktop")
			} else {
				ticket, err = s.client.ExportTicket(s.ctx)
			}
			if err != nil {
				return err
			}
			if s.externalBackend {
				_, err = s.policyPeer.client.PairVia(s.ctx, ticket, "e2e.peer")
			} else {
				_, err = s.policyPeer.client.Pair(s.ctx, ticket)
			}
			if err != nil {
				return err
			}
			var pending []api.PendingPeer
			if err = s.wait(func() bool {
				var e error
				pending, e = s.policyPeer.client.Pending(s.ctx)
				return e == nil && len(pending) == 1
			}); err != nil {
				return err
			}
			req := pending[0]
			if s.externalBackend {
				other, e := s.client.Pending(s.ctx)
				if e != nil || len(other) != 1 || other[0].Fingerprint != req.Fingerprint {
					return errors.New("both fixture fingerprints must match before approval")
				}
			}
			if _, err = s.policyPeer.client.Approve(s.ctx, req.ID, api.Approval{Fingerprint: req.Fingerprint, Revision: req.Revision, Bindings: req.Bindings}); err != nil {
				return err
			}
			s.result.Assertions = append(s.result.Assertions, "peer joined the Desktop invitation and approved the exact displayed request")
			return nil
		},
		"assert-paired": func(s *desktopScenario, a []string) error {
			if len(a) != 0 || s.policyPeer == nil {
				return errors.New("assert-paired requires an owned peer")
			}
			err := s.wait(func() bool {
				st, e := s.client.Status(s.ctx)
				pt, f := s.policyPeer.client.Status(s.ctx)
				return e == nil && f == nil && len(st.Peers) == 1 && len(pt.Peers) == 1 && len(st.Pending) == 0
			})
			if err == nil {
				s.result.Assertions = append(s.result.Assertions, "both production services list exactly one peer and Desktop has no pending request")
			}
			return err
		},
		"stop-policy-peer": func(s *desktopScenario, a []string) error {
			if len(a) != 0 || s.policyPeer == nil {
				return errors.New("stop-policy-peer requires an owned peer")
			}
			err := s.policyPeer.stop()
			s.policyPeer = nil
			return err
		},
		"config-equals": func(s *desktopScenario, a []string) error {
			if len(a) != 2 {
				return errors.New("config-equals path JSON-value")
			}
			var last any
			err := s.wait(func() bool {
				st, e := s.client.Config(s.ctx)
				if e != nil || st.Error != "" || st.ApplyError != "" {
					return false
				}
				b, _ := json.Marshal(st.Config)
				var value any
				_ = json.Unmarshal(b, &value)
				// Plugin IDs contain dots; traverse the fixed instances prefix explicitly.
				path := strings.Split(a[0], ".")
				if len(path) > 3 && path[0] == "plugins" && path[1] == "instances" {
					path = append([]string{"plugins", "instances", path[2] + "." + path[3]}, path[4:]...)
				}
				for _, key := range path {
					m, ok := value.(map[string]any)
					if !ok {
						return false
					}
					value = m[key]
				}
				last = value
				encoded, _ := json.Marshal(value)
				if string(encoded) != a[1] {
					return false
				}
				// Check the saved TOML too; a displayed or API-only value is insufficient.
				raw, e := os.ReadFile(filepath.Join(s.home, "config.toml"))
				if e != nil {
					return false
				}
				disk, e := config.Parse(raw)
				if e != nil {
					return false
				}
				encoded, _ = json.Marshal(disk)
				effective, _ := json.Marshal(st.Config)
				return bytes.Equal(encoded, effective)
			})
			if err != nil {
				return fmt.Errorf("%s expected %s, last=%v: %w", a[0], a[1], last, err)
			}
			s.result.Assertions = append(s.result.Assertions, "persisted "+a[0]+" = "+a[1])
			return nil
		},
		// ui-value checks what a control shows. The driver cannot choose from
		// macOS pop-up menus; the data-binding acceptance chooses them.
		"ui-value": func(s *desktopScenario, a []string) error {
			if len(a) != 3 {
				return errors.New("ui-value role label value")
			}
			last := ""
			err := s.wait(func() bool {
				node, e := s.find(a[0], a[1])
				last = node.Value
				return e == nil && node.Value == a[2]
			})
			if err != nil {
				return fmt.Errorf("%s %q shows %q, want %q: %w", a[0], a[1], last, a[2], err)
			}
			s.result.Assertions = append(s.result.Assertions, fmt.Sprintf("%s %q shows %q", a[0], a[1], a[2]))
			return nil
		},
		"restart-required": func(s *desktopScenario, a []string) error {
			if len(a) != 1 {
				return errors.New("restart-required key")
			}
			err := s.wait(func() bool {
				st, e := s.client.Config(s.ctx)
				return e == nil && slices.Contains(st.RestartRequired, a[0])
			})
			if err == nil {
				s.result.Assertions = append(s.result.Assertions, "restart required: "+a[0])
			}
			return err
		},
		"restart-clear": func(s *desktopScenario, a []string) error {
			if len(a) != 0 {
				return errors.New("restart-clear takes no arguments")
			}
			err := s.wait(func() bool { st, e := s.client.Config(s.ctx); return e == nil && len(st.RestartRequired) == 0 })
			if err == nil {
				s.result.Assertions = append(s.result.Assertions, "no pending restart settings")
			}
			return err
		},
		"plugin-probe": func(s *desktopScenario, a []string) error {
			if len(a) != 2 {
				return errors.New("plugin-probe kind disposition-or-error")
			}
			snap, e := s.pluginSnapshot(a[0])
			if e != nil {
				return e
			}
			r, e := s.client.Submit(s.ctx, api.SubmitInput{RequestID: fmt.Sprintf("probe-%d", time.Now().UnixNano()), Snapshot: snap})
			if a[1] == "sync_disabled" {
				var ae *api.Error
				if !errors.As(e, &ae) || ae.Reason != "sync_disabled" {
					return fmt.Errorf("expected sync_disabled, got %+v %v", r, e)
				}
			} else if e != nil || string(r.SyncDisposition) != a[1] {
				return fmt.Errorf("%s: expected %s, got %+v %v", a[0], a[1], r, e)
			}
			if a[1] == "queued" {
				if err := s.verifyPeerContent(snap); err != nil {
					return err
				}
			}
			s.result.Assertions = append(s.result.Assertions, "live Engine "+a[0]+" = "+a[1])
			return nil
		},
		"plugin-auto-probe": func(s *desktopScenario, a []string) error { return s.probeAutomaticPolicy(a) },
		"processor-probe": func(s *desktopScenario, a []string) error {
			if len(a) != 1 || (a[0] != "enabled" && a[0] != "disabled") {
				return errors.New("processor-probe enabled|disabled")
			}
			r, e := s.client.Submit(s.ctx, api.SubmitInput{RequestID: fmt.Sprintf("processor-%d", time.Now().UnixNano()), Snapshot: clip.Snapshot{Parts: []clip.Part{{Format: clip.Text, Data: []byte("plugin acceptance person@example.com " + strconv.FormatInt(time.Now().UnixNano(), 10))}}}})
			if e != nil {
				return e
			}
			it, e := s.client.GetHistory(s.ctx, string(r.HistoryID))
			if e != nil {
				return e
			}
			enabled := a[0] == "enabled"
			if slices.Contains(it.Tags, "builtin:text") != enabled || slices.Contains(it.Tags, "email") != enabled {
				return fmt.Errorf("processor tags do not match %s: %v", a[0], it.Tags)
			}
			actions, e := s.client.Actions(s.ctx)
			if e != nil {
				return e
			}
			count := 0
			for _, action := range actions {
				if action.PluginID == "builtin.text" {
					count++
				}
			}
			if (count == 4) != enabled || (!enabled && count != 0) {
				return fmt.Errorf("text Actions do not match %s: %d", a[0], count)
			}
			s.result.Assertions = append(s.result.Assertions, "real history classification/email tags and four text Actions: "+a[0])
			return nil
		},
		"restart-desktop": func(s *desktopScenario, a []string) error {
			if len(a) > 1 || (len(a) == 1 && a[0] != "setup") {
				return errors.New("restart-desktop [setup]")
			}
			// Pause the recorder across the actual quit/reopen gap; retain all sampled frames.
			s.captureMu.Lock()
			defer s.captureMu.Unlock()
			if err := s.stopApp(); err != nil {
				return err
			}
			s.app = nil
			s.appDone = nil
			if err := s.launchNative(); err != nil {
				return err
			}
			if len(a) == 0 {
				if err := s.wait(func() bool { _, e := s.client.Status(s.ctx); return e == nil }); err != nil {
					return err
				}
			}
			s.result.Assertions = append(s.result.Assertions, "Desktop quit stopped its owned service; reopened the same profile")
			return nil
		},
	}
}

func (s *desktopScenario) pluginSnapshot(kind string) (clip.Snapshot, error) {
	id := strconv.FormatInt(time.Now().UnixNano(), 10)
	switch kind {
	case "text":
		return clip.Snapshot{Parts: []clip.Part{{Format: clip.Text, Data: []byte("policy probe " + id)}}}, nil
	case "image":
		var b bytes.Buffer
		img := image.NewNRGBA(image.Rect(0, 0, 2, 2))
		img.Pix[0] = uint8(time.Now().UnixNano())
		if e := png.Encode(&b, img); e != nil {
			return clip.Snapshot{}, e
		}
		return clip.Snapshot{Parts: []clip.Part{{Format: clip.PNG, Data: b.Bytes()}}}, nil
	case "file", "large-file", "directory", "large-directory":
		p := filepath.Join(s.home, "probe-"+id)
		size := 32
		if kind == "large-file" {
			size = 2 << 20
		}
		if kind == "directory" || kind == "large-directory" {
			if e := os.MkdirAll(filepath.Join(p, "empty"), 0700); e != nil {
				return clip.Snapshot{}, e
			}
			data := []byte(id)
			if kind == "large-directory" {
				data = bytes.Repeat([]byte("d"), 600<<10)
				copy(data, []byte(id))
				if e := os.WriteFile(filepath.Join(p, "second.txt"), data, 0600); e != nil {
					return clip.Snapshot{}, e
				}
			}
			if e := os.WriteFile(filepath.Join(p, "member.txt"), data, 0600); e != nil {
				return clip.Snapshot{}, e
			}
		} else if e := os.WriteFile(p, append([]byte(id), bytes.Repeat([]byte("x"), size-len(id))...), 0600); e != nil {
			return clip.Snapshot{}, e
		}
		return clip.Snapshot{Files: []clip.File{{Name: filepath.Base(p), Path: p, Size: int64(size), Directory: kind == "directory" || kind == "large-directory"}}}, nil
	}
	return clip.Snapshot{}, fmt.Errorf("unknown probe kind %q", kind)
}
