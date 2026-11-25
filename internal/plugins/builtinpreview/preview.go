// Package builtinpreview implements the built-in Smart Preview enricher plugin
// ("builtin.preview"). It ships enabled by default and can be disabled wholly
// or per sub-preview in host configuration. External plugins share the exact
// same CapEnricher and versioned annotation contracts and may override or add
// preview annotations according to plugins.order.
package builtinpreview

import (
	"context"
	"errors"
	"slices"
	"strings"
	"unicode"
	"unicode/utf8"

	"github.com/uniclipboard/unic/plugin"
)

const (
	// ID is the stable built-in plugin identifier.
	ID = "builtin.preview"

	DefaultTimeoutSec = 8
	MinTimeoutSec     = 2
	MaxTimeoutSec     = 15
	MaxExcludedRules  = 64
)

// Settings configures builtin.preview sub-features. Zero values select defaults
// (Smart Preview enabled, link preview + favicon + og:image thumbnail enabled).
type Settings struct {
	LinkPreview       *bool    `toml:"link_preview,omitempty" json:"link_preview,omitempty"`
	FetchFavicon      *bool    `toml:"fetch_favicon,omitempty" json:"fetch_favicon,omitempty"`
	FetchThumbnail    *bool    `toml:"fetch_thumbnail,omitempty" json:"fetch_thumbnail,omitempty"`
	RequestTimeoutSec int      `toml:"request_timeout_sec,omitempty" json:"request_timeout_sec,omitempty"`
	ExcludedDomains   []string `toml:"excluded_domains,omitempty" json:"excluded_domains,omitempty"`
}

// Equal reports whether two Settings represent the same effective configuration.
func (s Settings) Equal(o Settings) bool {
	a, b := s.resolve(), o.resolve()
	return a.linkPreview == b.linkPreview &&
		a.fetchFavicon == b.fetchFavicon &&
		a.fetchThumbnail == b.fetchThumbnail &&
		a.requestTimeoutSec == b.requestTimeoutSec &&
		slices.Equal(a.excludedDomains, b.excludedDomains)
}

// Normalized returns a canonical Settings value with all defaults materialized
// and excluded domains deduplicated and sorted, suitable for deep-equality checks.
func (s Settings) Normalized() Settings {
	r := s.resolve()
	lp, fav, thumb := r.linkPreview, r.fetchFavicon, r.fetchThumbnail
	var domains []string
	if len(r.excludedDomains) > 0 {
		domains = slices.Clone(r.excludedDomains)
	}
	return Settings{
		LinkPreview:       &lp,
		FetchFavicon:      &fav,
		FetchThumbnail:    &thumb,
		RequestTimeoutSec: r.requestTimeoutSec,
		ExcludedDomains:   domains,
	}
}

// Validate checks bounds on user-supplied Smart Preview settings.
func (s Settings) Validate() error {
	if s.RequestTimeoutSec != 0 && (s.RequestTimeoutSec < MinTimeoutSec || s.RequestTimeoutSec > MaxTimeoutSec) {
		return errors.New("request_timeout_sec must be between 2 and 15")
	}
	if len(s.ExcludedDomains) > MaxExcludedRules {
		return errors.New("at most 64 excluded domains are allowed")
	}
	for _, raw := range s.ExcludedDomains {
		d := strings.TrimSpace(raw)
		if d == "" || len(d) > 253 || !utf8.ValidString(d) || strings.ContainsFunc(d, func(r rune) bool {
			return unicode.IsControl(r) || unicode.IsSpace(r)
		}) {
			return errors.New("invalid excluded domain entry")
		}
	}
	return nil
}

type resolvedSettings struct {
	linkPreview       bool
	fetchFavicon      bool
	fetchThumbnail    bool
	requestTimeoutSec int
	excludedDomains   []string
}

func boolOrDefault(v *bool, def bool) bool {
	if v == nil {
		return def
	}
	return *v
}

func (s Settings) resolve() resolvedSettings {
	timeout := s.RequestTimeoutSec
	if timeout == 0 {
		timeout = DefaultTimeoutSec
	}
	domains := make([]string, 0, len(s.ExcludedDomains))
	for _, d := range s.ExcludedDomains {
		d = strings.ToLower(strings.TrimSuffix(strings.TrimSpace(d), "."))
		if d != "" && !slices.Contains(domains, d) {
			domains = append(domains, d)
		}
	}
	slices.Sort(domains)
	return resolvedSettings{
		linkPreview:       boolOrDefault(s.LinkPreview, true),
		fetchFavicon:      boolOrDefault(s.FetchFavicon, true),
		fetchThumbnail:    boolOrDefault(s.FetchThumbnail, true),
		requestTimeoutSec: timeout,
		excludedDomains:   domains,
	}
}

// LinkPreviewEnabled reports whether URL link previewing is enabled.
func (s Settings) LinkPreviewEnabled() bool { return boolOrDefault(s.LinkPreview, true) }

// FetchFaviconEnabled reports whether site favicon fetching is enabled.
func (s Settings) FetchFaviconEnabled() bool { return boolOrDefault(s.FetchFavicon, true) }

// FetchThumbnailEnabled reports whether og:image / twitter:image fetching is enabled.
func (s Settings) FetchThumbnailEnabled() bool { return boolOrDefault(s.FetchThumbnail, true) }

type providerOutput struct {
	searchDocs []plugin.EnrichmentSearchDoc
	addTags    []string
	annotation plugin.EnrichmentAnnotation
}

// provider is one modular built-in preview recognizer (e.g. link_preview.v1 now,
// otp_preview.v1 or structured local previews in future iterations).
type provider interface {
	Name() string
	AnnotationType() string
	RequiredTags() []string
	Enabled(s resolvedSettings) bool
	Enrich(ctx context.Context, item plugin.Item, s resolvedSettings, allowLoopback bool) (providerOutput, bool, error)
}

// Plugin is the in-process Smart Preview enricher.
type Plugin struct {
	settings      resolvedSettings
	allowLoopback bool
	providers     []provider
}

// New validates settings and constructs the built-in Smart Preview plugin.
func New(s Settings) (*Plugin, error) {
	return newPlugin(s, false)
}

// NewForTest constructs Smart Preview with optional loopback access for local HTTP tests.
func NewForTest(s Settings, allowLoopback bool) (*Plugin, error) {
	return newPlugin(s, allowLoopback)
}

func newPlugin(s Settings, allowLoopback bool) (*Plugin, error) {
	if err := s.Validate(); err != nil {
		return nil, err
	}
	return &Plugin{
		settings:      s.resolve(),
		allowLoopback: allowLoopback,
		providers: []provider{
			linkPreviewProvider{},
		},
	}, nil
}

func (p *Plugin) Spec() plugin.Spec {
	var anns []string
	var reqTags []string
	first := true
	for _, prov := range p.providers {
		anns = append(anns, prov.AnnotationType())
		if !prov.Enabled(p.settings) {
			continue
		}
		pt := prov.RequiredTags()
		if first {
			reqTags = slices.Clone(pt)
			first = false
		} else {
			// Intersect required tags across active providers so every active
			// provider receives items it can inspect.
			reqTags = slices.DeleteFunc(reqTags, func(t string) bool { return !slices.Contains(pt, t) })
		}
	}
	return plugin.Spec{
		ID:           ID,
		Version:      "1.0.0",
		APIVersion:   1,
		Capabilities: []string{plugin.CapEnricher},
		Enricher: &plugin.EnricherSpec{
			Accepts:      []string{"text"},
			RequiredTags: reqTags,
			Triggers: []string{
				plugin.EnrichTriggerOnCommit,
				plugin.EnrichTriggerOnDemand,
				plugin.EnrichTriggerManual,
			},
			Annotations: anns,
			TimeoutMS:   15000,
		},
	}
}

func (*Plugin) Init(_ context.Context, _ plugin.Host, cfg plugin.Config) error {
	if len(cfg) != 0 {
		return errors.New("builtin.preview takes no raw config map")
	}
	return nil
}

func (*Plugin) Start(context.Context) error { return nil }
func (*Plugin) Close(context.Context) error { return nil }

func (p *Plugin) Enrich(ctx context.Context, c plugin.EnrichContext) error {
	item := c.Item()
	var res plugin.EnrichmentResult
	var lastErr error
	matchedAny := false

	for _, prov := range p.providers {
		if !prov.Enabled(p.settings) {
			continue
		}
		out, ok, err := prov.Enrich(ctx, item, p.settings, p.allowLoopback)
		if err != nil {
			lastErr = err
			continue
		}
		if !ok {
			continue
		}
		matchedAny = true
		res.SearchDocs = append(res.SearchDocs, out.searchDocs...)
		for _, t := range out.addTags {
			if !slices.Contains(res.AddTags, t) {
				res.AddTags = append(res.AddTags, t)
			}
		}
		if out.annotation.Type != "" {
			res.Annotations = append(res.Annotations, out.annotation)
		}
	}

	if matchedAny {
		res.Status = plugin.EnrichStatusOK
		return c.SetResult(res)
	}
	if lastErr != nil {
		return lastErr
	}
	return c.SetResult(plugin.EnrichmentResult{Status: plugin.EnrichStatusSkipped})
}
