package commands

import "github.com/UniClipboard/UniClipboard/apps/cli-go/internal/ui"

func notPorted() int {
	ui.Error("this command is not available in the Go CLI yet")
	return 1
}
