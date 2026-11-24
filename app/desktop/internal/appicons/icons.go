// Package appicons owns the Desktop application and tray artwork.
package appicons

import _ "embed"

// PNG is the Linux window icon and the image used in About.
//
//go:embed icon.png
var PNG []byte

// MacPNG is the Dock icon for the unbundled development binary.
//
//go:embed icon-macos.png
var MacPNG []byte

// WindowsPNG is the image mygo uses for the DPI-aware window icons.
//
//go:embed icon-windows.png
var WindowsPNG []byte

// TrayPNG is the original black-on-transparent cat template.
//
//go:embed tray-cat@2x.png
var TrayPNG []byte

// TrayGlyphSVG retains the exact v1 glyph geometry on its 24-unit grid.
//
//go:embed tray-glyph.svg
var TrayGlyphSVG []byte

// LogoSVG is the canonical black-field, white pixel-cat vector logo.
//
//go:embed unic-logo.svg
var LogoSVG []byte
