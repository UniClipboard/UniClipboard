// Package output renders stdout data in the CLI's stable formats.
package output

import (
	"bytes"
	"encoding/json"
	"fmt"
	"os"
	"strings"

	"github.com/UniClipboard/UniClipboard/apps/cli-go/internal/exitcode"
	"github.com/UniClipboard/UniClipboard/apps/cli-go/internal/ui"
)

// Pretty renders value like `serde_json::to_string_pretty`: two-space
// indent, no HTML escaping, and raw daemon JSON kept byte-for-byte except
// for indentation.
func Pretty(value any) (string, error) {
	var buf bytes.Buffer
	enc := json.NewEncoder(&buf)
	enc.SetEscapeHTML(false)
	enc.SetIndent("", "  ")
	if err := enc.Encode(value); err != nil {
		return "", err
	}
	out := strings.TrimSuffix(buf.String(), "\n")
	// serde_json does not escape the JSON-legal line/paragraph separators.
	out = strings.ReplaceAll(out, ` `, " ")
	out = strings.ReplaceAll(out, ` `, " ")
	return out, nil
}

// Compact renders value on one line (for JSON-lines event streams).
func Compact(value any) (string, error) {
	var buf bytes.Buffer
	enc := json.NewEncoder(&buf)
	enc.SetEscapeHTML(false)
	if err := enc.Encode(value); err != nil {
		return "", err
	}
	out := strings.TrimSuffix(buf.String(), "\n")
	out = strings.ReplaceAll(out, ` `, " ")
	return strings.ReplaceAll(out, ` `, " "), nil
}

// EmitJSON prints value as pretty JSON and returns code, or reports a
// serialization failure.
func EmitJSON(value any, context string) int {
	return EmitJSONWithCode(value, context, exitcode.Success)
}

// EmitJSONWithCode is EmitJSON with a caller-chosen success exit code.
func EmitJSONWithCode(value any, context string, code int) int {
	rendered, err := Pretty(value)
	if err != nil {
		ui.Error(fmt.Sprintf("Failed to serialize %s: %v", context, err))
		return exitcode.Error
	}
	fmt.Fprintln(os.Stdout, rendered)
	return code
}

// PrintResult prints JSON or the human text (`print_result`).
func PrintResult(value any, human string, asJSON bool) error {
	if !asJSON {
		fmt.Fprintln(os.Stdout, human)
		return nil
	}
	rendered, err := Pretty(value)
	if err != nil {
		return fmt.Errorf("Failed to serialize to JSON: %v", err)
	}
	fmt.Fprintln(os.Stdout, rendered)
	return nil
}
