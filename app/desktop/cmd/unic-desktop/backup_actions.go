package main

import (
	"context"
	"errors"
	"io"
	"os"
	"path/filepath"
	"time"

	"github.com/egoist/mygo"

	"github.com/uniclipboard/unic/app/desktop/internal/desktopui"
	"github.com/uniclipboard/unic/app/desktop/internal/i18n"
)

func bindBackupActions(screen *desktopui.Desktop, language func() string) {
	screen.SetBackupActions(func(ctx context.Context, data []byte) (string, error) {
		path, err := mygo.Dialog.Save(mygo.SaveDialogOptions{Title: i18n.Translate(language(), "导出配置"), DefaultPath: "unic-preferences-" + time.Now().UTC().Format("20060102T150405") + ".json", Filters: []mygo.FileFilter{{Name: "JSON", Extensions: []string{"json"}}}})
		if err != nil || path == "" {
			return "", err
		}
		return writePortableSettingsBackup(ctx, path, data)
	}, func(ctx context.Context) ([]byte, error) {
		paths, err := mygo.Dialog.Open(mygo.OpenDialogOptions{Title: i18n.Translate(language(), "导入配置"), Filters: []mygo.FileFilter{{Name: "JSON", Extensions: []string{"json"}}}})
		if err != nil || len(paths) == 0 {
			return nil, err
		}
		if len(paths) != 1 {
			return nil, errors.New("select one settings backup")
		}
		return readPortableSettingsBackup(ctx, paths[0])
	})
}

// Exclusive creation never replaces an existing destination, including a link.
// Roll back only the inode created by this operation if a write fails.
func writePortableSettingsBackup(ctx context.Context, path string, data []byte) (string, error) {
	if len(data) == 0 || len(data) > desktopui.MaxSettingsBackupBytes {
		return "", errors.New("invalid settings backup size")
	}
	if err := ctx.Err(); err != nil {
		return "", err
	}
	absolute, err := filepath.Abs(path)
	if err != nil {
		return "", errors.New("invalid settings backup destination")
	}
	parent, err := os.OpenRoot(filepath.Dir(absolute))
	if err != nil {
		return "", errors.New("settings backup destination is unavailable")
	}
	defer func() { _ = parent.Close() }()
	name := filepath.Base(absolute)
	file, err := parent.OpenFile(name, os.O_WRONLY|os.O_CREATE|os.O_EXCL, 0600)
	if err != nil {
		return "", errors.New("settings backup destination already exists or is unavailable")
	}
	owned, err := file.Stat()
	if err != nil {
		_ = file.Close()
		return "", errors.New("cannot inspect settings backup destination")
	}
	complete := false
	defer func() {
		_ = file.Close()
		if !complete {
			if current, err := parent.Lstat(name); err == nil && os.SameFile(owned, current) {
				_ = parent.Remove(name)
			}
		}
	}()
	if _, err = file.Write(data); err != nil {
		return "", errors.New("cannot write settings backup")
	}
	if err = ctx.Err(); err != nil {
		return "", err
	}
	if err = file.Sync(); err != nil {
		return "", errors.New("cannot save settings backup")
	}
	if err = file.Close(); err != nil {
		return "", errors.New("cannot close settings backup")
	}
	complete = true
	return absolute, nil
}

func readPortableSettingsBackup(ctx context.Context, path string) ([]byte, error) {
	if err := ctx.Err(); err != nil {
		return nil, err
	}
	before, err := os.Lstat(path)
	if err != nil || !before.Mode().IsRegular() || before.Size() > desktopui.MaxSettingsBackupBytes {
		return nil, errors.New("settings backup must be a bounded regular file")
	}
	file, err := os.Open(path)
	if err != nil {
		return nil, errors.New("cannot open settings backup")
	}
	defer func() { _ = file.Close() }()
	actual, err := file.Stat()
	if err != nil || !actual.Mode().IsRegular() || !os.SameFile(before, actual) {
		return nil, errors.New("settings backup changed while opening")
	}
	data, err := io.ReadAll(io.LimitReader(file, desktopui.MaxSettingsBackupBytes+1))
	if err != nil || len(data) == 0 || len(data) > desktopui.MaxSettingsBackupBytes {
		return nil, errors.New("settings backup is empty or exceeds the size limit")
	}
	if err = ctx.Err(); err != nil {
		return nil, err
	}
	return data, nil
}
