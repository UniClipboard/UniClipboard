package commands

import (
	"context"
	"strings"
	"time"

	"github.com/UniClipboard/UniClipboard/apps/cli-go/internal/cli"
	"github.com/UniClipboard/UniClipboard/apps/cli-go/internal/exitcode"
	"github.com/UniClipboard/UniClipboard/apps/cli-go/internal/ui"
)

// exitSigint is the exit code of a command stopped by Ctrl+C.
const exitSigint = 130

// optionalString returns the option value, or nil when it was not given.
func optionalString(ctx *cli.Context, long string) *string {
	if !ctx.Has(long) {
		return nil
	}
	value := ctx.String(long)
	return &value
}

// readNewPassphrase resolves a new space passphrase from `--passphrase` or
// a confirmed interactive prompt. Empty values are always rejected.
func readNewPassphrase(flag *string, asJSON bool) (string, int, bool) {
	switch {
	case flag != nil && strings.TrimSpace(*flag) == "":
		ui.Error("--passphrase is empty")
		return "", exitcode.Error, false
	case flag != nil:
		return *flag, 0, true
	case asJSON:
		ui.Error("--passphrase is required in --json mode")
		return "", exitcode.Error, false
	}
	passphrase, err := ui.PasswordWithConfirm("New space passphrase", "Confirm passphrase")
	if err != nil {
		ui.Error(err.Error())
		return "", exitcode.Error, false
	}
	if strings.TrimSpace(passphrase) == "" {
		ui.Error("Passphrase cannot be empty")
		return "", exitcode.Error, false
	}
	return passphrase, 0, true
}

// clearSpinner is `finish_and_clear` that tolerates a hidden (nil) spinner.
func clearSpinner(spinner *ui.Spinner) {
	if spinner != nil {
		spinner.Clear()
	}
}

// spinnerSuccess is `ui::spinner_finish_success`; a hidden spinner still
// prints the success line.
func spinnerSuccess(spinner *ui.Spinner, message string) {
	clearSpinner(spinner)
	ui.Success(message)
}

// spinnerError is `ui::spinner_finish_error`.
func spinnerError(spinner *ui.Spinner, message string) {
	clearSpinner(spinner)
	ui.Error(message)
}

// sleepCtx waits for d or until ctx is done; it reports whether the full
// duration elapsed.
func sleepCtx(ctx context.Context, d time.Duration) bool {
	timer := time.NewTimer(d)
	defer timer.Stop()
	select {
	case <-timer.C:
		return true
	case <-ctx.Done():
		return false
	}
}
