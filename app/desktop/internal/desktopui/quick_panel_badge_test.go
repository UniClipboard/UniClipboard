package desktopui

import (
	"bytes"
	"image"
	"image/color"
	"image/png"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"

	"github.com/uniclipboard/unic/app/desktop/internal/desktopkit"
	"github.com/uniclipboard/unic/app/desktop/internal/desktopui/preview"
)

func testSolidPNG(t *testing.T, w, h int, fill color.RGBA) ui.ImageSource {
	t.Helper()
	img := image.NewRGBA(image.Rect(0, 0, w, h))
	for y := 0; y < h; y++ {
		for x := 0; x < w; x++ {
			img.SetRGBA(x, y, fill)
		}
	}
	var buf bytes.Buffer
	if err := png.Encode(&buf, img); err != nil {
		t.Fatal(err)
	}
	src, err := ui.DecodeImage(buf.Bytes())
	if err != nil {
		t.Fatal(err)
	}
	return src
}

func hasPixelColor(img *image.RGBA, target color.RGBA, tolerance int) bool {
	b := img.Bounds()
	for y := b.Min.Y; y < b.Max.Y; y++ {
		for x := b.Min.X; x < b.Max.X; x++ {
			c := img.RGBAAt(x, y)
			dr := int(c.R) - int(target.R)
			dg := int(c.G) - int(target.G)
			db := int(c.B) - int(target.B)
			if dr < 0 {
				dr = -dr
			}
			if dg < 0 {
				dg = -dg
			}
			if db < 0 {
				db = -db
			}
			if dr <= tolerance && dg <= tolerance && db <= tolerance {
				return true
			}
		}
	}
	return false
}

func TestQPFileExtensionBadge(t *testing.T) {
	cases := []struct {
		name  string
		entry qpEntry
		want  string
	}{
		{
			name:  "png file",
			entry: qpEntry{kind: "文件", title: "PixPin_2026-11-24_10-31-22.png", text: "PixPin_2026-11-24_10-31-22.png"},
			want:  "PNG",
		},
		{
			name:  "lost png file keeps png",
			entry: qpEntry{kind: "文件", title: "PixPin_2026-11-24_10-31-22.png", text: "PixPin_2026-11-24_10-31-22.png", lost: true},
			want:  "PNG",
		},
		{
			name:  "multi-file uses first file",
			entry: qpEntry{kind: "文件", title: "界面规范.pdf", text: "界面规范.pdf\n组件清单.csv\n桌面布局.png"},
			want:  "PDF",
		},
		{
			name:  "truncated title falls back to full text",
			entry: qpEntry{kind: "文件", title: "very_long_file_name…", text: "very_long_file_name_full.webp"},
			want:  "WEBP",
		},
		{
			name:  "title fallback when text empty",
			entry: qpEntry{kind: "文件", title: "release-alpha-4-artifacts.pdf"},
			want:  "PDF",
		},
		{
			name:  "numeric and five-letter extensions",
			entry: qpEntry{kind: "文件", title: "archive.7z", text: "archive.7z"},
			want:  "7Z",
		},
		{
			name:  "swift extension",
			entry: qpEntry{kind: "文件", title: "App.swift", text: "App.swift"},
			want:  "SWIFT",
		},
		{
			name:  "no extension",
			entry: qpEntry{kind: "文件", title: "Makefile", text: "Makefile"},
			want:  "",
		},
		{
			name:  "dotfile",
			entry: qpEntry{kind: "文件", title: ".gitignore", text: ".gitignore"},
			want:  "",
		},
		{
			name:  "path directory with dot and extensionless file",
			entry: qpEntry{kind: "文件", title: "/tmp/a.b/Makefile", text: "/tmp/a.b/Makefile"},
			want:  "",
		},
		{
			name:  "truncated title with ellipsis and empty text",
			entry: qpEntry{kind: "文件", title: "report.v1…"},
			want:  "",
		},
		{
			name:  "overly long extension falls back",
			entry: qpEntry{kind: "文件", title: "notes.markdown", text: "notes.markdown"},
			want:  "",
		},
	}

	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			if got := qpFileExtensionBadge(tc.entry); got != tc.want {
				t.Fatalf("qpFileExtensionBadge(%+v) = %q, want %q", tc.entry, got, tc.want)
			}
		})
	}
}

func TestQuickPanelResultRowFileBadges(t *testing.T) {
	p := &quickPanelState{
		theme:    "light",
		mode:     "history",
		visible:  true,
		hovered:  -1,
		keyboard: true,
		entries: []qpEntry{
			{id: "png", title: "PixPin_2026-11-24_10-31-22.png", text: "PixPin_2026-11-24_10-31-22.png", kind: "文件", source: "本机", age: "今天 10:31"},
			{id: "lost-jpg", title: "photo.jpg", text: "photo.jpg", kind: "文件", source: "本机", age: "今天 10:20", lost: true},
			{id: "no-ext", title: "Makefile", text: "Makefile", kind: "文件", source: "本机", age: "今天 10:10"},
			{id: "dir", title: "release-artifacts", text: "release-artifacts", kind: "文件夹", source: "本机", age: "今天 10:00"},
		},
	}
	p.applySearch()

	tt := ui.NewTesterWith(p.panel, 380, 366, ui.TesterOptions{})
	if !tt.HasText("PNG") {
		t.Fatal("expected PNG badge for .png file entry")
	}
	if !tt.HasText("JPG") {
		t.Fatal("expected JPG badge for lost .jpg file entry")
	}
	if !tt.HasText("文件") {
		t.Fatal("expected fallback 文件 badge for extensionless file entry")
	}
	if !tt.HasText("文件夹") {
		t.Fatal("expected 文件夹 badge for folder entry")
	}
	if tt.HasText("PDF") {
		t.Fatal("unexpected hardcoded PDF badge for non-PDF files")
	}
	if tt.HasText("ZIP") {
		t.Fatal("unexpected hardcoded ZIP badge for lost non-ZIP file")
	}
}

func TestQuickPanelImageFilePreview(t *testing.T) {
	sentinel := color.RGBA{R: 22, G: 198, B: 88, A: 255}
	img := testSolidPNG(t, 48, 32, sentinel)

	p := &quickPanelState{
		theme:        "light",
		mode:         "history",
		visible:      true,
		hovered:      -1,
		keyboard:     true,
		previewImage: img,
		entries: []qpEntry{
			{id: "png", title: "PixPin_2026-11-24_10-31-22.png", text: "PixPin_2026-11-24_10-31-22.png", kind: "文件", source: "本机", age: "今天 10:31"},
			{id: "jpg", title: "photo.jpg", text: "photo.jpg", kind: "文件", source: "本机", age: "今天 10:25"},
			{id: "pdf", title: "spec.pdf", text: "spec.pdf", kind: "文件", source: "本机", age: "今天 10:20"},
			{id: "multi", title: "a.png", text: "a.png\nb.png", kind: "文件", source: "本机", age: "今天 10:15"},
		},
	}
	p.applySearch()

	if !p.quickHasImagePreview(p.rows[0]) {
		t.Fatal("expected single .png file to have image preview")
	}
	if !p.quickHasImagePreview(p.rows[1]) {
		t.Fatal("expected single .jpg file to have image preview")
	}
	if p.quickHasImagePreview(p.rows[2]) {
		t.Fatal("unexpected image preview for .pdf file")
	}
	if p.quickHasImagePreview(p.rows[3]) {
		t.Fatal("unexpected image preview for multi-file entry")
	}

	panel := ui.NewTesterWith(p.panel, 380, 366, ui.TesterOptions{})
	if !hasPixelColor(panel.Image(), sentinel, 8) {
		t.Fatal("expected quick panel row to render image thumbnail for .png file")
	}

	panel.Advance(510 * time.Millisecond)
	if p.previewID != "png" {
		t.Fatalf("previewID = %q, want png", p.previewID)
	}
	sat := ui.NewTesterWith(p.satellite, 400, 310, ui.TesterOptions{})
	if !hasPixelColor(sat.Image(), sentinel, 8) {
		t.Fatal("expected satellite preview to render image for .png file")
	}
	if !sat.HasText("PixPin_2026-11-24_10-31-22.png") {
		t.Fatal("expected satellite image preview to keep filename caption")
	}
	sat.Move(200, 140)
	sat.Advance(160 * time.Millisecond)
	if !sat.HasText("原始尺寸") {
		t.Fatal("expected image zoom control on hover for .png file")
	}

	panel.Key(0, ui.KeyDown)
	panel.Advance(130 * time.Millisecond)
	if p.previewID != "jpg" {
		t.Fatalf("previewID = %q, want jpg", p.previewID)
	}
	sat.Frame()
	sat.Move(200, 140)
	sat.Advance(160 * time.Millisecond)
	if !sat.HasText("800 × 500 · JPG · 来自 本机") {
		t.Fatal("expected satellite overlay to display JPG format for .jpg file")
	}

	panel.Key(0, ui.KeyDown)
	panel.Advance(130 * time.Millisecond)
	if p.previewID != "pdf" {
		t.Fatalf("previewID = %q, want pdf", p.previewID)
	}
	sat.Frame()
	if hasPixelColor(sat.Image(), sentinel, 8) {
		t.Fatal("unexpected image preview pixels for .pdf file")
	}
	if !sat.HasText("spec.pdf") {
		t.Fatal("expected normal file preview for .pdf file")
	}
}

func TestMainWindowImageFilePreviews(t *testing.T) {
	sentinel := color.RGBA{R: 230, G: 44, B: 145, A: 255}
	img := testSolidPNG(t, 64, 48, sentinel)

	for _, layout := range []string{"list", "grid"} {
		t.Run(layout, func(t *testing.T) {
			app := NewPreview(preview.Desktop{
				Page:     "history",
				Theme:    "light",
				Layout:   layout,
				Selected: "pdf",
				Image:    img,
				Entries: []preview.Entry{
					{
						ID:     "png-file",
						Title:  "PixPin_2026-11-24_10-31-22.png",
						Text:   "PixPin_2026-11-24_10-31-22.png",
						Source: "local",
						Date:   "今天 10:31",
						Size:   "128 KB",
						Kind:   desktopkit.KindFile,
					},
					{
						ID:     "pdf",
						Title:  "spec.pdf",
						Text:   "spec.pdf",
						Source: "local",
						Date:   "今天 10:20",
						Size:   "2.4 MB",
						Kind:   desktopkit.KindFile,
					},
				},
			})
			tt := ui.NewTesterWith(app.d.view, 960, 640, ui.TesterOptions{})
			if !hasPixelColor(tt.Image(), sentinel, 8) {
				t.Fatalf("expected %s layout to render preview image for single .png file entry", layout)
			}
		})
	}
}
