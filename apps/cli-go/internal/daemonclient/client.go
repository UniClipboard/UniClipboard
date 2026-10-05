// Package daemonclient is the Go counterpart of `uc-daemon-client` for the
// CLI: connection resolution, `/auth/connect` session tokens, enveloped
// requests, typed request errors, and the authenticated WebSocket.
package daemonclient

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"net/http"
	"net/url"
	"os"
	"strings"
	"time"

	"github.com/UniClipboard/UniClipboard/apps/cli-go/internal/daemonproc"
)

const (
	envBaseURL   = "UNICLIPBOARD_DAEMON_BASE_URL"
	envTokenPath = "UNICLIPBOARD_DAEMON_TOKEN_PATH"
	clientType   = "cli"
)

// Client talks to the local daemon as a `cli` client.
type Client struct {
	BaseURL string
	WSURL   string
	token   string
	pid     uint32
	http    *http.Client
}

// NewLocalHTTPClient builds an HTTP client that bypasses proxies, like
// `build_local_http_client`.
func NewLocalHTTPClient(timeout time.Duration) *http.Client {
	transport := http.DefaultTransport.(*http.Transport).Clone()
	transport.Proxy = nil
	return &http.Client{Transport: transport, Timeout: timeout}
}

// FromEnv mirrors `DaemonClientContext::from_env`.
func FromEnv() (*Client, error) {
	baseURL := nonEmptyEnv(envBaseURL)
	tokenPath := nonEmptyEnv(envTokenPath)
	pid := uint32(os.Getpid())
	if baseURL != "" || tokenPath != "" {
		if baseURL == "" {
			return nil, fmt.Errorf("%s must be set when using env-based daemon connection overrides", envBaseURL)
		}
		var token string
		if tokenPath != "" {
			data, err := os.ReadFile(tokenPath)
			if err != nil {
				if _, statErr := os.Stat(tokenPath); statErr != nil {
					return nil, fmt.Errorf("daemon auth token not found at %s (is the daemon running?)", tokenPath)
				}
				return nil, fmt.Errorf("failed to read daemon auth token at %s", tokenPath)
			}
			token = strings.TrimSpace(string(data))
			if token == "" {
				return nil, fmt.Errorf("daemon auth token at %s is empty", tokenPath)
			}
		} else {
			conn, err := daemonproc.ReadConnFile()
			if err != nil {
				return nil, err
			}
			if conn == nil {
				return nil, errors.New("daemon connection file not found (is the daemon running?)")
			}
			token = conn.Token
		}
		return &Client{BaseURL: baseURL, WSURL: toWS(baseURL), token: token, pid: pid, http: NewLocalHTTPClient(0)}, nil
	}
	conn, err := daemonproc.ReadConnFile()
	if err != nil {
		return nil, err
	}
	if conn == nil {
		return nil, errors.New("daemon connection file not found (is the daemon running?)")
	}
	base := conn.BaseURL()
	return &Client{BaseURL: base, WSURL: toWS(base) + "/ws", token: conn.Token, pid: pid, http: NewLocalHTTPClient(0)}, nil
}

func toWS(base string) string {
	if strings.HasPrefix(base, "http://") {
		return "ws://" + strings.TrimPrefix(base, "http://")
	}
	if strings.HasPrefix(base, "https://") {
		return "wss://" + strings.TrimPrefix(base, "https://")
	}
	return base
}

func nonEmptyEnv(name string) string { return strings.TrimSpace(os.Getenv(name)) }

// RequestError mirrors `DaemonRequestError`.
type RequestError struct {
	Kind    RequestErrorKind
	Path    string
	Status  int
	Code    string // empty when the daemon sent no code
	Message string
	Err     error
}

// RequestErrorKind enumerates the Rust error variants.
type RequestErrorKind int

const (
	ErrAuth RequestErrorKind = iota
	ErrTransport
	ErrStatus
	ErrDecode
)

func (e *RequestError) Error() string {
	switch e.Kind {
	case ErrAuth:
		return fmt.Sprintf("failed to authorize daemon request %s: %v", e.Path, e.Err)
	case ErrTransport:
		return fmt.Sprintf("failed to call daemon route %s: %v", e.Path, e.Err)
	case ErrStatus:
		suffix := ""
		if e.Code != "" {
			suffix = " [" + e.Code + "]"
		}
		return fmt.Sprintf("daemon request %s failed with status %s%s: %s", e.Path, StatusText(e.Status), suffix, e.Message)
	default:
		return fmt.Sprintf("failed to decode daemon response for %s: %v", e.Path, e.Err)
	}
}

func (e *RequestError) Unwrap() error { return e.Err }

// StatusText renders a status like Rust's `StatusCode` Display ("404 Not Found").
func StatusText(code int) string {
	if text := http.StatusText(code); text != "" {
		return fmt.Sprintf("%d %s", code, text)
	}
	return fmt.Sprintf("%d <unknown status code>", code)
}

// AsRequestError extracts a daemon request error from err's chain.
func AsRequestError(err error) (*RequestError, bool) {
	var re *RequestError
	ok := errors.As(err, &re)
	return re, ok
}

// ErrorCode returns the daemon error code carried by err, if any.
func ErrorCode(err error) string {
	if re, ok := AsRequestError(err); ok && re.Kind == ErrStatus {
		return re.Code
	}
	return ""
}

// IsNotFound reports a 404 daemon status error.
func IsNotFound(err error) bool {
	re, ok := AsRequestError(err)
	return ok && re.Kind == ErrStatus && re.Status == http.StatusNotFound
}

// DisplayMessage mirrors `commands::daemon_error_message`: prefer the
// daemon's human message over the full request error.
func DisplayMessage(err error) string {
	if re, ok := AsRequestError(err); ok && re.Kind == ErrStatus {
		return re.Message
	}
	return err.Error()
}

type sendError struct{ url string }

func (e sendError) Error() string { return fmt.Sprintf("error sending request for url (%s)", e.url) }

// SessionToken exchanges the bearer token for a short-lived session token.
func (c *Client) SessionToken(ctx context.Context) (string, error) {
	body, _ := json.Marshal(map[string]any{"pid": c.pid, "clientType": clientType})
	req, err := http.NewRequestWithContext(ctx, http.MethodPost, c.BaseURL+"/auth/connect", bytes.NewReader(body))
	if err != nil {
		return "", err
	}
	req.Header.Set("Authorization", "Bearer "+c.token)
	req.Header.Set("Content-Type", "application/json")
	resp, err := c.http.Do(req)
	if err != nil {
		return "", fmt.Errorf("failed to send session token exchange request: %w", err)
	}
	defer resp.Body.Close()
	data, _ := io.ReadAll(resp.Body)
	if resp.StatusCode < 200 || resp.StatusCode > 299 {
		return "", fmt.Errorf("session token exchange failed with status %s: %s", StatusText(resp.StatusCode), data)
	}
	var env struct {
		Data struct {
			SessionToken string `json:"sessionToken"`
		} `json:"data"`
	}
	if err := json.Unmarshal(data, &env); err != nil {
		return "", fmt.Errorf("failed to decode session token exchange response: %w", err)
	}
	return env.Data.SessionToken, nil
}

// Request describes one daemon HTTP call.
type Request struct {
	Method  string
	Path    string
	Query   url.Values
	JSON    any       // marshalled as the JSON body when non-nil
	Body    io.Reader // raw body (used when JSON is nil)
	Headers map[string]string
}

// Send performs an authorized request and returns the successful response.
// The caller closes the body.
func (c *Client) Send(ctx context.Context, r Request) (*http.Response, error) {
	token, err := c.SessionToken(ctx)
	if err != nil {
		return nil, &RequestError{Kind: ErrAuth, Path: r.Path, Err: err}
	}
	target := c.BaseURL + r.Path
	if len(r.Query) > 0 {
		target += "?" + r.Query.Encode()
	}
	body := r.Body
	if r.JSON != nil {
		data, err := marshalNoEscape(r.JSON)
		if err != nil {
			return nil, err
		}
		body = bytes.NewReader(data)
	}
	req, err := http.NewRequestWithContext(ctx, r.Method, target, body)
	if err != nil {
		return nil, &RequestError{Kind: ErrTransport, Path: r.Path, Err: err}
	}
	req.Header.Set("Authorization", "Session "+token)
	if r.JSON != nil {
		req.Header.Set("Content-Type", "application/json")
	}
	for k, v := range r.Headers {
		req.Header.Set(k, v)
	}
	resp, err := c.http.Do(req)
	if err != nil {
		if ctx.Err() != nil {
			return nil, ctx.Err()
		}
		return nil, &RequestError{Kind: ErrTransport, Path: r.Path, Err: sendError{url: target}}
	}
	if resp.StatusCode < 200 || resp.StatusCode > 299 {
		defer resp.Body.Close()
		raw, readErr := io.ReadAll(resp.Body)
		text := string(raw)
		if readErr != nil {
			text = "<failed to read body>"
		}
		code, message := parseErrorBody(text)
		return nil, &RequestError{Kind: ErrStatus, Path: r.Path, Status: resp.StatusCode, Code: code, Message: message}
	}
	return resp, nil
}

func parseErrorBody(body string) (string, string) {
	var parsed struct {
		Code    *string `json:"code"`
		Message *string `json:"message"`
	}
	if err := json.Unmarshal([]byte(body), &parsed); err != nil {
		return "", body
	}
	code := ""
	if parsed.Code != nil {
		code = *parsed.Code
	}
	if parsed.Message == nil {
		return code, body
	}
	return code, *parsed.Message
}

// Enveloped performs a request and decodes `{ "data": ... }` into out. out may
// be a *json.RawMessage to keep the daemon's exact bytes.
func (c *Client) Enveloped(ctx context.Context, r Request, out any) error {
	resp, err := c.Send(ctx, r)
	if err != nil {
		return err
	}
	defer resp.Body.Close()
	var env struct {
		Data json.RawMessage `json:"data"`
	}
	data, err := io.ReadAll(resp.Body)
	if err == nil {
		err = json.Unmarshal(data, &env)
	}
	if err == nil && out != nil {
		err = json.Unmarshal(env.Data, out)
	}
	if err != nil {
		return &RequestError{Kind: ErrDecode, Path: r.Path, Err: errors.New("error decoding response body")}
	}
	return nil
}

// Empty performs a request whose successful body is ignored.
func (c *Client) Empty(ctx context.Context, r Request) error {
	resp, err := c.Send(ctx, r)
	if err != nil {
		return err
	}
	io.Copy(io.Discard, resp.Body)
	return resp.Body.Close()
}

// Get is a shorthand for an enveloped GET.
func (c *Client) Get(ctx context.Context, path string, out any) error {
	return c.Enveloped(ctx, Request{Method: http.MethodGet, Path: path}, out)
}

// PathSegment percent-encodes a dynamic URL path segment like Rust's
// `encode_path_segment`.
func PathSegment(segment string) (string, error) {
	if segment == "." || segment == ".." {
		return "", errors.New("dynamic URL path segment cannot be `.` or `..`")
	}
	var b strings.Builder
	for i := 0; i < len(segment); i++ {
		ch := segment[i]
		if ch >= 'a' && ch <= 'z' || ch >= 'A' && ch <= 'Z' || ch >= '0' && ch <= '9' || ch == '-' || ch == '.' || ch == '_' || ch == '~' {
			b.WriteByte(ch)
		} else {
			fmt.Fprintf(&b, "%%%02X", ch)
		}
	}
	return b.String(), nil
}

func marshalNoEscape(v any) ([]byte, error) {
	var buf bytes.Buffer
	enc := json.NewEncoder(&buf)
	enc.SetEscapeHTML(false)
	if err := enc.Encode(v); err != nil {
		return nil, err
	}
	return bytes.TrimRight(buf.Bytes(), "\n"), nil
}
