// Package update implements the signed application update flow shared by the
// Go GUI host: manifest lookup, version comparison, a verified download and an
// in-place install. It speaks the same manifest format as the Tauri updater
// (`{version, notes, pub_date, platforms}` with minisign signatures), so both
// shells consume one release feed.
package update

import (
	"context"
	"encoding/base64"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"net/http"
	"os"
	"path/filepath"
	"runtime"
	"strings"
	"time"

	"aead.dev/minisign"
	"golang.org/x/mod/semver"
)

// ErrInstallUnsupported indicates that the current installation cannot install updates in place.
var ErrInstallUnsupported = errors.New("in-place update install is not implemented on this platform")

// Channel names an update feed. The empty channel resolves to Stable.
type Channel string

const (
	Stable Channel = "stable"
	Alpha  Channel = "alpha"
	Beta   Channel = "beta"
	RC     Channel = "rc"
)

// ParseChannel normalizes a channel name; unknown values fall back to Stable.
func ParseChannel(s string) Channel {
	switch strings.ToLower(s) {
	case "alpha":
		return Alpha
	case "beta":
		return Beta
	case "rc":
		return RC
	}
	return Stable
}

// DetectChannel derives the feed from the running version's prerelease tag.
func DetectChannel(version string) Channel {
	_, pre, ok := strings.Cut(version, "-")
	if !ok {
		return Stable
	}
	return ParseChannel(strings.SplitN(pre, ".", 2)[0])
}

// Manifest is the update feed document.
type Manifest struct {
	Version   string              `json:"version"`
	Notes     string              `json:"notes"`
	PubDate   string              `json:"pub_date"`
	Platforms map[string]Platform `json:"platforms"`
}

// Platform is one downloadable artifact with its minisign signature
// (base64 of the `.sig` file, as produced by the Tauri signer).
type Platform struct {
	Signature string `json:"signature"`
	URL       string `json:"url"`
}

// Release is an available update selected for this platform.
type Release struct {
	Version        string  `json:"version"`
	CurrentVersion string  `json:"currentVersion"`
	Body           *string `json:"body"`
	Date           *string `json:"date"`

	URL       string `json:"-"`
	Signature string `json:"-"`
}

// Client looks up and downloads releases for one installed version.
type Client struct {
	HTTP      *http.Client
	Endpoints func(Channel) []string
	PubKey    minisign.PublicKey
	Current   string
	// Targets are the manifest platform keys to try, most specific first.
	Targets []string
}

// ParsePublicKey decodes the Tauri `pubkey` value (base64 of the minisign `.pub` text).
func ParsePublicKey(encoded string) (minisign.PublicKey, error) {
	var key minisign.PublicKey
	text, err := base64.StdEncoding.DecodeString(strings.TrimSpace(encoded))
	if err != nil {
		return key, fmt.Errorf("decode public key: %w", err)
	}
	return key, key.UnmarshalText(text)
}

// DefaultTargets lists the manifest keys for the running platform.
func DefaultTargets(installer string) []string {
	arch := map[string]string{"amd64": "x86_64", "arm64": "aarch64"}[runtime.GOARCH]
	base := runtime.GOOS + "-" + arch
	if installer == "" {
		return []string{base}
	}
	return []string{base + "-" + installer, base}
}

// DefaultEndpoints are the production feeds for a channel.
func DefaultEndpoints(channel Channel) []string {
	return []string{
		fmt.Sprintf("https://release.uniclipboard.app/%s.json", channel),
		fmt.Sprintf("https://uniclipboard.github.io/UniClipboard/%s.json", channel),
	}
}

var errBadManifest = errors.New("update manifest is invalid")

// FailureKind is the analytics wire classification of a failed update check (snake_case, like the daemon's
// `UiUpdateFailureKind`): where the failure came from rather than a guess from the message text.
type FailureKind string

const (
	FailureNetwork FailureKind = "network"
	FailureHTTP    FailureKind = "http_error"
	FailureParse   FailureKind = "parse_error"
	FailureOther   FailureKind = "other"
)

// kindError tags an error with its failure kind without changing the message the user reads.
type kindError struct {
	kind FailureKind
	err  error
}

func (e *kindError) Error() string { return e.err.Error() }
func (e *kindError) Unwrap() error { return e.err }

func tagged(kind FailureKind, err error) error { return &kindError{kind: kind, err: err} }

// Classify reports the failure kind of an error returned by Check: transport failures are network, a non-2xx
// feed answer is http_error, an undecodable or invalid manifest and a bad signature are parse_error.
func Classify(err error) FailureKind {
	var tag *kindError
	switch {
	case errors.As(err, &tag):
		return tag.kind
	case errors.Is(err, errBadManifest):
		return FailureParse
	}
	return FailureOther
}

// Check returns the newest release for the channel, or nil when the installed
// version is current. The first endpoint that answers wins; later endpoints are
// fallbacks for transport or HTTP failures.
func (c *Client) Check(ctx context.Context, channel Channel) (*Release, error) {
	var lastErr error
	for _, endpoint := range c.Endpoints(channel) {
		manifest, err := c.fetch(ctx, endpoint)
		if err != nil {
			lastErr = err
			continue
		}
		return c.selectRelease(manifest)
	}
	if lastErr == nil {
		lastErr = errors.New("no update endpoints configured")
	}
	return nil, lastErr
}

func (c *Client) fetch(ctx context.Context, endpoint string) (*Manifest, error) {
	req, err := http.NewRequestWithContext(ctx, http.MethodGet, endpoint, nil)
	if err != nil {
		return nil, err
	}
	// The release service uses the installed version to merge notes for (current, latest].
	// Static fallback feeds ignore the query and still provide the latest release.
	query := req.URL.Query()
	query.Set("from", c.Current)
	req.URL.RawQuery = query.Encode()
	req.Header.Set("Accept", "application/json")
	resp, err := c.HTTP.Do(req)
	if err != nil {
		return nil, tagged(FailureNetwork, fmt.Errorf("connect %s: %w", endpoint, err))
	}
	defer resp.Body.Close()
	if resp.StatusCode == http.StatusNoContent {
		return &Manifest{}, nil
	}
	if resp.StatusCode != http.StatusOK {
		return nil, tagged(FailureHTTP, fmt.Errorf("http status %d from %s", resp.StatusCode, endpoint))
	}
	var manifest Manifest
	if err := json.NewDecoder(io.LimitReader(resp.Body, 4<<20)).Decode(&manifest); err != nil {
		return nil, fmt.Errorf("%w: %v", errBadManifest, err)
	}
	return &manifest, nil
}

func (c *Client) selectRelease(m *Manifest) (*Release, error) {
	if m.Version == "" {
		return nil, nil
	}
	latest, current := "v"+strings.TrimPrefix(m.Version, "v"), "v"+strings.TrimPrefix(c.Current, "v")
	if !semver.IsValid(latest) {
		return nil, fmt.Errorf("%w: version %q", errBadManifest, m.Version)
	}
	if semver.Compare(latest, current) <= 0 {
		return nil, nil
	}
	for _, target := range c.Targets {
		if p, ok := m.Platforms[target]; ok {
			if p.URL == "" || p.Signature == "" {
				return nil, fmt.Errorf("%w: platform %s has no url or signature", errBadManifest, target)
			}
			return &Release{
				Version: m.Version, CurrentVersion: c.Current,
				Body: nonEmpty(m.Notes), Date: nonEmpty(m.PubDate),
				URL: p.URL, Signature: p.Signature,
			}, nil
		}
	}
	return nil, fmt.Errorf("%w: no artifact for %s", errBadManifest, strings.Join(c.Targets, ", "))
}

func nonEmpty(s string) *string {
	if s == "" {
		return nil
	}
	return &s
}

// Progress receives download progress: total is -1 when the size is unknown.
type Progress func(chunk int, total int64)

// maxDownloadSize limits untrusted downloads before signature verification.
const maxDownloadSize int64 = 2 << 30

// Download streams a release to a private file and authenticates it before
// transferring ownership to the caller. Every failure removes the partial file.
func (c *Client) Download(ctx context.Context, rel *Release, progress Progress) (*Payload, error) {
	signature, parsed, err := decodeSignature(rel.Signature)
	if err != nil {
		return nil, err
	}
	req, err := http.NewRequestWithContext(ctx, http.MethodGet, rel.URL, nil)
	if err != nil {
		return nil, err
	}
	resp, err := c.HTTP.Do(req)
	if err != nil {
		return nil, fmt.Errorf("download: %w", err)
	}
	defer resp.Body.Close()
	if resp.StatusCode != http.StatusOK {
		return nil, fmt.Errorf("download: http status %d", resp.StatusCode)
	}
	if resp.ContentLength > maxDownloadSize {
		return nil, errors.New("update exceeds maximum download size")
	}
	dir, err := os.MkdirTemp("", "uniclipboard-update-")
	if err != nil {
		return nil, err
	}
	file, err := os.OpenFile(filepath.Join(dir, "payload"), os.O_CREATE|os.O_EXCL|os.O_RDWR, 0o600)
	if err != nil {
		os.RemoveAll(dir)
		return nil, err
	}
	payload := &Payload{file: file, dir: dir}
	keep := false
	defer func() {
		if !keep {
			_ = payload.Close()
		}
	}()
	reader := minisign.NewReader(io.LimitReader(resp.Body, maxDownloadSize+1))
	buf := make([]byte, 64<<10)
	for {
		n, readErr := reader.Read(buf)
		if n > 0 {
			payload.Size += int64(n)
			if payload.Size > maxDownloadSize {
				return nil, errors.New("update exceeds maximum download size")
			}
			if _, err := file.Write(buf[:n]); err != nil {
				return nil, fmt.Errorf("write update: %w", err)
			}
			if progress != nil {
				progress(n, resp.ContentLength)
			}
		}
		if readErr == io.EOF {
			break
		}
		if readErr != nil {
			return nil, fmt.Errorf("download: %w", readErr)
		}
	}
	if err := ctx.Err(); err != nil {
		return nil, err
	}
	valid := false
	if parsed.Algorithm == minisign.HashEdDSA {
		valid = reader.Verify(c.PubKey, signature)
	} else {
		valid, err = verifyLegacyFile(file, payload.Size, c.PubKey, signature)
	}
	if err != nil {
		return nil, err
	}
	if !valid {
		return nil, tagged(FailureParse, errors.New("signature verification failed"))
	}
	keep = true
	return payload, nil
}

func decodeSignature(encoded string) ([]byte, minisign.Signature, error) {
	var parsed minisign.Signature
	signature, err := base64.StdEncoding.DecodeString(strings.TrimSpace(encoded))
	if err == nil {
		err = parsed.UnmarshalText(signature)
	}
	if err != nil {
		return nil, parsed, tagged(FailureParse, fmt.Errorf("invalid update signature: %w", err))
	}
	return signature, parsed, nil
}

// VerifyFile authenticates an existing file without loading it into the Go heap.
func (c *Client) VerifyFile(file *os.File, encoded string) error {
	signature, parsed, err := decodeSignature(encoded)
	if err != nil {
		return err
	}
	info, err := file.Stat()
	if err != nil {
		return err
	}
	if info.Size() > maxDownloadSize {
		return errors.New("update exceeds maximum download size")
	}
	valid := false
	if parsed.Algorithm == minisign.HashEdDSA {
		if _, err = file.Seek(0, io.SeekStart); err != nil {
			return err
		}
		reader := minisign.NewReader(file)
		if _, err = io.Copy(io.Discard, reader); err != nil {
			return err
		}
		valid = reader.Verify(c.PubKey, signature)
	} else {
		valid, err = verifyLegacyFile(file, info.Size(), c.PubKey, signature)
	}
	if err != nil {
		return err
	}
	if !valid {
		return tagged(FailureParse, errors.New("signature verification failed"))
	}
	return nil
}

// Verify checks data against a base64-encoded minisign signature.
func (c *Client) Verify(data []byte, encodedSignature string) error {
	signature, err := base64.StdEncoding.DecodeString(strings.TrimSpace(encodedSignature))
	if err != nil {
		return tagged(FailureParse, fmt.Errorf("signature is not valid base64: %w", err))
	}
	if !minisign.Verify(c.PubKey, data, signature) {
		return tagged(FailureParse, errors.New("signature verification failed"))
	}
	return nil
}

// NewHTTPClient builds the client used for feed and artifact requests.
func NewHTTPClient() *http.Client { return &http.Client{Timeout: 0} }

// FeedTimeout bounds one manifest request.
const FeedTimeout = 30 * time.Second
