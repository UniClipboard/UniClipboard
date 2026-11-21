# Desktop BoardUI compositions

This internal package owns UniClipboard-specific layouts and presentation state
adapters. It depends on the public `app/desktop/boardui` package and maintained
mygo behavior primitives. Engine, CLI and service code must not depend on it.

The former platform UI Kit, raw CSS token snapshot, token parser/generator,
platform glyph overrides and skin selection are removed. There is one
source-bound semantic theme. Named tag colors map to BoardUI status semantics;
persisted custom tag hex colors remain caller data. OS window frames, keyboard
modifiers and system tray/Dock menus stay with the host.

The public controls own paint. This package preserves caller-owned values,
change/submission results, virtualized selection, paging, search-token editing,
async cancellation and safe confirmation defaults. These application
compositions are not claims that all official BoardUI controls or templates
were ported. Mature mygo editors, menu builders and focus scopes remain the
behavior implementation.

`make desktop-e2e CASE=boardui-composition` exercises the retained complete-view
component scenarios and emits PNG evidence. The production migration replay
also drives the full history/settings/Quick Panel compositions. See the
[native presentation contract](../../../../docs/contracts/boardui-native-contract.md).
