package desktopui

import (
	"bytes"
	"image"
	"image/color"
	"image/png"
	"os"
	"path/filepath"
	"testing"

	"github.com/egoist/mygo/ui"

	"github.com/uniclipboard/unic/app/desktop/internal/desktopkit"
	"github.com/uniclipboard/unic/app/desktop/internal/desktopui/preview"
)

func syntheticBitmap(t *testing.T, w, h int, c color.RGBA) *ui.Bitmap {
	t.Helper()
	img := image.NewRGBA(image.Rect(0, 0, w, h))
	for y := 0; y < h; y++ {
		for x := 0; x < w; x++ {
			// Give a subtle gradient so the rendered OG cover is visually crisp.
			r := uint8(min(255, int(c.R)+x*40/max(1, w)))
			g := uint8(min(255, int(c.G)+y*30/max(1, h)))
			img.SetRGBA(x, y, color.RGBA{R: r, G: g, B: c.B, A: 255})
		}
	}
	var buf bytes.Buffer
	if err := png.Encode(&buf, img); err != nil {
		t.Fatal(err)
	}
	bmp, err := ui.DecodeBitmap(buf.Bytes())
	if err != nil {
		t.Fatal(err)
	}
	return bmp
}

func TestLinkPreviewDetailAndGridCardsRender(t *testing.T) {
	fav := syntheticBitmap(t, 32, 32, color.RGBA{R: 37, G: 99, B: 235, A: 255})
	og := syntheticBitmap(t, 640, 360, color.RGBA{R: 16, G: 135, B: 129, A: 255})

	data := preview.Desktop{
		Page:     "history",
		Theme:    "light",
		Layout:   "list",
		Selected: "l-og",
		Entries: []preview.Entry{
			{
				ID:     "l-og",
				Title:  "https://docs.unic.example/guide/enricher",
				Text:   "https://docs.unic.example/guide/enricher",
				Kind:   desktopkit.KindLink,
				Source: "local",
				Date:   "今天 17:40",
				Tags:   []string{"builtin:link"},
			},
			{
				ID:     "l-text-only",
				Title:  "https://blog.unic.example/notes",
				Text:   "https://blog.unic.example/notes",
				Kind:   desktopkit.KindLink,
				Source: "local",
				Date:   "今天 17:39",
				Tags:   []string{"builtin:link"},
			},
			{
				ID:     "l-fallback",
				Title:  "https://plain.unic.example/raw",
				Text:   "https://plain.unic.example/raw",
				Kind:   desktopkit.KindLink,
				Source: "local",
				Date:   "今天 17:38",
				Tags:   []string{"builtin:link"},
			},
		},
	}

	desk := NewPreview(data)
	desk.d.entries[0].linkPreview = &linkPreviewInfo{
		siteName:    "UniClipboard Docs",
		title:       "异步历史富化能力（Enricher）指南",
		description: "了解如何基于 OG 封面图构建语义链接卡片与派生搜索索引。",
		canonical:   "https://docs.unic.example/guide/enricher",
		favicon:     fav,
		thumbnail:   &historyImage{bitmap: og, width: 640, height: 360},
	}
	desk.d.entries[1].linkPreview = &linkPreviewInfo{
		siteName:    "Unic Blog",
		title:       "无封面图链接预览卡片",
		description: "仅展示站点图标、标题、简介与集成 URL 尾栏。",
		favicon:     fav,
	}

	snapDir := os.Getenv("UNIC_LINK_PREVIEW_SNAP_DIR")
	for _, layout := range []string{"list", "grid"} {
		for _, theme := range []string{"light", "dark"} {
			desk.d.layout = layout
			desk.d.theme = theme
			desk.d.detailOpen = true
			desk.d.inspector = true
			for _, tc := range []struct {
				id       string
				wantText string
			}{
				{id: "l-og", wantText: "异步历史富化能力（Enricher）指南"},
				{id: "l-text-only", wantText: "无封面图链接预览卡片"},
				{id: "l-fallback", wantText: "plain.unic.example"},
			} {
				desk.d.selected = tc.id
				desk.d.list.Select(tc.id)
				tt := ui.NewTesterWith(desk.d.view, 1100, 720, ui.TesterOptions{Dark: theme == "dark"})
				if !tt.HasText(tc.wantText) {
					t.Fatalf("layout=%s theme=%s id=%s: expected text %q", layout, theme, tc.id, tc.wantText)
				}
				img := tt.Image()
				if img == nil || img.Bounds().Dx() != 1100 || img.Bounds().Dy() != 720 {
					t.Fatalf("unexpected render output for layout=%s theme=%s id=%s", layout, theme, tc.id)
				}
				if snapDir != "" && tc.id == "l-og" {
					_ = os.MkdirAll(snapDir, 0o755)
					f, err := os.Create(filepath.Join(snapDir, "link-preview-"+layout+"-"+theme+".png"))
					if err == nil {
						_ = png.Encode(f, img)
						_ = f.Close()
					}
				}
			}
		}
	}
}
