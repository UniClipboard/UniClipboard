# V1 tag controls in V2

## Reference and scope

Reference source: V1 `c6483d73c7a0234be68571fdb5fa3d38a17c4871`. The reference images were captured from the original React HistoryKitDetailTags and HistoryKitTagManager components with synthetic data in a browser. They establish component design, not installed-app native acceptance.

V2 reuses ComboField, Tag, Sheet, Table and the native context-menu adapter. It adds no dependency. Existing TagTint tokens own chip fill and ink. The [architecture](../architecture/overview.md#tag-controls-and-colors) records layout, keyboard behavior, protected built-ins, storage and service limits.

## Failure inventory

Specify the behavior before implementation: missing inline field; checkbox Sheet replacing inline assignment; missing palette; highlighted creation surface excluding swatches; swatch stealing focus or creating a tag; duplicate or invalid names; attached/built-in tags offered as custom creation; lost associations; incorrect keyboard selection/cycling/cancel; editor changing another entry; divergent colors; lost colors after refresh/reopen; invalid or stale configuration overwriting valid settings; management/menu/layout regressions.

## Repeatable acceptance

```sh
make desktop-e2e CASE=tag-editor
make desktop-e2e CASE=tag-manager
make desktop-e2e CASE=tag-manager-platforms
make desktop-e2e CASE=data-binding
```

The canonical runner freezes source and retains an HTML report, manifest, SHA256SUMS, assertion receipts and software-rendered PNG checkpoints. All content is synthetic. Each manifest binds the exact working source, including uncommitted changes. Private test profiles, source snapshots and binaries are cleaned up.

The initial tag-editor run `20261113T024911Z-89cd0e32` failed in all six skin/theme combinations because the inline ComboField was absent. The implementation run `20261113T025834Z-3754122b` passed all six combinations, including create/attach/remove, five swatches, Tab and Shift-Tab, Escape, click-away, selection changes and management color propagation. The management run `20261113T025857Z-8de3410a` passed its 30 theme/scenario combinations. The platform management run `20261113T025859Z-636b6826` passed all six combinations.

The real-service data-binding run `20261113T025758Z-f04a3255` passed. It exercised production unicd, local APIs, configuration CAS and SQLite history. Added checks cover color save/refresh/reopen, create-and-attach through both real APIs, rejection of invalid color keys/values, no raw tag names in color configuration and preservation of concurrent color changes.

The shared token-search regression run `20261113T030214Z-5cbdbc15` and fixed-column run `20261113T030217Z-3f222843` passed. The repository-pinned golangci-lint v2.15.0 reports zero issues and no formatting diff for the changed scope. The production desktop build succeeds; the executable is not launched. Ordinary docscheck passes; unrelated stale-page warnings are recorded separately.

## Merge menu icon follow-up

The Merge Into submenu now uses shared Kit IconMerge. The glyph is the Lucide merge icon, distributed with its license in `app/desktop/internal/uikit/licenses/lucide.txt`; no package dependency was added. It follows the existing native-menu template-image path and each skin's stroke styling.

The follow-up tag-manager run `20261113T032939Z-fcf43412` passed all 30 theme/scenario combinations and retained 31 report assertions. Pinned static analysis passed. A subsequent isolated macOS preview screenshot showed the real NSMenu with the merge icon and was shared through Lody. This is manual component evidence, not a new automated native acceptance case.

## Boundaries

Software-rendered checkpoints do not establish native font rasterization, native-window placement or OS input delivery. The native context-menu adapter remains unchanged. The six automated cases did not establish native app/window acceptance. The later native preview screenshot establishes only the visible menu component. System clipboard and cross-application paste acceptance remain NOT_RUN. Pixel-perfect identity between Chromium and mygo is not claimed. Global tag rename/merge/delete and standalone empty-tag creation remain existing service gaps; they are not presented as persisted operations.
