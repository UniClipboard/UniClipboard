package commands

import (
	"context"
	"encoding/json"
	"strconv"

	"github.com/UniClipboard/UniClipboard/apps/cli-go/internal/cli"
	"github.com/UniClipboard/UniClipboard/apps/cli-go/internal/exitcode"
	"github.com/UniClipboard/UniClipboard/apps/cli-go/internal/output"
	"github.com/UniClipboard/UniClipboard/apps/cli-go/internal/session"
	"github.com/UniClipboard/UniClipboard/apps/cli-go/internal/ui"
)

type statusOutput struct {
	SetupCompleted  bool               `json:"setup_completed"`
	EncryptionReady bool               `json:"encryption_ready"`
	SearchState     string             `json:"search_state"`
	SearchReason    *string            `json:"search_reason"`
	ProfileRecovery json.RawMessage    `json:"profile_recovery"`
	DeviceTrust     *deviceTrustStatus `json:"device_trust"`
}

type deviceTrustStatus struct {
	LocalMembership          string          `json:"local_membership"`
	CurrentChangeID          *string         `json:"current_change_id"`
	UpgradeRequiredDeviceIDs []string        `json:"upgrade_required_device_ids"`
	BlockedReason            json.RawMessage `json:"blocked_reason"`
}

type profileRecovery struct {
	State           string `json:"state"`
	BackgroundReady bool   `json:"backgroundReady"`
	Admission       *struct {
		Category string `json:"category"`
		Stage    string `json:"stage"`
		Action   string `json:"action"`
	} `json:"admission"`
}

func runStatus(ctx *cli.Context) int {
	warnLegacySpaceCommand("status", "space status")
	return runSpaceStatus(ctx)
}

func runSpaceStatus(ctx *cli.Context) int {
	lease, client, err := session.ConnectWithLease()
	if err != nil {
		return session.ExitCode(err)
	}
	defer lease.Release()
	bg := context.Background()

	var rawRecovery json.RawMessage
	if err := client.Get(bg, "/encryption/recovery", &rawRecovery); err != nil {
		ui.Error("Failed to query profile recovery status: " + err.Error())
		return exitcode.Error
	}
	var recovery profileRecovery
	json.Unmarshal(rawRecovery, &recovery)
	var encryption struct {
		SessionReady bool `json:"sessionReady"`
	}
	if err := client.Get(bg, "/encryption/state", &encryption); err != nil {
		ui.Error("Failed to query encryption status: " + err.Error())
		return exitcode.Error
	}
	out := statusOutput{SetupCompleted: true, EncryptionReady: encryption.SessionReady, ProfileRecovery: rawRecovery}
	if !recovery.BackgroundReady {
		reason := "profile_recovery_required"
		out.SearchState, out.SearchReason = "unavailable", &reason
		return emitStatus(out, recovery, ctx.JSON())
	}
	var search struct {
		State  string  `json:"state"`
		Reason *string `json:"reason"`
	}
	if err := client.Get(bg, "/search/status", &search); err != nil {
		ui.Error("Failed to query search status: " + err.Error())
		return exitcode.Error
	}
	out.SearchState, out.SearchReason = search.State, search.Reason
	var choices struct {
		DeviceTrust struct {
			LocalMembership string `json:"localMembership"`
			CurrentChange   *struct {
				ChangeID string `json:"changeId"`
			} `json:"currentChange"`
			Devices []struct {
				DeviceID      string `json:"deviceId"`
				Compatibility string `json:"compatibility"`
			} `json:"devices"`
			BlockedReason json.RawMessage `json:"blockedReason"`
		} `json:"deviceTrust"`
	}
	if err := client.Get(bg, "/member/device-group-choices", &choices); err != nil {
		ui.Error("Failed to query device trust status: " + err.Error())
		return exitcode.Error
	}
	trust := &deviceTrustStatus{LocalMembership: choices.DeviceTrust.LocalMembership, UpgradeRequiredDeviceIDs: []string{}, BlockedReason: nullIfEmpty(choices.DeviceTrust.BlockedReason)}
	if change := choices.DeviceTrust.CurrentChange; change != nil {
		trust.CurrentChangeID = &change.ChangeID
	}
	for _, d := range choices.DeviceTrust.Devices {
		if d.Compatibility == "upgrade_required" {
			trust.UpgradeRequiredDeviceIDs = append(trust.UpgradeRequiredDeviceIDs, d.DeviceID)
		}
	}
	out.DeviceTrust = trust
	return emitStatus(out, recovery, ctx.JSON())
}

func nullIfEmpty(raw json.RawMessage) json.RawMessage {
	if len(raw) == 0 {
		return json.RawMessage("null")
	}
	return raw
}

func yesNo(b bool) string {
	if b {
		return "yes"
	}
	return "no"
}

func emitStatus(out statusOutput, recovery profileRecovery, asJSON bool) int {
	if asJSON {
		return output.EmitJSON(out, "status response")
	}
	ui.Info("Setup completed", yesNo(out.SetupCompleted))
	ui.Info("Encryption ready", yesNo(out.EncryptionReady))
	ui.Info("Search state", out.SearchState)
	reason := "none"
	if out.SearchReason != nil {
		reason = *out.SearchReason
	}
	ui.Info("Search reason", reason)
	ui.Info("Profile recovery", recoveryStateLabels[recovery.State])
	if a := recovery.Admission; a != nil {
		ui.Info("Recovery category", admissionCategoryLabels[a.Category])
		ui.Info("Recovery stage", admissionStageLabels[a.Stage])
		ui.Info("Recommended action", admissionActionLabels[a.Action])
		ui.Warn("Existing data has not been deleted.")
		ui.Info("Diagnostics", "run `uniclip debug export-logs`")
	}
	if t := out.DeviceTrust; t != nil {
		ui.Info("Device membership", t.LocalMembership)
		change := "none"
		if t.CurrentChangeID != nil {
			change = *t.CurrentChangeID
		}
		ui.Info("Device trust change", change)
		ui.Info("Devices requiring update", strconv.Itoa(len(t.UpgradeRequiredDeviceIDs)))
	}
	return exitcode.Success
}
