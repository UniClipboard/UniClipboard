package main

import "image/color"

// Wails gives the macOS status item a 22 pt square. Keep the template image at 2x,
// with the design's 18 pt grid centred inside it: the 18 x 20 unit glyph is about
// 13.5 x 15 pt. These dimensions are platform-independent so Linux CI can check them.
const (
	macTrayImagePx  = 44
	macTrayArtPx    = 36
	macTrayCatArtPx = 34 // scale the full cat image; its visible alpha is about 31 px (15.5 pt) tall
)

func macTrayIconSpec() iconSpec {
	return iconSpec{size: macTrayImagePx, art: macTrayArtPx, pal: iconPalette{fg: color.NRGBA{A: 255}}}
}

func macCatTrayImage() ([]byte, error) {
	return scaledCat(macTrayIconSpec(), macTrayCatArtPx)
}
