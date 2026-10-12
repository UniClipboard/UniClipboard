// Measures what the main window loads before its first frame, from the bundler's chunk graph:
// the static import closure of the `index.html` entry plus the `src/startup-screen.tsx` chunk
// (main.tsx imports that chunk first and only then the rest of the app). Reports bytes, gzip bytes
// and the packages in that closure, and fails when the closure contains a forbidden package or
// grows past the given budget.
//   node e2e/startup-chunk-check.mjs --platform windows|macos|none [--budget-kb N] [--json out.json]
// The default budget is the closure measured before the platform kit entry pages (300.4 KiB gzip on
// all three targets, commit 5d2694d95) plus the growth they are allowed: 64 KiB with a skin, 4 KiB
// without one (Linux keeps the existing startup screen).
import assert from 'node:assert/strict'
import { writeFile } from 'node:fs/promises'
import path from 'node:path'
import { fileURLToPath } from 'node:url'
import { gzipSync } from 'node:zlib'
import { build, loadConfigFromFile } from 'vite'

const args = process.argv.slice(2)
const arg = name => {
  const index = args.indexOf(name)
  return index === -1 ? undefined : args[index + 1]
}
const platform = arg('--platform')
assert.ok(['windows', 'macos', 'none'].includes(platform ?? ''), '--platform windows|macos|none')
const budgetKb = arg('--budget-kb')
  ? Number(arg('--budget-kb'))
  : platform === 'none'
    ? 300.4 + 4
    : 300.4 + 64
const jsonOut = arg('--json')
const invokedFrom = process.cwd()
process.chdir(path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..'))

// The app itself (bootstrap, IPC, store) and the Fluent UI barrel must never reach the first frame;
// single Fluent packages (button, progress, provider) and the icons in use may.
const forbidden = [
  /\/scripts\/startup-splash\.tsx$/,
  /node_modules\/react-dom\/.*server/,
  /node_modules\/@fluentui\/react-components\//,
  /\/src\/bootstrap\.tsx$/,
  /\/src\/lib\/ipc\.ts$/,
  /\/src\/store\//,
]

const packageOf = id => {
  const match = id.match(/node_modules\/((?:@[^/]+\/)?[^/]+)/)
  if (match) return match[1]
  const skin = id.match(/\/src\/ui-kit\/skins\/([^/]+)\//)
  return skin ? `src/ui-kit/skins/${skin[1]}` : 'app source'
}

if (platform === 'none') delete process.env.UC_UI_PLATFORM
else process.env.UC_UI_PLATFORM = platform
let report
const loaded = await loadConfigFromFile({ command: 'build', mode: 'production' })
await build({
  ...loaded.config,
  configFile: false,
  logLevel: 'silent',
  build: { ...loaded.config.build, write: false },
  plugins: [
    ...loaded.config.plugins,
    {
      name: 'startup-closure',
      generateBundle(_, bundle) {
        const chunks = Object.values(bundle).filter(item => item.type === 'chunk')
        const byName = new Map(chunks.map(chunk => [chunk.fileName, chunk]))
        const roots = chunks.filter(
          chunk =>
            (chunk.isEntry && chunk.facadeModuleId?.endsWith('/index.html')) ||
            chunk.facadeModuleId?.endsWith('/src/startup-screen.tsx')
        )
        assert.equal(roots.length, 2, 'the index entry and the startup-screen chunk')
        const seen = new Set()
        const visit = chunk => {
          if (seen.has(chunk.fileName)) return
          seen.add(chunk.fileName)
          for (const next of chunk.imports) visit(byName.get(next))
        }
        roots.forEach(visit)
        const closure = [...seen].map(name => byName.get(name))
        // A module with no rendered code (a re-exporting barrel) does not load anything itself.
        const modules = closure.flatMap(chunk =>
          Object.entries(chunk.modules)
            .filter(([, info]) => info.renderedLength > 0)
            .map(([id]) => id)
        )
        const css = Object.values(bundle).filter(
          item =>
            item.type === 'asset' &&
            item.fileName.endsWith('.css') &&
            closure.some(chunk => chunk.viteMetadata?.importedCss?.has(item.fileName))
        )
        const packages = {}
        for (const chunk of closure)
          for (const [id, info] of Object.entries(chunk.modules))
            packages[packageOf(id)] = (packages[packageOf(id)] ?? 0) + info.renderedLength
        report = {
          platform,
          chunks: closure.map(chunk => ({
            file: chunk.fileName,
            bytes: Buffer.byteLength(chunk.code),
          })),
          jsBytes: closure.reduce((sum, chunk) => sum + Buffer.byteLength(chunk.code), 0),
          jsGzipBytes: closure.reduce((sum, chunk) => sum + gzipSync(chunk.code).length, 0),
          cssBytes: css.reduce((sum, asset) => sum + Buffer.byteLength(asset.source), 0),
          packages: Object.fromEntries(Object.entries(packages).sort((a, b) => b[1] - a[1])),
          forbidden: modules.filter(id => forbidden.some(pattern => pattern.test(id))),
        }
      },
    },
  ],
})
console.log(
  JSON.stringify(
    { ...report, packages: Object.fromEntries(Object.entries(report.packages).slice(0, 15)) },
    null,
    2
  )
)
if (jsonOut) await writeFile(path.resolve(invokedFrom, jsonOut), JSON.stringify(report, null, 2))
assert.deepEqual(report.forbidden, [], 'modules that must not load before the first frame')
assert.ok(
  report.jsGzipBytes <= budgetKb * 1024,
  `first-frame JS ${report.jsGzipBytes} B gzip exceeds ${budgetKb} KiB`
)
