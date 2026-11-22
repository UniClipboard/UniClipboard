package api

import (
	"context"
	"fmt"
	"net/url"

	"github.com/uniclipboard/unic/engine"
)

type BindingEdit struct {
	Revision int64 `json:"revision"`
	Enabled  bool  `json:"enabled"`
}

func (c *Client) SetBindingSync(ctx context.Context, id string, in BindingEdit) (out engine.DeviceBinding, err error) {
	err = c.do(ctx, "PATCH", "/v1/bindings/"+url.PathEscape(id), in, &out)
	return
}
func (c *Client) RemoveBinding(ctx context.Context, id string, revision int64) error {
	return c.do(ctx, "DELETE", fmt.Sprintf("/v1/bindings/%s?revision=%d", url.PathEscape(id), revision), nil, nil)
}
