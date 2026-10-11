package commands

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"net/http"
	"strconv"
	"strings"

	"github.com/UniClipboard/UniClipboard/apps/cli-go/internal/cli"
	"github.com/UniClipboard/UniClipboard/apps/cli-go/internal/exitcode"
	"github.com/UniClipboard/UniClipboard/apps/cli-go/internal/output"
	"github.com/UniClipboard/UniClipboard/apps/cli-go/internal/session"
	"github.com/UniClipboard/UniClipboard/apps/cli-go/internal/ui"
	"github.com/UniClipboard/UniClipboard/packages/desktop-host-go/daemonclient"
)

const deviceGroupChoicesPath = "/member/device-group-choices"

// deviceGroupChoices keeps the daemon's `DeviceGroupChoicesDto` bytes for
// JSON passthrough next to the fields the human renderer reads.
type deviceGroupChoices struct {
	raw         json.RawMessage
	Revision    uint64 `json:"revision"`
	DeviceTrust struct {
		LocalDeviceID     string `json:"localDeviceId"`
		LocalMembership   string `json:"localMembership"`
		SpaceDeviceUpdate *struct {
			Phase         string  `json:"phase"`
			Reason        *string `json:"reason"`
			Recovery      *string `json:"recovery"`
			NextRetryAtMs *int64  `json:"nextRetryAtMs"`
		} `json:"spaceDeviceUpdate"`
		Devices []trustDevice `json:"devices"`
	} `json:"deviceTrust"`
	Issues []deviceGroupIssue `json:"issues"`
}

type deviceGroupIssue struct {
	IssueID string              `json:"issueId"`
	Choices []deviceGroupOption `json:"choices"`
	Reason  *deviceGroupReason  `json:"reason"`
}

type deviceGroupOption struct {
	ChoiceID          string              `json:"choiceId"`
	IsCurrentGroup    bool                `json:"isCurrentGroup"`
	RequiresRePairing bool                `json:"requiresRePairing"`
	MemberDeviceIDs   []string            `json:"memberDeviceIds"`
	MembersComplete   bool                `json:"membersComplete"`
	Members           []deviceGroupDevice `json:"members"`
	SourceDeviceIDs   []string            `json:"sourceDeviceIds"`
	Impact            *deviceGroupImpact  `json:"impact"`
}

// These presentation facts are owned by the daemon, not inferred by the CLI.
type deviceGroupDevice struct {
	DeviceID    string `json:"deviceId"`
	DisplayName string `json:"displayName"`
}

type deviceGroupReason struct {
	Kind            string `json:"kind"`
	DetailsComplete bool   `json:"detailsComplete"`
	Changes         []struct {
		Kind   string            `json:"kind"`
		Actor  deviceGroupDevice `json:"actor"`
		Target deviceGroupDevice `json:"target"`
	} `json:"changes"`
	Decisions []struct {
		Decision string            `json:"decision"`
		Device   deviceGroupDevice `json:"device"`
		Target   deviceGroupDevice `json:"target"`
	} `json:"decisions"`
}

type deviceGroupImpact struct {
	PausedDeviceIDs              []string `json:"pausedDeviceIds"`
	RequiresRejoinDeviceIDs      []string `json:"requiresRejoinDeviceIds"`
	PendingConfirmationDeviceIDs []string `json:"pendingConfirmationDeviceIds"`
	LocalDeviceOutcome           string   `json:"localDeviceOutcome"`
}

func queryDeviceGroupChoices(client *daemonclient.Client) (*deviceGroupChoices, error) {
	var raw json.RawMessage
	if err := client.Get(context.Background(), deviceGroupChoicesPath, &raw); err != nil {
		return nil, err
	}
	state := &deviceGroupChoices{raw: raw}
	if err := json.Unmarshal(raw, state); err != nil {
		return nil, &daemonclient.RequestError{Kind: daemonclient.ErrDecode, Path: deviceGroupChoicesPath, Err: errors.New("error decoding response body")}
	}
	return state, nil
}

// trustErrorOutput mirrors the Rust `TrustErrorOutput`.
type trustErrorOutput struct {
	OK             bool    `json:"ok"`
	Code           string  `json:"code"`
	Message        string  `json:"message"`
	CurrentIssueID *string `json:"current_issue_id"`
}

type choiceOutput struct {
	OK     bool            `json:"ok"`
	Result json.RawMessage `json:"result"`
	State  json.RawMessage `json:"state"`
}

func runMemberTrustStatus(ctx *cli.Context) int {
	asJSON := ctx.JSON()
	if !asJSON {
		ui.Header("Device groups")
	}
	lease, client, err := session.ConnectWithLease()
	if err != nil {
		return session.ExitCode(err)
	}
	defer lease.Release()
	state, err := queryDeviceGroupChoices(client)
	if err != nil {
		code, message := deviceGroupQueryError(err)
		return emitTrustError(asJSON, code, message, nil)
	}
	if asJSON {
		return output.EmitJSON(state.raw, "device group choices")
	}
	renderDeviceGroups(state)
	return exitcode.Success
}

func runMemberTrustChoose(ctx *cli.Context) int {
	asJSON := ctx.JSON()
	if !asJSON {
		ui.Header("Choose device group")
	}
	lease, client, err := session.ConnectWithLease()
	if err != nil {
		return session.ExitCode(err)
	}
	defer lease.Release()
	state, err := queryDeviceGroupChoices(client)
	if err != nil {
		code, message := deviceGroupQueryError(err)
		return emitTrustError(asJSON, code, message, nil)
	}

	interactive := !asJSON && !ctx.Has("issue") && !ctx.Has("choice") && ui.StderrIsTerminal()
	if interactive {
		renderDeviceGroups(state)
	}

	firstRound := true
	for {
		var issueInput *string
		if ctx.Has("issue") {
			v := ctx.String("issue")
			issueInput = &v
		}

		var issue *deviceGroupIssue
		switch {
		case issueInput != nil:
			for i := range state.Issues {
				if state.Issues[i].IssueID == *issueInput {
					issue = &state.Issues[i]
					break
				}
			}
			if issue == nil {
				return emitTrustError(asJSON, "device_group_state_changed", "The requested issue is no longer current; review status.", nil)
			}
		case len(state.Issues) == 0:
			return emitTrustError(asJSON, "no_device_group_issues", "There are no device group issues.", nil)
		case len(state.Issues) == 1 || interactive:
			issue = &state.Issues[0]
		default:
			return emitTrustError(asJSON, "issue_id_required", "Multiple issues are available; pass --issue with an ID from status.", nil)
		}
		issueID := issue.IssueID
		if interactive && len(state.Issues) > 1 {
			ui.Info("handling issue", fmt.Sprintf("1 of %d", len(state.Issues)))
		}

		var choiceInput *string
		if ctx.Has("choice") {
			v := ctx.String("choice")
			choiceInput = &v
		} else if interactive && len(issue.Choices) > 1 {
			number, err := inputDeviceGroupChoice(len(issue.Choices))
			if err != nil {
				return emitTrustError(false, "selection_failed", err.Error(), &issueID)
			}
			v := issue.Choices[number].ChoiceID
			choiceInput = &v
		}
		var choice *deviceGroupOption
		switch {
		case choiceInput != nil:
			for i := range issue.Choices {
				if issue.Choices[i].ChoiceID == *choiceInput {
					choice = &issue.Choices[i]
					break
				}
			}
			if choice == nil {
				return emitTrustError(asJSON, "device_group_state_changed", "The requested choice is no longer current; review status.", &issueID)
			}
		case len(issue.Choices) == 0:
			return emitTrustError(asJSON, "no_device_group_choices", "The selected issue has no available choices.", &issueID)
		case len(issue.Choices) == 1:
			choice = &issue.Choices[0]
		default:
			return emitTrustError(asJSON, "choice_id_required", "Multiple choices are available; pass --choice with an ID from status.", &issueID)
		}

		confirmLocalRemoval := firstRound && ctx.Bool("confirm-local-removal")
		if choiceRemovesLocalDevice(choice, state.DeviceTrust.LocalDeviceID) && !confirmLocalRemoval {
			if !interactive {
				return emitTrustError(asJSON, "local_removal_confirmation_required",
					"This choice removes this device; pass --confirm-local-removal.", &issueID)
			}
			confirmed, err := ui.Confirm("This choice removes this device from the space. Continue?", false)
			if err != nil {
				return emitTrustError(false, "confirmation_failed", err.Error(), &issueID)
			}
			if !confirmed {
				ui.End("No device group choice was made.")
				return exitcode.Success
			}
			confirmLocalRemoval = true
		}

		request := struct {
			IssueID             string `json:"issueId"`
			ChoiceID            string `json:"choiceId"`
			ExpectedRevision    uint64 `json:"expectedRevision"`
			ConfirmLocalRemoval bool   `json:"confirmLocalRemoval"`
		}{issueID, choice.ChoiceID, state.Revision, confirmLocalRemoval}
		var rawResult json.RawMessage
		err = client.Enveloped(context.Background(), daemonclient.Request{Method: http.MethodPost, Path: deviceGroupChoicesPath, JSON: request}, &rawResult)
		if err != nil {
			return emitTrustError(asJSON, "device_group_choice_failed",
				"Failed to choose device group: "+daemonclient.DisplayMessage(err), &issueID)
		}
		latest, err := queryDeviceGroupChoices(client)
		if err != nil {
			return emitTrustError(asJSON, "device_group_refresh_failed",
				"Choice was submitted, but current state could not be read: "+daemonclient.DisplayMessage(err), &issueID)
		}
		var result struct {
			Outcome string `json:"outcome"`
		}
		json.Unmarshal(rawResult, &result)
		code := emitDeviceGroupChoice(rawResult, latest, asJSON, result.Outcome)
		if !interactive || ctx.Bool("once") || code != exitcode.Success ||
			(result.Outcome != "completed" && result.Outcome != "already_completed") ||
			choiceRemovesLocalDevice(choice, state.DeviceTrust.LocalDeviceID) || len(latest.Issues) == 0 {
			return code
		}
		state = latest
		firstRound = false
	}
}

func emitDeviceGroupChoice(rawResult json.RawMessage, state *deviceGroupChoices, asJSON bool, outcome string) int {
	success := outcome != "state_changed" && outcome != "local_device_confirmation_required"
	code := exitcode.Success
	if !success {
		code = exitcode.Error
	}
	if asJSON {
		return output.EmitJSONWithCode(choiceOutput{OK: success, Result: rawResult, State: state.raw}, "device group choice", code)
	}
	switch outcome {
	case "completed":
		ui.Success("Device group choice completed.")
	case "pending":
		ui.Warn("Device group choice is saved and still being completed.")
	case "re_pairing_required":
		ui.Warn("Device group changed; affected devices must be paired again.")
	case "already_completed":
		ui.Success("Device group choice was already completed.")
	case "state_changed":
		ui.Error("Device group state changed; review the latest choices.")
	case "local_device_confirmation_required":
		ui.Error("This choice requires explicit local removal confirmation.")
	}
	renderDeviceGroups(state)
	return code
}

// deviceGroupQueryError keeps the operation-local query failures distinct:
// unlock and recovery are not fixed by retrying, while
// `device_group_choices_unavailable` is.
func deviceGroupQueryError(err error) (string, string) {
	switch daemonclient.ErrorCode(err) {
	case "device_group_choices_unlock_required":
		return "device_group_choices_unlock_required",
			"Device groups cannot be read while this space is locked. Unlock it, then try again."
	case "device_group_choices_recovery_required":
		return "device_group_choices_recovery_required",
			"Space membership needs recovery before device groups can be read. Retrying will not fix this; keep your existing data."
	}
	return "device_group_choices_unavailable", "Failed to query device groups: " + daemonclient.DisplayMessage(err)
}

func renderDeviceGroups(state *deviceGroupChoices) {
	trust := &state.DeviceTrust
	ui.Info("revision", strconv.FormatUint(state.Revision, 10))
	names := deviceGroupNames(state)
	ui.Info("local_device", deviceGroupLabel(trust.LocalDeviceID, names))
	ui.Info("local_membership", trust.LocalMembership)
	ui.Info("pending_issues", strconv.Itoa(len(state.Issues)))
	// Engine-owned aggregate status, printed verbatim by wire name; an absent
	// field takes the DTO default (`completed`).
	if update := trust.SpaceDeviceUpdate; update != nil {
		ui.Info("space_device_update", update.Phase)
		if update.Reason != nil {
			ui.Info("space_device_update_reason", *update.Reason)
		}
		if update.Recovery != nil {
			ui.Info("space_device_update_recovery", *update.Recovery)
		}
		if update.NextRetryAtMs != nil {
			ui.Info("space_device_update_next_retry_at_ms", strconv.FormatInt(*update.NextRetryAtMs, 10))
		}
	} else {
		ui.Info("space_device_update", "completed")
	}
	var removals []trustDevice
	seenRemovals := make(map[string]bool)
	for _, device := range trust.Devices {
		if !device.IsLocal && device.awaitingRemovalAck() && !seenRemovals[device.DeviceID] {
			seenRemovals[device.DeviceID] = true
			removals = append(removals, device)
		}
	}
	ui.Info("removal_notifications", strconv.Itoa(len(removals)))
	for _, device := range removals {
		ui.Info("removed_device", deviceGroupLabel(device.DeviceID, names)+" (notifying other devices)")
	}
	for i, issue := range state.Issues {
		ui.Bar()
		ui.Info("issue", fmt.Sprintf("%d of %d", i+1, len(state.Issues)))
		ui.Info("issue_id", issue.IssueID)
		renderDeviceGroupReason(issue.Reason, names)
		for j, choice := range issue.Choices {
			description := "Choose the candidate device group"
			if choice.IsCurrentGroup {
				description = "Keep the current device group"
			}
			switch choice.ChoiceID {
			case "apply":
				description = ui.Green("Approve the proposed removal")
			case "keep":
				description = ui.Red("Reject the proposed removal and keep the current device group")
			}
			ui.Info(strconv.Itoa(j+1), description)
			ui.Info("choice_id", choice.ChoiceID)
			ids := append([]string(nil), choice.MemberDeviceIDs...)
			for _, member := range choice.Members {
				ids = append(ids, member.DeviceID)
			}
			ui.Info("group devices", deviceGroupLabels(ids, names))
			if len(choice.SourceDeviceIDs) > 0 {
				ui.Info("group reported by", deviceGroupLabels(choice.SourceDeviceIDs, names))
			}
			if !choice.MembersComplete {
				ui.Warn("Device group details are incomplete; only known devices are shown.")
			}
			if choice.RequiresRePairing {
				ui.Warn("This choice requires affected devices to pair again.")
			}
			if impact := choice.Impact; impact != nil {
				if len(impact.PausedDeviceIDs) > 0 {
					ui.Info("sync paused", deviceGroupLabels(impact.PausedDeviceIDs, names))
				}
				if len(impact.RequiresRejoinDeviceIDs) > 0 {
					ui.Warn("Must rejoin: " + deviceGroupLabels(impact.RequiresRejoinDeviceIDs, names))
				}
				if len(impact.PendingConfirmationDeviceIDs) > 0 {
					ui.Info("waiting for confirmation", deviceGroupLabels(impact.PendingConfirmationDeviceIDs, names))
				}
				ui.Info("this device membership", state.DeviceTrust.LocalMembership+" -> "+impact.LocalDeviceOutcome)
			}
			if choiceRemovesLocalDevice(&choice, trust.LocalDeviceID) {
				ui.Warn(ui.Red("Destructive: this choice removes this device from the space."))
			}
		}
	}
}

func renderDeviceGroupReason(reason *deviceGroupReason, names map[string]string) {
	if reason == nil {
		ui.Info("reason", "Device group details are unavailable.")
		return
	}
	descriptions := map[string]string{
		"pending_removal":               "A proposed device removal needs your decision.",
		"different_removals":            "Devices report different device removals.",
		"removal_decision_disagreement": "Devices disagree on a proposed removal.",
		"diverged_history":              "Devices report different device group histories.",
	}
	description := descriptions[reason.Kind]
	if description == "" {
		description = "Devices need a device group decision."
	}
	ui.Info("reason", description)
	for _, change := range reason.Changes {
		action := ""
		switch change.Kind {
		case "removed_device":
			action = " removed "
			if reason.Kind == "pending_removal" {
				action = " proposed removing "
			}
		case "added_device":
			action = " added "
		default:
			continue
		}
		ui.Info("change", deviceGroupLabel(change.Actor.DeviceID, names)+action+deviceGroupLabel(change.Target.DeviceID, names))
	}
	for _, decision := range reason.Decisions {
		action := ""
		switch decision.Decision {
		case "accepted":
			action = " approved removing "
		case "rejected":
			action = " rejected removing "
		default:
			continue
		}
		ui.Info("decision", deviceGroupLabel(decision.Device.DeviceID, names)+action+deviceGroupLabel(decision.Target.DeviceID, names))
	}
	if !reason.DetailsComplete {
		ui.Warn("Some device group history details are unavailable.")
	}
}

func deviceGroupNames(state *deviceGroupChoices) map[string]string {
	names := make(map[string]string)
	add := func(id, name string) {
		if names[id] == "" && strings.TrimSpace(name) != "" {
			names[id] = name
		}
	}
	for _, device := range state.DeviceTrust.Devices {
		add(device.DeviceID, device.DisplayName)
	}
	for _, issue := range state.Issues {
		if reason := issue.Reason; reason != nil {
			for _, change := range reason.Changes {
				add(change.Actor.DeviceID, change.Actor.DisplayName)
				add(change.Target.DeviceID, change.Target.DisplayName)
			}
			for _, decision := range reason.Decisions {
				add(decision.Device.DeviceID, decision.Device.DisplayName)
				add(decision.Target.DeviceID, decision.Target.DisplayName)
			}
		}
		for _, choice := range issue.Choices {
			for _, member := range choice.Members {
				add(member.DeviceID, member.DisplayName)
			}
		}
	}
	return names
}

func deviceGroupLabel(id string, names map[string]string) string {
	short := []rune(id)
	if len(short) > 12 {
		short = append(short[:12], '…')
	}
	if name := names[id]; name != "" {
		return name + " (" + string(short) + ")"
	}
	if id == "" {
		return "unknown device"
	}
	return string(short)
}

func deviceGroupLabels(ids []string, names map[string]string) string {
	seen := make(map[string]bool)
	var labels []string
	for _, id := range ids {
		if !seen[id] {
			seen[id] = true
			labels = append(labels, deviceGroupLabel(id, names))
		}
	}
	if len(labels) == 0 {
		return "(none)"
	}
	return strings.Join(labels, ", ")
}

func choiceRemovesLocalDevice(choice *deviceGroupOption, localID string) bool {
	if choice.Impact != nil {
		switch choice.Impact.LocalDeviceOutcome {
		case "removed":
			return true
		case "active":
			return false
		}
	}
	// Retain the conservative confirmation guard for older, incomplete DTOs.
	return !containsValue(choice.MemberDeviceIDs, localID)
}

func inputDeviceGroupChoice(count int) (int, error) {
	for {
		value, err := ui.Input(fmt.Sprintf("Choice number (1-%d)", count), true)
		if err != nil {
			return 0, err
		}
		number, err := strconv.Atoi(strings.TrimSpace(value))
		if err == nil && number >= 1 && number <= count {
			return number - 1, nil
		}
		ui.Warn(fmt.Sprintf("Enter a choice number from 1 to %d. No choice was submitted.", count))
	}
}

func emitTrustError(asJSON bool, code, message string, currentIssueID *string) int {
	if asJSON {
		return output.EmitJSONWithCode(trustErrorOutput{OK: false, Code: code, Message: message, CurrentIssueID: currentIssueID},
			"device group error", exitcode.Error)
	}
	ui.Error(message)
	return exitcode.Error
}

func containsValue(values []string, value string) bool {
	for _, item := range values {
		if item == value {
			return true
		}
	}
	return false
}
