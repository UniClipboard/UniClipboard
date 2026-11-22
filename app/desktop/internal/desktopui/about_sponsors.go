package desktopui

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"net/http"
	"net/url"
	"os"
	"strings"
	"time"
	"unicode"
	"unicode/utf8"

	"github.com/egoist/mygo/ui"

	"github.com/uniclipboard/unic/internal/releaseinfo"
)

const (
	defaultSponsorsAPIURL = "https://www.uniclipboard.app/api/v1/sponsors"
	sponsorChannelURL     = "https://afdian.com/a/mkdir700"
	githubRepositoryURL   = "https://github.com/UniClipboard/UniClipboard"
	privacyPolicyURL      = "https://github.com/UniClipboard/UniClipboard"
	termsOfServiceURL     = "https://github.com/UniClipboard/UniClipboard"
	aboutMaintainersURL   = "https://github.com/UniClipboard/UniClipboard/blob/main/ABOUT.md"
	releaseDownloadURL    = "https://uniclipboard.app/download"

	sponsorsFetchTimeout  = 8 * time.Second
	maxSponsorsBodyBytes  = 1 << 20
	maxSponsorAvatarBytes = 2 << 20
	maxSponsorsCount      = 64
)

// Sponsor matches the public website feed returned by GET /api/v1/sponsors.
type Sponsor struct {
	ID     string `json:"id"`
	Name   string `json:"name"`
	URL    string `json:"url,omitempty"`
	Since  string `json:"since,omitempty"`
	Note   string `json:"note,omitempty"`
	Avatar string `json:"avatar,omitempty"`
	Tier   string `json:"tier,omitempty"`
}

type sponsorsResponse struct {
	Items []Sponsor `json:"items"`
	Count int       `json:"count"`
}

func allowedSponsorURL(raw string) bool {
	u, err := url.Parse(strings.TrimSpace(raw))
	if err != nil || u.User != nil {
		return false
	}
	if u.Scheme == "https" && u.Host != "" {
		return true
	}
	if u.Scheme == "http" {
		host := u.Hostname()
		return host == "127.0.0.1" || host == "::1" || host == "localhost"
	}
	return false
}

func allowedExternalURL(raw string) bool {
	u, err := url.Parse(strings.TrimSpace(raw))
	if err != nil || u.User != nil || u.Host == "" {
		return false
	}
	return u.Scheme == "https" || u.Scheme == "http"
}

func sponsorHTTPClient() *http.Client {
	return &http.Client{
		Timeout: sponsorsFetchTimeout,
		CheckRedirect: func(req *http.Request, via []*http.Request) error {
			if len(via) > 5 || !allowedSponsorURL(req.URL.String()) {
				return errors.New("unsafe redirect")
			}
			return nil
		},
	}
}

func fetchSponsorsHTTP(ctx context.Context) ([]Sponsor, error) {
	endpoint := strings.TrimSpace(os.Getenv("UNIC_SPONSORS_API_URL"))
	if endpoint == "" {
		endpoint = defaultSponsorsAPIURL
	}
	if !allowedSponsorURL(endpoint) {
		return nil, errors.New("invalid sponsors endpoint")
	}
	reqCtx, cancel := context.WithTimeout(ctx, sponsorsFetchTimeout)
	defer cancel()
	req, err := http.NewRequestWithContext(reqCtx, http.MethodGet, endpoint, nil)
	if err != nil {
		return nil, err
	}
	req.Header.Set("Accept", "application/json")
	req.Header.Set("User-Agent", "UniClipboard/"+releaseinfo.Version)
	resp, err := sponsorHTTPClient().Do(req)
	if err != nil {
		return nil, err
	}
	defer func() { _ = resp.Body.Close() }()
	if resp.StatusCode != http.StatusOK {
		return nil, fmt.Errorf("sponsors fetch failed: %d", resp.StatusCode)
	}
	data, err := io.ReadAll(io.LimitReader(resp.Body, maxSponsorsBodyBytes+1))
	if err != nil {
		return nil, err
	}
	if len(data) > maxSponsorsBodyBytes {
		return nil, errors.New("sponsors response exceeds limit")
	}
	return parseSponsorsJSON(data)
}

func parseSponsorsJSON(data []byte) ([]Sponsor, error) {
	var payload sponsorsResponse
	if err := json.Unmarshal(data, &payload); err != nil {
		return nil, err
	}
	out := make([]Sponsor, 0, min(len(payload.Items), maxSponsorsCount))
	for i, item := range payload.Items {
		if len(out) >= maxSponsorsCount {
			break
		}
		item.Name = strings.TrimSpace(item.Name)
		if item.Name == "" {
			continue
		}
		if item.ID == "" {
			item.ID = fmt.Sprintf("sponsor-%d", i)
		}
		out = append(out, item)
	}
	return out, nil
}

func fetchSponsorAvatarHTTP(ctx context.Context, rawURL string) ([]byte, error) {
	if !allowedSponsorURL(rawURL) {
		return nil, errors.New("invalid avatar URL")
	}
	reqCtx, cancel := context.WithTimeout(ctx, sponsorsFetchTimeout)
	defer cancel()
	req, err := http.NewRequestWithContext(reqCtx, http.MethodGet, rawURL, nil)
	if err != nil {
		return nil, err
	}
	req.Header.Set("User-Agent", "UniClipboard/"+releaseinfo.Version)
	resp, err := sponsorHTTPClient().Do(req)
	if err != nil {
		return nil, err
	}
	defer func() { _ = resp.Body.Close() }()
	if resp.StatusCode != http.StatusOK {
		return nil, fmt.Errorf("avatar fetch failed: %d", resp.StatusCode)
	}
	data, err := io.ReadAll(io.LimitReader(resp.Body, maxSponsorAvatarBytes+1))
	if err != nil {
		return nil, err
	}
	if len(data) == 0 || len(data) > maxSponsorAvatarBytes {
		return nil, errors.New("avatar exceeds size limit")
	}
	return data, nil
}

// sponsorInitials returns a deterministic 1-2 character monogram for sponsors without an avatar.
func sponsorInitials(name string) string {
	trimmed := strings.TrimSpace(name)
	if trimmed == "" {
		return "?"
	}
	ascii := true
	for _, r := range trimmed {
		if r >= 128 {
			ascii = false
			break
		}
	}
	if ascii {
		parts := strings.Fields(trimmed)
		if len(parts) >= 2 {
			return strings.ToUpper(parts[0][:1] + parts[1][:1])
		}
		if len(trimmed) >= 2 {
			return strings.ToUpper(trimmed[:2])
		}
		return strings.ToUpper(trimmed)
	}
	r, _ := utf8.DecodeRuneInString(trimmed)
	if r == utf8.RuneError {
		return "?"
	}
	return string(unicode.ToUpper(r))
}

func (d *viewState) ensureSponsorsLoaded() {
	if d.live == nil || d.about.sponsorsStarted {
		return
	}
	d.about.sponsorsStarted = true
	fetcher := d.about.fetchSponsors
	if fetcher == nil {
		fetcher = fetchSponsorsHTTP
	}
	l := d.live
	go func() {
		items, err := fetcher(l.ctx)
		if err != nil {
			items = nil
		}
		apply := func() {
			d.about.sponsorsLoaded = true
			d.about.sponsors = items
			d.startSponsorAvatarLoads()
		}
		select {
		case l.results <- apply:
			l.wake()
		case <-l.ctx.Done():
		}
	}()
}

func (d *viewState) startSponsorAvatarLoads() {
	if d.live == nil || len(d.about.sponsors) == 0 {
		return
	}
	if d.about.sponsorAvatars == nil {
		d.about.sponsorAvatars = map[string]ui.ImageSource{}
	}
	if d.about.sponsorAvatarStarted == nil {
		d.about.sponsorAvatarStarted = map[string]bool{}
	}
	fetcher := d.about.fetchAvatar
	if fetcher == nil {
		fetcher = fetchSponsorAvatarHTTP
	}
	l := d.live
	for _, s := range d.about.sponsors {
		avatarURL := strings.TrimSpace(s.Avatar)
		if avatarURL == "" || !allowedSponsorURL(avatarURL) || d.about.sponsorAvatarStarted[s.ID] {
			continue
		}
		d.about.sponsorAvatarStarted[s.ID] = true
		id := s.ID
		go func() {
			raw, err := fetcher(l.ctx, avatarURL)
			if err != nil || len(raw) == 0 {
				return
			}
			img, err := ui.DecodeImage(raw)
			if err != nil || img == nil {
				return
			}
			apply := func() {
				if d.about.sponsorAvatars == nil {
					d.about.sponsorAvatars = map[string]ui.ImageSource{}
				}
				d.about.sponsorAvatars[id] = img
			}
			select {
			case l.results <- apply:
				l.wake()
			case <-l.ctx.Done():
			}
		}()
	}
}

func demoSponsors() []Sponsor {
	return []Sponsor{
		{ID: "demo-gold", Name: "Ada Lovelace", Tier: "gold", URL: sponsorChannelURL},
		{ID: "demo-2", Name: "Linus Torvalds", Note: "Keep shipping!", Tier: "regular", URL: githubRepositoryURL},
		{ID: "demo-3", Name: "云端用户", Note: "跨设备同步很好用", Tier: "regular"},
	}
}
