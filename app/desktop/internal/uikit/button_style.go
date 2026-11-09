package uikit

import (
	"strconv"
	"strings"

	"github.com/egoist/mygo/ui"
)

// cssShadowLayer is one layer of a CSS box-shadow token.
type cssShadowLayer struct {
	inset              bool
	x, y, blur, spread float32
	color              ui.Color
}

// cssSplit splits s on sep outside parentheses.
func cssSplit(s string, sep func(r rune) bool) []string {
	var out []string
	depth, start := 0, 0
	for i, r := range s {
		switch {
		case r == '(':
			depth++
		case r == ')':
			depth--
		case depth == 0 && sep(r):
			if i > start {
				out = append(out, strings.TrimSpace(s[start:i]))
			}
			start = i + 1
		}
	}
	if t := strings.TrimSpace(s[start:]); t != "" {
		out = append(out, t)
	}
	return out
}

// cssParseShadows reads a CSS box-shadow list such as
// "0 0.5px 1.5px rgba(0, 0, 0, 0.2), inset 0 0 0 0.5px #0001". "none" or unknown yields nil.
func cssParseShadows(raw string) []cssShadowLayer {
	var out []cssShadowLayer
	for _, layer := range cssSplit(raw, func(r rune) bool { return r == ',' }) {
		var sh cssShadowLayer
		var nums []float32
		for _, f := range cssSplit(layer, func(r rune) bool { return r == ' ' }) {
			switch {
			case f == "inset":
				sh.inset = true
			case strings.HasPrefix(f, "rgb"), strings.HasPrefix(f, "#"), f == "transparent":
				sh.color, _ = parseColor(f)
			default:
				v, err := strconv.ParseFloat(strings.TrimSuffix(f, "px"), 32)
				if err == nil {
					nums = append(nums, float32(v))
				}
			}
		}
		if len(nums) < 2 || sh.color.A == 0 {
			continue
		}
		sh.x, sh.y = nums[0], nums[1]
		if len(nums) > 2 {
			sh.blur = nums[2]
		}
		if len(nums) > 3 {
			sh.spread = nums[3]
		}
		out = append(out, sh)
	}
	return out
}

// cssApplyShadows paints every layer of a CSS box-shadow list on e.
func cssApplyShadows(e ui.Element, raw string) ui.Element {
	for _, s := range cssParseShadows(raw) {
		if s.inset {
			e.InsetShadow(s.x, s.y, s.blur, s.spread, s.color)
		} else {
			e.Shadow(s.x, s.y, s.blur, s.spread, s.color)
		}
	}
	return e
}

// shade darkens (amount > 0) or lightens (< 0) a colour, for pressed states derived from the base.
func cssShade(c ui.Color, amount float32) ui.Color {
	if amount < 0 {
		return c.MixOklab(ui.RGB(255, 255, 255), -amount)
	}
	return c.MixOklab(ui.RGB(0, 0, 0), amount)
}
