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

const report = {
  components: { steps: [] },
  fullPage: { steps: [] },
  fullPageWindows: { steps: [] },
  fullPageLinuxSystemFrame: { steps: [] },
}
const writeReport = () =>
  writeFile(path.join(output, 'browser-result.json'), JSON.stringify(report, null, 2))

async function runPhase(name, entry, port, size, body, extraQuery = '') {
  const server = await serveFixture(entry, port)
  const browser = await openBrowser(size.width, size.height)
  // Match the design artboards' 1280×800 so screenshots compare at the same size.
  await browser.setViewport({ width: size.width, height: size.height })
  // The theme follows the system by default; pin light so runs compare with the
  // (light) design artboards regardless of the host's appearance.
  await browser.emulate('colorScheme', 'light')
  const phase = report[name]
  const shot = async file => browser.saveScreenshot(path.join(output, `${name}-${file}.png`))
  try {
    await browser.url(`http://127.0.0.1:${port}/${query}${extraQuery}`)
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

    await type('/')
    await waitOptions(['Text3', 'Rich Text0', 'Image0', 'File2'])
    await shot('01-type-candidate-counts')
    phase.steps.push('/ candidates Text 3 / Rich Text 0 / Image 0 / File 2')

    await browser.keys(['Backspace'])
    await type('/text')
    await browser.keys(['Enter'])
    await waitTotal(3)
    await type('ext:md')
    await waitOptions(['.md0'])
    await shot('02-ext-candidate-count-with-text-chip')
    await browser.keys(['Enter'])
    await waitTotal(0)
    phase.steps.push('chips /text + ext:md -> 0 entries (.md candidate showed 0)')

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
// B1 type-ahead with an `@` chip, B2 filtered results, B3 no results with
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
    // The list-column panel shows "<value><n items>" and marks the highlighted row with ↵.
    const listOptionTexts = async () =>
      (await optionTexts(browser)).map(text => text.replace('↵', ''))
    const waitOptions = expected =>
      browser.waitUntil(
        async () => JSON.stringify(await listOptionTexts()) === JSON.stringify(expected),
        { timeout: 15_000, timeoutMsg: `options never became ${JSON.stringify(expected)}` }
      )
    const keys = text => browser.keys(text.split(''))

    await waitRows(5)
    const disable = await browser.$('//button[normalize-space()="Disable"]')
    if (await disable.isExisting()) await disable.click()
    await shot('00-history')
    phase.steps.push('complete /history page: sidebar, 5 rows, preview')
    // Column geometry, compared against the design's 220 | 560 | 500 at 1280×800.
    phase.metrics = await browser.execute(() => {
      const width = el => (el ? Math.round(el.getBoundingClientRect().width) : null)
      return {
        rootFontSize: getComputedStyle(document.documentElement).fontSize,
        sidebar: width(document.querySelector('aside:has(nav[aria-label="Library"])')),
        list: width(document.querySelector('[data-panel-id="history-list"], #history-list')),
        preview: width(
          document.querySelector('[data-panel-id="history-preview"], #history-preview')
        ),
        groupWidth: width(document.querySelector('#history-list')?.parentElement),
      }
    })

    // The search field sits at the top of the list column (no toolbar trigger).
    const input = await browser.$('[role="combobox"][aria-label="Search and filter"]')
    await input.waitForExist({ timeout: 10_000 })
    await input.click()
    await keys('@e2e')
    await waitOptions(['e2e-phone3 items'])
    await browser.keys(['Enter'])
    await waitRows(3)
    await keys('/')
    await waitOptions(['Text3 items', 'Rich Text0 items', 'Image0 items', 'File0 items'])
    await shot('B1-typeahead-with-from-chip')
    phase.steps.push('B1: @e2e-phone chip + "/" -> Text 3 / Rich Text 0 / Image 0 / File 0')

    await browser.keys(['Enter'])
    await waitRows(3)
    await keys('agenda')
    await browser.keys(['Enter'])
    await waitRows(1)
    await shot('B2-filtered-results')
    phase.steps.push('B2: chips @e2e-phone + Text, keyword "agenda" -> 1 row')

    await browser.keys(Array(6).fill('Backspace'))
    await waitRows(3)
    await keys('ext:md')
    await waitOptions(['.md0 items'])
    await browser.keys(['Enter'])
    await waitRows(0)
    const relaxations = () =>
      browser.execute(() =>
        [...document.querySelectorAll('button')]
          .filter(b => /^Without .*\d+ items?$/.test(b.textContent ?? ''))
          .map(b => [b.getAttribute('aria-label'), b.textContent, b.disabled])
      )
    await browser.waitUntil(async () => (await relaxations()).length === 3, {
      timeout: 15_000,
      timeoutMsg: 'relaxations never appeared on the page',
    })
    assert.deepEqual(await relaxations(), [
      ['Remove filter: Text', 'Without Text0 items', true],
      ['Remove filter: e2e-phone', 'Without e2e-phone0 items', true],
      ['Remove filter: .md', 'Without .md3 items', false],
    ])
    assert.match(await browser.$('body').getText(), /No items match all 3 filters/)
    // The design shows B3 with suggestions closed.
    await browser.keys(['Escape'])
    await browser.waitUntil(async () => (await browser.$$('[role="option"]')).length === 0, {
      timeout: 5_000,
      timeoutMsg: 'suggestions never closed',
    })
    await shot('B3-no-results-relaxations')
    phase.steps.push(
      'B3: 3 chips -> 0 rows; drop Text 0, drop e2e-phone 0 (both disabled), drop .md 3'
    )

    // The chip's own X shares the aria-label; the relaxation is the one with a count.
    await browser.execute(() =>
      [...document.querySelectorAll('button')]
        .find(b => b.textContent === 'Without .md3 items')
        .click()
    )
    await waitRows(3)
    await shot('B3-after-dropping-md')
    phase.steps.push('clicking the .md relaxation -> 3 rows')

    // Chips are ordered type -> source -> extension, so the last one is the source.
    await input.click()
    await browser.keys(['Backspace'])
    assert.equal(await input.getValue(), '@e2e-phone')
    await waitRows(3)
    await waitOptions(['e2e-phone3 items'])
    await shot('chip-edit-backspace')
    await browser.keys(['Enter'])
    await waitRows(3)
    phase.steps.push(
      'Backspace reopened the source chip as "@e2e-phone" (name, not id); Enter re-applies it'
    )

    // ── Shell navigation: History <-> Devices <-> Settings via the sidebar.
    const path = () => browser.execute(() => location.pathname)
    const waitPath = expected =>
      browser.waitUntil(async () => (await path()) === expected, {
        timeout: 10_000,
        timeoutMsg: `never navigated to ${expected}`,
      })
    const clickText = (text, { inSidebar }) =>
      browser.execute(
        (wanted, sidebar) => {
          const target = [...document.querySelectorAll('a, button')].find(
            el =>
              Boolean(el.closest('aside:has(nav[aria-label="Library"])')) === sidebar &&
              el.textContent.trim() === wanted
          )
          if (!target) throw new Error(`no ${sidebar ? 'sidebar' : 'page'} control "${wanted}"`)
          target.click()
        },
        text,
        inSidebar
      )
    const pageText = () => browser.execute(() => document.body.innerText)
    assert.equal(
      await browser.execute(() => document.querySelectorAll('[aria-label="Library"]').length),
      1,
      'one Library sidebar'
    )
    assert.equal(
      await browser.execute(() => document.querySelectorAll('aside.w-12').length),
      0,
      'no icon rail on macOS'
    )

    await clickText('Manage', { inSidebar: true })
    await waitPath('/devices')
    await browser.waitUntil(async () => (await pageText()).includes('1 of 1 online'), {
      timeout: 10_000,
      timeoutMsg: 'Devices sidebar never showed "1 of 1 online"',
    })
    await shot('nav-01-devices')
    phase.steps.push('sidebar "Manage" -> /devices; shared sidebar shows DEVICES "1 of 1 online"')

    // Device management stays in the main content: list + detail.
    const detailText = () =>
      browser.execute(() => document.querySelector('main main')?.innerText ?? '')
    await browser.execute(() => {
      const row = [...document.querySelectorAll('button, [role="button"], a')].find(
        el => !el.closest('nav[aria-label="Library"]') && el.textContent.includes('e2e-phone')
      )
      row.click()
    })
    await browser.waitUntil(async () => (await detailText()).includes('e2e-phone'), {
      timeout: 10_000,
      timeoutMsg: 'mobile device detail never opened',
    })
    await shot('nav-02-devices-mobile-detail')
    await browser.execute(() =>
      [...document.querySelectorAll('button')].find(b => b.title === 'Join another space').click()
    )
    const dialog = await browser.$('[role="dialog"]')
    await dialog.waitForDisplayed({ timeout: 10_000 })
    await shot('nav-03-devices-join-space-dialog')
    await browser.keys(['Escape'])
    await dialog.waitForDisplayed({ reverse: true, timeout: 10_000 })
    phase.steps.push(
      'device list in main content: selecting e2e-phone opens its detail; "Join another space" dialog opens and cancels'
    )

    await clickText('Pinned', { inSidebar: true })
    await waitPath('/history')
    await browser.waitUntil(
      () =>
        browser.execute(
          () =>
            document
              .querySelector('nav[aria-label="Library"] [aria-current="true"]')
              ?.textContent.trim() === 'Pinned'
        ),
      { timeout: 10_000, timeoutMsg: 'Pinned never became the active Library row' }
    )
    await waitRows(0)
    await shot('nav-04-history-pinned-from-devices')
    phase.steps.push('Devices sidebar "Pinned" -> /history with Pinned active (0 pinned rows)')

    await clickText('All items', { inSidebar: true })
    // Let the filter settle before leaving: the page snapshots its state on unmount.
    await browser.waitUntil(
      () =>
        browser.execute(
          () =>
            document
              .querySelector('nav[aria-label="Library"] [aria-current="true"]')
              ?.textContent.trim() === 'All items'
        ),
      { timeout: 10_000, timeoutMsg: 'All items never became the active Library row' }
    )
    await waitRows(3)
    await clickText('Settings', { inSidebar: true })
    await waitPath('/settings')
    await shot('nav-05-settings')
    await clickText('Back', { inSidebar: false })
    await waitPath('/history')
    await waitRows(3)
    phase.steps.push('sidebar "Settings" -> /settings; Back -> /history')

    assert.deepEqual(
      await browser.execute(() => window.__ucPageErrors ?? []),
      [],
      'no uncaught page errors'
    )
  }
)

// ── C. Windows / Linux branches of the same complete page ────────────────────
// Browser DOM checks of the platform branch only (navigator overridden by the
// fixture) — not a native Windows/Linux acceptance. These platforms keep the
// pre-slice-13 shell: icon rail navigation, toolbar search overlay, window
// controls; the Devices page has no second (Library) navigation.
async function railBranch(browser, phase, shot, { platform, windowControls }) {
  const rowCount = () =>
    browser.execute(() => document.querySelectorAll('[data-testid="history-row"]').length)
  await browser.waitUntil(async () => (await rowCount()) === 5, {
    timeout: 20_000,
    timeoutMsg: 'history list never showed 5 rows',
  })
  const disable = await browser.$('//button[normalize-space()="Disable"]')
  if (await disable.isExisting()) await disable.click()
  const path = () => browser.execute(() => location.pathname)
  const waitPath = expected =>
    browser.waitUntil(async () => (await path()) === expected, {
      timeout: 10_000,
      timeoutMsg: `never navigated to ${expected}`,
    })
  const dom = () =>
    browser.execute(() => ({
      platform: document.documentElement.dataset.ucPlatform,
      rail: document.querySelectorAll('aside.w-12').length,
      railLinks: [...document.querySelectorAll('aside.w-12 a')].map(a =>
        a.getAttribute('aria-label')
      ),
      library: document.querySelectorAll('nav[aria-label="Library"]').length,
      dragStripInLibrary: document.querySelectorAll(
        'aside:has(nav[aria-label="Library"]) > [data-tauri-drag-region]'
      ).length,
      manageLink: [...document.querySelectorAll('a')].some(a => a.textContent.trim() === 'Manage'),
      inlineSearch: document.querySelectorAll('[role="combobox"]').length,
      toolbarTrigger: document.querySelectorAll('button[aria-label="Search and filter"]').length,
      windowControls: ['最小化', '最大化', '关闭'].filter(label =>
        document.querySelector(`button[aria-label="${label}"]`)
      ),
    }))

  const history = await dom()
  assert.equal(history.platform, platform)
  assert.equal(history.rail, 1, 'icon rail present')
  assert.deepEqual(history.railLinks, ['History', 'Devices', 'Settings'])
  assert.equal(history.library, 1, 'History keeps its pre-slice Library panel')
  assert.equal(history.dragStripInLibrary, 0, 'no macOS traffic-light strip')
  assert.equal(history.manageLink, false, 'no second Devices entry in the Library panel')
  assert.equal(history.inlineSearch, 0, 'no list-column search')
  assert.equal(history.toolbarTrigger, 1, 'toolbar search trigger')
  assert.deepEqual(history.windowControls, windowControls)
  await shot('00-history')
  phase.steps.push(
    `${platform}: icon rail [History, Devices, Settings]; Library panel without strip/Manage; toolbar search trigger; window controls ${JSON.stringify(windowControls)}`
  )

  await (await browser.$('button[aria-label="Search and filter"]')).click()
  const input = await browser.$('[data-testid="history-search-surface"] [role="combobox"]')
  await input.waitForExist({ timeout: 10_000 })
  await browser.keys(['/'])
  await browser.waitUntil(
    async () =>
      JSON.stringify(await optionTexts(browser)) ===
      JSON.stringify(['Text3', 'Rich Text0', 'Image0', 'File2']),
    { timeout: 15_000, timeoutMsg: 'toolbar search candidates never showed counts' }
  )
  await shot('01-toolbar-search-counts')
  await browser.keys(['Enter'])
  await browser.waitUntil(async () => (await rowCount()) === 3, {
    timeout: 15_000,
    timeoutMsg: 'Text filter from the toolbar search never applied',
  })
  await browser.keys(['Escape'])
  phase.steps.push(
    'toolbar search overlay: / candidates Text 3 / Rich Text 0 / Image 0 / File 2; Enter applies Text'
  )

  await (await browser.$('aside.w-12 a[aria-label="Devices"]')).click()
  await waitPath('/devices')
  await (await browser.$('[data-testid="devices-add-device"]')).waitForExist({ timeout: 10_000 })
  const devices = await dom()
  assert.equal(devices.rail, 1)
  assert.equal(devices.library, 0, 'Devices has no second navigation on this platform')
  await shot('02-devices')
  phase.steps.push('rail Devices -> /devices: device list + detail, no Library sidebar')

  await (await browser.$('aside.w-12 a[aria-label="Settings"]')).click()
  await waitPath('/settings')
  await shot('03-settings')
  await browser.execute(() =>
    [...document.querySelectorAll('a, button')].find(el => el.textContent.trim() === 'Back').click()
  )
  await waitPath('/devices')
  await (await browser.$('aside.w-12 a[aria-label="History"]')).click()
  await waitPath('/history')
  phase.steps.push('rail Settings -> /settings; Back -> /devices; rail History -> /history')

  assert.deepEqual(
    await browser.execute(() => window.__ucPageErrors ?? []),
    [],
    'no uncaught page errors'
  )
}

await runPhase(
  'fullPageWindows',
  'e2e/fixtures/history-full-app.tsx',
  1465,
  { width: 1280, height: 800 },
  (browser, phase, shot) =>
    railBranch(browser, phase, shot, {
      platform: 'windows',
      windowControls: ['最小化', '最大化', '关闭'],
    }),
  '&platform=windows'
)

await runPhase(
  'fullPageLinuxSystemFrame',
  'e2e/fixtures/history-full-app.tsx',
  1467,
  { width: 1280, height: 800 },
  (browser, phase, shot) =>
    railBranch(browser, phase, shot, { platform: 'linux', windowControls: [] }),
  '&platform=linux&frame=system'
)

console.log(
  `PASS ${Object.entries(report)
    .map(([name, phase]) => `${name}=${phase.result}`)
    .join(' ')}`
)
