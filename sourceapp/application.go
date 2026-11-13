// Package sourceapp defines portable clipboard application provenance.
package sourceapp

import (
	"bytes"
	"encoding/json"
	"errors"
	"image/png"
	"slices"
	"strings"
	"unicode"
	"unicode/utf8"
)

const MaxIconBytes = 16 << 10
const MaxIconDimension = 128

// Application is a sender-supplied hint, never proof of application identity.
// Identifier is a bundle ID, application ID, WM_CLASS or executable basename.
// It must not contain a local executable path or a window title.
type Application struct {
	Platform    string `json:"platform"`
	Identifier  string `json:"identifier"`
	Name        string `json:"name,omitempty"`
	Attribution string `json:"attribution"` // clipboard_owner, source_marker or foreground
	IconPNG     []byte `json:"icon_png,omitempty"`
}

func (a *Application) Clone() *Application {
	if a == nil {
		return nil
	}
	b := *a
	b.IconPNG = slices.Clone(a.IconPNG)
	return &b
}

func (a *Application) Validate() error {
	if a == nil {
		return nil
	}
	if a.Platform != "darwin" && a.Platform != "windows" && a.Platform != "linux" && a.Platform != "android" && a.Platform != "ios" {
		return errors.New("invalid source application platform")
	}
	if a.Attribution != "clipboard_owner" && a.Attribution != "source_marker" && a.Attribution != "foreground" {
		return errors.New("invalid source application attribution")
	}
	for _, s := range []string{a.Identifier, a.Name} {
		if len(s) > 256 || !utf8.ValidString(s) || strings.ContainsAny(s, "/\\") || strings.IndexFunc(s, unicode.IsControl) >= 0 {
			return errors.New("invalid source application identity")
		}
	}
	if strings.TrimSpace(a.Identifier) == "" {
		return errors.New("missing source application identifier")
	}
	if !ValidIcon(a.IconPNG) {
		return errors.New("invalid source application icon")
	}
	return nil
}

// ValidIcon checks encoded and decoded bounds before allocating a raster.
func ValidIcon(data []byte) bool {
	if len(data) == 0 {
		return true
	}
	if len(data) > MaxIconBytes {
		return false
	}
	cfg, err := png.DecodeConfig(bytes.NewReader(data))
	if err != nil || cfg.Width < 1 || cfg.Height < 1 || cfg.Width > MaxIconDimension || cfg.Height > MaxIconDimension {
		return false
	}
	_, err = png.Decode(bytes.NewReader(data))
	return err == nil
}

// Encode returns the exact bytes charged to the storage budget.
func (a *Application) Encode() []byte {
	if a == nil {
		return nil
	}
	b, _ := json.Marshal(a)
	return b
}

func Decode(data []byte) *Application {
	if len(data) == 0 {
		return nil
	}
	var a Application
	if json.Unmarshal(data, &a) != nil || a.Validate() != nil {
		return nil
	}
	return &a
}
