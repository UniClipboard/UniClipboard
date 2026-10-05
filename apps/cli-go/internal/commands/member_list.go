package commands

import (
	"context"
	"fmt"
	"net/http"

	"github.com/UniClipboard/UniClipboard/apps/cli-go/internal/cli"
	"github.com/UniClipboard/UniClipboard/apps/cli-go/internal/daemonclient"
	"github.com/UniClipboard/UniClipboard/apps/cli-go/internal/exitcode"
	"github.com/UniClipboard/UniClipboard/apps/cli-go/internal/output"
	"github.com/UniClipboard/UniClipboard/apps/cli-go/internal/session"
	"github.com/UniClipboard/UniClipboard/apps/cli-go/internal/ui"
)

// memberEntry is one roster row; field order matches the Rust `MemberDto`.
type memberEntry struct {
	DeviceID   string `json:"device_id"`
	DeviceName string `json:"device_name"`
	IsLocal    bool   `json:"is_local"`
	State      string `json:"state"`
}

// runMembers is the hidden, deprecated `members` / `devices` alias.
func runMembers(ctx *cli.Context) int {
	ui.Warn("`uniclip members` and `uniclip devices` are deprecated; use `uniclip member list` instead.")
	return runMemberList(ctx)
}

// runMemberList lists the local device plus paired peers with each peer's
// reachability; `--probe` refreshes presence first.
func runMemberList(ctx *cli.Context) int {
	asJSON := ctx.JSON()
	if !asJSON {
		ui.Header("Members")
	}
	lease, client, err := session.ConnectWithLease()
	if err != nil {
		return session.ExitCode(err)
	}
	defer lease.Release()
	bg := context.Background()

	if ctx.Bool("probe") {
		var spinner *ui.Spinner
		if !asJSON {
			spinner = ui.NewSpinner("Probing paired peers...")
		}
		var report struct {
			Total   uint32 `json:"total"`
			Online  uint32 `json:"online"`
			Offline uint32 `json:"offline"`
			Errors  uint32 `json:"errors"`
		}
		err := client.Enveloped(bg, daemonclient.Request{Method: http.MethodPost, Path: "/presence/refresh"}, &report)
		switch {
		case err == nil && spinner != nil:
			spinner.FinishSuccess(fmt.Sprintf("Probed %d peer(s): %d online, %d offline, %d error(s)",
				report.Total, report.Online, report.Offline, report.Errors))
		case err != nil && spinner != nil:
			spinner.FinishError(fmt.Sprintf("Probe round failed: %v (showing last-known state)", err))
		case err != nil:
			ui.Warn(fmt.Sprintf("Probe round failed: %v (showing last-known state)", err))
		}
	}

	var remote []struct {
		PeerID     string `json:"peerId"`
		DeviceName string `json:"deviceName"`
		Channel    string `json:"channel"`
	}
	if err := client.Get(bg, "/paired-devices", &remote); err != nil {
		ui.Error("Failed to list paired devices: " + err.Error())
		return exitcode.Error
	}

	entries := make([]memberEntry, 0, 1+len(remote))
	// The local device is optional: a failed lookup only omits its row.
	var local struct {
		PeerID     string `json:"peerId"`
		DeviceName string `json:"deviceName"`
	}
	if err := client.Get(bg, "/device/me", &local); err == nil {
		entries = append(entries, memberEntry{DeviceID: local.PeerID, DeviceName: local.DeviceName, IsLocal: true, State: "online"})
	}
	for _, member := range remote {
		state := "unknown"
		switch member.Channel {
		case "direct", "relay":
			state = "online"
		case "offline":
			state = "offline"
		}
		entries = append(entries, memberEntry{DeviceID: member.PeerID, DeviceName: member.DeviceName, State: state})
	}

	if asJSON {
		return output.EmitJSON(entries, "roster")
	}
	ui.Bar()
	if len(entries) == 0 {
		ui.Info("members", "(none)")
	} else {
		for _, entry := range entries {
			tag := ""
			if entry.IsLocal {
				tag = " [local]"
			}
			ui.Info("·", fmt.Sprintf("%s (%s)%s", entry.DeviceName, entry.State, tag))
		}
	}
	ui.Bar()
	return exitcode.Success
}
