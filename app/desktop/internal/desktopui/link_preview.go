package desktopui

import (
	"context"
	"encoding/json"
	"io"
	"strings"

	"github.com/egoist/mygo/ui"

	"github.com/uniclipboard/unic/app/desktop/internal/desktopkit"
	api "github.com/uniclipboard/unic/internal/localapi"
	"github.com/uniclipboard/unic/plugin"
)

func bitmapDrawOver(p *historyImage) func(*ui.Painter, ui.Rect) {
	if p != nil && !p.released {
		return p.afterAcceleratedPaint
	}
	return nil
}

func loadLinkPreview(ctx context.Context, client *api.Client, id string, edge int) *linkPreviewInfo {
	if client == nil {
		return nil
	}
	anns, err := client.HistoryAnnotations(ctx, id)
	if err != nil || len(anns) == 0 {
		return nil
	}
	for _, ann := range anns {
		if ann.Type != plugin.AnnotationLinkPreviewV1 {
			continue
		}
		var lp plugin.LinkPreviewV1
		if err := json.Unmarshal(ann.Payload, &lp); err != nil {
			continue
		}
		if strings.TrimSpace(lp.Title) == "" && strings.TrimSpace(lp.Description) == "" {
			continue
		}
		info := &linkPreviewInfo{
			siteName:    lp.SiteName,
			title:       lp.Title,
			description: lp.Description,
			canonical:   lp.CanonicalURL,
		}
		if lp.FaviconAsset != "" {
			if r, err := client.HistoryAnnotationAsset(ctx, id, string(ann.PluginID), ann.Type, lp.FaviconAsset); err == nil {
				raw, rerr := io.ReadAll(io.LimitReader(r, (64<<10)+1))
				_ = r.Close()
				if rerr == nil && len(raw) > 0 && len(raw) <= 64<<10 {
					if img, derr := ui.DecodeImage(raw); derr == nil {
						info.favicon = img
					}
				}
			}
		}
		if lp.ImageAsset != "" {
			if r, err := client.HistoryAnnotationAsset(ctx, id, string(ann.PluginID), ann.Type, lp.ImageAsset); err == nil {
				pic, _ := readPreviewImage(ctx, r, edge)
				_ = r.Close()
				if pic != nil {
					info.thumbnail = pic
				}
			}
		}
		return info
	}
	return nil
}

// renderLinkPreviewCard renders the unified link preview card in both the
// split detail pane and the grid floating drawer. When an OpenGraph cover
// image (og:image) is present, the card is anchored visually around the OG
// hero media with an integrated publisher badge, editorial summary and
// recessed bottom URL footer strip.
func (d *viewState) renderLinkPreviewCard(c *ui.Context, k desktopkit.Kit, v entry, linkBody string, highlights []string, cardBg, footerBg, border ui.Color) {
	host, _ := urlHostAndPath(linkBody)
	rawURL := strings.TrimSpace(linkBody)
	lp := v.linkPreview

	if lp == nil {
		// Standard fallback card when no link_preview.v1 annotation is present.
		ui.Column(c.Key("link-preview")).Padding(14, 16).Radius(12).Gap(10).
			Background(cardBg).Border(1, border).Shrink(0).Children(func() {
			if host != "" {
				bg, fg := k.TagTint("blue")
				ui.Row(c.Key("host")).Gap(6).AlignItems(ui.Center).Children(func() {
					ui.Box(c.Key("ico")).Size(18, 18).Radius(5).Background(bg).Center().Shrink(0).Children(func() {
						k.Icon(c, desktopkit.IconLink, 11).TextColor(fg)
					})
					k.HighlightedText(c.Key("h"), host, highlights, desktopkit.HighlightTextProps{
						FontSize:   12,
						FontWeight: 600,
						TextColor:  fg,
						SingleLine: true,
					})
				})
			}
			if title := d.entryTitle(v); title != "" && title != rawURL {
				k.HighlightedText(c.Key("t"), title, highlights, desktopkit.HighlightTextProps{
					FontSize:   15,
					FontWeight: 600,
					TextColor:  k.Color("text-primary"),
				})
			}
			ui.Box(c.Key("url-box")).Padding(9, 11).Radius(8).
				Background(footerBg).Border(1, border).Children(func() {
				k.HighlightedText(c, rawURL, highlights, desktopkit.HighlightTextProps{
					FontSize:  11.5,
					TextColor: k.Color("accent-500"),
					Monospace: true,
				})
			})
		})
		return
	}

	siteLabel := lp.siteName
	if siteLabel == "" {
		siteLabel = host
	}
	displayURL := rawURL
	if displayURL == "" {
		displayURL = lp.canonical
	}
	desc := shortText(lp.description, 180)
	hasOG := lp.thumbnail != nil && lp.thumbnail.bitmap != nil

	ui.Column(c.Key("link-preview")).Radius(12).Clip().
		Background(cardBg).Border(1, border).Shrink(0).Children(func() {
		if hasOG {
			// OG Hero stage: full-bleed cover image with a publisher pill anchored over the lower-left edge.
			hero := ui.Box(c.Key("og-hero")).FillWidth().Height(172).Clip().
				Background(footerBg).BorderWidth(0, 0, 1, 0).BorderColor(border)
			hero.Children(func() {
				imgEl := ui.Image(c.Key("og-img"), lp.thumbnail.bitmap).Fill().Fit(ui.Cover).Label(lp.title)
				if drawOver := bitmapDrawOver(lp.thumbnail); drawOver != nil {
					imgEl.DrawOver(drawOver)
				}
				if siteLabel != "" || lp.favicon != nil {
					pillBg := k.Color("background-primary-default").Alpha(0.88)
					if k.Dark {
						pillBg = k.Color("background-secondary-default").Alpha(0.90)
					}
					ui.Row(c.Key("og-site-pill")).Absolute().Left(10).Bottom(10).
						Padding(4, 9).Radius(999).Gap(6).AlignItems(ui.Center).
						Background(pillBg).Border(1, border).Children(func() {
						if lp.favicon != nil {
							ui.Image(c.Key("fav"), lp.favicon).Size(14, 14).Radius(3).Shrink(0).Fit(ui.Contain).Label(siteLabel)
						} else {
							k.Icon(c.Key("fav-ico"), desktopkit.IconLink, 11).TextColor(k.Color("accent-500"))
						}
						if siteLabel != "" {
							k.HighlightedText(c.Key("site"), siteLabel, highlights, desktopkit.HighlightTextProps{
								FontSize:   11.5,
								FontWeight: 600,
								TextColor:  k.Color("text-primary"),
								SingleLine: true,
							})
						}
					})
				}
			})
		}

		// Editorial content section (title + description; publisher header shown here when no OG hero).
		ui.Column(c.Key("lp-body")).Padding(12, 14).Gap(6).Children(func() {
			if !hasOG && (siteLabel != "" || lp.favicon != nil) {
				bg, fg := k.TagTint("blue")
				ui.Row(c.Key("host")).Gap(6).AlignItems(ui.Center).Children(func() {
					if lp.favicon != nil {
						ui.Image(c.Key("fav"), lp.favicon).Size(16, 16).Radius(4).Shrink(0).Fit(ui.Contain).Label(siteLabel)
					} else {
						ui.Box(c.Key("ico")).Size(18, 18).Radius(5).Background(bg).Center().Shrink(0).Children(func() {
							k.Icon(c, desktopkit.IconLink, 11).TextColor(fg)
						})
					}
					if siteLabel != "" {
						k.HighlightedText(c.Key("h"), siteLabel, highlights, desktopkit.HighlightTextProps{
							FontSize:   12,
							FontWeight: 600,
							TextColor:  fg,
							SingleLine: true,
						})
					}
				})
			}
			if lp.title != "" {
				k.HighlightedText(c.Key("t"), lp.title, highlights, desktopkit.HighlightTextProps{
					FontSize:   14.5,
					FontWeight: 600,
					TextColor:  k.Color("text-primary"),
				})
			}
			if desc != "" {
				k.HighlightedText(c.Key("desc"), desc, highlights, desktopkit.HighlightTextProps{
					FontSize:  12,
					TextColor: k.Color("text-secondary"),
				})
			}
		})

		// Integrated URL footer strip bound directly to the bottom of the OG/link card.
		if displayURL != "" {
			ui.Row(c.Key("url-strip")).Padding(9, 14).Gap(7).AlignItems(ui.Center).
				Background(footerBg).BorderWidth(1, 0, 0, 0).BorderColor(border).Children(func() {
				k.Icon(c.Key("url-ico"), desktopkit.IconLink, 11.5).TextColor(k.Color("accent-500")).Shrink(0)
				k.HighlightedText(c.Key("url-text"), displayURL, highlights, desktopkit.HighlightTextProps{
					FontSize:  11,
					TextColor: k.Color("accent-500"),
					Monospace: true,
				}).Grow(1).MinWidth(0)
			})
		}
	})
}
