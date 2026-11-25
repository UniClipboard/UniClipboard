package announcements

import (
	"context"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"
)

func TestFetchAnnouncementsWithActionURL(t *testing.T) {
	var gotQuery string
	body := `{"announcements":[` +
		`{"id":"v2-blog","severity":"info","title":"UniClipboard 2.0","content":"**New** release","actionUrl":"https://www.uniclipboard.app/blog/v2","requireAcknowledgement":false,"publishedAt":"2026-11-25T00:00:00Z","endsAt":null},` +
		`{"id":"maint-1","severity":"warning","title":"Maintenance","content":"Scheduled window","actionUrl":null,"requireAcknowledgement":true,"publishedAt":"2026-11-24T12:00:00Z","endsAt":"2026-11-26T00:00:00Z"}` +
		`]}`
	status := http.StatusOK
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		gotQuery = r.URL.RawQuery
		w.WriteHeader(status)
		_, _ = w.Write([]byte(body))
	}))
	defer srv.Close()

	items, err := Fetch(context.Background(), srv.Client(), srv.URL, "2.0.0-alpha.1", "alpha", "zh-CN")
	if err != nil {
		t.Fatalf("Fetch failed: %v", err)
	}
	if len(items) != 2 {
		t.Fatalf("len=%d want 2", len(items))
	}
	if items[0].ID != "v2-blog" || items[0].ActionURL != "https://www.uniclipboard.app/blog/v2" {
		t.Fatalf("unexpected item[0]: %+v", items[0])
	}
	if items[1].ID != "maint-1" || items[1].ActionURL != "" || !items[1].RequireAcknowledgement {
		t.Fatalf("unexpected item[1]: %+v", items[1])
	}
	for _, want := range []string{"product=desktop", "target_version=2.0.0-alpha.1", "channel=alpha", "locale=zh-CN"} {
		if !strings.Contains(gotQuery, want) {
			t.Fatalf("query %q missing %q", gotQuery, want)
		}
	}

	// Unsupported channel omits channel query param.
	if _, err := Fetch(context.Background(), srv.Client(), srv.URL, "2.0.0", "dev", "en"); err != nil {
		t.Fatalf("Fetch dev channel failed: %v", err)
	}
	if strings.Contains(gotQuery, "channel=") {
		t.Fatalf("dev channel should omit channel param: %q", gotQuery)
	}

	// Empty server list returns empty slice, no error.
	body = `{"announcements":[]}`
	items, err = Fetch(context.Background(), srv.Client(), srv.URL, "2.0.0", "stable", "en")
	if err != nil || len(items) != 0 {
		t.Fatalf("empty list: items=%v err=%v", items, err)
	}

	// Reject malformed payloads and unsafe actionUrl schemes.
	invalid := []string{
		`{`,
		`{}`,
		`{"announcements":null}`,
		`{"announcements":[{"id":"bad","severity":"other","title":"Bad","content":"","publishedAt":"2026-11-25T00:00:00Z"}]}`,
		`{"announcements":[{"id":"bad-url","severity":"info","title":"Bad","content":"","actionUrl":"javascript:alert(1)","publishedAt":"2026-11-25T00:00:00Z"}]}`,
		`{"announcements":[{"id":"http-url","severity":"info","title":"Bad","content":"","actionUrl":"http://example.com","publishedAt":"2026-11-25T00:00:00Z"}]}`,
		`{"announcements":[{"id":"cred-url","severity":"info","title":"Bad","content":"","actionUrl":"https://user:pass@example.com","publishedAt":"2026-11-25T00:00:00Z"}]}`,
	}
	for _, raw := range invalid {
		body = raw
		if _, err := Fetch(context.Background(), srv.Client(), srv.URL, "2.0.0", "stable", "en"); err == nil {
			t.Fatalf("expected error for payload %s", raw)
		}
	}

	// Non-200 status returns error.
	status = http.StatusServiceUnavailable
	body = `{"error":"unavailable"}`
	if _, err := Fetch(context.Background(), srv.Client(), srv.URL, "2.0.0", "stable", "en"); err == nil {
		t.Fatal("expected error for HTTP 503")
	}

	// Reject non-HTTPS non-loopback endpoint.
	if _, err := Fetch(context.Background(), srv.Client(), "http://release.example.com/announcements", "2.0.0", "stable", "en"); err == nil {
		t.Fatal("expected error for non-HTTPS remote endpoint")
	}
}
