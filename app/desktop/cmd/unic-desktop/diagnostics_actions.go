package main

import (
	"context"
	"errors"
	"os"
	"os/exec"
	"path/filepath"
	"runtime"
	"time"

	"github.com/egoist/mygo"

	"github.com/uniclipboard/unic/app/desktop/internal/desktopui"
	"github.com/uniclipboard/unic/app/desktop/internal/i18n"
	"github.com/uniclipboard/unic/internal/servicelaunch"
)

func bindDesktopDiagnostics(screen *desktopui.Desktop, profile string, language func() string) {
	screen.SetDiagnosticsActions(func(ctx context.Context) (string, error) {
		selected, err := mygo.Dialog.Save(mygo.SaveDialogOptions{Title: i18n.Translate(language(), "导出诊断日志"), DefaultPath: "unic-diagnostics-" + time.Now().UTC().Format("20060102T150405Z") + ".zip"})
		if err != nil || selected == "" {
			return "", err
		}
		if err = ctx.Err(); err != nil {
			return "", err
		}
		return exportDesktopDiagnosticLogs(ctx, profile, selected)
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

// Use the same resolver as the daemon, then select the CLI in that exact private
// installation directory. Never execute an unrelated command found on PATH.
func exportDesktopDiagnosticLogs(ctx context.Context, profile, output string) (string, error) {
	daemon, err := servicelaunch.Sibling()
	if err != nil {
		return "", errors.New("cannot locate the private CLI")
	}
	name := "unic"
	if runtime.GOOS == "windows" {
		name += ".exe"
	}
	cli := filepath.Join(filepath.Dir(daemon), name)
	if err = servicelaunch.CheckExecutable(cli); err != nil {
		return "", errors.New("the private CLI is unavailable")
	}
	cmd := exec.CommandContext(ctx, cli, "dev", "logs", "export", "--output", output)
	cmd.Env = append(os.Environ(), "UNIC_HOME="+profile)
	servicelaunch.ConfigureProcess(cmd)
	// The CLI's raw stderr can include local paths. Keep native errors out of the
	// GUI and telemetry; success is determined by process completion, not stdout.
	if err = cmd.Run(); err != nil {
		return "", errors.New("cannot export local diagnostic logs")
	}
	if err = ctx.Err(); err != nil {
		return "", err
	}
	return filepath.Abs(output)
}
