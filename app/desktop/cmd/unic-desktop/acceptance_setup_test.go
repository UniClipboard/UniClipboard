package main

import (
	"strings"
	"testing"

	"github.com/egoist/mygo/ui"

	proof "github.com/uniclipboard/unic/app/desktop/internal/e2eproof"
	"github.com/uniclipboard/unic/app/desktop/internal/setupui"
	"github.com/uniclipboard/unic/internal/v1migration"
)

// Failure inventory: see docs/exec/0026-first-install-setup.md. This exercises
// the actual renderer with semantic controls, without native windows or secrets.
func scenarioSetupPages(t *testing.T) {
	eachGUISkin(t, func(t *testing.T, dark bool) {
		for _, stage := range []string{"welcome", "welcome-detecting", "welcome-none", "welcome-error", "confirm", "invalid", "password", "conflict", "credentials", "import", "verify", "stopping", "complete", "complete-retry", "partial", "cancelled", "failed"} {
			t.Run(stage, func(t *testing.T) {
				fields := setupui.Fields{}
				count := 0
				action := func() { count++ }
				model := setupui.Model{Stage: stage, Source: "/synthetic/v1", Found: true, Progress: v1migration.Progress{Stage: stage, Processed: 3, Total: 5}, Report: v1migration.Report{Status: stage, Total: 5, Imported: 2, Skipped: 1, Failed: 1, MissingParts: 2, Verified: 3, VerificationFailed: 1}}
				if strings.HasPrefix(stage, "welcome-") {
					model.Stage = "welcome"
				}
				if stage == "welcome-detecting" {
					model.Detecting = true
				}
				if stage == "welcome-none" {
					model.Found = false
				}
				if stage == "welcome-error" {
					model.Error = "无法检查 v1 数据。请重试或手动选择目录。"
				}
				if stage == "complete-retry" {
					model.Stage = "complete"
					model.Report.Imported = 0
					model.Report.Skipped = 5
					model.Report.Verified = 5
				}
				if stage != "partial" {
					model.Report.Failed, model.Report.MissingParts, model.Report.VerificationFailed = 0, 0, 0
				}
				if stage == "invalid" {
					model.Error = "这个目录不是受支持的 v1 数据目录。请选择其他目录。"
				}
				if stage == "password" {
					model.Error = "口令不正确。请重试。"
					fields.Passphrase = "synthetic-password"
				}
				actions := setupui.Actions{Begin: action, Redetect: action, Pick: action, Start: action, Unlock: action, Stop: action, Enter: action}
				view := ui.NewTesterWith(func(c *ui.Context) { setupui.Render(c, model, &fields, actions) }, 640, 560, ui.TesterOptions{Dark: dark})
				proof.Capture(t, view)
				expected := map[string]string{"welcome": "开始使用", "confirm": "开始迁移", "invalid": "暂不迁移", "password": "解锁并继续", "conflict": "重试", "credentials": "停止迁移", "import": "停止迁移", "verify": "停止迁移", "complete": "开始使用", "partial": "进入应用", "cancelled": "继续迁移", "failed": "重试"}[stage]
				if strings.HasPrefix(stage, "welcome-") {
					expected = "开始使用"
				}
				if stage == "complete-retry" {
					expected = "开始使用"
				}
				if expected != "" {
					guiClick(t, view, expected)
					if count != 1 {
						t.Fatal("action not dispatched")
					}
				}
				if stage == "stopping" {
					if _, ok := view.AccessByRole(ui.RoleButton, "进入应用"); ok {
						t.Fatal("can enter before safe stop")
					}
				}
				if stage == "partial" {
					if _, ok := view.AccessByRole(ui.RoleButton, "开始使用"); ok {
						t.Fatal("partial shown as complete")
					}
				}
			})
		}
	})
}
