# Findings

- #1365: Linux WebView numeric shortcuts call handleSelect, which restores and pastes then dismisses. Tab/Shift+Tab cycle QUICK_FILTER_ORDER; Ctrl+N/P move selection. Native panel has separate key bindings.
- #1378: Cargo enables release-health, but sentry-core 0.48.5 defaults auto_session_tracking to false. Backend uses Default and has no manual start_session call. Do not claim session uploads solely from a Cargo feature. Document current disabled state and existing transport gate.
- #2112: deploy/vps/docker-compose.yml already uses uniclip health. apps/cli-go/e2e/healthcheck.py already covers real processes, profiles, stale records and restarts. Source presence is not fresh runtime proof.
- #583: original source path is retired. Search DTO now has payloadState; panel uses the unified live-search projection. Need current full-path acceptance before closure.
- #1687: current Go CLI has a service command with start/restart/status/stop; original install/uninstall and Windows scope needs reconciliation.
- #2110: VISION explicitly retains tray host and daemon in lightweight mode. Reported process presence alone does not establish a defect.
- Local gh credentials are invalid. The connected GitHub account successfully reads this repository.
