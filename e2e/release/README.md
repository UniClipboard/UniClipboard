# Release process acceptance

Failure inventory recorded before implementation:

- Version, build number, source or required target identities disagree.
- Package assembly selects a different executable or loses the uniclip alias.
- An archive escapes its extraction directory or omits the sibling daemon.
- An already published package version has conflicting bytes.
- A required platform package is missing but the main package is exposed.
- Promotion repacks/reuploads or selects mutable latest artifacts.
- Stable advances to an older build or changes alpha's pointer.
- An external partial failure is incorrectly reported as complete success.
- Dirty source reaches real publication or loopback fixture acceptance authorizes a public release.
- A checksum or source changes after candidate acceptance.
- Workflow/registry authorization is assumed from package existence.

Acceptance uses the real Go packager, production CLI/service, npm pack/install and a disposable HTTP npm registry fixture. It emits a sanitized replayable source/command/assertion manifest.
No unit tests are added. GUI, production npm/OIDC, signing, Store and GitCode operations remain separate.
