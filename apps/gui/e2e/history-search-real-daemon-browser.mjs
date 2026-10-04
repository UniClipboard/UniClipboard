import assert from 'node:assert/strict'
import { spawn } from 'node:child_process'
import { mkdir, writeFile } from 'node:fs/promises'
import path from 'node:path'

// Drives e2e/fixtures/history-search-real-daemon.tsx (the real History search
// components) in headless Chrome against a REAL daemon. Launched by
// tests/e2e/tests/history_search_counts.rs with UC_E2E_HISTORY_BROWSER=1, which
// seeds the daemon (3 texts indexed as `other`, design-notes.md, todo.txt) and
// passes a content-unlocked GUI session token.
//
// Env: UC_E2E_DAEMON_URL, UC_E2E_GUI_TOKEN, UC_E2E_ARTIFACT_DIR,
//      PLAYWRIGHT_MODULE (a Playwright install), PLAYWRIGHT_CHANNEL (default chrome).
const { chromium } = await import(process.env.PLAYWRIGHT_MODULE || 'playwright')
const daemonUrl = process.env.UC_E2E_DAEMON_URL
const token = process.env.UC_E2E_GUI_TOKEN
const output = process.env.UC_E2E_ARTIFACT_DIR || '/tmp/uc-history-search-browser'
const port = Number(process.env.UI_FIXTURE_PORT || 1461)
assert.ok(daemonUrl && token, 'UC_E2E_DAEMON_URL and UC_E2E_GUI_TOKEN are required')
await mkdir(output, { recursive: true })

const server = spawn('node', ['e2e/visual-effects-server.mjs'], {
  env: {
    ...process.env,
    UI_FIXTURE_ENTRY: 'e2e/fixtures/history-search-real-daemon.tsx',
    UI_FIXTURE_PORT: String(port),
  },
  stdio: ['ignore', 'pipe', 'inherit'],
})
await new Promise((resolve, reject) => {
  const timer = setTimeout(() => reject(new Error('fixture server did not start')), 120_000)
  server.stdout.on('data', chunk => {
    if (String(chunk).includes(`127.0.0.1:${port}`)) {
      clearTimeout(timer)
      resolve()
    }
  })
  server.on('exit', code => reject(new Error(`fixture server exited ${code}`)))
})

const steps = []
const consoleErrors = []
const failedResponses = []
let page
const browser = await chromium.launch({
  headless: true,
  channel: process.env.PLAYWRIGHT_CHANNEL || 'chrome',
})
try {
  page = await browser.newPage({ locale: 'en-US', viewport: { width: 760, height: 560 } })
  page.on('console', msg => {
    if (msg.type() === 'error') consoleErrors.push(msg.text())
  })
  page.on('pageerror', error => consoleErrors.push(`pageerror: ${error.message}`))
  page.on('response', res => {
    if (res.status() >= 400) {
      const u = new URL(res.url())
      u.searchParams.delete('auth')
      failedResponses.push(`${res.status()} ${u}`)
    }
  })
  const url = `http://127.0.0.1:${port}/?daemon=${encodeURIComponent(daemonUrl)}&token=${encodeURIComponent(token)}`
  await page.goto(url)

  const input = page.getByRole('combobox', { name: 'Search and filter' })
  const results = page.getByTestId('results')
  const waitTotal = async total => {
    await page.waitForFunction(
      expected => {
        const el = document.querySelector('[data-testid="results"]')
        return el?.dataset.loading === 'false' && el.dataset.total === String(expected)
      },
      total,
      { timeout: 15_000 }
    )
  }
  const shot = async name => {
    await page.screenshot({ path: path.join(output, `${name}.png`) })
  }

  await waitTotal(5)
  steps.push('initial list: 5 entries')

  // B2: per-candidate counts while typing a `type:` token.
  await input.click()
  await input.fill('type:')
  await page.waitForFunction(
    () => [...document.querySelectorAll('[role="option"]')].some(o => o.textContent === 'File2'),
    null,
    { timeout: 15_000 }
  )
  const optionTexts = await page.locator('[role="option"]').allTextContents()
  assert.deepEqual(optionTexts, ['Text0', 'Rich Text0', 'Image0', 'File2'])
  await shot('01-type-candidate-counts')
  steps.push(`type: candidates ${JSON.stringify(optionTexts)}`)

  // Commit chips, then narrow with ext:md.
  await input.fill('type:file')
  await input.press('Enter')
  await waitTotal(2)
  await input.fill('ext:md')
  await page.waitForFunction(
    () => [...document.querySelectorAll('[role="option"]')].some(o => o.textContent === '.md1'),
    null,
    { timeout: 15_000 }
  )
  await shot('02-ext-candidate-counts-with-type-file')
  await input.press('Enter')
  await waitTotal(1)
  steps.push('chips type:file + ext:md -> 1 entry')

  // Chip editing: Backspace in the empty input reopens the last chip as a token.
  await input.press('Backspace')
  assert.equal(await input.inputValue(), 'ext:md')
  await waitTotal(2)
  await shot('03-backspace-reopens-ext-chip')
  steps.push('Backspace reopened ext:md as editable text; list back to 2')
  await input.fill('')
  await input.press('Escape')

  // B3: a search that finds nothing offers one-filter relaxations.
  await input.fill('agenda')
  await input.press('Enter')
  await waitTotal(0)
  // Scoped to the empty state: the search box's chip X shares this name.
  const relax = results.getByRole('button', { name: 'Remove filter: File' })
  await relax.waitFor({ timeout: 15_000 })
  assert.match(await relax.textContent(), /1 results/)
  await page.mouse.move(0, 0)
  await shot('04-zero-result-relaxation')
  await relax.click()
  await waitTotal(1)
  assert.match(await results.textContent(), /meeting agenda/)
  await shot('05-after-removing-type-chip')
  steps.push('agenda + type:file -> 0; "Remove filter: File (1 results)" -> 1 entry')

  assert.deepEqual(failedResponses, [], 'no failed requests')
  assert.deepEqual(
    consoleErrors.filter(e => e.startsWith('pageerror')),
    [],
    'no uncaught page errors'
  )
  console.log(`PASS: ${steps.length} steps`)
} catch (error) {
  await page?.screenshot({ path: path.join(output, 'failure.png') })
  throw error
} finally {
  await writeFile(
    path.join(output, 'browser-result.json'),
    JSON.stringify({ steps, consoleErrors, failedResponses }, null, 2)
  )
  await browser.close()
  server.kill()
}
