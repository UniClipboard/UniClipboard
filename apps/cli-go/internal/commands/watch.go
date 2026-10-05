package commands

import (
	"context"
	"encoding/json"
	"fmt"
	"os"
	"os/signal"
	"strings"
	"time"

	"github.com/UniClipboard/UniClipboard/apps/cli-go/internal/cli"
	"github.com/UniClipboard/UniClipboard/apps/cli-go/internal/daemonclient"
	"github.com/UniClipboard/UniClipboard/apps/cli-go/internal/exitcode"
	"github.com/UniClipboard/UniClipboard/apps/cli-go/internal/output"
	"github.com/UniClipboard/UniClipboard/apps/cli-go/internal/session"
	"github.com/UniClipboard/UniClipboard/apps/cli-go/internal/ui"
)

// inboundNotice is the `clipboard.inbound_notice` payload.
type inboundNotice struct {
	FromDevice      *string `json:"fromDevice"`
	SnapshotHash    *string `json:"snapshotHash"`
	TextPreview     *string `json:"textPreview"`
	Representations []struct {
		MimeType  *string `json:"mimeType"`
		SizeBytes *int64  `json:"sizeBytes"`
	} `json:"representations"`
	Action *string `json:"action"`
	AtMs   *int64  `json:"atMs"`
}

func (n *inboundNotice) valid() bool {
	if n.FromDevice == nil || n.SnapshotHash == nil || n.Action == nil || n.AtMs == nil {
		return false
	}
	for _, rep := range n.Representations {
		if rep.SizeBytes == nil {
			return false
		}
	}
	return true
}

// watchNotice mirrors the Rust `DaemonNoticeDto` JSON line.
type watchNotice struct {
	FromDevice   string  `json:"from_device"`
	SnapshotHash string  `json:"snapshot_hash"`
	Text         *string `json:"text,omitempty"`
	RepSummary   *string `json:"rep_summary,omitempty"`
	Action       string  `json:"action"`
	AtMs         int64   `json:"at_ms"`
}

func runWatch(ctx *cli.Context) int {
	asJSON := ctx.JSON()
	if !asJSON {
		ui.Header("Watch inbound clipboard")
	}
	client, err := session.ConnectOrSpawnOneshot(time.Time{})
	if err != nil {
		return session.ExitCode(err)
	}

	spinner := ui.NewSpinner("Subscribing to daemon clipboard events...")
	ws, events, err := subscribeNotices(client)
	if err != nil {
		spinner.FinishError("Failed to subscribe: " + err.Error())
		return exitcode.Error
	}
	spinner.FinishSuccess("Subscribed via daemon WS")
	defer func() { ws.Close() }()

	if !asJSON {
		ui.Info("status", "Listening via daemon — press Ctrl-C to stop")
		ui.Bar()
	}
	ui.RawStderr("WATCH_READY")

	interrupt := make(chan os.Signal, 1)
	signal.Notify(interrupt, os.Interrupt)
	reconnected := false
	for {
		select {
		case <-interrupt:
			if !asJSON {
				ui.End("Stopped")
			}
			return exitcode.Success
		default:
		}
		select {
		case <-interrupt:
			if !asJSON {
				ui.End("Stopped")
			}
			return exitcode.Success
		case event, open := <-events:
			if open {
				if event.Type != wsEventInboundNotice || len(event.Payload) == 0 {
					continue
				}
				var notice inboundNotice
				if json.Unmarshal(event.Payload, &notice) != nil || !notice.valid() {
					continue
				}
				renderNotice(&notice, asJSON)
				continue
			}
			if reconnected {
				if !asJSON {
					ui.Warn("Daemon WS channel closed again; exiting.")
				}
				return exitcode.Error
			}
			if !asJSON {
				ui.Warn("Daemon connection lost — reconnecting...")
			}
			newClient, err := session.WaitAndReconnect(inboundReconnectTimeout)
			if err != nil {
				return session.ExitCode(err)
			}
			newWS, newEvents, err := subscribeNotices(newClient)
			if err != nil {
				ui.Error("Failed to re-subscribe after reconnect: " + err.Error())
				return exitcode.Error
			}
			ws.Close()
			ws, events = newWS, newEvents
			reconnected = true
			if !asJSON {
				ui.Warn("Reconnected — events during daemon restart may have been missed")
			}
		}
	}
}

func subscribeNotices(client *daemonclient.Client) (*daemonclient.WS, <-chan daemonclient.Event, error) {
	ctx := context.Background()
	ws, err := client.DialWS(ctx)
	if err != nil {
		return nil, nil, err
	}
	if err := ws.Subscribe(ctx, wsTopicClipboard); err != nil {
		ws.Close()
		return nil, nil, err
	}
	return ws, ws.Events(ctx), nil
}

func renderNotice(notice *inboundNotice, asJSON bool) {
	var repSummary *string
	if len(notice.Representations) > 0 {
		parts := make([]string, 0, len(notice.Representations))
		for _, rep := range notice.Representations {
			mime := "?"
			if rep.MimeType != nil {
				mime = *rep.MimeType
			}
			parts = append(parts, fmt.Sprintf("%s/%dB", mime, *rep.SizeBytes))
		}
		summary := fmt.Sprintf("[envelope:%d rep(s) %s]", len(notice.Representations), strings.Join(parts, ", "))
		repSummary = &summary
	}

	if asJSON {
		line, err := output.Compact(watchNotice{
			FromDevice:   *notice.FromDevice,
			SnapshotHash: *notice.SnapshotHash,
			Text:         notice.TextPreview,
			RepSummary:   repSummary,
			Action:       *notice.Action,
			AtMs:         *notice.AtMs,
		})
		if err == nil {
			fmt.Fprintln(os.Stdout, line)
		}
		return
	}

	var body string
	switch {
	case notice.TextPreview != nil:
		body = truncateNoticePreview(*notice.TextPreview)
	case repSummary != nil:
		body = *repSummary
	default:
		body = "(undecodable envelope)"
	}
	ui.Info("·", fmt.Sprintf("[%s] %s (%s)", *notice.FromDevice, body, *notice.Action))
}

func truncateNoticePreview(text string) string {
	const maxChars = 120
	singleLine := strings.ReplaceAll(text, "\n", `\n`)
	runes := []rune(singleLine)
	if len(runes) > maxChars {
		return string(runes[:maxChars]) + "…"
	}
	return singleLine
}
