// Package cloudauth implements RFC 8252 loopback OAuth 2.0 + PKCE login against
// UniClipboard Cloud (https://cloud.uniclipboard.app) and mints a user-scoped
// Hyper Relay API key for automatic configuration.
package cloudauth

import (
	"bytes"
	"context"
	"crypto/rand"
	"crypto/sha256"
	"crypto/subtle"
	"encoding/base64"
	"encoding/json"
	"errors"
	"fmt"
	"html"
	"io"
	"net"
	"net/http"
	"net/url"
	"os"
	"os/exec"
	"runtime"
	"strings"
	"sync"
	"time"
)

const (
	// DefaultCloudURL is the official UniClipboard Cloud origin.
	DefaultCloudURL = "https://cloud.uniclipboard.app"

	defaultHTTPTimeout = 20 * time.Second
	maxJSONRespBytes   = 64 << 10
)

// LoginOptions configures LoginForRelayKey.
type LoginOptions struct {
	// CloudURL overrides UNIC_CLOUD_URL and DefaultCloudURL when non-empty.
	CloudURL string
	// DeviceName is displayed on the consent screen and used as the API key label.
	DeviceName string
	// OpenBrowser is invoked with the authorization URL once the loopback listener
	// is ready. If nil, OpenSystemBrowser is used.
	OpenBrowser func(authURL string) error
	// HTTPClient overrides the HTTP client used for token/API-key exchanges.
	HTTPClient *http.Client
}

// RelayCredential is the provisioned Cloud Relay API key and metadata.
// APIKey is a secret and must never be logged or echoed in error messages.
type RelayCredential struct {
	KeyID            string `json:"id"`
	Name             string `json:"name"`
	KeyPrefix        string `json:"key_prefix"`
	APIKey           string `json:"api_key"`
	RelayURL         string `json:"relay_url"`
	MaxTransferBytes int64  `json:"max_transfer_bytes"`
}

// ResolveCloudURL validates and normalizes the target UniClipboard Cloud origin.
func ResolveCloudURL(explicit string) (string, error) {
	raw := strings.TrimSpace(explicit)
	if raw == "" {
		raw = strings.TrimSpace(os.Getenv("UNIC_CLOUD_URL"))
	}
	if raw == "" {
		raw = DefaultCloudURL
	}
	u, err := url.Parse(raw)
	if err != nil || u.Host == "" || u.User != nil || u.RawQuery != "" || u.Fragment != "" {
		return "", errors.New("cloud URL must be a valid http(s) origin")
	}
	scheme := strings.ToLower(u.Scheme)
	host := strings.ToLower(u.Hostname())
	isLoopback := host == "127.0.0.1" || host == "localhost" || host == "::1"
	if scheme != "https" && (scheme != "http" || !isLoopback) {
		return "", errors.New("cloud URL must use https (or loopback http for local testing)")
	}
	u.Scheme = scheme
	u.Path = strings.TrimRight(u.Path, "/")
	return u.String(), nil
}

// OpenSystemBrowser launches rawURL in the operating system's default browser.
func OpenSystemBrowser(rawURL string) error {
	u, err := url.Parse(strings.TrimSpace(rawURL))
	if err != nil || (u.Scheme != "https" && u.Scheme != "http") || u.Host == "" {
		return errors.New("only http and https URLs are supported")
	}
	var cmd *exec.Cmd
	switch runtime.GOOS {
	case "darwin":
		cmd = exec.Command("open", u.String())
	case "windows":
		cmd = exec.Command("rundll32", "url.dll,FileProtocolHandler", u.String())
	default:
		cmd = exec.Command("xdg-open", u.String())
	}
	return cmd.Start()
}

// LoginForRelayKey runs the RFC 8252 loopback OAuth 2.0 + PKCE flow against
// UniClipboard Cloud and mints a fresh user-scoped Hyper Relay API key.
func LoginForRelayKey(ctx context.Context, opts LoginOptions) (RelayCredential, error) {
	cloudURL, err := ResolveCloudURL(opts.CloudURL)
	if err != nil {
		return RelayCredential{}, err
	}
	deviceName := strings.TrimSpace(opts.DeviceName)
	if deviceName == "" {
		if h, herr := os.Hostname(); herr == nil {
			deviceName = strings.TrimSpace(h)
		}
	}
	if deviceName == "" {
		deviceName = "UniClipboard device"
	}
	if len(deviceName) > 120 {
		deviceName = deviceName[:120]
	}

	verifier, err := randomBase64URL(32) // 43 chars, valid RFC 7636 verifier
	if err != nil {
		return RelayCredential{}, err
	}
	challengeSum := sha256.Sum256([]byte(verifier))
	challenge := base64.RawURLEncoding.EncodeToString(challengeSum[:])

	state, err := randomBase64URL(24)
	if err != nil {
		return RelayCredential{}, err
	}

	ln, err := net.Listen("tcp", "127.0.0.1:0")
	if err != nil {
		return RelayCredential{}, fmt.Errorf("start local callback server: %w", err)
	}
	defer func() { _ = ln.Close() }()

	redirectURI := fmt.Sprintf("http://%s/cb", ln.Addr().String())

	type callbackResult struct {
		code string
		err  error
	}
	resCh := make(chan callbackResult, 1)
	var once sync.Once
	deliver := func(r callbackResult) {
		once.Do(func() { resCh <- r })
	}

	mux := http.NewServeMux()
	mux.HandleFunc("/cb", func(w http.ResponseWriter, r *http.Request) {
		if r.Method != http.MethodGet {
			http.Error(w, "method not allowed", http.StatusMethodNotAllowed)
			return
		}
		q := r.URL.Query()
		if errParam := strings.TrimSpace(q.Get("error")); errParam != "" {
			writeCallbackHTML(w, http.StatusBadRequest, "Authorization failed", "UniClipboard Cloud denied or canceled the request.")
			deliver(callbackResult{err: fmt.Errorf("cloud authorization failed: %s", sanitizeToken(errParam))})
			return
		}
		gotState := q.Get("state")
		code := strings.TrimSpace(q.Get("code"))
		if subtle.ConstantTimeCompare([]byte(gotState), []byte(state)) != 1 || code == "" {
			writeCallbackHTML(w, http.StatusBadRequest, "Invalid callback", "State verification failed. Please retry from UniClipboard.")
			deliver(callbackResult{err: errors.New("cloud callback state mismatch")})
			return
		}
		writeCallbackHTML(w, http.StatusOK, "Authorization complete", "You can close this tab and return to UniClipboard.")
		deliver(callbackResult{code: code})
	})

	srv := &http.Server{
		Handler:           mux,
		ReadHeaderTimeout: 10 * time.Second,
	}
	go func() { _ = srv.Serve(ln) }()
	defer func() {
		shutCtx, cancel := context.WithTimeout(context.Background(), 2*time.Second)
		defer cancel()
		_ = srv.Shutdown(shutCtx)
	}()

	q := url.Values{}
	q.Set("redirect_uri", redirectURI)
	q.Set("state", state)
	q.Set("code_challenge", challenge)
	q.Set("code_challenge_method", "S256")
	q.Set("device_name", deviceName)
	authURL := cloudURL + "/cli/authorize?" + q.Encode()

	openFn := opts.OpenBrowser
	if openFn == nil {
		openFn = OpenSystemBrowser
	}
	if err := openFn(authURL); err != nil {
		return RelayCredential{}, fmt.Errorf("open browser for cloud login: %w", err)
	}

	var code string
	select {
	case <-ctx.Done():
		return RelayCredential{}, ctx.Err()
	case cb := <-resCh:
		if cb.err != nil {
			return RelayCredential{}, cb.err
		}
		code = cb.code
	}

	httpCli := opts.HTTPClient
	if httpCli == nil {
		httpCli = &http.Client{Timeout: defaultHTTPTimeout}
	}

	accessToken, err := exchangeCode(ctx, httpCli, cloudURL, code, verifier, redirectURI, deviceName)
	if err != nil {
		return RelayCredential{}, err
	}
	return createRelayAPIKey(ctx, httpCli, cloudURL, accessToken, deviceName)
}

func exchangeCode(ctx context.Context, cli *http.Client, cloudURL, code, verifier, redirectURI, deviceName string) (string, error) {
	payload, err := json.Marshal(map[string]string{
		"code":          code,
		"code_verifier": verifier,
		"redirect_uri":  redirectURI,
		"device_name":   deviceName,
	})
	if err != nil {
		return "", err
	}
	req, err := http.NewRequestWithContext(ctx, http.MethodPost, cloudURL+"/api/auth/token", bytes.NewReader(payload))
	if err != nil {
		return "", err
	}
	req.Header.Set("Content-Type", "application/json")
	resp, err := cli.Do(req)
	if err != nil {
		return "", fmt.Errorf("cloud token exchange failed: %w", err)
	}
	defer func() { _ = resp.Body.Close() }()
	body, err := io.ReadAll(io.LimitReader(resp.Body, maxJSONRespBytes))
	if err != nil {
		return "", errors.New("failed to read cloud token response")
	}
	if resp.StatusCode != http.StatusOK {
		return "", fmt.Errorf("cloud token exchange failed (http %d)", resp.StatusCode)
	}
	var out struct {
		AccessToken string `json:"access_token"`
	}
	if err := json.Unmarshal(body, &out); err != nil || strings.TrimSpace(out.AccessToken) == "" {
		return "", errors.New("cloud token exchange returned invalid response")
	}
	return strings.TrimSpace(out.AccessToken), nil
}

func createRelayAPIKey(ctx context.Context, cli *http.Client, cloudURL, accessToken, deviceName string) (RelayCredential, error) {
	payload, err := json.Marshal(map[string]string{
		"name": deviceName,
	})
	if err != nil {
		return RelayCredential{}, err
	}
	req, err := http.NewRequestWithContext(ctx, http.MethodPost, cloudURL+"/api/relay/v2/api-keys", bytes.NewReader(payload))
	if err != nil {
		return RelayCredential{}, err
	}
	req.Header.Set("Content-Type", "application/json")
	req.Header.Set("Authorization", "Bearer "+accessToken)
	resp, err := cli.Do(req)
	if err != nil {
		return RelayCredential{}, fmt.Errorf("cloud relay api key request failed: %w", err)
	}
	defer func() { _ = resp.Body.Close() }()
	body, err := io.ReadAll(io.LimitReader(resp.Body, maxJSONRespBytes))
	if err != nil {
		return RelayCredential{}, errors.New("failed to read cloud relay api key response")
	}
	if resp.StatusCode != http.StatusCreated && resp.StatusCode != http.StatusOK {
		if resp.StatusCode == http.StatusTooManyRequests {
			return RelayCredential{}, errors.New("cloud relay api key limit reached; revoke an unused key in UniClipboard Cloud")
		}
		return RelayCredential{}, fmt.Errorf("cloud relay api key creation failed (http %d)", resp.StatusCode)
	}
	var cred RelayCredential
	if err := json.Unmarshal(body, &cred); err != nil || strings.TrimSpace(cred.APIKey) == "" {
		return RelayCredential{}, errors.New("cloud returned invalid relay api key")
	}
	cred.APIKey = strings.TrimSpace(cred.APIKey)
	cred.RelayURL = strings.TrimSpace(cred.RelayURL)
	return cred, nil
}

func randomBase64URL(nBytes int) (string, error) {
	b := make([]byte, nBytes)
	if _, err := rand.Read(b); err != nil {
		return "", err
	}
	return base64.RawURLEncoding.EncodeToString(b), nil
}

func sanitizeToken(raw string) string {
	var b strings.Builder
	for _, r := range raw {
		if (r >= 'a' && r <= 'z') || (r >= 'A' && r <= 'Z') || (r >= '0' && r <= '9') || r == '_' || r == '-' {
			b.WriteRune(r)
		}
		if b.Len() >= 64 {
			break
		}
	}
	if b.Len() == 0 {
		return "unknown"
	}
	return b.String()
}

func writeCallbackHTML(w http.ResponseWriter, status int, title, body string) {
	w.Header().Set("Content-Type", "text/html; charset=utf-8")
	w.Header().Set("Cache-Control", "no-store")
	w.WriteHeader(status)
	_, _ = fmt.Fprintf(w, `<!doctype html><html lang="en"><head><meta charset="utf-8"><title>%s</title><style>body{font-family:-apple-system,BlinkMacSystemFont,Segoe UI,sans-serif;max-width:460px;margin:18vh auto;padding:0 20px;color:#111;line-height:1.5}h1{font-size:20px;margin:0 0 8px}p{color:#555;margin:0}</style></head><body><h1>%s</h1><p>%s</p></body></html>`,
		html.EscapeString(title), html.EscapeString(title), html.EscapeString(body))
}
