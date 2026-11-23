package desktopui

import (
	"testing"

	"github.com/uniclipboard/unic/app/desktop/internal/desktopkit"
)

func TestSettingsSidebarItemsHaveDistinctIcons(t *testing.T) {
	seen := make(map[desktopkit.IconName]string)
	var shortcutsIcon, quickPanelIcon desktopkit.IconName
	for _, section := range settingsSections {
		for _, item := range section.items {
			if item.Icon == "" {
				t.Fatalf("settings item %q has empty icon", item.ID)
			}
			if prev, dup := seen[item.Icon]; dup {
				t.Fatalf("settings items %q and %q share icon %q", prev, item.ID, item.Icon)
			}
			seen[item.Icon] = item.ID
			switch item.ID {
			case "shortcuts":
				shortcutsIcon = item.Icon
			case "quickPanel":
				quickPanelIcon = item.Icon
			}
		}
	}
	if shortcutsIcon != desktopkit.IconKeyboard {
		t.Fatalf("shortcuts icon = %q, want %q", shortcutsIcon, desktopkit.IconKeyboard)
	}
	if quickPanelIcon != desktopkit.IconQuickPanel {
		t.Fatalf("quickPanel icon = %q, want %q", quickPanelIcon, desktopkit.IconQuickPanel)
	}
}
