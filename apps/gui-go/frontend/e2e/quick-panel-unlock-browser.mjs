// Real-daemon unlock through the standalone QuickPanelApp. The native Wails transport is
// stubbed by history-full-app.tsx; this is a browser check, not GTK desktop acceptance.
import assert from 'node:assert/strict'
import { spawn, spawnSync } from 'node:child_process'
import { copyFile, mkdir, mkdtemp, readFile, readdir, writeFile } from 'node:fs/promises'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const frontend = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const root = path.resolve(frontend, '../../..')
const args = process.argv.slice(2)
const option = name => (args.includes(name) ? args[args.indexOf(name) + 1] : undefined)
const platform = option('--platform') ?? 'linux'
assert(['linux', 'macos', 'windows'].includes(platform))
const out = path.resolve(option('--out') ?? path.join(root, 'target/gui-go/panel-unlock/browser'))
await mkdir(out, { recursive: true })
const sandbox = await mkdtemp(path.join(out, 'uc-gui-go-'))
const profile = `gui-go-panel-${path.basename(sandbox)}`
const home = path.join(sandbox, 'home')
await mkdir(home)
for (const name of ['uniclip', 'uniclipd']) {
  const source =
    option(`--${name}`) ??
    path.join(root, name === 'uniclip' ? 'target/gui-go/uniclip' : 'target/debug/uniclipd')
  await copyFile(source, path.join(sandbox, name))
}
const env = {
  HOME: home,
  PATH: `${sandbox}:${process.env.PATH}`,
  UC_PROFILE: profile,
  UNICLIPBOARD_ENV: 'development',
  UC_PORTABLE: '1',
  UC_DISABLE_SYSTEM_CLIPBOARD: '1',
  UC_GUI_GO_ISOLATED: '1',
  XDG_DATA_HOME: path.join(home, '.local/share'),
  XDG_CONFIG_HOME: path.join(home, '.config'),
  XDG_CACHE_HOME: path.join(home, '.cache'),
  XDG_STATE_HOME: path.join(home, '.local/state'),
}
const passphrase = 'gui-go-synthetic-passphrase'
const marker = 'standalone-panel-synthetic-history'
const checks = []
let server, browser
function cli(...arguments_) {
  const result = spawnSync(path.join(sandbox, 'uniclip'), arguments_, {
    env,
    encoding: 'utf8',
    timeout: 90000,
  })
  assert.equal(result.status, 0, result.stderr || result.stdout)
}
function check(name, value) {
  assert(value, name)
  checks.push(name)
}
try {
  cli('space', 'init', '--passphrase', passphrase, '--device-name', 'panel-browser-test')
  cli('start')
  const connName = (await readdir(sandbox, { recursive: true })).find(name =>
    name.endsWith('daemon.conn')
  )
  assert(connName)
  const conn = JSON.parse(await readFile(path.join(sandbox, connName), 'utf8'))
  const base = `http://127.0.0.1:${conn.port}`
  const connected = await fetch(`${base}/auth/connect`, {
    method: 'POST',
    headers: { Authorization: `Bearer ${conn.token}`, 'Content-Type': 'application/json' },
    body: JSON.stringify({ pid: process.pid, clientType: 'gui' }),
  })
  const session = (await connected.json()).data.sessionToken
  async function request(route, body) {
    const response = await fetch(`${base}${route}`, {
      method: body === undefined ? 'GET' : 'POST',
      headers: { Authorization: `Session ${session}`, 'Content-Type': 'application/json' },
      ...(body === undefined ? {} : { body: JSON.stringify(body) }),
    })
    assert(response.ok, `${route}: ${response.status}`)
    return (await response.json()).data
  }
  await request('/content-lock/unlock', { passphrase })
  await request('/clipboard/dispatch', { text: marker })
  await request('/content-lock/revoke', {})
  server = spawn(process.execPath, ['e2e/visual-effects-server.mjs'], {
    cwd: frontend,
    env: {
      ...process.env,
      UC_UI_PLATFORM: platform,
      UI_FIXTURE_ENTRY: 'e2e/fixtures/history-full-app.tsx',
      UI_FIXTURE_PORT: '0',
    },
    stdio: ['ignore', 'pipe', 'pipe'],
  })
  let output = ''
  server.stdout.on('data', chunk => {
    output += chunk
  })
  server.stderr.on('data', chunk => {
    output += chunk
  })
  const deadline = Date.now() + 120000
  while (!output.match(/http:\/\/127\.0\.0\.1:\d+/) && Date.now() < deadline) {
    assert(server.exitCode === null, output)
    await new Promise(resolve => setTimeout(resolve, 100))
  }
  const url = output.match(/http:\/\/127\.0\.0\.1:\d+/)?.[0]
  assert(url, output)
  const { chromium } = await import(process.env.PLAYWRIGHT_MODULE || 'playwright')
  browser = await chromium.launch({
    executablePath: process.env.CHROMIUM_PATH || '/usr/bin/chromium',
    headless: true,
  })
  const page = await browser.newPage({ viewport: { width: 800, height: 560 } })
  const open = async deny => {
    const params = new URLSearchParams({
      surface: 'quick-panel',
      platform,
      daemon: base,
      token: session,
    })
    if (deny) params.set('denyKeyring', '1')
    await page.goto(`${url}/?${params}`)
    await page.locator('#quick-panel-unlock-title').waitFor()
  }
  await open(false)
  await page.locator('section button').click()
  await page.getByText(marker, { exact: false }).first().waitFor()
  check(
    'keyring unlock grants real daemon content access',
    (await request('/content-lock')).unlocked
  )
  await request('/content-lock/revoke', {})
  await open(true)
  await page.locator('section button').click()
  const input = page.locator('input#unlock-passphrase')
  await input.waitFor()
  await input.fill('wrong-passphrase')
  await input.press('Enter')
  await page.locator('input#unlock-passphrase[aria-invalid=true]').waitFor()
  check(
    'wrong passphrase shows the expected localized error',
    (await page.locator('#unlock-passphrase-error').textContent()).includes('Wrong passphrase')
  )
  check('wrong passphrase leaves the daemon locked', !(await request('/content-lock')).unlocked)
  check(
    'wrong passphrase leaves history hidden',
    !(await page.getByText(marker, { exact: false }).count())
  )
  for (const [width, height] of [
    [800, 560],
    [360, 420],
    [288, 336],
  ]) {
    await page.setViewportSize({ width, height })
    for (const theme of ['light', 'dark']) {
      await page.evaluate(
        theme => document.documentElement.classList.toggle('dark', theme === 'dark'),
        theme
      )
      await page.screenshot({ path: path.join(out, `${platform}-${width}-${theme}.png`) })
      const box = await input.boundingBox()
      check(
        `passphrase input fits ${width}x${height} ${theme}`,
        box && box.x >= 0 && box.x + box.width <= width
      )
    }
  }
  await page.setViewportSize({ width: 800, height: 560 })
  await input.fill(passphrase)
  await input.press('Enter')
  await page.getByText(marker, { exact: false }).first().waitFor()
  check(
    'passphrase unlock grants real daemon content access',
    (await request('/content-lock')).unlocked
  )
  const calls = await page.evaluate(() => window.__ucNativeCalls)
  check(
    'standalone panel never requests the main unlock window',
    !calls.some(call => call.command === 'ShowContentUnlock')
  )
  check(
    'standalone panel uses the real passphrase command',
    calls.some(call => call.command === 'UnlockContent')
  )
  const errors = await page.evaluate(() => window.__ucPageErrors)
  check('no unhandled page errors', errors.length === 0)
  await writeFile(
    path.join(out, 'result.json'),
    JSON.stringify(
      { passed: true, platform, checks, nativeTransport: 'stubbed; daemon HTTP/WS real' },
      null,
      2
    )
  )
  console.log(JSON.stringify({ passed: true, platform, checks: checks.length }))
} finally {
  await browser?.close()
  server?.kill('SIGTERM')
  cli('stop')
}
