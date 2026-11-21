package fixtures

import (
	"bytes"
	"image"
	"image/color"
	"image/draw"
	"image/png"
	"sync"

	"github.com/egoist/mygo/ui"
)

// A deterministic local image keeps image history and Quick Look usable offline.
var Image = sync.OnceValue(func() ui.ImageSource {
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

var appIcons = sync.OnceValue(func() map[string]ui.ImageSource {
	makeIcon := func(paint func(im *image.RGBA)) ui.ImageSource {
		im := image.NewRGBA(image.Rect(0, 0, 32, 32))
		paint(im)
		var b bytes.Buffer
		if err := png.Encode(&b, im); err != nil {
			panic(err)
		}
		src, err := ui.DecodeImage(b.Bytes())
		if err != nil {
			panic(err)
		}
		return src
	}
	fill := func(im *image.RGBA, r image.Rectangle, c color.RGBA) {
		draw.Draw(im, r, &image.Uniform{C: c}, image.Point{}, draw.Src)
	}
	circle := func(im *image.RGBA, cx, cy, rad float64, c color.RGBA) {
		for y := 0; y < 32; y++ {
			for x := 0; x < 32; x++ {
				dx, dy := float64(x)-cx, float64(y)-cy
				if dx*dx+dy*dy <= rad*rad {
					im.SetRGBA(x, y, c)
				}
			}
		}
	}
	return map[string]ui.ImageSource{
		"chrome": makeIcon(func(im *image.RGBA) {
			fill(im, im.Bounds(), color.RGBA{255, 255, 255, 255})
			for y := 0; y < 32; y++ {
				for x := 0; x < 32; x++ {
					dx, dy := float64(x)-15.5, float64(y)-15.5
					if dx*dx+dy*dy <= 13*13 {
						switch {
						case dy < -2:
							im.SetRGBA(x, y, color.RGBA{234, 67, 53, 255})
						case dx < 0:
							im.SetRGBA(x, y, color.RGBA{52, 168, 83, 255})
						default:
							im.SetRGBA(x, y, color.RGBA{251, 188, 4, 255})
						}
					}
				}
			}
			circle(im, 15.5, 15.5, 6.2, color.RGBA{255, 255, 255, 255})
			circle(im, 15.5, 15.5, 4.7, color.RGBA{26, 115, 232, 255})
		}),
		"vscode": makeIcon(func(im *image.RGBA) {
			fill(im, im.Bounds(), color.RGBA{24, 34, 54, 255})
			fill(im, image.Rect(20, 5, 26, 27), color.RGBA{56, 189, 248, 255})
			for i := 0; i < 14; i++ {
				fill(im, image.Rect(7+i, 10+i/2, 11+i, 13+i/2), color.RGBA{0, 122, 204, 255})
				fill(im, image.Rect(7+i, 19-i/2, 11+i, 22-i/2), color.RGBA{0, 122, 204, 255})
			}
		}),
		"figma": makeIcon(func(im *image.RGBA) {
			fill(im, im.Bounds(), color.RGBA{30, 30, 34, 255})
			circle(im, 12.5, 9, 4.2, color.RGBA{242, 78, 30, 255})
			circle(im, 19.5, 9, 4.2, color.RGBA{255, 114, 98, 255})
			circle(im, 12.5, 16, 4.2, color.RGBA{162, 89, 255, 255})
			circle(im, 19.5, 16, 4.2, color.RGBA{26, 188, 254, 255})
			circle(im, 12.5, 23, 4.2, color.RGBA{10, 207, 131, 255})
		}),
		"notion": makeIcon(func(im *image.RGBA) {
			fill(im, im.Bounds(), color.RGBA{250, 250, 252, 255})
			fill(im, image.Rect(4, 4, 28, 28), color.RGBA{28, 28, 32, 255})
			fill(im, image.Rect(6, 6, 26, 26), color.RGBA{255, 255, 255, 255})
			fill(im, image.Rect(9, 9, 12, 23), color.RGBA{28, 28, 32, 255})
			fill(im, image.Rect(20, 9, 23, 23), color.RGBA{28, 28, 32, 255})
			for i := 0; i < 11; i++ {
				fill(im, image.Rect(11+i, 10+i, 13+i, 13+i), color.RGBA{28, 28, 32, 255})
			}
		}),
		"finder": makeIcon(func(im *image.RGBA) {
			fill(im, image.Rect(0, 0, 16, 32), color.RGBA{56, 189, 248, 255})
			fill(im, image.Rect(16, 0, 32, 32), color.RGBA{224, 242, 254, 255})
			fill(im, image.Rect(9, 10, 12, 14), color.RGBA{15, 23, 42, 255})
			fill(im, image.Rect(20, 10, 23, 14), color.RGBA{15, 23, 42, 255})
			fill(im, image.Rect(10, 21, 22, 23), color.RGBA{15, 23, 42, 255})
		}),
		"slack": makeIcon(func(im *image.RGBA) {
			fill(im, im.Bounds(), color.RGBA{74, 21, 75, 255})
			fill(im, image.Rect(7, 12, 15, 15), color.RGBA{54, 197, 240, 255})
			fill(im, image.Rect(12, 7, 15, 15), color.RGBA{46, 182, 125, 255})
			fill(im, image.Rect(17, 17, 25, 20), color.RGBA{236, 178, 46, 255})
			fill(im, image.Rect(17, 17, 20, 25), color.RGBA{224, 30, 90, 255})
		}),
		"wechat": makeIcon(func(im *image.RGBA) {
			fill(im, im.Bounds(), color.RGBA{7, 193, 96, 255})
			circle(im, 13, 14, 7, color.RGBA{255, 255, 255, 255})
			circle(im, 10.5, 13, 1.3, color.RGBA{7, 193, 96, 255})
			circle(im, 15.5, 13, 1.3, color.RGBA{7, 193, 96, 255})
			circle(im, 20, 19, 5.5, color.RGBA{232, 255, 240, 255})
		}),
	}
})

// AppIcon returns a deterministic synthetic source-application icon for preview fixtures.
func AppIcon(name string) ui.ImageSource {
	return appIcons()[name]
}
