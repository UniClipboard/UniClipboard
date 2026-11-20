package desktopkit

import (
	"github.com/egoist/mygo/ui"

	"github.com/uniclipboard/unic/app/desktop/boardui"
)

type SegmentedItem struct {
	Value, Label string
	Icon         IconName
	IconOnly     bool
}
type SegmentedControlProps struct {
	Items   []SegmentedItem
	Value   *string
	Label   string
	Toolbar bool
}
type SegmentedResult struct {
	Element ui.Element
	Changed bool
}

func (k Kit) SegmentedControl(c *ui.Context, p SegmentedControlProps) SegmentedResult {
	host := ui.Box(c).MinWidth(0)
	value := p.Value
	if value == nil {
		value = ui.Local(host, "value", func() string { return "" })
	}
	before := *value
	items := make([]boardui.SegmentedItem, len(p.Items))
	for i, it := range p.Items {
		items[i] = boardui.SegmentedItem{Value: it.Value, Label: it.Label, IconOnly: it.IconOnly}
		if it.Icon != "" {
			icon := it.Icon
			idx := i
			items[i].Leading = func(size float32) { k.Icon(c.Key(idx), icon, size).AccessHidden(true) }
		}
	}
	var track ui.Element
	host.Children(func() {
		track = k.native().SegmentedControl(c.Key("segments"), boardui.SegmentedProps{Value: value, Items: items, Label: p.Label})
	})
	return SegmentedResult{Element: track, Changed: before != *value}
}
