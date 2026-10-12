// Prove from the bundler's module graph that a platform build of the UI kit gallery contains only
// its own skin: the Windows bundle has no macOS or Linux skin module, the macOS bundle has no Fluent
// UI, Griffel, Windows or Linux skin module, the Linux bundle has none of the other skins, Fluent UI
// or Griffel. Also checks that a missing or unknown UC_UI_PLATFORM fails.
//   node e2e/ui-kit-bundle-check.mjs [output-dir] [evidence-file]
// The full app (the production entries main, quick-panel, updater) is checked too: its main window
// takes the kit pages with only its own skin on every target. The Linux app must not reach the
// legacy page modules, which stay in the repository but are unused there; a build without a target
// (vitest, coverage) keeps the existing pages and no kit skin. The full app still bundles
// @base-ui/react through the existing shared components in src/components/ui, so the app check does
// not forbid that package. A human-readable summary is printed and written to [evidence-file].
import assert from 'node:assert/strict'
import { existsSync, readdirSync, readFileSync, statSync } from 'node:fs'
import { mkdtemp, readFile, writeFile } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import path from 'node:path'
import { fileURLToPath } from 'node:url'
import { gzipSync } from 'node:zlib'
import { build, loadConfigFromFile } from 'vite'

// Vite reads its config and the entry relative to the frontend, whatever directory runs this.
const invokedFrom = process.cwd()
process.chdir(path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..'))
const input = 'e2e/fixtures/ui-kit-gallery.tsx'
const output = process.argv[2]
  ? path.resolve(invokedFrom, process.argv[2])
  : await mkdtemp(path.join(tmpdir(), 'ui-kit-bundles-'))
const evidenceFile = process.argv[3] ? path.resolve(invokedFrom, process.argv[3]) : undefined
const platforms = ['windows', 'macos', 'linux']
const skin = name => new RegExp(`/src/ui-kit/skins/${name}/`)
const otherSkins = platform => platforms.filter(name => name !== platform).map(skin)
const fluent = /node_modules\/@fluentui\//
const griffel = /node_modules\/@griffel\//
const forbidden = {
  windows: [...otherSkins('windows'), /node_modules\/@base-ui\//],
  macos: [...otherSkins('macos'), fluent, griffel],
  linux: [...otherSkins('linux'), fluent, griffel],
}
const required = {
  windows: [skin('windows'), /node_modules\/@fluentui\/react-button\//],
  macos: [skin('macos'), /node_modules\/@base-ui\/react\//],
  linux: [skin('linux')],
}

const packageOf = id => {
  const match = id.match(/node_modules\/((?:@[^/]+\/)?[^/]+)/)
  if (match) return match[1]
  const skin = id.match(/\/src\/ui-kit\/skins\/([^/]+)\//)
  return skin ? `src/ui-kit/skins/${skin[1]}` : 'app source'
}

// Total bytes of the emitted JS and CSS (raw and gzip) of one build.
function bundleSize(dir) {
  const size = { files: 0, bytes: 0, gzip: 0 }
  const walk = current => {
    for (const entry of readdirSync(current, { withFileTypes: true })) {
      const file = path.join(current, entry.name)
      if (entry.isDirectory()) walk(file)
      else if (/\.(js|css)$/.test(entry.name)) {
        size.files += 1
        size.bytes += statSync(file).size
        size.gzip += gzipSync(readFileSync(file)).length
      }
    }
  }
  walk(dir)
  return size
}

async function buildGallery(platform, entry = input) {
  const previous = process.env.UC_UI_PLATFORM
  if (platform === undefined) delete process.env.UC_UI_PLATFORM
  else process.env.UC_UI_PLATFORM = platform
  const modules = new Set()
  // Modules in the import closure of each entry chunk: static imports only, and with lazy chunks.
  const entries = {}
  const outDir = path.join(output, `${entry ? 'gallery' : 'app'}-${platform ?? 'unset'}`)
  try {
    const loaded = await loadConfigFromFile({ command: 'build', mode: 'production' })
    await build({
      ...loaded.config,
      configFile: false,
      logLevel: 'silent',
      plugins: [
        ...loaded.config.plugins,
        {
          name: 'collect-bundled-modules',
          generateBundle(_, bundle) {
            for (const chunk of Object.values(bundle)) {
              if (chunk.type !== 'chunk') continue
              for (const [id, info] of Object.entries(chunk.modules)) {
                if (info.renderedLength > 0) modules.add(id.split('?')[0])
              }
              if (!chunk.isEntry) continue
              const closure = lazy => {
                const ids = new Set()
                const seen = new Set()
                const visit = file => {
                  const current = bundle[file]
                  if (!current || current.type !== 'chunk' || seen.has(file)) return
                  seen.add(file)
                  for (const [id, info] of Object.entries(current.modules)) {
                    if (info.renderedLength > 0) ids.add(id.split('?')[0])
                  }
                  current.imports.forEach(visit)
                  if (lazy) current.dynamicImports.forEach(visit)
                }
                visit(chunk.fileName)
                return [...ids].sort()
              }
              entries[chunk.name] = { static: closure(false), lazy: closure(true) }
            }
          },
        },
      ],
      build: {
        ...loaded.config.build,
        outDir,
        emptyOutDir: true,
        ...(entry ? { rollupOptions: { input: entry } } : {}),
      },
    })
    return { ok: true, modules: [...modules].sort(), entries, size: bundleSize(outDir) }
  } catch (error) {
    return { ok: false, error: error.message.replace(/\s+/g, ' ').slice(0, 400) }
  } finally {
    if (previous === undefined) delete process.env.UC_UI_PLATFORM
    else process.env.UC_UI_PLATFORM = previous
  }
}

const readMarker = async (dir, fallback) =>
  (await readFile(path.join(dir, '.ui-platform'), 'utf8').catch(() => fallback)).trim()

const report = {}
for (const platform of platforms) {
  const result = await buildGallery(platform)
  if (!result.ok) {
    report[platform] = { built: false, error: result.error }
    continue
  }
  const packages = {}
  for (const id of result.modules) packages[packageOf(id)] = (packages[packageOf(id)] ?? 0) + 1
  const leaked = result.modules.filter(id => forbidden[platform].some(pattern => pattern.test(id)))
  const missing = required[platform].filter(pattern => !result.modules.some(id => pattern.test(id)))
  report[platform] = {
    built: true,
    moduleCount: result.modules.length,
    packages,
    leaked,
    missing: missing.map(String),
    size: result.size,
  }
  await writeFile(path.join(output, `${platform}-modules.txt`), `${result.modules.join('\n')}\n`)
  report[platform].marker = await readMarker(path.join(output, `gallery-${platform}`), 'missing')
}
for (const [label, platform] of [
  ['unset', undefined],
  ['unknown', 'beos'],
]) {
  const result = await buildGallery(platform)
  report[`platform-${label}`] = result.ok ? { built: true } : { built: false, error: result.error }
}
// The production entries (main window, quick panel, updater) as vite.config.ts builds them.
const kitPage = /\/src\/pages\/history-kit\/HistoryKitPage\.tsx$/
// Legacy page sources stay in the repository; a kit target must not reach them.
const legacyPages = [
  'src/pages/HistoryPage.tsx',
  'src/pages/SetupPage.tsx',
  'src/pages/UnlockPage.tsx',
  'src/pages/ProfileRecoveryPage.tsx',
  'src/components/app/StartupProgressScreen.tsx',
  'src/components/app/AppStatusScreen.tsx',
]
// Every platform uses kit settings; untargeted tests keep the legacy modules.
const legacySettings = [
  'src/pages/SettingsPage.tsx',
  'src/components/setting/setting-parts/legacy.tsx',
]
const pattern = file => new RegExp(`/${file.replaceAll('.', '\\.')}$`)
const legacyModules = legacyPages.map(pattern)
const legacySettingsModules = legacySettings.map(pattern)
const existingPage = legacyModules[0]
const kitSettings = /\/src\/pages\/settings-kit\/SettingsKitPage\.tsx$/
const existingSettings = /\/src\/pages\/SettingsPage\.tsx$/
const kitSettingParts = /\/src\/components\/setting\/setting-parts\/kit\.tsx$/
const existingSettingParts = /\/src\/components\/setting\/setting-parts\/legacy-row\.tsx$/
const appExpectations = {
  windows: {
    required: [kitPage, kitSettings, kitSettingParts, skin('windows')],
    forbidden: [...legacyModules, existingSettings, existingSettingParts, ...otherSkins('windows')],
  },
  macos: {
    required: [kitPage, kitSettings, kitSettingParts, skin('macos')],
    forbidden: [
      ...legacyModules,
      existingSettings,
      existingSettingParts,
      ...otherSkins('macos'),
      fluent,
    ],
  },
  linux: {
    required: [kitPage, kitSettings, kitSettingParts, skin('linux')],
    forbidden: [
      ...legacyModules,
      ...legacySettingsModules,
      ...otherSkins('linux'),
      fluent,
      griffel,
    ],
  },
  unset: {
    required: [existingPage, existingSettings, existingSettingParts],
    forbidden: [kitPage, kitSettings, kitSettingParts, ...platforms.map(skin)],
  },
}
for (const [label, expectation] of Object.entries(appExpectations)) {
  const result = await buildGallery(label === 'unset' ? undefined : label, null)
  if (!result.ok) {
    report[`app-${label}`] = { built: false, error: result.error }
    continue
  }
  await writeFile(path.join(output, `app-${label}-modules.txt`), `${result.modules.join('\n')}\n`)
  const kitSkins = new Set(
    result.modules.map(id => id.match(/\/src\/ui-kit\/skins\/([^/]+)\//)?.[1]).filter(Boolean)
  )
  report[`app-${label}`] = {
    built: true,
    moduleCount: result.modules.length,
    skins: [...kitSkins].sort(),
    leaked: result.modules.filter(id => expectation.forbidden.some(pattern => pattern.test(id))),
    missing: expectation.required
      .filter(pattern => !result.modules.some(id => pattern.test(id)))
      .map(String),
    // Whether each document entry reaches any `@/ui-kit` source, statically or through lazy chunks.
    entries: Object.fromEntries(
      Object.entries(result.entries).map(([name, closures]) => [
        name,
        {
          staticModules: closures.static.length,
          lazyModules: closures.lazy.length,
          staticUiKit: closures.static.some(id => /\/src\/ui-kit\//.test(id)),
          lazyUiKit: closures.lazy.some(id => /\/src\/ui-kit\//.test(id)),
        },
      ])
    ),
    fluentPackages: [
      ...new Set(result.modules.filter(id => fluent.test(id)).map(id => packageOf(id))),
    ],
    griffelPackages: [
      ...new Set(result.modules.filter(id => griffel.test(id)).map(id => packageOf(id))),
    ],
    size: result.size,
    marker: await readMarker(path.join(output, `app-${label}`), 'none'),
  }
}
report.legacySourcesOnDisk = Object.fromEntries(
  [...legacyPages, ...legacySettings].map(file => [file, existsSync(path.resolve(file))])
)
await writeFile(path.join(output, 'report.json'), JSON.stringify(report, null, 2))
console.log(JSON.stringify(report, (key, value) => (key === 'packages' ? undefined : value), 2))
console.log(`module lists and report in ${output}`)

const kib = bytes => `${(bytes / 1024).toFixed(1)} KiB`
const summary = ['UI kit bundle isolation evidence', '']
for (const label of [...platforms, 'unset']) {
  const app = report[`app-${label}`]
  summary.push(`== real app entries, UC_UI_PLATFORM=${label === 'unset' ? '(unset)' : label} ==`)
  if (!app?.built) {
    summary.push(`  build failed: ${app?.error}`, '')
    continue
  }
  const forbiddenText =
    label === 'unset' ? 'kit page and every skin' : 'other skins and legacy page modules'
  summary.push(
    `  marker .ui-platform: ${app.marker}`,
    `  modules in the bundle: ${app.moduleCount}`,
    `  kit skins included: ${app.skins.join(', ') || 'none'}`,
    `  forbidden (${forbiddenText}) found: ${app.leaked.length}${app.leaked.length ? ` -> ${app.leaked.join(', ')}` : ''}`,
    `  @fluentui packages: ${app.fluentPackages.join(', ') || 'absent'}`,
    `  @griffel packages: ${app.griffelPackages.join(', ') || 'absent'}`,
    `  bundle size (js+css, ${app.size.files} files): ${kib(app.size.bytes)} raw, ${kib(app.size.gzip)} gzip`
  )
  for (const [name, entry] of Object.entries(app.entries)) {
    summary.push(
      `  document ${name}: imports @/ui-kit statically: ${entry.staticUiKit ? 'yes' : 'no'} (${entry.staticModules} modules), with lazy chunks: ${entry.lazyUiKit ? 'yes' : 'no'} (${entry.lazyModules} modules)`
    )
  }
  summary.push('')
}
summary.push('== gallery fixture (kit only) ==')
for (const platform of platforms) {
  const gallery = report[platform]
  summary.push(
    gallery.built
      ? `  ${platform}: ${gallery.moduleCount} modules, ${Object.keys(gallery.packages)
          .filter(name => name.startsWith('src/ui-kit/skins/'))
          .join(
            ', '
          )}, leaked ${gallery.leaked.length}, ${kib(gallery.size.bytes)} raw / ${kib(gallery.size.gzip)} gzip`
      : `  ${platform}: build failed: ${gallery.error}`
  )
}
summary.push('', '== legacy page sources still on disk ==')
for (const [file, present] of Object.entries(report.legacySourcesOnDisk)) {
  summary.push(`  ${present ? 'present' : 'MISSING'}  ${file}`)
}
console.log(summary.join('\n'))
if (evidenceFile) await writeFile(evidenceFile, `${summary.join('\n')}\n`)

for (const platform of platforms) {
  assert.equal(report[platform].built, true, `the ${platform} gallery must build`)
  assert.deepEqual(
    report[platform].leaked,
    [],
    `the ${platform} bundle contains other targets' modules`
  )
  assert.deepEqual(report[platform].missing, [])
  assert.equal(report[platform].marker, platform, `the ${platform} bundle records its target`)
}
assert.match(
  report['platform-unset'].error,
  /needs UC_UI_PLATFORM=windows\|macos\|linux \(got nothing\)/
)
assert.match(
  report['platform-unknown'].error,
  /UC_UI_PLATFORM=windows\|macos\|linux \(got "beos"\)/
)
for (const label of [...platforms, 'unset']) {
  const app = report[`app-${label}`]
  assert.equal(app.built, true, `the ${label} app must build: ${app.error ?? ''}`)
  assert.deepEqual(app.leaked, [], `the ${label} app bundles modules of another target`)
  assert.deepEqual(app.missing, [], `the ${label} app misses its History page or skin`)
  assert.equal(app.marker, label === 'unset' ? 'none' : label, `the ${label} app marker`)
}
assert.deepEqual(
  Object.entries(report.legacySourcesOnDisk).filter(([, present]) => !present),
  [],
  'the legacy page sources must stay in the repository'
)
