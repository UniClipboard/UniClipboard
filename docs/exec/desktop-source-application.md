# Desktop source application detail header

## Scope

Show the captured application's cached icon and name after device/time in the history detail header.
Bind the existing history API field through `projectHistory` and keep application identity separate from clipboard preview images.
Use the existing image decoder, semantic UI Kit text/icon components and localization catalogue.
No dependency, backend protocol or configuration changes are needed.

## Failure inventory specified before implementation

API projection drops metadata; cached icons do not render; malformed icons break the header;
missing names hide usable identifiers; unknown or switched records retain stale provenance;
remote records show the local application; long names displace time or actions;
localized UI translates application names.

## Acceptance

The registered `source-application` software-view case exercises complete history views using service DTOs.
It covers macOS, Linux and Windows skins, light/dark themes, long labels at two viewport widths,
valid/missing/invalid icons, identifier fallback, unknown provenance and selection changes.
It retains rendered PNGs and geometry/pixel assertions through the canonical Desktop E2E runner.

Validated on macOS arm64 with Go 1.27.3. Initial HEAD: `b4fff3b8f57d512274322de0ed0317e52a912c72`.
Acceptance ran before implementation commits. All four reports identify the same final production UI files by SHA-256.
Retained evidence checksums were verified after execution.

| Command | Result | Report |
| --- | --- | --- |
| `make desktop-e2e CASE=source-application` | 48 view subcases passed; 48 PNG checkpoints; cleanup passed | `.build/desktop-e2e/20261112T124642Z-c7fccbd3/index.html` |
| `make desktop-e2e CASE=toolbar-alignment` | Passed | `.build/desktop-e2e/20261112T124722Z-99a7638d/index.html` |
| `make desktop-e2e CASE=data-binding` | Passed with a production service, including application metadata and icon after detail loading and background refresh | `.build/desktop-e2e/20261112T124839Z-f2c34b6a/index.html` |
| `make desktop-e2e CASE=i18n` | Passed | `.build/desktop-e2e/20261112T124856Z-36b01e74/index.html` |
| `go build -o .build/desktop-source-application/unic-desktop ./app/desktop/cmd/unic-desktop` | Passed | `.build/desktop-source-application/build-proof.json` |
| `go vet ./app/desktop/internal/desktopui ./app/desktop/internal/i18n` | Passed | Static analysis |
| `go run ./tools/docscheck` | Passed with 8 stale-page warnings | Documentation links and rules |
| Pre-commit `go run ./tools/docscheck -bump docs/architecture/desktop-runtime.md` attempt | Refused because source was uncommitted | Verification marker follows implementation commits |

The initial view scenario failed because the header lacked application metadata.
Screenshot inspection then found that a long application name could shrink device/time.
The final layout gives device/time a bounded nonshrinking area and truncates the application name first.
The strengthened scenario compares time width against the short-name baseline.

Software rendering does not establish native window behavior or actual OS clipboard attribution.
The full Desktop suite includes native cases and was not run. Only the four explicitly listed cases were selected.

No native app or browser is launched during this task.

## Publication preparation

Implementation source commit: `7aff73d6caa87ea37c31d4e80603286bd42d4b6b`.
The [architecture verification marker](../architecture/desktop-runtime.md) was refreshed after implementation commits.
Acceptance limits above remain unchanged. Native clipboard attribution and native-window acceptance remain not run.
