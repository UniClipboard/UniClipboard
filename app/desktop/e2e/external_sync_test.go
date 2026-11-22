package e2e

import (
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"os"
	"os/exec"
	"path/filepath"
	"runtime"
	"strings"

	"github.com/uniclipboard/unic/internal/clip"
	api "github.com/uniclipboard/unic/internal/localapi"
)

// Failure inventory: docs/exec/sync-plugin-workflow.md. This seeds only the
// installed fixture artifact; real GitHub installation is covered by CLI E2E.
func (s *desktopScenario) seedExternalPlugin(home string) error {
	binary := filepath.Join(s.private, "loopback-plugin")
	if _, err := os.Stat(binary); os.IsNotExist(err) {
		cmd := exec.CommandContext(s.ctx, "go", "build", "-race", "-o", binary, "./plugins/examples/loopback/cmd/plugin")
		cmd.Dir = s.root
		if out, err := cmd.CombinedOutput(); err != nil {
			return fmt.Errorf("build external fixture: %w (%s)", err, out)
		}
	}
	program, err := os.ReadFile(binary)
	if err != nil {
		return err
	}
	schema, err := os.ReadFile(filepath.Join(s.root, "plugins/examples/loopback/settings.schema.json"))
	if err != nil {
		return err
	}
	hash := func(data []byte) string { h := sha256.Sum256(data); return hex.EncodeToString(h[:]) }
	manifest := fmt.Sprintf("manifest_version=1\nid='e2e.tcp'\nversion='1.0.0'\napi_version=1\nkind='go'\nmodule='example.com/loopback'\npackage='.'\ncapabilities=['sync_backend']\ntransport_kind='peer_sessions'\nsettings_schema='settings.schema.json'\nsettings_sha256='%s'\n", hash(schema))
	commit := strings.Repeat("d", 40)
	installed := filepath.Join(home, "plugins", "installed")
	dir := filepath.Join(installed, "e2e.tcp", commit)
	if err := os.MkdirAll(filepath.Join(dir, "bin"), 0700); err != nil {
		return err
	}
	for name, data := range map[string][]byte{"spec.toml": []byte(manifest), "settings.schema.json": schema} {
		if err := os.WriteFile(filepath.Join(dir, name), data, 0600); err != nil {
			return err
		}
	}
	name := "plugin"
	if runtime.GOOS == "windows" {
		name += ".exe"
	}
	if err := os.WriteFile(filepath.Join(dir, "bin", name), program, 0700); err != nil {
		return err
	}
	lock := map[string]any{"e2e.tcp": map[string]any{"repository": "https://github.com/example/loopback", "commit": commit, "version": "1.0.0", "api_version": 1, "source": "acceptance-fixture", "manifest_sha256": hash([]byte(manifest)), "sha256": hash(program), "goos": runtime.GOOS, "goarch": runtime.GOARCH}}
	data, err := json.Marshal(lock)
	if err != nil {
		return err
	}
	if err := os.WriteFile(filepath.Join(installed, "installed.lock"), data, 0600); err != nil {
		return err
	}
	s.result.Assertions = append(s.result.Assertions, "external fixture program sha256="+hash(program)+"; installed fixture seed, not a production network protocol")
	return nil
}

func (s *desktopScenario) externalRoundtrip() error {
	if s.policyPeer == nil {
		return errors.New("external roundtrip requires owned peer")
	}
	for _, flow := range []struct {
		from, to      *api.Client
		text, backend string
	}{{s.client, s.policyPeer.client, "external-desktop-to-peer", "e2e.desktop"}, {s.policyPeer.client, s.client, "external-peer-to-desktop", "e2e.peer"}} {
		result, err := flow.from.Submit(s.ctx, api.SubmitInput{RequestID: flow.text, Snapshot: clip.Snapshot{Parts: []clip.Part{{Format: clip.Text, Data: []byte(flow.text)}}}})
		if err != nil {
			return err
		}
		if string(result.SyncDisposition) != "queued" {
			return errors.New("external submission was not queued")
		}
		if err := s.wait(func() bool {
			page, e := flow.to.ListHistory(s.ctx, "", "")
			if e != nil {
				return false
			}
			for _, item := range page.Items {
				if item.Kind == "text" {
					content, e := flow.to.HistoryContent(s.ctx, item.ID, 0)
					if e != nil {
						continue
					}
					data, e := io.ReadAll(io.LimitReader(content, 8192))
					_ = content.Close()
					if e == nil && string(data) == flow.text {
						return true
					}
				}
			}
			return false
		}); err != nil {
			return err
		}
		if err := s.wait(func() bool {
			page, e := flow.from.ListHistory(s.ctx, "", "")
			if e != nil {
				return false
			}
			for _, item := range page.Items {
				for _, backend := range item.Backends {
					if backend.BackendID == flow.backend && backend.State == "sent" {
						return true
					}
				}
			}
			return false
		}); err != nil {
			return err
		}
	}
	s.result.Assertions = append(s.result.Assertions, "external non-Tailcat cross-instance text transfers verified byte for byte in both directions with per-instance sent results")
	return nil
}
