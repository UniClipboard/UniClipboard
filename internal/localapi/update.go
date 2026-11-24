package api

import (
	"context"
	"encoding/json"
	"io"
	"net/http"
	"net/url"
)

type DesktopUpdateRequest struct {
	CurrentVersion string `json:"current_version"`
	PublicKey      string `json:"public_key"`
}

type DesktopUpdateSelection struct {
	Token    string          `json:"token"`
	Manifest json.RawMessage `json:"manifest"`
}

func (c *Client) CheckDesktopUpdate(ctx context.Context, req DesktopUpdateRequest) (out DesktopUpdateSelection, err error) {
	err = c.do(ctx, http.MethodPost, "/v1/desktop/update/check", req, &out)
	return
}

// OpenDesktopUpdate streams the archive selected by a successful check.
func (c *Client) OpenDesktopUpdate(ctx context.Context, token string) (io.ReadCloser, error) {
	req, err := http.NewRequestWithContext(ctx, http.MethodGet, "http://unic/v1/desktop/update/archive/"+url.PathEscape(token), nil)
	if err != nil {
		return nil, err
	}
	resp, err := c.hc.Do(req)
	if err != nil {
		return nil, err
	}
	if resp.StatusCode != http.StatusOK {
		_ = resp.Body.Close()
		return nil, &Error{Message: "desktop update archive is unavailable"}
	}
	return resp.Body, nil
}
