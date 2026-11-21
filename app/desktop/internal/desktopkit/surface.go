package desktopkit

import (
	"github.com/egoist/mygo/ui"

	"github.com/uniclipboard/unic/app/desktop/boardui"
)

// UiRoot installs the single BoardUI scheme for all supported desktop hosts.
func (k Kit) UiRoot(c *ui.Context, body func()) ui.Element {
	k.Apply(c)
	return ui.Column(c).Fill().Font(boardui.FontFamily).FontSize(k.baseFont()).TextColor(k.Color("text-primary")).Background(k.Color("background-full")).Children(body)
}
func (k Kit) baseFont() float32 { return boardui.Typography("body-regular").Size }

// workspaceCanvas returns the unified primary workspace background for the toolbar,
// list/grid browsing panes and detail pane. In light mode this is white; in dark mode
// it steps gently above the sidebar (#171717 -> ~#1D1D1D) while staying cleanly below
// elevated cards (#262626), avoiding an inverted dark-bright-black split.
func (k Kit) workspaceCanvas() ui.Color {
	if k.Dark {
		return k.Color("background-secondary-default").MixOklab(k.Color("background-primary-default"), .38)
	}
	return k.Color("background-primary-default")
}

// WorkspaceSurface returns the unified primary workspace background for toolbars,
// list/grid browsing panes, detail panes and overview canvases.
func (k Kit) WorkspaceSurface() ui.Color {
	return k.workspaceCanvas()
}

// CardSurface returns the elevated card surface (grid cards, preview frames, inset lists).
func (k Kit) CardSurface() ui.Color {
	return k.Color("background-primary-default")
}

// SubtleCardSurface returns the fill for secondary metadata/link cards placed on the
// main detail canvas: quiet tinted gray in light mode, elevated card surface in dark mode.
func (k Kit) SubtleCardSurface() ui.Color {
	if k.Dark {
		return k.Color("background-primary-default")
	}
	return k.Color("background-secondary-default")
}

// DrawerInnerSurface returns the recessed card fill for metadata/link cards placed
// inside the already-elevated D1-b floating drawer.
func (k Kit) DrawerInnerSurface() ui.Color {
	if k.Dark {
		return k.workspaceCanvas()
	}
	return k.Color("background-secondary-default")
}

// Surface is an application region, not an additional official BoardUI component.
func (k Kit) Surface(c *ui.Context, region SurfaceRegion) ui.Element {
	bg := k.workspaceCanvas()
	if region == RegionSidebar {
		bg = k.Color("background-secondary-default")
	}
	return ui.Column(c).MinWidth(0).MinHeight(0).Background(bg)
}
