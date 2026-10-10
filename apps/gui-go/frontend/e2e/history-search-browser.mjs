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
      browser
        .waitUntil(
          async () => JSON.stringify(await listOptionTexts()) === JSON.stringify(expected),
          { timeout: 15_000 }
        )
        .catch(async () => {
          throw new Error(
            `options never became ${JSON.stringify(expected)}; last ${JSON.stringify(await listOptionTexts())}`
          )
        })
    const keys = text => browser.keys(text.split(''))

    await waitRows(5)
    const disable = await browser.$('//button[normalize-space()="Disable"]')
    if (await disable.isExisting()) await disable.click()
    await shot('00-history')
    phase.steps.push('complete /history page: sidebar, 5 rows, preview')

    // ── Detail column (HDetail.dc.html `item`): fixed content box for text, the
    // design's three fact cards, a text-only Send and the real C key on Copy.
    const detail = () =>
      browser.execute(() => {
        const root = document.querySelector('[data-testid="clipboard-detail"]')
        const box = root?.querySelector('.rounded-\\[0\\.875rem\\]')
        const footer = root?.querySelector('footer')
        const send = [...(footer?.querySelectorAll('button') ?? [])].find(b =>
          b.textContent.includes('Send to device')
        )
        return {
          boxHeight: box ? Math.round(box.getBoundingClientRect().height) : null,
          cards: [...(root?.querySelectorAll('.grid > div') ?? [])].map(card =>
            [...card.children].map(child => child.textContent)
          ),
          copyText: footer?.querySelector('button')?.textContent ?? null,
          sendIconVisible: send
            ? [...send.querySelectorAll('svg')].some(svg => svg.getBoundingClientRect().width > 0)
            : null,
        }
      })
    // A real pointer click: rows select on pointer events, not a synthetic click().
    const selectRow = async text => {
      for (const row of await browser.$$('[data-testid="history-row"]')) {
        if ((await row.getText()).includes(text)) return row.click()
      }
      throw new Error(`no history row containing "${text}"`)
    }
    const detailShows = text =>
      browser.execute(
        wanted =>
          document.querySelector('[data-testid="clipboard-detail"]')?.textContent.includes(wanted),
        text
      )
    const waitDetail = (label, predicate) =>
      browser.waitUntil(async () => predicate(await detail()), {
        timeout: 10_000,
        timeoutMsg: `${label}: detail never settled`,
      })

    await selectRow('meeting agenda')
    await browser.waitUntil(() => detailShows('meeting agenda: roadmap review'), {
      timeout: 10_000,
      timeoutMsg: 'detail never showed the selected text entry',
    })
    await waitDetail('text', d => d.cards.length === 3)
    const textDetail = await detail()
    phase.detailText = textDetail
    assert.equal(textDetail.boxHeight, 170, 'text content box is the design 170px')
    assert.deepEqual(
      textDetail.cards.map(([label]) => label),
      ['Copied', 'Size', 'Stored'],
      'text cards: Copied / Size / Stored'
    )
    assert.equal(textDetail.cards[1][1], '30 chars', 'size of the selected entry')
    assert.equal(textDetail.cards[2][1], 'Encrypted at rest')
    assert.equal(textDetail.copyText, 'CopyC', 'Copy carries its real C key')
    assert.equal(textDetail.sendIconVisible, false, 'Send is text-only at full width')
    await shot('A2-detail-text')
    phase.steps.push(
      'A2 detail (text): 170px box; cards Copied / Size / Stored "Encrypted at rest"; Copy C; Send text-only'
    )

    await selectRow('design-notes.md')
    await browser.waitUntil(() => detailShows('design-notes.md'), {
      timeout: 10_000,
      timeoutMsg: 'detail never showed the selected file entry',
    })
    await waitDetail('file', d => !d.cards.some(([label]) => label === 'Stored'))
    const fileDetail = await detail()
    phase.detailFile = fileDetail
    assert.ok(fileDetail.boxHeight > 170, 'file content box keeps filling the column')
    assert.deepEqual(
      fileDetail.cards.map(([label]) => label),
      ['Copied'],
      'a single file never claims encrypted storage (and has no size label)'
    )
    await shot('A2-detail-file')
    phase.steps.push('detail (single file): box fills the column; only Copied, no Stored')
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

    // ── Local tags by name: a tag made over the daemon API, then seen by name
    // (never by its opaque id) in the sidebar, the `#` candidates and the chip.
    const daemon = async (method, path, body) => {
      const response = await fetch(`${daemonUrl}${path}`, {
        method,
        headers: { Authorization: `Session ${token}`, 'Content-Type': 'application/json' },
        body: body === undefined ? undefined : JSON.stringify(body),
      })
      const json = await response.json().catch(() => null)
      assert.equal(response.status, 200, `${method} ${path}: ${JSON.stringify(json)}`)
      return json.data
    }
    // Leave History for Devices and come back: the page remounts and reloads its
    // tags (a browser refresh would drop the fixture's stubbed startup).
    const remountHistory = async () => {
      const clickSidebar = text =>
        browser.execute(wanted => {
          const target = [
            ...document.querySelectorAll(
              'aside:has(nav[aria-label="Library"]) a, aside:has(nav[aria-label="Library"]) button'
            ),
          ].find(el => el.textContent.trim().replace(/\d+$/, '') === wanted)
          if (!target) throw new Error(`no sidebar control "${wanted}"`)
          target.click()
        }, text)
      await browser.execute(() =>
        document.querySelector('aside:has(nav[aria-label="Library"]) a[href="/devices"]').click()
      )
      await browser.waitUntil(
        async () => (await browser.execute(() => location.pathname)) === '/devices',
        { timeout: 10_000, timeoutMsg: 'never reached /devices' }
      )
      await clickSidebar('All items')
      await browser.waitUntil(
        async () => (await browser.execute(() => location.pathname)) === '/history',
        { timeout: 10_000, timeoutMsg: 'never came back to /history' }
      )
      await waitRows(5)
    }
    const texts = await daemon('GET', '/search/query?query=&contentTypes=text')
    const textIds = Object.fromEntries(texts.items.map(item => [item.textPreview, item.entryId]))
    const deploy = (await daemon('POST', '/history/tags', { name: 'Deploy' })).tag.tagId
    await daemon('POST', `/history/tags/${deploy}/entries/add`, {
      entryIds: [
        textIds['meeting agenda: roadmap review'],
        textIds['release notes for version 1.2'],
      ],
    })
    await remountHistory()
    const sidebarTagRows = () =>
      browser.execute(() =>
        [
          ...document.querySelectorAll(
            'aside:has(nav[aria-label="Library"]) button, aside:has(nav[aria-label="Library"]) a'
          ),
        ]
          .map(el => el.textContent.trim())
          .filter(text => text.startsWith('#'))
      )
    await browser.waitUntil(async () => (await sidebarTagRows()).includes('#Deploy2'), {
      timeout: 10_000,
      timeoutMsg: 'sidebar never listed #Deploy with 2 items',
    })
    assert.ok(
      !(await sidebarTagRows()).some(text => text.includes(deploy)),
      'the opaque tag id never shows'
    )
    const tagInput = await browser.$('[role="combobox"][aria-label="Search and filter"]')
    await tagInput.click()
    await keys('#dep')
    await waitOptions(['#Deploy2 items'])
    await browser.keys(['Enter'])
    await waitRows(2)
    assert.equal(
      await browser.execute(
        () => document.querySelectorAll('[aria-label="Remove filter: #Deploy"]').length
      ),
      1,
      'the chip names the tag'
    )
    await shot('C0-tag-by-name')
    phase.steps.push(
      'local tag "Deploy" on 2 texts (API): sidebar "#Deploy 2"; "#dep" -> "#Deploy 2 items"; chip "#Deploy" -> 2 rows'
    )
    await browser.execute(() =>
      document.querySelector('[aria-label="Remove filter: #Deploy"]').click()
    )
    await waitRows(5)
    await daemon('DELETE', `/history/tags/${deploy}`)
    await remountHistory()
    await browser.waitUntil(async () => !(await sidebarTagRows()).includes('#Deploy2'), {
      timeout: 10_000,
      timeoutMsg: 'deleted tag stayed in the sidebar',
    })

    // ── C1: tag from the detail column, all through the UI.
    const detailChips = () =>
      browser.execute(() =>
        [...document.querySelectorAll('[data-testid="detail-tag-chip"]')].map(chip =>
          chip.textContent.trim()
        )
      )
    const editorOptions = () =>
      browser.execute(() =>
        [
          ...document.querySelectorAll(
            '[role="listbox"][aria-label="Tag suggestions"] [role="option"]'
          ),
        ].map(option => option.textContent.replace(/\s+/g, ' ').trim())
      )
    const openTagEditor = async () => {
      await browser.keys(['t'])
      await (await browser.$('input[aria-label="Tag name"]')).waitForExist({ timeout: 5_000 })
    }
    const waitSidebarTag = (text, label) =>
      browser.waitUntil(async () => (await sidebarTagRows()).includes(text), {
        timeout: 10_000,
        timeoutMsg: `sidebar never showed ${text} (${label})`,
      })

    await selectRow('meeting agenda')
    await browser.waitUntil(() => detailShows('meeting agenda: roadmap review'), {
      timeout: 10_000,
      timeoutMsg: 'detail never showed the meeting agenda entry',
    })
    await openTagEditor()
    await keys('Deploy')
    assert.deepEqual(await editorOptions(), ['Create#Deploy↵'])
    await shot('C1-create-tag')
    await browser.keys(['Enter'])
    await browser.waitUntil(async () => (await detailChips()).includes('#Deploy'), {
      timeout: 10_000,
      timeoutMsg: 'the new tag never showed on the entry',
    })
    await waitSidebarTag('#Deploy1', 'after create')
    phase.steps.push(
      'C1: T on "meeting agenda", type "Deploy", Enter -> chip #Deploy, sidebar #Deploy 1'
    )

    await selectRow('release notes')
    await browser.waitUntil(() => detailShows('release notes for version 1.2'), {
      timeout: 10_000,
      timeoutMsg: 'detail never showed the release notes entry',
    })
    await openTagEditor()
    await keys('dep')
    await browser.waitUntil(
      async () =>
        JSON.stringify(await editorOptions()) === JSON.stringify(['Create#dep↵', '#Deploy1']),
      { timeout: 5_000, timeoutMsg: 'editor never offered "Create #dep" then #Deploy' }
    )
    await shot('C1-similar-tag')
    await browser.keys(['ArrowDown', 'Enter'])
    await browser.waitUntil(async () => (await detailChips()).includes('#Deploy'), {
      timeout: 10_000,
      timeoutMsg: 'the existing tag never attached',
    })
    await waitSidebarTag('#Deploy2', 'after attaching the existing tag')
    const facetAfterAttach = await daemon('GET', '/search/tags')
    const deployTag = (await daemon('GET', '/history/tags')).find(tag => tag.name === 'Deploy')
    assert.ok(deployTag, 'Deploy exists over the API')
    assert.equal(deployTag.entryCount, 2)
    assert.ok(
      facetAfterAttach.some(tag => tag.tagId === deployTag.tagId && tag.count === 2),
      'the index agrees: 2 entries'
    )
    await shot('C1-existing-tag-attached')
    phase.steps.push(
      'C1: "dep" offers Create #dep then #Deploy 1; ↓ Enter attaches #Deploy -> sidebar 2, API entryCount 2, /search/tags 2'
    )

    await selectRow('meeting agenda')
    await browser.waitUntil(async () => (await detailChips()).includes('#Deploy'), {
      timeout: 10_000,
      timeoutMsg: 'chip missing on the first entry',
    })
    await (await browser.$('button[aria-label="Remove #Deploy"]')).click()
    await browser.waitUntil(async () => (await detailChips()).length === 0, {
      timeout: 10_000,
      timeoutMsg: 'the chip never went away',
    })
    await waitSidebarTag('#Deploy1', 'after removing')
    phase.steps.push('C1: ✕ on #Deploy -> chip gone, sidebar #Deploy 1')

    await daemon('DELETE', `/history/tags/${deployTag.tagId}`)
    await remountHistory()
    await browser.waitUntil(
      async () => !(await sidebarTagRows()).some(text => text.startsWith('#Deploy')),
      { timeout: 10_000, timeoutMsg: 'deleted tag stayed in the sidebar' }
    )

    // ── C2: tag several rows. Docker on 2 of 3, Deploy on 1 of 3 (via the API),
    // then partial tags, ⌥-click and "Tag…" through the UI.
    const docker = (await daemon('POST', '/history/tags', { name: 'Docker' })).tag.tagId
    const deploy2 = (await daemon('POST', '/history/tags', { name: 'Deploy' })).tag.tagId
    const meeting = textIds['meeting agenda: roadmap review']
    const release = textIds['release notes for version 1.2']
    await daemon('POST', `/history/tags/${docker}/entries/add`, { entryIds: [meeting, release] })
    await daemon('POST', `/history/tags/${deploy2}/entries/add`, { entryIds: [meeting] })
    await remountHistory()
    // ⌘-click checks a row (the checkboxes only appear once one is checked).
    const check = title =>
      browser.execute(label => {
        const card = [...document.querySelectorAll('[data-testid="history-card"]')].find(el =>
          el.textContent.includes(label)
        )
        if (!card) throw new Error(`no row for ${label}`)
        // The row's click target is its full-size "Open" button.
        card
          .querySelector(':scope > button')
          .dispatchEvent(new MouseEvent('click', { bubbles: true, metaKey: true }))
      }, title)
    await check('meeting agenda: roadmap review')
    await check('release notes for version 1.2')
    await check('grocery list: milk, eggs, coffee')
    const selectionChips = () =>
      browser.execute(() =>
        [...document.querySelectorAll('[data-testid="selection-tag-chip"]')].map(chip =>
          chip.textContent.trim()
        )
      )
    const waitSelectionChips = (expected, label) =>
      browser.waitUntil(
        async () => JSON.stringify(await selectionChips()) === JSON.stringify(expected),
        { timeout: 10_000, timeoutMsg: `selection chips never became ${expected} (${label})` }
      )
    await waitSelectionChips(['#Docker2/3', '#Deploy1/3'], 'initial')
    await shot('C2-selection-tags')
    phase.steps.push('C2: 3 rows checked -> detail shows #Docker 2/3, #Deploy 1/3')

    await browser.execute(() =>
      document
        .querySelector('[data-testid="selection-tag-chip"][aria-label^="Add #Deploy"]')
        .click()
    )
    await waitSelectionChips(['#Deploy3/3', '#Docker2/3'], 'after completing Deploy')
    await waitSidebarTag('#Deploy3', 'after completing Deploy')
    await browser.execute(() =>
      document
        .querySelector('[data-testid="selection-tag-chip"][aria-label^="Add #Docker"]')
        .dispatchEvent(new MouseEvent('click', { bubbles: true, altKey: true }))
    )
    await waitSelectionChips(['#Deploy3/3'], 'after ⌥-click on Docker')
    assert.equal(
      (await daemon('GET', '/history/tags')).find(tag => tag.name === 'Docker').entryCount,
      0
    )
    phase.steps.push(
      'C2: click partial #Deploy -> 3/3 (sidebar 3); ⌥-click #Docker -> gone from all (API 0)'
    )

    await browser.execute(() =>
      [...document.querySelectorAll('[role="toolbar"][aria-label="Bulk actions"] button')]
        .find(button => button.textContent.trim() === 'Tag…')
        .click()
    )
    await (await browser.$('input[aria-label="Tag name"]')).waitForExist({ timeout: 5_000 })
    await keys('Release')
    await browser.keys(['Enter'])
    await waitSelectionChips(['#Deploy3/3', '#Release3/3'], 'after Tag… Release')
    await shot('C2-tag-all')
    phase.steps.push('C2: bulk bar "Tag…" -> type "Release" Enter -> #Release 3/3')

    await browser.keys(['Escape'])
    await browser.waitUntil(
      async () =>
        (await browser.execute(
          () => document.querySelectorAll('[data-testid="selection-detail"]').length
        )) === 0,
      { timeout: 5_000, timeoutMsg: 'selection panel stayed after Escape' }
    )
    for (const tag of await daemon('GET', '/history/tags')) {
      await daemon('DELETE', `/history/tags/${tag.tagId}`)
    }
    await remountHistory()
    await browser.waitUntil(
      async () => !(await sidebarTagRows()).some(text => /^#(Deploy|Docker|Release)/.test(text)),
      { timeout: 10_000, timeoutMsg: 'tags stayed in the sidebar after cleanup' }
    )

    // ── C3: the tag manager. Deploy (meeting, release), Release (release,
    // grocery) and an unused Temp, made over the API.
    const grocery = textIds['grocery list: milk, eggs, coffee']
    const deploy3 = (await daemon('POST', '/history/tags', { name: 'Deploy' })).tag.tagId
    const release3 = (await daemon('POST', '/history/tags', { name: 'Release' })).tag.tagId
    await daemon('POST', '/history/tags', { name: 'Temp' })
    await daemon('POST', `/history/tags/${deploy3}/entries/add`, { entryIds: [meeting, release] })
    await daemon('POST', `/history/tags/${release3}/entries/add`, { entryIds: [release, grocery] })
    await remountHistory()
    await browser.execute(() => document.querySelector('button[aria-label="Manage tags"]').click())
    const managerRows = () =>
      browser.execute(() =>
        [...document.querySelectorAll('[role="dialog"] [data-testid="tag-manager-row"]')].map(row =>
          row.textContent.trim()
        )
      )
    const waitManagerRows = (expected, label) =>
      browser.waitUntil(
        async () => JSON.stringify(await managerRows()) === JSON.stringify(expected),
        { timeout: 10_000, timeoutMsg: `manager rows never became ${expected} (${label})` }
      )
    await waitManagerRows(['#Deploy2', '#Release2', '#Tempunused0'], 'opened')
    await shot('C3-tag-manager')
    phase.steps.push('C3: sidebar Manage -> Tags · 3: #Deploy 2, #Release 2, #Temp unused 0')

    const openRowMenu = name =>
      browser.execute(label => {
        document.querySelector(`button[aria-label="Actions for ${label}"]`).click()
      }, name)
    const pickMenuItem = async text => {
      await browser.waitUntil(
        () =>
          browser.execute(
            wanted =>
              [...document.querySelectorAll('[role="menuitem"]')].some(
                item => item.textContent.trim() === wanted
              ),
            text
          ),
        { timeout: 5_000, timeoutMsg: `menu item "${text}" never appeared` }
      )
      await browser.execute(wanted => {
        ;[...document.querySelectorAll('[role="menuitem"]')]
          .find(item => item.textContent.trim() === wanted)
          .click()
      }, text)
    }
    const renameTo = async (name, next) => {
      await openRowMenu(name)
      await pickMenuItem('Rename…')
      const field = await browser.$(`input[aria-label="Rename ${name}"]`)
      await field.waitForExist({ timeout: 5_000 })
      await field.click()
      await browser.keys(['Meta', 'a'])
      await browser.keys(['Meta'])
      await keys(next)
      await browser.keys(['Enter'])
    }

    // Renaming to a taken name (any case) offers the merge instead.
    await renameTo('#Release', 'deploy')
    const mergeOffer = await browser.$('//button[normalize-space()="Merge into #Deploy"]')
    await mergeOffer.waitForExist({ timeout: 10_000 })
    assert.match(await browser.$('[role="dialog"]').getText(), /#Deploy already exists\./)
    await shot('C3-rename-conflict')
    await mergeOffer.click()
    await waitManagerRows(['#Deploy3', '#Tempunused0'], 'after merging Release into Deploy')
    const afterMerge = await daemon('GET', '/history/tags')
    assert.deepEqual(
      afterMerge.map(tag => [tag.name, tag.entryCount]),
      [
        ['Deploy', 3],
        ['Temp', 0],
      ]
    )
    phase.steps.push(
      'C3: rename #Release -> "deploy" says #Deploy already exists; "Merge into #Deploy" -> Deploy 3 (API agrees)'
    )

    await renameTo('#Temp', 'Scratch')
    await waitManagerRows(['#Deploy3', '#Scratchunused0'], 'after renaming Temp')
    await openRowMenu('#Scratch')
    await pickMenuItem('Delete tag (items stay)')
    await waitManagerRows(['#Deploy3'], 'after deleting Scratch')
    phase.steps.push('C3: rename #Temp -> Scratch; delete #Scratch (items stay)')

    await browser.execute(() =>
      [...document.querySelectorAll('[role="dialog"] button')]
        .find(button => button.textContent.trim() === '+ New tag')
        .click()
    )
    await (await browser.$('input[aria-label="New tag name"]')).waitForExist({ timeout: 5_000 })
    await keys('Inbox')
    await browser.keys(['Enter'])
    await waitManagerRows(['#Deploy3', '#Inboxunused0'], 'after + New tag')
    await openRowMenu('#Inbox')
    await pickMenuItem('Delete tag (items stay)')
    await waitManagerRows(['#Deploy3'], 'after deleting Inbox')
    phase.steps.push('C3: + New tag "Inbox" -> unused row; deleted again')

    await openRowMenu('#Deploy')
    await pickMenuItem('Show items')
    await browser.waitUntil(
      async () =>
        (await browser.execute(() => document.querySelectorAll('[role="dialog"]').length)) === 0,
      { timeout: 5_000, timeoutMsg: 'manager stayed open after Show items' }
    )
    await waitRows(3)
    await shot('C3-show-items')
    phase.steps.push('C3: Show items on #Deploy -> manager closes, list shows its 3 entries')

    // Picking the active tag row again clears the filter.
    await browser.execute(() =>
      [
        ...document.querySelectorAll(
          'aside:has(nav[aria-label="Library"]) button, aside:has(nav[aria-label="Library"]) a'
        ),
      ]
        .find(el => el.textContent.trim() === '#Deploy3')
        .click()
    )
    await waitRows(5)
    await daemon('DELETE', `/history/tags/${deploy3}`)
    await remountHistory()
    await browser.waitUntil(
      async () =>
        !(await sidebarTagRows()).some(text => /^#(Deploy|Release|Temp|Scratch)/.test(text)),
      { timeout: 10_000, timeoutMsg: 'tags stayed in the sidebar after the manager steps' }
    )

    // The search field sits at the top of the list column (no toolbar trigger).
    const input = await browser.$('[role="combobox"][aria-label="Search and filter"]')
    await input.waitForExist({ timeout: 10_000 })
    await input.click()
    await keys('@e2e')
    await waitOptions(['e2e-phone3 items'])
    await browser.keys(['Enter'])
    await waitRows(3)
    await keys('/')
    // Two-part counts (b069bcf79): all items, then those matching the other chips.
    await waitOptions([
      'Text3 items · 3 from e2e-phone',
      'Rich Text0 items · none from e2e-phone',
      'Image0 items · none from e2e-phone',
      'File2 items · none from e2e-phone',
    ])
    await shot('B1-typeahead-with-from-chip')
    phase.steps.push(
      'B1: @e2e-phone chip + "/" -> Text 3·3 / Rich Text 0·none / Image 0·none / File 2·none from e2e-phone'
    )

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
    await waitOptions(['.md1 item · none type Text · from e2e-phone'])
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
    await waitOptions(['e2e-phone3 items · 3 type Text'])
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
              // Library rows end with their item count ("Pinned0").
              el.textContent.trim().replace(/\d+$/, '') === wanted
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

    // Devices' "Manage" link (Tags has a "Manage" button of its own).
    await browser.execute(() =>
      document.querySelector('aside:has(nav[aria-label="Library"]) a[href="/devices"]').click()
    )
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
              ?.textContent.trim()
              .replace(/\d+$/, '') === 'Pinned'
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
              ?.textContent.trim()
              .replace(/\d+$/, '') === 'All items'
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
