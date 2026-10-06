package commands

import (
	"context"
	"encoding/json"
	"os"
	"os/signal"
	"time"

	"github.com/UniClipboard/UniClipboard/apps/cli-go/internal/exitcode"
	"github.com/UniClipboard/UniClipboard/apps/cli-go/internal/session"
	"github.com/UniClipboard/UniClipboard/apps/cli-go/internal/ui"
	"github.com/UniClipboard/UniClipboard/packages/desktop-host-go/daemonclient"
)

const (
	inboundReconnectTimeout = 60 * time.Second

	wsTopicClipboard    = "clipboard"
	wsTopicFileTransfer = "file-transfer"

	wsEventNewContent          = "clipboard.new_content"
	wsEventIncomingPending     = "clipboard.incoming_pending"
	wsEventInboundNotice       = "clipboard.inbound_notice"
	wsEventTransferProgress    = "file-transfer.progress"
	wsEventTransferStatusChang = "file-transfer.status_changed"
)

// inboundEntry is the `clipboard.new_content` payload (InboundEntryEvent).
type inboundEntry struct {
	EntryID    string
	Preview    string
	Origin     string
	FromDevice string
}

// decodeInboundEntry decodes a `clipboard.new_content` payload, requiring
// the fields serde requires.
func decodeInboundEntry(payload json.RawMessage) (inboundEntry, bool) {
	var raw struct {
		EntryID    *string `json:"entryId"`
		Preview    *string `json:"preview"`
		Origin     *string `json:"origin"`
		FromDevice string  `json:"fromDevice"`
	}
	if json.Unmarshal(payload, &raw) != nil || raw.EntryID == nil || raw.Preview == nil || raw.Origin == nil {
		return inboundEntry{}, false
	}
	return inboundEntry{EntryID: *raw.EntryID, Preview: *raw.Preview, Origin: *raw.Origin, FromDevice: raw.FromDevice}, true
}

// Inbound activity events, mirroring `InboundActivityEvent`.
type (
	activityPending struct {
		EntryID   string
		AttemptID *string
		Filenames []string
	}
	activityProgress struct {
		EntryID          *string
		AttemptID        *string
		Receiving        bool
		BytesTransferred uint64
		TotalBytes       *uint64
	}
	activityStatus struct {
		EntryID   *string
		AttemptID *string
		Status    string
		Reason    *string
	}
	activityCompleted struct {
		EntryID string
	}
)

// decodeActivity maps one WS event onto an inbound activity event, or nil
// when the event is not part of the activity stream.
func decodeActivity(event daemonclient.Event) any {
	if len(event.Payload) == 0 {
		return nil
	}
	switch event.Type {
	case wsEventIncomingPending:
		var p struct {
			EntryID    *string  `json:"entryId"`
			AttemptID  *string  `json:"attemptId"`
			FromDevice *string  `json:"fromDevice"`
			TotalBytes *uint64  `json:"totalBytes"`
			Filenames  []string `json:"filenames"`
		}
		if json.Unmarshal(event.Payload, &p) != nil || p.EntryID == nil || p.FromDevice == nil {
			return nil
		}
		return activityPending{EntryID: *p.EntryID, AttemptID: p.AttemptID, Filenames: p.Filenames}
	case wsEventTransferProgress:
		var p struct {
			TransferID       *string `json:"transferId"`
			EntryID          *string `json:"entryId"`
			AttemptID        *string `json:"attemptId"`
			PeerID           *string `json:"peerId"`
			Direction        *string `json:"direction"`
			BytesTransferred *uint64 `json:"bytesTransferred"`
			TotalBytes       *uint64 `json:"totalBytes"`
		}
		if json.Unmarshal(event.Payload, &p) != nil || p.TransferID == nil || p.PeerID == nil ||
			p.Direction == nil || p.BytesTransferred == nil ||
			(*p.Direction != "sending" && *p.Direction != "receiving") {
			return nil
		}
		return activityProgress{EntryID: p.EntryID, AttemptID: p.AttemptID, Receiving: *p.Direction == "receiving",
			BytesTransferred: *p.BytesTransferred, TotalBytes: p.TotalBytes}
	case wsEventTransferStatusChang:
		var p struct {
			TransferID *string `json:"transferId"`
			EntryID    *string `json:"entryId"`
			AttemptID  *string `json:"attemptId"`
			Status     *string `json:"status"`
			Reason     *string `json:"reason"`
		}
		if json.Unmarshal(event.Payload, &p) != nil || p.TransferID == nil || p.Status == nil {
			return nil
		}
		return activityStatus{EntryID: p.EntryID, AttemptID: p.AttemptID, Status: *p.Status, Reason: p.Reason}
	case wsEventNewContent:
		entry, ok := decodeInboundEntry(event.Payload)
		if !ok || entry.Origin != "remote" {
			return nil
		}
		return activityCompleted{EntryID: entry.EntryID}
	}
	return nil
}

// inboundWaitMode selects the subscription an inboundWait session uses.
type inboundWaitMode int

const (
	waitEntries inboundWaitMode = iota
	waitActivity
)

// activityReconnected is delivered by nextActivity after a daemon restart.
type activityReconnected struct{}

// inboundWait is one subscribed daemon session shared by the one-shot inbound
// waits (`get --wait`, `recv`). The subscription is established before the
// caller starts waiting, and the control lease keeps a transient daemon alive
// for the whole operation.
type inboundWait struct {
	client      *daemonclient.Client
	lease       *daemonclient.Lease
	ws          *daemonclient.WS
	events      <-chan daemonclient.Event
	mode        inboundWaitMode
	reconnected bool
	interrupt   chan os.Signal
}

func connectInboundWait(client *daemonclient.Client, lease *daemonclient.Lease, mode inboundWaitMode) (*inboundWait, int) {
	ws, events, err := subscribeInbound(client, mode)
	if err != nil {
		lease.Release()
		if mode == waitEntries {
			ui.Error("Failed to subscribe inbound entries: " + err.Error())
		} else {
			ui.Error("Failed to subscribe inbound activity: " + err.Error())
		}
		return nil, exitcode.Error
	}
	interrupt := make(chan os.Signal, 1)
	signal.Notify(interrupt, os.Interrupt)
	return &inboundWait{client: client, lease: lease, ws: ws, events: events, mode: mode, interrupt: interrupt}, exitcode.Success
}

func subscribeInbound(client *daemonclient.Client, mode inboundWaitMode) (*daemonclient.WS, <-chan daemonclient.Event, error) {
	ctx := context.Background()
	ws, err := client.DialWS(ctx)
	if err != nil {
		return nil, nil, err
	}
	topics := []string{wsTopicClipboard}
	if mode == waitActivity {
		topics = append(topics, wsTopicFileTransfer)
	}
	if err := ws.Subscribe(ctx, topics...); err != nil {
		ws.Close()
		return nil, nil, err
	}
	return ws, ws.Events(ctx), nil
}

func (w *inboundWait) close() {
	w.ws.Close()
	w.lease.Release()
}

func (w *inboundWait) interrupted() bool {
	select {
	case <-w.interrupt:
		return true
	default:
		return false
	}
}

// next waits for one remote entry. ok=false with code 0 means Ctrl-C.
func (w *inboundWait) next() (inboundEntry, bool, int) {
	for {
		if w.interrupted() {
			return inboundEntry{}, false, exitcode.Success
		}
		select {
		case <-w.interrupt:
			return inboundEntry{}, false, exitcode.Success
		case event, open := <-w.events:
			if !open {
				if done, code := w.reconnectOrInterrupt(); done {
					return inboundEntry{}, false, code
				}
				continue
			}
			if event.Type != wsEventNewContent {
				continue
			}
			entry, ok := decodeInboundEntry(event.Payload)
			if !ok || entry.Origin != "remote" {
				continue
			}
			return entry, true, exitcode.Success
		}
	}
}

// nextActivity waits for the next inbound activity event (or
// activityReconnected). A nil event with code 0 means Ctrl-C.
func (w *inboundWait) nextActivity() (any, int) {
	for {
		if w.interrupted() {
			return nil, exitcode.Success
		}
		select {
		case <-w.interrupt:
			return nil, exitcode.Success
		case event, open := <-w.events:
			if !open {
				if done, code := w.reconnectOrInterrupt(); done {
					return nil, code
				}
				return activityReconnected{}, exitcode.Success
			}
			if decoded := decodeActivity(event); decoded != nil {
				return decoded, exitcode.Success
			}
		}
	}
}

// reconnectOrInterrupt races a reconnect against Ctrl-C. done=true ends the
// wait with code (0 for Ctrl-C).
func (w *inboundWait) reconnectOrInterrupt() (bool, int) {
	if w.interrupted() {
		return true, exitcode.Success
	}
	result := make(chan int, 1)
	go func() { result <- w.reconnect() }()
	select {
	case <-w.interrupt:
		return true, exitcode.Success
	case code := <-result:
		if code != exitcode.Success {
			return true, code
		}
		return false, exitcode.Success
	}
}

func (w *inboundWait) reconnect() int {
	if w.reconnected {
		ui.Error("Inbound channel closed again; exiting.")
		return exitcode.Error
	}
	ui.Warn("Daemon connection lost — reconnecting...")
	client, err := session.WaitAndReconnect(inboundReconnectTimeout)
	if err != nil {
		return session.ExitCode(err)
	}
	lease, err := client.HoldLease(context.Background())
	if err != nil {
		ui.Error("Failed to re-acquire lease after reconnect: " + err.Error())
		return exitcode.Error
	}
	ws, events, err := subscribeInbound(client, w.mode)
	if err != nil {
		lease.Release()
		if w.mode == waitEntries {
			ui.Error("Failed to re-subscribe after reconnect: " + err.Error())
		} else {
			ui.Error("Failed to re-subscribe inbound activity: " + err.Error())
		}
		return exitcode.Error
	}
	w.ws.Close()
	w.lease.Release()
	w.client, w.lease, w.ws, w.events = client, lease, ws, events
	w.reconnected = true
	ui.Warn("Reconnected — events during daemon restart may have been missed")
	return exitcode.Success
}
