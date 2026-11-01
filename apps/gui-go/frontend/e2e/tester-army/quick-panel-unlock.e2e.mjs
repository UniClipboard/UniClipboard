import assert from 'node:assert/strict'
import { spawn } from 'node:child_process'
import { writeFile } from 'node:fs/promises'
import path from 'node:path'
import { test, installSession } from './backend-page.mjs'
import { backendOutput as out, backendPlatform as platform } from './backend-suites.mjs'

test('quick-panel-unlock: original real-backend assertions', async ({
  app,
  backend: daemon,
  createContext,
}) => {
  const base = daemon.baseUrl
  const session = daemon.sessionToken
  const passphrase = daemon.passphrase
  const conn = { token: daemon.fileToken }
  const marker = 'standalone-panel-synthetic-history'
  const checks = []
  function check(name, value) {
    assert(value, name)
    checks.push(name)
  }
  async function request(route, body) {
    const response = await fetch(`${base}${route}`, {
      method: body === undefined ? 'GET' : 'POST',
      headers: { Authorization: `Session ${session}`, 'Content-Type': 'application/json' },
      ...(body === undefined ? {} : { body: JSON.stringify(body) }),
    })
    assert(response.ok, `${route}: ${response.status}`)
    return (await response.json()).data
  }
  let page = await (await createContext({ viewport: { width: 800, height: 560 } })).newPage()
  const panelUrl = deny => {
    const params = new URLSearchParams({
      surface: 'quick-panel',
      platform,
    })
    if (deny) params.set('denyKeyring', '1')
    return `${app.baseUrl}/history.html?${params}`
  }
  await installSession(page, daemon)
  const open = async deny => {
    await page.goto(panelUrl(deny))
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
    (
      await input.evaluate(input =>
        (input.getAttribute('aria-describedby') || '')
          .split(/\s+/)
          .map(id => document.getElementById(id)?.textContent || '')
          .join(' ')
      )
    ).includes('Wrong passphrase')
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
  // Give the grant to a short-lived authenticated GUI client, then destroy this page.
  // Waiting beyond the former PID watcher interval makes the old behavior fail here.
  await page.close()
  await request('/content-lock/revoke', {})
  const client = spawn(
    process.execPath,
    [
      '-e',
      `
    let input = '';
    process.stdin.on('data', chunk => { input += chunk; });
    process.stdin.on('end', async () => {
      try {
        const { base, token, passphrase } = JSON.parse(input);
        const connected = await fetch(base + '/auth/connect', {
          method: 'POST', headers: { Authorization: 'Bearer ' + token, 'Content-Type': 'application/json' },
          body: JSON.stringify({ pid: process.pid, clientType: 'gui' }),
        });
        if (!connected.ok) throw new Error('Client authentication failed');
        const session = (await connected.json()).data.sessionToken;
        const unlocked = await fetch(base + '/content-lock/unlock', {
          method: 'POST', headers: { Authorization: 'Session ' + session, 'Content-Type': 'application/json' },
          body: JSON.stringify({ passphrase }),
        });
        if (!unlocked.ok || !(await unlocked.json()).data.unlocked) throw new Error('Client unlock failed');
        process.exitCode = 0;
      } catch {
        process.exitCode = 1;
      }
    });
  `,
    ],
    { stdio: ['pipe', 'ignore', 'ignore'], windowsHide: true }
  )
  await new Promise((resolve, reject) => {
    const timeout = setTimeout(() => {
      client.kill()
      reject(new Error('Short-lived unlock client timed out'))
    }, 30000)
    client.once('error', error => {
      clearTimeout(timeout)
      reject(error)
    })
    client.once('exit', code => {
      clearTimeout(timeout)
      if (code === 0) resolve()
      else reject(new Error('Short-lived unlock client failed'))
    })
    client.stdin.end(JSON.stringify({ base, token: conn.token, passphrase }))
  })
  await new Promise(resolve => setTimeout(resolve, 2500))
  check(
    'authorization survives the unlocking client process exit',
    (await request('/content-lock')).unlocked
  )
  page = await (await createContext({ viewport: { width: 800, height: 560 } })).newPage()
  await installSession(page, daemon)
  await page.goto(panelUrl(false))
  await page.getByText(marker, { exact: false }).first().waitFor()
  check(
    'reopened WebView panel shows history without unlocking again',
    !(await page.locator('#quick-panel-unlock-title').count())
  )
  await request('/content-lock/revoke', {})
  await page.locator('#quick-panel-unlock-title').waitFor()
  check(
    'explicit revocation still locks the reopened panel',
    !(await request('/content-lock')).unlocked
  )
  await writeFile(
    path.join(out, 'result.json'),
    JSON.stringify(
      { passed: true, platform, checks, nativeTransport: 'stubbed; daemon HTTP/WS real' },
      null,
      2
    )
  )
  console.log(JSON.stringify({ passed: true, platform, checks: checks.length }))
})
