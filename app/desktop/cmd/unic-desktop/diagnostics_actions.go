package main

import (
	"context"
	"errors"
	"os"
	"path/filepath"
	"time"

	"github.com/egoist/mygo"

	"github.com/uniclipboard/unic/app/desktop/internal/desktopui"
	"github.com/uniclipboard/unic/app/desktop/internal/i18n"
	api "github.com/uniclipboard/unic/internal/localapi"
	"github.com/uniclipboard/unic/internal/logarchive"
)

func bindDesktopDiagnostics(screen *desktopui.Desktop, profile string, client *api.Client, language func() string) {
	screen.SetDiagnosticsActions(func(ctx context.Context) (string, error) {
		selected, err := mygo.Dialog.Save(mygo.SaveDialogOptions{Title: i18n.Translate(language(), "导出诊断日志"), DefaultPath: "unic-diagnostics-" + time.Now().UTC().Format("20060102T150405Z") + ".zip"})
		if err != nil || selected == "" {
			return "", err
		}
		if err = ctx.Err(); err != nil {
			return "", err
		}
		return exportDesktopDiagnosticLogs(ctx, profile, selected, client)
	}, func(ctx context.Context) error {
		if err := ctx.Err(); err != nil {
			return err
		}
		path := filepath.Join(profile, "logs")
		info, err := os.Lstat(path)
		if err != nil || !info.IsDir() || info.Mode()&os.ModeSymlink != 0 {
			return errors.New("local logs directory is unavailable")
		}
		return mygo.Shell.OpenPath(path)
	})
}

func exportDesktopDiagnosticLogs(ctx context.Context, profile, output string, client *api.Client) (string, error) {
	return logarchive.Export(ctx, profile, output, "unic-desktop", client)
}
