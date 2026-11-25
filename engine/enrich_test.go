package engine_test

import (
	"bytes"
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"image"
	"image/color"
	"image/png"
	"io"
	"net/http"
	"net/http/httptest"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
	"sync/atomic"
	"testing"
	"time"

	"github.com/uniclipboard/unic/engine"
	"github.com/uniclipboard/unic/internal/plugins/builtinclassify"
	"github.com/uniclipboard/unic/plugin"
)

func buildLinkPreviewPlugin(t *testing.T) (binPath, digest string, spec plugin.Spec) {
	t.Helper()
	dir := t.TempDir()
	binPath = filepath.Join(dir, "unic-plugin-linkpreview")
	cmd := exec.Command("go", "build", "-o", binPath, "../plugins/examples/linkpreview")
	out, err := cmd.CombinedOutput()
	if err != nil {
		t.Fatalf("build linkpreview plugin: %v\n%s", err, out)
	}
	raw, err := os.ReadFile(binPath)
	if err != nil {
		t.Fatalf("read plugin binary: %v", err)
	}
	sum := sha256.Sum256(raw)
	digest = hex.EncodeToString(sum[:])

	schemaBytes, err := os.ReadFile("../plugins/examples/linkpreview/settings.schema.json")
	if err != nil {
		t.Fatalf("read settings.schema.json: %v", err)
	}
	schemaSum := sha256.Sum256(schemaBytes)

	spec = plugin.Spec{
		ID:             "unic.linkpreview",
		Version:        "0.1.0",
		APIVersion:     1,
		SettingsSHA256: hex.EncodeToString(schemaSum[:]),
		Capabilities:   []string{plugin.CapEnricher},
		Enricher: &plugin.EnricherSpec{
			Accepts:      []string{"text"},
			RequiredTags: []string{plugin.TagLink},
			Triggers: []string{
				plugin.EnrichTriggerOnCommit,
				plugin.EnrichTriggerOnDemand,
				plugin.EnrichTriggerManual,
			},
			Annotations: []string{plugin.AnnotationLinkPreviewV1},
			TimeoutMS:   15000,
		},
	}
	return binPath, digest, spec
}

func testPNG(w, h int, c color.RGBA) []byte {
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

func waitForAnnotation(t *testing.T, e *engine.Engine, id engine.HistoryID) []engine.HistoryAnnotation {
	t.Helper()
	deadline := time.Now().Add(10 * time.Second)
	for time.Now().Before(deadline) {
		anns, err := e.GetHistoryAnnotations(context.Background(), id)
		if err == nil && len(anns) > 0 {
			return anns
		}
		time.Sleep(30 * time.Millisecond)
	}
	t.Fatalf("timed out waiting for annotation on %s", id)
	return nil
}

func canonicalTempDir(t *testing.T) string {
	t.Helper()
	dir, err := filepath.EvalSymlinks(t.TempDir())
	if err != nil {
		t.Fatal(err)
	}
	return dir
}

func TestEnricherLinkPreviewRealProcessE2E(t *testing.T) {
	binPath, digest, spec := buildLinkPreviewPlugin(t)
	favBytes := testPNG(64, 64, color.RGBA{R: 15, G: 90, B: 230, A: 255})
	ogBytes := testPNG(960, 540, color.RGBA{R: 40, G: 170, B: 110, A: 255})

	var hits atomic.Int32
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		hits.Add(1)
		switch r.URL.Path {
		case "/article/42":
			w.Header().Set("Content-Type", "text/html; charset=utf-8")
			_, _ = w.Write([]byte(`<!doctype html><html><head>
				<meta property="og:site_name" content="Unic Engineering">
				<meta property="og:title" content="星际剪贴板架构详解">
				<meta property="og:description" content="深入解析异步富化流水线与安全边界设计。">
				<meta property="og:image" content="/img/og.png">
				<link rel="icon" href="/img/fav.png">
			</head><body>ok</body></html>`))
		case "/img/fav.png":
			w.Header().Set("Content-Type", "image/png")
			_, _ = w.Write(favBytes)
		case "/img/og.png":
			w.Header().Set("Content-Type", "image/png")
			_, _ = w.Write(ogBytes)
		case "/boom":
			http.Error(w, "upstream failure", http.StatusInternalServerError)
		default:
			http.NotFound(w, r)
		}
	}))
	defer srv.Close()

	dataDir := canonicalTempDir(t)
	pluginDataDir := filepath.Join(dataDir, "plugins", "data", "unic.linkpreview")
	if err := os.MkdirAll(pluginDataDir, 0o700); err != nil {
		t.Fatal(err)
	}

	ctx := context.Background()
	cls, err := builtinclassify.New(nil, nil)
	if err != nil {
		t.Fatal(err)
	}
	eng, err := engine.New(ctx, engine.Options{
		DataDir: dataDir,
		Plugins: []engine.PluginInstance{
			{Plugin: cls, Builtin: true},
			{
				Process: &engine.PluginProcess{
					Path:   binPath,
					SHA256: digest,
					Args:   []string{"-allow-loopback-for-test"},
					Dir:    pluginDataDir,
					Spec:   spec,
				},
			},
		},
	})
	if err != nil {
		t.Fatalf("engine.New: %v", err)
	}
	defer func() { _ = eng.Close(context.Background()) }()
	if err := eng.Start(ctx); err != nil {
		t.Fatalf("engine.Start: %v", err)
	}

	// 1. Commit a standalone URL and verify async enrichment + OG image + derived search.
	urlText := srv.URL + "/article/42"
	h, err := eng.ImportContent(ctx, engine.ContentInput{
		Representations: []engine.RepresentationInput{{MIME: engine.MimeText, Reader: strings.NewReader(urlText), Size: int64(len(urlText))}},
	})
	if err != nil {
		t.Fatalf("ImportContent: %v", err)
	}
	res, err := eng.CommitContent(ctx, engine.CommitRequest{RequestID: "req-link-1", LeaseID: h.LeaseID})
	_ = eng.ReleaseContent(ctx, h.LeaseID)
	if err != nil {
		t.Fatalf("CommitContent: %v", err)
	}

	anns := waitForAnnotation(t, eng, res.HistoryID)
	if len(anns) != 1 || anns[0].Type != plugin.AnnotationLinkPreviewV1 || anns[0].PluginID != "unic.linkpreview" {
		t.Fatalf("unexpected annotations: %+v", anns)
	}
	var lp plugin.LinkPreviewV1
	if err := json.Unmarshal(anns[0].Payload, &lp); err != nil {
		t.Fatalf("unmarshal link_preview.v1: %v", err)
	}
	if lp.Title != "星际剪贴板架构详解" || lp.SiteName != "Unic Engineering" || lp.FaviconAsset != "favicon" || lp.ImageAsset != "thumbnail" {
		t.Fatalf("unexpected link preview payload: %+v", lp)
	}

	// Read both binary assets and verify valid images.
	for _, key := range []string{"favicon", "thumbnail"} {
		rc, meta, err := eng.OpenHistoryAnnotationAsset(ctx, res.HistoryID, "unic.linkpreview", plugin.AnnotationLinkPreviewV1, key)
		if err != nil {
			t.Fatalf("OpenHistoryAnnotationAsset(%s): %v", key, err)
		}
		b, rerr := io.ReadAll(rc)
		_ = rc.Close()
		if rerr != nil || len(b) == 0 || int64(len(b)) != meta.Size {
			t.Fatalf("asset %s read mismatch: err=%v len=%d size=%d", key, rerr, len(b), meta.Size)
		}
		if _, _, derr := image.DecodeConfig(bytes.NewReader(b)); derr != nil {
			t.Fatalf("asset %s not valid image: %v", key, derr)
		}
	}

	// Search by webpage title (absent from raw URL!).
	page, err := eng.SearchHistory(ctx, engine.HistorySearchRequest{
		Query: engine.HistorySearchQuery{TextTerms: []string{"星际剪贴板"}},
	})
	if err != nil {
		t.Fatalf("SearchHistory(title): %v", err)
	}
	if len(page.Hits) != 1 || page.Hits[0].Item.ID != res.HistoryID || page.Hits[0].Preview.MatchKind != "link_title" {
		t.Fatalf("expected link_title hit, got: %+v", page.Hits)
	}
	if len(page.Hits[0].Preview.Highlights) == 0 {
		t.Fatalf("expected non-empty highlight ranges")
	}

	// Search by webpage description.
	descPage, err := eng.SearchHistory(ctx, engine.HistorySearchRequest{
		Query: engine.HistorySearchQuery{TextTerms: []string{"异步富化流水线"}},
	})
	if err != nil || len(descPage.Hits) != 1 || descPage.Hits[0].Preview.MatchKind != "link_desc" {
		t.Fatalf("expected link_desc hit, err=%v hits=%+v", err, descPage.Hits)
	}

	// 2. Commit a failing URL (HTTP 500) and verify base search NEVER reports ErrSearchIncomplete.
	boomURL := srv.URL + "/boom"
	hb, err := eng.ImportContent(ctx, engine.ContentInput{
		Representations: []engine.RepresentationInput{{MIME: engine.MimeText, Reader: strings.NewReader(boomURL), Size: int64(len(boomURL))}},
	})
	if err != nil {
		t.Fatal(err)
	}
	resBoom, err := eng.CommitContent(ctx, engine.CommitRequest{RequestID: "req-link-boom", LeaseID: hb.LeaseID})
	_ = eng.ReleaseContent(ctx, hb.LeaseID)
	if err != nil {
		t.Fatal(err)
	}
	time.Sleep(300 * time.Millisecond)
	boomSearch, err := eng.SearchHistory(ctx, engine.HistorySearchRequest{
		Query: engine.HistorySearchQuery{TextTerms: []string{"/boom"}},
	})
	if err != nil || len(boomSearch.Hits) != 1 || boomSearch.Hits[0].Item.ID != resBoom.HistoryID || boomSearch.IndexIncomplete {
		t.Fatalf("base search must stay complete after failed enrichment: err=%v page=%+v", err, boomSearch)
	}

	// 3. Delete enriched item and verify cascade removes annotations, assets, and derived search hits.
	it, err := eng.GetItem(ctx, res.HistoryID)
	if err != nil {
		t.Fatal(err)
	}
	if err := eng.DeleteItem(ctx, engine.DeleteItemRequest{ID: it.ID, ExpectedRevision: it.Revision}); err != nil {
		t.Fatalf("DeleteItem: %v", err)
	}
	if _, err := eng.GetHistoryAnnotations(ctx, it.ID); err == nil {
		t.Fatal("expected not_found after delete")
	}
	afterDel, err := eng.SearchHistory(ctx, engine.HistorySearchRequest{
		Query: engine.HistorySearchQuery{TextTerms: []string{"星际剪贴板"}},
	})
	if err != nil || len(afterDel.Hits) != 0 {
		t.Fatalf("expected 0 hits after cascade delete, err=%v hits=%d", err, len(afterDel.Hits))
	}
}

func TestEnricherAbsentAndDisabledZeroNetworkOverhead(t *testing.T) {
	binPath, digest, spec := buildLinkPreviewPlugin(t)
	var hits atomic.Int32
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		hits.Add(1)
		w.Header().Set("Content-Type", "text/html")
		_, _ = w.Write([]byte(`<html><head><title>Should Never Be Fetched</title></head></html>`))
	}))
	defer srv.Close()

	for _, tc := range []struct {
		name     string
		disabled bool
		install  bool
	}{
		{name: "absent", install: false},
		{name: "installed_disabled", install: true, disabled: true},
	} {
		t.Run(tc.name, func(t *testing.T) {
			hits.Store(0)
			dataDir := canonicalTempDir(t)
			cls, err := builtinclassify.New(nil, nil)
			if err != nil {
				t.Fatal(err)
			}
			plugins := []engine.PluginInstance{{Plugin: cls, Builtin: true}}
			if tc.install {
				plugins = append(plugins, engine.PluginInstance{
					Disabled: tc.disabled,
					Process: &engine.PluginProcess{
						Path:   binPath,
						SHA256: digest,
						Args:   []string{"-allow-loopback-for-test"},
						Spec:   spec,
					},
				})
			}
			ctx := context.Background()
			eng, err := engine.New(ctx, engine.Options{DataDir: dataDir, Plugins: plugins})
			if err != nil {
				t.Fatalf("engine.New: %v", err)
			}
			defer func() { _ = eng.Close(context.Background()) }()
			if err := eng.Start(ctx); err != nil {
				t.Fatalf("engine.Start: %v", err)
			}
			u := srv.URL + "/zero"
			h, err := eng.ImportContent(ctx, engine.ContentInput{
				Representations: []engine.RepresentationInput{{MIME: engine.MimeText, Reader: strings.NewReader(u), Size: int64(len(u))}},
			})
			if err != nil {
				t.Fatal(err)
			}
			res, err := eng.CommitContent(ctx, engine.CommitRequest{RequestID: "req-zero", LeaseID: h.LeaseID})
			_ = eng.ReleaseContent(ctx, h.LeaseID)
			if err != nil {
				t.Fatal(err)
			}
			time.Sleep(250 * time.Millisecond)
			if hits.Load() != 0 {
				t.Fatalf("expected 0 HTTP requests when plugin is %s, got %d", tc.name, hits.Load())
			}
			anns, err := eng.GetHistoryAnnotations(ctx, res.HistoryID)
			if err != nil || len(anns) != 0 {
				t.Fatalf("expected empty annotations, err=%v anns=%+v", err, anns)
			}
		})
	}
}

func TestEnricherLiveStartAndOnDemand(t *testing.T) {
	binPath, digest, spec := buildLinkPreviewPlugin(t)
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		w.Header().Set("Content-Type", "text/html; charset=utf-8")
		_, _ = w.Write([]byte(`<html><head><meta property="og:title" content="Live Started Enricher Title"></head></html>`))
	}))
	defer srv.Close()

	ctx := context.Background()
	dataDir := canonicalTempDir(t)
	cls, err := builtinclassify.New(nil, nil)
	if err != nil {
		t.Fatal(err)
	}
	eng, err := engine.New(ctx, engine.Options{
		DataDir: dataDir,
		Plugins: []engine.PluginInstance{{Plugin: cls, Builtin: true}},
	})
	if err != nil {
		t.Fatal(err)
	}
	defer func() { _ = eng.Close(context.Background()) }()
	if err := eng.Start(ctx); err != nil {
		t.Fatal(err)
	}

	u := srv.URL + "/ondemand"
	h, err := eng.ImportContent(ctx, engine.ContentInput{
		Representations: []engine.RepresentationInput{{MIME: engine.MimeText, Reader: strings.NewReader(u), Size: int64(len(u))}},
	})
	if err != nil {
		t.Fatal(err)
	}
	res, err := eng.CommitContent(ctx, engine.CommitRequest{RequestID: "req-live", LeaseID: h.LeaseID})
	_ = eng.ReleaseContent(ctx, h.LeaseID)
	if err != nil {
		t.Fatal(err)
	}

	if !engine.SupportsLivePluginStart(spec) {
		t.Fatal("expected enricher-only plugin to support live start")
	}
	pDir := filepath.Join(dataDir, "plugins", "data", "unic.linkpreview")
	_ = os.MkdirAll(pDir, 0o700)
	err = eng.StartExternalPlugin(ctx, engine.PluginInstance{
		Process: &engine.PluginProcess{
			Path:   binPath,
			SHA256: digest,
			Args:   []string{"-allow-loopback-for-test"},
			Dir:    pDir,
			Spec:   spec,
		},
	}, engine.BackendSyncPolicy{}, func() error { return nil })
	if err != nil {
		t.Fatalf("StartExternalPlugin: %v", err)
	}

	if err := eng.EnrichHistory(ctx, engine.HistoryEnrichRequest{HistoryID: res.HistoryID}); err != nil {
		t.Fatalf("EnrichHistory: %v", err)
	}
	anns := waitForAnnotation(t, eng, res.HistoryID)
	if len(anns) != 1 {
		t.Fatalf("expected 1 annotation after live start + on_demand, got %d", len(anns))
	}
	page, err := eng.SearchHistory(ctx, engine.HistorySearchRequest{
		Query: engine.HistorySearchQuery{TextTerms: []string{"Live Started Enricher"}},
	})
	if err != nil || len(page.Hits) != 1 {
		t.Fatalf("expected search hit after live on-demand enrich, err=%v hits=%d", err, len(page.Hits))
	}
}
