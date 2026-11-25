// Package announcements reads public release announcements from FlareRelease.
package announcements

import (
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

	"github.com/uniclipboard/unic/internal/releaseinfo"
)

const (
	// Endpoint is the public FlareRelease announcements feed.
	Endpoint = "https://release.uniclipboard.app/announcements"

	fetchTimeout     = 8 * time.Second
	maxResponseBytes = 1 << 20
	maxActionURLLen  = 2048
)

// Announcement is one public release announcement from FlareRelease.
type Announcement struct {
	ID                     string  `json:"id"`
	Severity               string  `json:"severity"`
	Title                  string  `json:"title"`
	Content                string  `json:"content"`
	ActionURL              string  `json:"actionUrl,omitempty"`
	RequireAcknowledgement bool    `json:"requireAcknowledgement"`
	PublishedAt            string  `json:"publishedAt"`
	EndsAt                 *string `json:"endsAt"`
}

// DefaultEndpoint returns UNIC_ANNOUNCEMENTS_URL when set, otherwise Endpoint.
func DefaultEndpoint() string {
	if custom := strings.TrimSpace(os.Getenv("UNIC_ANNOUNCEMENTS_URL")); custom != "" {
		return custom
	}
	return Endpoint
}

// DefaultClient returns a bounded HTTP client enforcing safe redirect targets.
func DefaultClient() *http.Client {
	return &http.Client{
		Timeout: fetchTimeout,
		CheckRedirect: func(req *http.Request, via []*http.Request) error {
			if len(via) > 5 || !allowedFeedURL(req.URL.String()) {
				return errors.New("unsafe announcements redirect")
			}
			return nil
		},
	}
}

func allowedFeedURL(raw string) bool {
	u, err := url.Parse(strings.TrimSpace(raw))
	if err != nil || u.User != nil || u.Host == "" {
		return false
	}
	if u.Scheme == "https" {
		return true
	}
	if u.Scheme == "http" {
		host := u.Hostname()
		return host == "127.0.0.1" || host == "::1" || host == "localhost"
	}
	return false
}

func allowedActionURL(raw string) bool {
	trimmed := strings.TrimSpace(raw)
	if len(trimmed) > maxActionURLLen {
		return false
	}
	u, err := url.Parse(trimmed)
	if err != nil || u.User != nil || u.Host == "" {
		return false
	}
	return u.Scheme == "https"
}

// Fetch reads active release announcements for the installed Desktop version,
// channel and UI locale. Targeting, scheduling and withdrawal stay server-owned.
func Fetch(ctx context.Context, client *http.Client, endpoint, version, channel, locale string) ([]Announcement, error) {
	if endpoint == "" {
		endpoint = DefaultEndpoint()
	}
	if !allowedFeedURL(endpoint) {
		return nil, errors.New("invalid announcements endpoint")
	}
	if client == nil {
		client = DefaultClient()
	}
	u, err := url.Parse(endpoint)
	if err != nil {
		return nil, fmt.Errorf("invalid announcements endpoint: %w", err)
	}
	q := u.Query()
	q.Set("product", "desktop")
	if v := strings.TrimSpace(version); v != "" {
		q.Set("target_version", v)
	}
	if l := strings.TrimSpace(locale); l != "" {
		q.Set("locale", l)
	}
	if channel == "stable" || channel == "alpha" {
		q.Set("channel", channel)
	}
	u.RawQuery = q.Encode()

	reqCtx, cancel := context.WithTimeout(ctx, fetchTimeout)
	defer cancel()
	req, err := http.NewRequestWithContext(reqCtx, http.MethodGet, u.String(), nil)
	if err != nil {
		return nil, fmt.Errorf("create announcements request: %w", err)
	}
	req.Header.Set("Accept", "application/json")
	req.Header.Set("User-Agent", "UniClipboard/"+releaseinfo.Version)

	resp, err := client.Do(req)
	if err != nil {
		return nil, fmt.Errorf("fetch announcements: %w", err)
	}
	defer func() { _ = resp.Body.Close() }()
	if resp.StatusCode != http.StatusOK {
		return nil, fmt.Errorf("announcements returned HTTP %d", resp.StatusCode)
	}
	body, err := io.ReadAll(io.LimitReader(resp.Body, maxResponseBytes+1))
	if err != nil || len(body) > maxResponseBytes {
		return nil, errors.New("cannot read announcements response")
	}
	return parseResponse(body)
}

func parseResponse(body []byte) ([]Announcement, error) {
	var result struct {
		Announcements *[]Announcement `json:"announcements"`
	}
	if err := json.Unmarshal(body, &result); err != nil || result.Announcements == nil {
		return nil, errors.New("invalid announcements response")
	}
	items := make([]Announcement, 0, len(*result.Announcements))
	for _, item := range *result.Announcements {
		item.ID = strings.TrimSpace(item.ID)
		item.Title = strings.TrimSpace(item.Title)
		item.ActionURL = strings.TrimSpace(item.ActionURL)
		if item.ID == "" || item.Title == "" || (item.Severity != "info" && item.Severity != "warning" && item.Severity != "critical") {
			return nil, errors.New("invalid announcement fields")
		}
		if _, err := time.Parse(time.RFC3339, item.PublishedAt); err != nil {
			return nil, errors.New("invalid announcement publication time")
		}
		if item.EndsAt != nil {
			if _, err := time.Parse(time.RFC3339, *item.EndsAt); err != nil {
				return nil, errors.New("invalid announcement expiry time")
			}
		}
		if item.ActionURL != "" && !allowedActionURL(item.ActionURL) {
			return nil, errors.New("invalid announcement actionUrl")
		}
		items = append(items, item)
	}
	return items, nil
}
