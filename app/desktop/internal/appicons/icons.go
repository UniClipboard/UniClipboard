// Package appicons owns the original v1 Desktop artwork.
package appicons

import _ "embed"

// PNG is the shared application image.
//
//go:embed icon.png
var PNG []byte

// TrayPNG is the original black-on-transparent cat template.
//
//go:embed tray-cat@2x.png
var TrayPNG []byte
