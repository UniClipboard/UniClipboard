package desktopui

import (
	"encoding/json"
	"os"
	"path/filepath"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"

	proof "github.com/uniclipboard/unic/app/desktop/internal/e2eproof"
)

// Failure cases: Backspace restores badge text instead of deleting it; deleting
// a multi-tag badge only removes its last tag; deleting one badge clears others.
func scenarioSearchBadgeDelete(t *testing.T) {
	type result struct{ Skin, Action, Query, Tags, Source, Type string }
	var results []result
	defer func() {
		if dir := os.Getenv("UNIC_DESKTOP_E2E_CASE_ARTIFACTS"); dir != "" {
			if err := os.MkdirAll(dir, 0755); err != nil {
				t.Error(err)
				return
			}
			b, err := json.MarshalIndent(results, "", "  ")
			if err != nil {
				t.Error(err)
				return
			}
			if err := os.WriteFile(filepath.Join(dir, "search-badge-delete.json"), append(b, '\n'), 0644); err != nil {
				t.Error(err)
			}
		}
	}()
	for _, skin := range []string{"macos", "linux", "windows"} {
		for _, action := range []string{"backspace", "remove"} {
			t.Run(skin+"/"+action, func(t *testing.T) {
				t.Setenv("UNIC_UI_SKIN", skin)
				d := newDemo("history", "dark")
				for i := range d.tags {
					if d.tags[i].name == "设计" {
						d.tags[i].name = "DesignReviewAndInterfaceSpecification"
					}
				}
				tt := ui.NewTesterWith(d.view, 1000, 720, ui.TesterOptions{Dark: true})
				proof.Capture(t, tt)
				if err := tt.Click("搜索历史"); err != nil {
					t.Fatal(err)
				}
				tt.Advance(400 * time.Millisecond)
				for _, word := range []string{"/file", "#DesignReviewAndInterfaceSpecification", "#工作"} {
					tt.Type(word)
					tt.Key(0, ui.KeyEnter)
				}
				if len(selectedSearchTags(d.tag)) != 2 || d.kind != "文件" {
					t.Fatal("badge setup failed")
				}
				if action == "backspace" {
					tt.Key(0, ui.KeyBackspace)
				} else {
					if err := tt.Click("移除标签条件"); err != nil {
						t.Fatal(err)
					}
				}
				if d.tag != "" || d.kind != "文件" || d.search.buffer != "" || d.query != "" {
					t.Fatalf("badge was not deleted whole: tags=%q type=%q buffer=%q query=%q", d.tag, d.kind, d.search.buffer, d.query)
				}
				tt.Type("界面规范")
				if d.search.buffer != "界面规范" || len(d.filtered()) != 1 {
					t.Fatal("badge deletion lost input focus or the remaining condition")
				}
				results = append(results, result{skin, action, d.query, d.tag, d.source, d.kind})
			})
		}
	}
}
