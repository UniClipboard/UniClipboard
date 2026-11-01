import { mkdtemp, readFile, rm, writeFile } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import path from 'node:path'
import { fileURLToPath } from 'node:url'
import { build, loadConfigFromFile, preview } from 'vite'
import { buildLeaseEnv, claimBuildLease } from './build-lease.mjs'

process.chdir(fileURLToPath(new URL('../..', import.meta.url)))
const lease = process.env[buildLeaseEnv]
const outDir = lease
  ? await claimBuildLease(lease)
  : await mkdtemp(path.join(tmpdir(), 'uc-gui-browser-'))
console.log(
  JSON.stringify({
    event: 'fixture-build',
    buildId: path.basename(lease || outDir),
    leased: Boolean(lease),
    pid: process.pid,
  })
)
let server
let stopping = false
async function stop() {
  if (stopping) return
  stopping = true
  if (server) await server.close()
  if (!lease) await rm(outDir, { recursive: true, force: true })
  console.log(
    JSON.stringify({
      event: 'fixture-cleanup',
      buildId: path.basename(lease || outDir),
      directoryRemoved: !lease,
      cleanupOwner: lease ? 'runner' : 'server',
    })
  )
  process.exit(0)
}
process.on('SIGTERM', stop)
process.on('SIGINT', stop)
if (process.env.CI !== 'true') process.stdin.on('end', stop)
try {
  const loaded = await loadConfigFromFile({ command: 'build', mode: 'test' })
  const fixture = process.env.UC_TESTER_FIXTURE || 'settings'
  const mainDocument = fixture === 'main-document'
  const input = mainDocument
    ? 'index.html'
    : fixture.endsWith('.html')
      ? fixture
      : `e2e/fixtures/${fixture}.tsx`
  const inputs =
    process.env.UC_TESTER_DAEMON === '1' ? [input, 'e2e/fixtures/history-full-app.tsx'] : [input]
  if (process.env.UC_TESTER_BACKENDS === '1')
    inputs.push('e2e/fixtures/history-search-components.tsx')
  await build({
    ...loaded.config,
    configFile: false,
    logLevel: 'warn',
    build: {
      ...loaded.config.build,
      outDir,
      manifest: true,
      rollupOptions: { input: inputs },
    },
  })
  await writeFile(
    path.join(outDir, 'runner-blank.html'),
    '<!doctype html><title>Runner bootstrap</title>'
  )
  const manifest = JSON.parse(await readFile(path.join(outDir, '.vite/manifest.json'), 'utf8'))
  for (const input of mainDocument ? [] : inputs) {
    const entry = manifest[input]
    const name = input.includes('history-full-app')
      ? 'history.html'
      : input.includes('history-search-components')
        ? 'components.html'
        : 'index.html'
    await writeFile(
      path.join(outDir, name),
      `<!doctype html><html><head><meta charset="utf-8">${(entry.css || []).map(file => `<link rel="stylesheet" href="/${file}">`).join('')}</head><body><div id="root"></div><script type="module" src="/${entry.file}"></script></body></html>`
    )
  }
  server = await preview({
    configFile: false,
    build: { outDir },
    preview: {
      host: '127.0.0.1',
      port: Number(process.argv[2]),
      strictPort: true,
    },
  })
  server.printUrls()
} catch (error) {
  if (!lease) await rm(outDir, { recursive: true, force: true })
  throw error
}
