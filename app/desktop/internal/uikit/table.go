package uikit

import (
	"maps"
	"slices"

	"github.com/egoist/mygo/ui"
)

// TableColumn 对应 contract.ts TableColumn。
type TableColumn struct {
	ID    string
	Label string
	// Align 为 ui.Start / ui.Center / ui.End；零值按 Start。
	Align ui.Align
	// Fill 占满剩余宽度。
	Fill bool
	// Width 是固定宽度（Fill 为 false 时；0 用默认 140）。
	Width float32
}

// TableRow 对应 contract.ts TableRow：Cells 按列 id 给出单元格内容（在单元格内构建）。
type TableRow struct {
	ID    string
	Cells map[string]func(c *ui.Context)
}

// TableProps 对应 contract.ts TableProps。选区与事件在 *ListState（键盘同 GroupedList：
// 上下移动、Shift 扩选、全选、平台删除键）。
type TableProps struct {
	Label   string
	Columns []TableColumn
	// FixedColumns disables column resizing and reordering for this table.
	FixedColumns bool
	Rows         []TableRow
	Menu         func(ids []string) []MenuEntry
	// Empty 在没有行时代替表格显示。
	Empty string
}

// Table 是多列表格：mygo 的虚拟 Table，选中色按皮肤覆盖。
func (k Kit) Table(c *ui.Context, st *ListState, p TableProps) ui.Element {
	lk := k.listLook()
	if len(p.Rows) == 0 {
		st.begin()
		st.bind(nil, nil)
		st.sel.Clear()
		return ui.Column(c).Grow(1).AlignItems(ui.Center).Justify(ui.Center).Label(p.Label).Children(func() {
			k.Text(c, p.Empty, TextProps{Tone: ToneSecondary})
		})
	}
	ids := make([]string, len(p.Rows))
	for i, r := range p.Rows {
		ids[i] = r.ID
	}
	st.begin()
	st.bind(ids, nil)
	st.list.Label = nil
	cols := make([]ui.TableColumn, len(p.Columns))
	for i, col := range p.Columns {
		w := col.Width
		if col.Fill {
			w = 0
		} else if w == 0 {
			w = 140
		}
		cols[i] = ui.TableColumn{Title: col.Label, ID: col.ID, Width: w, Align: col.Align, Fixed: p.FixedColumns}
	}
	// mygo persists a resized filling column as a fixed width. Keep a trailing
	// flexible column when that removes the final source of width allocation.
	hasFill, hasFlex := false, false
	for i, col := range p.Columns {
		hasFill = hasFill || col.Fill
		w := cols[i].Width
		if saved, ok := st.list.Columns.Widths[col.ID]; ok {
			w = saved
		}
		hasFlex = hasFlex || w <= 0
	}
	order := slices.DeleteFunc(slices.Clone(st.list.Columns.Order), func(id string) bool {
		return !slices.ContainsFunc(cols, func(col ui.TableColumn) bool { return col.ID == id })
	})
	for _, col := range cols {
		if !slices.Contains(order, col.ID) {
			order = append(order, col.ID)
		}
	}
	if hasFill && !hasFlex {
		last := order[len(order)-1]
		for i := range cols {
			if cols[i].ID == last {
				cols[i].MinWidth = max(cols[i].Width, 16)
				cols[i].Width = 0
				break
			}
		}
		st.list.Columns.Widths = maps.Clone(st.list.Columns.Widths)
		delete(st.list.Columns.Widths, last)
	}

	if hasFill && st.tableWidth > 0 {
		for i := range cols {
			var reserved float32
			before := true
			for _, id := range order {
				if id == cols[i].ID {
					before = false
					continue
				}
				j := slices.IndexFunc(cols, func(col ui.TableColumn) bool { return col.ID == id })
				w := cols[j].Width
				if saved, ok := st.list.Columns.Widths[id]; ok {
					w = saved
				}
				if w <= 0 {
					w = max(cols[j].MinWidth, 16)
					// mygo freezes flexible columns before the dragged divider.
					if before {
						w = max(w, st.tableColumnWidths[id])
					}
				}
				reserved += max(w, cols[j].MinWidth, 16)
			}
			cols[i].MaxWidth = max(cols[i].MinWidth, 16, st.tableWidth-reserved)
		}
	}

	restore := k.withListTheme(c)
	const macSpacing = float32(3.25)
	st.list.RoundRows = k.Skin == MacOS
	if st.tableColumnWidths == nil {
		st.tableColumnWidths = make(map[string]float32)
	}
	st.list.HeaderCell = func(cell ui.Element, i int) {
		st.tableColumnWidths[cols[i].ID] = cell.Bounds().W
		if k.Skin == MacOS {
			cell.Padding(4, 12).FontSize(11).TextColor(k.Color("label-2"))
		}
	}
	if k.Skin == MacOS {
		theme := *c.Theme()
		theme.Spacing = macSpacing
		theme.Radius = 6
		c.SetTheme(&theme)
	}
	e := ui.Table(c, &st.list, cols, len(p.Rows), func(row, ci int) {
		cell := p.Rows[row].Cells[p.Columns[ci].ID]
		if cell == nil {
			return
		}
		box := ui.Row(c).AlignItems(ui.Center).MinHeight(lk.rowH*0.6).Padding(0, lk.padX/2)
		if k.Skin == MacOS {
			box.MinHeight(20).Padding(0, 4).Gap(8).Grow(1).MinWidth(0).Justify(p.Columns[ci].Align)
			if row%2 == 1 && !st.Has(p.Rows[row].ID) {
				stripe := k.Color("alt-row")
				// Extend through the native cell padding; the cell clips the stripe.
				box.Draw(func(painter *ui.Painter, r ui.Rect) {
					px, py := macSpacing*2.5, macSpacing*1.5
					painter.Fill(ui.Rect{X: r.X - px, Y: r.Y - py, W: r.W + 2*px, H: r.H + 2*py}, stripe, 0)
				})
			}
		}
		box.Children(func() { cell(c) })
	})
	restore()
	st.tableWidth = e.Bounds().W
	e.Label(p.Label)
	k.tableSelectAll(e, st)
	st.settle(e, k)
	st.listContextMenu(e, p.Menu)
	return e
}

func (k Kit) tableSelectAll(e ui.Element, st *ListState) {
	if e.Shortcut(ui.Cmd, ui.KeyA) {
		for _, id := range st.ids {
			if id != "" {
				st.sel.Add(id)
			}
		}
	}
}
