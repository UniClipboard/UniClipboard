# Release process acceptance

Failure inventory recorded before implementation:

- Version, build number, source or required target identities disagree.
- A clean Windows-style Git checkout changes release metadata bytes and causes cross-platform assembly to reject the candidate.
- Package assembly selects a different executable or loses the uniclip alias.
- npm installs the CLI but leaves its required sibling daemon unavailable as a command.
- An interactive v2 command omits the warning when the v1 directory exists, or warns on a fresh installation or a non-directory path.
- A compatibility warning contaminates JSON output or noninteractive command output.
- Migration help accesses credentials, creates a target or starts a daemon.
- A published CLI package omits the in-process migration importer.
- Migration loses its partial-result exit code, modifies v1 data, or duplicates imported records on retry.
- An archive escapes its extraction directory or omits the sibling daemon.
- An already published package version has conflicting bytes.
- A required platform package is missing but the main package is exposed.
- Promotion repacks/reuploads or selects mutable latest artifacts.
- Stable advances to an older build or changes alpha's pointer.
- An existing v1 alpha tag blocks a strictly newer v2 release, or an unknown/same-base legacy tag is silently replaced.
- An external partial failure is incorrectly reported as complete success.
- Dirty source reaches real publication or loopback fixture acceptance authorizes a public release.
- A checksum or source changes after candidate acceptance.
- Workflow/registry authorization is assumed from package existence.

Acceptance uses the real Go packager, production CLI/service, npm pack/install and a disposable HTTP npm registry fixture. It emits a sanitized replayable source/command/assertion manifest.
No unit tests are added. GUI, production npm/OIDC, signing, Store and GitCode operations remain separate.
