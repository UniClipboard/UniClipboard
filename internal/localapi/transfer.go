package api

import (
	"context"
	"net/url"

	"github.com/uniclipboard/unic/internal/clip"
)

// StartTransferInput is the content to offer to one temporary receiver.
type StartTransferInput struct {
	Snapshot clip.Snapshot `json:"snapshot"`
}

// Transfer states.
const (
	TransferWaiting   = "waiting"   // listening; nobody has pulled yet
	TransferSending   = "sending"   // a receiver is pulling
	TransferDelivered = "delivered" // the receiver acknowledged every item
	TransferFailed    = "failed"
	TransferExpired   = "expired"
	TransferCancelled = "cancelled"
)

// Transfer is one temporary offer (0022). Link and Code are secrets that grant one
// pull, so they appear only in the response that creates the transfer.
type Transfer struct {
	ID          string `json:"id"`
	State       string `json:"state"`
	Reason      string `json:"reason,omitempty"` // set when State is failed
	Link        string `json:"link,omitempty"`
	Code        string `json:"code,omitempty"` // "NNN-NNN"; empty when the code service is unavailable
	ExpiresAtMs int64  `json:"expires_at_ms"`
	Items       int    `json:"items"`
	Bytes       int64  `json:"bytes"`
}

// ReceiveInput names where to pull from: a 6-digit code or a link.
type ReceiveInput struct {
	Source string `json:"source"`
}

// ReceivedItem describes one pulled item. Text is set only for text items.
type ReceivedItem struct {
	Kind string `json:"kind"`
	Name string `json:"name,omitempty"`
	MIME string `json:"mime"`
	Size int64  `json:"size"`
	Text string `json:"text,omitempty"`
}

// ReceiveResult lists what a pull stored. The content is in local history and in
// the received list.
type ReceiveResult struct {
	Items []ReceivedItem `json:"items"`
}

// TransferBackend is implemented by hosts that support temporary transfers.
type TransferBackend interface {
	StartTransfer(context.Context, StartTransferInput) (Transfer, error)
	Transfer(context.Context, string) (Transfer, error)
	CancelTransfer(context.Context, string) error
	Receive(context.Context, ReceiveInput) (ReceiveResult, error)
}

func (c *Client) StartTransfer(ctx context.Context, in StartTransferInput) (out Transfer, err error) {
	err = c.do(ctx, "POST", "/v1/transfers", in, &out)
	return
}
func (c *Client) Transfer(ctx context.Context, id string) (out Transfer, err error) {
	err = c.do(ctx, "GET", "/v1/transfers/"+url.PathEscape(id), nil, &out)
	return
}
func (c *Client) CancelTransfer(ctx context.Context, id string) error {
	return c.do(ctx, "DELETE", "/v1/transfers/"+url.PathEscape(id), nil, nil)
}

// Receive pulls from a temporary sender. It can take longer than an ordinary
// request, so the CLI exempts /v1/receive from its request deadline.
func (c *Client) Receive(ctx context.Context, in ReceiveInput) (out ReceiveResult, err error) {
	err = c.do(ctx, "POST", "/v1/receive", in, &out)
	return
}
