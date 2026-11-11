# Desktop live data integration

## Approved scope

The existing mygo/UI Kit demo supplies the desktop composition and interactions.
The normal desktop opens history directly. Setup, space creation, passphrase unlock and backup-recovery entry flows are removed.
Service startup failure is an application error, not a setup or unlock state.
The existing service owner, clipboard adapter, tray and login registration remain authoritative.

## Execution order

1. Bind paginated history, selected content, restore, revision-checked deletion and tag assignment.
2. Bind existing device pairing and saved configuration.
3. Address search, tag-library metadata, device policy and quick-panel OS integration as explicit capability gaps.

## Ownership and data flow

The mygo composition owns navigation, selection and transient interaction state.
The desktop binding owns asynchronous localapi requests, content caching and cancellation.
unicd owns Engine, history, trust and configuration.
All UI state changes run through queued completions on the view thread.
Reads may refresh. Lost write responses are uncertain and never retried automatically.
History and device changes trigger fresh snapshots. Periodic reconciliation covers lossy events.
Content readers are bounded and closed. Switching records must never show another record's payload.

## Failure scenarios before implementation

- Silent reconciliation follow-up: initial loads and changed queries must expose loading; timer/event refreshes and pagination must not insert a loading row above existing history. Empty and populated history, selected previews, query cancellation, failed reads and retry must retain correct loading ownership. Observe at least two real timer intervals. Keep event-triggered updates and five-second recovery after missed notifications. Audit SSE replay before removing reconciliation.
- Image-file preview follow-up: a single PNG/JPEG file must render its real pixels without changing the stored file kind or copy semantics. Non-image files and multi-file records must retain their file list. Corrupt images, payloads above 32 MiB, images above 16 million pixels, unavailable files, selection changes and reconciliation must never expose fixture pixels, stale content or unbounded reads.
- No service, service restart, event loss and stale asynchronous responses.
- Empty history, multiple pages, deletion during selection and expired/unavailable content.
- Different content representations, large payloads and damaged image data.
- Restore without adapter, adapter disconnect, failed clipboard writes and duplicate requests.
- Stale revisions, invalid tags, cancellation and partially completed multi-item deletion.
- Pairing denial/expiry, mismatched fingerprints and revisions, closure of invitation view.
- Configuration conflicts, saved-but-not-applied settings and unsupported native login registration.
- Fake fixture data leaking into the normal desktop or removed entry routes remaining reachable.

## Acceptance

Use a private profile and production unicd process. Exercise the real desktop binding without a renderer.
Use the existing directory clipboard adapter to verify restore bytes.
Record source and executable SHA-256 values, scenario outcomes and explicit native limits in a JSON receipt.
Delete private profiles, logs and binaries after acceptance; retain only the small receipt.
Native appearance, focus, menus and OS paste remain separate manual acceptance.

## Progress

Implemented in the working tree:

- Normal desktop reuses the approved composition and starts on history.
- Setup, space creation, unlock and recovery-preview code/routes are removed, including the quick-panel unlock fixture.
- History supports server-side type/device/tag/time filtering, cursor pagination, text/PNG/file-name previews, restore, revision-checked tag assignment and deletion.
- Single-image files now decode through the same bounded image reader as image records. The detail pane uses the shared image frame and Quick Look, shows the filename and real dimensions, and preserves file copy/type semantics. Corrupt, unsupported, oversized and multi-file content retains its file list. The process scenario checks PNG/JPEG dimensions, reconciliation, exact restored file bytes and the fallback cases; native appearance remains unverified.
- Refresh preserves loaded depth. Obsolete filter responses are discarded. Only the selected full payload is cached.
- Background reconciliation is silent in both the history window and quick panel. Only initial reads and changed queries expose foreground loading; the in-flight flag still serializes reads and disables duplicate pagination. The process scenarios cover two idle timer intervals with empty and selected history, event updates, rapid query replacement, pagination, quick-panel reconciliation and a real service restart.
- Event recovery audit: the host hub uses a 32-event lossy queue, SSE emits no replay ID, and the local client has no replay cursor. Keep five-second reconciliation and event-triggered reads. Replacing periodic reconciliation with event-only updates requires a reliable recovery contract first; see the [local service contract](../contracts/local-service-contract.md#local-event-delivery).
- The device sheet calls existing pairing, fingerprint/revision approval, rejection, candidate and removal APIs.
- General, Sync and Storage settings use the existing revision-checked configuration API. Launcher-owned native login status is displayed separately.
- Unavailable operations never report fixture success in the normal desktop.

## Repeatable process command

```sh
make desktop-e2e CASE=data-binding
```

The test builds a race-enabled production service, starts two private service processes and uses the desktop binding against real sockets.
The directory clipboard exercises the existing IPC adapter without touching the system clipboard.
The receipt records source hashes, the service binary hash, platform, toolchain and individual checks.
Temporary profiles and service binaries are deleted on success and failure.

## Remaining capability gaps

- Search, rich-text previews, extension filters and exact candidate counts are now implemented; see the [search execution receipt](desktop-search-implementation.md). Virtual code/link classifiers remain unavailable.
- Persistent pins/favorites and full tag-library entities, colors, merging and bulk management.
- Full device details, device rename/binding routes, fine-grained policy and latency/direct/relay status.
- Sending an existing history ID directly to selected devices.
- Quick-panel production search/Copy/Delete are connected. Global shortcut preferences, target capture and OS paste dispatch are implemented in the [system execution receipt](desktop-quick-panel-system.md). Native acceptance and Wayland automatic paste remain open.
- Appearance, font, arbitrary shortcut editing, proxy, relay and security configuration not supported by the current service contract. Fixed quick-panel shortcut choices are now supported.
- Native visual, focus, menu and clipboard acceptance. No application, renderer or browser was launched.

## Verification notes

- The live-data process E2E and the production desktop build passed.
- Pinned golangci-lint v2.15.0 reports zero new issues against HEAD for the affected desktop/localapi packages.
- The installed default lint executable is v1 and cannot read the repository's v2 configuration. Use the pinned version with the installed project toolchain.
- Additional shared/service checks exposed 25 host failures with `unavailable: cannot open default file cache`. All 25 reproduce in an independent archive of baseline `d305a77e2d243e6db79b77f56dc259f8d5fa646e`. The other selected packages passed.
- Implementation commit: `66a3d9b5b036007cfee548cc37838908d7156f95`. The acceptance source hashes match this committed source.
- After the code commit, architecture verification markers were updated for overview, headless integration, desktop runtime and identity/trust. docscheck passes without warnings.
- Existing view-renderer tests were compiled but not executed because native/offscreen visual work requires explicit authorization.
