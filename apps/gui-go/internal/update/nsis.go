package update

import (
	"archive/zip"
	"errors"
	"fmt"
	"io"
	"os"
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
func ExtractInstaller(payload *os.File, destination io.Writer) error {
	header := make([]byte, 4)
	n, _ := payload.ReadAt(header, 0)
	if n >= 4 && string(header) == "PK\x03\x04" {
		info, err := payload.Stat()
		if err != nil {
			return err
		}
		zr, err := zip.NewReader(payload, info.Size())
		if err != nil {
			return fmt.Errorf("read update archive: %w", err)
		}
		for _, f := range zr.File {
			if f.FileInfo().IsDir() || !strings.EqualFold(path.Ext(f.Name), ".exe") {
				continue
			}
			if f.UncompressedSize64 > uint64(maxDownloadSize) {
				return ErrNoInstaller
			}
			rc, err := f.Open()
			if err != nil {
				return err
			}
			defer rc.Close()
			return copyExecutable(rc, destination, []byte("MZ"), maxDownloadSize, ErrNoInstaller)
		}
		return ErrNoInstaller
	}
	if _, err := payload.Seek(0, io.SeekStart); err != nil {
		return err
	}
	return copyExecutable(payload, destination, []byte("MZ"), maxDownloadSize, ErrNoInstaller)
}

// copyExecutable validates the executable header and bounds decompression while
// copying with io.Copy's fixed-size buffer. Callers remove incomplete staging.
func copyExecutable(source io.Reader, destination io.Writer, magic []byte, limit int64, invalid error) error {
	header := make([]byte, len(magic))
	if _, err := io.ReadFull(source, header); err != nil || string(header) != string(magic) {
		return invalid
	}
	if _, err := destination.Write(header); err != nil {
		return err
	}
	n, err := io.Copy(destination, io.LimitReader(source, limit-int64(len(header))+1))
	if err != nil {
		return err
	}
	if n+int64(len(header)) > limit {
		return invalid
	}
	return nil
}
