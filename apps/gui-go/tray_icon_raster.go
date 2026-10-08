package main

import (
	"fmt"
	"image"
	"image/color"
	"image/draw"
	"math"
	"strconv"
	"strings"

	"golang.org/x/image/vector"
)

// The tray cat is drawn from the design's own SVG path data, so the artwork has one source (tray_icon_design.go) and every
// platform receives pixels from the same renderer. Only what the design uses is implemented: M L H V C S Q A Z (absolute and
// relative), solid fills and knock-outs (destination-out).

// point is a position in design units (the 24 x 24 grid).
type point struct{ x, y float64 }

// subpath is one flattened sub-path in design units.
type subpath struct {
	pts    []point
	closed bool
}

// parsePath flattens SVG path data. Curves become short line segments (the icon is at most a few dozen pixels, so a fixed
// subdivision is enough).
func parsePath(d string) ([]subpath, error) {
	p := &pathParser{s: d}
	var out []subpath
	var cur *subpath
	var pos, start, lastC, lastQ point
	var prev byte
	finish := func() {
		if cur != nil && len(cur.pts) > 1 {
			out = append(out, *cur)
		}
		cur = nil
	}
	begin := func(pt point) {
		finish()
		cur = &subpath{pts: []point{pt}}
		pos, start = pt, pt
	}
	for {
		cmd, ok := p.command()
		if !ok {
			break
		}
		rel := cmd >= 'a' && cmd <= 'z'
		abs := func(x, y float64) point {
			if rel {
				return point{pos.x + x, pos.y + y}
			}
			return point{x, y}
		}
		upper := cmd &^ 0x20
		if upper == 'Z' {
			if cur != nil {
				cur.closed = true
				pos = start
				finish()
				cur = &subpath{pts: []point{start}}
			}
			prev = 'Z'
			continue
		}
		first := true
		for first || p.moreNumbers() {
			first = false
			switch upper {
			case 'M':
				n, err := p.nums(2)
				if err != nil {
					return nil, err
				}
				begin(abs(n[0], n[1]))
				// Further coordinate pairs after M are implicit line-tos.
				if rel {
					cmd = 'l'
				} else {
					cmd = 'L'
				}
				upper = 'L'
			case 'L':
				n, err := p.nums(2)
				if err != nil {
					return nil, err
				}
				pos = abs(n[0], n[1])
				cur.pts = append(cur.pts, pos)
			case 'H':
				n, err := p.nums(1)
				if err != nil {
					return nil, err
				}
				if rel {
					pos.x += n[0]
				} else {
					pos.x = n[0]
				}
				cur.pts = append(cur.pts, pos)
			case 'V':
				n, err := p.nums(1)
				if err != nil {
					return nil, err
				}
				if rel {
					pos.y += n[0]
				} else {
					pos.y = n[0]
				}
				cur.pts = append(cur.pts, pos)
			case 'C', 'S':
				var c1, c2, e point
				if upper == 'C' {
					n, err := p.nums(6)
					if err != nil {
						return nil, err
					}
					c1, c2, e = abs(n[0], n[1]), abs(n[2], n[3]), abs(n[4], n[5])
				} else {
					n, err := p.nums(4)
					if err != nil {
						return nil, err
					}
					c1 = pos
					if prev == 'C' || prev == 'S' {
						c1 = point{2*pos.x - lastC.x, 2*pos.y - lastC.y}
					}
					c2, e = abs(n[0], n[1]), abs(n[2], n[3])
				}
				cur.pts = appendCubic(cur.pts, pos, c1, c2, e)
				lastC, pos = c2, e
			case 'Q':
				n, err := p.nums(4)
				if err != nil {
					return nil, err
				}
				c, e := abs(n[0], n[1]), abs(n[2], n[3])
				c1 := point{pos.x + 2.0/3*(c.x-pos.x), pos.y + 2.0/3*(c.y-pos.y)}
				c2 := point{e.x + 2.0/3*(c.x-e.x), e.y + 2.0/3*(c.y-e.y)}
				cur.pts = appendCubic(cur.pts, pos, c1, c2, e)
				lastQ, pos = c, e
			case 'A':
				n, err := p.nums(3)
				if err != nil {
					return nil, err
				}
				large, err := p.flag()
				if err != nil {
					return nil, err
				}
				sweep, err := p.flag()
				if err != nil {
					return nil, err
				}
				xy, err := p.nums(2)
				if err != nil {
					return nil, err
				}
				e := abs(xy[0], xy[1])
				cur.pts = appendArc(cur.pts, pos, n[0], n[1], n[2], large, sweep, e)
				pos = e
			default:
				return nil, fmt.Errorf("unsupported path command %q", cmd)
			}
			prev = upper
			_ = lastQ
		}
	}
	finish()
	return out, nil
}

type pathParser struct {
	s string
	i int
}

func (p *pathParser) skip() {
	for p.i < len(p.s) && (p.s[p.i] == ' ' || p.s[p.i] == ',' || p.s[p.i] == '\n' || p.s[p.i] == '\t') {
		p.i++
	}
}

func (p *pathParser) command() (byte, bool) {
	p.skip()
	if p.i >= len(p.s) {
		return 0, false
	}
	c := p.s[p.i]
	if (c >= 'a' && c <= 'z') || (c >= 'A' && c <= 'Z') {
		p.i++
		return c, true
	}
	return 0, false
}

func (p *pathParser) moreNumbers() bool {
	p.skip()
	if p.i >= len(p.s) {
		return false
	}
	c := p.s[p.i]
	return c == '-' || c == '+' || c == '.' || (c >= '0' && c <= '9')
}

func (p *pathParser) num() (float64, error) {
	p.skip()
	start := p.i
	if p.i < len(p.s) && (p.s[p.i] == '-' || p.s[p.i] == '+') {
		p.i++
	}
	seenDot := false
	for p.i < len(p.s) {
		c := p.s[p.i]
		if c >= '0' && c <= '9' {
			p.i++
		} else if c == '.' && !seenDot {
			seenDot = true
			p.i++
		} else {
			break
		}
	}
	v, err := strconv.ParseFloat(p.s[start:p.i], 64)
	if err != nil {
		return 0, fmt.Errorf("bad number at %d in %q", start, p.s)
	}
	return v, nil
}

func (p *pathParser) nums(n int) ([]float64, error) {
	out := make([]float64, n)
	for i := range out {
		v, err := p.num()
		if err != nil {
			return nil, err
		}
		out[i] = v
	}
	return out, nil
}

// flag reads an arc flag, which may be written without a separator ("a2 2 0 0 14 0").
func (p *pathParser) flag() (bool, error) {
	p.skip()
	if p.i >= len(p.s) || (p.s[p.i] != '0' && p.s[p.i] != '1') {
		return false, fmt.Errorf("bad arc flag at %d in %q", p.i, p.s)
	}
	v := p.s[p.i] == '1'
	p.i++
	return v, nil
}

const curveSteps = 24

func appendCubic(pts []point, p0, c1, c2, p3 point) []point {
	for i := 1; i <= curveSteps; i++ {
		t := float64(i) / curveSteps
		u := 1 - t
		pts = append(pts, point{
			u*u*u*p0.x + 3*u*u*t*c1.x + 3*u*t*t*c2.x + t*t*t*p3.x,
			u*u*u*p0.y + 3*u*u*t*c1.y + 3*u*t*t*c2.y + t*t*t*p3.y,
		})
	}
	return pts
}

// appendArc implements the SVG endpoint-to-centre arc conversion (SVG 1.1 appendix F.6).
func appendArc(pts []point, from point, rx, ry, rotDeg float64, large, sweep bool, to point) []point {
	if from == to {
		return pts
	}
	rx, ry = math.Abs(rx), math.Abs(ry)
	if rx == 0 || ry == 0 {
		return append(pts, to)
	}
	phi := rotDeg * math.Pi / 180
	cosP, sinP := math.Cos(phi), math.Sin(phi)
	dx, dy := (from.x-to.x)/2, (from.y-to.y)/2
	x1 := cosP*dx + sinP*dy
	y1 := -sinP*dx + cosP*dy
	if lambda := x1*x1/(rx*rx) + y1*y1/(ry*ry); lambda > 1 {
		s := math.Sqrt(lambda)
		rx, ry = rx*s, ry*s
	}
	num := rx*rx*ry*ry - rx*rx*y1*y1 - ry*ry*x1*x1
	den := rx*rx*y1*y1 + ry*ry*x1*x1
	coef := 0.0
	if den != 0 && num > 0 {
		coef = math.Sqrt(num / den)
	}
	if large == sweep {
		coef = -coef
	}
	cxp, cyp := coef*rx*y1/ry, -coef*ry*x1/rx
	cx := cosP*cxp - sinP*cyp + (from.x+to.x)/2
	cy := sinP*cxp + cosP*cyp + (from.y+to.y)/2
	angle := func(ux, uy, vx, vy float64) float64 {
		a := math.Atan2(ux*vy-uy*vx, ux*vx+uy*vy)
		return a
	}
	th1 := angle(1, 0, (x1-cxp)/rx, (y1-cyp)/ry)
	dth := angle((x1-cxp)/rx, (y1-cyp)/ry, (-x1-cxp)/rx, (-y1-cyp)/ry)
	if !sweep && dth > 0 {
		dth -= 2 * math.Pi
	} else if sweep && dth < 0 {
		dth += 2 * math.Pi
	}
	steps := int(math.Ceil(math.Abs(dth) / (math.Pi / 2) * 12))
	if steps < 1 {
		steps = 1
	}
	for i := 1; i <= steps; i++ {
		th := th1 + dth*float64(i)/float64(steps)
		x, y := rx*math.Cos(th), ry*math.Sin(th)
		pts = append(pts, point{cosP*x - sinP*y + cx, sinP*x + cosP*y + cy})
	}
	pts[len(pts)-1] = to
	return pts
}

// canvas is a premultiplied float RGBA surface; painting blends over, erasing removes coverage (destination-out).
type canvas struct {
	w, h int
	pix  []float32 // r, g, b, a premultiplied, 4 per pixel
	// scale maps design units to pixels; (ox, oy) is where design unit (0,0) lands.
	scale, ox, oy float64
}

func newCanvas(size int, artPx float64) *canvas {
	return &canvas{w: size, h: size, pix: make([]float32, size*size*4), scale: artPx / 24, ox: (float64(size) - artPx) / 2, oy: (float64(size) - artPx) / 2}
}

func (c *canvas) toPx(p point) (float32, float32) {
	return float32(p.x*c.scale + c.ox), float32(p.y*c.scale + c.oy)
}

// coverage rasterizes the polygons into a w x h coverage mask.
func (c *canvas) coverage(polys [][]point) []uint8 {
	z := vector.NewRasterizer(c.w, c.h)
	z.DrawOp = draw.Src
	for _, poly := range polys {
		if len(poly) < 3 {
			continue
		}
		x, y := c.toPx(poly[0])
		z.MoveTo(x, y)
		for _, pt := range poly[1:] {
			x, y = c.toPx(pt)
			z.LineTo(x, y)
		}
		z.ClosePath()
	}
	mask := image.NewAlpha(image.Rect(0, 0, c.w, c.h))
	z.Draw(mask, mask.Bounds(), image.Opaque, image.Point{})
	return mask.Pix
}

func fillPolys(paths []subpath) [][]point {
	polys := make([][]point, 0, len(paths))
	for _, sp := range paths {
		polys = append(polys, sp.pts)
	}
	return polys
}

// paint blends the colour with the given opacity over the surface where the polygons cover it.
func (c *canvas) paint(polys [][]point, col color.NRGBA, opacity float64) {
	mask := c.coverage(polys)
	r, g, b := float32(col.R)/255, float32(col.G)/255, float32(col.B)/255
	for i, m := range mask {
		if m == 0 {
			continue
		}
		a := float32(m) / 255 * float32(opacity) * float32(col.A) / 255
		px := c.pix[i*4 : i*4+4]
		inv := 1 - a
		px[0] = r*a + px[0]*inv
		px[1] = g*a + px[1]*inv
		px[2] = b*a + px[2]*inv
		px[3] = a + px[3]*inv
	}
}

// erase removes the surface where the polygons cover it (a transparent knock-out).
func (c *canvas) erase(polys [][]point) {
	mask := c.coverage(polys)
	for i, m := range mask {
		if m == 0 {
			continue
		}
		keep := 1 - float32(m)/255
		px := c.pix[i*4 : i*4+4]
		for k := range px {
			px[k] *= keep
		}
	}
}

// image converts the surface to a straight-alpha image.
func (c *canvas) image() *image.NRGBA {
	img := image.NewNRGBA(image.Rect(0, 0, c.w, c.h))
	for i := 0; i < c.w*c.h; i++ {
		px := c.pix[i*4 : i*4+4]
		a := px[3]
		if a <= 0 {
			continue
		}
		img.Pix[i*4+0] = clamp8(px[0] / a)
		img.Pix[i*4+1] = clamp8(px[1] / a)
		img.Pix[i*4+2] = clamp8(px[2] / a)
		img.Pix[i*4+3] = clamp8(a)
	}
	return img
}

func clamp8(v float32) uint8 {
	v = v*255 + 0.5
	if v < 0 {
		return 0
	}
	if v > 255 {
		return 255
	}
	return uint8(v)
}

// circlePath and ellipsePath build the arc path data for the shapes the design draws as SVG elements.
func ellipsePath(cx, cy, rx, ry float64) string {
	f := func(v float64) string { return strconv.FormatFloat(v, 'f', -1, 64) }
	return strings.Join([]string{"M", f(cx - rx), " ", f(cy), "a", f(rx), " ", f(ry), " 0 1 0 ", f(2 * rx), " 0a", f(rx), " ", f(ry), " 0 1 0 ", f(-2 * rx), " 0z"}, "")
}
