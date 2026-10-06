package update

import (
	"archive/zip"
	"bytes"
	"errors"
	"fmt"
	"io"
	"path"
	"strings"
)

// The Windows installer contract of the Tauri updater (tauri-plugin-updater 2.10.1, `Update::install_inner`):
// the NSIS setup program is started with `<install mode args> /UPDATE /ARGS <current arguments>` and the
// application exits. `installMode` is not configured in tauri.conf.json, so the default `passive` applies:
// `/P` (progress bar only) and `/R` (the installer restarts the application with the `/ARGS` that follow).
var nsisPassiveArgs = []string{"/P", "/R"}

// NSISArgs builds the installer command line for an in-place update that relaunches with currentArgs.
func NSISArgs(currentArgs []string) string {
	args := append([]string{}, nsisPassiveArgs...)
	args = append(args, "/UPDATE", "/ARGS")
	for _, arg := range currentArgs {
		args = append(args, EscapeNSISArg(arg))
	}
	return strings.Join(args, " ")
}

// EscapeNSISArg quotes an argument for the NSIS command line (Tauri `escape_nsis_current_exe_arg`): Windows
// argument quoting, and additionally anything containing `/` is quoted so NSIS does not read it as one of its own
// switches. An empty argument is quoted so it is not lost.
func EscapeNSISArg(arg string) string {
	quote := arg == "" || strings.ContainsAny(arg, " \t/")
	var b strings.Builder
	if quote {
		b.WriteByte('"')
	}
	backslashes := 0
	for _, r := range arg {
		if r == '\\' {
			backslashes++
		} else {
			if r == '"' {
				b.WriteString(strings.Repeat(`\`, backslashes+1)) // 2n+1 backslashes before an inner quote
			}
			backslashes = 0
		}
		b.WriteRune(r)
	}
	if quote {
		b.WriteString(strings.Repeat(`\`, backslashes)) // 2n before the closing quote
		b.WriteByte('"')
	}
	return b.String()
}

// ErrNoInstaller is returned when the payload is neither a Windows executable nor a zip holding one.
var ErrNoInstaller = errors.New("update payload is neither an installer executable nor a zip containing one")

// ExtractInstaller returns the NSIS installer from a verified payload. The release feed carries either the
// `*-setup.exe` itself or the `*.nsis.zip` wrapper (scripts/assemble-update-manifest.js accepts both), so both are
// recognised the way the Tauri updater's `extract` does: a zip is searched for its first `.exe`, anything else
// must be a PE image.
func ExtractInstaller(payload []byte) ([]byte, error) {
	switch {
	case bytes.HasPrefix(payload, []byte("PK\x03\x04")):
		zr, err := zip.NewReader(bytes.NewReader(payload), int64(len(payload)))
		if err != nil {
			return nil, fmt.Errorf("read update archive: %w", err)
		}
		for _, f := range zr.File {
			if f.FileInfo().IsDir() || !strings.EqualFold(path.Ext(f.Name), ".exe") {
				continue
			}
			rc, err := f.Open()
			if err != nil {
				return nil, err
			}
			data, err := io.ReadAll(rc)
			_ = rc.Close()
			if err != nil {
				return nil, err
			}
			if !isPE(data) {
				return nil, ErrNoInstaller
			}
			return data, nil
		}
		return nil, ErrNoInstaller
	case isPE(payload):
		return payload, nil
	}
	return nil, ErrNoInstaller
}

func isPE(data []byte) bool { return bytes.HasPrefix(data, []byte("MZ")) }
