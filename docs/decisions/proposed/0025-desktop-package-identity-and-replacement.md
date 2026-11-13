# 0025: Desktop package identity and replacement

- Status: Accepted; runtime/construction implemented and native v2 lifecycle partially accepted. Production activation and real v1 replacement acceptance remain pending.
- Date: 2026-11-13.
- Execution plan: [v2 package channel parity](../../exec/0018-v2-package-channel-plan.md).
- Requirements: [release contract](../../contracts/release-contract.md#desktop-installation-identity-and-update-ownership).

## Context

v2 must retain the existing distribution channels while replacing the v1 Desktop program.
Separate official v1/v2 installations would expose duplicate launch entries and allow competing clipboard listeners.
Sharing program identity does not make the v1 and v2 data formats or device identities compatible.
Desktop and standalone CLI packages also need separate file ownership.

## Decision

Keep the product name `UniClipboard` and existing channel package identifiers, including the `uniclipboard` base name and existing channel-specific names.
Retain the Desktop application identifier `app.uniclipboard.desktop`.
Within the same distribution channel, v2 replaces the installed v1 program rather than creating a parallel official installation.
Alpha and stable share the same installation identity. Switching channels replaces the program while preserving v2 data and does not imply an automatic downgrade.
Development builds use a separate installation identity and data directory.

Use `unic-desktop`, `unic` and `unicd` as executable names. Keep `uniclip` as the CLI alias.
Desktop and CLI retain independent product versions and increasing build numbers.
The first v2 Desktop release uses product version `2.0.0` and build number `1`, as explicitly selected by the user. This selection is not a published release.
Desktop locks one exact accepted CLI build and manifest.
Native package versions must order increasing product versions and builds correctly; exact per-format encoding is defined in the release contract and exercised by construction/process acceptance.

Place the bundled CLI and service in the Desktop application's private directory.
Desktop installation must not overwrite an independently installed CLI or its public commands.
Package-manager installations update through their owning package manager.
Accepted direct-download installations use the application update path for their supported format.

Keep v2 data separate from v1 data. Import v1 history only through explicit migration.
Replacing the program preserves v1 data for migration and never silently reuses v1 settings, identities or trust.
Uninstall removes files owned by that installation and preserves user data by default.

## Consequences

Existing channel upgrade paths remain usable without a new v2 package name.
Official simultaneous v1/v2 Desktop operation is outside the supported installation model.
Cross-package-manager replacement and transfer of file ownership require explicit handling; this decision does not authorize one installer to remove another manager's files.
Replacement, channel switching, independent CLI coexistence and data preservation require real package lifecycle acceptance before release.
This decision defines intended behavior. It does not establish that current installers implement it.
