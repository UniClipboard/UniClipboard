package api

import "context"

// SyncBackendInfo identifies a running configured transport without addresses.
type SyncBackendInfo struct {
	State            string `json:"state"`
	ID               string `json:"id"`
	ImplementationID string `json:"implementation_id"`
	TransportKind    string `json:"transport_kind"`
	Pairing          bool   `json:"pairing"`
	ExternalChannels bool   `json:"external_channels"`
}

func (c *Client) SyncBackends(ctx context.Context) (out []SyncBackendInfo, err error) {
	err = c.do(ctx, "GET", "/v1/sync-backends", nil, &out)
	return
}
func (c *Client) NewCodeVia(ctx context.Context, backend string) (out Code, err error) {
	err = c.do(ctx, "POST", "/v1/code", map[string]string{"backend_id": backend}, &out)
	return
}
func (c *Client) ExportTicketVia(ctx context.Context, backend string) (string, error) {
	var out struct {
		Ticket string `json:"ticket"`
	}
	err := c.do(ctx, "POST", "/v1/pairing-ticket", map[string]string{"backend_id": backend}, &out)
	return out.Ticket, err
}

func (c *Client) PairVia(ctx context.Context, addr, backend string) (out PairResult, err error) {
	err = c.do(ctx, "POST", "/v1/peers", map[string]string{"addr": addr, "backend_id": backend}, &out)
	return
}
func (c *Client) JoinVia(ctx context.Context, code, backend string) (out PairResult, err error) {
	err = c.do(ctx, "POST", "/v1/join", map[string]string{"code": code, "backend_id": backend}, &out)
	return
}
