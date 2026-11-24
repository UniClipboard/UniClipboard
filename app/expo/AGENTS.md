# Expo mobile shell

Read [README.md](README.md) and the
[repository architecture](../../docs/architecture/overview.md) before changes.

- This is one Expo application for Android and iOS. Do not place its generated
  native projects in the sibling `app/ios` or `app/android` directories.
- Native builds integrate the Go Engine in-process via `modules/unic-engine`
  (`bindings/mobile`). When `UnicEngine` is absent in JS-only runs, keep demo
  operations explicit via `mode: "unavailable"` and never report a real pair,
  transfer, copy or sync success.
- Keep `app/expo/package.json` `dependencies` and `devDependencies` pinned to the
  accepted Expo 57 versions and retain the mandatory navigation patches.
- Reuse existing components in `src/components/ui` and platform settings rows.
  Shared UI must not branch on `Platform.OS`. Use platform-resolved files for
  substantial differences. Keep identical screen composition shared.
- Keep iOS sheets outside animated sub-pages. Their stable parent owns state.
- Keep whole settings rows interactive. Use existing full-row components.
- Preserve locked Expo/UI/navigation versions and mandatory UI patches.
  Read the exact Expo 57 versioned documentation before changing SDK usage.
- Run `npm run verify` for bundle acceptance. Use native Maestro E2E for
  interaction acceptance. Produce a receipt and screenshots. Do not add unit
  tests after implementing code.
- Do not claim native acceptance from TypeScript, prebuild or bundle checks.
- Keep documentation in English. Retain upstream license attribution.
