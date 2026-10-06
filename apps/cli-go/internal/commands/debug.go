package commands

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"net/http"
	"strconv"
	"strings"
	"time"

	"github.com/UniClipboard/UniClipboard/apps/cli-go/internal/cli"
	"github.com/UniClipboard/UniClipboard/apps/cli-go/internal/exitcode"
	"github.com/UniClipboard/UniClipboard/apps/cli-go/internal/output"
	"github.com/UniClipboard/UniClipboard/apps/cli-go/internal/session"
	"github.com/UniClipboard/UniClipboard/apps/cli-go/internal/ui"
	"github.com/UniClipboard/UniClipboard/packages/desktop-host-go/daemonclient"
)

type debugStatusOutput struct {
	DebugMode           bool   `json:"debugMode"`
	EffectiveLogProfile string `json:"effectiveLogProfile"`
	RestartRequired     bool   `json:"restartRequired"`
}

type debugUpdateOutput struct {
	DebugMode       bool `json:"debugMode"`
	RestartRequired bool `json:"restartRequired"`
}

// captureStatus is the subset of `DiagnosticStatusDto` the human view renders.
type captureStatus struct {
	RunID   string `json:"runId"`
	Capture struct {
		Mode        string  `json:"mode"`
		CaptureID   *string `json:"captureId"`
		RemainingMs uint64  `json:"remainingMs"`
	} `json:"capture"`
	LocalFile string `json:"localFile"`
}

// logExportResult is the subset of `LogExportResultDto` the human view renders.
type logExportResult struct {
	Path              string   `json:"path"`
	IncludedFiles     []string `json:"includedFiles"`
	Since             string   `json:"since"`
	EnginePreparation struct {
		Flush string `json:"flush"`
	} `json:"enginePreparation"`
	Collection struct {
		UnreadableFiles []string `json:"unreadableFiles"`
		TruncatedFiles  []string `json:"truncatedFiles"`
	} `json:"collection"`
}

// debugClient connects like the Rust debug commands: reuse or spawn a
// oneshot daemon, without holding the control lease.
func debugClient() (*daemonclient.Client, int) {
	client, err := session.ConnectOrSpawnOneshot(time.Time{})
	if err != nil {
		return nil, session.ExitCode(err)
	}
	return client, exitcode.Success
}

func runDebugStatus(ctx *cli.Context) int {
	client, code := debugClient()
	if client == nil {
		return code
	}
	var status debugStatusOutput
	if err := client.Get(context.Background(), "/diagnostics/debug", &status); err != nil {
		return printDiagnosticsError("Failed to read debug status", err)
	}
	if ctx.JSON() {
		return printDiagnosticsJSON(status)
	}
	ui.Header("Debug")
	debugMode := "off"
	if status.DebugMode {
		debugMode = "on"
	}
	ui.Info("debugMode", debugMode)
	ui.Info("effectiveLogProfile", status.EffectiveLogProfile)
	ui.Info("restartRequired", strconv.FormatBool(status.RestartRequired))
	return exitcode.Success
}

func runDebugOn(ctx *cli.Context) int  { return setDebugMode(ctx, true) }
func runDebugOff(ctx *cli.Context) int { return setDebugMode(ctx, false) }

func setDebugMode(ctx *cli.Context, enabled bool) int {
	client, code := debugClient()
	if client == nil {
		return code
	}
	var result debugUpdateOutput
	req := daemonclient.Request{Method: http.MethodPut, Path: "/diagnostics/debug", JSON: map[string]bool{"enabled": enabled}}
	if err := client.Enveloped(context.Background(), req, &result); err != nil {
		if enabled {
			return printDiagnosticsError("Failed to enable debug mode", err)
		}
		return printDiagnosticsError("Failed to disable debug mode", err)
	}
	if ctx.JSON() {
		return printDiagnosticsJSON(result)
	}
	if result.DebugMode {
		ui.Success("Debug mode enabled.")
	} else {
		ui.Success("Debug mode disabled.")
	}
	if result.RestartRequired {
		ui.Warn("Restart the daemon for the logging profile change to fully take effect.")
		ui.Info("command", "uniclip stop && uniclip start")
	}
	return exitcode.Success
}

func runDebugCaptureStatus(ctx *cli.Context) int {
	client, code := debugClient()
	if client == nil {
		return code
	}
	var raw json.RawMessage
	if err := client.Get(context.Background(), "/diagnostics/capture", &raw); err != nil {
		return printDiagnosticsError("Failed to read capture status", err)
	}
	return printCaptureStatus(raw, ctx.JSON())
}

func runDebugCaptureStart(ctx *cli.Context) int {
	client, code := debugClient()
	if client == nil {
		return code
	}
	var raw json.RawMessage
	body := map[string]uint64{"durationSeconds": ctx.Uint("minutes") * 60}
	req := daemonclient.Request{Method: http.MethodPost, Path: "/diagnostics/capture/start", JSON: body}
	if err := client.Enveloped(context.Background(), req, &raw); err != nil {
		return printDiagnosticsError("Failed to start detailed capture", err)
	}
	return printCaptureStatus(raw, ctx.JSON())
}

func runDebugCaptureStop(ctx *cli.Context) int {
	client, code := debugClient()
	if client == nil {
		return code
	}
	captureID, _ := ctx.Arg(0)
	var result string
	req := daemonclient.Request{Method: http.MethodPost, Path: "/diagnostics/capture/stop", JSON: map[string]string{"captureId": captureID}}
	err := client.Enveloped(context.Background(), req, &result)
	if err == nil && result != "stopped" && result != "alreadyStopped" && result != "differentCapture" {
		err = &daemonclient.RequestError{Kind: daemonclient.ErrDecode, Path: "/diagnostics/capture/stop", Err: errors.New("error decoding response body")}
	}
	if err != nil {
		return printDiagnosticsError("Failed to stop detailed capture", err)
	}
	if ctx.JSON() {
		return printDiagnosticsJSON(result)
	}
	// Rust prints the enum's Debug form (the PascalCase variant name).
	ui.Success("Capture stop result: " + strings.ToUpper(result[:1]) + result[1:])
	return exitcode.Success
}

func runDebugExportLogs(ctx *cli.Context) int {
	client, code := debugClient()
	if client == nil {
		return code
	}
	var raw json.RawMessage
	sinceHours := ctx.Uint("since-hours")
	req := daemonclient.Request{Method: http.MethodPost, Path: "/diagnostics/log-export", JSON: map[string]uint64{"sinceHours": sinceHours}}
	err := client.Enveloped(context.Background(), req, &raw)
	var result logExportResult
	var since string
	if err == nil {
		if json.Unmarshal(raw, &result) != nil {
			err = &daemonclient.RequestError{Kind: daemonclient.ErrDecode, Path: "/diagnostics/log-export", Err: errors.New("error decoding response body")}
		} else if since, err = chronoRFC3339(result.Since); err != nil {
			err = &daemonclient.RequestError{Kind: daemonclient.ErrDecode, Path: "/diagnostics/log-export", Err: errors.New("error decoding response body")}
		}
	}
	if err != nil {
		return printDiagnosticsError("Failed to export logs", err)
	}
	if ctx.JSON() {
		return printDiagnosticsJSON(raw)
	}
	ui.Success("Logs exported.")
	ui.Info("path", result.Path)
	ui.Info("includedFiles", strconv.Itoa(len(result.IncludedFiles)))
	ui.Info("since", since)
	ui.Info("engineFlush", result.EnginePreparation.Flush)
	ui.Info("unreadableFiles", strconv.Itoa(len(result.Collection.UnreadableFiles)))
	ui.Info("truncatedFiles", strconv.Itoa(len(result.Collection.TruncatedFiles)))
	return exitcode.Success
}

// chronoRFC3339 re-renders a UTC timestamp like chrono's
// `DateTime<Utc>::to_rfc3339`: automatic sub-second precision (none, 3, 6
// or 9 digits) and a `+00:00` offset.
func chronoRFC3339(value string) (string, error) {
	t, err := time.Parse(time.RFC3339Nano, value)
	if err != nil {
		return "", err
	}
	t = t.UTC()
	nanos := t.Nanosecond()
	frac := ""
	switch {
	case nanos == 0:
	case nanos%1_000_000 == 0:
		frac = fmt.Sprintf(".%03d", nanos/1_000_000)
	case nanos%1_000 == 0:
		frac = fmt.Sprintf(".%06d", nanos/1_000)
	default:
		frac = fmt.Sprintf(".%09d", nanos)
	}
	return t.Format("2006-01-02T15:04:05") + frac + "+00:00", nil
}

func printCaptureStatus(raw json.RawMessage, asJSON bool) int {
	if asJSON {
		return printDiagnosticsJSON(raw)
	}
	var status captureStatus
	json.Unmarshal(raw, &status)
	ui.Header("Detailed connection capture")
	ui.Info("mode", status.Capture.Mode)
	if status.Capture.CaptureID != nil {
		ui.Info("captureId", *status.Capture.CaptureID)
		ui.Info("remainingMs", strconv.FormatUint(status.Capture.RemainingMs, 10))
	}
	ui.Info("runId", status.RunID)
	// Rust renders the enum's Debug name lowercased.
	ui.Info("localFile", strings.ToLower(status.LocalFile))
	return exitcode.Success
}

func printDiagnosticsJSON(value any) int {
	rendered, err := output.Pretty(value)
	if err != nil {
		ui.Error(fmt.Sprintf("Failed to serialize JSON: %v", err))
		return exitcode.Error
	}
	fmt.Println(rendered)
	return exitcode.Success
}

func printDiagnosticsError(prefix string, err error) int {
	ui.Error(prefix + ": " + daemonclient.DisplayMessage(err))
	return exitcode.Error
}
