package uikit

import "github.com/egoist/mygo/ui"

// TextProps selects the type ramp step and colour of Text (contract.ts TextProps).
type TextProps struct {
	Role TextRole
	Tone TextTone
	// Emphasized is bold within the role, e.g. a device name inside a sentence.
	Emphasized bool
	// Monospace uses the platform's monospaced face.
	Monospace bool
	// Large is text read and copied by hand on another device, e.g. an invitation code.
	Large bool
	// Truncate keeps one line with an ellipsis.
	Truncate bool
}

type typeStep struct {
	size, line float32
	weight     int
}

// typeStep returns the type ramp of the skin: Adwaita (GnomeType), AppKit and Windows 11.
func (k Kit) typeStep(role TextRole, large bool) typeStep {
	if large {
		switch k.Skin {
		case MacOS:
			return typeStep{34, 40, 600}
		case Windows:
			return typeStep{k.Px("fontSizeHero700"), k.Px("lineHeightHero700"), 600}
		}
		return typeStep{34, 40, 800}
	}
	switch k.Skin {
	case MacOS:
		return [...]typeStep{RoleBody: {13, 18, 400}, RoleTitle: {15, 20, 700}, RoleHeadline: {13, 16, 700},
			RoleCallout: {12, 16, 400}, RoleCaption: {11, 14, 400}}[role]
	case Windows:
		base3 := typeStep{k.Px("fontSizeBase300"), k.Px("lineHeightBase300"), 400}
		base2 := typeStep{k.Px("fontSizeBase200"), k.Px("lineHeightBase200"), 400}
		switch role {
		case RoleTitle:
			return typeStep{k.Px("fontSizeBase500"), k.Px("lineHeightBase500"), 600}
		case RoleHeadline:
			base3.weight = 600
			return base3
		case RoleCallout, RoleCaption:
			return base2
		}
		return base3
	}
	return [...]typeStep{RoleBody: {14.7, 20, 400}, RoleTitle: {21, 26, 800}, RoleHeadline: {14.7, 20, 700},
		RoleCallout: {13, 18, 400}, RoleCaption: {12, 16, 400}}[role]
}

// toneColor is the semantic text colour of the skin.
func (k Kit) toneColor(t TextTone) ui.Color {
	switch k.Skin {
	case MacOS:
		return k.Color([...]string{TonePrimary: "label", ToneSecondary: "label-2", ToneTertiary: "label-3",
			TonePositive: "green", ToneCritical: "red"}[t])
	case Windows:
		return k.Color([...]string{TonePrimary: "colorNeutralForeground1", ToneSecondary: "colorNeutralForeground2",
			ToneTertiary: "colorNeutralForeground3", TonePositive: "w11-status-positive", ToneCritical: "w11-status-critical"}[t])
	}
	return k.Color([...]string{TonePrimary: "fg", ToneSecondary: "fg2", ToneTertiary: "fg3",
		TonePositive: "success-text", ToneCritical: "destructive-text"}[t])
}

// Text shows s in the skin's type ramp.
func (k Kit) Text(c *ui.Context, s string, p TextProps) ui.Element {
	st := k.typeStep(p.Role, p.Large)
	w := st.weight
	if p.Emphasized {
		w = emphasisWeight(k.Skin, w)
	}
	e := ui.Text(c, s).FontSize(st.size).FixedLineHeight(st.line).FontWeight(w).TextColor(k.toneColor(p.Tone))
	if p.Monospace {
		e.Font("monospace")
		if p.Large {
			e.LetterSpacing(0.08 * st.size)
		}
	}
	if p.Truncate {
		e.SingleLine()
	}
	return e
}

func emphasisWeight(s SkinID, w int) int {
	switch s {
	case MacOS, Windows:
		return max(w, 600)
	}
	return max(w, 700)
}
