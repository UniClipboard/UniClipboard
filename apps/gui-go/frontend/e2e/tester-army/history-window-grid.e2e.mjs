import { writeFile } from 'node:fs/promises'
import { openGuiSession } from '../browser-daemon-session.mjs'
import { assertBoundedGrid } from '../history-bounded-grid.mjs'
import { test, installSession } from './backend-page.mjs'
import { backendOutput as output, backendPlatform as platform } from './backend-suites.mjs'

test('history-window-grid: every real record with bounded bidirectional navigation', async ({
  app,
  backend,
  createContext,
}) => {
  const context = await createContext({
    viewport: { width: 1280, height: 12_000 },
    reducedMotion: 'reduce',
  })
  const page = await context.newPage()
  await installSession(page, backend)
  await page.goto(`${app.baseUrl}/history.html?platform=${platform}`)
  await page.locator('[data-item-id]').first().waitFor({ timeout: 60000 })
  const consent = page.getByRole('alertdialog').getByRole('button', { name: 'Disable' })
  await consent.waitFor({ timeout: 5000 }).catch(() => {})
  if (await consent.isVisible()) await consent.click()
  const token = await openGuiSession(backend)
  const api = async route => {
    const response = await fetch(backend.daemonUrl + route, {
      headers: { Authorization: `Session ${token}` },
    })
    return { status: response.status, body: response.ok ? await response.json() : null }
  }
  const result = await assertBoundedGrid({
    page,
    rows: () => page.locator('[data-item-id]'),
    list: () => page.getByRole('listbox'),
    api,
  })
  await page.screenshot({ path: `${output}/bounded-grid.png` })
  await writeFile(`${output}/bounded-grid.json`, JSON.stringify(result, null, 2) + '\n')
})
