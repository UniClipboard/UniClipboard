//go:build unix

package e2e

import (
	"errors"
	"os"
	"path/filepath"
	"strconv"
	"syscall"

	"github.com/uniclipboard/unic/internal/clip"
	api "github.com/uniclipboard/unic/internal/localapi"
)

// peerStallCommands make a send visibly slow without a test transport: a paused peer stops
// reading, so the sender stalls at a stable partial byte count until the peer resumes.
func peerStallCommands() map[string]func(*desktopScenario, []string) error {
	signal := func(sig syscall.Signal) func(*desktopScenario, []string) error {
		return func(s *desktopScenario, a []string) error {
			if len(a) != 0 || s.policyPeer == nil {
				return errors.New("peer signal takes no arguments and requires an owned peer")
			}
			return s.policyPeer.cmd.Process.Signal(sig)
		}
	}
	return map[string]func(*desktopScenario, []string) error{
		"peer-pause":  signal(syscall.SIGSTOP),
		"peer-resume": signal(syscall.SIGCONT),
		// peer-kill: end the peer abruptly; a send in flight fails. Its home stays for peer-restart.
		"peer-kill": func(s *desktopScenario, a []string) error {
			if len(a) != 0 || s.policyPeer == nil {
				return errors.New("peer-kill takes no arguments and requires an owned peer")
			}
			p := s.policyPeer
			_ = p.cmd.Process.Signal(syscall.SIGCONT)
			err := p.cmd.Process.Kill()
			<-p.done
			s.policyPeer = nil
			return err
		},
		// peer-restart: start the killed peer again on its old home and wait until it is connected.
		"peer-restart": func(s *desktopScenario, a []string) error {
			if len(a) != 0 || s.policyPeer != nil {
				return errors.New("peer-restart takes no arguments and requires a killed peer")
			}
			if _, _, err := s.spawnPeer(); err != nil {
				return err
			}
			return s.wait(func() bool {
				st, e := s.client.Status(s.ctx)
				return e == nil && len(st.Peers) == 1 && st.Peers[0].Connected
			})
		},
		// peer-pause-at-bytes n: pause the peer once the newest entry's send passed n megabytes.
		"peer-pause-at-bytes": func(s *desktopScenario, a []string) error {
			if len(a) != 1 || s.policyPeer == nil {
				return errors.New("peer-pause-at-bytes megabytes requires an owned peer")
			}
			mb, err := strconv.Atoi(a[0])
			if err != nil || mb <= 0 {
				return errors.New("megabytes must be a positive integer")
			}
			err = s.wait(func() bool {
				page, e := s.client.ListHistory(s.ctx, "", "")
				if e != nil || len(page.Items) == 0 {
					return false
				}
				for _, t := range page.Items[0].Targets {
					for _, it := range t.Items {
						if it.State == "transferring" && it.CompletedBytes >= int64(mb)<<20 {
							return true
						}
					}
				}
				return false
			})
			if err != nil {
				return err
			}
			return s.policyPeer.cmd.Process.Signal(syscall.SIGSTOP)
		},
		// submit-sized-file id megabytes: a local-only history entry holding one file.
		"submit-sized-file": func(s *desktopScenario, a []string) error {
			if len(a) != 2 {
				return errors.New("submit-sized-file id megabytes")
			}
			mb, err := strconv.Atoi(a[1])
			if err != nil || mb <= 0 {
				return errors.New("megabytes must be a positive integer")
			}
			p := filepath.Join(s.home, "big-"+a[0]+".bin")
			if err = os.WriteFile(p, make([]byte, mb<<20), 0600); err != nil {
				return err
			}
			_, err = s.client.Submit(s.ctx, api.SubmitInput{RequestID: a[0], Snapshot: clip.Snapshot{Files: []clip.File{{Name: filepath.Base(p), Path: p, Size: int64(mb) << 20}}}})
			return err
		},
	}
}
