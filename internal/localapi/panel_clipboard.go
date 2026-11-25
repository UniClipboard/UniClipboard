package api

import (
	"context"
	"net/http"

	"github.com/uniclipboard/unic/engine"
	"github.com/uniclipboard/unic/internal/clip"
)

// PanelApplyBeginRequest requests bounded explicit clipboard application
// authorization for Quick Panel on Windows and Linux.
type PanelApplyBeginRequest struct {
	Profile        string         `json:"profile"`
	ServiceID      string         `json:"service_id,omitempty"`
	RequestID      string         `json:"request_id"`
	HistoryID      string         `json:"history_id"`
	Plain          bool           `json:"plain,omitempty"`
	ObservedBefore *clip.Snapshot `json:"observed_before,omitempty"`
}

// PanelApplyGrant carries the authorized application ticket and materialized
// snapshot for the Quick Panel process to write to the native clipboard.
type PanelApplyGrant struct {
	ServiceID string                    `json:"service_id"`
	Epoch     string                    `json:"epoch"`
	Token     engine.ApplyToken         `json:"token"`
	Snapshot  clip.Snapshot             `json:"snapshot"`
	HasFiles  bool                      `json:"has_files,omitempty"`
	Replayed  bool                      `json:"replayed,omitempty"`
	Restore   *engine.RestoreSyncResult `json:"restore,omitempty"`
}

// PanelApplyCompleteRequest reports the verified write or failure outcome of
// one authorized Quick Panel clipboard application.
type PanelApplyCompleteRequest struct {
	Profile   string            `json:"profile"`
	ServiceID string            `json:"service_id"`
	RequestID string            `json:"request_id"`
	Token     engine.ApplyToken `json:"token"`
	Outcome   string            `json:"outcome"`
	ReadBack  *clip.Snapshot    `json:"read_back,omitempty"`
	ErrorCode string            `json:"error_code,omitempty"`
}

// PanelApplyCompleteReply reports whether the service accepted the completion
// and any restore-sync result for a user restore.
type PanelApplyCompleteReply struct {
	Applied bool                      `json:"applied"`
	Restore *engine.RestoreSyncResult `json:"restore,omitempty"`
}

// PanelApplyReleaseRequest releases a held panel file lease once the clipboard
// no longer holds the applied file paths or the panel exits.
type PanelApplyReleaseRequest struct {
	Profile      string            `json:"profile"`
	ServiceID    string            `json:"service_id"`
	Token        engine.ApplyToken `json:"token"`
	CurrentPaths []string          `json:"current_paths,omitempty"`
}

func (c *Client) PanelApplyBegin(ctx context.Context, req PanelApplyBeginRequest) (out PanelApplyGrant, err error) {
	err = c.do(ctx, http.MethodPost, "/v1/desktop/panel-apply/begin", req, &out)
	return
}

func (c *Client) PanelApplyComplete(ctx context.Context, req PanelApplyCompleteRequest) (out PanelApplyCompleteReply, err error) {
	err = c.do(ctx, http.MethodPost, "/v1/desktop/panel-apply/complete", req, &out)
	return
}

func (c *Client) PanelApplyRelease(ctx context.Context, req PanelApplyReleaseRequest) error {
	return c.do(ctx, http.MethodPost, "/v1/desktop/panel-apply/release", req, nil)
}
