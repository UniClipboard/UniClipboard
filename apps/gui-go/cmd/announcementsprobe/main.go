// Command announcementsprobe verifies the public announcement HTTP contract with synthetic data.
package main

import (
	"context"
	"encoding/json"
	"fmt"
	"net/http"
	"net/http/httptest"
	"os"
	"strings"
	"time"

	"github.com/UniClipboard/UniClipboard/apps/gui-go/internal/announcements"
)

func main() {
	checks := []string{}
	check := func(name string, ok bool) {
		if !ok {
			panic(name)
		}
		checks = append(checks, name)
	}
	body := `{"announcements":[{"id":"notice-1","severity":"critical","title":"Maintenance","content":"**Read this**","requireAcknowledgement":true,"publishedAt":"2026-11-08T00:00:00Z","endsAt":null}]}`
	status := http.StatusOK
	var query string
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		query = r.URL.RawQuery
		w.Header().Set("Content-Type", "application/json")
		w.WriteHeader(status)
		fmt.Fprint(w, body)
	}))
	client := server.Client()
	client.Timeout = time.Second
	fetch := func(channel, locale string) ([]announcements.Announcement, error) {
		return announcements.Fetch(context.Background(), client, server.URL+"/announcements", "1.6.1-alpha.2", channel, locale)
	}
	items, err := fetch("alpha", "zh-CN")
	check("HTTP feed returns full announcement metadata", err == nil && len(items) == 1 && items[0].RequireAcknowledgement && items[0].Severity == "critical" && items[0].Content == "**Read this**")
	check("query binds installed version, Desktop, channel and language", strings.Contains(query, "target_version=1.6.1-alpha.2") && strings.Contains(query, "product=desktop") && strings.Contains(query, "channel=alpha") && strings.Contains(query, "locale=zh-CN"))
	_, err = fetch("rc", "en-US")
	check("unsupported channel still requests product-wide notices", err == nil && !strings.Contains(query, "channel="))
	body = `{"announcements":[]}`
	items, err = fetch("stable", "en-US")
	check("refresh replaces previous notices with empty server state", err == nil && items != nil && len(items) == 0)
	for _, invalid := range []string{`{`, `{}`, `{"announcements":null}`, `{"announcements":[{"id":"bad","title":"bad","severity":"other"}]}`, strings.Repeat("x", (1<<20)+1)} {
		body = invalid
		_, err = fetch("stable", "en-US")
		check(fmt.Sprintf("reject invalid response %d", len(checks)), err != nil)
	}
	status = http.StatusServiceUnavailable
	body = `{"error":"unavailable"}`
	_, err = fetch("stable", "en-US")
	check("HTTP failure stays distinguishable from an empty list", err != nil)
	ctx, cancel := context.WithCancel(context.Background())
	cancel()
	_, err = announcements.Fetch(ctx, client, server.URL, "1.6.1", "stable", "en")
	check("request cancellation propagates", err != nil)
	server.Close()
	_, err = fetch("stable", "en-US")
	check("offline request fails", err != nil)
	result := map[string]any{"passed": true, "scope": "real HTTP transport and production announcement client with synthetic local feed; no GUI or hosted-service acceptance", "checks": checks, "cleanup": "HTTP server closed"}
	encoder := json.NewEncoder(os.Stdout)
	encoder.SetIndent("", "  ")
	if err := encoder.Encode(result); err != nil {
		panic(err)
	}
}
