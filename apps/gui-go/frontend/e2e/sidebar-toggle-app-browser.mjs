// Failure modes: duplicate controls, stale Show/Hide labels in compact drawers, focus loss,
// and keyboard shortcuts diverging from the actual expanded state. Uses a real isolated daemon.
import assert from 'node:assert/strict'
import { spawn } from 'node:child_process'
import { readFile, writeFile } from 'node:fs/promises'
import path from 'node:path'
import { remote } from 'webdriverio'

const out = path.resolve(process.argv[2])
const info = JSON.parse(await readFile(path.join(out, 'daemon.json'), 'utf8'))
const conn = JSON.parse(await readFile(info.conn, 'utf8'))
const session = await fetch(`${info.daemonUrl}/auth/connect`, {
  method: 'POST',
  headers: { Authorization: `Bearer ${conn.token}`, 'Content-Type': 'application/json' },
  body: JSON.stringify({ pid: process.pid, clientType: 'gui' }),
})
assert.ok(session.ok)
const token = (await session.json()).data.sessionToken
const unlock = await fetch(`${info.daemonUrl}/content-lock/unlock`, {
  method: 'POST',
  headers: { Authorization: `Session ${token}`, 'Content-Type': 'application/json' },
  body: JSON.stringify({ passphrase: 'hunter22hunter22' }),
})
assert.ok(unlock.ok)
const port = Number(process.env.UI_FIXTURE_PORT || 1599)
const server = spawn(process.execPath, ['e2e/visual-effects-server.mjs'], {
  cwd: path.resolve('apps/gui-go/frontend'),
  env: {
    ...process.env,
    UC_UI_PLATFORM: 'macos',
    UI_FIXTURE_PORT: String(port),
    UI_FIXTURE_ENTRY: 'e2e/fixtures/history-full-app.tsx',
  },
  stdio: ['ignore', 'pipe', 'pipe'],
})
const result = {
  passed: false,
  scope: 'complete production frontend -> real isolated daemon; native host stubbed',
  checks: [],
}
let browser
try {
  await new Promise((resolve, reject) => {
    const timeout = setTimeout(() => reject(new Error('fixture server timeout')), 180000)
    server.stdout.on('data', data => {
      if (String(data).includes('Local:')) {
        clearTimeout(timeout)
        resolve()
      }
    })
    server.on('exit', code => {
      clearTimeout(timeout)
      reject(new Error(`fixture server exited ${code}`))
    })
  })
  browser = await remote({
    logLevel: 'error',
    capabilities: {
      browserName: 'chrome',
      'goog:chromeOptions': { args: ['--headless=new', '--window-size=1200,720'] },
    },
  })
  const url = new URL(`http://127.0.0.1:${port}/`)
  url.searchParams.set('daemon', info.daemonUrl)
  url.searchParams.set('token', token)
  await browser.url(url.href)
  await browser.$('.mac-split-toggle').waitForExist({ timeout: 60000 })
  // Dismiss only the synthetic profile's first-launch telemetry prompt.
  await browser.execute(() =>
    [...document.querySelectorAll('button')].find(b => b.textContent?.trim() === 'Disable')?.click()
  )
  for (const width of [1200, 900]) {
    await browser.setWindowSize(width, 720)
    await browser.executeAsync(done => setTimeout(done, 500))
    for (let turn = 0; turn < 4; turn++) {
      const before = await browser.$('.mac-split-toggle').getAttribute('aria-expanded')
      await browser.$('.mac-split-toggle').click()
      await browser.executeAsync(done => setTimeout(done, 400))
      const state = await browser.execute(() => {
        const all = [...document.querySelectorAll('.mac-split-toggle')]
        const toggle = all[0]
        return {
          count: all.length,
          expanded: toggle.getAttribute('aria-expanded'),
          label: toggle.getAttribute('aria-label'),
          focused: document.activeElement === toggle,
        }
      })
      result.checks.push({ width, turn, before, ...state })
      assert.equal(state.count, 1)
      assert.notEqual(state.expanded, before)
      assert.equal(state.label, state.expanded === 'true' ? 'Hide sidebar' : 'Show sidebar')
      assert.equal(state.focused, true)
      await browser.saveScreenshot(path.join(out, `${width}-${turn}.png`))
    }
    const before = await browser.$('.mac-split-toggle').getAttribute('aria-expanded')
    await browser.keys(['Control', 'Meta', 's'])
    await browser.executeAsync(done => setTimeout(done, 400))
    assert.notEqual(await browser.$('.mac-split-toggle').getAttribute('aria-expanded'), before)
  }
  result.passed = true
} finally {
  await writeFile(
    path.join(out, 'sidebar-toggle-result.json'),
    JSON.stringify(result, null, 2) + '\n'
  )
  if (browser) await browser.deleteSession()
  if (server.exitCode === null) server.kill('SIGTERM')
}
