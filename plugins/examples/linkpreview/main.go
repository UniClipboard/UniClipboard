package main

import (
	"context"
	"crypto/sha256"
	_ "embed"
	"encoding/hex"
	"encoding/json"
	"flag"
	"fmt"
	"os"
	"strings"
	"sync"

	"github.com/uniclipboard/unic/plugin"
)

//go:embed settings.schema.json
var settingsSchemaBytes []byte

var settingsSchemaSHA256 = func() string {
	sum := sha256.Sum256(settingsSchemaBytes)
	return hex.EncodeToString(sum[:])
}()

type linkPreviewConfig struct {
	AutoFetch         bool
	FetchFavicon      bool
	FetchThumbnail    bool
	RequestTimeoutSec int
	ExcludedDomains   []string
}

func defaultConfig() linkPreviewConfig {
	return linkPreviewConfig{
		AutoFetch:         true,
		FetchFavicon:      true,
		FetchThumbnail:    true,
		RequestTimeoutSec: 8,
	}
}

type linkPreviewPlugin struct {
	mu            sync.RWMutex
	cfg           linkPreviewConfig
	allowLoopback bool
}

func main() {
	allowLoopback := flag.Bool("allow-loopback-for-test", false, "allow loopback targets in automated tests only")
	flag.Parse()
	if err := plugin.Serve(&linkPreviewPlugin{
		cfg:           defaultConfig(),
		allowLoopback: *allowLoopback,
	}); err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
}

func (*linkPreviewPlugin) Spec() plugin.Spec {
	return plugin.Spec{
		ID:             "unic.linkpreview",
		Version:        "0.1.0",
		APIVersion:     1,
		SettingsSHA256: settingsSchemaSHA256,
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
}

func (p *linkPreviewPlugin) Init(_ context.Context, _ plugin.Host, raw plugin.Config) error {
	cfg := defaultConfig()
	for k, v := range raw {
		switch k {
		case "auto_fetch":
			b, ok := v.(bool)
			if !ok {
				return plugin.ErrInvalidConfig
			}
			cfg.AutoFetch = b
		case "fetch_favicon":
			b, ok := v.(bool)
			if !ok {
				return plugin.ErrInvalidConfig
			}
			cfg.FetchFavicon = b
		case "fetch_thumbnail":
			b, ok := v.(bool)
			if !ok {
				return plugin.ErrInvalidConfig
			}
			cfg.FetchThumbnail = b
		case "request_timeout_sec":
			n, ok := jsonInt(v)
			if !ok || n < 2 || n > 15 {
				return plugin.ErrInvalidConfig
			}
			cfg.RequestTimeoutSec = n
		case "excluded_domains":
			list, ok := v.([]any)
			if !ok {
				return plugin.ErrInvalidConfig
			}
			out := make([]string, 0, len(list))
			for _, elem := range list {
				s, ok := elem.(string)
				if !ok || len(s) > 253 {
					return plugin.ErrInvalidConfig
				}
				s = strings.ToLower(strings.TrimSpace(s))
				if s != "" {
					out = append(out, s)
				}
			}
			cfg.ExcludedDomains = out
		default:
			return plugin.ErrInvalidConfig
		}
	}
	p.mu.Lock()
	p.cfg = cfg
	p.mu.Unlock()
	return nil
}

func jsonInt(v any) (int, bool) {
	switch n := v.(type) {
	case float64:
		i := int(n)
		return i, float64(i) == n
	case int:
		return n, true
	case int64:
		return int(n), true
	case json.Number:
		i, err := n.Int64()
		return int(i), err == nil
	default:
		return 0, false
	}
}

func (*linkPreviewPlugin) Start(context.Context) error { return nil }
func (*linkPreviewPlugin) Close(context.Context) error { return nil }

func (p *linkPreviewPlugin) Enrich(ctx context.Context, c plugin.EnrichContext) error {
	p.mu.RLock()
	cfg := p.cfg
	allowLoopback := p.allowLoopback
	p.mu.RUnlock()

	if c.Trigger() == plugin.EnrichTriggerOnCommit && !cfg.AutoFetch {
		return c.SetResult(plugin.EnrichmentResult{Status: plugin.EnrichStatusSkipped})
	}
	item := c.Item()
	if item.Type != "text" {
		return c.SetResult(plugin.EnrichmentResult{Status: plugin.EnrichStatusSkipped})
	}
	parsed, ok := extractStandaloneURL(item.Text)
	if !ok {
		return c.SetResult(plugin.EnrichmentResult{Status: plugin.EnrichStatusSkipped})
	}
	if isExcludedDomain(parsed.Hostname(), cfg.ExcludedDomains) {
		return c.SetResult(plugin.EnrichmentResult{Status: plugin.EnrichStatusSkipped})
	}

	preview, assets, err := fetchLinkPreview(ctx, parsed.String(), cfg, allowLoopback)
	if err != nil {
		return err
	}
	if strings.TrimSpace(preview.Title) == "" && strings.TrimSpace(preview.Description) == "" {
		return c.SetResult(plugin.EnrichmentResult{Status: plugin.EnrichStatusSkipped})
	}
	payload, err := json.Marshal(preview)
	if err != nil {
		return err
	}
	var docs []plugin.EnrichmentSearchDoc
	if preview.Title != "" {
		docs = append(docs, plugin.EnrichmentSearchDoc{Kind: "link_title", Text: preview.Title})
	}
	if preview.Description != "" {
		docs = append(docs, plugin.EnrichmentSearchDoc{Kind: "link_desc", Text: preview.Description})
	}
	return c.SetResult(plugin.EnrichmentResult{
		Status:     plugin.EnrichStatusOK,
		SearchDocs: docs,
		Annotations: []plugin.EnrichmentAnnotation{
			{
				Type:        plugin.AnnotationLinkPreviewV1,
				PayloadJSON: payload,
				Assets:      assets,
			},
		},
	})
}
