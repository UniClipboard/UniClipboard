// Command uniclip is the UniClipboard command-line interface.
package main

import (
	"os"

	"github.com/UniClipboard/UniClipboard/apps/cli-go/internal/cli"
	"github.com/UniClipboard/UniClipboard/apps/cli-go/internal/commands"
)

func main() {
	os.Exit(cli.Execute(commands.Tree()))
}
