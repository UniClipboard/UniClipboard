package e2e

import (
	"bytes"
	"context"
	"crypto/sha256"
	"errors"
	"fmt"
	"io/fs"
	"os"
	"os/exec"
	"path/filepath"
	"slices"
	"strings"
	"time"

	"github.com/uniclipboard/unic/internal/clip"
	api "github.com/uniclipboard/unic/internal/localapi"
	"github.com/uniclipboard/unic/internal/testboard"
)

type pluginPolicyPeer struct {
	client *api.Client
	cmd    *exec.Cmd
	done   chan error
}

func (p *pluginPolicyPeer) stop() error {
	ctx, cancel := context.WithTimeout(context.Background(), 8*time.Second)
	defer cancel()
	st, err := p.client.Service(ctx)
	if err == nil {
		err = p.client.StopService(ctx, st.ID)
	}
	select {
	case exit := <-p.done:
		return errors.Join(err, exit)
	case <-ctx.Done():
		killErr := p.cmd.Process.Kill()
		waitErr := <-p.done
		return errors.Join(errors.New("policy peer required a forced stop"), killErr, waitErr)
	}
}
func (s *desktopScenario) connectPolicyPeer(args []string) error {
	if len(args) != 0 || s.policyPeer != nil {
		return errors.New("connect-policy-peer takes no arguments and creates one peer")
	}
	home := filepath.Join(s.private, "policy-peer")
	if err := os.MkdirAll(home, 0700); err != nil {
		return err
	}
	if err := os.WriteFile(filepath.Join(home, "config.toml"), []byte("[telemetry]\nenabled=false\n"), 0600); err != nil {
		return err
	}
	binary := filepath.Join(os.Getenv("UNIC_DESKTOP_E2E_BIN"), "unicd")
	if os.PathSeparator == '\\' {
		binary += ".exe"
	}
	cmd := exec.Command(binary)
	for _, v := range os.Environ() {
		if !strings.HasPrefix(v, "UNIC_") && !strings.HasPrefix(v, "MYGO_AUTOMATION=") {
			cmd.Env = append(cmd.Env, v)
		}
	}
	cmd.Env = append(cmd.Env, "UNIC_HOME="+home, "UNIC_RENDEZVOUS_URL=http://127.0.0.1:1")
	log, err := os.Create(filepath.Join(s.private, "policy-peer.log"))
	if err != nil {
		return err
	}
	cmd.Stdout = log
	cmd.Stderr = log
	if err = cmd.Start(); err != nil {
		_ = log.Close()
		return err
	}
	p := &pluginPolicyPeer{client: api.NewClient(filepath.Join(home, "unic.sock")), cmd: cmd, done: make(chan error, 1)}
	s.policyPeer = p
	go func() { err := cmd.Wait(); _ = log.Close(); p.done <- err }()
	var ticket string
	if err = s.wait(func() bool { var e error; ticket, e = p.client.ExportTicket(s.ctx); return e == nil }); err != nil {
		return err
	}
	if _, err = s.client.Pair(s.ctx, ticket); err != nil {
		return err
	}
	for _, client := range []*api.Client{s.client, p.client} {
		var pending []api.PendingPeer
		if err = s.wait(func() bool { var e error; pending, e = client.Pending(s.ctx); return e == nil && len(pending) == 1 }); err != nil {
			return err
		}
		req := pending[0]
		if _, err = client.Approve(s.ctx, req.ID, api.Approval{Fingerprint: req.Fingerprint, Revision: req.Revision, Bindings: req.Bindings}); err != nil {
			return err
		}
	}
	if err = s.wait(func() bool {
		st, e := s.client.Status(s.ctx)
		return e == nil && len(st.Peers) == 1 && st.Peers[0].Connected
	}); err != nil {
		return err
	}
	if err = s.policyBarrier(); err != nil {
		return err
	}
	s.result.Assertions = append(s.result.Assertions, "two production services paired with exact fingerprint/revision approvals and a delivered connectivity barrier")
	return nil
}
func (s *desktopScenario) policyBarrier() error {
	text := fmt.Sprintf("policy barrier %d", time.Now().UnixNano())
	if _, err := s.client.Submit(s.ctx, api.SubmitInput{RequestID: text, Snapshot: clip.Snapshot{Parts: []clip.Part{{Format: clip.Text, Data: []byte(text)}}}}); err != nil {
		return err
	}
	return s.wait(func() bool {
		page, e := s.policyPeer.client.ListHistory(s.ctx, "", "")
		if e != nil {
			return false
		}
		for _, item := range page.Items {
			if item.Kind != "text" {
				continue
			}
			var b bytes.Buffer
			if s.policyPeer.client.CopyHistoryContent(s.ctx, item.ID, 0, &b) == nil && b.String() == text {
				return true
			}
		}
		return false
	})
}
func (s *desktopScenario) probeAutomaticPolicy(a []string) error {
	if len(a) != 2 || s.policyPeer == nil {
		return errors.New("plugin-auto-probe requires kind, disposition and connected policy peer")
	}
	snap, err := s.pluginSnapshot("large-" + a[0])
	if err != nil {
		return err
	}
	before, err := s.policyPeer.client.ListHistory(s.ctx, "", "")
	if err != nil {
		return err
	}
	local, err := s.client.ListHistory(s.ctx, "", "")
	if err != nil {
		return err
	}
	seen := map[string]bool{}
	for _, it := range local.Items {
		seen[it.ID] = true
	}
	if err = testboard.Dir(filepath.Join(s.home, "board")).Write(snap); err != nil {
		return err
	}
	if a[1] == "queued" {
		return s.verifyPeerContent(snap)
	}
	if a[1] != "auto_file_too_large" {
		return errors.New("unsupported automatic policy assertion")
	}
	// History intentionally omits backend summaries when no send is attempted.
	// Require a fresh local capture, a bounded receiver-side absence interval,
	// then a delivered text barrier on the same live connection.
	if err = s.wait(func() bool {
		page, e := s.client.ListHistory(s.ctx, "", "")
		if e != nil {
			return false
		}
		for _, it := range page.Items {
			if !seen[it.ID] && it.Origin == "local_system" && ((a[0] == "file" && it.Kind == "files") || (a[0] == "directory" && it.Kind == "directory")) {
				return true
			}
		}
		return false
	}); err != nil {
		return err
	}
	until := time.Now().Add(1500 * time.Millisecond)
	for time.Now().Before(until) {
		page, e := s.policyPeer.client.ListHistory(s.ctx, "", "")
		if e != nil {
			return e
		}
		if len(page.Items) != len(before.Items) {
			return errors.New("automatic oversized content reached the receiver")
		}
		select {
		case <-s.ctx.Done():
			return s.ctx.Err()
		case <-time.After(80 * time.Millisecond):
		}
	}
	if err = s.policyBarrier(); err != nil {
		return err
	}
	s.result.Assertions = append(s.result.Assertions, "automatic "+a[0]+" retained locally; receiver unchanged for 1.5s; live connection verified by delivered barrier")
	return nil
}
func (s *desktopScenario) verifyPeerContent(snap clip.Snapshot) error {
	if s.policyPeer == nil {
		return errors.New("content verification requires policy peer")
	}
	var expected []string
	for _, file := range snap.Files {
		err := filepath.WalkDir(file.Path, func(path string, entry fs.DirEntry, err error) error {
			if err != nil {
				return err
			}
			if entry.IsDir() {
				return nil
			}
			b, e := os.ReadFile(path)
			if e != nil {
				return e
			}
			expected = append(expected, fmt.Sprintf("%x", sha256.Sum256(b)))
			return nil
		})
		if err != nil {
			return err
		}
	}
	slices.Sort(expected)
	err := s.wait(func() bool {
		page, e := s.policyPeer.client.ListHistory(s.ctx, "", "")
		if e != nil {
			return false
		}
		for _, item := range page.Items {
			if item.Origin != "remote_device" || (snap.Files[0].Directory && item.Kind != "directory") || (!snap.Files[0].Directory && item.Kind != "files") {
				continue
			}
			var actual []string
			for i, part := range item.Parts {
				if part.Kind != "file" {
					continue
				}
				var b bytes.Buffer
				if s.policyPeer.client.CopyHistoryContent(s.ctx, item.ID, i, &b) != nil {
					return false
				}
				actual = append(actual, fmt.Sprintf("%x", sha256.Sum256(b.Bytes())))
			}
			slices.Sort(actual)
			if slices.Equal(actual, expected) {
				return true
			}
		}
		return false
	})
	if err == nil {
		s.result.Assertions = append(s.result.Assertions, fmt.Sprintf("receiver content SHA-256 matches all %d source file payloads: %v", len(expected), expected))
	}
	return err
}
