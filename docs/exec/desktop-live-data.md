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
- Refresh preserves loaded depth. Obsolete filter responses are discarded. Only the selected full payload is cached.
- The device sheet calls existing pairing, fingerprint/revision approval, rejection, candidate and removal APIs.
- General, Sync and Storage settings use the existing revision-checked configuration API. Launcher-owned native login status is displayed separately.
- Unavailable operations never report fixture success in the normal desktop.

## Repeatable process command

```sh
UNIC_DESKTOP_DATA_E2E=1 \
UNIC_DESKTOP_DATA_RECEIPT="$PWD/.build/desktop-live-data/receipt.json" \
go test -race ./app/desktop/internal/desktopdemo -run '^TestDesktopLiveDataE2E$' -count=1 -v
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
- Quick-panel production search/Copy/Delete are now connected. Global shortcut preferences, target capture and OS paste remain unavailable.
- Appearance, font, shortcut, proxy, relay and security configuration not supported by the current service contract.
- Native visual, focus, menu and clipboard acceptance. No application, renderer or browser was launched.

## Verification notes

- The live-data process E2E and the production desktop build passed.
- Pinned golangci-lint v2.15.0 reports zero new issues against HEAD for the affected desktop/localapi packages.
- The installed default lint executable is v1 and cannot read the repository's v2 configuration. Use the pinned version with the installed project toolchain.
- Additional shared/service checks exposed 25 host failures with `unavailable: cannot open default file cache`. All 25 reproduce in an independent archive of baseline `d305a77e2d243e6db79b77f56dc259f8d5fa646e`. The other selected packages passed.
- Implementation commit: `66a3d9b5b036007cfee548cc37838908d7156f95`. The acceptance source hashes match this committed source.
- After the code commit, architecture verification markers were updated for overview, headless integration, desktop runtime and identity/trust. docscheck passes without warnings.
- Existing view-renderer tests were compiled but not executed because native/offscreen visual work requires explicit authorization.
