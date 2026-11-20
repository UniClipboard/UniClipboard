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

// Surface is an application region, not an additional official BoardUI component.
func (k Kit) Surface(c *ui.Context, region SurfaceRegion) ui.Element {
	name := "background-primary-default"
	if region == RegionSidebar {
		name = "background-secondary-default"
	}
	return ui.Column(c).MinWidth(0).MinHeight(0).Background(k.Color(name))
}
