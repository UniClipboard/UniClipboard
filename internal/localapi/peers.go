package api

import (
	"context"
	"fmt"
	"net/url"
)

// PeerRename is the body of PATCH /v1/peers/{id}. Revision is required; an
// empty Name clears the user-assigned name and reverts to the reported name.
type PeerRename struct {
	Revision int64  `json:"revision"`
	Name     string `json:"name"`
}

// RenamePeer updates or clears the user's custom name for a device using CAS.
func (c *Client) RenamePeer(ctx context.Context, id string, in PeerRename) (out PeerStatus, err error) {
	err = c.do(ctx, "PATCH", "/v1/peers/"+url.PathEscape(id), in, &out)
	return
}

// UnpairAt revokes a device only when its current revision matches revision.
func (c *Client) UnpairAt(ctx context.Context, id string, revision int64) error {
	return c.do(ctx, "DELETE", fmt.Sprintf("/v1/peers/%s?revision=%d", url.PathEscape(id), revision), nil, nil)
}
