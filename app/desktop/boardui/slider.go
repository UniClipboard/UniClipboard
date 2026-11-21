package boardui

import (
	"fmt"
	"time"

	"github.com/egoist/mygo/ui"
)

// SliderProps implements a source single horizontal slider. The exact-value
// bubble is visible by default. Range and RTL are outside this API.
type SliderProps struct {
	Label, ThumbLabel                string
	Min, Max, Step                   float64
	Disabled, HideLabel, HideTooltip bool
	FormatValue                      func(float64) string
}

// Slider delegates dragging, keyboard range changes and accessibility to SliderBase.
func (k Kit) Slider(c *ui.Context, value *float64, p SliderProps) ui.Element {
	if value == nil {
		panic("boardui: Slider requires Value")
	}
	if p.Max <= p.Min {
		panic("boardui: Slider requires Max > Min")
	}
	var slider ui.Element
	ui.Column(c).MinWidth(0).Gap(8).Children(func() {
		if p.Label != "" && !p.HideLabel {
			k.Text(c.Key("label"), p.Label, "body-medium")
		}
		name := p.ThumbLabel
		if name == "" {
			name = p.Label
		}
		if name == "" {
			name = "Value"
		}
		slider = ui.SliderBase(c.Key("slider"), value, p.Min, p.Max).Label(name).Disabled(p.Disabled).DimDisabled(false).FocusRing(false).Height(32).Width(100).Padding(0)
		step := p.Step
		if step <= 0 {
			step = 1
		}
		slider.Step(step)
		if !p.HideTooltip {
			slider.Margin(32, 0, 0)
		}
		if p.Disabled {
			slider.Opacity(.5)
		}
		fraction := float32((*value - p.Min) / (p.Max - p.Min))
		fraction = min(max(fraction, 0), 1)
		dragging := slider.Pressed()
		a := ui.ElementTransition{Duration: 150 * time.Millisecond, Ease: ui.CubicBezier(.25, .1, .25, 1), Colors: true, Shadows: true}
		slider.Children(func() {
			fill := "background-tertiary-default"
			if slider.Hovered() {
				fill = "background-tertiary-hover"
			}
			ui.Box(c.Key("track")).Absolute().Left(0).Right(0).Top(13).Height(6).Radius(3).Background(Token(k.Dark, fill)).InsetShadow(0, 1, 1, 0, ui.RGBA(0, 0, 0, .06)).Transition(a).AccessHidden(true)
			ui.Box(c.Key("selected")).Absolute().Left(0).Top(13).WidthPercent(fraction*100).Height(6).Radius(3).
				Gradient(Token(k.Dark, "accent-500"), Token(k.Dark, "accent-600"), 90).InsetShadow(0, 1, 0, 0, ui.RGBA(255, 255, 255, .24)).Shadow(0, 1, 2, 0, ui.RGBA(4, 80, 226, .16)).AccessHidden(true)
			thumb := ui.Box(c.Key("thumb")).Absolute().LeftPercent(fraction*100).Top(6).Translate(-10, 0).Size(20, 20).Radius(10).Center().
				Background(Token(k.Dark, "control-indicator-background")).Border(1, Token(k.Dark, "border-button-default")).
				AccessHidden(true)
			if !dragging {
				thumb.Shadow(0, 2, 4, 0, ui.RGBA(0, 0, 0, .1)).InsetShadow(0, 1, 0, 0, ui.RGBA(255, 255, 255, .8))
			}
			scale := float32(1)
			if thumb.Hovered() && !p.Disabled {
				scale = 1.05
				thumb.Border(1, Token(k.Dark, "border-button-hover"))
			}
			if dragging && !p.Disabled {
				scale = 1.1
				thumb.Border(1, Token(k.Dark, "accent-500")).Shadow(0, 3, 8, 0, Token(k.Dark, "accent-600").Alpha(.22))
			}
			if p.Disabled {
				scale = 1
			}
			thumb.AnimatePose(ui.Pose{Scale: scale, X: -10}, ui.Animation{Duration: 150 * time.Millisecond, Ease: ui.CubicBezier(.25, .1, .25, 1)})
			if slider.FocusVisible() {
				thumb.Draw(func(pt *ui.Painter, r ui.Rect) { pt.FocusRing(r, [4]float32{10, 10, 10, 10}) })
			}
			thumb.Children(func() {
				from, to := Token(k.Dark, "accent-500"), Token(k.Dark, "accent-600")
				if dragging || slider.Focused() {
					from = sourceBrightness(from, 1.1)
					to = sourceBrightness(to, 1.1)
				}
				ui.Box(c.Key("dot")).Size(8, 8).Radius(4).Gradient(from, to, 180).InsetShadow(0, 1, 0, 0, ui.RGBA(255, 255, 255, .24))
				if !p.HideTooltip {
					text := fmt.Sprintf("%g", *value)
					if p.FormatValue != nil {
						text = p.FormatValue(*value)
					}
					bubble := ui.Box(c.Key("bubble")).Absolute().Bottom(29).LeftPercent(50).TranslatePercent(-50, 0).Height(28).MinWidth(32).PaddingX(8).Radius(8).Border(1, Token(k.Dark, "border-button-default")).Background(Token(k.Dark, "background-primary-default")).Center().AccessHidden(true)
					shadow := "xs"
					if dragging || slider.Focused() {
						bubble.TranslatePercent(-50, 0).Translate(0, -2)
						shadow = "dropdown"
					}
					k.shadows(bubble, shadow)
					bubble.Children(func() {
						k.Text(c.Key("value"), text, "caption-1-medium").SingleLine()
						ui.Box(c.Key("arrow")).Absolute().Bottom(-4.5).LeftPercent(50).TranslatePercent(-50, 0).Rotate(45).Size(8, 8).Background(Token(k.Dark, "background-primary-default")).Border(1, Token(k.Dark, "border-button-default")).BorderWidth(0, 1, 1, 0)
					})
				}
			})
		})
	})
	return slider
}

// sourceBrightness implements the source brightness-110 sRGB channel filter.
func sourceBrightness(c ui.Color, scale float32) ui.Color {
	c.R = uint8(min(float32(c.R)*scale+0.5, 255))
	c.G = uint8(min(float32(c.G)*scale+0.5, 255))
	c.B = uint8(min(float32(c.B)*scale+0.5, 255))
	return c
}
