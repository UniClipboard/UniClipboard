package boardui

import "github.com/egoist/mygo/ui"

// InputProps dresses mygo's mature text editor. Required displays the source
// marker; validation stays with the caller. The current mygo accessibility
// bridge has no separate required state, so it is also announced in description.
type InputProps struct {
	Label, Description, Error, Placeholder, Size string
	Password, Disabled, ReadOnly, Required       bool
	// HideLabel keeps Label accessible without painting a duplicate label.
	HideLabel bool
	MaxLength int
}

// Input renders the composed label, editor and description/error. It returns
// the actual editable element, not the field container.
func (k Kit) Input(c *ui.Context, value *string, p InputProps) ui.Element {
	if value == nil {
		panic("boardui: Input requires Value")
	}
	m := Geometry("input", normalizedSize(p.Size))
	name := p.Label
	if name == "" {
		name = p.Placeholder
	}
	var input ui.Element
	ui.Column(c).MinWidth(0).Gap(Number("input-field-gap")).Children(func() {
		var label ui.Element
		if p.Label != "" && !p.HideLabel {
			ui.Row(c.Key("label-row")).Gap(2).Children(func() {
				label = k.Text(c.Key("label"), p.Label, "body-medium")
				if p.Required {
					k.Text(c.Key("required"), "*", "body-medium").TextColor(k.Color("text-error-primary")).AccessHidden(true)
				}
			})
		}
		input = ui.TextInputBase(c.Key("editor"), value).Label(name).
			Placeholder(p.Placeholder).Disabled(p.Disabled).DimDisabled(false).ReadOnly(p.ReadOnly).
			Invalid(p.Error != "").Height(m.Height).PaddingStart(m.PaddingX + Number("input-inner-start")).PaddingEnd(m.PaddingX).
			Radius(m.Radius).MinWidth(0).FocusRing(false)
		dressType(input, "body-regular")
		if p.Label != "" && !p.HideLabel {
			input.LabelledBy(label)
		}
		if p.MaxLength > 0 {
			input.MaxLength(p.MaxLength)
		}
		if p.Password {
			input.Password()
		}
		fill := "background-tertiary-default"
		ink := "text-primary"
		placeholder := "text-tertiary"
		if p.Disabled {
			fill = "input-disabled-background"
			ink = "input-disabled-text"
			placeholder = "input-disabled-text"
		}
		if p.Error != "" {
			fill = "background-tertiary-error"
			placeholder = "text-error-placeholder"
		}
		if input.Focused() && p.Error == "" && !p.Disabled {
			placeholder = "text-primary"
		}
		input.Background(k.Color(fill)).TextColor(k.Color(ink)).PlaceholderColor(k.Color(placeholder))
		if p.Error == "" && !p.Disabled {
			if input.Focused() {
				input.InsetShadow(0, 0, 0, 2, k.Color("border-button-active"))
			} else if input.Hovered() {
				input.InsetShadow(0, 0, 0, 2, k.Color("border-button-hover"))
			}
		}
		description := p.Description
		if p.Required {
			input.Description("Required.")
		}
		if description != "" && p.Error == "" {
			hint := k.Text(c.Key("description"), description, "caption-1-medium").Padding(Number("input-hint-padding-top"), 0, 0).TextColor(k.Color("text-secondary"))
			input.DescribedBy(hint)
		}
		if p.Error != "" {
			err := k.Text(c.Key("error"), p.Error, "caption-1-medium").Padding(Number("input-hint-padding-top"), 0, 0).TextColor(k.Color("text-error-primary"))
			input.ErrorMessage(err)
		}
	})
	return input
}
