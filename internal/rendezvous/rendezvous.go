// Package rendezvous talks to uc-rendezvous's `/v1/web-pairings` routes, which
// map a 6-digit code to an opaque ticket for five minutes. unic stores a
// pairing ticket there so that another device can find this one by typing
// six digits instead of the whole address.
//
// Codes, tickets, tokens and addresses are secrets: never log them.
//
// Design: docs/architecture/identity-and-trust.md.
package rendezvous

import (
	"bytes"
	"context"
	"crypto/rand"
	"encoding/base64"
	"encoding/json"
	"errors"
	"io"
	"net/http"
	"os"
	"strings"
	"time"
	"unicode"
	"unicode/utf8"

	"golang.org/x/text/unicode/norm"
)

// DefaultURL is the production service; UNIC_RENDEZVOUS_URL overrides it.
const DefaultURL = "https://rendezvous.uniclipboard.app"

// TicketKind marks unic pairing tickets. The website's transfer tickets use
// "uc-web-try"; each side rejects the other's.
const TicketKind = "unic-pair"

const (
	// MaxLifetime is the server's fixed code TTL; no code lives longer.
	MaxLifetime = 300 * time.Second
	// minLifetime admits the 5-second TTL of a local development service.
	minLifetime    = 5 * time.Second
	requestTimeout = 10 * time.Second
)

// TextPrefix starts the copyable text form of a ticket.
const TextPrefix = "unic-pair:"

// MaxTicketBytes bounds a whole ticket in either form.
const MaxTicketBytes = 2048

// MaxAccess bounds the opaque access description a backend puts in a ticket.
const MaxAccess = 1024

// Transport kinds a ticket may name.
const (
	TransportPeerSessions = "peer_sessions"
	TransportObjectStore  = "object_store"
)

// Ticket is what a code stands for (identity-protocol.md §6): the issuing
// device's public key, a backend-defined access description, and a one-time
// token. It is a secret like a password and grants no trust by itself.
type Ticket struct {
	Transport string // TransportPeerSessions or TransportObjectStore
	Backend   string // the sync backend instance that issued it and must handle Access
	Device    []byte // the issuer's identity public key (32 bytes)
	Access    []byte // opaque to the core; at most MaxAccess bytes
	Token     []byte // 16 random bytes
}

type ticketJSON struct {
	V    int    `json:"v"`
	Kind string `json:"kind"`
	TP   string `json:"tp"`
	BE   string `json:"be"`
	Dev  string `json:"dev"`
	Acc  string `json:"acc"`
	Tok  string `json:"tok"`
}

// NewToken returns a fresh 128-bit one-time token.
func NewToken() []byte {
	b := make([]byte, 16)
	_, _ = rand.Read(b)
	return b
}

var b64 = base64.RawURLEncoding

// Encode is the JSON form stored at the code service.
func (t Ticket) Encode() string {
	b, _ := json.Marshal(ticketJSON{V: 2, Kind: TicketKind, TP: t.Transport, BE: t.Backend,
		Dev: b64.EncodeToString(t.Device), Acc: b64.EncodeToString(t.Access), Tok: b64.EncodeToString(t.Token)})
	return string(b)
}

// Text is the copyable form: "unic-pair:" and the base64url of Encode.
func (t Ticket) Text() string { return TextPrefix + b64.EncodeToString([]byte(t.Encode())) }

// ParseTicket accepts the JSON form of a unic ticket and nothing else.
func ParseTicket(raw string) (Ticket, bool) {
	var tj ticketJSON
	if len(raw) > MaxTicketBytes || json.Unmarshal([]byte(raw), &tj) != nil || tj.V != 2 || tj.Kind != TicketKind {
		return Ticket{}, false
	}
	if tj.TP != TransportPeerSessions && tj.TP != TransportObjectStore {
		return Ticket{}, false
	}
	if tj.BE == "" || len(tj.BE) > 128 || !utf8.ValidString(tj.BE) {
		return Ticket{}, false
	}
	dev, e1 := b64.DecodeString(tj.Dev)
	acc, e2 := b64.DecodeString(tj.Acc)
	tok, e3 := b64.DecodeString(tj.Tok)
	if e1 != nil || e2 != nil || e3 != nil || len(dev) != 32 || len(tok) != 16 || len(acc) == 0 || len(acc) > MaxAccess {
		return Ticket{}, false
	}
	return Ticket{Transport: tj.TP, Backend: tj.BE, Device: dev, Access: acc, Token: tok}, true
}

// ParseText accepts what a person pastes: the text form, with surrounding
// white space and line breaks tolerated.
func ParseText(in string) (Ticket, bool) {
	s := strings.Join(strings.Fields(in), "")
	if !strings.HasPrefix(s, TextPrefix) || len(s) > MaxTicketBytes*2 {
		return Ticket{}, false
	}
	raw, err := b64.DecodeString(strings.TrimPrefix(s, TextPrefix))
	if err != nil {
		return Ticket{}, false
	}
	return ParseTicket(string(raw))
}

// NormalizeCode accepts what people type or paste ("482913", "482 913",
// "482-913", full-width digits) and returns the wire form "NNN-NNN".
func NormalizeCode(in string) (string, bool) {
	s := norm.NFKC.String(in)
	var d strings.Builder
	for _, r := range s {
		switch {
		case r >= '0' && r <= '9':
			d.WriteRune(r)
		case unicode.IsSpace(r) || r == '-' || (r >= '‐' && r <= '―'):
		default:
			return "", false
		}
	}
	if d.Len() != 6 {
		return "", false
	}
	v := d.String()
	return v[:3] + "-" + v[3:], true
}

// Reason classifies a failure for the user.
type Reason string

const (
	Incomplete  Reason = "incomplete"   // not six digits
	Expired     Reason = "expired"      // unknown, expired or already used (404, 409)
	Invalid     Reason = "invalid"      // the code holds something other than a unic ticket
	RateLimited Reason = "rate-limited" // 429
	Unavailable Reason = "unavailable"  // 5xx, network, timeout, malformed reply
)

// Error is a failed code operation.
type Error struct{ Reason Reason }

func (e *Error) Error() string {
	switch e.Reason {
	case Incomplete:
		return "enter all 6 digits of the code"
	case Expired:
		return "this code has expired or was already used; ask for a new one"
	case Invalid:
		return "that is not a unic pairing code"
	case RateLimited:
		return "too many tries; wait a minute and try again"
	default:
		return "the pairing code service is unavailable; pair with the full address instead"
	}
}

// ReasonOf returns the reason of a rendezvous error, or "".
func ReasonOf(err error) Reason {
	var e *Error
	if errors.As(err, &e) {
		return e.Reason
	}
	return ""
}

type Client struct {
	URL  string
	HTTP *http.Client
}

// New returns a client for UNIC_RENDEZVOUS_URL or the production service.
func New() *Client {
	return NewWithURL(os.Getenv("UNIC_RENDEZVOUS_URL"))
}

// NewWithURL returns a client for u, or for the production service if u is empty.
func NewWithURL(u string) *Client {
	if u == "" {
		u = DefaultURL
	}
	return &Client{URL: strings.TrimRight(u, "/"), HTTP: &http.Client{Timeout: requestTimeout}}
}

func (c *Client) post(ctx context.Context, path string, in, out any) error {
	b, _ := json.Marshal(in)
	req, err := http.NewRequestWithContext(ctx, "POST", c.URL+path, bytes.NewReader(b))
	if err != nil {
		return &Error{Unavailable}
	}
	req.Header.Set("Content-Type", "application/json")
	resp, err := c.HTTP.Do(req)
	if err != nil {
		if ctx.Err() != nil {
			return ctx.Err()
		}
		return &Error{Unavailable}
	}
	defer func() { _ = resp.Body.Close() }()
	switch {
	case resp.StatusCode == 404 || resp.StatusCode == 409:
		return &Error{Expired}
	case resp.StatusCode == 400:
		return &Error{Incomplete}
	case resp.StatusCode == 429:
		return &Error{RateLimited}
	case resp.StatusCode != 200:
		return &Error{Unavailable}
	}
	if out == nil {
		return nil
	}
	if json.NewDecoder(io.LimitReader(resp.Body, 64<<10)).Decode(out) != nil {
		return &Error{Unavailable}
	}
	return nil
}

// Code is a registered code; Expires is on this machine's clock.
type Code struct {
	Code    string // "NNN-NNN"
	Expires time.Time
}

// Display renders the code as "NNN NNN".
func (c Code) Display() string { return strings.Replace(c.Code, "-", " ", 1) }

// Create registers t and returns a fresh code for it.
func (c *Client) Create(ctx context.Context, t Ticket) (Code, error) {
	sent := time.Now()
	var r struct {
		Code        string  `json:"code"`
		ExpiresAtMs float64 `json:"expiresAtMs"`
	}
	if err := c.post(ctx, "/v1/web-pairings", map[string]string{"ticket": t.Encode()}, &r); err != nil {
		return Code{}, err
	}
	code, ok := NormalizeCode(r.Code)
	if !ok || r.ExpiresAtMs <= 0 {
		return Code{}, &Error{Unavailable}
	}
	return Code{Code: code, Expires: localDeadline(sent, time.Now(), time.UnixMilli(int64(r.ExpiresAtMs)))}, nil
}

// localDeadline turns the server's expiry (its clock) into one on ours: the
// lifetime is clamped to [minLifetime, MaxLifetime] and never ends later than
// MaxLifetime after the request was sent.
func localDeadline(sent, received, serverExpires time.Time) time.Time {
	life := serverExpires.Sub(received)
	life = max(minLifetime, min(MaxLifetime, life))
	d := received.Add(life)
	if limit := sent.Add(MaxLifetime); d.After(limit) {
		d = limit
	}
	return d
}

// Resolve looks a code up. It does not use it up; see Consume.
func (c *Client) Resolve(ctx context.Context, code string) (Ticket, error) {
	n, ok := NormalizeCode(code)
	if !ok {
		return Ticket{}, &Error{Incomplete}
	}
	var r struct {
		Ticket string `json:"ticket"`
	}
	if err := c.post(ctx, "/v1/web-pairings/resolve", map[string]string{"code": n}, &r); err != nil {
		return Ticket{}, err
	}
	t, ok := ParseTicket(r.Ticket)
	if !ok {
		return Ticket{}, &Error{Invalid}
	}
	return t, nil
}

// Consume stops later lookups of a code. Best effort: the code expires on its
// own within five minutes.
func (c *Client) Consume(ctx context.Context, code string) {
	if n, ok := NormalizeCode(code); ok {
		_ = c.post(ctx, "/v1/web-pairings/consume", map[string]string{"code": n}, nil)
	}
}

// CreateRaw registers an opaque ticket text and returns its code. The browser
// transfer page uses this for its own ticket kind.
func (c *Client) CreateRaw(ctx context.Context, ticket string) (Code, error) {
	sent := time.Now()
	var r struct {
		Code        string  `json:"code"`
		ExpiresAtMs float64 `json:"expiresAtMs"`
	}
	if err := c.post(ctx, "/v1/web-pairings", map[string]string{"ticket": ticket}, &r); err != nil {
		return Code{}, err
	}
	code, ok := NormalizeCode(r.Code)
	if !ok || r.ExpiresAtMs <= 0 {
		return Code{}, &Error{Unavailable}
	}
	return Code{Code: code, Expires: localDeadline(sent, time.Now(), time.UnixMilli(int64(r.ExpiresAtMs)))}, nil
}

// ResolveRaw looks a code up and returns the stored ticket text unparsed.
func (c *Client) ResolveRaw(ctx context.Context, code string) (string, error) {
	n, ok := NormalizeCode(code)
	if !ok {
		return "", &Error{Incomplete}
	}
	var r struct {
		Ticket string `json:"ticket"`
	}
	if err := c.post(ctx, "/v1/web-pairings/resolve", map[string]string{"code": n}, &r); err != nil {
		return "", err
	}
	return r.Ticket, nil
}
