# Desktop module separation

## Execution contract

The production command uses `desktopui` for rendering and service binding and `i18n` for translation. The independent `unic-desktop-demo` command uses `desktopdemo`, which supplies synthetic data to the same renderer. The production command and shared renderer have no dependency on the demo launcher or fixture builders.

`app/desktop/internal/i18n/locales.json` is the single embedded translation catalogue. The lookup package depends only on the standard library. Both desktop commands retain the same translation fallback and five supported languages. User content and custom tag names remain literal.

The `desktopui/preview` package contains only explicit preview input records. It lets demo fixture builders and same-package view acceptance reuse the original synthetic data without introducing an import cycle. Production startup does not construct preview data. The demo command replaces the previous production `--demo` mode; it starts no service.

## Failure inventory

Demo dependency in production; fixture dependency in renderer; translation dependency on UI; missing embed; changed fallback; untranslated tray; altered fixture content or image; wrong preview theme; lost scenario registry; obsolete command documentation; help opening a window; normal/lightweight startup regression; unverified PR head.

## Acceptance commands

```sh
make desktop-e2e CASE=production-boundary
make desktop-e2e CASE=i18n
make desktop-e2e CASE=tag-manager
make desktop-e2e CASE=data-binding
make desktop-e2e CASE=quick-panel
make desktop-e2e CASE=startup-lifecycle
```

Each canonical report retains source/binary identity, assertions and manifest/checksums. View cases retain software-rendered checkpoints; process cases retain structured receipts. Native windows, system clipboard, tray delivery and OS focus are not exercised by these cases.

## Results

| Case | Status | Report assertions | Report |
|---|---|---|---|
| `production-boundary` | PASS | 1 | `.build/desktop-e2e/20261112T092728Z-ea1832d5/index.html` |
| `i18n` | PASS | 61 | `.build/desktop-e2e/20261112T092728Z-cb44d999/index.html` |
| `data-binding` | PASS | 1 | `.build/desktop-e2e/20261112T092731Z-f9ea8361/index.html` |
| `quick-panel` | PASS | 12 | `.build/desktop-e2e/20261112T092810Z-675fb420/index.html` |
| `tag-manager` | PASS | 31 | `.build/desktop-e2e/20261112T092835Z-cd422fc5/index.html` |

The dependency process receipt records the six boundary checks and both binary SHA-256 values. The translation catalogue bytes match the pre-refactor catalogue exactly. All existing fixture strings remain unchanged; the three removed constructor strings are navigation defaults now supplied by the shared preview constructor.

The repository-pinned golangci-lint v2.15.0 reports zero change-scoped issues for Desktop. Its scoped formatting check and the ordinary documentation checker pass. The PATH lint binary cannot read the repository v2 configuration; the pinned binary was used instead.

Startup lifecycle acceptance passed with 1 report assertion at `.build/desktop-e2e/20261112T093133Z-29844362/index.html`. The detailed process receipt covers normal/lightweight startup, service ownership and settings behavior. Native acceptance and the complete acceptance suite remain outside this nonvisual refactor acceptance.
