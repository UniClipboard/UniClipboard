package commands

import (
	"context"
	"errors"
	"net/http"
	"strconv"

	"github.com/UniClipboard/UniClipboard/apps/cli-go/internal/cli"
	"github.com/UniClipboard/UniClipboard/apps/cli-go/internal/exitcode"
	"github.com/UniClipboard/UniClipboard/apps/cli-go/internal/output"
	"github.com/UniClipboard/UniClipboard/apps/cli-go/internal/session"
	"github.com/UniClipboard/UniClipboard/apps/cli-go/internal/ui"
	"github.com/UniClipboard/UniClipboard/packages/desktop-host-go/daemonclient"
)

// joinSpaceResponse is the daemon's `JoinSpaceResponse`, internally tagged
// by `status`; only the fields of the tagged variant are meaningful.
type joinSpaceResponse struct {
	Status                     string  `json:"status"`
	JoinID                     string  `json:"joinId"`
	PeerUpgradeRequired        bool    `json:"peerUpgradeRequired"`
	TargetSpaceID              *string `json:"targetSpaceId"`
	SponsorDeviceID            *string `json:"sponsorDeviceId"`
	SponsorIdentityFingerprint *string `json:"sponsorIdentityFingerprint"`
	CancelRequested            bool    `json:"cancelRequested"`
	NextRetryAtMs              *int64  `json:"nextRetryAtMs"`
	Reason                     string  `json:"reason"`
	JoinedSpace                *struct {
		SponsorDeviceID            string  `json:"sponsorDeviceId"`
		SponsorIdentityFingerprint string  `json:"sponsorIdentityFingerprint"`
		SpaceID                    string  `json:"spaceId"`
		SelfDeviceID               string  `json:"selfDeviceId"`
		SelfIdentityFingerprint    string  `json:"selfIdentityFingerprint"`
		MigratedRecords            *uint64 `json:"migratedRecords"`
		PreservedUnreadableRecords *uint64 `json:"preservedUnreadableRecords"`
	} `json:"joinedSpace"`
}

const (
	joinActive         = "active"
	joinPending        = "pending"
	joinProcessing     = "processing"
	joinNeedsAttention = "needs_attention"
	joinRejected       = "rejected"
	joinTerminated     = "terminated"
)

var errUnknownJoinStatus = errors.New("unknown join status")

// validate rejects payloads the Rust enum would fail to decode.
func (r *joinSpaceResponse) validate() error {
	switch r.Status {
	case joinActive:
		if r.JoinedSpace == nil {
			return errUnknownJoinStatus
		}
	case joinProcessing:
		if r.TargetSpaceID == nil || r.SponsorDeviceID == nil || r.SponsorIdentityFingerprint == nil {
			return errUnknownJoinStatus
		}
	case joinPending, joinNeedsAttention, joinRejected, joinTerminated:
	default:
		return errUnknownJoinStatus
	}
	return nil
}

// decodeJoinResponse decodes `{ "data": JoinSpaceResponse }` from a daemon call.
func decodeJoinResponse(ctx context.Context, client *daemonclient.Client, req daemonclient.Request) (*joinSpaceResponse, error) {
	var resp joinSpaceResponse
	if err := client.Enveloped(ctx, req, &resp); err != nil {
		return nil, err
	}
	if resp.validate() != nil {
		return nil, &daemonclient.RequestError{Kind: daemonclient.ErrDecode, Path: req.Path, Err: errors.New("error decoding response body")}
	}
	return &resp, nil
}

// queryCurrentJoin reads `deviceTrust.currentJoin` from the device-group choices.
func queryCurrentJoin(ctx context.Context, client *daemonclient.Client) (*joinSpaceResponse, error) {
	const path = "/member/device-group-choices"
	var choices struct {
		DeviceTrust struct {
			CurrentJoin *joinSpaceResponse `json:"currentJoin"`
		} `json:"deviceTrust"`
	}
	if err := client.Get(ctx, path, &choices); err != nil {
		return nil, err
	}
	current := choices.DeviceTrust.CurrentJoin
	if current != nil && current.validate() != nil {
		return nil, &daemonclient.RequestError{Kind: daemonclient.ErrDecode, Path: path, Err: errors.New("error decoding response body")}
	}
	return current, nil
}

type joinIntent int

const (
	intentStart joinIntent = iota
	intentStatus
	intentCancel
)

type joinErrorOutput struct {
	OK      bool   `json:"ok"`
	Code    string `json:"code"`
	Message string `json:"message"`
}

type joinSuccessOutput struct {
	PeerUpgradeRequired        bool    `json:"peer_upgrade_required"`
	OK                         bool    `json:"ok"`
	Status                     string  `json:"status"`
	JoinID                     string  `json:"join_id"`
	SpaceID                    string  `json:"space_id"`
	SelfDeviceID               string  `json:"self_device_id"`
	SelfDeviceName             *string `json:"self_device_name"`
	SelfFingerprint            string  `json:"self_fingerprint"`
	SponsorDeviceID            string  `json:"sponsor_device_id"`
	SponsorFingerprint         string  `json:"sponsor_fingerprint"`
	MigratedRecords            *uint64 `json:"migrated_records,omitempty"`
	PreservedUnreadableRecords *uint64 `json:"preserved_unreadable_records,omitempty"`
}

type joinPendingOutput struct {
	PeerUpgradeRequired bool    `json:"peer_upgrade_required"`
	OK                  bool    `json:"ok"`
	Status              string  `json:"status"`
	JoinID              string  `json:"join_id"`
	TargetSpaceID       *string `json:"target_space_id"`
	SponsorDeviceID     *string `json:"sponsor_device_id"`
	SponsorFingerprint  *string `json:"sponsor_fingerprint"`
	CancelRequested     bool    `json:"cancel_requested"`
}

type joinProcessingOutput struct {
	PeerUpgradeRequired bool   `json:"peer_upgrade_required"`
	OK                  bool   `json:"ok"`
	Status              string `json:"status"`
	JoinID              string `json:"join_id"`
	TargetSpaceID       string `json:"target_space_id"`
	SponsorDeviceID     string `json:"sponsor_device_id"`
	SponsorFingerprint  string `json:"sponsor_fingerprint"`
}

type joinTerminalOutput struct {
	OK     bool   `json:"ok"`
	Status string `json:"status"`
	JoinID string `json:"join_id"`
	Reason string `json:"reason"`
}

type joinNeedsAttentionOutput struct {
	OK            bool   `json:"ok"`
	Status        string `json:"status"`
	JoinID        string `json:"join_id"`
	Reason        string `json:"reason"`
	Recovery      string `json:"recovery"`
	NextRetryAtMs *int64 `json:"next_retry_at_ms"`
}

func emitJoinError(asJSON bool, code, message string, exit int) int {
	if asJSON {
		return output.EmitJSONWithCode(joinErrorOutput{OK: false, Code: code, Message: message}, "join error", exit)
	}
	ui.Error(message)
	return exit
}

func renderJoinError(prefix string, err error, asJSON bool) int {
	if asJSON {
		code := daemonclient.ErrorCode(err)
		if code == "" {
			code = "unknown"
		}
		return output.EmitJSONWithCode(joinErrorOutput{OK: false, Code: code, Message: daemonclient.DisplayMessage(err)}, "join error response", exitcode.Error)
	}
	return emitJoinError(false, "join_failed", prefix+": "+daemonclient.DisplayMessage(err), exitcode.Error)
}

// joinOutcomeOK mirrors `join_response_outcome`.
func joinOutcomeOK(r *joinSpaceResponse, intent joinIntent) bool {
	switch intent {
	case intentStart:
		return r.Status != joinNeedsAttention && r.Status != joinRejected && r.Status != joinTerminated
	case intentStatus:
		return true
	}
	return (r.Status == joinPending && r.CancelRequested) ||
		((r.Status == joinRejected || r.Status == joinTerminated) && r.Reason == "cancelled")
}

func derefOr(s *string) string {
	if s == nil {
		return ""
	}
	return *s
}

// renderJoinResponse mirrors `render_join_response`. spinner may be nil
// (the Rust hidden progress bar).
func renderJoinResponse(r *joinSpaceResponse, spinner *ui.Spinner, completedMessage string, deviceName *string, asJSON bool, intent joinIntent) int {
	ok := joinOutcomeOK(r, intent)
	exit := exitcode.Success
	if !ok {
		exit = exitcode.Error
	}
	switch r.Status {
	case joinActive:
		js := r.JoinedSpace
		if asJSON {
			clearSpinner(spinner)
			return output.EmitJSONWithCode(joinSuccessOutput{
				PeerUpgradeRequired: r.PeerUpgradeRequired, OK: ok, Status: "active", JoinID: r.JoinID,
				SpaceID: js.SpaceID, SelfDeviceID: js.SelfDeviceID, SelfDeviceName: deviceName,
				SelfFingerprint: js.SelfIdentityFingerprint, SponsorDeviceID: js.SponsorDeviceID,
				SponsorFingerprint: js.SponsorIdentityFingerprint, MigratedRecords: js.MigratedRecords,
				PreservedUnreadableRecords: js.PreservedUnreadableRecords,
			}, "join response", exit)
		}
		if ok {
			spinnerSuccess(spinner, completedMessage)
		} else {
			spinnerError(spinner, "Join request was not cancelled")
		}
		ui.Info("join_id", r.JoinID)
		ui.Info("space_id", js.SpaceID)
		ui.Info("self_device_id", js.SelfDeviceID)
		if deviceName != nil {
			ui.Info("self_device_name", *deviceName)
		}
		ui.Info("self_fingerprint", js.SelfIdentityFingerprint)
		ui.Info("sponsor_device_id", js.SponsorDeviceID)
		ui.Info("sponsor_fingerprint", js.SponsorIdentityFingerprint)
		if js.MigratedRecords != nil {
			ui.Info("migrated_records", strconv.FormatUint(*js.MigratedRecords, 10))
		}
		if js.PreservedUnreadableRecords != nil {
			ui.Info("preserved_unreadable_records", strconv.FormatUint(*js.PreservedUnreadableRecords, 10))
		}
		return exit
	case joinPending:
		if asJSON {
			clearSpinner(spinner)
			return output.EmitJSONWithCode(joinPendingOutput{
				PeerUpgradeRequired: r.PeerUpgradeRequired, OK: ok, Status: "pending", JoinID: r.JoinID,
				TargetSpaceID: r.TargetSpaceID, SponsorDeviceID: r.SponsorDeviceID,
				SponsorFingerprint: r.SponsorIdentityFingerprint, CancelRequested: r.CancelRequested,
			}, "join response", exit)
		}
		switch {
		case ok && intent == intentCancel:
			spinnerSuccess(spinner, "Join cancellation requested")
		case ok:
			spinnerSuccess(spinner, "Join request is pending")
		default:
			spinnerError(spinner, "Join request was not cancelled")
		}
		ui.Info("join_id", r.JoinID)
		if r.TargetSpaceID != nil {
			ui.Info("target_space_id", *r.TargetSpaceID)
		}
		if r.SponsorDeviceID != nil {
			ui.Info("sponsor_device_id", *r.SponsorDeviceID)
		}
		if r.SponsorIdentityFingerprint != nil {
			ui.Info("sponsor_fingerprint", *r.SponsorIdentityFingerprint)
		}
		ui.Info("cancel_requested", strconv.FormatBool(r.CancelRequested))
		return exit
	case joinProcessing:
		if asJSON {
			clearSpinner(spinner)
			return output.EmitJSONWithCode(joinProcessingOutput{
				PeerUpgradeRequired: r.PeerUpgradeRequired, OK: ok, Status: "processing", JoinID: r.JoinID,
				TargetSpaceID: derefOr(r.TargetSpaceID), SponsorDeviceID: derefOr(r.SponsorDeviceID),
				SponsorFingerprint: derefOr(r.SponsorIdentityFingerprint),
			}, "join response", exit)
		}
		clearSpinner(spinner)
		ui.Info("status", "processing")
		ui.Info("join_id", r.JoinID)
		ui.Info("target_space_id", derefOr(r.TargetSpaceID))
		ui.Info("sponsor_device_id", derefOr(r.SponsorDeviceID))
		return exit
	case joinNeedsAttention:
		const reason, recovery = "outcome_cannot_be_proven", "preserve_data_and_contact_support"
		if asJSON {
			clearSpinner(spinner)
			return output.EmitJSONWithCode(joinNeedsAttentionOutput{
				OK: ok, Status: "needs_attention", JoinID: r.JoinID, Reason: reason, Recovery: recovery, NextRetryAtMs: r.NextRetryAtMs,
			}, "join response", exit)
		}
		if ok {
			clearSpinner(spinner)
		} else {
			spinnerError(spinner, "Join result needs attention")
		}
		ui.Info("status", "needs_attention")
		ui.Info("join_id", r.JoinID)
		ui.Info("reason", reason)
		ui.Info("recovery", recovery)
		if r.NextRetryAtMs != nil {
			ui.Info("next_retry_at_ms", strconv.FormatInt(*r.NextRetryAtMs, 10))
		}
		return exit
	}
	// rejected / terminated
	if asJSON {
		clearSpinner(spinner)
		return output.EmitJSONWithCode(joinTerminalOutput{OK: ok, Status: r.Status, JoinID: r.JoinID, Reason: r.Reason}, "join response", exit)
	}
	switch {
	case intent == intentCancel && ok:
		spinnerSuccess(spinner, "Join cancelled")
	case intent == intentStatus && ok:
		clearSpinner(spinner)
		ui.Info("status", r.Status)
	case r.Status == joinRejected:
		spinnerError(spinner, "Join request was rejected")
	default:
		spinnerError(spinner, "Join request ended")
	}
	ui.Info("join_id", r.JoinID)
	ui.Info("reason", r.Reason)
	return exit
}

func renderNoCurrentJoin(asJSON bool, message string) int {
	if asJSON {
		return output.EmitJSON(statusResult{OK: true, Status: "none"}, "join status")
	}
	ui.Info("status", "none")
	ui.End(message)
	return exitcode.Success
}

func runJoinStatus(ctx *cli.Context) int {
	warnLegacySpaceCommand("join status", "space join status")
	return runSpaceJoinStatus(ctx)
}

func runJoinCancel(ctx *cli.Context) int {
	warnLegacySpaceCommand("join cancel", "space join cancel")
	return runSpaceJoinCancel(ctx)
}

// runSpaceJoinStatus reports the Engine-owned current join.
func runSpaceJoinStatus(ctx *cli.Context) int {
	asJSON := ctx.JSON()
	if !asJSON {
		ui.Header("Join status")
	}
	lease, client, err := session.ConnectSetupWithLease(true)
	if err != nil {
		return session.ExitCode(err)
	}
	defer lease.Release()
	current, err := queryCurrentJoin(context.Background(), client)
	if err != nil {
		return emitJoinError(asJSON, "join_status_unavailable", "Failed to query join status: "+daemonclient.DisplayMessage(err), exitcode.Error)
	}
	if current == nil {
		return renderNoCurrentJoin(asJSON, "No current join request.")
	}
	return renderJoinResponse(current, nil, "Join completed", nil, asJSON, intentStatus)
}

// runSpaceJoinCancel cancels the current pending join, or reports the
// current join when there is nothing to cancel.
func runSpaceJoinCancel(ctx *cli.Context) int {
	asJSON := ctx.JSON()
	if !asJSON {
		ui.Header("Cancel join")
	}
	lease, client, err := session.ConnectSetupWithLease(true)
	if err != nil {
		return session.ExitCode(err)
	}
	defer lease.Release()
	bg := context.Background()
	current, err := queryCurrentJoin(bg, client)
	if err != nil {
		return emitJoinError(asJSON, "join_status_unavailable", "Failed to query join status: "+daemonclient.DisplayMessage(err), exitcode.Error)
	}
	switch {
	case current == nil:
		return renderNoCurrentJoin(asJSON, "No pending join request to cancel.")
	case current.Status == joinPending && current.CancelRequested:
		return renderJoinResponse(current, nil, "Join cancellation requested", nil, asJSON, intentCancel)
	case current.Status != joinPending:
		return renderJoinResponse(current, nil, "Join completed", nil, asJSON, intentStatus)
	}
	response, err := decodeJoinResponse(bg, client, daemonclient.Request{
		Method: http.MethodPost,
		Path:   "/v2/setup/cancel-join",
		JSON:   map[string]string{"joinId": current.JoinID},
	})
	if err != nil {
		return emitJoinError(asJSON, "join_cancel_failed", "Failed to cancel join: "+daemonclient.DisplayMessage(err), exitcode.Error)
	}
	return renderJoinResponse(response, nil, "Join cancelled", nil, asJSON, intentCancel)
}
