package api

import "context"

type ChannelPairRequest struct {
	BackendID string `json:"backend_id"`
	Code      string `json:"code,omitempty"`
	Resume    bool   `json:"resume,omitempty"`
}

func (c *Client) PairChannel(ctx context.Context, in ChannelPairRequest) (out PeerStatus, err error) {
	err = c.do(ctx, "POST", "/v1/channels/pair", in, &out)
	return
}
