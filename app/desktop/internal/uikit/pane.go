package uikit

import "github.com/egoist/mygo/ui"

// PaneKind is the role of a column of the content region below the toolbar band.
type PaneKind int

const (
	// PaneList is the browsing column beside a detail (the history list).
	PaneList PaneKind = iota
	// PaneDetail fills the remaining width.
	PaneDetail
	// PaneMain is a browsing column that fills the width by itself (the grid without its inspector).
	PaneMain
)

// PaneProps mirrors contract.ts PaneProps.
type PaneProps struct {
	Kind  PaneKind
	Label string // accessible name of the region
	// DetailOpen is the same pointer given to SplitViewProps.DetailOpen. In the single-pane
	// layout (window narrower than SinglePaneWidth) the list shows while it is false and the
	// detail while it is true; the hidden pane is not built.
	DetailOpen *bool
}

// Pane is one column of the SplitView content region; body builds its content.
func (k Kit) Pane(c *ui.Context, p PaneProps, body func()) ui.Element {
	t := k.layTok()
	w, _ := c.Size()
	if p.DetailOpen != nil && w < SinglePaneWidth {
		if (p.Kind == PaneDetail) != *p.DetailOpen && p.Kind != PaneMain {
			return ui.Box(c).Size(0, 0).AccessHidden(true)
		}
	}
	e := ui.Column(c).Role(ui.RoleRegion).Label(p.Label).MinWidth(0).MinHeight(0).AlignItems(ui.Stretch)
	single := p.DetailOpen != nil && w < SinglePaneWidth
	list, detail := float32(470), float32(340)
	listMin, detailMin := float32(300), float32(300)
	switch k.Skin {
	case MacOS:
		list = 500
	case Windows:
		list, listMin, detail, detailMin = 480, 288, 336, 288
	}
	switch p.Kind {
	case PaneList:
		if single {
			e.Grow(1)
		} else {
			e.Basis(list).Grow(0).Shrink(1).MinWidth(listMin)
		}
		switch k.Skin {
		case Linux:
			e.Background(t.View).InsetShadow(-1, 0, 0, 0, t.Border).Padding(12, 0, 0)
		case MacOS:
			e.InsetShadow(-0.5, 0, 0, 0, t.Border)
		default:
			e.InsetShadow(-1, 0, 0, 0, t.Border)
		}
	case PaneDetail:
		if single {
			e.Grow(1)
		} else {
			e.Basis(detail).Grow(1).Shrink(1).MinWidth(detailMin)
		}
		e.Background(t.Detail)
		if k.Skin == Linux {
			e.Padding(12, 0, 0)
		}
	default:
		e.Grow(1)
		if k.Skin == Linux {
			e.Padding(12, 0, 0)
		}
	}
	return e.Children(func() {
		if body != nil {
			body()
		}
	})
}
