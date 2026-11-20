package desktopui

import (
	"fmt"

	"github.com/egoist/mygo/ui"

	"github.com/uniclipboard/unic/app/desktop/internal/desktopkit"
)

// folderPreview shows a folder entry as a header (icon, name, nature) and,
// for snapshots, a summary line and an indented tree.
func (d *viewState) folderPreview(c *ui.Context, k desktopkit.Kit, v *entry, secondary desktopkit.TextProps) {
	info := v.folder
	if info == nil {
		info = &folderInfo{}
	}
	ui.Column(c.Key("folder")).FillWidth().Padding(20).Gap(14).Children(func() {
		for i, name := range info.names {
			ui.Row(c.Key(fmt.Sprint("head", i))).Gap(14).Children(func() {
				k.KindBadge(c.Key(fmt.Sprint("badge", i)), desktopkit.KindBadgeProps{Kind: desktopkit.KindFolder, Label: d.tr("文件夹")})
				ui.Column(c.Key(fmt.Sprint("title", i))).Gap(2).Children(func() {
					k.Text(c, name, desktopkit.TextProps{})
				})
			})
		}
		if info.listError {
			k.Text(c, d.tr("无法读取文件夹，可能已移动或删除。"), secondary)
			return
		}
		k.Text(c, fmt.Sprintf("%d %s · %d %s · %s", info.files, d.tr("个文件"), info.dirs, d.tr("个文件夹"), humanBytes(info.bytes)), secondary)
		for i, r := range info.rows {
			ui.Row(c.Key(fmt.Sprint("row", i))).Gap(8).Padding(0, 0, 0, float32(r.depth)*16).Children(func() {
				icon := desktopkit.IconFile
				if r.dir {
					icon = desktopkit.IconFolder
				}
				k.Icon(c, icon, 14)
				k.Text(c, r.name, desktopkit.TextProps{})
				if !r.dir {
					k.Text(c, humanBytes(r.size), secondary)
				}
			})
		}
		if info.truncated {
			k.Text(c, d.tr("仅显示前 200 项"), secondary)
		}
	})
}
