package commands

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"net/http"

	"github.com/UniClipboard/UniClipboard/apps/cli-go/internal/cli"
	"github.com/UniClipboard/UniClipboard/apps/cli-go/internal/exitcode"
	"github.com/UniClipboard/UniClipboard/apps/cli-go/internal/output"
	"github.com/UniClipboard/UniClipboard/apps/cli-go/internal/session"
	"github.com/UniClipboard/UniClipboard/apps/cli-go/internal/ui"
	"github.com/UniClipboard/UniClipboard/packages/desktop-host-go/daemonclient"
)

// upgradeStatus is the daemon's `UpgradeStatusDto` (tagged by `kind`).
type upgradeStatus struct {
	Kind    string  `json:"kind"`
	Current string  `json:"current"`
	From    *string `json:"from"`
	To      string  `json:"to"`
}

type upgradeAckOutput struct {
	Acknowledged string `json:"acknowledged"`
}

// runUpgrade handles bare `uniclip upgrade`: the read-only status check.
func runUpgrade(ctx *cli.Context) int { return runUpgradeStatus(ctx) }

func runUpgradeStatus(ctx *cli.Context) int {
	lease, client, err := session.ConnectWithLease()
	if err != nil {
		return session.ExitCode(err)
	}
	defer lease.Release()

	var raw json.RawMessage
	err = client.Get(context.Background(), "/upgrade/status", &raw)
	var status upgradeStatus
	if err == nil {
		status, err = decodeUpgradeStatus(raw)
	}
	if err != nil {
		ui.Error(fmt.Sprintf("Failed to detect upgrade status: %v", err))
		return exitcode.Error
	}
	// The daemon DTO and the CLI output share one wire shape, so the JSON
	// form passes the daemon bytes through.
	if err := output.PrintResult(raw, status.human(), ctx.JSON()); err != nil {
		ui.Error(err.Error())
		return exitcode.Error
	}
	return exitcode.Success
}

func decodeUpgradeStatus(raw json.RawMessage) (upgradeStatus, error) {
	var status upgradeStatus
	decodeErr := &daemonclient.RequestError{Kind: daemonclient.ErrDecode, Path: "/upgrade/status", Err: errors.New("error decoding response body")}
	if err := json.Unmarshal(raw, &status); err != nil {
		return status, decodeErr
	}
	switch status.Kind {
	case "fresh_install", "no_change", "upgraded":
		return status, nil
	case "downgraded":
		if status.From != nil {
			return status, nil
		}
	}
	return status, decodeErr
}

func (s upgradeStatus) human() string {
	switch s.Kind {
	case "fresh_install":
		return "Status: fresh install\nCurrent version: " + s.Current
	case "no_change":
		return "Status: no change\nCurrent version: " + s.Current
	case "upgraded":
		from := "<unknown>"
		if s.From != nil {
			from = *s.From
		}
		return "Status: upgraded\nFrom: " + from + "\nTo:   " + s.To
	default:
		return "Status: downgraded\nFrom: " + *s.From + "\nTo:   " + s.To
	}
}

func runUpgradeAck(ctx *cli.Context) int {
	lease, client, err := session.ConnectWithLease()
	if err != nil {
		return session.ExitCode(err)
	}
	defer lease.Release()

	var ack upgradeAckOutput
	req := daemonclient.Request{Method: http.MethodPost, Path: "/upgrade/ack"}
	if err := client.Enveloped(context.Background(), req, &ack); err != nil {
		ui.Error(fmt.Sprintf("Failed to acknowledge upgrade: %v", err))
		return exitcode.Error
	}
	if err := output.PrintResult(ack, "Cursor advanced to "+ack.Acknowledged, ctx.JSON()); err != nil {
		ui.Error(err.Error())
		return exitcode.Error
	}
	return exitcode.Success
}
