# Sources: Engine v0.20.0-rc.11 desktop 集成

## Official release

- `https://github.com/UniClipboard/Engine/releases/tag/v0.20.0-rc.11`
- Release published at `2026-07-29T14:29:14Z`.
- Release tag commit: `8f9d09789cbe14d3d6bd328edca17fa6a0b14ef9`.
- Previous desktop tag commit: `b742208f230b779cc4bc741e5b190cb7134d18db`.

## Official comparison

- `https://github.com/UniClipboard/Engine/compare/core-v0.20.0-rc.11...v0.20.0-rc.11`
- The new tag is 7 commits ahead and includes peer re-online recovery, interrupted relay configuration recovery, active clipboard lifecycle consolidation, and LAN compatibility routing isolation.

## Current desktop state

- `Cargo.toml` uses `https://github.com/UniClipboard/core.git` with `core-v0.20.0-rc.11`.
- `Cargo.lock` resolves the old tag to `b742208f230b779cc4bc741e5b190cb7134d18db`.
- `scripts/architecture/check-core-repository.mjs` enforces the same old source.
