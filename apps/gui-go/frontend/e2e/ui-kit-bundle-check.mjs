// Prove from the bundler's module graph that a platform build of the UI kit gallery contains only
// its own skin: the Windows bundle has no macOS skin module, the macOS bundle has no Fluent UI,
// Griffel or Windows skin module. Also checks that a missing or unknown UC_UI_PLATFORM fails.
//   node e2e/ui-kit-bundle-check.mjs [output-dir]
// The full app (the production entries) is checked too: its main window takes the kit History page
// with only its own skin on Windows and macOS, and a build without a target (Linux) keeps the
// existing page and no kit skin. The full app still bundles @base-ui/react through the existing
// shared components in src/components/ui, so the app check does not forbid that package.
import assert from 'node:assert/strict'
import { mkdtemp, readFile, writeFile } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import path from 'node:path'
import { fileURLToPath } from 'node:url'
import { build, loadConfigFromFile } from 'vite'

// Vite reads its config and the entry relative to the frontend, whatever directory runs this.
const invokedFrom = process.cwd()
process.chdir(path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..'))
const input = 'e2e/fixtures/ui-kit-gallery.tsx'
const output = process.argv[2]
  ? path.resolve(invokedFrom, process.argv[2])
  : await mkdtemp(path.join(tmpdir(), 'ui-kit-bundles-'))
const forbidden = {
  windows: [/\/src\/ui-kit\/skins\/macos\//, /node_modules\/@base-ui\//],
  macos: [
    /\/src\/ui-kit\/skins\/windows\//,
    /node_modules\/@fluentui\//,
    /node_modules\/@griffel\//,
  ],
}
const required = {
  windows: [/\/src\/ui-kit\/skins\/windows\//, /node_modules\/@fluentui\/react-button\//],
  macos: [/\/src\/ui-kit\/skins\/macos\//, /node_modules\/@base-ui\/react\//],
}

const packageOf = id => {
  const match = id.match(/node_modules\/((?:@[^/]+\/)?[^/]+)/)
  if (match) return match[1]
  const skin = id.match(/\/src\/ui-kit\/skins\/([^/]+)\//)
  return skin ? `src/ui-kit/skins/${skin[1]}` : 'app source'
}

async function buildGallery(platform, entry = input) {
  const previous = process.env.UC_UI_PLATFORM
  if (platform === undefined) delete process.env.UC_UI_PLATFORM
  else process.env.UC_UI_PLATFORM = platform
  const modules = new Set()
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
            }
          },
        },
      ],
      build: {
        ...loaded.config.build,
        outDir: path.join(output, `${entry ? 'gallery' : 'app'}-${platform ?? 'unset'}`),
        emptyOutDir: true,
        ...(entry ? { rollupOptions: { input: entry } } : {}),
      },
    })
    return { ok: true, modules: [...modules].sort() }
  } catch (error) {
    return { ok: false, error: error.message.replace(/\s+/g, ' ').slice(0, 400) }
  } finally {
    if (previous === undefined) delete process.env.UC_UI_PLATFORM
    else process.env.UC_UI_PLATFORM = previous
  }
}

const report = {}
for (const platform of ['windows', 'macos']) {
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
  }
  await writeFile(path.join(output, `${platform}-modules.txt`), `${result.modules.join('\n')}\n`)
  report[platform].marker = (
    await readFile(path.join(output, `gallery-${platform}`, '.ui-platform'), 'utf8').catch(
      () => 'missing'
    )
  ).trim()
}
for (const [label, platform] of [
  ['unset', undefined],
  ['unknown', 'linux'],
]) {
  const result = await buildGallery(platform)
  report[`platform-${label}`] = result.ok ? { built: true } : { built: false, error: result.error }
}
// The production entries (main window, quick panel, updater) as vite.config.ts builds them.
const kitPage = /\/src\/pages\/history-kit\/HistoryKitPage\.tsx$/
const existingPage = /\/src\/pages\/HistoryPage\.tsx$/
const kitSettings = /\/src\/pages\/settings-kit\/SettingsKitPage\.tsx$/
const existingSettings = /\/src\/pages\/SettingsPage\.tsx$/
const kitSettingParts = /\/src\/components\/setting\/setting-parts\/kit\.tsx$/
const existingSettingParts = /\/src\/components\/setting\/setting-parts\/legacy-row\.tsx$/
const skin = name => new RegExp(`/src/ui-kit/skins/${name}/`)
const appExpectations = {
  windows: {
    required: [kitPage, kitSettings, kitSettingParts, skin('windows')],
    forbidden: [existingPage, existingSettings, existingSettingParts, skin('macos')],
  },
  macos: {
    required: [kitPage, kitSettings, kitSettingParts, skin('macos')],
    forbidden: [
      existingPage,
      existingSettings,
      existingSettingParts,
      skin('windows'),
      /node_modules\/@fluentui\//,
    ],
  },
  unset: {
    required: [existingPage, existingSettings, existingSettingParts],
    forbidden: [kitPage, kitSettings, kitSettingParts, skin('windows'), skin('macos')],
  },
}
for (const [label, expectation] of Object.entries(appExpectations)) {
  const result = await buildGallery(label === 'unset' ? undefined : label, null)
  if (!result.ok) {
    report[`app-${label}`] = { built: false, error: result.error }
    continue
  }
  await writeFile(path.join(output, `app-${label}-modules.txt`), `${result.modules.join('\n')}\n`)
  report[`app-${label}`] = {
    built: true,
    moduleCount: result.modules.length,
    leaked: result.modules.filter(id => expectation.forbidden.some(pattern => pattern.test(id))),
    missing: expectation.required
      .filter(pattern => !result.modules.some(id => pattern.test(id)))
      .map(String),
    marker: (
      await readFile(path.join(output, `app-${label}`, '.ui-platform'), 'utf8').catch(() => 'none')
    ).trim(),
  }
}
await writeFile(path.join(output, 'report.json'), JSON.stringify(report, null, 2))
console.log(JSON.stringify(report, (key, value) => (key === 'packages' ? undefined : value), 2))
console.log(`module lists and report in ${output}`)

assert.equal(report.windows.built, true, 'the Windows gallery must build')
assert.deepEqual(report.windows.leaked, [], 'the Windows bundle contains macOS-only modules')
assert.deepEqual(report.windows.missing, [])
assert.equal(report.windows.marker, 'windows', 'the Windows bundle records its target')
if (report.macos.built) {
  assert.deepEqual(report.macos.leaked, [], 'the macOS bundle contains Windows-only modules')
  assert.deepEqual(report.macos.missing, [])
  assert.equal(report.macos.marker, 'macos', 'the macOS bundle records its target')
} else {
  assert.match(report.macos.error, /macos skin of @\/ui-kit is missing/)
  console.log('macOS skin not present in this tree: the macOS bundle check is NOT RUN')
}
assert.match(report['platform-unset'].error, /needs UC_UI_PLATFORM=windows\|macos \(got nothing\)/)
assert.match(report['platform-unknown'].error, /UC_UI_PLATFORM=windows\|macos \(got "linux"\)/)
for (const label of ['windows', 'macos', 'unset']) {
  const app = report[`app-${label}`]
  assert.equal(app.built, true, `the ${label} app must build: ${app.error ?? ''}`)
  assert.deepEqual(app.leaked, [], `the ${label} app bundles modules of another target`)
  assert.deepEqual(app.missing, [], `the ${label} app misses its History page or skin`)
  assert.equal(app.marker, label === 'unset' ? 'none' : label, `the ${label} app marker`)
}
