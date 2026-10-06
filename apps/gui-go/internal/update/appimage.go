package update

import (
	"archive/tar"
	"bytes"
	"compress/gzip"
	"errors"
	"fmt"
	"io"
	"os"
	"path"
	"path/filepath"
	"strings"
)

// The Linux update contract of the Tauri updater: only an AppImage installs itself. The release feed carries the
// verified payload as `<name>.AppImage.tar.gz` (the updater artifact) or as the bare `.AppImage`; the file at
// `$APPIMAGE` (the AppImage the running process was started from) is replaced and the application restarts from it.
// deb and rpm installs are owned by the package manager and never reach this code (the install kind sends the
// frontend to its "update with your package manager" dialog).

// maxAppImageSize bounds what is read out of an archive (an AppImage with bundled GTK/WebKit libraries is
// a few hundred MB).
const maxAppImageSize = 2 << 30

// ErrNoAppImage is returned when the payload is neither an ELF executable nor a tar.gz holding one.
var ErrNoAppImage = errors.New("update payload is neither an AppImage nor a tar.gz containing one")

// ExtractAppImage returns the AppImage from a verified payload.
func ExtractAppImage(payload []byte) ([]byte, error) {
	switch {
	case isELF(payload):
		return payload, nil
	case len(payload) >= 2 && payload[0] == 0x1f && payload[1] == 0x8b:
		gz, err := gzip.NewReader(bytes.NewReader(payload))
		if err != nil {
			return nil, ErrNoAppImage
		}
		tr := tar.NewReader(gz)
		for {
			header, err := tr.Next()
			if errors.Is(err, io.EOF) {
				return nil, ErrNoAppImage
			}
			if err != nil {
				return nil, fmt.Errorf("read update archive: %w", err)
			}
			if header.Typeflag != tar.TypeReg || !strings.HasSuffix(strings.ToLower(path.Base(header.Name)), ".appimage") {
				continue
			}
			data, err := io.ReadAll(io.LimitReader(tr, maxAppImageSize+1))
			if err != nil {
				return nil, fmt.Errorf("read update archive: %w", err)
			}
			if len(data) > maxAppImageSize || !isELF(data) {
				return nil, ErrNoAppImage
			}
			return data, nil
		}
	}
	return nil, ErrNoAppImage
}

func isELF(b []byte) bool { return len(b) > 4 && bytes.Equal(b[:4], []byte{0x7f, 'E', 'L', 'F'}) }

// InstallAppImage replaces the file at target with the AppImage in payload. The new file is staged in the same
// directory (so a missing write permission is found before anything changes, and the swap is one rename on one
// volume), carries the old file's permission bits plus the executable bits, and replaces the target atomically; the
// running process keeps its old inode until it exits.
func InstallAppImage(payload []byte, target string) error {
	data, err := ExtractAppImage(payload)
	if err != nil {
		return err
	}
	info, err := os.Stat(target)
	if err != nil {
		return fmt.Errorf("current AppImage: %w", err)
	}
	if !info.Mode().IsRegular() {
		return fmt.Errorf("%s is not a regular file", target)
	}
	staged, err := os.CreateTemp(filepath.Dir(target), ".uc-update-*")
	if err != nil {
		return fmt.Errorf("stage update beside %s: %w", target, err)
	}
	stagedPath := staged.Name()
	defer os.Remove(stagedPath) // a no-op after the rename
	_, werr := staged.Write(data)
	if cerr := staged.Close(); werr == nil {
		werr = cerr
	}
	if werr != nil {
		return fmt.Errorf("write update: %w", werr)
	}
	if err := os.Chmod(stagedPath, info.Mode().Perm()|0o111); err != nil {
		return err
	}
	if err := os.Rename(stagedPath, target); err != nil {
		return fmt.Errorf("replace %s: %w", target, err)
	}
	return nil
}
