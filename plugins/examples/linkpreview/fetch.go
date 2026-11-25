package main

import (
	"bytes"
	"context"
	"errors"
	"image"
	_ "image/gif"
	"image/jpeg"
	"image/png"
	"io"
	"net"
	"net/http"
	"net/netip"
	"net/url"
	"strconv"
	"strings"
	"time"
	"unicode"
	"unicode/utf8"

	"github.com/disintegration/imaging"
	_ "golang.org/x/image/webp"
	"golang.org/x/net/html"

	"github.com/uniclipboard/unic/plugin"
)

const (
	maxHTMLBytes      = 512 << 10
	maxRedirects      = 5
	maxFaviconDownB   = 256 << 10
	maxFaviconOutB    = 64 << 10
	maxFaviconEdge    = 64
	maxThumbDownB     = 4 << 20
	maxThumbOutB      = 256 << 10
	maxThumbEdge      = 640
	maxImagePixels    = 16_000_000
	linkUserAgent     = "unic-linkpreview/0.1"
	maxCanonicalBytes = 2048
)

var (
	errBlockedTarget = errors.New("target blocked by SSRF policy")
	errNotHTML       = errors.New("response is not HTML")
)

func extractStandaloneURL(text string) (*url.URL, bool) {
	trimmed := strings.TrimSpace(text)
	if trimmed == "" || len(trimmed) > maxCanonicalBytes {
		return nil, false
	}
	if strings.ContainsAny(trimmed, " \t\r\n") {
		return nil, false
	}
	u, err := url.Parse(trimmed)
	if err != nil {
		return nil, false
	}
	if (u.Scheme != "http" && u.Scheme != "https") || u.Host == "" || u.User != nil {
		return nil, false
	}
	return u, true
}

func isExcludedDomain(host string, rules []string) bool {
	h := strings.ToLower(strings.TrimSuffix(strings.TrimSpace(host), "."))
	for _, rule := range rules {
		r := strings.ToLower(strings.TrimSuffix(strings.TrimSpace(rule), "."))
		if r == "" {
			continue
		}
		if strings.HasPrefix(r, "*.") {
			suffix := strings.TrimPrefix(r, "*.")
			if h == suffix || strings.HasSuffix(h, "."+suffix) {
				return true
			}
			continue
		}
		if h == r {
			return true
		}
	}
	return false
}

func isBlockedHostname(host string, allowLoopback bool) bool {
	h := strings.ToLower(strings.TrimSuffix(strings.TrimSpace(host), "."))
	if h == "" {
		return true
	}
	if !allowLoopback && (h == "localhost" || strings.HasSuffix(h, ".localhost")) {
		return true
	}
	if strings.HasSuffix(h, ".local") || strings.HasSuffix(h, ".internal") || strings.HasSuffix(h, ".home.arpa") {
		return true
	}
	return false
}

func isAllowedAddr(addr netip.Addr, allowLoopback bool) bool {
	addr = addr.Unmap()
	if !addr.IsValid() {
		return false
	}
	if addr.IsLoopback() {
		return allowLoopback
	}
	if addr.IsPrivate() || addr.IsLinkLocalUnicast() || addr.IsLinkLocalMulticast() || addr.IsMulticast() || addr.IsUnspecified() || addr.IsInterfaceLocalMulticast() {
		return false
	}
	// Block RFC 6598 CGNAT (100.64.0.0/10), 0.0.0.0/8, and IPv4 benchmark/reserved ranges.
	if addr.Is4() {
		a4 := addr.As4()
		if a4[0] == 0 || (a4[0] == 100 && a4[1]&0xc0 == 64) || a4[0] >= 240 {
			return false
		}
	}
	return true
}

func newSafeHTTPClient(timeout time.Duration, allowLoopback bool) *http.Client {
	baseDialer := &net.Dialer{Timeout: min(timeout, 5*time.Second), KeepAlive: 0}
	transport := &http.Transport{
		DisableKeepAlives: true,
		Proxy:             nil,
		DialContext: func(ctx context.Context, network, addr string) (net.Conn, error) {
			host, port, err := net.SplitHostPort(addr)
			if err != nil {
				return nil, errBlockedTarget
			}
			if isBlockedHostname(host, allowLoopback) {
				return nil, errBlockedTarget
			}
			if ip, err := netip.ParseAddr(host); err == nil {
				if !isAllowedAddr(ip, allowLoopback) {
					return nil, errBlockedTarget
				}
				return baseDialer.DialContext(ctx, network, net.JoinHostPort(ip.Unmap().String(), port))
			}
			ips, err := net.DefaultResolver.LookupNetIP(ctx, "ip", host)
			if err != nil || len(ips) == 0 {
				return nil, err
			}
			var allowed []netip.Addr
			for _, ip := range ips {
				if !isAllowedAddr(ip, allowLoopback) {
					return nil, errBlockedTarget
				}
				allowed = append(allowed, ip.Unmap())
			}
			var lastErr error
			for _, ip := range allowed {
				conn, derr := baseDialer.DialContext(ctx, network, net.JoinHostPort(ip.String(), port))
				if derr == nil {
					return conn, nil
				}
				lastErr = derr
			}
			return nil, lastErr
		},
	}
	return &http.Client{
		Transport: transport,
		Timeout:   timeout,
		CheckRedirect: func(req *http.Request, via []*http.Request) error {
			if len(via) >= maxRedirects {
				return errors.New("too many redirects")
			}
			if req.URL == nil || (req.URL.Scheme != "http" && req.URL.Scheme != "https") || req.URL.Host == "" || req.URL.User != nil {
				return errBlockedTarget
			}
			if isBlockedHostname(req.URL.Hostname(), allowLoopback) {
				return errBlockedTarget
			}
			// Never forward cookies or referer across redirects.
			req.Header.Del("Cookie")
			req.Header.Del("Authorization")
			req.Header.Del("Referer")
			return nil
		},
	}
}

type parsedHTMLMeta struct {
	canonical   string
	siteName    string
	title       string
	description string
	faviconURL  string
	imageURL    string
}

func fetchLinkPreview(ctx context.Context, rawURL string, cfg linkPreviewConfig, allowLoopback bool) (plugin.LinkPreviewV1, []plugin.EnrichmentAsset, error) {
	timeout := time.Duration(cfg.RequestTimeoutSec) * time.Second
	if timeout <= 0 {
		timeout = 8 * time.Second
	}
	reqCtx, cancel := context.WithTimeout(ctx, timeout)
	defer cancel()

	client := newSafeHTTPClient(timeout, allowLoopback)
	req, err := http.NewRequestWithContext(reqCtx, http.MethodGet, rawURL, nil)
	if err != nil {
		return plugin.LinkPreviewV1{}, nil, err
	}
	req.Header.Set("User-Agent", linkUserAgent)
	req.Header.Set("Accept", "text/html,application/xhtml+xml;q=0.9,*/*;q=0.1")

	resp, err := client.Do(req)
	if err != nil {
		return plugin.LinkPreviewV1{}, nil, err
	}
	defer func() { _ = resp.Body.Close() }()
	if resp.StatusCode < 200 || resp.StatusCode >= 300 {
		return plugin.LinkPreviewV1{}, nil, errors.New("http status " + strconv.Itoa(resp.StatusCode))
	}
	ct := strings.ToLower(strings.TrimSpace(resp.Header.Get("Content-Type")))
	if ct != "" && !strings.HasPrefix(ct, "text/html") && !strings.HasPrefix(ct, "application/xhtml+xml") {
		return plugin.LinkPreviewV1{}, nil, errNotHTML
	}

	body, err := io.ReadAll(io.LimitReader(resp.Body, maxHTMLBytes))
	if err != nil {
		return plugin.LinkPreviewV1{}, nil, err
	}
	baseURL := resp.Request.URL
	meta := parseHTMLMeta(bytes.NewReader(body), baseURL)

	out := plugin.LinkPreviewV1{
		CanonicalURL: sanitizeURL(meta.canonical),
		SiteName:     cleanText(meta.siteName, 128),
		Title:        cleanText(meta.title, 256),
		Description:  cleanText(meta.description, 512),
	}
	if out.CanonicalURL == "" && baseURL != nil {
		out.CanonicalURL = sanitizeURL(baseURL.String())
	}
	if out.SiteName == "" && baseURL != nil {
		out.SiteName = cleanText(baseURL.Hostname(), 128)
	}
	if strings.TrimSpace(out.Title) == "" && strings.TrimSpace(out.Description) == "" {
		return out, nil, nil
	}

	var assets []plugin.EnrichmentAsset
	if cfg.FetchFavicon {
		favURL := meta.faviconURL
		if favURL == "" && baseURL != nil {
			if u, err := baseURL.Parse("/favicon.ico"); err == nil {
				favURL = u.String()
			}
		}
		if favURL != "" {
			if data, mimeType, ok := fetchRasterAsset(reqCtx, client, favURL, maxFaviconDownB, maxFaviconEdge, maxFaviconOutB, false); ok {
				out.FaviconAsset = "favicon"
				assets = append(assets, plugin.EnrichmentAsset{Key: "favicon", MIME: mimeType, Data: data})
			}
		}
	}
	if cfg.FetchThumbnail && meta.imageURL != "" {
		if data, mimeType, ok := fetchRasterAsset(reqCtx, client, meta.imageURL, maxThumbDownB, maxThumbEdge, maxThumbOutB, true); ok {
			out.ImageAsset = "thumbnail"
			assets = append(assets, plugin.EnrichmentAsset{Key: "thumbnail", MIME: mimeType, Data: data})
		}
	}
	return out, assets, nil
}

func parseHTMLMeta(r io.Reader, base *url.URL) parsedHTMLMeta {
	var m parsedHTMLMeta
	var titleTag, ogTitle, twTitle string
	var metaDesc, ogDesc, twDesc string
	var ogImg, twImg string
	var iconHref, appleIconHref string

	z := html.NewTokenizer(r)
	inTitle := false
	for {
		tt := z.Next()
		switch tt {
		case html.ErrorToken:
			goto done
		case html.StartTagToken, html.SelfClosingTagToken:
			tn, hasAttr := z.TagName()
			tag := strings.ToLower(string(tn))
			if tag == "body" {
				goto done
			}
			if tag == "title" {
				if tt == html.StartTagToken {
					inTitle = true
				}
				continue
			}
			if !hasAttr {
				continue
			}
			attrs := map[string]string{}
			for {
				k, v, more := z.TagAttr()
				attrs[strings.ToLower(string(k))] = string(v)
				if !more {
					break
				}
			}
			switch tag {
			case "meta":
				content := strings.TrimSpace(attrs["content"])
				if content == "" {
					continue
				}
				prop := strings.ToLower(strings.TrimSpace(attrs["property"]))
				name := strings.ToLower(strings.TrimSpace(attrs["name"]))
				switch {
				case prop == "og:title" && ogTitle == "":
					ogTitle = content
				case (name == "twitter:title" || prop == "twitter:title") && twTitle == "":
					twTitle = content
				case prop == "og:description" && ogDesc == "":
					ogDesc = content
				case (name == "twitter:description" || prop == "twitter:description") && twDesc == "":
					twDesc = content
				case name == "description" && metaDesc == "":
					metaDesc = content
				case prop == "og:site_name" && m.siteName == "":
					m.siteName = content
				case (prop == "og:image" || prop == "og:image:secure_url") && ogImg == "":
					ogImg = content
				case (name == "twitter:image" || prop == "twitter:image" || name == "twitter:image:src") && twImg == "":
					twImg = content
				}
			case "link":
				rel := strings.ToLower(strings.TrimSpace(attrs["rel"]))
				href := strings.TrimSpace(attrs["href"])
				if href == "" {
					continue
				}
				switch {
				case rel == "canonical" && m.canonical == "":
					m.canonical = resolveRef(base, href)
				case (rel == "icon" || rel == "shortcut icon") && iconHref == "":
					iconHref = resolveRef(base, href)
				case strings.Contains(rel, "apple-touch-icon") && appleIconHref == "":
					appleIconHref = resolveRef(base, href)
				}
			}
		case html.EndTagToken:
			tn, _ := z.TagName()
			if strings.ToLower(string(tn)) == "title" {
				inTitle = false
			}
		case html.TextToken:
			if inTitle && titleTag == "" {
				titleTag = strings.TrimSpace(string(z.Text()))
			}
		}
	}

done:
	m.title = firstNonEmpty(ogTitle, twTitle, titleTag)
	m.description = firstNonEmpty(ogDesc, twDesc, metaDesc)
	m.imageURL = resolveRef(base, firstNonEmpty(ogImg, twImg))
	m.faviconURL = firstNonEmpty(iconHref, appleIconHref)
	return m
}

func firstNonEmpty(vals ...string) string {
	for _, v := range vals {
		if strings.TrimSpace(v) != "" {
			return v
		}
	}
	return ""
}

func resolveRef(base *url.URL, raw string) string {
	raw = strings.TrimSpace(raw)
	if raw == "" {
		return ""
	}
	u, err := url.Parse(raw)
	if err != nil {
		return ""
	}
	if base != nil {
		u = base.ResolveReference(u)
	}
	if (u.Scheme != "http" && u.Scheme != "https") || u.Host == "" || u.User != nil {
		return ""
	}
	return u.String()
}

func sanitizeURL(raw string) string {
	raw = strings.TrimSpace(raw)
	if raw == "" || len(raw) > maxCanonicalBytes || !utf8.ValidString(raw) {
		return ""
	}
	u, err := url.Parse(raw)
	if err != nil || (u.Scheme != "http" && u.Scheme != "https") || u.Host == "" || u.User != nil {
		return ""
	}
	return u.String()
}

func cleanText(raw string, maxRunes int) string {
	if !utf8.ValidString(raw) {
		raw = strings.ToValidUTF8(raw, "")
	}
	var b strings.Builder
	prevSpace := false
	for _, r := range raw {
		if unicode.IsControl(r) && !unicode.IsSpace(r) {
			continue
		}
		if unicode.IsSpace(r) {
			if !prevSpace {
				b.WriteByte(' ')
				prevSpace = true
			}
			continue
		}
		b.WriteRune(r)
		prevSpace = false
	}
	s := strings.TrimSpace(b.String())
	runes := []rune(s)
	if len(runes) > maxRunes {
		s = strings.TrimSpace(string(runes[:maxRunes]))
	}
	return s
}

func fetchRasterAsset(ctx context.Context, client *http.Client, rawURL string, maxDown, maxEdge, maxOut int, preferJPEG bool) ([]byte, string, bool) {
	req, err := http.NewRequestWithContext(ctx, http.MethodGet, rawURL, nil)
	if err != nil {
		return nil, "", false
	}
	req.Header.Set("User-Agent", linkUserAgent)
	req.Header.Set("Accept", "image/png,image/jpeg,image/webp,image/gif,*/*;q=0.1")

	resp, err := client.Do(req)
	if err != nil {
		return nil, "", false
	}
	defer func() { _ = resp.Body.Close() }()
	if resp.StatusCode < 200 || resp.StatusCode >= 300 {
		return nil, "", false
	}
	ct := strings.ToLower(strings.TrimSpace(resp.Header.Get("Content-Type")))
	if strings.Contains(ct, "svg") || strings.HasPrefix(ct, "text/") {
		return nil, "", false
	}
	raw, err := io.ReadAll(io.LimitReader(resp.Body, int64(maxDown)+1))
	if err != nil || len(raw) == 0 || len(raw) > maxDown {
		return nil, "", false
	}
	cfg, _, err := image.DecodeConfig(bytes.NewReader(raw))
	if err != nil || cfg.Width <= 0 || cfg.Height <= 0 || int64(cfg.Width)*int64(cfg.Height) > maxImagePixels {
		return nil, "", false
	}
	img, _, err := image.Decode(bytes.NewReader(raw))
	if err != nil {
		return nil, "", false
	}
	if img.Bounds().Dx() > maxEdge || img.Bounds().Dy() > maxEdge {
		img = imaging.Fit(img, maxEdge, maxEdge, imaging.Lanczos)
	}
	if !preferJPEG {
		var buf bytes.Buffer
		if err := png.Encode(&buf, img); err == nil && buf.Len() > 0 && buf.Len() <= maxOut {
			return buf.Bytes(), "image/png", true
		}
	}
	for _, quality := range []int{82, 72, 60} {
		var buf bytes.Buffer
		if err := jpeg.Encode(&buf, img, &jpeg.Options{Quality: quality}); err == nil && buf.Len() > 0 && buf.Len() <= maxOut {
			return buf.Bytes(), "image/jpeg", true
		}
	}
	return nil, "", false
}
