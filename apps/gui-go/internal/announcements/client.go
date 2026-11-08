// Package announcements reads public release announcements without persisting their content.
package announcements

import (
	"context"
	"encoding/json"
	"fmt"
	"io"
	"net/http"
	"net/url"
	"strings"
	"time"
)

const Endpoint = "https://release.uniclipboard.app/announcements"
const maxResponseBytes = 1 << 20

type Announcement struct {
	ID                     string  `json:"id"`
	Severity               string  `json:"severity"`
	Title                  string  `json:"title"`
	Content                string  `json:"content"`
	RequireAcknowledgement bool    `json:"requireAcknowledgement"`
	PublishedAt            string  `json:"publishedAt"`
	EndsAt                 *string `json:"endsAt"`
}

// Fetch leaves targeting, scheduling and withdrawal decisions to FlareRelease.
func Fetch(ctx context.Context, client *http.Client, endpoint, version, channel, locale string) ([]Announcement, error) {
	u, err := url.Parse(endpoint)
	if err != nil {
		return nil, fmt.Errorf("invalid announcements endpoint")
	}
	q := u.Query()
	q.Set("product", "desktop")
	q.Set("target_version", version)
	q.Set("locale", locale)
	// The service currently exposes only stable and alpha Desktop channels.
	if channel == "stable" || channel == "alpha" {
		q.Set("channel", channel)
	}
	u.RawQuery = q.Encode()
	req, err := http.NewRequestWithContext(ctx, http.MethodGet, u.String(), nil)
	if err != nil {
		return nil, fmt.Errorf("create announcements request: %w", err)
	}
	req.Header.Set("Accept", "application/json")
	response, err := client.Do(req)
	if err != nil {
		return nil, fmt.Errorf("fetch announcements: %w", err)
	}
	defer response.Body.Close()
	if response.StatusCode != http.StatusOK {
		return nil, fmt.Errorf("announcements returned HTTP %d", response.StatusCode)
	}
	body, err := io.ReadAll(io.LimitReader(response.Body, maxResponseBytes+1))
	if err != nil || len(body) > maxResponseBytes {
		return nil, fmt.Errorf("cannot read announcements response")
	}
	var result struct {
		Announcements *[]Announcement `json:"announcements"`
	}
	if err := json.Unmarshal(body, &result); err != nil || result.Announcements == nil {
		return nil, fmt.Errorf("invalid announcements response")
	}
	for _, item := range *result.Announcements {
		if item.ID == "" || strings.TrimSpace(item.Title) == "" || (item.Severity != "info" && item.Severity != "warning" && item.Severity != "critical") {
			return nil, fmt.Errorf("invalid announcement fields")
		}
		if _, err := time.Parse(time.RFC3339, item.PublishedAt); err != nil {
			return nil, fmt.Errorf("invalid announcement publication time")
		}
		if item.EndsAt != nil {
			if _, err := time.Parse(time.RFC3339, *item.EndsAt); err != nil {
				return nil, fmt.Errorf("invalid announcement expiry time")
			}
		}
	}
	return *result.Announcements, nil
}
