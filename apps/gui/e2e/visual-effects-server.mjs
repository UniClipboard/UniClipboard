import { mkdtemp, readFile, writeFile } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import path from 'node:path'
import { build, preview, loadConfigFromFile } from 'vite'

// Measure production components, without development hot reload or its error overlay.
const loaded = await loadConfigFromFile({ command: 'build', mode: 'test' })
const outDir = await mkdtemp(path.join(tmpdir(), 'smooth-fixture-'))
const input = process.env.UI_FIXTURE_ENTRY || 'e2e/fixtures/visual-effects.tsx'
await build({
  ...loaded.config,
  configFile: false,
  logLevel: 'warn',
  build: {
    ...loaded.config.build,
    outDir,
    manifest: true,
    emptyOutDir: false,
    rollupOptions: { input },
  },
})
const manifest = JSON.parse(await readFile(path.join(outDir, '.vite/manifest.json'), 'utf8'))
const entry = manifest[input]
await writeFile(
  path.join(outDir, 'index.html'),
  `<!doctype html><html><head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1">${(entry.css || []).map(file => `<link rel="stylesheet" href="/${file}">`).join('')}</head><body><div id="root"></div><script type="module" src="/${entry.file}"></script></body></html>`
)
const server = await preview({
  configFile: false,
  build: { outDir },
  preview: {
    host: '127.0.0.1',
    port: Number(process.env.UI_FIXTURE_PORT || 1439),
    strictPort: true,
  },
})
server.printUrls()
