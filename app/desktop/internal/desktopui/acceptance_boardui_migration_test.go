package desktopui

// Failure inventory (fixed before production migration implementation):
// - a production command/renderer/preview still imports the removed UI Kit;
// - a legacy OS-skin selector or environment override still changes appearance;
// - a move of the old Kit merely renames its platform skins;
// - history/settings/quick-panel retain a system font or old semantic palette;
// - settings navigation, caller state or return-to-history stops working;
// - a dark/scale switch loses state, clips controls or restores the old skin;
// - synthetic data reaches a real profile, system clipboard or account;
// - a software screenshot is described as native-window/accessibility proof.
// This scenario crosses the complete production renderer with supplied fixtures.
// Existing first-install-setup and quick-panel scenarios remain independent
// companion acceptance; this new case does not delete or replace their evidence.

import (
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"fmt"
	"go/ast"
	"go/parser"
	"go/token"
	"image/png"
	"os"
	"path/filepath"
	"runtime"
	"slices"
	"strconv"
	"strings"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"

	"github.com/uniclipboard/unic/app/desktop/boardui"
)

func scenarioBoardUIProductionMigration(t *testing.T) {
	root, err := filepath.Abs("../../../..")
	if err != nil {
		t.Fatal(err)
	}
	out := os.Getenv("UNIC_DESKTOP_E2E_CASE_ARTIFACTS")
	if out == "" {
		t.Fatal("canonical artifact directory is required")
	}
	if err = os.MkdirAll(out, 0700); err != nil {
		t.Fatal(err)
	}
	receipt := map[string]any{
		"scope": "production renderer with synthetic supplied data; software view",
		"go":    runtime.Version(), "os": runtime.GOOS, "source_head": os.Getenv("UNIC_DESKTOP_E2E_SOURCE_HEAD"),
		"native_window": "NOT_RUN", "native_ime": "NOT_RUN", "native_accessibility_bridge": "NOT_RUN",
		"system_clipboard": "NOT_RUN", "companions": []string{"first-install-setup", "quick-panel", "production-boundary"},
	}
	checks := []string{}
	views := []map[string]any{}
	defer func() {
		receipt["passed"] = !t.Failed()
		receipt["checks"] = checks
		receipt["views"] = views
		b, e := json.MarshalIndent(receipt, "", "  ")
		if e != nil {
			t.Error(e)
			return
		}
		if e = os.WriteFile(filepath.Join(out, "boardui-production-migration.json"), append(b, '\n'), 0600); e != nil {
			t.Error(e)
		}
	}()
	sources, violations := boarduiMigrationSources(t, root)
	receipt["source_sha256"] = sources
	receipt["source_violations"] = violations
	if _, e := os.Stat(filepath.Join(root, "app/desktop/internal/uikit")); e == nil {
		violations = append(violations, "app/desktop/internal/uikit still exists")
	} else if !os.IsNotExist(e) {
		t.Fatal(e)
	}
	receipt["source_violations"] = violations
	for _, violation := range violations {
		t.Errorf("production BoardUI migration: %s", violation)
	}
	if len(violations) == 0 {
		checks = append(checks, "legacy Kit tree, imports and appearance selectors are absent from production source")
	}
	if err = boardui.LoadFonts(); err != nil {
		t.Fatal(err)
	}

	capture := func(tt *ui.Tester, name string) {
		t.Helper()
		file, e := os.Create(filepath.Join(out, name+".png"))
		if e != nil {
			t.Fatal(e)
		}
		encodeErr := png.Encode(file, tt.Image())
		closeErr := file.Close()
		if encodeErr != nil {
			t.Fatal(encodeErr)
		}
		if closeErr != nil {
			t.Fatal(closeErr)
		}
		views = append(views, map[string]any{"id": name, "accessibility": tt.AccessAll(), "width_px": tt.Image().Bounds().Dx(), "height_px": tt.Image().Bounds().Dy()})
	}
	validateTheme := func(t *testing.T, observed ui.Theme, dark bool, where string) {
		t.Helper()
		if observed.Font != boardui.FontFamily {
			t.Errorf("%s font=%q; want %q", where, observed.Font, boardui.FontFamily)
		}
		if observed.FontSize != boardui.Typography("body-regular").Size {
			t.Errorf("%s font size=%g; want BoardUI body", where, observed.FontSize)
		}
		for _, item := range []struct {
			name  string
			got   ui.Color
			token string
		}{
			{"background", observed.Background, "background-full"},
			{"text", observed.Text, "text-primary"},
			{"surface", observed.Surface, "background-primary-default"},
			{"border", observed.Border, "border-button-default"},
			{"focus", observed.Focus, "border-focus-ring"},
		} {
			if item.got != boardui.Token(dark, item.token) {
				t.Errorf("%s %s is not source-derived BoardUI %s", where, item.name, item.token)
			}
		}
	}
	for _, override := range []string{"linux", "windows", "macos"} {
		for _, dark := range []bool{false, true} {
			theme := "light"
			if dark {
				theme = "dark"
			}
			t.Run(override+"/"+theme, func(t *testing.T) {
				// These values are intentionally hostile old overrides. They must no
				// longer choose a skin on any host. They are not native OS acceptance.
				t.Setenv("UNIC_UI_SKIN", override)
				d := newDemo("history", theme)
				selected := d.selected
				titles := make([]string, len(d.entries))
				for i := range d.entries {
					titles[i] = d.entries[i].title
				}
				var observed ui.Theme
				view := func(c *ui.Context) { d.view(c); observed = *c.Theme() }
				tt := ui.NewTesterWith(view, 1280, 840, ui.TesterOptions{Dark: dark})
				if dark {
					tt.SetScale(2)
				}
				label := override + "-" + theme
				validateTheme(t, observed, dark, label+" history")
				capture(tt, label+"-history")
				if d.page != "history" || !tt.HasText(d.tr("资料库")) {
					t.Fatal("history view did not open")
				}
				if e := tt.Click(d.tr("设置…")); e != nil {
					t.Fatal(e)
				}
				tt.Advance(500 * time.Millisecond)
				if d.page != "settings" || !tt.HasText(d.tr("通用")) {
					t.Fatal("settings navigation failed")
				}
				validateTheme(t, observed, dark, label+" settings")
				capture(tt, label+"-settings")
				field, ok := tt.AccessByRole(ui.RoleTextField, d.tr("设备名称"))
				if !ok {
					t.Fatal("device-name editor missing")
				}
				x, y := field.Bounds.X+field.Bounds.W/2, field.Bounds.Y+field.Bounds.H/2
				tt.Press(x, y)
				tt.Release(x, y)
				tt.Command("selectAll")
				tt.Type("BoardUI 中文 🙂")
				if d.deviceName != "BoardUI 中文 🙂" {
					t.Fatalf("native editor value=%q", d.deviceName)
				}
				if e := tt.Click(d.tr("返回历史")); e != nil {
					t.Fatal(e)
				}
				tt.Advance(500 * time.Millisecond)
				if d.page != "history" || !tt.HasText(d.tr("资料库")) {
					t.Fatal("return to history failed")
				}
				if d.selected != selected {
					t.Fatal("settings navigation lost history selection")
				}
				for i := range d.entries {
					if d.entries[i].title != titles[i] {
						t.Fatal("settings changed history data")
					}
				}
				if d.deviceName != "BoardUI 中文 🙂" {
					t.Fatal("settings value did not survive return")
				}
				capture(tt, label+"-returned")
				p := newQuickDemo(theme, "history")
				var quickTheme ui.Theme
				qt := ui.NewTesterWith(func(c *ui.Context) { p.panel(c); quickTheme = *c.Theme() }, 380, 366, ui.TesterOptions{Dark: dark})
				validateTheme(t, quickTheme, dark, label+" quick panel")
				if _, ok := qt.AccessByRole(ui.RoleTextField, p.tr("搜索快捷面板")); !ok {
					t.Fatal("quick panel search missing")
				}
				capture(qt, label+"-quick-panel")
				checks = append(checks, label+": production history/settings/return and quick-panel use BoardUI resources; synthetic state retained")
			})
		}
	}
}

// boarduiMigrationSources reads actual production files, not dependency aliases.
// It retains hashes for each inspected file so the failure is replayable.
func boarduiMigrationSources(t *testing.T, root string) (map[string]string, []string) {
	t.Helper()
	sources := map[string]string{}
	violations := []string{}
	e := filepath.WalkDir(filepath.Join(root, "app/desktop"), func(path string, entry os.DirEntry, e error) error {
		if e != nil {
			return e
		}
		if entry.IsDir() {
			return nil
		}
		if !strings.HasSuffix(path, ".go") || strings.HasSuffix(path, "_test.go") {
			return nil
		}
		raw, e := os.ReadFile(path)
		if e != nil {
			return e
		}
		relative, e := filepath.Rel(root, path)
		if e != nil {
			return e
		}
		digest := sha256.Sum256(raw)
		sources[filepath.ToSlash(relative)] = hex.EncodeToString(digest[:])
		fset := token.NewFileSet()
		file, e := parser.ParseFile(fset, path, raw, 0)
		if e != nil {
			return e
		}
		aliases := map[string]string{}
		for _, imp := range file.Imports {
			value, e := strconv.Unquote(imp.Path.Value)
			if e != nil {
				return e
			}
			alias := filepath.Base(value)
			if imp.Name != nil {
				alias = imp.Name.Name
			}
			aliases[alias] = value
			if strings.HasSuffix(value, "/app/desktop/internal/uikit") {
				violations = append(violations, relative+": imports legacy internal/uikit")
			}
		}
		ast.Inspect(file, func(node ast.Node) bool {
			switch n := node.(type) {
			case *ast.BasicLit:
				if n.Kind == token.STRING {
					value, e := strconv.Unquote(n.Value)
					if e == nil && value == "UNIC_UI_SKIN" {
						violations = append(violations, fmt.Sprintf("%s:%d: legacy appearance environment selector", relative, fset.Position(n.Pos()).Line))
					}
				}
			case *ast.SelectorExpr:
				id, ok := n.X.(*ast.Ident)
				if !ok {
					return true
				}
				importPath := aliases[id.Name]
				if (strings.Contains(importPath, "/boardui") || strings.HasSuffix(importPath, "/internal/desktopkit")) && slices.Contains([]string{"DefaultSkin", "Linux", "MacOS", "Windows"}, n.Sel.Name) {
					violations = append(violations, fmt.Sprintf("%s:%d: legacy appearance selector %s.%s", relative, fset.Position(n.Pos()).Line, id.Name, n.Sel.Name))
				}
			case *ast.SwitchStmt:
				if selector, ok := n.Tag.(*ast.SelectorExpr); ok && selector.Sel.Name == "Skin" {
					violations = append(violations, fmt.Sprintf("%s:%d: legacy Skin switch", relative, fset.Position(n.Pos()).Line))
				}
			case *ast.IfStmt:
				ast.Inspect(n.Cond, func(condition ast.Node) bool {
					if selector, ok := condition.(*ast.SelectorExpr); ok && selector.Sel.Name == "Skin" {
						violations = append(violations, fmt.Sprintf("%s:%d: legacy Skin appearance branch", relative, fset.Position(n.Pos()).Line))
					}
					return true
				})
			case *ast.CallExpr:
				if id, ok := n.Fun.(*ast.Ident); ok && id.Name == "DefaultSkin" {
					violations = append(violations, fmt.Sprintf("%s:%d: local legacy DefaultSkin call", relative, fset.Position(n.Pos()).Line))
				}
			}
			return true
		})
		return nil
	})
	if e != nil {
		t.Fatal(e)
	}
	slices.Sort(violations)
	return sources, violations
}
