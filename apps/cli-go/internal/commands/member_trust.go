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
	"github.com/UniClipboard/UniClipboard/apps/cli-go/internal/daemonclient"
	"github.com/UniClipboard/UniClipboard/apps/cli-go/internal/exitcode"
	"github.com/UniClipboard/UniClipboard/apps/cli-go/internal/output"
	"github.com/UniClipboard/UniClipboard/apps/cli-go/internal/session"
	"github.com/UniClipboard/UniClipboard/apps/cli-go/internal/ui"
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
}

type deviceGroupOption struct {
	ChoiceID          string   `json:"choiceId"`
	IsCurrentGroup    bool     `json:"isCurrentGroup"`
	RequiresRePairing bool     `json:"requiresRePairing"`
	MemberDeviceIDs   []string `json:"memberDeviceIds"`
	MembersComplete   bool     `json:"membersComplete"`
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

	interactive := !asJSON && ui.StderrIsTerminal()
	if interactive {
		renderDeviceGroups(state)
	}

	var issueInput *string
	if ctx.Has("issue") {
		v := ctx.String("issue")
		issueInput = &v
	} else if interactive && len(state.Issues) > 1 {
		v, err := ui.Input("Issue ID", false)
		if err != nil {
			return emitTrustError(false, "selection_failed", err.Error(), nil)
		}
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
	case len(state.Issues) == 1:
		issue = &state.Issues[0]
	default:
		return emitTrustError(asJSON, "issue_id_required", "Multiple issues are available; pass --issue with an ID from status.", nil)
	}
	issueID := issue.IssueID

	var choiceInput *string
	if ctx.Has("choice") {
		v := ctx.String("choice")
		choiceInput = &v
	} else if interactive && len(issue.Choices) > 1 {
		v, err := ui.Input("Choice ID", false)
		if err != nil {
			return emitTrustError(false, "selection_failed", err.Error(), &issueID)
		}
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

	confirmLocalRemoval := ctx.Bool("confirm-local-removal")
	if !containsValue(choice.MemberDeviceIDs, state.DeviceTrust.LocalDeviceID) && !confirmLocalRemoval {
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
	return emitDeviceGroupChoice(rawResult, latest, asJSON)
}

func emitDeviceGroupChoice(rawResult json.RawMessage, state *deviceGroupChoices, asJSON bool) int {
	var result struct {
		Outcome string `json:"outcome"`
	}
	json.Unmarshal(rawResult, &result)
	success := result.Outcome != "state_changed" && result.Outcome != "local_device_confirmation_required"
	code := exitcode.Success
	if !success {
		code = exitcode.Error
	}
	if asJSON {
		return output.EmitJSONWithCode(choiceOutput{OK: success, Result: rawResult, State: state.raw}, "device group choice", code)
	}
	switch result.Outcome {
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
	ui.Info("local_device_id", trust.LocalDeviceID)
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
	for _, device := range trust.Devices {
		if !device.IsLocal && device.awaitingRemovalAck() {
			removals = append(removals, device)
		}
	}
	ui.Info("removal_notifications", strconv.Itoa(len(removals)))
	for _, device := range removals {
		ui.Info("removed_device", device.DisplayName+" (notifying other devices)")
	}
	for _, issue := range state.Issues {
		ui.Bar()
		ui.Info("issue_id", issue.IssueID)
		for _, choice := range issue.Choices {
			group := "candidate"
			if choice.IsCurrentGroup {
				group = "current"
			}
			ui.Info("choice", fmt.Sprintf("%s (%s; members=%s; re_pairing=%t; complete=%t)",
				choice.ChoiceID, group, strings.Join(choice.MemberDeviceIDs, ","), choice.RequiresRePairing, choice.MembersComplete))
		}
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
