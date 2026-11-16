package trayicon

import (
	"context"
	"os"
	"os/exec"
	"strings"
	"time"
)

func platformSpec() spec {
	env := linuxDesktopEnv{getenv: os.Getenv, readFile: readLinuxThemeFile}
	return spec{size: 22, glyph: 22, cat: 22, foreground: linuxTrayPalette(env, gsetting)}
}
func gsetting(schema, key string) string {
	ctx, cancel := context.WithTimeout(context.Background(), time.Second)
	defer cancel()
	out, err := exec.CommandContext(ctx, "gsettings", "get", schema, key).Output()
	if err != nil {
		return ""
	}
	return strings.TrimSpace(string(out))
}
func watchTheme(ctx context.Context, changed func()) {
	env := linuxDesktopEnv{getenv: os.Getenv, readFile: readLinuxThemeFile}
	if !env.isKDE() {
		return
	}
	poll := newKDEThemePoll(env, changed)
	go func() {
		ticker := time.NewTicker(time.Second)
		defer ticker.Stop()
		for {
			select {
			case <-ctx.Done():
				return
			case <-ticker.C:
				poll()
			}
		}
	}()
}
