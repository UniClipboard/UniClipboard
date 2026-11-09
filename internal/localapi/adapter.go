package api

import (
	"context"
	"encoding/json"
	"net/http"

	"github.com/uniclipboard/unic/engine"
	"github.com/uniclipboard/unic/internal/clip"
)

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
