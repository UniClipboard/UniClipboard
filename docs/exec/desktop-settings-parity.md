# Desktop settings parity: parallel work agreement

- Date: 2026-11-22.
- Status: Step 1 complete. Three workers have implemented A, the existing-API B slice, D color mode, E permission guidance and G portable backup in the working tree. C landed in PR #2288. F has a capability review; contract-dependent work and other remaining parity are listed below. Native acceptance remains separate.
- v1 reference: `origin/v1` at `c6483d73c7a0234be68571fdb5fa3d38a17c4871`.
- Initial v2 baseline: `542a1d01d41f397a5f4462b70826dcdfd6bb12e3`.
- Reconciled v2 baseline: `fcf5611662039c1db79fc6537c4f79aff2a5b436`, the merge commit for [PR #2288](https://github.com/UniClipboard/UniClipboard/pull/2288), merged on 2026-11-22.
- Scope: Prepare independent work packages A–G, then execute the user-authorized parallel settings work. This does not authorize native execution, publication or deployment.

## Authority and existing behavior

This is an execution agreement, not a public API contract. Existing fields and behavior remain governed by the [Desktop architecture](../architecture/desktop-runtime.md), [repository boundaries](../architecture/overview.md), [local service contract](../contracts/local-service-contract.md), [quick-panel contract](../contracts/desktop-quick-panel-contract.md) and [observability contract](../contracts/observability-contract.md).

Production settings dispatch through `liveSettingsContent` in `app/desktop/internal/desktopui/live_settings.go`. Controls in the demo path of `settings.go` do not establish production support. The working tree adds the production Appearance page; standalone shortcuts, security and network still reach the unavailable-state fallback. Permission guidance lives under Quick Panel.

Use `internal/localconfig.Config` and its existing revision-checked read/write path for TOML-owned preferences. Existing mygo updater preferences are an exception: preserve their existing store and tray-owned API rather than duplicating them in TOML. Do not add another settings store, persistence framework or generic renderer. Desktop presentation and native integrations stay in `app/desktop`; Engine and root internal packages must not depend on Desktop packages.

## Owners and files

Owners below are task roles, not running agents or assigned people. One implementer owns each role when that work starts. I is the integration role and the sole editor of shared files. A–G may prepare patches for shared files, but I applies those patches serially.

| Role | Owned implementation area | Proposed production view file | Primary acceptance case |
|---|---|---|---|
| A: Diagnostics | Existing diagnostic client operations, local export, native directory opening | `app/desktop/internal/desktopui/settings_diagnostics.go` | `settings-diagnostics` |
| B: Storage | Storage inventory, safe cleanup, history deletion and search maintenance | `app/desktop/internal/desktopui/settings_storage.go` | `settings-storage` |
| C: About and updates | Preserve landed product identity, updater policy, channel selection, installation-owner guidance, sponsors and product links; follow up on release announcements and remaining acceptance | Existing `app/desktop/internal/desktopui/about.go` and `about_sponsors.go` | `settings-updates` (proposed follow-up, not an existing canonical case) |
| D: Appearance and sound | Desktop presentation preferences and native sound application | `app/desktop/internal/desktopui/settings_appearance.go` | `settings-appearance` |
| E: Shortcuts and quick panel | Shortcut editing, registration, target-independent panel preferences and permission guidance | `app/desktop/internal/desktopui/settings_quick_panel.go` | `settings-shortcuts` |
| F: Network | Tailcat capability mapping, instance-owned transport settings and native status projection | `app/desktop/internal/desktopui/settings_network.go` | `settings-network` |
| G: Configuration backup | Config export, import preview, validation and recovery | `app/desktop/internal/desktopui/settings_backup.go` | `settings-backup` |

The execution status table below distinguishes implemented files/cases from remaining reservations. E's permission subset uses `settings_shortcuts.go`; it does not replace the existing quick-panel shortcut selector. Do not create `settings_about.go` as a parallel implementation of the landed About view. Keep rendering functions in the existing `desktopui` package. Do not introduce packages solely to split work. Preserve existing storage/quick-panel rendering while adding domain-owned sections.

I owns these shared files and surfaces:

- `internal/localconfig/config.go`, its manager and shared configuration conversion/validation code.
- `app/desktop/internal/desktopui/settings.go`, `live_settings.go`, `live.go` and shared live state/save callbacks.
- `internal/localapi/api.go`, shared DTOs, service route registration and shared host assembly.
- `app/desktop/cmd/unic-desktop/roles*.go`, `gui.go` and other shared process assembly.
- Locale dictionaries, settings category routing, application dependency files and native BoardUI public controls.
- E2E registry, generated catalogue, shared runner, architecture pages and public contracts.

Owned domain files under `app/desktop/internal/appupdates`, `quickpanel`, service host/server and root observability/store remain with C, E, A/B as appropriate. Each group must list its exact additional files before editing. F owns Tailcat changes only after its capability review. Two groups must not edit the same domain file concurrently.

## Field reservations

**Existing** means the baseline already stores the field. **Reserved** means the proposed spelling, type, scope and owning role are allocated here. Reserved fields are not implemented or public-contract changes. The defaults below define the implementation proposal; contract-dependent details remain gated. Each owner must verify runtime support before asking I to integrate a field. Omit any reserved field whose behavior does not need persistence.

| Role | Existing fields to reuse | Reserved additions | Application boundary |
|---|---|---|---|
| A | `logging.level`; `telemetry.enabled`; `telemetry.logs.enabled`, `.level`; `telemetry.metrics.enabled`; `telemetry.traces.enabled`; `telemetry.analytics.enabled` | None | Use the existing observability validation and live-application rules. Temporary log overrides remain runtime operations, not new config fields. |
| B | `history.auto_cleanup`, `.retention_days`, `.max_items`, `.excluded_tags`, `.max_bytes`; `file_cache.max_bytes`; `inbox` | `desktop.confirm_before_delete: bool`, only if a user-selectable confirmation policy is retained | Keep current live/restart rules. Statistics, clearing and search repair are operations, not saved booleans. |
| C | `desktop.update_channel`; mygo updater `AutomaticChecks` and `AutomaticDownloads` via tray role IPC | None. Withdraw the earlier `desktop.update_auto_check` and `desktop.update_auto_download` reservations. | Channel uses revision-checked TOML. Automatic preferences use existing updater setters/store. Preserve saved download preference while automatic checks are off. Package-manager ownership remains authoritative. |
| D | `desktop.system_window_frame`; `desktop.language` | `desktop.color_mode: string` (`system`, `light`, `dark`); `desktop.ui_scale: float`; `desktop.palette: string`; `desktop.custom_colors: map[string]string`; `desktop.follow_omarchy_theme: bool`; `desktop.sound_enabled: bool` | Desktop only. Theme/scale/sound changes need an actual runtime application path. Preserve the existing close/reopen requirement for window frames. Determine stable color keys from the existing BoardUI tokens before schema integration. |
| E | `desktop.quick_panel_shortcut` | `desktop.shortcut_overrides: map[string]string`; `desktop.quick_panel_enabled: bool`; `desktop.quick_panel_position: string` (`center`, `follow_cursor`); `desktop.quick_panel_double_tap_modifier: string` | Desktop tray owns global registration. Preserve effective/requested status, conflict recovery and lightweight-mode behavior. General shortcut syntax and modifier values require capability/contract review first. |
| F | Existing Tailcat instance identity and `plugins.instances.<id>.config` container; existing backend content policies | No transport key is frozen yet | Transport settings belong to the backend instance, not global `desktop.*` or `sync.*`. Determine supported fields through the maintained Tailcat API first. |
| G | The complete typed configuration and existing revision semantics | None | Export/import is an operation. Do not persist a backup of config inside config. |

### Proposed defaults and validation

| Reserved field | Default and validation | Effective time |
|---|---|---|
| `desktop.confirm_before_delete` | `true`; boolean | Next user deletion. Keep explicit bulk-delete confirmation regardless of this preference. |
| `desktop.color_mode` | `system`; `system`, `light`, `dark` only | Existing windows and subsequent windows after save. |
| `desktop.ui_scale` | `1.0`; finite numeric value from `0.75` through `2.0` | Existing windows and subsequent windows after save. Preserve OS DPI scaling as a separate input. |
| `desktop.palette` | `default`; built-in palette identifiers only | After save. Start with the existing BoardUI palette; add only supported palettes. |
| `desktop.custom_colors` | Empty map; documented semantic color keys and canonical `#RRGGBB` values only | After save. Keys require token mapping before integration. |
| `desktop.follow_omarchy_theme` | `false`; boolean | After save where a supported theme source exists. Otherwise expose unavailable status. |
| `desktop.sound_enabled` | `false`; boolean | Next supported interaction. Do not play a sound merely to apply the preference. |
| `desktop.shortcut_overrides` | Empty map; registered action IDs only; syntax and conflict rules await E's contract decision | After successful registration/application. Preserve the previous effective binding on failure. |
| `desktop.quick_panel_enabled` | `true`; boolean | After save. Disable all panel invocation paths without changing the saved shortcut. |
| `desktop.quick_panel_position` | `center`; `center`, `follow_cursor` only | Next panel opening. Bound geometry to the available display. |
| `desktop.quick_panel_double_tap_modifier` | `disabled`; accepted platform modifier values await E's capability review | After successful native registration. Unsupported values must not be accepted silently. |

These proposed defaults do not alter current behavior in this documentation step. Theme integration, palette expansion, shortcut syntax and network fields must pass their stated capability/contract prerequisites. A failed native application must remain visible separately from the stored preference.

C's updater defaults and persistence follow the existing mygo updater implementation. Do not apply the withdrawn proposal's rejection rule to `AutomaticDownloads=true` with `AutomaticChecks=false`: the landed view disables and visually clears the download switch while preserving the saved download preference. Reenabling checks restores that preference.

Do not rename existing keys or create aliases for v1 keys. Preserve unrelated fields on every save. Do not submit a stale whole-config snapshot after an asynchronous operation. Use the existing revision-conflict result and re-read before retrying; never overwrite an external edit silently.

The current `desktop.quick_panel_shortcut` fixed-choice contract remains authoritative. Free-form recording and a shortcut override map extend that contract. E must present the reproduction or parity requirement, affected clause and proposed semantics, then obtain the user's decision before changing the contract or implementing that extension.

F must publish a capability table covering LAN-only routing, overlay addresses, built-in relays, custom relays, credentials, probes and congestion control. For each item, identify the public API, validation, restart requirement and secret handling. An unsupported item stays explicitly unsupported; it must not be simulated by a saved value that has no runtime effect. Any public-contract change follows the same decision rule as E.

## Operations and responsibility boundaries

| Role | Reuse first | New operation requirements |
|---|---|---|
| A | `internal/localapi/diagnostics.go`, `internal/observability/export.go`, CLI log export and current log-level APIs | Export to a user-selected destination. Keep diagnostics sanitized. Native path opening belongs to Desktop, not Engine. |
| B | Existing Engine history/search APIs, managed-resource ledger and service host | Inventory and cleanup run in the service. Preserve active leases and user-managed files. Search rebuild cannot become an unbounded GUI-thread task. Propose missing API DTOs before adding routes. |
| C | Existing `about.go`, `about_sponsors.go`, `app/desktop/internal/appupdates`, tray `status`, `check-updates` and `set-update-prefs` IPC | Extend the landed path only. Project real check/download/install states. Never label a queued check or download as an installed update. Announcements remain separate from sponsors and update installation. |
| D | Existing BoardUI tokens, mygo theme/preferences and current native dependencies | Apply preferences to main window, setup and quick panel consistently. Do not restore retired skins or override OS accessibility policy. |
| E | `app/desktop/internal/quickpanel`, current global shortcut owner and native capability probes | Permission checks report OS state. Do not trigger paste during permission checks or shortcut editing. Unsupported Wayland target injection remains unavailable. |
| F | Existing Tailcat backend, plugin Schema and instance configuration route | Keep credential values out of config exports, logs and ordinary status. Preserve backend isolation and approval requirements. |
| G | Existing typed decoder, validator, revision checks and atomic config file writer | Parse and validate before commit. Preview changes and exclusions. Reject stale revisions. Separate saved state, runtime apply failure and restart requirements. |

No new HTTP paths or exported Engine signatures are declared by this plan. Use existing operations where possible. If an operation is missing, its owner supplies a small DTO/route proposal to I. Read applicable contracts before implementation; obtain the user's decision for public-contract changes.

Configuration backup exports portable preferences only. Exclude credentials, device private keys, pairing material, plugin private directories, raw telemetry tokens and machine-specific paths. G must provide an explicit field allowlist, including rules for plugin Schema secret fields, before implementing export. Do not infer secrets from field names. Import cannot bypass first-install/migration gates or reset device identity.

The initial G scope covers typed TOML configuration, including `desktop.update_channel`. It does not implicitly include mygo's separate updater preference store. State that exclusion in the backup preview; do not claim to back up all Desktop preferences. A later extension must use the updater's supported APIs rather than copying its private files or inventing TOML aliases.

## Dependencies and integration order

1. I records the existing baseline and accepts each group's field/default/application proposal. Integrate only the minimal schema required by that group's first complete path. Do not add all reserved fields in advance.
2. A, B and D can start independently after their owned files are recorded. C starts from PR #2288's existing implementation and addresses only remaining work. E can inspect capabilities and write acceptance scenarios in parallel; contract-dependent implementation waits for the user's decision. F starts with its Tailcat capability table.
3. G can prepare export/import scenarios and a portable-field allowlist against the existing schema. Its full compatibility matrix waits for A–F field proposals to stabilize.
4. Each group writes its E2E scenarios before runtime implementation. I registers the cases and applies shared-file patches in a serial queue.
5. Each group delivers one complete vertical path: read, edit or invoke, persistence when applicable, runtime effect, failure handling, and evidence. Keep working paths intact while adding the next one.
6. I integrates group patches and runs the affected cross-group E2E cases. G finishes with the final schema and imports an export in a fresh isolated profile.

The schema queue is ordered by ready, independently verifiable work. It is not a requirement to wait for every group before implementing one group. No group may merge generated catalogue or locale changes independently while another group owns those shared surfaces.

## Acceptance and artifacts

Use the [canonical Desktop E2E entry](../../app/desktop/e2e/README.md) and [evidence contract](../contracts/desktop-e2e-contract.md). Do not add unit tests after implementation. Write failure scenarios before any isolated harness. Prefer real production service/CLI processes and Desktop view actions through the canonical runner.

Common scenarios: saved value survives restart; effective value matches the UI; an external edit causes a visible revision conflict; invalid input preserves the last valid state; runtime apply failure is visible; a request completed after cancellation cannot overwrite newer UI state; unavailable platform capabilities remain visible; unrelated configuration remains intact.

Each group retains a small checksummed receipt containing source identity, executable identities where used, replay command, scenario verdicts, cleanup result and native acceptance limits. Keep the first failed artifact and set a run/time limit before retrying. Do not commit profiles, credentials, binaries or large recordings. Native clipboard, permission, focus and installer acceptance are separate from process/view fixtures. This step starts no browser, app or VM.

## Deferred decisions

- Security page, automatic unlock and passphrase changes: establish the v2 security model before restoring v1 controls.
- Restore last entry on startup: define whether this restores selection, writes the clipboard or synchronizes content. Do not add a field until the behavior is decided.
- Upgrade snapshots: define the snapshot contents, creation point, ownership, retention and restore procedure. G covers configuration backups only until this decision exists.
- Separate file retention remains superseded by unified history cleanup. Do not recreate it for visual parity.
- Sync frequency was a v1 coming-soon control. It is not part of parity implementation.

## PR #2288 reconciliation

The merged source implements the About header with product version and channel badge, manual update checks, automatic-check and background-download controls, stable/alpha selection with alpha confirmation, installation-owner guidance, sponsor loading and product/legal links. The production and demo About paths reuse that composition. Do not list these items as missing or recreate them in another settings page.

The live code reads updater status from the tray and applies automatic preferences through `set-update-prefs`. Channel changes still use the revision-checked service configuration; an accepted switch to alpha saves the channel before requesting a check. Shared role assembly, live state, routing and locale files changed in this PR. Subsequent workers must start from the reconciled baseline and preserve these changes.

Remaining C work:

- Release announcements: the v1 Notifications section is distinct from sponsors. PR #2288 does not add a release-announcement list or category.
- Canonical settings E2E evidence for updater preference persistence across tray restart, download-preference preservation, alpha-confirm cancellation, failed saves/checks and external update ownership. Inspect existing acceptance coverage before adding cases; do not label the proposed `settings-updates` case as already registered.
- Native updater window, actual OS update interactions, external browser launch and cross-platform acceptance. The PR reports software-view checks; those are not native acceptance.
- Real signed delivery and installation remain subject to the release architecture and are not established by this settings PR.

This reconciliation used the merged source and GitHub PR metadata. It did not rerun the PR's tests or launch a GUI, updater, browser or VM. The existing native-update architecture paragraph already describes the landed ownership and IPC; it needs no duplicate behavior description from this planning change.

## Step 1 completion record

- [x] Verify v1 and v2 source identities and existing production dispatch.
- [x] Separate existing fields from reservations and contract-dependent fields.
- [x] Assign A–G ownership and a single shared-file integration role.
- [x] Define operation boundaries, dependency order and acceptance requirements.
- [x] Start three implementation workers after the user authorizes parallel execution.
- [ ] Record E's public-contract decision and F's maintained-API capability table before their dependent implementation.
- [x] Record proposed defaults and application timing; identify values that require capability or contract decisions.

Documentation validation is recorded separately from runtime acceptance. This plan does not claim that any missing setting works.

## Parallel execution status

The initial fields/defaults tables above remain a planning record. Only
`desktop.color_mode` has been added to the configuration in this execution;
other unimplemented reservations are not accepted configuration keys. C's two
withdrawn update keys must not be added. Native acceptance remains NOT_RUN for
the new controls and actions.

| Group | Working-tree delivery | Remaining work |
|---|---|---|
| A | [Diagnostics](desktop-settings-diagnostics.md): existing persistent levels/signals, effective status, temporary overrides and native action bindings; canonical process case | Native dialogs, exact installed private-CLI callback execution, actual folder opening and visual/platform acceptance |
| B | [Storage](desktop-settings-storage.md): aggregate managed-cache usage, refresh, host directory action and canonical process case | Full disk inventory, safe manual maintenance, clear-all history and search maintenance need the recorded operation/contract decisions |
| C | PR #2288 About/update controls | Announcements, canonical updater persistence and native/release acceptance listed above |
| D | [Appearance](desktop-settings-appearance.md): persisted color mode, all-role native theme wiring and canonical process case | UI scale, palette/custom colors, sound, Omarchy following and native scheme acceptance |
| E | [Permission guidance](desktop-settings-shortcuts.md): passive status, explicit settings action, cancellation/generation checks and canonical process case | Recording/reset customization, enablement/position, modifier double tap and native acceptance; free-form global chords and core-command remapping require the two [contract decisions](desktop-settings-shortcut-capabilities.md) |
| F | [Maintained API capability table](desktop-settings-network-capabilities.md) | Required upstream controls and transport address/reapproval/active-probe decisions; no ineffective network fields were added |
| G | [Portable backup](desktop-settings-backup.md): strict 26-field allowlist, preview/confirmation, original-revision save and canonical process case | Native file-panel/file-action integration acceptance; upgrade snapshots remain a separate deferred decision |

The five new canonical process cases are `settings-diagnostics`,
`settings-storage`, `settings-appearance`, `settings-shortcuts` and
`settings-backup`. Their receipts identify frozen source and actual service
executables. Earlier integration/fixture failures remain failed and retained;
they do not count as final functional failures once their documented causes are
corrected. Group-specific receipts state the exact verdicts and replay commands.
