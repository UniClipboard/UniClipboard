package commands

import (
	"context"
	"encoding/json"
	"net/http"
	"strconv"

	"github.com/UniClipboard/UniClipboard/apps/cli-go/internal/cli"
	"github.com/UniClipboard/UniClipboard/apps/cli-go/internal/daemonclient"
	"github.com/UniClipboard/UniClipboard/apps/cli-go/internal/exitcode"
	"github.com/UniClipboard/UniClipboard/apps/cli-go/internal/output"
	"github.com/UniClipboard/UniClipboard/apps/cli-go/internal/session"
	"github.com/UniClipboard/UniClipboard/apps/cli-go/internal/ui"
)

// trustDevice is the subset of `DeviceTrustRelationshipDto` the CLI reads.
type trustDevice struct {
	DeviceID          string `json:"deviceId"`
	DisplayName       string `json:"displayName"`
	IsLocal           bool   `json:"isLocal"`
	Membership        string `json:"membership"`
	GroupRelationship string `json:"groupRelationship"`
}

// awaitingRemovalAck reports an Engine-declared removal whose notification
// to the other devices is still in flight.
func (d trustDevice) awaitingRemovalAck() bool {
	return d.Membership == "removed" && d.GroupRelationship == "awaiting_removal_acknowledgement"
}

// runMemberRemove records an offline-first removal intent for one member.
func runMemberRemove(ctx *cli.Context) int {
	asJSON := ctx.JSON()
	peerID, _ := ctx.Arg(0)
	if !asJSON {
		ui.Header("Member removal")
	}
	lease, client, err := session.ConnectWithLease()
	if err != nil {
		return session.ExitCode(err)
	}
	defer lease.Release()

	var raw json.RawMessage
	err = client.Enveloped(context.Background(), daemonclient.Request{
		Method: http.MethodPost,
		Path:   "/pairing/unpair",
		JSON: struct {
			PeerID string `json:"peerId"`
		}{peerID},
	}, &raw)
	if err != nil {
		ui.Error("Failed to remove member: " + daemonclient.DisplayMessage(err))
		return exitcode.Error
	}
	if asJSON {
		return output.EmitJSON(raw, "device trust")
	}
	var snapshot struct {
		Revision uint64        `json:"revision"`
		Devices  []trustDevice `json:"devices"`
	}
	json.Unmarshal(raw, &snapshot)
	ui.Success("Member removed from this device.")
	for _, device := range snapshot.Devices {
		if device.DeviceID == peerID && device.awaitingRemovalAck() {
			ui.Info("delivery", "notifying other devices")
			break
		}
	}
	ui.Info("revision", strconv.FormatUint(snapshot.Revision, 10))
	return exitcode.Success
}
