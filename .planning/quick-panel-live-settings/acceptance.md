# Live settings acceptance

Task: quick-panel-live-settings-fixes-20261101. Workflow: ea0df3cd-fe66-4ddb-ae09-669ec82677a3.
Base: 145ae0c87c58e62465a876ff0d3249032dff3f2a. Preserve the existing dirty accessibility implementation.

## User flows and failures (written before implementation)

1. Disable and enable the panel. Expect no restart banner or restart call. Failed saves must restore the previous switch value. Test both failure directions and both themes on all three skins.
2. Return from macOS settings after granting or revoking Accessibility. Query the system without prompting or persisting permission. Request a helper settings refresh independently of the query result: only the helper can determine its own permission. Concurrent focus/visibility events must coalesce, with one trailing check to observe a change during an in-flight query. Failed queries must remain errors. Refresh must read authoritative settings under the host shortcut lock, skip disabled/lightweight/fallback panels, and not launch a missing helper. Reapplying a modifier must retry failed initialization while leaving an existing identical listener intact. Revocation must suppress keyboard observation in the helper itself.
3. Save center/follow-cursor and open the panel once. Load the current options before positioning and showing. Hide or toggle during the read must prevent late display. A prior session response must not change the current session. Background settings refresh must not steal focus or reposition a visible panel. An options failure must leave the panel reachable with the last known position.

## Evidence

Browser E2E: real settings components, scripted host results, assertions JSON, screenshots, run ID, build/source identities, cleanup receipt and SHA256. This proves UI behavior only.
Native: on a dedicated macOS test host, build the acceptance bundle with a private HOME/profile and clipboard disabled; record each executable signing identity and actual TCC behavior for the UI/resident/helper. Grant/revoke the helper's responsible process, return to settings, then verify double-tap opens the native panel without restarting. Repeat checks and toggles. Change position in both directions and verify the first opening, rapid hide/reopen, focus changes and delayed options. Never send real keys or modify permissions on the personal host. If no dedicated host is available, mark native acceptance NOT RUN.

## Approach

Remove only obsolete quick-panel restart UI and six locale keys. Add one host refresh command with generated contracts and owner forwarding; reuse helper reload_settings. Keep settings and listener ownership unchanged. Move first positioning/show to options completion in the existing panel reducer; reject obsolete adapter responses. Add no unit tests or production test fields.
