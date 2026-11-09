package uikit

import "github.com/egoist/mygo/ui"

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
	Rows    []TableRow
	Menu    func(ids []string) []MenuEntry
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
		cols[i] = ui.TableColumn{Title: col.Label, ID: col.ID, Width: w, Align: col.Align}
	}
	restore := k.withListTheme(c)
	e := ui.Table(c, &st.list, cols, len(p.Rows), func(row, ci int) {
		cell := p.Rows[row].Cells[p.Columns[ci].ID]
		if cell == nil {
			return
		}
		box := ui.Row(c).AlignItems(ui.Center).MinHeight(lk.rowH*0.6).Padding(0, lk.padX/2)
		box.Children(func() { cell(c) })
	})
	restore()
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
