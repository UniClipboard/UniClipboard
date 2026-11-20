package desktopui

import (
	"fmt"
	"slices"
	"strings"
	"time"

	"github.com/egoist/mygo/ui"
)

// boarduiMenuItems reads the actual in-window accessible menu/listbox. It does
// not fabricate a system-menu request or invoke a command behind the view.
func boarduiMenuItems(tt *ui.Tester) []string {
	var items []string
	for _, node := range tt.AccessAll() {
		if node.Offscreen || node.Name == "" {
			continue
		}
		switch node.Role {
		case ui.RoleMenuItem, ui.RoleMenuItemCheckBox, ui.RoleMenuItemRadio, ui.RoleOption:
			if !slices.Contains(items, node.Name) {
				items = append(items, node.Name)
			}
		}
	}
	return items
}

// boarduiChooseMenu drives visible options and nested menus by pointer. The
// caller's existing state/persistence assertions remain the acceptance verdict.
func boarduiChooseMenu(tt *ui.Tester, path ...string) error {
	if len(path) == 0 {
		return fmt.Errorf("empty menu choice")
	}
	if len(boarduiMenuItems(tt)) == 0 {
		return fmt.Errorf("in-window popup has no accessible menu/listbox items")
	}
	// Failure mode: the history group "今天" or sidebar device name can be
	// identical to an open option. Text-only Click would hit that background
	// label. Resolve a real visible option/menu-item role and use its bounds.
	entry := func(label string) (ui.AccessInfo, error) {
		var matches []ui.AccessInfo
		for _, node := range tt.AccessAll() {
			if node.Offscreen || node.Name != label {
				continue
			}
			switch node.Role {
			case ui.RoleMenuItem, ui.RoleMenuItemCheckBox, ui.RoleMenuItemRadio, ui.RoleOption:
				matches = append(matches, node)
			}
		}
		if len(matches) != 1 {
			return ui.AccessInfo{}, fmt.Errorf("visible popup entry %q matched %d roles", label, len(matches))
		}
		if matches[0].Disabled {
			return ui.AccessInfo{}, fmt.Errorf("popup entry %q is disabled", label)
		}
		if matches[0].Bounds.W <= 0 || matches[0].Bounds.H <= 0 {
			return ui.AccessInfo{}, fmt.Errorf("popup entry %q has no visible bounds", label)
		}
		return matches[0], nil
	}
	for i, label := range path {
		node, err := entry(label)
		if err != nil {
			return err
		}
		x, y := node.Bounds.X+node.Bounds.W/2, node.Bounds.Y+node.Bounds.H/2
		if i < len(path)-1 {
			tt.Move(x, y)
			tt.Advance(300 * time.Millisecond)
			if _, err := entry(path[i+1]); err != nil {
				tt.ClickAt(x, y)
			}
			tt.Advance(300 * time.Millisecond)
		} else {
			tt.ClickAt(x, y)
		}
	}
	return nil
}

// boarduiTableCell scopes a duplicate visible tag label to its real table cell.
// Scroll only that table when a virtual row is outside its viewport. Never send
// an event to the background sidebar merely because its text is identical.
func boarduiTableCell(tt *ui.Tester, tableName, label string) (ui.Rect, error) {
	for attempt := 0; attempt < 4; attempt++ {
		table, ok := tt.AccessByRole(ui.RoleTable, tableName)
		if !ok {
			return ui.Rect{}, fmt.Errorf("table %q missing", tableName)
		}
		within := func(r ui.Rect) bool {
			return r.X >= table.Bounds.X-.1 && r.X+r.W <= table.Bounds.X+table.Bounds.W+.1 && r.Y >= table.Bounds.Y-.1 && r.Y+r.H <= table.Bounds.Y+table.Bounds.H+.1 && r.W > 0 && r.H > 0
		}
		all := tt.AccessAll()
		for _, cell := range all {
			if (cell.Role == ui.RoleCell || cell.Role == ui.RoleRowHeader) && !cell.Offscreen && within(cell.Bounds) && (cell.Name == label || strings.HasPrefix(cell.Name, label+" ")) {
				return cell.Bounds, nil
			}
		}
		for _, text := range all {
			if text.Role != ui.RoleText || text.Name != label || text.Offscreen || !within(text.Bounds) {
				continue
			}
			x, y := text.Bounds.X+text.Bounds.W/2, text.Bounds.Y+text.Bounds.H/2
			for _, cell := range all {
				if cell.Role != ui.RoleCell && cell.Role != ui.RoleRowHeader {
					continue
				}
				r := cell.Bounds
				if !cell.Offscreen && within(r) && x >= r.X && x < r.X+r.W && y >= r.Y && y < r.Y+r.H {
					return r, nil
				}
			}
		}
		if attempt < 3 {
			tt.Scroll(table.Bounds.X+table.Bounds.W/2, table.Bounds.Y+table.Bounds.H/2, 0, table.Bounds.H)
		}
	}
	return ui.Rect{}, fmt.Errorf("visible table cell %q in %q missing", label, tableName)
}

func boarduiTableClick(tt *ui.Tester, tableName, label string, mods ui.Modifiers, secondary bool) error {
	r, err := boarduiTableCell(tt, tableName, label)
	if err != nil {
		return err
	}
	x, y := r.X+r.W/2, r.Y+r.H/2
	if secondary {
		tt.RightClickAt(x, y)
	} else {
		tt.ClickAtWith(mods, x, y)
	}
	return nil
}
