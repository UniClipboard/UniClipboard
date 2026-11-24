package luascripts

import (
	"context"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"os"
	"path/filepath"
	"testing"
	"time"

	api "github.com/uniclipboard/unic/internal/localapi"
)

func TestFetchMarketplaceRemoteCacheAndFallback(t *testing.T) {
	dir := t.TempDir()
	luaDir := filepath.Join(dir, "lua")
	if err := os.MkdirAll(luaDir, 0700); err != nil {
		t.Fatal(err)
	}

	var hits int
	failRemote := false
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		hits++
		if failRemote {
			w.WriteHeader(http.StatusServiceUnavailable)
			return
		}
		cat := api.PluginMarketplace{
			Version:   1,
			UpdatedAt: "2026-11-22",
			Plugins: []api.MarketplaceEntry{
				{
					ID:            "unic.wenshushu",
					Name:          "文叔叔临时分享",
					Author:        "UniClipboard 官方",
					Official:      true,
					Category:      "share",
					Version:       "0.2.0",
					Summary:       "测试远端清单版本",
					ZeroConfig:    true,
					Repository:    "https://github.com/UniClipboard/unic-plugin-wenshushu",
					Ref:           "fc968e8efd8e4ac204194c21c2c08e34e9c26e7e",
					BuildFallback: true,
					Capabilities:  []string{"action"},
				},
			},
		}
		_ = json.NewEncoder(w).Encode(cat)
	}))
	defer srv.Close()

	t.Setenv("UNIC_PLUGIN_REGISTRY_URL", srv.URL)

	ctx := context.Background()
	first := FetchMarketplace(ctx, luaDir, false)
	if first.Source != "remote" || first.Warning != "" || len(first.Plugins) != 1 || first.Plugins[0].Version != "0.2.0" {
		t.Fatalf("unexpected remote fetch: %+v", first)
	}
	if hits != 1 {
		t.Fatalf("expected 1 remote hit, got %d", hits)
	}

	// Second call within TTL uses warm cache without hitting remote server.
	second := FetchMarketplace(ctx, luaDir, false)
	if second.Source != "cache" || second.Warning != "" || hits != 1 {
		t.Fatalf("expected cached catalog without network hit: %+v hits=%d", second, hits)
	}

	// Explicit refresh hits remote server; if remote fails, falls back to cache with warning.
	failRemote = true
	third := FetchMarketplace(ctx, luaDir, true)
	if third.Source != "cache" || third.Warning == "" || hits != 2 {
		t.Fatalf("expected cache fallback with warning on refresh failure: %+v hits=%d", third, hits)
	}

	// Removing cache forces bundled fallback with warning.
	_ = os.Remove(marketplaceCachePath(luaDir))
	fourth := FetchMarketplace(ctx, luaDir, false)
	if fourth.Source != "bundled" || fourth.Warning == "" || len(fourth.Plugins) == 0 || fourth.Plugins[0].ID != "unic.wenshushu" {
		t.Fatalf("expected bundled fallback: %+v", fourth)
	}

	// Explicit offline mode ("off") returns bundled without warning or network calls.
	t.Setenv("UNIC_PLUGIN_REGISTRY_URL", "off")
	fifth := FetchMarketplace(ctx, luaDir, true)
	if fifth.Source != "bundled" || fifth.Warning != "" {
		t.Fatalf("expected clean bundled mode when registry=off: %+v", fifth)
	}
}

func TestMarketplaceValidationRejectsInvalidEntries(t *testing.T) {
	valid := BundledMarketplace()
	if err := validateMarketplace(valid); err != nil {
		t.Fatalf("bundled catalog must be valid: %v", err)
	}
	badRepo := BundledMarketplace()
	badRepo.Plugins[0].Repository = "http://evil.example/repo"
	if err := validateMarketplace(badRepo); err == nil {
		t.Fatal("expected non-github repository to be rejected")
	}
	badCat := BundledMarketplace()
	badCat.Plugins[0].Category = "unknown"
	if err := validateMarketplace(badCat); err == nil {
		t.Fatal("expected unknown category to be rejected")
	}
	badID := BundledMarketplace()
	badID.Plugins[0].ID = "builtin.tailcat"
	if err := validateMarketplace(badID); err == nil {
		t.Fatal("expected reserved builtin ID to be rejected")
	}
	// Expired cache triggers refresh.
	dir := t.TempDir()
	cachePath := filepath.Join(dir, marketplaceCacheFile)
	old := marketplaceCacheRecord{FetchedAt: time.Now().Add(-time.Hour), Catalog: valid}
	if err := writeMarketplaceCache(cachePath, old); err != nil {
		t.Fatal(err)
	}
	got, err := readMarketplaceCache(cachePath)
	if err != nil || len(got.Catalog.Plugins) != len(valid.Plugins) {
		t.Fatalf("cache roundtrip failed: %v %+v", err, got)
	}
}
