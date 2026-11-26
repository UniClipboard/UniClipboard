package api

import (
	"context"
	"encoding/json"
	"net/http"

	"github.com/uniclipboard/unic/engine"
	"github.com/uniclipboard/unic/internal/clip"
)

// AdapterRegistration identifies one local adapter process across reconnects.
// LaunchID is local-only metadata; it is never an analytics distinct_id.
type AdapterRegistration struct {
	LaunchID string `json:"launch_id,omitempty"`
}

// AdapterRequest is restricted to a single owner-only local socket session.
type AdapterRequest struct {
	Session string          `json:"session,omitempty"`
	Method  string          `json:"method"`
	Value   json.RawMessage `json:"value,omitempty"`
}
type AdapterReply struct {
	Value json.RawMessage `json:"value,omitempty"`
	Error *engine.Error   `json:"error,omitempty"`
}

// AdapterImport carries the optional clipboard image only on the private
// import route. Shared clipboard reads and command results omit it.
type AdapterImport struct {
	clip.Snapshot
	FileImagePNG []byte `json:"file_image_png,omitempty"`
}
type DesktopCommand struct {
	ID        string           `json:"id"`
	Kind      string           `json:"kind"`
	Snapshot  clip.Snapshot    `json:"snapshot,omitempty"`
	HistoryID engine.HistoryID `json:"history_id,omitempty"`
	RequestID string           `json:"request_id,omitempty"`
}
type DesktopResult struct {
	ID       string                   `json:"id"`
	Snapshot clip.Snapshot            `json:"snapshot,omitempty"`
	Restore  engine.RestoreSyncResult `json:"restore"`
	Error    *engine.Error            `json:"error,omitempty"`
}

func (c *Client) AdapterCall(ctx context.Context, session, method string, in, out any) error {
	value, err := json.Marshal(in)
	if err != nil {
		return err
	}
	var reply AdapterReply
	if err = c.do(ctx, http.MethodPost, "/v1/adapter", AdapterRequest{session, method, value}, &reply); err != nil {
		return err
	}
	if reply.Error != nil {
		return reply.Error
	}
	if out != nil {
		return json.Unmarshal(reply.Value, out)
	}
	return nil
}

// ClipboardApplication is the result used by the Quick Panel before paste.
type ClipboardApplication struct {
	Mark    string `json:"mark,omitempty"`
	Failure string `json:"failure,omitempty"`
}

type ClipboardApplicationRequest struct {
	ID    string `json:"id"`
	Plain bool   `json:"plain"`
}

const ReasonClipboardUnsupported = "clipboard_unsupported"

func (c *Client) EnableDesktopClipboard(ctx context.Context) error {
	return c.do(ctx, http.MethodPost, "/v1/desktop/clipboard/enable", nil, nil)
}

func (c *Client) ApplyDesktopClipboard(ctx context.Context, id string, plain bool) (out ClipboardApplication, err error) {
	err = c.do(ctx, http.MethodPost, "/v1/desktop/clipboard/apply", ClipboardApplicationRequest{ID: id, Plain: plain}, &out)
	return
}

func (c *Client) MatchDesktopClipboard(ctx context.Context, mark string) (out bool, err error) {
	err = c.do(ctx, http.MethodPost, "/v1/desktop/clipboard/match", mark, &out)
	return
}

// EnableDesktopNotifications reports whether unicd owns system pair notifications.
func (c *Client) EnableDesktopNotifications(ctx context.Context) (enabled bool, err error) {
	err = c.do(ctx, http.MethodPost, "/v1/desktop/notifications/enable", nil, &enabled)
	return
}

// DisableDesktopNotifications releases daemon-owned pair notifications when a
// Desktop tray process takes over native notification delivery.
func (c *Client) DisableDesktopNotifications(ctx context.Context) error {
	return c.do(ctx, http.MethodPost, "/v1/desktop/notifications/disable", nil, nil)
}
