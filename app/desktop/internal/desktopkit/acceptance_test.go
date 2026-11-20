package desktopkit

import (
	"os"
	"testing"

	"github.com/egoist/mygo/ui"

	"github.com/uniclipboard/unic/app/desktop/boardui"
	proof "github.com/uniclipboard/unic/app/desktop/internal/e2eproof"
)

// Existing complete widget-view scenarios are retained under one canonical
// BoardUI composition case. This is a migration of existing coverage, not new
// unit tests written after product code. Source-token and parser unit tests
// were removed; code-field normalization remains covered through real input.
func TestDesktopAcceptance(t *testing.T) {
	if os.Getenv("UNIC_DESKTOP_E2E") != "1" {
		t.Skip("run make desktop-e2e CASE=boardui-composition")
	}
	t.Run("boardui-composition", scenarioDesktopKitComposition)
}

func scenarioDesktopKitComposition(t *testing.T) {
	if err := boardui.LoadFonts(); err != nil {
		t.Fatal(err)
	}
	t.Run("ActionBar", scenarioCompositionActionBar)
	t.Run("Button", scenarioCompositionButton)
	t.Run("ButtonRowRemove", scenarioCompositionButtonRowRemove)
	t.Run("ChoiceList", scenarioCompositionChoiceList)
	t.Run("CodeField", scenarioCompositionCodeField)
	t.Run("ComboField", scenarioCompositionComboField)
	t.Run("AlertDialogConfirmCancel", scenarioCompositionAlertDialogConfirmCancel)
	t.Run("AlertDialogEscapeAndBusy", scenarioCompositionAlertDialogEscapeAndBusy)
	t.Run("AlertDialogConfirmDisabled", scenarioCompositionAlertDialogConfirmDisabled)
	t.Run("EntryPage", scenarioCompositionEntryPage)
	t.Run("FactList", scenarioCompositionFactList)
	t.Run("TextField", scenarioCompositionTextField)
	t.Run("SearchField", scenarioCompositionSearchField)
	t.Run("SearchToken", scenarioCompositionSearchToken)
	t.Run("FormField", scenarioCompositionFormField)
	t.Run("GroupedGrid", scenarioCompositionGroupedGrid)
	t.Run("GroupedGridReveal", scenarioCompositionGroupedGridReveal)
	t.Run("DetailHeader", scenarioCompositionDetailHeader)
	t.Run("EntityHeader", scenarioCompositionEntityHeader)
	t.Run("TagNoticeDisclosure", scenarioCompositionTagNoticeDisclosure)
	t.Run("Indicators", scenarioCompositionIndicators)
	t.Run("IconsAndText", scenarioCompositionIconsAndText)
	t.Run("Surface", scenarioCompositionSurface)
	t.Run("InsetList", scenarioCompositionInsetList)
	t.Run("GroupedListGroupsKeysAndWindow", scenarioCompositionGroupedListGroupsKeysAndWindow)
	t.Run("GroupedListViewportPlaceholders", scenarioCompositionGroupedListViewportPlaceholders)
	t.Run("ItemListSelectKeysDelete", scenarioCompositionItemListSelectKeysDelete)
	t.Run("ItemListMultiSelect", scenarioCompositionItemListMultiSelect)
	t.Run("ItemListEmptyAndHighlight", scenarioCompositionItemListEmptyAndHighlight)
	t.Run("MasterDetail", scenarioCompositionMasterDetail)
	t.Run("MenuChoose", scenarioCompositionMenuChoose)
	t.Run("MenuKeyboardAndEscape", scenarioCompositionMenuKeyboardAndEscape)
	t.Run("ContextMenu", scenarioCompositionContextMenu)
	t.Run("Pane", scenarioCompositionPane)
	t.Run("PreviewFrame", scenarioCompositionPreviewFrame)
	t.Run("QuickLookKeysAndClose", scenarioCompositionQuickLookKeysAndClose)
	t.Run("TokenSearchField", scenarioCompositionTokenSearchField)
	t.Run("SegmentedControl", scenarioCompositionSegmentedControl)
	t.Run("Select", scenarioCompositionSelect)
	t.Run("Settings", scenarioCompositionSettings)
	t.Run("SheetDoneEscapeFocus", scenarioCompositionSheetDoneEscapeFocus)
	t.Run("SidebarNav", scenarioCompositionSidebarNav)
	t.Run("SidebarNavCompact", scenarioCompositionSidebarNavCompact)
	t.Run("SidebarNavEntryStatus", scenarioCompositionSidebarNavEntryStatus)
	t.Run("SourceList", scenarioCompositionSourceList)
	t.Run("SourceListDisabledAction", scenarioCompositionSourceListDisabledAction)
	t.Run("SplitView", scenarioCompositionSplitView)
	t.Run("SplitViewOverlayEscape", scenarioCompositionSplitViewOverlayEscape)
	t.Run("StepList", scenarioCompositionStepList)
	t.Run("Table", scenarioCompositionTable)
	t.Run("TableEmpty", scenarioCompositionTableEmpty)
	t.Run("TaskPanel", scenarioCompositionTaskPanel)
	t.Run("ToggleControls", scenarioCompositionToggleControls)
	t.Run("ToggleDisabled", scenarioCompositionToggleDisabled)
	t.Run("ToolbarButtons", scenarioCompositionToolbarButtons)
	t.Run("WithTooltip", scenarioCompositionWithTooltip)
	t.Run("Transfer", scenarioCompositionTransfer)
	t.Run("ContentUnavailable", scenarioCompositionContentUnavailable)
}

// compositionTester binds every exercised view to a final PNG, including
// failure states. The canonical runner creates its isolated artifact directory.
func compositionTester(t *testing.T, view func(*ui.Context), width, height int) *ui.Tester {
	t.Helper()
	tt := ui.NewTester(view, width, height)
	proof.Capture(t, tt)
	return tt
}
