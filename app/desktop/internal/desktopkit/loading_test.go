package desktopkit

import (
	"testing"

	"github.com/egoist/mygo/ui"
)

func TestSmoothLoadingAndHighlightComponents(t *testing.T) {
	layEach(t, func(t *testing.T, k Kit) {
		var listSt ListState
		var gridSt ListState
		active := true
		tt := ui.NewTesterWith(func(c *ui.Context) {
			k.Apply(c)
			k.SplitView(c, SplitViewProps{
				Title:          "全部历史",
				Subtitle:       "12",
				SubtitleMuted:  active,
				ToolbarLoading: active,
			}, func() {
				ui.Column(c).Grow(1).Gap(8).Padding(12).Children(func() {
					k.HighlightedText(c.Key("single"), "Desktop Alpha build 4 release", []string{"release"}, HighlightTextProps{
						FontSize:   13,
						FontWeight: 600,
						SingleLine: true,
					})
					k.HighlightedText(c.Key("code"), "go test -race ./app/desktop/...", []string{"desktop"}, HighlightTextProps{
						FontSize:  12,
						Monospace: true,
						MaxLines:  3,
						OnDark:    true,
					})
					k.ShimmerBox(c.Key("skel"), 8).Size(120, 18)
					k.GroupedList(c.Key("list"), &listSt, GroupedListProps{
						Label:  "历史条目",
						Groups: []ListGroup{{ID: "today", Title: "今天", Count: 2}},
						Items: []ItemListItem{
							{
								ID: "img1", Title: "BoardUI_Spec.png", CardRow: true,
								Kind: KindImage, ImageLoading: true, Highlight: []string{"Spec"},
							},
							{
								ID: "txt1", Title: "Release notes", Excerpt: "Verify release checklist",
								CardRow: true, Kind: KindText, Highlight: []string{"release"},
							},
						},
					}).Height(140)
					k.GroupedGrid(c.Key("grid"), &gridSt, GroupedGridProps{
						Label:  "历史网格",
						Groups: []ListGroup{{ID: "today", Title: "今天", Count: 2}},
						Items: []GridItem{
							{
								ID: "g1", Title: "Screenshot_Spec.png", Kind: KindImage,
								ImageLoading: true, Highlight: []string{"Spec"},
							},
							{
								ID: "g2", Title: "Build command", Text: "docker compose up --build",
								Kind: KindCode, Highlight: []string{"docker"},
							},
						},
					}).Height(180)
				})
			})
		}, 920, 620, ui.TesterOptions{Dark: k.Dark})

		if !tt.HasText("12") {
			t.Fatalf("muted subtitle missing: %v", tt.Texts())
		}
		if !tt.HasText("Desktop Alpha build 4 release") {
			t.Fatalf("highlighted single-line text missing: %v", tt.Texts())
		}
		if !tt.HasText("Verify release checklist") {
			t.Fatalf("highlighted list excerpt missing: %v", tt.Texts())
		}
		if !tt.HasText("docker compose up --build") {
			t.Fatalf("highlighted grid code text missing: %v", tt.Texts())
		}

		// Verify rendering cleanly settles when loading turns off.
		active = false
		tt.Frame()
		_ = tt.Image()
	})
}
