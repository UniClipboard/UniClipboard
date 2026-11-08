package host

import (
	"context"
	"net/http"
	"os"
	"path/filepath"
	"testing"

	"github.com/uniclipboard/unic/app/desktop/internal/api"
	"github.com/uniclipboard/unic/app/desktop/internal/config"
)

// Explicit opt-in. Exercise the same socket client and preview endpoint as the
// GUI without starting networking, clipboard observation, or applying content.
func TestLocalMigratedHistoryVisible(t *testing.T) {
	target := os.Getenv("UNIC_V1_TEST_TARGET")
	if target == "" {
		t.Skip("set UNIC_V1_TEST_TARGET to verify a migrated local directory")
	}
	ctx := context.Background()
	cfg := config.Static(config.Default())
	n, err := Open(ctx, Options{Dir: target, Settings: cfg})
	if err != nil {
		t.Fatal(err)
	}
	defer func() { _ = n.Close(ctx) }()
	dir, err := os.MkdirTemp("", "unic-api-")
	if err != nil {
		t.Fatal(err)
	}
	defer func() { _ = os.RemoveAll(dir) }()
	// macOS has short UNIX socket limits; use a short name in /tmp instead.
	if len(dir) > 70 {
		_ = os.RemoveAll(dir)
		dir, err = os.MkdirTemp("/tmp", "unic-api-")
		if err != nil {
			t.Fatal(err)
		}
	}
	socket := filepath.Join(dir, "s")
	ln, err := api.Listen(socket)
	if err != nil {
		t.Fatal(err)
	}
	server := &http.Server{Handler: api.Handler(n, cfg)}
	go func() { _ = server.Serve(ln) }()
	defer func() { _ = server.Close() }()
	client := api.NewClient(socket)
	seen, kinds, previewed := map[string]bool{}, map[string]int{}, map[string]bool{}
	cursor := ""
	for {
		page, err := client.ListHistory(ctx, cursor, "")
		if err != nil {
			t.Fatal("history socket request failed")
		}
		for _, item := range page.Items {
			if seen[item.ID] {
				t.Fatal("history pagination repeated a record")
			}
			seen[item.ID] = true
			if item.Origin != "migration" {
				continue
			}
			kinds[item.Kind]++
			if !previewed[item.Kind] {
				if _, err := client.HistoryPreview(ctx, item); err != nil {
					t.Fatal("migrated history preview unavailable")
				}
				previewed[item.Kind] = true
			}
		}
		cursor = page.NextCursor
		if cursor == "" {
			break
		}
	}
	if len(kinds) == 0 {
		t.Fatal("no migrated history visible")
	}
	t.Logf("GUI socket API: visible=%d migrated_kinds=%v", len(seen), kinds)
}
