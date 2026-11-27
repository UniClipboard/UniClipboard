// Package relayclient implements the HTTPS client for the Direct-to-S3
// Hyper Relay HTTP API (docs/contracts/hybrid-relay-contract.md §3).
// Control-plane calls authenticate with the relay origin while ciphertext chunk
// bytes are uploaded/downloaded directly against S3-compatible object storage
// via short-lived presigned URLs without leaking relay credentials.
package relayclient

import (
	"bufio"
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"net"
	"net/http"
	"net/netip"
	"net/url"
	"strings"
	"time"

	"github.com/uniclipboard/unic/internal/relaycrypto"
)

const (
	// DefaultURL is the production official relay endpoint; UNIC_RELAY_URL or settings override it.
	DefaultURL = "https://relay.uniclipboard.app"

	defaultRequestTimeout = 30 * time.Second
	defaultStorageTimeout = 60 * time.Second
	presignRefreshSkew    = 10 * time.Second
	maxStorageRedirects   = 3
)

// Error is a redacted HTTP relay failure mapped to a stable class.
// It never includes raw presigned URLs, access keys, or delivery tokens.
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

// Config configures a Direct-to-S3 Relay HTTP client.
type Config struct {
	BaseURL              string
	AccessKey            string
	HTTPClient           *http.Client
	SSEClient            *http.Client
	StorageClient        *http.Client
	AllowLoopbackStorage bool
	Now                  func() time.Time
}

// Client speaks the Direct-to-S3 Relay HTTP API to a single configured origin.
type Client struct {
	origin            string
	accessKey         string
	allowLoopback     bool
	httpCli           *http.Client
	sseCli            *http.Client
	storageOverride   *http.Client
	directStorageCli  *http.Client
	proxiedStorageCli *http.Client
	now               func() time.Time
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
	allowLoopback := cfg.AllowLoopbackStorage || isLoopbackOrigin(origin)

	c := &Client{
		origin:          origin,
		accessKey:       cfg.AccessKey,
		allowLoopback:   allowLoopback,
		httpCli:         httpCli,
		sseCli:          sseCli,
		storageOverride: cfg.StorageClient,
		now:             now,
	}
	c.directStorageCli = c.newStorageHTTPClient(false)
	c.proxiedStorageCli = c.newStorageHTTPClient(true)
	return c, nil
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

// ChunkUploadGrant is one presigned PUT grant returned on transfer creation or upload-url refresh.
// Callers must never log URL because it carries temporary bearer signatures.
type ChunkUploadGrant struct {
	Index       int               `json:"index"`
	Method      string            `json:"method"`
	URL         string            `json:"url"`
	Headers     map[string]string `json:"headers,omitempty"`
	ExpiresAtMs int64             `json:"expires_at_ms"`
}

// CreateTransferResponse is returned by CreateTransfer.
type CreateTransferResponse struct {
	TransferID    string             `json:"transfer_id"`
	ExpiresAtMs   int64              `json:"expires_at_ms"`
	MaxChunkBytes int                `json:"max_chunk_bytes"`
	UploadMode    string             `json:"upload_mode"`
	UploadURLs    []ChunkUploadGrant `json:"upload_urls,omitempty"`
}

// PresignedDownload is returned by GET .../transfers/{id}/chunks/{index}.
// Callers must never log URL because it carries temporary bearer signatures.
type PresignedDownload struct {
	Method      string            `json:"method"`
	URL         string            `json:"url"`
	Headers     map[string]string `json:"headers,omitempty"`
	ExpiresAtMs int64             `json:"expires_at_ms"`
	Size        int64             `json:"size"`
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

// ---- Direct object storage SSRF guard & credential stripping ----

var cgnatPrefix = netip.MustParsePrefix("100.64.0.0/10")

func isLoopbackOrigin(origin string) bool {
	u, err := url.Parse(origin)
	if err != nil {
		return false
	}
	return isLoopbackHost(u.Hostname())
}

func isLoopbackHost(host string) bool {
	h := strings.ToLower(strings.TrimSpace(host))
	if h == "localhost" {
		return true
	}
	addr, err := netip.ParseAddr(h)
	if err != nil {
		return false
	}
	return addr.Unmap().IsLoopback()
}

func isAllowedStorageIP(addr netip.Addr, allowLoopback bool) bool {
	addr = addr.Unmap()
	if !addr.IsValid() {
		return false
	}
	if addr.IsLoopback() {
		return allowLoopback
	}
	if addr.IsPrivate() ||
		addr.IsLinkLocalUnicast() ||
		addr.IsLinkLocalMulticast() ||
		addr.IsMulticast() ||
		addr.IsUnspecified() ||
		addr.IsInterfaceLocalMulticast() ||
		cgnatPrefix.Contains(addr) {
		return false
	}
	return true
}

func (c *Client) validateStorageURL(rawURL string) (*url.URL, error) {
	u, err := url.Parse(strings.TrimSpace(rawURL))
	if err != nil || u.Host == "" || u.User != nil || u.Opaque != "" || u.Fragment != "" {
		return nil, &Error{Code: "invalid_storage_url"}
	}
	scheme := strings.ToLower(u.Scheme)
	host := strings.ToLower(u.Hostname())
	if host == "" {
		return nil, &Error{Code: "invalid_storage_url"}
	}
	switch scheme {
	case "https":
		// Allowed; IP/hostname restrictions still enforced below.
	case "http":
		// Plain HTTP is strictly limited to loopback test storage when enabled.
		if !c.allowLoopback || !isLoopbackHost(host) {
			return nil, &Error{Code: "invalid_storage_url"}
		}
	default:
		return nil, &Error{Code: "invalid_storage_url"}
	}
	if host == "localhost" || strings.HasSuffix(host, ".localhost") {
		if !c.allowLoopback {
			return nil, &Error{Code: "invalid_storage_url"}
		}
		return u, nil
	}
	if addr, err := netip.ParseAddr(host); err == nil {
		if !isAllowedStorageIP(addr, c.allowLoopback) {
			return nil, &Error{Code: "invalid_storage_url"}
		}
	}
	return u, nil
}

func safeStorageDialContext(allowLoopback bool) func(ctx context.Context, network, addr string) (net.Conn, error) {
	dialer := &net.Dialer{
		Timeout:   15 * time.Second,
		KeepAlive: 30 * time.Second,
	}
	return func(ctx context.Context, network, addr string) (net.Conn, error) {
		host, port, err := net.SplitHostPort(addr)
		if err != nil {
			return nil, &Error{Code: "invalid_storage_url"}
		}
		if ip, perr := netip.ParseAddr(host); perr == nil {
			if !isAllowedStorageIP(ip, allowLoopback) {
				return nil, &Error{Code: "invalid_storage_url"}
			}
			return dialer.DialContext(ctx, network, net.JoinHostPort(ip.Unmap().String(), port))
		}
		ips, err := net.DefaultResolver.LookupNetIP(ctx, "ip", host)
		if err != nil || len(ips) == 0 {
			return nil, &Error{Code: "unavailable"}
		}
		var allowed []netip.Addr
		for _, ip := range ips {
			if !isAllowedStorageIP(ip, allowLoopback) {
				return nil, &Error{Code: "invalid_storage_url"}
			}
			allowed = append(allowed, ip.Unmap())
		}
		var lastErr error
		for _, ip := range allowed {
			conn, derr := dialer.DialContext(ctx, network, net.JoinHostPort(ip.String(), port))
			if derr == nil {
				return conn, nil
			}
			lastErr = derr
		}
		if lastErr != nil {
			return nil, lastErr
		}
		return nil, &Error{Code: "unavailable"}
	}
}

func stripRelayHeaders(h http.Header) {
	for k := range h {
		lk := strings.ToLower(k)
		if lk == "authorization" || lk == "x-unic-relay-key" || strings.HasPrefix(lk, "x-unic-") {
			h.Del(k)
		}
	}
}

func applySafeStorageHeaders(h http.Header, headers map[string]string) {
	for k, v := range headers {
		lk := strings.ToLower(strings.TrimSpace(k))
		if lk == "" || lk == "authorization" || lk == "host" || lk == "content-length" ||
			lk == "x-unic-relay-key" || strings.HasPrefix(lk, "x-unic-") {
			continue
		}
		h.Set(k, v)
	}
}

func (c *Client) newStorageHTTPClient(useProxy bool) *http.Client {
	tr := http.DefaultTransport.(*http.Transport).Clone()
	if useProxy {
		tr.Proxy = http.ProxyFromEnvironment
	} else {
		tr.Proxy = nil
		tr.DialContext = safeStorageDialContext(c.allowLoopback)
	}
	return &http.Client{
		Timeout:   defaultStorageTimeout,
		Transport: tr,
		CheckRedirect: func(req *http.Request, via []*http.Request) error {
			if len(via) >= maxStorageRedirects {
				return &Error{Code: "unavailable"}
			}
			// Never follow redirects on PUT uploads.
			if len(via) > 0 && via[0].Method != http.MethodGet && via[0].Method != http.MethodHead {
				return &Error{Code: "invalid_storage_url"}
			}
			if _, err := c.validateStorageURL(req.URL.String()); err != nil {
				return err
			}
			stripRelayHeaders(req.Header)
			return nil
		},
	}
}

func (c *Client) doStorage(req *http.Request) (*http.Response, error) {
	stripRelayHeaders(req.Header)
	cli := c.directStorageCli
	if c.storageOverride != nil {
		cli = c.storageOverride
	} else if p, err := http.ProxyFromEnvironment(req); err == nil && p != nil {
		cli = c.proxiedStorageCli
	}
	resp, err := cli.Do(req)
	if err != nil {
		if req.Context().Err() != nil {
			return nil, req.Context().Err()
		}
		var re *Error
		if errors.As(err, &re) {
			return nil, re
		}
		return nil, &Error{Code: "unavailable"}
	}
	return resp, nil
}

// ---- Control-plane & Direct-to-S3 endpoints ----

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

// CreateTransfer announces a transfer (single-chunk or multi-chunk) with its
// signed outer envelope and receives inline presigned PUT grants.
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
	if err := json.NewDecoder(io.LimitReader(resp.Body, 512<<10)).Decode(&out); err != nil {
		return CreateTransferResponse{}, &Error{Code: "unavailable"}
	}
	if out.UploadMode != "presigned" {
		return CreateTransferResponse{}, &Error{Code: "unavailable"}
	}
	return out, nil
}

// RequestChunkUploadURL issues or refreshes a short-lived presigned PUT URL for chunk index.
func (c *Client) RequestChunkUploadURL(ctx context.Context, mailboxID, deliveryToken, transferID string, index int) (ChunkUploadGrant, error) {
	path := fmt.Sprintf("/v1/relay/mailboxes/%s/transfers/%s/chunks/%d/upload-url", mailboxID, transferID, index)
	req, err := http.NewRequestWithContext(ctx, http.MethodPost, c.origin+path, nil)
	if err != nil {
		return ChunkUploadGrant{}, &Error{Code: "invalid_argument"}
	}
	req.Header.Set("Authorization", "Bearer "+deliveryToken)
	resp, err := c.do(req)
	if err != nil {
		return ChunkUploadGrant{}, err
	}
	defer func() { _ = resp.Body.Close() }()
	if resp.StatusCode != http.StatusOK {
		_, _ = io.Copy(io.Discard, io.LimitReader(resp.Body, 4096))
		return ChunkUploadGrant{}, mapHTTPStatus(resp.StatusCode)
	}
	var grant ChunkUploadGrant
	if err := json.NewDecoder(io.LimitReader(resp.Body, 64<<10)).Decode(&grant); err != nil {
		return ChunkUploadGrant{}, &Error{Code: "unavailable"}
	}
	grant.Index = index
	if grant.URL == "" {
		return ChunkUploadGrant{}, &Error{Code: "invalid_storage_url"}
	}
	return grant, nil
}

// CommitChunk notifies the relay control plane that chunk index was uploaded to S3.
func (c *Client) CommitChunk(ctx context.Context, mailboxID, deliveryToken, transferID string, index int) error {
	path := fmt.Sprintf("/v1/relay/mailboxes/%s/transfers/%s/chunks/%d/commit", mailboxID, transferID, index)
	req, err := http.NewRequestWithContext(ctx, http.MethodPost, c.origin+path, nil)
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
	if resp.StatusCode != http.StatusNoContent && resp.StatusCode != http.StatusOK {
		return mapHTTPStatus(resp.StatusCode)
	}
	return nil
}

func (c *Client) uploadToPresignedURL(ctx context.Context, grant ChunkUploadGrant, ciphertext []byte) error {
	method := strings.ToUpper(strings.TrimSpace(grant.Method))
	if method == "" {
		method = http.MethodPut
	}
	if method != http.MethodPut {
		return &Error{Code: "invalid_storage_url"}
	}
	parsed, err := c.validateStorageURL(grant.URL)
	if err != nil {
		return err
	}
	req, err := http.NewRequestWithContext(ctx, http.MethodPut, parsed.String(), bytes.NewReader(ciphertext))
	if err != nil {
		return &Error{Code: "invalid_storage_url"}
	}
	req.ContentLength = int64(len(ciphertext))
	applySafeStorageHeaders(req.Header, grant.Headers)

	resp, err := c.doStorage(req)
	if err != nil {
		return err
	}
	defer func() { _ = resp.Body.Close() }()
	_, _ = io.Copy(io.Discard, io.LimitReader(resp.Body, 4096))
	if resp.StatusCode != http.StatusOK && resp.StatusCode != http.StatusCreated && resp.StatusCode != http.StatusNoContent {
		return mapHTTPStatus(resp.StatusCode)
	}
	return nil
}

func (c *Client) grantUsable(grant *ChunkUploadGrant) bool {
	if grant == nil || strings.TrimSpace(grant.URL) == "" {
		return false
	}
	if grant.ExpiresAtMs <= 0 {
		return true
	}
	return time.UnixMilli(grant.ExpiresAtMs).After(c.now().Add(presignRefreshSkew))
}

// UploadChunk uploads one ciphertext chunk directly to S3-compatible object storage
// using the supplied inline grant (or requesting/refreshing a fresh presigned URL)
// and then commits the chunk with the relay control plane.
func (c *Client) UploadChunk(ctx context.Context, mailboxID, deliveryToken, transferID string, index int, ciphertext []byte, grant *ChunkUploadGrant) error {
	usedCached := c.grantUsable(grant)
	var effective ChunkUploadGrant
	if usedCached {
		effective = *grant
	} else {
		var err error
		effective, err = c.RequestChunkUploadURL(ctx, mailboxID, deliveryToken, transferID, index)
		if err != nil {
			return err
		}
	}

	if err := c.uploadToPresignedURL(ctx, effective, ciphertext); err != nil {
		var re *Error
		if usedCached && errors.As(err, &re) && (re.Status == http.StatusForbidden || re.Status == http.StatusUnauthorized) {
			fresh, rerr := c.RequestChunkUploadURL(ctx, mailboxID, deliveryToken, transferID, index)
			if rerr != nil {
				return rerr
			}
			if err = c.uploadToPresignedURL(ctx, fresh, ciphertext); err != nil {
				return err
			}
		} else {
			return err
		}
	}
	return c.CommitChunk(ctx, mailboxID, deliveryToken, transferID, index)
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

// DownloadChunk requests a short-lived presigned GET URL from the relay control
// plane and downloads ciphertext chunk index directly from object storage, bounded by maxBytes.
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
	var pd PresignedDownload
	if err := json.NewDecoder(io.LimitReader(resp.Body, 64<<10)).Decode(&pd); err != nil {
		return nil, &Error{Code: "unavailable"}
	}
	method := strings.ToUpper(strings.TrimSpace(pd.Method))
	if method != "" && method != http.MethodGet {
		return nil, &Error{Code: "invalid_storage_url"}
	}
	if pd.Size > maxBytes {
		return nil, &Error{Code: "limit_exceeded"}
	}
	parsed, err := c.validateStorageURL(pd.URL)
	if err != nil {
		return nil, err
	}
	sReq, err := http.NewRequestWithContext(ctx, http.MethodGet, parsed.String(), nil)
	if err != nil {
		return nil, &Error{Code: "invalid_storage_url"}
	}
	applySafeStorageHeaders(sReq.Header, pd.Headers)

	sResp, err := c.doStorage(sReq)
	if err != nil {
		return nil, err
	}
	defer func() { _ = sResp.Body.Close() }()
	if sResp.StatusCode != http.StatusOK {
		_, _ = io.Copy(io.Discard, io.LimitReader(sResp.Body, 4096))
		return nil, mapHTTPStatus(sResp.StatusCode)
	}
	data, err := io.ReadAll(io.LimitReader(sResp.Body, maxBytes+1))
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
