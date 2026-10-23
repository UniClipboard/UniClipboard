package commands

import (
	"context"
	"fmt"
	"strings"
	"time"

	"github.com/UniClipboard/UniClipboard/apps/cli-go/internal/exitcode"
	"github.com/UniClipboard/UniClipboard/apps/cli-go/internal/ui"
	"github.com/UniClipboard/UniClipboard/packages/desktop-host-go/daemonclient"
)

// targetPollInterval is the poll interval while waiting for target devices.
const targetPollInterval = 500 * time.Millisecond

// peerSnapshot holds the `PeerSnapshotDto` fields the target wait reads.
type peerSnapshot struct {
	PeerID    string `json:"peerId"`
	IsPaired  bool   `json:"isPaired"`
	Connected bool   `json:"connected"`
}

// targetReadiness decides whether the send targets are connected, using only
// the daemon's reported `connected` state. With `--peer`, every listed device
// must be connected; without it, one connected paired device is enough.
// It returns ready, or the reason it is still waiting. No paired device at
// all counts as ready: waiting cannot help and dispatch reports it.
func targetReadiness(peers []peerSnapshot, requested []string) (bool, string) {
	if len(requested) == 0 {
		paired := 0
		for _, peer := range peers {
			if peer.IsPaired {
				paired++
				if peer.Connected {
					return true, ""
				}
			}
		}
		if paired == 0 {
			return true, ""
		}
		return false, fmt.Sprintf("none of %d paired device(s) is connected", paired)
	}
	var pending []string
	for _, id := range requested {
		var found *peerSnapshot
		for i := range peers {
			if peers[i].PeerID == id {
				found = &peers[i]
				break
			}
		}
		switch {
		case found == nil:
			pending = append(pending, id+" (unknown device)")
		case !found.Connected:
			pending = append(pending, id+" (offline)")
		}
	}
	if len(pending) == 0 {
		return true, ""
	}
	return false, strings.Join(pending, ", ")
}

// waitForSendTargets polls the peer list until the targets are connected or
// the deadline passes. Only read-only queries are repeated.
func waitForSendTargets(requested []string, deadline time.Time, budgetSecs uint64) int {
	client, err := daemonclient.FromEnv()
	if err != nil {
		ui.Error("Failed to connect to daemon: " + err.Error())
		return exitcode.Error
	}
	var spinner *ui.Spinner
	defer func() {
		if spinner != nil {
			spinner.Clear()
		}
	}()
	waiting := pollSendTargets(requested, deadline, func() ([]peerSnapshot, error) {
		var peers []peerSnapshot
		err := client.Get(context.Background(), "/peers", &peers)
		return peers, err
	}, func() {
		if spinner == nil {
			spinner = ui.NewSpinner("Waiting for target device(s) to connect...")
		}
	})
	if waiting == "" {
		return exitcode.Success
	}
	spinner.Clear()
	ui.Error(fmt.Sprintf("Timed out after %ds waiting for target device(s) to connect: %s. Nothing was sent.", budgetSecs, waiting))
	ui.Warn("Bring the device online, check `uniclip members`, or raise --connect-timeout.")
	return exitcode.Error
}

// pollSendTargets returns the last waiting reason on timeout, or an empty
// string to let dispatch report its authoritative result. Only queries repeat.
func pollSendTargets(requested []string, deadline time.Time, query func() ([]peerSnapshot, error), onWaiting func()) string {
	for {
		peers, err := query()
		if err != nil {
			return ""
		}
		ready, waiting := targetReadiness(peers, requested)
		if ready {
			return ""
		}
		onWaiting()
		if !time.Now().Before(deadline) {
			return waiting
		}
		pause := targetPollInterval
		if remaining := time.Until(deadline); remaining < pause {
			pause = max(remaining, 0)
		}
		time.Sleep(pause)
	}
}
