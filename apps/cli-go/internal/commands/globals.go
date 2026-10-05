package commands

import (
	"os"

	"github.com/UniClipboard/UniClipboard/apps/cli-go/internal/cli"
	"github.com/UniClipboard/UniClipboard/apps/cli-go/internal/ui"
)

// applyGlobalFlags exports `--dev` and `--profile` to the environment before
// any path resolution, so a spawned daemon inherits them too.
func applyGlobalFlags(ctx *cli.Context) {
	if ctx.Bool("dev") {
		os.Setenv("UNICLIPBOARD_ENV", "development")
	}
	if ctx.Has("profile") {
		os.Setenv("UC_PROFILE", ctx.String("profile"))
	}
}

func warnLegacySpaceCommand(command, replacement string) {
	ui.Warn("`uniclip " + command + "` is deprecated; use `uniclip " + replacement + "` instead. This alias will be removed in a future release.")
}

func warnMobileSyncAlias() {
	ui.Warn("`mobile-sync` is deprecated; use `mobile` instead. This alias will be removed in a future release.")
}
