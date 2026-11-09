package desktopdemo

import (
	"encoding/json"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"
	"unicode/utf8"

	"github.com/egoist/mygo/ui"
)

// Failure cases: syntax is sent as literal text; candidate selection loses
// surrounding text or focus; counts ignore other conditions; chips drift from
// filter controls; Backspace deletes instead of editing; Escape clears too soon.
// Drive the full demo with real typing, pointer and keyboard events. No native
// application is launched; this verifies the event/layout/rendering boundary.
func TestTokenSearchE2E(t *testing.T) {
	type sample struct {
		Skin, Theme, Scenario, Query, Source, Tags, Type, Time string
		Results                                                int
	}
	var results []sample
	defer func() {
		if dir := os.Getenv("UNIC_UI_TEST_ARTIFACTS"); dir != "" {
			if err := os.MkdirAll(dir, 0755); err != nil {
				t.Error(err)
				return
			}
			b, err := json.MarshalIndent(results, "", "  ")
			if err != nil {
				t.Error(err)
				return
			}
			if err := os.WriteFile(filepath.Join(dir, "token-search.json"), append(b, '\n'), 0644); err != nil {
				t.Error(err)
			}
		}
	}()
	for _, skin := range []string{"macos", "linux", "windows"} {
		for _, theme := range []string{"dark", "light"} {
			for _, scenario := range []string{"device", "tags", "type", "time", "extension", "middle", "literal", "paste", "pointer", "quote", "filters", "keyboard", "escape", "zero"} {
				t.Run(skin+"/"+theme+"/"+scenario, func(t *testing.T) {
					t.Setenv("UNIC_UI_SKIN", skin)
					d := newDemo("history", theme)
					tt := ui.NewTesterWith(d.view, 1100, 720, ui.TesterOptions{Dark: theme == "dark"})
					if err := tt.Click("搜索历史"); err != nil {
						t.Fatal(err)
					}
					tt.Advance(400 * time.Millisecond)
					choose := func(label string) {
						t.Helper()
						if _, ok := tt.AccessByRole(ui.RoleOption, label); !ok {
							t.Fatalf("candidate missing: %s", label)
						}
						tt.Key(0, ui.KeyEnter)
					}
					switch scenario {
					case "device":
						tt.Type("@Linux")
						choose("Linux 工作站")
						if d.source != "linux" || d.query != "" || len(d.filtered()) != 2 {
							t.Fatal("device condition not applied")
						}
						tt.Key(0, ui.KeyBackspace)
						if d.source != "" || d.search.buffer != "" {
							t.Fatal("Backspace did not delete the whole condition")
						}
						tt.Type("@Linux")
						tt.Key(0, ui.KeyTab)
						if d.source != "linux" {
							t.Fatal("Tab did not complete device")
						}
					case "tags":
						tt.Type("#设计")
						choose("设计")
						if len(d.filtered()) != 2 {
							t.Fatal("tag filter not applied")
						}
						tt.Type("#工作")
						if !tt.HasText("1 项") {
							t.Fatal("candidate count ignores the existing tag")
						}
						choose("工作")
						if len(d.filtered()) != 1 || d.filtered()[0].id != "files" {
							t.Fatal("tag intersection not applied")
						}
						tt.Key(0, ui.KeyBackspace)
						if d.tag != "" || d.search.buffer != "" {
							t.Fatal("Backspace did not delete the whole tag badge")
						}
					case "type":
						tt.Type("/image")
						choose("图片")
						if d.kind != "图片" || len(d.filtered()) != 1 {
							t.Fatal("type condition not applied")
						}
						if err := tt.Click("移除类型条件"); err != nil {
							t.Fatal(err)
						}
						if len(d.filtered()) != 22 {
							t.Fatal("removing type chip did not reset filter")
						}
					case "time":
						tt.Type("on:yesterday")
						choose("昨天")
						if d.timeRange != "yesterday" || len(d.filtered()) != 14 {
							t.Fatal("time condition not applied")
						}
					case "extension":
						tt.Type("ext:png")
						choose(".png")
						if len(d.filtered()) != 2 {
							t.Fatal("extension condition not applied")
						}
						if err := tt.Click("清除搜索"); err != nil {
							t.Fatal(err)
						}
						if len(d.filtered()) != 22 {
							t.Fatal("clear did not reset extension")
						}
					case "middle":
						tt.Type("HTTP @Lin 服务示例")
						for i := 0; i < utf8.RuneCountInString(" 服务示例"); i++ {
							tt.Key(0, ui.KeyLeft)
						}
						if _, ok := tt.AccessByRole(ui.RoleOption, "Linux 工作站"); !ok {
							t.Fatal("caret did not activate middle condition")
						}
						tt.Key(0, ui.KeyTab)
						if strings.Join(strings.Fields(d.query), " ") != "HTTP 服务示例" || d.source != "linux" || len(d.filtered()) != 1 {
							t.Fatal("completion lost surrounding text")
						}
						tt.Type("X")
						if d.search.buffer != "HTTP X 服务示例" {
							t.Fatalf("completion lost caret position: %q", d.search.buffer)
						}
					case "literal":
						tt.Type("/tmp/example @nobody")
						tt.Key(0, ui.KeyEnter)
						if d.query != "/tmp/example @nobody" || d.source != "" || d.kind != "" {
							t.Fatal("unknown syntax did not remain literal")
						}
					case "paste":
						tt.Type("/image #设计 ext:png ")
						if d.kind != "图片" || d.tag != "设计" || len(d.filtered()) != 1 {
							t.Fatal("completed pasted conditions were not applied")
						}
					case "pointer":
						tt.Type("@Linux")
						option, ok := tt.AccessByRole(ui.RoleOption, "Linux 工作站")
						if !ok {
							t.Fatal("source option missing")
						}
						tt.ClickAt(option.Bounds.X+option.Bounds.W/2, option.Bounds.Y+option.Bounds.H/2)
						tt.Type("HTTP 服务示例")
						if d.source != "linux" || d.query != "HTTP 服务示例" || len(d.filtered()) != 1 {
							t.Fatalf("pointer completion: source=%q query=%q buffer=%q results=%d", d.source, d.query, d.search.buffer, len(d.filtered()))
						}
					case "quote":
						text := `"@Linux" "/image" \#设计 `
						tt.Type(text)
						tt.Key(0, ui.KeyEnter)
						if d.query != text || d.source != "" || d.kind != "" || d.tag != "" {
							t.Fatal("quoted or escaped words became conditions")
						}
					case "filters":
						tt.Key(0, ui.KeyEscape)
						if err := tt.Click("来源设备"); err != nil {
							t.Fatal(err)
						}
						var err error
						if skin == "macos" {
							err = tt.ChooseMenuItem("Linux 工作站")
						} else {
							err = tt.Click("Linux 工作站")
						}
						if err != nil {
							t.Fatal(err)
						}
						if d.source != "linux" || len(d.filtered()) != 2 {
							t.Fatal("source filter did not apply")
						}
						if err := tt.Click("移除设备条件"); err != nil {
							t.Fatal(err)
						}
						if d.source != "" || len(d.filtered()) != 22 {
							t.Fatal("chip and filter control drifted")
						}
					case "keyboard":
						tt.Type("@")
						tt.Key(0, ui.KeyUp)
						tt.Key(0, ui.KeyTab)
						if d.source != "phone" {
							t.Fatal("Up did not wrap to the last candidate")
						}
						if err := tt.Click("清除搜索"); err != nil {
							t.Fatal(err)
						}
						active, _ := tt.TextInputActive()
						if !active {
							t.Fatal("clearing conditions lost input focus")
						}
						tt.Type("@")
						tt.Key(0, ui.KeyDown)
						tt.Key(0, ui.KeyEnter)
						if d.source != "windows" {
							t.Fatalf("Down choice: source=%q buffer=%q highlight=%d", d.source, d.search.buffer, d.search.highlight)
						}
					case "escape":
						tt.Type("#设计")
						choose("设计")
						tt.Type("@Lin")
						tt.Key(0, ui.KeyEscape)
						if d.tag != "设计" || d.query != "@Lin" {
							t.Fatal("first Escape did not preserve text and filters")
						}
						tt.Key(0, ui.KeyEscape)
						if d.query != "" || d.tag != "" || len(d.filtered()) != 22 {
							t.Fatal("second Escape did not clear the search")
						}
						tt.Key(0, ui.KeyEscape)
						tt.Advance(300 * time.Millisecond)
						if skin == "macos" {
							info, _ := tt.AccessByRole(ui.RoleSearch, "")
							if info.Bounds.W != 32 {
								t.Fatal("third Escape did not collapse search")
							}
						}
					case "zero":
						tt.Type("/richtext")
						choose("富文本")
						if d.kind != "富文本" || len(d.filtered()) != 0 {
							t.Fatal("zero-count candidate did not apply")
						}
						if err := tt.Click("清除搜索"); err != nil {
							t.Fatal(err)
						}
						if len(d.filtered()) != 22 {
							t.Fatal("zero-result search could not be cleared")
						}
					}
					results = append(results, sample{skin, theme, scenario, d.query, d.source, d.tag, d.kind, d.timeRange, len(d.filtered())})
				})
			}
		}
	}
}
