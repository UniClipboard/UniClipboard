package desktopui

import (
	"encoding/json"
	"fmt"
	"os"
	"path/filepath"
	"regexp"
	"strconv"
	"strings"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"

	proof "github.com/uniclipboard/unic/app/desktop/internal/e2eproof"
)

// Failure inventory: missing catalogue entries; lost format arguments;
// translated content, device names, tags or filter IDs; stale navigation after
// switching; unsupported persisted languages; selection not applied.
func scenarioI18n(t *testing.T) {
	raw, err := os.ReadFile("../i18n/locales.json")
	if err != nil {
		t.Fatal(err)
	}
	var translations map[string]map[string]string
	if err := json.Unmarshal(raw, &translations); err != nil {
		t.Fatal(err)
	}
	formats := regexp.MustCompile(`%[-+.#0-9]*[dfs]`)
	for key, values := range translations {
		for _, code := range []string{"en", "ja", "pt-BR", "ru"} {
			text := values[code]
			if text == "" {
				t.Fatalf("missing translation %s: %s", code, key)
			}
			if strings.Join(formats.FindAllString(text, -1), ",") != strings.Join(formats.FindAllString(key, -1), ",") {
				t.Fatalf("format mismatch %s: %s", code, key)
			}
		}
	}
	sources, err := filepath.Glob("*.go")
	if err != nil {
		t.Fatal(err)
	}
	literals := regexp.MustCompile(`\.tr\(("(?:[^"\\]|\\.)*")\)`)
	for _, path := range sources {
		if strings.HasSuffix(path, "_test.go") {
			continue
		}
		source, err := os.ReadFile(path)
		if err != nil {
			t.Fatal(err)
		}
		for _, match := range literals.FindAllSubmatch(source, -1) {
			key, err := strconv.Unquote(string(match[1]))
			if err != nil {
				t.Fatal(err)
			}
			if translations[key] == nil {
				t.Fatalf("uncatalogued source in %s: %s", path, key)
			}
		}
	}
	for _, skin := range []string{"macos", "linux", "windows"} {
		for _, theme := range []string{"light", "dark"} {
			for _, language := range []struct{ code, general, history, native string }{
				{"zh-CN", "通用", "资料库", "简体中文"}, {"en", "General", "Library", "English"},
				{"ja", "一般", "ライブラリ", "日本語"}, {"pt-BR", "Geral", "Biblioteca", "Português (Brasil)"}, {"ru", "Общие", "Библиотека", "Русский"},
			} {
				t.Run(skin+"/"+theme+"/"+language.code, func(t *testing.T) {
					t.Setenv("UNIC_UI_SKIN", skin)
					d := newDemo("settings", theme)
					tt := ui.NewTesterWith(d.view, 1100, 760, ui.TesterOptions{Dark: theme == "dark"})
					proof.Capture(t, tt)
					role := ui.RolePopUpButton
					target, ok := tt.AccessByRole(role, "界面语言")
					if !ok {
						t.Fatal("language selector missing")
					}
					x, y := target.Bounds.X+target.Bounds.W/2, target.Bounds.Y+target.Bounds.H/2
					tt.Press(x, y)
					tt.Release(x, y)
					if skin == "macos" {
						if err := tt.ChooseMenuItem(language.native); err != nil {
							t.Fatal(err)
						}
					} else {
						if err := tt.Click(language.native); err != nil {
							t.Fatal(err)
						}
					}
					tt.Advance(400 * time.Millisecond)
					if d.currentLanguage() != language.code {
						t.Fatalf("selection not applied: %s", d.currentLanguage())
					}
					for id, source := range map[string]string{"builtin:text": "文本", "builtin:image": "图片", "builtin:files": "文件", "builtin:link": "链接"} {
						if tag := d.tagByID(id); tag == nil || !tag.builtin || d.tagLabel(id) != d.tr(source) {
							t.Fatalf("builtin label/identity mismatch: %s", id)
						}
					}
					if !tt.HasText(language.general) {
						t.Fatalf("missing settings navigation: %s", language.general)
					}
					t.Run("settings", func(t *testing.T) {
						proof.Capture(t, tt)
						if !tt.HasText(d.tr("界面语言")) {
							t.Fatal("localized language setting missing")
						}
						// Failures: duplicate visible switch captions; missing
						// accessible name; mouse or keyboard no longer toggles.
						for _, item := range []struct{ key, label string }{
							{"startup", "登录时启动"}, {"tray", "显示托盘图标"},
							{"sound", "交互声音"}, {"telemetry", "发送匿名使用数据"},
						} {
							label := d.tr(item.label)
							count := 0
							for _, text := range tt.Texts() {
								if text == label {
									count++
								}
							}
							// Texts includes the accessible switch name in addition
							// to the visible row caption; a drawn duplicate adds a third.
							if count != 2 {
								t.Fatalf("expected row caption plus accessible switch name for %s, got %d", label, count)
							}
							sw, ok := tt.AccessByRole(ui.RoleSwitch, label)
							if !ok {
								t.Fatalf("switch accessible name missing: %s", label)
							}
							if sw.Bounds.W > 56 {
								t.Fatalf("switch contains extra visible content: %s (%g points)", label, sw.Bounds.W)
							}
							before := d.options[item.key]
							x, y := sw.Bounds.X+sw.Bounds.W/2, sw.Bounds.Y+sw.Bounds.H/2
							tt.Press(x, y)
							tt.Release(x, y)
							if d.options[item.key] == before {
								t.Fatalf("switch click failed: %s", label)
							}
							tt.Key(0, ui.KeySpace)
							if d.options[item.key] != before {
								t.Fatalf("switch keyboard failed: %s", label)
							}
						}

					})
					if err := tt.Click(d.tr("返回历史")); err != nil {
						t.Fatal(err)
					}
					tt.Advance(400 * time.Millisecond)
					if !tt.HasText(language.history) {
						t.Fatalf("missing library: %s", language.history)
					}
					if d.entries[0].title != "周一产品讨论" || d.devices[1].name != "工作电脑" || d.tags[0].name != "工作" {
						t.Fatal("user data changed")
					}
					d.kind = "文件"
					tt.Advance(20 * time.Millisecond)
					if len(d.filtered()) != 1 || d.filtered()[0].id != "files" {
						t.Fatal("localized filter changed internal identity")
					}
					if strings.Contains(fmt.Sprintf(d.tr("已选择 %d 项"), 2), "%!") {
						t.Fatal("invalid formatted label")
					}
					p := newQuickDemo(theme, "history")
					p.language = language.code
					qt := ui.NewTesterWith(p.panel, 380, 366, ui.TesterOptions{Dark: theme == "dark"})
					proof.Capture(t, qt)
					if !qt.HasText(p.tr("操作")) {
						t.Fatal("quick panel language missing")
					}
					p.menu = "main"
					st := ui.NewTesterWith(p.satellite, 380, 360, ui.TesterOptions{Dark: theme == "dark"})
					proof.Capture(t, st)
					if !st.HasText(p.tr("复制到剪贴板")) && !st.HasText(p.tr("只复制")) {
						t.Fatal("quick actions language missing")
					}
				})
			}
		}
	}
}
