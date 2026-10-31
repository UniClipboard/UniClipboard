# Remaining browser scenarios

## Failure model before changes

- Palette tokens are inline styles; ordinary policy CSS cannot override them.
- A component fixture without a presentation-effects provider selects the conservative startup policy.
- A full-page fixture can install host bindings but omit visual-effects initialization.
- Old selectors can point to removed controls; assertions must not be weakened or obsolete UI restored solely for tests.
- Failed image setup can leave later keyboard scenarios without their original focus precondition.
- Virtualized rows cannot be equated with all loaded data without checking the viewport and load requests.
- Dialog exit animations can retain an intercepting backdrop after the initiating action.
- Search and sidebar layout expectations can conflict with current product contracts; retain exact failing assertions when no equivalent interpretation exists.

## Scope

Preserve Engine thumbnail-503 evidence. Do not edit Engine or substitute synthetic successful thumbnails. Retain native acceptance launchers. Diagnose and fix Desktop-local product/fixture defects, then rerun exact-source browser E2E with artifacts.

## Confirmed before implementation

At e20a9a009, ui-kit/macos fails its simulated-native-material assertion. visual-effects/macos fails the unchanged shadow-token assertion. UiRoot supplies palette tokens inline; globals.css supplies non-important low-effects overrides. The gallery has no GuiVisualEffects provider, whose conservative default is lowEffects=true.

## Verified local changes

All original visual-effects assertions pass on three skins after policy precedence is corrected. All gallery and entry assertions pass on three skins after fixture policy is explicit. Compact light/dark scenarios pass after platform selection input and resize synchronization. The macOS prototype passes after canonical palette tokens are applied. History keyboard and tag-manager scenarios now pass without changing their assertions; dark mode reads the rendered CSS color scheme.

## Explicit contract conflicts retained

- StartupSettings exposes normal/lightweight; silent is a legacy stored value, not a selectable mode. The old settings-design startup assertion remains unchanged.
- HistoryListRow explicitly has no hover action bar. The original hover-action checks remain unchanged.
- Original warm-image fixture drops the last original-blob lease on selection changes; current cache revokes the URL. Do not remove release ownership solely to satisfy an old warm-cache precondition.
- HISTORY_WINDOW_CAPACITY is 50, while grid-paging requires more than 100 simultaneously rendered records. Keep the old assertion, preserve the bounded model, and report the conflict.
- Search metadata and modal-navigation focus contracts remain as documented. Do not restore obsolete UI merely for test success.

React Doctor 0.9.18 reports score 98 and no new diagnostics against e20a9a009 (newCount=0, matchedCount=1). The matched lazy-state-init diagnostic exists in the baseline prototype code. The isolated same-version baseline and current JSON are retained as evidence; no suppression or unrelated cleanup was added.
