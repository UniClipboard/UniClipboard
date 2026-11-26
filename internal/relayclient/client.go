// Package relayclient implements the HTTPS client for Relay HTTP API v1
// (docs/contracts/hybrid-relay-contract.md §3). It carries only ciphertext
// and opaque routing identifiers and has no dependency on tailcat or tcnet.
package relayclient

import (
	"bufio"
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"net/http"
	"strings"
	"time"

	"github.com/uniclipboard/unic/internal/relaycrypto"
)

const (
	// DefaultURL is the production official relay endpoint; UNIC_RELAY_URL or settings override it.
	DefaultURL = "https://relay.uniclipboard.app"

	defaultRequestTimeout = 30 * time.Second
)

// Error is a redacted HTTP relay failure mapped to a stable class.
type Error struct {
	Status int
	Code   string
}

func (e *Error) Error() string {
	if e.Status > 0 {
		return fmt.Sprintf("relay: %s (http %d)", e.Code, e.Status)
	}
	return "relay: " + e.Code
}

// Config configures a Relay HTTP API v1 client.
type Config struct {
	BaseURL    string
	AccessKey  string
	HTTPClient *http.Client
	SSEClient  *http.Client
	Now        func() time.Time
}

// Client speaks Relay HTTP API v1 to a single configured origin.
type Client struct {
	origin    string
	accessKey string
	httpCli   *http.Client
	sseCli    *http.Client
	now       func() time.Time
}

// New creates a Client for cfg.BaseURL (defaults to DefaultURL when empty).
func New(cfg Config) (*Client, error) {
	raw := strings.TrimSpace(cfg.BaseURL)
	if raw == "" {
		raw = DefaultURL
	}
	origin, err := relaycrypto.NormalizeRelayOrigin(raw)
	if err != nil {
		return nil, err
	}
	httpCli := cfg.HTTPClient
	if httpCli == nil {
		httpCli = &http.Client{Timeout: defaultRequestTimeout}
	}
	sseCli := cfg.SSEClient
	if sseCli == nil {
		sseCli = &http.Client{} // no overall timeout for long-lived SSE streams
	}
	now := cfg.Now
	if now == nil {
		now = time.Now
	}
	return &Client{
		origin:    origin,
		accessKey: cfg.AccessKey,
		httpCli:   httpCli,
		sseCli:    sseCli,
		now:       now,
	}, nil
}

// Origin returns the canonical relay origin.
func (c *Client) Origin() string { return c.origin }

// ValidatePeerOrigin enforces SSRF protection by verifying a peer-advertised
// relay origin matches this client's configured origin exactly.
func (c *Client) ValidatePeerOrigin(peerOrigin string) error {
	norm, err := relaycrypto.NormalizeRelayOrigin(peerOrigin)
	if err != nil || norm != c.origin {
		return &Error{Code: "origin_mismatch"}
	}
	return nil
}

// CreateTransferResponse is returned by CreateTransfer.
type CreateTransferResponse struct {
	TransferID    string `json:"transfer_id"`
	ExpiresAtMs   int64  `json:"expires_at_ms"`
	MaxChunkBytes int    `json:"max_chunk_bytes"`
}

// TransferSummary is one pending transfer entry returned by ListTransfers.
type TransferSummary struct {
	TransferID      string               `json:"transfer_id"`
	Envelope        relaycrypto.Envelope `json:"envelope"`
	AvailableBitmap string               `json:"available_bitmap"`
	ExpiresAtMs     int64                `json:"expires_at_ms"`
}

// SSEEvent is one server-sent event delivered on SubscribeEvents.
type SSEEvent struct {
	ID              string                `json:"id"`
	Event           string                `json:"event"`
	TransferID      string                `json:"transfer_id"`
	Envelope        *relaycrypto.Envelope `json:"envelope,omitempty"`
	ChunkIndex      *int                  `json:"chunk_index,omitempty"`
	AvailableBitmap string                `json:"available_bitmap,omitempty"`
	ExpiresAtMs     int64                 `json:"expires_at_ms,omitempty"`
}

func (c *Client) applyAccessKey(req *http.Request) {
	if c.accessKey != "" {
		req.Header.Set("X-Unic-Relay-Key", c.accessKey)
	}
}

func (c *Client) signOwnerRequest(req *http.Request, owner relaycrypto.Signer, body []byte) error {
	hdr, err := relaycrypto.SignOwnerAuth(owner, c.origin, req.Method, req.URL.RequestURI(), body, c.now(), "")
	if err != nil {
		return err
	}
	req.Header.Set("X-Unic-PubKey", hdr.PubKey)
	req.Header.Set("X-Unic-Timestamp", hdr.Timestamp)
	req.Header.Set("X-Unic-Nonce", hdr.Nonce)
	req.Header.Set("X-Unic-Sig", hdr.Sig)
	return nil
}

func mapHTTPStatus(status int) error {
	switch status {
	case http.StatusBadRequest:
		return &Error{Status: status, Code: "invalid_argument"}
	case http.StatusUnauthorized, http.StatusForbidden:
		return &Error{Status: status, Code: "permission_denied"}
	case http.StatusNotFound, http.StatusGone:
		return &Error{Status: status, Code: "not_found"}
	case http.StatusConflict:
		return &Error{Status: status, Code: "conflict"}
	case http.StatusRequestEntityTooLarge:
		return &Error{Status: status, Code: "limit_exceeded"}
	case http.StatusTooManyRequests:
		return &Error{Status: status, Code: "resource_exhausted"}
	default:
		return &Error{Status: status, Code: "unavailable"}
	}
}

func (c *Client) do(req *http.Request) (*http.Response, error) {
	c.applyAccessKey(req)
	resp, err := c.httpCli.Do(req)
	if err != nil {
		if req.Context().Err() != nil {
			return nil, req.Context().Err()
		}
		return nil, &Error{Code: "unavailable"}
	}
	return resp, nil
}

// RegisterSender registers or updates an allowed sender delivery token hash on mailboxID.
func (c *Client) RegisterSender(ctx context.Context, owner relaycrypto.Signer, mailboxID, tokenHash string, maxBytes int64) error {
	var body []byte
	if maxBytes > 0 {
		body, _ = json.Marshal(map[string]int64{"max_bytes": maxBytes})
	}
	path := fmt.Sprintf("/v1/relay/mailboxes/%s/senders/%s", mailboxID, tokenHash)
	req, err := http.NewRequestWithContext(ctx, http.MethodPut, c.origin+path, bytes.NewReader(body))
	if err != nil {
		return &Error{Code: "invalid_argument"}
	}
	if len(body) > 0 {
		req.Header.Set("Content-Type", "application/json")
	}
	if err := c.signOwnerRequest(req, owner, body); err != nil {
		return err
	}
	resp, err := c.do(req)
	if err != nil {
		return err
	}
	defer func() { _ = resp.Body.Close() }()
	_, _ = io.Copy(io.Discard, io.LimitReader(resp.Body, 4096))
	if resp.StatusCode != http.StatusNoContent && resp.StatusCode != http.StatusOK {
		return mapHTTPStatus(resp.StatusCode)
	}
	return nil
}

// RevokeSender removes a previously registered sender delivery token hash.
func (c *Client) RevokeSender(ctx context.Context, owner relaycrypto.Signer, mailboxID, tokenHash string) error {
	path := fmt.Sprintf("/v1/relay/mailboxes/%s/senders/%s", mailboxID, tokenHash)
	req, err := http.NewRequestWithContext(ctx, http.MethodDelete, c.origin+path, nil)
	if err != nil {
		return &Error{Code: "invalid_argument"}
	}
	if err := c.signOwnerRequest(req, owner, nil); err != nil {
		return err
	}
	resp, err := c.do(req)
	if err != nil {
		return err
	}
	defer func() { _ = resp.Body.Close() }()
	_, _ = io.Copy(io.Discard, io.LimitReader(resp.Body, 4096))
	if resp.StatusCode != http.StatusNoContent && resp.StatusCode != http.StatusOK && resp.StatusCode != http.StatusNotFound {
		return mapHTTPStatus(resp.StatusCode)
	}
	return nil
}

// UploadSingle uploads a 1-RTT single-chunk transfer.
func (c *Client) UploadSingle(ctx context.Context, mailboxID, deliveryToken string, env relaycrypto.Envelope, ciphertext0 []byte) error {
	frame, err := relaycrypto.EncodeSingleFrame(env, ciphertext0)
	if err != nil {
		return err
	}
	path := fmt.Sprintf("/v1/relay/mailboxes/%s/transfers/%s/single", mailboxID, env.TransferID)
	req, err := http.NewRequestWithContext(ctx, http.MethodPut, c.origin+path, bytes.NewReader(frame))
	if err != nil {
		return &Error{Code: "invalid_argument"}
	}
	req.Header.Set("Content-Type", "application/octet-stream")
	req.Header.Set("Authorization", "Bearer "+deliveryToken)
	resp, err := c.do(req)
	if err != nil {
		return err
	}
	defer func() { _ = resp.Body.Close() }()
	_, _ = io.Copy(io.Discard, io.LimitReader(resp.Body, 4096))
	if resp.StatusCode != http.StatusOK && resp.StatusCode != http.StatusNoContent {
		return mapHTTPStatus(resp.StatusCode)
	}
	return nil
}

// CreateTransfer announces a multi-chunk transfer with its signed outer envelope.
func (c *Client) CreateTransfer(ctx context.Context, mailboxID, deliveryToken string, env relaycrypto.Envelope) (CreateTransferResponse, error) {
	body, err := json.Marshal(env)
	if err != nil {
		return CreateTransferResponse{}, err
	}
	path := fmt.Sprintf("/v1/relay/mailboxes/%s/transfers", mailboxID)
	req, err := http.NewRequestWithContext(ctx, http.MethodPost, c.origin+path, bytes.NewReader(body))
	if err != nil {
		return CreateTransferResponse{}, &Error{Code: "invalid_argument"}
	}
	req.Header.Set("Content-Type", "application/json")
	req.Header.Set("Authorization", "Bearer "+deliveryToken)
	resp, err := c.do(req)
	if err != nil {
		return CreateTransferResponse{}, err
	}
	defer func() { _ = resp.Body.Close() }()
	if resp.StatusCode != http.StatusOK && resp.StatusCode != http.StatusCreated {
		_, _ = io.Copy(io.Discard, io.LimitReader(resp.Body, 4096))
		return CreateTransferResponse{}, mapHTTPStatus(resp.StatusCode)
	}
	var out CreateTransferResponse
	if err := json.NewDecoder(io.LimitReader(resp.Body, 64<<10)).Decode(&out); err != nil {
		return CreateTransferResponse{}, &Error{Code: "unavailable"}
	}
	return out, nil
}

// UploadChunk uploads one ciphertext chunk for an announced multi-chunk transfer.
func (c *Client) UploadChunk(ctx context.Context, mailboxID, deliveryToken, transferID string, index int, ciphertext []byte) error {
	path := fmt.Sprintf("/v1/relay/mailboxes/%s/transfers/%s/chunks/%d", mailboxID, transferID, index)
	req, err := http.NewRequestWithContext(ctx, http.MethodPut, c.origin+path, bytes.NewReader(ciphertext))
	if err != nil {
		return &Error{Code: "invalid_argument"}
	}
	req.Header.Set("Content-Type", "application/octet-stream")
	req.Header.Set("Authorization", "Bearer "+deliveryToken)
	resp, err := c.do(req)
	if err != nil {
		return err
	}
	defer func() { _ = resp.Body.Close() }()
	_, _ = io.Copy(io.Discard, io.LimitReader(resp.Body, 4096))
	if resp.StatusCode != http.StatusNoContent && resp.StatusCode != http.StatusOK {
		return mapHTTPStatus(resp.StatusCode)
	}
	return nil
}

// ListTransfers retrieves unexpired pending transfers for mailboxID.
func (c *Client) ListTransfers(ctx context.Context, owner relaycrypto.Signer, mailboxID string) ([]TransferSummary, error) {
	path := fmt.Sprintf("/v1/relay/mailboxes/%s/transfers", mailboxID)
	req, err := http.NewRequestWithContext(ctx, http.MethodGet, c.origin+path, nil)
	if err != nil {
		return nil, &Error{Code: "invalid_argument"}
	}
	if err := c.signOwnerRequest(req, owner, nil); err != nil {
		return nil, err
	}
	resp, err := c.do(req)
	if err != nil {
		return nil, err
	}
	defer func() { _ = resp.Body.Close() }()
	if resp.StatusCode != http.StatusOK {
		_, _ = io.Copy(io.Discard, io.LimitReader(resp.Body, 4096))
		return nil, mapHTTPStatus(resp.StatusCode)
	}
	var res struct {
		Transfers []TransferSummary `json:"transfers"`
	}
	if err := json.NewDecoder(io.LimitReader(resp.Body, 8<<20)).Decode(&res); err != nil {
		return nil, &Error{Code: "unavailable"}
	}
	return res.Transfers, nil
}

// DownloadChunk fetches ciphertext chunk index, bounded by maxBytes.
func (c *Client) DownloadChunk(ctx context.Context, owner relaycrypto.Signer, mailboxID, transferID string, index int, maxBytes int64) ([]byte, error) {
	if maxBytes <= 0 {
		maxBytes = relaycrypto.MaxChunkSize + relaycrypto.GCMTagSize
	}
	path := fmt.Sprintf("/v1/relay/mailboxes/%s/transfers/%s/chunks/%d", mailboxID, transferID, index)
	req, err := http.NewRequestWithContext(ctx, http.MethodGet, c.origin+path, nil)
	if err != nil {
		return nil, &Error{Code: "invalid_argument"}
	}
	if err := c.signOwnerRequest(req, owner, nil); err != nil {
		return nil, err
	}
	resp, err := c.do(req)
	if err != nil {
		return nil, err
	}
	defer func() { _ = resp.Body.Close() }()
	if resp.StatusCode != http.StatusOK {
		_, _ = io.Copy(io.Discard, io.LimitReader(resp.Body, 4096))
		return nil, mapHTTPStatus(resp.StatusCode)
	}
	data, err := io.ReadAll(io.LimitReader(resp.Body, maxBytes+1))
	if err != nil {
		if ctx.Err() != nil {
			return nil, ctx.Err()
		}
		return nil, &Error{Code: "unavailable"}
	}
	if int64(len(data)) > maxBytes {
		return nil, &Error{Code: "limit_exceeded"}
	}
	return data, nil
}

// AckTransfer acknowledges completion so the server deletes stored chunks immediately.
func (c *Client) AckTransfer(ctx context.Context, owner relaycrypto.Signer, mailboxID, transferID string) error {
	path := fmt.Sprintf("/v1/relay/mailboxes/%s/transfers/%s/ack", mailboxID, transferID)
	req, err := http.NewRequestWithContext(ctx, http.MethodPost, c.origin+path, nil)
	if err != nil {
		return &Error{Code: "invalid_argument"}
	}
	if err := c.signOwnerRequest(req, owner, nil); err != nil {
		return err
	}
	resp, err := c.do(req)
	if err != nil {
		return err
	}
	defer func() { _ = resp.Body.Close() }()
	_, _ = io.Copy(io.Discard, io.LimitReader(resp.Body, 4096))
	if resp.StatusCode != http.StatusNoContent && resp.StatusCode != http.StatusOK && resp.StatusCode != http.StatusNotFound {
		return mapHTTPStatus(resp.StatusCode)
	}
	return nil
}

// DeleteTransferAsOwner deletes a pending transfer on behalf of the mailbox owner (e.g. DiscardReceive).
func (c *Client) DeleteTransferAsOwner(ctx context.Context, owner relaycrypto.Signer, mailboxID, transferID string) error {
	path := fmt.Sprintf("/v1/relay/mailboxes/%s/transfers/%s", mailboxID, transferID)
	req, err := http.NewRequestWithContext(ctx, http.MethodDelete, c.origin+path, nil)
	if err != nil {
		return &Error{Code: "invalid_argument"}
	}
	if err := c.signOwnerRequest(req, owner, nil); err != nil {
		return err
	}
	resp, err := c.do(req)
	if err != nil {
		return err
	}
	defer func() { _ = resp.Body.Close() }()
	_, _ = io.Copy(io.Discard, io.LimitReader(resp.Body, 4096))
	if resp.StatusCode != http.StatusNoContent && resp.StatusCode != http.StatusOK && resp.StatusCode != http.StatusNotFound {
		return mapHTTPStatus(resp.StatusCode)
	}
	return nil
}

// DeleteTransferAsSender cancels and cleans up an in-flight upload on behalf of the sender.
func (c *Client) DeleteTransferAsSender(ctx context.Context, mailboxID, deliveryToken, transferID string) error {
	path := fmt.Sprintf("/v1/relay/mailboxes/%s/transfers/%s", mailboxID, transferID)
	req, err := http.NewRequestWithContext(ctx, http.MethodDelete, c.origin+path, nil)
	if err != nil {
		return &Error{Code: "invalid_argument"}
	}
	req.Header.Set("Authorization", "Bearer "+deliveryToken)
	resp, err := c.do(req)
	if err != nil {
		return err
	}
	defer func() { _ = resp.Body.Close() }()
	_, _ = io.Copy(io.Discard, io.LimitReader(resp.Body, 4096))
	if resp.StatusCode != http.StatusNoContent && resp.StatusCode != http.StatusOK && resp.StatusCode != http.StatusNotFound {
		return mapHTTPStatus(resp.StatusCode)
	}
	return nil
}

// SubscribeEvents streams SSE mailbox events until ctx is cancelled, the stream
// ends, or onEvent returns a non-nil error.
func (c *Client) SubscribeEvents(ctx context.Context, owner relaycrypto.Signer, mailboxID, lastEventID string, onEvent func(SSEEvent) error) error {
	path := fmt.Sprintf("/v1/relay/mailboxes/%s/events", mailboxID)
	req, err := http.NewRequestWithContext(ctx, http.MethodGet, c.origin+path, nil)
	if err != nil {
		return &Error{Code: "invalid_argument"}
	}
	req.Header.Set("Accept", "text/event-stream")
	if lastEventID != "" {
		req.Header.Set("Last-Event-ID", lastEventID)
	}
	if err := c.signOwnerRequest(req, owner, nil); err != nil {
		return err
	}
	c.applyAccessKey(req)
	resp, err := c.sseCli.Do(req)
	if err != nil {
		if ctx.Err() != nil {
			return ctx.Err()
		}
		return &Error{Code: "unavailable"}
	}
	defer func() { _ = resp.Body.Close() }()
	if resp.StatusCode != http.StatusOK {
		_, _ = io.Copy(io.Discard, io.LimitReader(resp.Body, 4096))
		return mapHTTPStatus(resp.StatusCode)
	}

	sc := bufio.NewScanner(resp.Body)
	sc.Buffer(make([]byte, 64<<10), 1<<20)
	var curID, curEvent string
	var dataBuf bytes.Buffer
	flush := func() error {
		if dataBuf.Len() == 0 {
			curID, curEvent = "", ""
			return nil
		}
		var ev SSEEvent
		if err := json.Unmarshal(dataBuf.Bytes(), &ev); err != nil {
			dataBuf.Reset()
			curID, curEvent = "", ""
			return nil
		}
		if ev.ID == "" {
			ev.ID = curID
		}
		if ev.Event == "" {
			ev.Event = curEvent
		}
		dataBuf.Reset()
		curID, curEvent = "", ""
		if onEvent != nil {
			return onEvent(ev)
		}
		return nil
	}

	for sc.Scan() {
		line := sc.Text()
		if line == "" {
			if err := flush(); err != nil {
				return err
			}
			continue
		}
		if strings.HasPrefix(line, ":") {
			continue
		}
		if strings.HasPrefix(line, "id:") {
			curID = strings.TrimSpace(strings.TrimPrefix(line, "id:"))
		} else if strings.HasPrefix(line, "event:") {
			curEvent = strings.TrimSpace(strings.TrimPrefix(line, "event:"))
		} else if strings.HasPrefix(line, "data:") {
			if dataBuf.Len() > 0 {
				dataBuf.WriteByte('\n')
			}
			dataBuf.WriteString(strings.TrimSpace(strings.TrimPrefix(line, "data:")))
		}
	}
	if err := sc.Err(); err != nil {
		if ctx.Err() != nil {
			return ctx.Err()
		}
		return &Error{Code: "unavailable"}
	}
	if err := flush(); err != nil {
		return err
	}
	if ctx.Err() != nil {
		return ctx.Err()
	}
	return errors.New("relay: event stream ended")
}
