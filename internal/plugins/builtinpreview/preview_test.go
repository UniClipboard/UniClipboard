package builtinpreview

import (
	"bytes"
	"context"
	"encoding/json"
	"image"
	"image/color"
	"image/png"
	"io"
	"net/http"
	"net/http/httptest"
	"net/netip"
	"sync/atomic"
	"testing"

	"github.com/uniclipboard/unic/plugin"
)

type fakeEnrichCtx struct {
	trigger string
	item    plugin.Item
	res     plugin.EnrichmentResult
	set     bool
}

func (f *fakeEnrichCtx) Trigger() string                                      { return f.trigger }
func (f *fakeEnrichCtx) Item() plugin.Item                                    { return f.item }
func (f *fakeEnrichCtx) Parts() []plugin.ActionPart                           { return nil }
func (f *fakeEnrichCtx) OpenPart(context.Context, int) (io.ReadCloser, error) { return nil, nil }
func (f *fakeEnrichCtx) SetResult(r plugin.EnrichmentResult) error {
	f.res, f.set = r, true
	return nil
}

func syntheticPNG(w, h int, c color.RGBA) []byte {
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

func TestSettingsValidationAndDefaults(t *testing.T) {
	var def Settings
	if err := def.Validate(); err != nil {
		t.Fatalf("default settings invalid: %v", err)
	}
	if !def.LinkPreviewEnabled() || !def.FetchFaviconEnabled() || !def.FetchThumbnailEnabled() {
		t.Fatal("expected all default preview flags enabled")
	}
	if err := (Settings{RequestTimeoutSec: 1}).Validate(); err == nil {
		t.Fatal("expected timeout < 2 to fail")
	}
	if err := (Settings{RequestTimeoutSec: 16}).Validate(); err == nil {
		t.Fatal("expected timeout > 15 to fail")
	}
	if err := (Settings{ExcludedDomains: []string{"bad domain.com"}}).Validate(); err == nil {
		t.Fatal("expected whitespace in excluded domain to fail")
	}
}

func TestSmartPreviewLinkEnrichAndToggle(t *testing.T) {
	fav := syntheticPNG(96, 96, color.RGBA{R: 30, G: 90, B: 220, A: 255})
	og := syntheticPNG(1200, 630, color.RGBA{R: 20, G: 160, B: 110, A: 255})
	var hits atomic.Int32
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		hits.Add(1)
		switch r.URL.Path {
		case "/doc":
			w.Header().Set("Content-Type", "text/html; charset=utf-8")
			_, _ = w.Write([]byte(`<html><head>
				<meta property="og:site_name" content="Unic Smart Preview">
				<meta property="og:title" content="智能预览内置插件">
				<meta property="og:description" content="默认启用并可随时关闭。">
				<meta property="og:image" content="/og.png">
				<link rel="icon" href="/fav.png">
			</head><body>ok</body></html>`))
		case "/fav.png":
			w.Header().Set("Content-Type", "image/png")
			_, _ = w.Write(fav)
		case "/og.png":
			w.Header().Set("Content-Type", "image/png")
			_, _ = w.Write(og)
		default:
			http.NotFound(w, r)
		}
	}))
	defer srv.Close()

	// Production SSRF blocks loopback by default.
	prod, err := New(Settings{})
	if err != nil {
		t.Fatal(err)
	}
	if prod.Spec().ID != ID || prod.Spec().Enricher == nil {
		t.Fatalf("unexpected spec: %+v", prod.Spec())
	}
	fcBlocked := &fakeEnrichCtx{trigger: plugin.EnrichTriggerOnCommit, item: plugin.Item{Type: "text", Text: srv.URL + "/doc"}}
	if err := prod.Enrich(context.Background(), fcBlocked); err == nil {
		t.Fatal("expected SSRF policy to block loopback in production mode")
	}
	if !isAllowedAddr(netip.MustParseAddr("93.184.216.34"), false) || isAllowedAddr(netip.MustParseAddr("127.0.0.1"), false) {
		t.Fatal("unexpected IP allowlist behavior")
	}

	// Loopback enabled for test verifies full enrichment + scaled assets.
	p, err := NewForTest(Settings{}, true)
	if err != nil {
		t.Fatal(err)
	}
	fc := &fakeEnrichCtx{trigger: plugin.EnrichTriggerOnCommit, item: plugin.Item{Type: "text", Text: srv.URL + "/doc"}}
	if err := p.Enrich(context.Background(), fc); err != nil {
		t.Fatalf("Enrich: %v", err)
	}
	if !fc.set || fc.res.Status != plugin.EnrichStatusOK || len(fc.res.Annotations) != 1 {
		t.Fatalf("unexpected enrich result: %+v", fc.res)
	}
	var lp plugin.LinkPreviewV1
	if err := json.Unmarshal(fc.res.Annotations[0].PayloadJSON, &lp); err != nil {
		t.Fatal(err)
	}
	if lp.Title != "智能预览内置插件" || lp.FaviconAsset != "favicon" || lp.ImageAsset != "thumbnail" {
		t.Fatalf("unexpected payload: %+v", lp)
	}
	for _, a := range fc.res.Annotations[0].Assets {
		cfg, _, err := image.DecodeConfig(bytes.NewReader(a.Data))
		if err != nil {
			t.Fatalf("asset %s invalid: %v", a.Key, err)
		}
		if a.Key == "thumbnail" && (cfg.Width > maxThumbEdge || cfg.Height > maxThumbEdge) {
			t.Fatalf("thumbnail not resized: %dx%d", cfg.Width, cfg.Height)
		}
	}

	// Disabling link_preview makes zero network requests and returns skipped.
	hits.Store(0)
	off := false
	pOff, err := NewForTest(Settings{LinkPreview: &off}, true)
	if err != nil {
		t.Fatal(err)
	}
	fcOff := &fakeEnrichCtx{trigger: plugin.EnrichTriggerOnCommit, item: plugin.Item{Type: "text", Text: srv.URL + "/doc"}}
	if err := pOff.Enrich(context.Background(), fcOff); err != nil {
		t.Fatalf("Enrich disabled: %v", err)
	}
	if fcOff.res.Status != plugin.EnrichStatusSkipped || hits.Load() != 0 {
		t.Fatalf("expected skipped + 0 hits, got status=%s hits=%d", fcOff.res.Status, hits.Load())
	}
}
