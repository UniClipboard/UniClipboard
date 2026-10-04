import assert from 'node:assert/strict'
import { spawn } from 'node:child_process'
import { mkdir, writeFile } from 'node:fs/promises'
import path from 'node:path'
import { remote } from 'webdriverio'

// Browser half of tests/e2e/tests/history_search_counts.rs (UC_E2E_HISTORY_BROWSER=1).
// Headless Chrome via the repo's own webdriverio, against the REAL daemon that
// test seeded: 3 texts ingested through mobile LAN sync, design-notes.md and
// todo.txt captured as files. Two separate phases, reported separately:
//   A. components: e2e/fixtures/history-search-components.tsx — only the
//      search box + zero-result relaxations. Not a page-level acceptance.
//   B. full page:  e2e/fixtures/history-full-app.tsx — the complete frontend
//      (`@/bootstrap`) on /history; only the Tauri native layer is stubbed.
// Env: UC_E2E_DAEMON_URL, UC_E2E_GUI_TOKEN, UC_E2E_ARTIFACT_DIR.
const daemonUrl = process.env.UC_E2E_DAEMON_URL
const token = process.env.UC_E2E_GUI_TOKEN
const output = process.env.UC_E2E_ARTIFACT_DIR || '/tmp/uc-history-search-browser'
assert.ok(daemonUrl && token, 'UC_E2E_DAEMON_URL and UC_E2E_GUI_TOKEN are required')
await mkdir(output, { recursive: true })
const query = `?daemon=${encodeURIComponent(daemonUrl)}&token=${encodeURIComponent(token)}`

async function serveFixture(entry, port) {
  const server = spawn('node', ['e2e/visual-effects-server.mjs'], {
    env: { ...process.env, UI_FIXTURE_ENTRY: entry, UI_FIXTURE_PORT: String(port) },
    stdio: ['ignore', 'pipe', 'inherit'],
  })
  await new Promise((resolve, reject) => {
    const timer = setTimeout(() => reject(new Error(`${entry}: server did not start`)), 180_000)
    server.stdout.on('data', chunk => {
      if (String(chunk).includes(`127.0.0.1:${port}`)) {
        clearTimeout(timer)
        resolve()
      }
    })
    server.on('exit', code => reject(new Error(`${entry}: server exited ${code}`)))
  })
  return server
}

async function openBrowser(width, height) {
  return remote({
    logLevel: 'error',
    capabilities: {
      browserName: 'chrome',
      'goog:chromeOptions': {
        args: ['--headless=new', `--window-size=${width},${height}`, '--lang=en-US'],
        prefs: { intl: { accept_languages: 'en-US' } },
      },
    },
  })
}

const report = { components: { steps: [] }, fullPage: { steps: [] } }
const writeReport = () =>
  writeFile(path.join(output, 'browser-result.json'), JSON.stringify(report, null, 2))

async function runPhase(name, entry, port, size, body) {
  const server = await serveFixture(entry, port)
  const browser = await openBrowser(size.width, size.height)
  const phase = report[name]
  const shot = async file => browser.saveScreenshot(path.join(output, `${name}-${file}.png`))
  try {
    await browser.url(`http://127.0.0.1:${port}/${query}`)
    await body(browser, phase, shot)
    phase.result = 'pass'
  } catch (error) {
    phase.result = 'fail'
    phase.error = String(error?.stack ?? error)
    await shot('failure').catch(() => {})
    throw error
  } finally {
    phase.pageErrors = await browser.execute(() => window.__ucPageErrors ?? []).catch(() => [])
    phase.nativeCalls = await browser.execute(() => window.__ucNativeCalls ?? []).catch(() => [])
    await writeReport()
    await browser.deleteSession()
    server.kill()
  }
}

const optionTexts = browser =>
  browser.execute(() => [...document.querySelectorAll('[role="option"]')].map(o => o.textContent))

// ── A. components ───────────────────────────────────────────────────────────
await runPhase(
  'components',
  'e2e/fixtures/history-search-components.tsx',
  1461,
  { width: 760, height: 560 },
  async (browser, phase, shot) => {
    const input = await browser.$('[role="combobox"]')
    const waitTotal = total =>
      browser.waitUntil(
        () =>
          browser.execute(expected => {
            const el = document.querySelector('[data-testid="results"]')
            return el?.dataset.loading === 'false' && el.dataset.total === String(expected)
          }, total),
        { timeout: 15_000, timeoutMsg: `list total never became ${total}` }
      )
    const waitOptions = expected =>
      browser.waitUntil(
        async () => JSON.stringify(await optionTexts(browser)) === JSON.stringify(expected),
        {
          timeout: 15_000,
          timeoutMsg: `options never became ${JSON.stringify(expected)}`,
        }
      )
    const type = async text => {
      await input.click()
      await browser.keys(text.split(''))
    }

    await waitTotal(5)
    phase.steps.push('initial list: 5 entries')

    await type('type:')
    await waitOptions(['Text3', 'Rich Text0', 'Image0', 'File2'])
    await shot('01-type-candidate-counts')
    phase.steps.push('type: candidates Text 3 / Rich Text 0 / Image 0 / File 2')

    await browser.keys(['Backspace', 'Backspace', 'Backspace', 'Backspace', 'Backspace'])
    await type('type:text')
    await browser.keys(['Enter'])
    await waitTotal(3)
    await type('ext:md')
    await waitOptions(['.md0'])
    await shot('02-ext-candidate-count-with-text-chip')
    await browser.keys(['Enter'])
    await waitTotal(0)
    phase.steps.push('chips type:text + ext:md -> 0 entries (.md candidate showed 0)')

    // B3: both one-chip relaxations, singular and plural counts.
    await browser.waitUntil(
      async () => (await browser.$$('[data-testid="results"] button')).length === 2,
      { timeout: 15_000, timeoutMsg: 'relaxation buttons never appeared' }
    )
    const relaxTexts = await browser.execute(() =>
      [...document.querySelectorAll('[data-testid="results"] button')].map(b => [
        b.getAttribute('aria-label'),
        b.textContent,
        b.disabled,
      ])
    )
    assert.deepEqual(relaxTexts, [
      ['Remove filter: Text', 'Text1 result', false],
      ['Remove filter: .md', '.md3 results', false],
    ])
    await browser.$('body').moveTo({ xOffset: 1, yOffset: 1 })
    await shot('03-zero-result-relaxations')
    phase.steps.push('relaxations: drop Text -> "1 result", drop .md -> "3 results"')

    // Chip editing: Backspace in the empty input reopens the last chip.
    await input.click()
    await browser.keys(['Backspace'])
    assert.equal(await input.getValue(), 'ext:md')
    await waitTotal(3)
    await shot('04-backspace-reopens-ext-chip')
    phase.steps.push('Backspace reopened ext:md as editable text; list back to 3')

    assert.deepEqual(
      (await browser.execute(() => window.__ucPageErrors ?? [])).filter(Boolean),
      [],
      'no uncaught page errors'
    )
  }
)

// ── B. full page ────────────────────────────────────────────────────────────
// Same demo data, complete /history page. States mirror the design artboards:
// B1 type-ahead with a `from:` chip, B2 filtered results, B3 no results with
// one-filter relaxations.
await runPhase(
  'fullPage',
  'e2e/fixtures/history-full-app.tsx',
  1463,
  { width: 1280, height: 800 },
  async (browser, phase, shot) => {
    const rowCount = () =>
      browser.execute(() => document.querySelectorAll('[data-testid="history-row"]').length)
    const waitRows = n =>
      browser.waitUntil(async () => (await rowCount()) === n, {
        timeout: 20_000,
        timeoutMsg: `history list never showed ${n} rows`,
      })
    const waitOptions = expected =>
      browser.waitUntil(
        async () => JSON.stringify(await optionTexts(browser)) === JSON.stringify(expected),
        { timeout: 15_000, timeoutMsg: `options never became ${JSON.stringify(expected)}` }
      )
    const keys = text => browser.keys(text.split(''))

    await waitRows(5)
    const disable = await browser.$('//button[normalize-space()="Disable"]')
    if (await disable.isExisting()) await disable.click()
    await shot('00-history')
    phase.steps.push('complete /history page: sidebar, 5 rows, preview')

    await (await browser.$('button[aria-label="Search and filter"]')).click()
    const input = await browser.$('[role="combobox"]')
    await input.waitForExist({ timeout: 10_000 })
    await keys('from:e2e')
    await waitOptions(['e2e-phone3'])
    await browser.keys(['Enter'])
    await waitRows(3)
    await keys('type:')
    await waitOptions(['Text3', 'Rich Text0', 'Image0', 'File0'])
    await shot('B1-typeahead-with-from-chip')
    phase.steps.push('B1: from:e2e-phone chip + "type:" -> Text 3 / Rich Text 0 / Image 0 / File 0')

    await browser.keys(['Enter'])
    await waitRows(3)
    await keys('agenda')
    await browser.keys(['Enter'])
    await waitRows(1)
    await shot('B2-filtered-results')
    phase.steps.push('B2: chips from:e2e-phone + Text, keyword "agenda" -> 1 row')

    await browser.keys(Array(6).fill('Backspace'))
    await waitRows(3)
    await keys('ext:md')
    await waitOptions(['.md0'])
    await browser.keys(['Enter'])
    await waitRows(0)
    const relaxations = () =>
      browser.execute(() =>
        [...document.querySelectorAll('button')]
          .filter(b => /\d+ results?$/.test(b.textContent ?? ''))
          .map(b => [b.getAttribute('aria-label'), b.textContent, b.disabled])
      )
    await browser.waitUntil(async () => (await relaxations()).length === 3, {
      timeout: 15_000,
      timeoutMsg: 'relaxations never appeared on the page',
    })
    assert.deepEqual(await relaxations(), [
      ['Remove filter: Text', 'Text0 results', true],
      ['Remove filter: e2e-phone', 'e2e-phone0 results', true],
      ['Remove filter: .md', '.md3 results', false],
    ])
    await shot('B3-no-results-relaxations')
    phase.steps.push(
      'B3: 3 chips -> 0 rows; drop Text 0, drop e2e-phone 0 (both disabled), drop .md 3'
    )

    // The chip's own X shares the aria-label; the relaxation is the one with a count.
    await browser.execute(() =>
      [...document.querySelectorAll('button')].find(b => b.textContent === '.md3 results').click()
    )
    await waitRows(3)
    await shot('B3-after-dropping-md')
    phase.steps.push('clicking the .md relaxation -> 3 rows')

    // Chips are ordered type -> source -> extension, so the last one is the source.
    await input.click()
    await browser.keys(['Backspace'])
    assert.equal(await input.getValue(), 'from:e2e-phone')
    await waitRows(3)
    await waitOptions(['e2e-phone3'])
    await shot('chip-edit-backspace')
    await browser.keys(['Enter'])
    await waitRows(3)
    phase.steps.push(
      'Backspace reopened the source chip as "from:e2e-phone" (name, not id); Enter re-applies it'
    )

    assert.deepEqual(
      await browser.execute(() => window.__ucPageErrors ?? []),
      [],
      'no uncaught page errors'
    )
  }
)

console.log(`PASS components=${report.components.result} fullPage=${report.fullPage.result}`)
