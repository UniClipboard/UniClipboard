package desktopkit

import (
	"github.com/egoist/mygo/ui"

	"github.com/uniclipboard/unic/app/desktop/boardui"
)

type SelectItem struct{ Value, Label string }
type SelectProps struct {
	Items              []SelectItem
	Value              *string
	Label, Placeholder string
	Disabled, Filter   bool
}
type SelectResult struct {
	Element ui.Element
	Changed bool
}

func (k Kit) Select(c *ui.Context, p SelectProps) SelectResult {
	host := ui.Column(c).MinWidth(0)
	value := p.Value
	if value == nil {
		value = ui.Local(host, "value", func() string { return "" })
	}
	before := *value
	items := make([]boardui.SelectItem, len(p.Items))
	for i, it := range p.Items {
		items[i] = boardui.SelectItem{Value: it.Value, Label: it.Label}
	}
	size := "md"
	if p.Filter {
		size = "sm"
	}
	var trigger ui.Element
	host.Children(func() {
		trigger = k.native().Select(c.Key("select"), boardui.SelectProps{Value: value, Items: items, Label: p.Label, HideLabel: true, Placeholder: p.Placeholder, Disabled: p.Disabled, Size: size})
	})
	return SelectResult{Element: trigger, Changed: before != *value}
}
