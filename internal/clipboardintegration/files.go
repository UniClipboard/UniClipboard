package clipboardintegration

import (
	"encoding/base64"
	"errors"
	"path/filepath"
	"strings"
)

const receivedFilePrefix = "desktop-received:"

// desktopFilePath interprets both local-copy references and custom receive references.
func desktopFilePath(ref string) (string, error) {
	if value, ok := strings.CutPrefix(ref, receivedFilePrefix); ok {
		decoded, err := base64.RawURLEncoding.DecodeString(value)
		if err != nil {
			return "", errors.New("invalid desktop file reference")
		}
		ref = string(decoded)
	}
	if !filepath.IsAbs(ref) {
		return "", errors.New("invalid desktop file reference")
	}
	return ref, nil
}
