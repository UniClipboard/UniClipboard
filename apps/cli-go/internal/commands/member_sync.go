package commands

import (
	"context"
	"net/http"
	"strings"

	"github.com/UniClipboard/UniClipboard/apps/cli-go/internal/cli"
	"github.com/UniClipboard/UniClipboard/apps/cli-go/internal/exitcode"
	"github.com/UniClipboard/UniClipboard/apps/cli-go/internal/output"
	"github.com/UniClipboard/UniClipboard/apps/cli-go/internal/session"
	"github.com/UniClipboard/UniClipboard/apps/cli-go/internal/ui"
	"github.com/UniClipboard/UniClipboard/packages/desktop-host-go/daemonclient"
)

// contentTypesPatch mirrors `ContentTypesPatchDto` (wire field order).
type contentTypesPatch struct {
	Text        *bool `json:"text"`
	Image       *bool `json:"image"`
	Link        *bool `json:"link"`
	File        *bool `json:"file"`
	CodeSnippet *bool `json:"codeSnippet"`
	RichText    *bool `json:"richText"`
}

// memberSyncPatch mirrors `MemberSyncPreferencesPatchDto`; unset fields are
// sent as `null`, like serde does for `Option::None`.
type memberSyncPatch struct {
	SendEnabled         *bool              `json:"sendEnabled"`
	ReceiveEnabled      *bool              `json:"receiveEnabled"`
	SendContentTypes    *contentTypesPatch `json:"sendContentTypes"`
	ReceiveContentTypes *contentTypesPatch `json:"receiveContentTypes"`
}

type contentTypes struct {
	Text        bool `json:"text"`
	Image       bool `json:"image"`
	Link        bool `json:"link"`
	File        bool `json:"file"`
	CodeSnippet bool `json:"codeSnippet"`
	RichText    bool `json:"richText"`
}

type memberSyncPreferences struct {
	SendEnabled         bool         `json:"sendEnabled"`
	ReceiveEnabled      bool         `json:"receiveEnabled"`
	SendContentTypes    contentTypes `json:"sendContentTypes"`
	ReceiveContentTypes contentTypes `json:"receiveContentTypes"`
}

// memberSyncOutput mirrors the Rust `MemberSyncOutput`.
type memberSyncOutput struct {
	OK                  bool     `json:"ok"`
	Status              string   `json:"status"`
	DeviceID            string   `json:"device_id"`
	DeviceName          string   `json:"device_name"`
	SendEnabled         bool     `json:"send_enabled"`
	ReceiveEnabled      bool     `json:"receive_enabled"`
	SendContentTypes    []string `json:"send_content_types"`
	ReceiveContentTypes []string `json:"receive_content_types"`
}

type memberSyncErrorOutput struct {
	OK      bool   `json:"ok"`
	Code    string `json:"code"`
	Message string `json:"message"`
}

type syncPatchError string

func (e syncPatchError) Error() string { return string(e) }

var validContentTypes = []string{"text", "image", "file", "link", "rich-text", "code-snippet"}

func parseContentTypes(value string) (*contentTypesPatch, error) {
	var values []string
	for _, item := range strings.Split(value, ",") {
		if item = strings.TrimSpace(item); item != "" {
			values = append(values, asciiLower(item))
		}
	}
	if len(values) == 0 {
		return nil, syncPatchError("content type list must not be empty")
	}
	for _, v := range values {
		if v == "all" || v == "none" {
			if len(values) != 1 {
				return nil, syncPatchError("`all` and `none` cannot be combined with other content types")
			}
			return newContentTypesPatch(values[0] == "all", nil), nil
		}
	}
	for _, v := range values {
		if !containsValue(validContentTypes, v) {
			return nil, syncPatchError("unknown content type `" + v + "`")
		}
	}
	return newContentTypesPatch(false, values), nil
}

func newContentTypesPatch(all bool, enabled []string) *contentTypesPatch {
	has := func(name string) *bool {
		b := all || containsValue(enabled, name)
		return &b
	}
	return &contentTypesPatch{
		Text: has("text"), Image: has("image"), Link: has("link"), File: has("file"),
		CodeSnippet: has("code-snippet"), RichText: has("rich-text"),
	}
}

func buildMemberSyncPatch(ctx *cli.Context) (*memberSyncPatch, error) {
	if !ctx.Has("send") && !ctx.Has("receive") && !ctx.Has("send-types") && !ctx.Has("receive-types") {
		return nil, syncPatchError("provide at least one sync setting to change")
	}
	onOff := func(long string) *bool {
		if !ctx.Has(long) {
			return nil
		}
		b := ctx.String(long) == "on"
		return &b
	}
	patch := &memberSyncPatch{SendEnabled: onOff("send"), ReceiveEnabled: onOff("receive")}
	var err error
	if ctx.Has("send-types") {
		if patch.SendContentTypes, err = parseContentTypes(ctx.String("send-types")); err != nil {
			return nil, err
		}
	}
	if ctx.Has("receive-types") {
		if patch.ReceiveContentTypes, err = parseContentTypes(ctx.String("receive-types")); err != nil {
			return nil, err
		}
	}
	return patch, nil
}

func runMemberSyncShow(ctx *cli.Context) int {
	device, _ := ctx.Arg(0)
	return runMemberSync(device, nil, ctx.JSON())
}

func runMemberSyncSet(ctx *cli.Context) int {
	device, _ := ctx.Arg(0)
	patch, err := buildMemberSyncPatch(ctx)
	if err != nil {
		return emitMemberSyncError(ctx.JSON(), "invalid_sync_settings", err.Error())
	}
	return runMemberSync(device, patch, ctx.JSON())
}

func runMemberSync(selector string, patch *memberSyncPatch, asJSON bool) int {
	if !asJSON {
		if patch != nil {
			ui.Header("Update member sync")
		} else {
			ui.Header("Member sync")
		}
	}
	lease, client, err := session.ConnectWithLease()
	if err != nil {
		return session.ExitCode(err)
	}
	defer lease.Release()
	state, err := queryDeviceGroupChoices(client)
	if err != nil {
		return emitMemberSyncError(asJSON, "device_trust_unavailable", "Failed to resolve member: "+daemonclient.DisplayMessage(err))
	}

	// Names resolve only in an interactive terminal; scripts must pass an
	// exact device ID.
	interactive := !asJSON && ui.StderrIsTerminal()
	var selected *trustDevice
	devices := state.DeviceTrust.Devices
	for i := range devices {
		if devices[i].DeviceID == selector {
			selected = &devices[i]
			break
		}
	}
	if selected == nil {
		var matches []*trustDevice
		for i := range devices {
			if asciiLower(devices[i].DisplayName) == asciiLower(selector) {
				matches = append(matches, &devices[i])
			}
		}
		switch {
		case len(matches) == 0:
			return emitMemberSyncError(asJSON, "member_not_found", "No member matches that device.")
		case !interactive:
			return emitMemberSyncError(asJSON, "device_id_required", "Non-interactive use requires an exact device ID.")
		case len(matches) > 1:
			return emitMemberSyncError(asJSON, "ambiguous_device_name", "More than one member has that name; use a device ID.")
		}
		selected = matches[0]
	}

	bg := context.Background()
	path, pathErr := daemonclient.PathSegment(selected.DeviceID)
	path = "/member/" + path + "/sync-preferences"
	status := "current"
	if patch != nil {
		if pathErr != nil {
			return emitMemberSyncError(asJSON, "sync_update_failed", "Failed to update member sync settings: "+pathErr.Error())
		}
		var result struct {
			Success bool `json:"success"`
		}
		err := client.Enveloped(bg, daemonclient.Request{Method: http.MethodPatch, Path: path, JSON: patch}, &result)
		if err != nil {
			return emitMemberSyncError(asJSON, "sync_update_failed", "Failed to update member sync settings: "+daemonclient.DisplayMessage(err))
		}
		if !result.Success {
			return emitMemberSyncError(asJSON, "sync_update_rejected", "The member sync settings were not updated.")
		}
		status = "updated"
	}
	if pathErr != nil {
		return emitMemberSyncError(asJSON, "sync_read_failed", "Failed to read saved member sync settings: "+pathErr.Error())
	}
	var preferences memberSyncPreferences
	if err := client.Get(bg, path, &preferences); err != nil {
		return emitMemberSyncError(asJSON, "sync_read_failed", "Failed to read saved member sync settings: "+daemonclient.DisplayMessage(err))
	}

	out := memberSyncOutput{
		OK: true, Status: status, DeviceID: selected.DeviceID, DeviceName: selected.DisplayName,
		SendEnabled: preferences.SendEnabled, ReceiveEnabled: preferences.ReceiveEnabled,
		SendContentTypes:    enabledContentTypes(preferences.SendContentTypes),
		ReceiveContentTypes: enabledContentTypes(preferences.ReceiveContentTypes),
	}
	if asJSON {
		return output.EmitJSON(out, "member sync preferences")
	}
	ui.Info("device_id", out.DeviceID)
	ui.Info("device_name", out.DeviceName)
	ui.Info("send", onOffLabel(out.SendEnabled))
	ui.Info("receive", onOffLabel(out.ReceiveEnabled))
	ui.Info("send_types", strings.Join(out.SendContentTypes, ","))
	ui.Info("receive_types", strings.Join(out.ReceiveContentTypes, ","))
	if status == "updated" {
		ui.End("Member sync settings updated.")
	} else {
		ui.End("Member sync settings loaded.")
	}
	return exitcode.Success
}

func enabledContentTypes(types contentTypes) []string {
	enabled := []string{}
	for _, entry := range []struct {
		on   bool
		name string
	}{{types.Text, "text"}, {types.Image, "image"}, {types.File, "file"}, {types.Link, "link"},
		{types.RichText, "rich-text"}, {types.CodeSnippet, "code-snippet"}} {
		if entry.on {
			enabled = append(enabled, entry.name)
		}
	}
	return enabled
}

func onOffLabel(value bool) string {
	if value {
		return "on"
	}
	return "off"
}

func emitMemberSyncError(asJSON bool, code, message string) int {
	if asJSON {
		return output.EmitJSONWithCode(memberSyncErrorOutput{OK: false, Code: code, Message: message}, "member sync error", exitcode.Error)
	}
	ui.Error(message)
	return exitcode.Error
}
