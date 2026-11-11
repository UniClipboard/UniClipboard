package api

import (
	"context"
	"encoding/json"
	"io"
	"net/http"
	"net/url"
	"strconv"

	"github.com/uniclipboard/unic/engine"
	"github.com/uniclipboard/unic/internal/clip"
)

type SubmitInput struct {
	RequestID string        `json:"request_id"`
	Snapshot  clip.Snapshot `json:"snapshot"`
	// DeviceIDs limits the send to these paired devices; nil sends to all.
	DeviceIDs []string `json:"device_ids,omitempty"`
}
type ActionInput struct {
	RequestID string `json:"request_id"`
	Apply     bool   `json:"apply"`
}
type ActionOutput struct {
	Item       HistoryItem             `json:"item"`
	Submission engine.SubmissionResult `json:"submission"`
	Applied    bool                    `json:"applied"`
	ApplyError *engine.Error           `json:"apply_error,omitempty"`
}
type HeadlessBackend interface {
	Submit(context.Context, SubmitInput) (engine.SubmissionResult, error)
	Submission(context.Context, string) (engine.SubmissionResult, error)
	Operation(context.Context, string) (engine.Operation, error)
	Received(context.Context) ([]engine.ReceivedContent, error)
	ReceivedContent(context.Context, string, int) (HistoryContent, error)
	DismissReceived(context.Context, string) error
	Capabilities(context.Context) (engine.Capabilities, error)
	ExecuteAction(context.Context, string, string, ActionInput) (ActionOutput, error)
}

func (c *Client) Submit(ctx context.Context, v SubmitInput) (out engine.SubmissionResult, err error) {
	err = c.do(ctx, "POST", "/v1/submissions", v, &out)
	return
}
func (c *Client) Submission(ctx context.Context, id string) (out engine.SubmissionResult, err error) {
	err = c.do(ctx, "GET", "/v1/submissions/"+url.PathEscape(id), nil, &out)
	return
}
func (c *Client) Capabilities(ctx context.Context) (out engine.Capabilities, err error) {
	err = c.do(ctx, "GET", "/v1/capabilities", nil, &out)
	return
}
func (c *Client) Received(ctx context.Context) (out []engine.ReceivedContent, err error) {
	err = c.do(ctx, "GET", "/v1/received", nil, &out)
	return
}
func (c *Client) DismissReceived(ctx context.Context, id string) error {
	return c.do(ctx, "DELETE", "/v1/received/"+url.PathEscape(id), nil, nil)
}
func (c *Client) ExecuteAction(ctx context.Context, id, key string, in ActionInput) (out ActionOutput, err error) {
	err = c.do(ctx, "POST", "/v1/history/"+url.PathEscape(id)+"/actions/"+key, in, &out)
	return
}
func (c *Client) CopyReceivedContent(ctx context.Context, id string, part int, w io.Writer) error {
	return c.copyContent(ctx, "/v1/received/"+url.PathEscape(id)+"/content?part="+strconv.Itoa(part), w)
}
func (c *Client) copyContent(ctx context.Context, path string, w io.Writer) error {
	req, e := http.NewRequestWithContext(ctx, "GET", "http://unic"+path, nil)
	if e != nil {
		return e
	}
	resp, e := c.hc.Do(req)
	if e != nil {
		return e
	}
	defer func() { _ = resp.Body.Close() }()
	if resp.StatusCode != 200 {
		var v errBody
		_ = json.NewDecoder(resp.Body).Decode(&v)
		return &Error{Message: v.Error, Reason: v.Reason}
	}
	_, e = io.Copy(w, resp.Body)
	return e
}
