package builtinpreview_test

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
	"path/filepath"
	"strings"
	"sync/atomic"
	"testing"
	"time"

	"github.com/uniclipboard/unic/engine"
	"github.com/uniclipboard/unic/internal/plugins/builtinclassify"
	"github.com/uniclipboard/unic/internal/plugins/builtinpreview"
	"github.com/uniclipboard/unic/plugin"
)

func canonicalTempDir(t *testing.T) string {
	t.Helper()
	dir, err := filepath.EvalSymlinks(t.TempDir())
	if err != nil {
		t.Fatal(err)
	}
	return dir
}

func makeTestPNG(t *testing.T, w, h int, c color.RGBA) []byte {
	t.Helper()
	img := image.NewRGBA(image.Rect(0, 0, w, h))
	for y := 0; y < h; y++ {
		for x := 0; x < w; x++ {
			img.SetRGBA(x, y, c)
		}
	}
	var buf bytes.Buffer
	if err := png.Encode(&buf, img); err != nil {
		t.Fatal(err)
	}
	return buf.Bytes()
}

func startTestLinkServer(t *testing.T) (*httptest.Server, *atomic.Int32) {
	t.Helper()
	var hits atomic.Int32
	og := makeTestPNG(t, 800, 420, color.RGBA{R: 20, G: 140, B: 120, A: 255})
	fav := makeTestPNG(t, 32, 32, color.RGBA{R: 30, G: 90, B: 220, A: 255})
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		hits.Add(1)
		switch r.URL.Path {
		case "/article":
			w.Header().Set("Content-Type", "text/html; charset=utf-8")
			_, _ = io.WriteString(w, `<!doctype html><html><head>
				<title>Smart Preview Engine Title</title>
				<meta property="og:site_name" content="Unic Smart Docs">
				<meta property="og:description" content="Built-in Smart Preview enriches committed links asynchronously.">
				<meta property="og:image" content="/og.png">
				<link rel="icon" href="/favicon.png">
			</head><body>ok</body></html>`)
		case "/og.png":
			w.Header().Set("Content-Type", "image/png")
			_, _ = w.Write(og)
		case "/favicon.png":
			w.Header().Set("Content-Type", "image/png")
			_, _ = w.Write(fav)
		default:
			http.NotFound(w, r)
		}
	}))
	t.Cleanup(srv.Close)
	return srv, &hits
}

func commitLinkText(t *testing.T, e *engine.Engine, reqID, urlStr string) engine.HistoryID {
	t.Helper()
	ctx := context.Background()
	h, err := e.ImportContent(ctx, engine.ContentInput{
		Representations: []engine.RepresentationInput{{
			MIME:   engine.MimeText,
			Reader: strings.NewReader(urlStr),
			Size:   int64(len(urlStr)),
		}},
	})
	if err != nil {
		t.Fatalf("ImportContent: %v", err)
	}
	res, err := e.CommitContent(ctx, engine.CommitRequest{
		RequestID: engine.RequestID(reqID),
		LeaseID:   h.LeaseID,
	})
	_ = e.ReleaseContent(ctx, h.LeaseID)
	if err != nil {
		t.Fatalf("CommitContent: %v", err)
	}
	return res.HistoryID
}

func waitForAnnotations(t *testing.T, e *engine.Engine, id engine.HistoryID, wantCount int) []engine.HistoryAnnotation {
	t.Helper()
	deadline := time.Now().Add(8 * time.Second)
	for time.Now().Before(deadline) {
		anns, err := e.GetHistoryAnnotations(context.Background(), id)
		if err != nil {
			t.Fatalf("GetHistoryAnnotations: %v", err)
		}
		if len(anns) >= wantCount {
			return anns
		}
		time.Sleep(40 * time.Millisecond)
	}
	t.Fatalf("timed out waiting for %d annotations on %s", wantCount, id)
	return nil
}

func TestBuiltinSmartPreviewEngineLifecycleEnabledAndDisabled(t *testing.T) {
	ctx := context.Background()
	srv, hits := startTestLinkServer(t)

	// 1. Enabled Smart Preview: enriches committed link and populates search + OG/favicon assets.
	classifier, err := builtinclassify.New(nil, nil)
	if err != nil {
		t.Fatal(err)
	}
	previewer, err := builtinpreview.NewForTest(builtinpreview.Settings{}, true)
	if err != nil {
		t.Fatal(err)
	}
	e, err := engine.New(ctx, engine.Options{
		DataDir: canonicalTempDir(t),
		Plugins: []engine.PluginInstance{
			{Plugin: classifier},
			{Plugin: previewer, Builtin: true},
		},
	})
	if err != nil {
		t.Fatalf("engine.New: %v", err)
	}
	defer func() { _ = e.Close(context.Background()) }()
	if err := e.Start(ctx); err != nil {
		t.Fatalf("engine.Start: %v", err)
	}

	histID := commitLinkText(t, e, "req-smart-1", srv.URL+"/article")
	anns := waitForAnnotations(t, e, histID, 1)
	if anns[0].PluginID != builtinpreview.ID || anns[0].Type != plugin.AnnotationLinkPreviewV1 {
		t.Fatalf("unexpected annotation: %+v", anns[0])
	}
	var payload plugin.LinkPreviewV1
	if err := json.Unmarshal(anns[0].Payload, &payload); err != nil {
		t.Fatal(err)
	}
	if payload.Title != "Smart Preview Engine Title" || payload.ImageAsset != "thumbnail" || payload.FaviconAsset != "favicon" {
		t.Fatalf("unexpected payload: %+v", payload)
	}
	rc, meta, err := e.OpenHistoryAnnotationAsset(ctx, histID, builtinpreview.ID, plugin.AnnotationLinkPreviewV1, "thumbnail")
	if err != nil {
		t.Fatalf("OpenHistoryAnnotationAsset(thumbnail): %v", err)
	}
	assetBytes, _ := io.ReadAll(rc)
	_ = rc.Close()
	if !strings.HasPrefix(meta.MIME, "image/") || int64(len(assetBytes)) != meta.Size || len(assetBytes) == 0 {
		t.Fatalf("unexpected thumbnail asset meta=%+v len=%d", meta, len(assetBytes))
	}

	searchRes, err := e.SearchHistory(ctx, engine.HistorySearchRequest{
		Query: engine.HistorySearchQuery{TextTerms: []string{"Smart Preview Engine Title"}},
	})
	if err != nil {
		t.Fatalf("SearchHistory: %v", err)
	}
	if len(searchRes.Hits) != 1 || searchRes.Hits[0].Item.ID != histID || searchRes.Hits[0].Preview.MatchKind != "link_title" {
		t.Fatalf("expected derived link_title search hit for %s, got %+v", histID, searchRes.Hits)
	}

	// 2. Disabled Smart Preview: zero HTTP requests and ordinary history still works cleanly.
	hitsBefore := hits.Load()
	classifier2, _ := builtinclassify.New(nil, nil)
	previewer2, _ := builtinpreview.NewForTest(builtinpreview.Settings{}, true)
	e2, err := engine.New(ctx, engine.Options{
		DataDir: canonicalTempDir(t),
		Plugins: []engine.PluginInstance{
			{Plugin: classifier2},
			{Plugin: previewer2, Builtin: true, Disabled: true},
		},
	})
	if err != nil {
		t.Fatalf("engine.New disabled: %v", err)
	}
	defer func() { _ = e2.Close(context.Background()) }()
	if err := e2.Start(ctx); err != nil {
		t.Fatalf("engine.Start disabled: %v", err)
	}
	histID2 := commitLinkText(t, e2, "req-smart-2", srv.URL+"/article")
	time.Sleep(150 * time.Millisecond)
	if hits.Load() != hitsBefore {
		t.Fatalf("disabled Smart Preview made %d unexpected HTTP requests", hits.Load()-hitsBefore)
	}
	if anns2, err := e2.GetHistoryAnnotations(ctx, histID2); err != nil || len(anns2) != 0 {
		t.Fatalf("expected no annotations when disabled, got %+v err=%v", anns2, err)
	}
}

type customPreviewOverride struct{}

func (customPreviewOverride) Spec() plugin.Spec {
	return plugin.Spec{
		ID:           "ext.custompreview",
		Version:      "0.1.0",
		APIVersion:   1,
		Capabilities: []string{plugin.CapEnricher},
		Enricher: &plugin.EnricherSpec{
			Accepts:      []string{"text"},
			RequiredTags: []string{plugin.TagLink},
			Triggers:     []string{plugin.EnrichTriggerOnCommit},
			Annotations:  []string{plugin.AnnotationLinkPreviewV1},
		},
	}
}
func (customPreviewOverride) Init(context.Context, plugin.Host, plugin.Config) error { return nil }
func (customPreviewOverride) Start(context.Context) error                            { return nil }
func (customPreviewOverride) Close(context.Context) error                            { return nil }
func (customPreviewOverride) Enrich(_ context.Context, ec plugin.EnrichContext) error {
	payload, _ := json.Marshal(plugin.LinkPreviewV1{
		CanonicalURL: strings.TrimSpace(ec.Item().Text),
		SiteName:     "External Override",
		Title:        "External Plugin Wins",
	})
	return ec.SetResult(plugin.EnrichmentResult{
		Status: plugin.EnrichStatusOK,
		Annotations: []plugin.EnrichmentAnnotation{{
			Type:        plugin.AnnotationLinkPreviewV1,
			PayloadJSON: payload,
		}},
	})
}

func TestExternalEnricherCanOverrideBuiltinPreviewViaOrder(t *testing.T) {
	ctx := context.Background()
	srv, _ := startTestLinkServer(t)

	classifier, _ := builtinclassify.New(nil, nil)
	previewer, _ := builtinpreview.NewForTest(builtinpreview.Settings{}, true)
	e, err := engine.New(ctx, engine.Options{
		DataDir:     canonicalTempDir(t),
		PluginOrder: []string{"ext.custompreview", builtinpreview.ID},
		Plugins: []engine.PluginInstance{
			{Plugin: classifier},
			{Plugin: previewer, Builtin: true},
			{Plugin: customPreviewOverride{}},
		},
	})
	if err != nil {
		t.Fatalf("engine.New: %v", err)
	}
	defer func() { _ = e.Close(context.Background()) }()
	if err := e.Start(ctx); err != nil {
		t.Fatalf("engine.Start: %v", err)
	}

	histID := commitLinkText(t, e, "req-smart-3", srv.URL+"/article")
	anns := waitForAnnotations(t, e, histID, 2)
	if anns[0].PluginID != "ext.custompreview" {
		t.Fatalf("expected external plugin first per PluginOrder, got order: %s, %s", anns[0].PluginID, anns[1].PluginID)
	}
}

func TestExternalPluginCannotSpoofBuiltinPreviewID(t *testing.T) {
	previewer, _ := builtinpreview.New(builtinpreview.Settings{})
	_, err := engine.New(context.Background(), engine.Options{
		DataDir: canonicalTempDir(t),
		Plugins: []engine.PluginInstance{
			{Plugin: previewer, Builtin: false},
		},
	})
	if err == nil {
		t.Fatal("expected engine.New to reject builtin.preview without Builtin=true")
	}
}
