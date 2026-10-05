package commands

import (
	"encoding/base64"
	"errors"
	"os"

	"github.com/UniClipboard/UniClipboard/apps/cli-go/internal/ui"
)

// copyToTerminal asks the terminal attached to stderr to copy value through
// an OSC 52 request. It never calls the platform clipboard API directly.
func copyToTerminal(value string) error {
	if !ui.StderrIsTerminal() {
		return errors.New("Cannot copy because the terminal output is not connected to a terminal")
	}
	request := "\x1b]52;c;" + base64.StdEncoding.EncodeToString([]byte(value)) + "\x07"
	if _, err := os.Stderr.WriteString(request); err != nil {
		return errors.New("Failed to send the result to the terminal clipboard: " + rustIOError(err))
	}
	return nil
}
