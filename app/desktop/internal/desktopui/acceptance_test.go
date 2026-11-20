package desktopui

import (
	"os"
	"testing"
)

// TestDesktopAcceptance is called only by the canonical Desktop runner.
func TestDesktopAcceptance(t *testing.T) {
	if os.Getenv("UNIC_DESKTOP_E2E") != "1" {
		t.Skip("run make desktop-e2e")
	}
	t.Run("boardui-production-migration", scenarioBoardUIProductionMigration)
	t.Run("i18n", scenarioI18n)
	t.Run("window-chrome", scenarioWindowChrome)
	t.Run("tray-icon-settings", scenarioTrayIconSettings)
	t.Run("boardui-surfaces", scenarioBoardUISurfaces)
	t.Run("interaction-motion", scenarioInteractionMotion)
	t.Run("data-binding", scenarioDesktopLiveData)
	t.Run("boardui-menus", scenarioBoardUIMenus)
	t.Run("quick-panel", scenarioQuickPanel)
	t.Run("quick-panel-screenshots", scenarioQuickPanelScreenshots)
	t.Run("unified-scrollbars", scenarioUnifiedScrollbars)
	t.Run("search-badge-delete", scenarioSearchBadgeDelete)
	t.Run("search-disclosure", scenarioSearchDisclosure)
	t.Run("search-disclosure-reduced-motion", scenarioSearchDisclosureReducedMotion)
	t.Run("search-contract", scenarioHistorySearchProcess)
	t.Run("incremental-browsing", scenarioIncrementalBrowsing)
	t.Run("event-wakeup", scenarioEventWakeup)
	t.Run("image-previews", scenarioImagePreviews)
	t.Run("send-device-menu", scenarioSendDeviceMenu)
	t.Run("sheet-exit-stability", scenarioSheetExitStability)
	t.Run("sheet-motion", scenarioSheetMotion)
	t.Run("sidebar-collapse", scenarioSidebarCollapse)
	t.Run("tag-fixed-columns", scenarioTagFixedColumns)
	t.Run("tag-editor", scenarioTagEditor)
	t.Run("tag-manager", scenarioTagManager)
	t.Run("tag-manager-platforms", scenarioTagManagerPlatforms)
	t.Run("token-search", scenarioTokenSearch)
	t.Run("toolbar-alignment", scenarioToolbarAlignment)
	t.Run("source-application", scenarioSourceApplication)
}
