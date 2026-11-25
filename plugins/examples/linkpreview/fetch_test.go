package main

import (
	"bytes"
	"context"
	"image"
	"image/color"
	"image/png"
	"net/http"
	"net/http/httptest"
	"net/netip"
	"strings"
	"testing"
)

func makePNG(w, h int, c color.RGBA) []byte {
	img := image.NewRGBA(image.Rect(0, 0, w, h))
	for y := 0; y < h; y++ {
		for x := 0; x < w; x++ {
			img.SetRGBA(x, y, c)
		}
	}
	var buf bytes.Buffer
	_ = png.Encode(&buf, img)
	return buf.Bytes()
}

func TestExtractStandaloneURL(t *testing.T) {
	if _, ok := extractStandaloneURL("https://example.com/a?b=1"); !ok {
		t.Fatal("expected valid standalone URL")
	}
	for _, bad := range []string{
		"",
		"check out https://example.com today",
		"https://example.com\nsecond line",
		"ftp://example.com/file",
		"https://user:pass@example.com/secret",
		"javascript:alert(1)",
	} {
		if _, ok := extractStandaloneURL(bad); ok {
			t.Fatalf("expected %q to be rejected", bad)
		}
	}
}

func TestSSRFGuards(t *testing.T) {
	blocked := []string{
		"127.0.0.1",
		"10.0.0.1",
		"172.16.5.4",
		"192.168.1.20",
		"169.254.169.254",
		"100.64.0.1",
		"::1",
		"fc00::1",
		"fe80::1",
		"0.0.0.0",
	}
	for _, ip := range blocked {
		if isAllowedAddr(netip.MustParseAddr(ip), false) {
			t.Fatalf("expected %s to be blocked", ip)
		}
	}
	if !isAllowedAddr(netip.MustParseAddr("93.184.216.34"), false) {
		t.Fatal("expected public IPv4 to be allowed")
	}
	if !isBlockedHostname("localhost", false) || !isBlockedHostname("nas.local", false) || !isBlockedHostname("db.internal", false) {
		t.Fatal("expected local/internal hostnames to be blocked")
	}
}

func TestFetchLinkPreviewWithOGImageAndFavicon(t *testing.T) {
	fav := makePNG(128, 128, color.RGBA{R: 20, G: 100, B: 240, A: 255})
	og := makePNG(1200, 630, color.RGBA{R: 30, G: 180, B: 120, A: 255})
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		switch r.URL.Path {
		case "/post":
			w.Header().Set("Content-Type", "text/html; charset=utf-8")
			_, _ = w.Write([]byte(`<!doctype html><html><head>
				<title>Fallback Title</title>
				<meta property="og:site_name" content="UniClipboard Docs">
				<meta property="og:title" content="异步历史富化设计指南">
				<meta property="og:description" content="了解 unic 如何通过 enricher 能力安全扩展链接预览与 OCR。">
				<meta property="og:image" content="/assets/cover.png">
				<link rel="icon" href="/assets/icon.png">
			</head><body><p>Hello</p></body></html>`))
		case "/assets/icon.png":
			w.Header().Set("Content-Type", "image/png")
			_, _ = w.Write(fav)
		case "/assets/cover.png":
			w.Header().Set("Content-Type", "image/png")
			_, _ = w.Write(og)
		default:
			http.NotFound(w, r)
		}
	}))
	defer srv.Close()

	// Without allowLoopback, localhost must be blocked.
	if _, _, err := fetchLinkPreview(context.Background(), srv.URL+"/post", defaultConfig(), false); err == nil {
		t.Fatal("expected SSRF policy to block loopback target by default")
	}

	preview, assets, err := fetchLinkPreview(context.Background(), srv.URL+"/post", defaultConfig(), true)
	if err != nil {
		t.Fatalf("fetchLinkPreview: %v", err)
	}
	if preview.Title != "异步历史富化设计指南" || preview.SiteName != "UniClipboard Docs" {
		t.Fatalf("unexpected preview: %+v", preview)
	}
	if !strings.Contains(preview.Description, "enricher") {
		t.Fatalf("unexpected description: %q", preview.Description)
	}
	if preview.FaviconAsset != "favicon" || preview.ImageAsset != "thumbnail" {
		t.Fatalf("expected both favicon and thumbnail assets, got %+v", preview)
	}
	if len(assets) != 2 {
		t.Fatalf("expected 2 assets, got %d", len(assets))
	}
	for _, a := range assets {
		cfg, _, err := image.DecodeConfig(bytes.NewReader(a.Data))
		if err != nil {
			t.Fatalf("asset %s invalid image: %v", a.Key, err)
		}
		if a.Key == "favicon" && (cfg.Width > maxFaviconEdge || cfg.Height > maxFaviconEdge || len(a.Data) > maxFaviconOutB) {
			t.Fatalf("favicon exceeded bounds: %dx%d %d bytes", cfg.Width, cfg.Height, len(a.Data))
		}
		if a.Key == "thumbnail" && (cfg.Width > maxThumbEdge || cfg.Height > maxThumbEdge || len(a.Data) > maxThumbOutB) {
			t.Fatalf("thumbnail exceeded bounds: %dx%d %d bytes", cfg.Width, cfg.Height, len(a.Data))
		}
	}
}

func TestFetchLinkPreviewCorruptImageFallsBackGracefully(t *testing.T) {
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		switch r.URL.Path {
		case "/page":
			w.Header().Set("Content-Type", "text/html")
			_, _ = w.Write([]byte(`<html><head>
				<meta property="og:title" content="Good Title">
				<meta property="og:image" content="/bad.png">
			</head></html>`))
		case "/bad.png":
			w.Header().Set("Content-Type", "image/png")
			_, _ = w.Write([]byte("not-a-real-png"))
		default:
			http.NotFound(w, r)
		}
	}))
	defer srv.Close()

	preview, assets, err := fetchLinkPreview(context.Background(), srv.URL+"/page", defaultConfig(), true)
	if err != nil {
		t.Fatalf("expected text metadata to succeed despite bad og:image: %v", err)
	}
	if preview.Title != "Good Title" || preview.ImageAsset != "" || len(assets) != 0 {
		t.Fatalf("unexpected fallback result: preview=%+v assets=%d", preview, len(assets))
	}
}

func TestExcludedDomains(t *testing.T) {
	rules := []string{"corp.example.com", "*.internal.net"}
	if !isExcludedDomain("corp.example.com", rules) || !isExcludedDomain("git.internal.net", rules) || !isExcludedDomain("internal.net", rules) {
		t.Fatal("expected domains to be excluded")
	}
	if isExcludedDomain("public.example.com", rules) {
		t.Fatal("did not expect public.example.com to be excluded")
	}
}
