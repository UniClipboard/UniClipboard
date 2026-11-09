package desktopdemo

import (
	"bytes"
	_ "embed"
	"image"
	"image/color"
	"image/draw"
	"image/png"
	"sync"

	"github.com/egoist/mygo/ui"
)

//go:embed app-icon.png
var appIconPNG []byte

var appIcon = sync.OnceValue(func() ui.ImageSource {
	src, err := ui.DecodeImage(appIconPNG)
	if err != nil {
		panic(err)
	}
	return src
})

// A deterministic local image keeps image history and Quick Look usable offline.
var fixtureImage = sync.OnceValue(func() ui.ImageSource {
	im := image.NewRGBA(image.Rect(0, 0, 800, 500))
	fill := func(r image.Rectangle, col color.RGBA) {
		draw.Draw(im, r, &image.Uniform{C: col}, image.Point{}, draw.Src)
	}
	fill(im.Bounds(), color.RGBA{230, 236, 247, 255})
	fill(image.Rect(32, 32, 768, 468), color.RGBA{249, 250, 253, 255})
	fill(image.Rect(32, 32, 205, 468), color.RGBA{217, 226, 241, 255})
	fill(image.Rect(225, 54, 748, 82), color.RGBA{217, 226, 241, 255})
	for i := 0; i < 5; i++ {
		y := 110 + i*62
		fill(image.Rect(50, y, 185, y+28), color.RGBA{191, 207, 233, 255})
		fill(image.Rect(225, y, 440, y+46), color.RGBA{230, 236, 247, 255})
	}
	fill(image.Rect(466, 110, 748, 340), color.RGBA{172, 199, 236, 255})
	fill(image.Rect(546, 374, 668, 412), color.RGBA{48, 116, 224, 255})
	var b bytes.Buffer
	if err := png.Encode(&b, im); err != nil {
		panic(err)
	}
	src, err := ui.DecodeImage(b.Bytes())
	if err != nil {
		panic(err)
	}
	return src
})
