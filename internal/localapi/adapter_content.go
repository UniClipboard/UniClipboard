package api

import (
	"context"
	"encoding/json"
	"io"
	"net/http"
	"net/url"

	"github.com/uniclipboard/unic/engine"
)

func (c *Client) AdapterContent(ctx context.Context, session string, l engine.LeaseID, p engine.PartID) (io.ReadCloser, error) {
	q := url.Values{"lease": {string(l)}, "part": {string(p)}}
	req, err := http.NewRequestWithContext(ctx, "GET", "http://unic/v1/adapter/content?"+q.Encode(), nil)
	if err != nil {
		return nil, err
	}
	req.Header.Set("X-Unic-Adapter", session)
	resp, err := c.hc.Do(req)
	if err != nil {
		return nil, err
	}
	if resp.StatusCode != 200 {
		defer func() { _ = resp.Body.Close() }()
		var e errBody
		_ = json.NewDecoder(resp.Body).Decode(&e)
		return nil, &engine.Error{Code: engine.Code(e.Reason), Message: "adapter content unavailable"}
	}
	return resp.Body, nil
}
