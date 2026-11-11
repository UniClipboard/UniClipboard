# Desktop acceptance migration

All original assertions and subcase matrices are retained in package-local scenario functions. Only `TestDesktopAcceptance` dispatches these functions. The central runner selects a catalogue case and verifies Go JSON events. Software-view scenarios remain explicitly distinct from native-window acceptance. The three GUI scenarios cover the legacy `gui.view` retained in the application; they do not establish coverage of the active `desktopdemo` composition.

| Original entry | Canonical case | Evidence |
|---|---|---|
| `TestGUINavigation` | `gui-navigation` | view |
| `TestGUIApprovalAndReconnect` | `gui-approval-reconnect` | view |
| `TestGUIEmptyHistoryLayout` | `gui-empty-layout` | view |
| `TestStartupProcesses` | `startup-lifecycle` | process |
| `TestLiquidGlassE2E` | `liquid-glass` | view |
| `TestInteractionMotionE2E` | `interaction-motion` | view |
| `TestDesktopLiveDataE2E` | `data-binding` | process |
| `TestNativeMenusE2E` | `native-menus` | view |
| `TestQuickPanelE2E` | `quick-panel` | view |
| `TestQuickPanelScreenshotsE2E` | `quick-panel-screenshots` | view |
| `TestUnifiedScrollbarsE2E` | `unified-scrollbars` | view |
| `TestSearchBadgeDeleteE2E` | `search-badge-delete` | view |
| `TestSearchDisclosureE2E` | `search-disclosure` | view |
| `TestSearchDisclosureReducedMotionE2E` | `search-disclosure-reduced-motion` | view |
| `TestHistorySearchProcessE2E` | `search-contract` | process |
| `TestSendDeviceMenuE2E` | `send-device-menu` | view |
| `TestSheetExitStabilityE2E` | `sheet-exit-stability` | view |
| `TestSheetMotionE2E` | `sheet-motion` | view |
| `TestSidebarCollapseE2E` | `sidebar-collapse` | view |
| `TestTagFixedColumnsE2E` | `tag-fixed-columns` | view |
| `TestTagManagerE2E` | `tag-manager` | view |
| `TestTagManagerPlatformsE2E` | `tag-manager-platforms` | view |
| `TestTokenSearchE2E` | `token-search` | view |
| `TestToolbarAlignmentE2E` | `toolbar-alignment` | view |
| `TestQuickPanelProcessE2E` | `quick-panel-contract` | process |

The original files and independent shell/Make entries are removed. Package-local scenario files preserve access to private view state without exporting testing interfaces into product code. UI Kit component tests are outside this migration: they are component checks, not Desktop E2E cases.
