import assert from 'node:assert/strict'
import { spawn } from 'node:child_process'
import { once } from 'node:events'
import { writeFile } from 'node:fs/promises'
import { createInterface } from 'node:readline'
import { test as base } from '@e2e-dev/web'
import { expect } from 'e2e'

const test = base.extend({
  daemon: async (_, provide) => {
    const proc = spawn('python3', ['apps/gui-go/frontend/e2e/tester-army/daemon.py'], {
      stdio: ['pipe', 'pipe', 'pipe'],
    })
    const exited = once(proc, 'exit')
    const lines = createInterface({ input: proc.stdout })
    let cleanup
    lines.on('line', line => {
      const message = JSON.parse(line)
      if (message.phase === 'cleanup') cleanup = message
    })
    // Keep all bootstrap/session data off the runner's output and artifacts.
    proc.stderr.resume()
    try {
      const ready = await Promise.race([
        once(lines, 'line').then(([line]) => JSON.parse(line)),
        exited.then(([code]) => {
          throw new Error(`isolated daemon helper exited ${code}`)
        }),
        new Promise((_, reject) => {
          const timer = setTimeout(
            () => reject(new Error('isolated daemon bootstrap timed out')),
            120_000
          )
          timer.unref()
          proc.once('exit', () => clearTimeout(timer))
        }),
      ])
      await provide(ready)
    } finally {
      proc.stdin.end()
      const [code] = await exited
      lines.close()
      await writeFile(
        'target/tester-army-daemon/isolation.json',
        JSON.stringify(
          {
            exitCode: code,
            ...cleanup,
          },
          null,
          2
        ) + '\n'
      )
      assert.equal(code, 0, 'isolated daemon cleanup failed')
      assert.equal(cleanup?.homeRemoved, true)
      assert.equal(cleanup?.daemonExited, true)
      assert.equal(cleanup?.systemClipboardDisabled, true)
      assert.equal(cleanup?.guiIsolated, true)
    }
  },
})

test('real daemon history reaches the browser and survives reload', async ({
  app,
  browser,
  screen,
  daemon,
}) => {
  await browser.addInitScript(
    ({ baseUrl, sessionToken }) => {
      window.__ucBrowserDaemon = { baseUrl, sessionToken }
    },
    { baseUrl: daemon.baseUrl, sessionToken: daemon.sessionToken }
  )
  await app.open('/history.html')
  const consent = screen.getByRole('button', { name: /Disable|禁用/ })
  // The isolated profile can show the first-launch diagnostics prompt.
  await expect(screen.getByRole('listbox', { name: '剪贴板历史' })).toBeVisible()
  if (await consent.isVisible()) await consent.click()
  await expect(screen.getByText(daemon.text, { exact: true }).first()).toBeVisible()
  const saved = browser.waitForResponse(/\/favorite/)
  await screen.getByRole('button', { name: '置顶', exact: true }).click()
  await saved
  await expect(screen.getByRole('button', { name: '取消置顶', exact: true })).toBeVisible()
  const response = await fetch(`${daemon.baseUrl}/clipboard/entries?limit=100`, {
    headers: { Authorization: `Session ${daemon.sessionToken}` },
  })
  assert.equal(response.status, 200)
  const entries = (await response.json()).data
  assert.equal(entries.length, 1)
  assert.equal(entries[0].isFavorited, true)
  await app.screenshot('real-daemon-history')
  await app.open('/history.html')
  await expect(screen.getByText(daemon.text, { exact: true }).first()).toBeVisible()
  await expect(screen.getByRole('button', { name: '取消置顶', exact: true })).toBeVisible()
  assert.deepEqual(await browser.evaluate(() => window.__ucPageErrors), [])
  await app.screenshot('real-daemon-history-reloaded')
})
