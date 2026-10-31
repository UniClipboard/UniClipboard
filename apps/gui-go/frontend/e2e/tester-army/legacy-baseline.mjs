import assert from 'node:assert/strict'
import { spawn } from 'node:child_process'
import { once } from 'node:events'
import { createWriteStream } from 'node:fs'
import { mkdir, writeFile } from 'node:fs/promises'
import { createRequire } from 'node:module'
import { createServer } from 'node:net'
import path from 'node:path'
import { suites, retainedReasons } from './suites.mjs'

const [name, platform] = process.argv.slice(2)
assert.ok(
  suites[name]?.platforms.includes(platform),
  'choose a known synthetic browser suite/platform'
)
assert.ok(retainedReasons[name], 'baseline is restricted to retained original scripts')
const root = process.cwd()
const output = path.resolve('target/tester-army-baseline', platform, name)
await mkdir(output, { recursive: true })
const require = createRequire(import.meta.url)
const modulePath = require
  .resolve('playwright-core', { paths: [require.resolve('@e2e-dev/web')] })
  .replace(/index\.js$/, 'index.mjs')
const { chromium } = await import(modulePath)
const listener = createServer()
await new Promise(resolve => listener.listen(0, '127.0.0.1', resolve))
const port = listener.address().port
await new Promise(resolve => listener.close(resolve))
const base = `http://127.0.0.1:${port}`
const env = {
  ...process.env,
  PLAYWRIGHT_MODULE: modulePath,
  PLAYWRIGHT_EXECUTABLE_PATH: chromium.executablePath(),
  PLAYWRIGHT_EXECUTABLE: chromium.executablePath(),
  UI_KIT_PLATFORMS: platform,
  UI_KIT_PLATFORM: platform,
  UI_FIXTURE_PORT: String(port),
  UC_TESTER_FIXTURE: suites[name].fixture,
  SMOOTH_TEST_PLATFORM: platform,
  UI_KIT_TEST_URL: base,
  HISTORY_COMPACT_URL: base,
  MACOS_DESIGN_URL: base,
  TAG_MENU_URL: base,
  TAG_MENU_OUTPUT: output,
  SETTINGS_TEST_URL: base,
  SETTINGS_SCROLL_TEST_URL: base,
  TYPOGRAPHY_TEST_URL: base,
  SMOOTH_TEST_URL: base,
}
if (platform === 'none') delete env.UC_UI_PLATFORM
else env.UC_UI_PLATFORM = platform
for (const key of [
  'SETTINGS_TEST_OUTPUT',
  'SETTINGS_SCROLL_TEST_OUTPUT',
  'TYPOGRAPHY_TEST_OUTPUT',
  'HISTORY_HOVER_OUTPUT',
  'SMOOTH_TEST_OUTPUT',
  'UI_KIT_TEST_OUTPUT',
])
  env[key] = output
const selfHosted = ['app-entry-kit', 'file-preview', 'history-detail-regressions'].includes(name)
let server
let serverExit
const log = createWriteStream(path.join(output, 'run.log'))
try {
  if (!selfHosted) {
    server = spawn(
      process.execPath,
      ['apps/gui-go/frontend/e2e/tester-army/server.mjs', String(port)],
      { env, stdio: ['ignore', 'pipe', 'pipe'] }
    )
    serverExit = once(server, 'exit')
    server.stdout.pipe(log, { end: false })
    server.stderr.pipe(log, { end: false })
    await Promise.race([
      new Promise(resolve =>
        server.stdout.on('data', chunk => {
          if (chunk.toString().includes('Local:')) resolve()
        })
      ),
      serverExit.then(([code]) => {
        throw new Error(`baseline server exited ${code}`)
      }),
    ])
  }
  const filename = name === 'history-detail-regressions' ? `${name}.mjs` : `${name}-browser.mjs`
  const args =
    name === 'app-entry-kit'
      ? ['--platform', platform, '--out', output]
      : name === 'history-detail-regressions'
        ? [output]
        : []
  const proc = spawn(process.execPath, [`e2e/${filename}`, ...args], {
    cwd: path.join(root, 'apps/gui-go/frontend'),
    env,
    stdio: ['ignore', 'pipe', 'pipe'],
  })
  proc.stdout.pipe(log, { end: false })
  proc.stderr.pipe(log, { end: false })
  const [exitCode] = await once(proc, 'exit')
  await writeFile(
    path.join(output, 'result.json'),
    JSON.stringify(
      {
        name,
        platform,
        exitCode,
        source: filename,
        playwrightVersion: require('playwright-core/package.json').version,
      },
      null,
      2
    ) + '\n'
  )
  console.log(`${name}/${platform}: legacy exit ${exitCode}`)
} finally {
  if (server) {
    server.kill()
    await serverExit
  }
  log.end()
}
