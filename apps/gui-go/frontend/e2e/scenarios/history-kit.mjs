import assert from 'node:assert/strict'
import { execFileSync } from 'node:child_process'
import { writeFile } from 'node:fs/promises'
import path from 'node:path'
import { redactEvidence } from '../evidence-redaction.mjs'
import { assertBoundedGrid } from '../history-bounded-grid.mjs'

export async function runScenarios({
  daemon,
  output,
  platform,
  cdp,
  browser,
  createContext,
  preparePage,
  guiSession,
  nativePage,
}) {
  const pagingCount = Math.max(0, daemon.seeded - 8)
  const pagingEntry = index => `Paging entry ${String(index).padStart(3, '0')}`
  const results = {
    // Inside a test VM the checkout is not a git work tree there; the orchestrator passes what it built from.
    head:
      process.env.HISTORY_KIT_HEAD ??
      execFileSync('git', ['rev-parse', 'HEAD'], { encoding: 'utf8' }).trim(),
    dirty: process.env.HISTORY_KIT_HEAD
      ? process.env.HISTORY_KIT_DIRTY === '1'
      : execFileSync('git', ['status', '--porcelain'], { encoding: 'utf8' }).trim() !== '',
    platform,
    mode: cdp ? 'native-cdp' : 'browser-fixture',
    profile: daemon.profile,
    steps: [],
  }
  let page
  async function step(name, run) {
    try {
      const detail = await run()
      results.steps.push({ name, ok: true, detail })
      console.log(`PASS ${name}`)
    } catch (error) {
      results.steps.push({ name, ok: false, error: redactEvidence(String(error?.stack ?? error)) })
      console.log(`FAIL ${name}`)
      try {
        const diagnostic = await page.evaluate(() => ({
          focused: {
            tag: document.activeElement?.tagName,
            role: document.activeElement?.getAttribute('role'),
            label: document.activeElement?.getAttribute('aria-label'),
          },
          renderedRows: document.querySelectorAll('[data-item-id]').length,
          selectedRows: document.querySelectorAll('[data-item-id][aria-selected="true"]').length,
          historyWindow: {
            total: document.documentElement.dataset.historyWindowTotal,
            count: document.documentElement.dataset.historyWindowCount,
            offset: document.documentElement.dataset.historyWindowOffset,
          },
        }))
        await writeFile(
          path.join(output, `diagnostic-${name}.json`),
          redactEvidence(JSON.stringify(diagnostic, null, 2)) + '\n'
        )
      } catch (diagnosticError) {
        results.steps.at(-1).diagnosticError = redactEvidence(String(diagnosticError))
      }
      await page?.screenshot({ path: path.join(output, `failed-${name}.png`) }).catch(() => {})
      await reset().catch(() => {})
    }
  }
  // App-level prompts that are not part of History (the first-run diagnostics consent) are answered the way a user
  // would, inside the isolated profile.
  async function dismissStartupPrompts() {
    const consent = page.getByRole('alertdialog').getByRole('button', { name: 'Disable' })
    await consent.waitFor({ timeout: 5000 }).catch(() => {})
    if (await consent.isVisible()) {
      await page.screenshot({ path: path.join(output, 'startup-prompt.png') })
      await consent.click({ timeout: 5000 })
      await consent.waitFor({ state: 'hidden' })
    }
  }
  // After a failed step: close popups, clear the search and go back to All items, so later steps start clean.
  async function reset() {
    await page.keyboard.press('Escape')
    await page.keyboard.press('Escape')
    const field = page.getByRole('combobox', { name: 'Search history' })
    if (await field.inputValue()) await field.fill('')
    const clearAll = page.getByRole('button', { name: 'Clear all' })
    if (await clearAll.isVisible()) await clearAll.click()
    await page.getByRole('navigation').getByText('All items', { exact: true }).click()
    await rows().first().waitFor({ timeout: 10_000 })
  }
  async function shot(name) {
    await page.waitForTimeout(300)
    await page.screenshot({ path: path.join(output, `${name}.png`) })
  }

  // Daemon read-back with the driver's own unlocked session: what the page did must be persisted.
  let apiToken
  async function api(pathname) {
    apiToken ??= await guiSession()
    const request = () =>
      fetch(`${daemon.daemonUrl}${pathname}`, {
        headers: { Authorization: `Session ${apiToken}` },
      })
    let response = await request()
    if (response.status === 401) {
      apiToken = await guiSession()
      response = await request()
    }
    return { status: response.status, body: response.ok ? await response.json() : null }
  }
  const entry = async id => api(`/clipboard/entries/${encodeURIComponent(id)}`)
  // The entry list carries the pinned flag (the single-entry read does not).
  async function listed(id) {
    const { body } = await api('/clipboard/entries?limit=1000')
    const items = Array.isArray(body?.data) ? body.data : []
    const found = Array.isArray(items) ? items.find(item => item.id === id) : undefined
    return found
  }
  // Waits until the rendered row count settles on `expected` (search and filters are debounced and async).
  async function rowCountBecomes(expected, timeout = 10_000) {
    const end = Date.now() + timeout
    let count = await rows().count()
    while (count !== expected && Date.now() < end) {
      await page.waitForTimeout(200)
      count = await rows().count()
    }
    return count
  }
  const rows = () => page.locator('[data-item-id]')
  // Ctrl+click as Windows delivers it. Chromium on macOS turns a real Ctrl+click into a context-menu click, so the
  // browser run dispatches the pointer press itself; the native WebView2 run uses real input.
  const modifier = platform === 'macos' ? 'Meta' : 'Control'
  const ctrlClick = locator =>
    cdp
      ? locator.click({ modifiers: [modifier] })
      : locator.evaluate((el, modifier) => {
          for (const type of ['mousedown', 'mouseup', 'click'])
            el.dispatchEvent(
              new MouseEvent(type, {
                bubbles: true,
                cancelable: true,
                ctrlKey: modifier === 'Control',
                metaKey: modifier === 'Meta',
                button: 0,
              })
            )
        }, modifier)
  const row = text => rows().filter({ hasText: text }).first()
  const list = () => page.getByRole('listbox', { name: 'Clipboard history' })
  const detail = () => page.getByRole('region', { name: 'Item detail' })
  const favoriteButton = name =>
    (platform === 'windows' ? detail() : page).getByRole('button', { name, exact: true })
  const selectedIds = () =>
    page.evaluate(() =>
      [...document.querySelectorAll('[data-item-id][aria-selected="true"]')].map(
        el => el.dataset.itemId
      )
    )
  const activeDescription = () =>
    page.evaluate(() => {
      const el = document.activeElement
      return el
        ? `${el.tagName.toLowerCase()}[role=${el.getAttribute('role')}][name=${el.getAttribute('aria-label') ?? el.textContent?.trim().slice(0, 40)}]`
        : null
    })
  // The real app starts content-locked: unlock through its own unlock screen (keyring first, then the passphrase).
  async function unlockIfLocked() {
    const unlock = page.locator('[data-testid="unlock-content"]')
    // The History page's panes; the app's state shell also renders a <main> around the lock screen, so that is no signal.
    const ready = page.locator('[data-pane]')
    await Promise.race([
      unlock.waitFor({ timeout: 60_000 }),
      ready.first().waitFor({ timeout: 60_000 }),
    ])
    if (!(await unlock.isVisible())) return
    await page.screenshot({ path: path.join(output, 'native-unlock.png') })
    try {
      await unlock.click()
    } catch (error) {
      // A native reload can restore its existing session after the lock screen was observed.
      if (
        error?.name !== 'TimeoutError' ||
        (await unlock.isVisible()) ||
        !(await ready.first().isVisible())
      )
        throw error
      ;(results.nativeUnlockTransitions ??= []).push({
        state: 'content-ready-before-unlock-click',
        viewport: page.viewportSize(),
      })
    }
    const passphrase = page.locator('#unlock-passphrase')
    await Promise.race([
      passphrase.waitFor({ timeout: 30_000 }),
      ready.first().waitFor({ timeout: 30_000 }),
    ])
    if (await passphrase.isVisible()) {
      await passphrase.fill(daemon.passphrase)
      await passphrase.press('Enter')
    }
    await ready.first().waitFor({ timeout: 60_000 })
  }
  // A fresh document from the app's root: the asset server knows no client route such as /history, so reloading
  // that URL fails; the app routes to History again after it starts.
  async function reloadApp() {
    await page.goto(new URL('/', page.url()).href)
  }
  // The app also keeps its quick panel as a hidden window; the History page is the main window's document. Right after
  // launch the main window may not exist yet (or still show a transient blank page), so wait for it.
  async function mainWindowPage() {
    const end = Date.now() + 60_000
    for (;;) {
      const pages = browser.contexts().flatMap(context => context.pages())
      const main =
        nativePage && !nativePage.isClosed()
          ? nativePage
          : pages.find(candidate => {
              const url = candidate.url()
              return url.startsWith('http://wails.localhost') && !/quick-panel|updater/.test(url)
            })
      if (main) return main
      assert.ok(Date.now() < end, `no main window among ${pages.map(candidate => candidate.url())}`)
      await new Promise(resolve => setTimeout(resolve, 500))
    }
  }
  // A fresh native profile must not see layout another launch stored: the host gives a named or portable profile its
  // own WebView2 data folder (webview_data.go). Before that, every launch of the exe shared %APPDATA%\<exe name>.
  const storedLayoutKeys = [
    'uc.history.kit.view',
    'uc.history.kit.gridSize',
    'uc.library.hidden.v1',
  ]
  async function assertNoStoredLayout() {
    const found = await page.evaluate(
      keys => keys.filter(key => localStorage.getItem(key) !== null),
      storedLayoutKeys
    )
    results.storedLayoutAtStart = found
    assert.deepEqual(found, [], 'a fresh profile starts with no stored History layout')
  }
  async function newPage(options = {}) {
    if (!cdp) await closePage()
    if (cdp) {
      page ??= await mainWindowPage()
      if (!results.nativeUrl) {
        results.nativeUrl = page.url()
        page.on('pageerror', error => results.consoleErrors.push(error.message))
        await assertNoStoredLayout()
      }
      await page.emulateMedia({
        colorScheme: options.colorScheme ?? 'light',
        reducedMotion: 'reduce',
      })
      if (options.viewport) await page.setViewportSize(options.viewport)
      await reloadApp()
      await unlockIfLocked()
      await rows().first().waitFor({ timeout: 60_000 })
      await dismissStartupPrompts()
      return
    }
    const context = await createContext({
      viewport: { width: 1280, height: 800 },
      deviceScaleFactor: 2,
      reducedMotion: 'reduce',
      colorScheme: 'light',
      ...options,
    })
    page = await context.newPage()
    page.on('pageerror', error => results.consoleErrors.push(error.message))
    await preparePage(page)
    await rows().first().waitFor({ timeout: 60_000 })
    await dismissStartupPrompts()
  }
  results.consoleErrors = []
  const closePage = async () => {
    if (!cdp && page) {
      await page.context().close()
      page = undefined
    }
  }
  const geometry = () =>
    page.evaluate(() => {
      const box = selector => {
        const el = document.querySelector(selector)
        if (!el) return null
        const r = el.getBoundingClientRect()
        return {
          x: Math.round(r.x),
          y: Math.round(r.y),
          w: Math.round(r.width),
          h: Math.round(r.height),
        }
      }
      return {
        searchField: box('[data-search-field]'),
        navPane: box('[data-surface="sidebar"]'),
        listPane: box('[data-pane="list"]'),
        detailPane: box('[data-pane="detail"]'),
        row: box('[data-item-id]'),
        pageOverflowX: document.documentElement.scrollWidth - window.innerWidth,
        skin: document.querySelector('[data-ui-skin]')?.getAttribute('data-ui-skin'),
        colorScheme:
          document.querySelector('[data-ui-skin]')?.getAttribute('data-color-scheme') ??
          getComputedStyle(document.querySelector('[data-ui-skin]')).colorScheme,
        material: [...document.querySelectorAll('[data-window-material]')].map(el =>
          el.getAttribute('data-window-material')
        ),
      }
    })

  async function seededRun() {
    await newPage()
    await step('page-loads', async () => {
      await shot('default-light')
      const g = await geometry()
      assert.equal(g.skin, platform)
      assert.ok(
        g.material.every(m => m === 'solid'),
        `window material ${g.material}`
      )
      assert.ok(g.pageOverflowX <= 0, `horizontal overflow ${g.pageOverflowX}`)
      return { rows: await rows().count(), geometry: g }
    })

    await step('select-and-detail', async () => {
      await row('git rebase -i HEAD~3').click()
      await detail().getByText('git rebase -i HEAD~3').first().waitFor()
      const ids = await selectedIds()
      assert.equal(ids.length, 1)
      return { selected: ids }
    })

    await step('image-entry', async () => {
      // A real image entry (seeded through the daemon's loopback mobile LAN listener): its row shows the thumbnail
      // and the detail the decoded picture, both from daemon bytes.
      const target = rows()
        .filter({ has: page.locator('img') })
        .first()
      await target.waitFor({ timeout: 15_000 })
      const thumb = await target.locator('img').evaluate(img => ({
        complete: img.complete,
        width: img.naturalWidth,
        src: img.src.slice(0, 5),
      }))
      await target.click()
      const picture = detail().locator('img').first()
      await picture.waitFor({ timeout: 15_000 })
      await page.waitForFunction(
        el => el.complete && el.naturalWidth > 0,
        await picture.elementHandle(),
        { timeout: 15_000 }
      )
      const shown = await picture.evaluate(img => ({
        width: img.naturalWidth,
        height: img.naturalHeight,
      }))
      assert.equal(shown.width, 160, `detail image width ${shown.width}`)
      assert.ok(thumb.width > 0, 'the row thumbnail decoded')
      await shot('image-detail')
      return { thumb, shown, title: await target.getAttribute('aria-label') }
    })

    await step('keyboard-arrows', async () => {
      await list().focus()
      const before = (await selectedIds())[0]
      await page.keyboard.press('ArrowDown')
      const down = (await selectedIds())[0]
      assert.notEqual(down, before, 'ArrowDown moved the selection')
      await page.keyboard.press('ArrowUp')
      assert.equal((await selectedIds())[0], before, 'ArrowUp moved it back')
      await page.keyboard.press('Home')
      const first = await rows().first().getAttribute('data-item-id')
      assert.equal((await selectedIds())[0], first, 'Home selects the first row')
      return { focus: await activeDescription() }
    })

    await step('copy', async () => {
      await row('https://github.com/UniClipboard/UniClipboard').click()
      const restore = page.waitForResponse(r => r.url().includes('/clipboard/restore/'))
      // A failed click must not leave this wait to reject unhandled after the step has already failed.
      restore.catch(() => {})
      await detail().getByRole('button', { name: 'Copy' }).click()
      const response = await restore
      assert.ok(response.ok(), `restore ${response.status()}`)
      await detail().getByRole('button', { name: 'Copied' }).waitFor({ timeout: 5000 })
      await shot('copy-feedback')
      return { restore: response.status() }
    })

    await step('pin-persists', async () => {
      const target = row('Q4 roadmap')
      await target.click()
      const id = await target.getAttribute('data-item-id')
      const saved = page.waitForResponse(
        response =>
          new URL(response.url()).pathname === `/clipboard/entries/${id}/favorite` &&
          response.request().method() === 'POST'
      )
      saved.catch(() => {})
      await favoriteButton('Pin').click()
      await saved
      await favoriteButton('Unpin').waitFor()
      const read = await listed(id)
      const favorited = read?.isFavorited
      assert.equal(favorited, true, `daemon read-back ${JSON.stringify(read)}`)
      await page.getByRole('navigation').getByText('Pinned', { exact: true }).click()
      await row('Q4 roadmap').waitFor()
      const pinnedRows = await rowCountBecomes(1)
      assert.equal(pinnedRows, 1, 'Pinned view shows the pinned entry only')
      await shot('pinned-view')
      await page.getByRole('navigation').getByText('All items', { exact: true }).click()
      await row('Paging entry').waitFor()
      return { id, favorited, pinnedRows }
    })

    await step('search-and-delete-key-in-field', async () => {
      await row('git rebase').click()
      await page.keyboard.press(`${modifier}+f`)
      const field = page.getByRole('combobox', { name: 'Search history' })
      assert.ok(await field.evaluate(el => el === document.activeElement), 'Ctrl+F focuses search')
      await field.fill('docker')
      await row('docker compose').waitFor()
      const matches = await rowCountBecomes(1)
      assert.equal(matches, 1, `search "docker" matches ${matches}`)
      await shot('search-results')
      await page.keyboard.press('End')
      await page.keyboard.press('Delete')
      await page.keyboard.press('Backspace')
      await page.waitForTimeout(300)
      assert.equal(
        await page.getByRole('alertdialog').count(),
        0,
        'Delete in the field opens no dialog'
      )
      assert.equal(await field.inputValue(), 'docke', 'Backspace edited the text only')
      await field.fill('no-such-entry-zz')
      const empty = await rowCountBecomes(0)
      await shot('search-no-results')
      await field.fill('')
      // The focused field keeps its suggestion flyout open over the list pane; Escape closes it.
      await page.keyboard.press('Escape')
      await row('Paging entry').waitFor()
      return { matches, noResultRows: empty }
    })

    await step('filter-type', async () => {
      await page.getByRole('combobox', { name: 'Type' }).click()
      await page.getByRole('option', { name: 'Link' }).click()
      await row('https://github.com').waitFor()
      const links = await rowCountBecomes(1)
      assert.equal(links, 1, `Type=Link shows ${links}`)
      // The chosen filter becomes a token of the search field (board WB2) and the scope bar replaces the filters.
      const token = page.getByRole('button', { name: 'Remove filter: Link' })
      await token.waitFor()
      await shot('filter-link')
      await token.click()
      await row('Paging entry').waitFor()
      await page.getByRole('combobox', { name: 'Type' }).waitFor()
      return { links, token: 'Type: Link' }
    })

    await step('multi-select', async () => {
      await rows().nth(0).click()
      await rows()
        .nth(2)
        .click({ modifiers: ['Shift'] })
      let ids = await selectedIds()
      assert.equal(ids.length, 3, `shift-click range ${ids.length}`)
      await detail().getByText('3 Items Selected').waitFor()
      await ctrlClick(rows().nth(1))
      ids = await selectedIds()
      assert.equal(ids.length, 2, `ctrl-click removes ${ids.length}`)
      const checkboxes = await page.locator('[data-row-check="checked"]').count()
      let appearance
      if (platform === 'windows') {
        assert.ok(checkboxes >= 2, `multi-select check marks ${checkboxes}`)
      } else {
        // AppKit and Adwaita explicitly use filled selected rows without checkboxes.
        assert.equal(checkboxes, 0, 'this skin represents selection without checkboxes')
        appearance = await rows().evaluateAll(cards => ({
          selected: cards
            .filter(card => card.getAttribute('aria-selected') === 'true')
            .map(card => getComputedStyle(card).backgroundColor),
          unselected: getComputedStyle(cards[3]).backgroundColor,
        }))
        assert.equal(appearance.selected.length, 2)
        for (const color of appearance.selected)
          assert.notEqual(color, appearance.unselected, 'selected rows have a visible accent fill')
      }
      await shot('multi-select')
      await page.keyboard.press(`${modifier}+a`)
      await page.waitForTimeout(300)
      const all = (await selectedIds()).length
      assert.ok(all >= 10, `select all ${all}`)
      await page.keyboard.press('Escape')
      return { range: 3, afterCtrlClick: 2, selectAll: all, checkboxes, appearance }
    })

    await step('context-menu-focus-return', async () => {
      const target = row('git rebase')
      await target.click()
      await target.click({ button: 'right' })
      const menu = page.getByRole('menu')
      await menu.waitFor()
      await shot('context-menu')
      const items = await menu.getByRole('menuitem').allTextContents()
      await page.keyboard.press('Escape')
      await menu.waitFor({ state: 'hidden' })
      const focus = await activeDescription()
      assert.ok(/listbox/.test(focus ?? ''), `focus after menu: ${focus}`)
      return { items, focus }
    })

    await step('delete-confirm-cancel-restores-focus', async () => {
      const target = row('Meeting notes')
      await target.click()
      await page.keyboard.press('Delete')
      const dialog = page.getByRole('alertdialog')
      await dialog.waitFor()
      await shot('delete-confirm')
      const focusInDialog = await activeDescription()
      await page.keyboard.press('Escape')
      await dialog.waitFor({ state: 'hidden' })
      const focus = await activeDescription()
      assert.ok(/listbox/.test(focus ?? ''), `focus after cancel: ${focus}`)
      assert.equal(await row('Meeting notes').count(), 1)
      return { focusInDialog, focus }
    })

    await step('delete-one-persists', async () => {
      const target = row('Meeting notes')
      const id = await target.getAttribute('data-item-id')
      await target.click()
      await page.keyboard.press('Delete')
      await page.getByRole('alertdialog').getByRole('button', { name: 'Delete' }).click()
      await target.waitFor({ state: 'detached' })
      const focusAfterDelete = await activeDescription()
      results.focusAfterDelete = focusAfterDelete
      const read = await entry(id)
      assert.notEqual(read.status, 200, `deleted entry still readable: ${read.status}`)
      return { id, readBack: read.status }
    })

    await step('delete-batch-persists', async () => {
      const a = row(pagingEntry(pagingCount - 1))
      const b = row(pagingEntry(pagingCount - 2))
      const ids = [await a.getAttribute('data-item-id'), await b.getAttribute('data-item-id')]
      await a.click()
      await ctrlClick(b)
      await detail().getByText('2 Items Selected').waitFor()
      await page.keyboard.press('Delete')
      await page.getByRole('alertdialog').getByRole('button', { name: 'Delete' }).click()
      await page.waitForTimeout(1500)
      const reads = await Promise.all(ids.map(entry))
      results.debugBatch = {
        reads: reads.map(r => r.status),
        rendered: [await a.count(), await b.count()],
      }
      await a.waitFor({ state: 'detached' })
      await b.waitFor({ state: 'detached' })
      assert.ok(
        reads.every(r => r.status !== 200),
        `read-back ${reads.map(r => r.status)}`
      )
      return { ids, readBack: reads.map(r => r.status) }
    })

    await step('tags-add-remove-persist', async () => {
      const target = row('git rebase')
      await target.click()
      const id = await target.getAttribute('data-item-id')
      const tags = detail().getByRole('group', { name: 'Tags' })
      await tags.getByRole('button', { name: 'Add tag' }).click()
      const input = page.getByRole('combobox', { name: 'Tag name' })
      await input.fill('deploy')
      await input.press('Enter')
      await tags.getByText('#deploy').waitFor()
      await row('git rebase').getByText('#deploy').waitFor()
      await shot('tag-added')
      // Read back through a fresh load from the daemon.
      await newPage()
      await row('git rebase').click()
      await detail().getByRole('group', { name: 'Tags' }).getByText('#deploy').waitFor()
      await detail().getByRole('button', { name: 'Remove #deploy' }).click()
      await detail()
        .getByRole('group', { name: 'Tags' })
        .getByText('#deploy')
        .waitFor({ state: 'detached' })
      await newPage()
      await row('git rebase').click()
      await detail().getByRole('button', { name: 'Add tag' }).waitFor()
      const stillTagged = await detail()
        .getByRole('group', { name: 'Tags' })
        .getByText('#deploy')
        .count()
      assert.equal(stillTagged, 0, 'tag removal persisted')
      return { id, addedAndReloaded: true, removedAndReloaded: true }
    })

    await step('send-menu', async () => {
      // The loopback mobile device the image upload registers is no space member, so nothing can be sent to: the only
      // entry must be the disabled "No paired devices" (no false success).
      await row('git rebase').click()
      await detail().getByRole('button', { name: 'Send to…' }).click()
      const menu = page.getByRole('menu').first()
      await menu.waitFor()
      const items = await menu.getByRole('menuitem').allTextContents()
      assert.deepEqual(items, ['No paired devices'], `send targets ${items}`)
      assert.equal(
        await menu
          .getByRole('menuitem', { name: 'No paired devices' })
          .getAttribute('aria-disabled'),
        'true'
      )
      await shot('send-menu')
      await page.keyboard.press('Escape')
      await menu.waitFor({ state: 'hidden' })
      await page.waitForFunction(
        () => {
          const active = document.activeElement
          return /Send to/.test(active?.getAttribute('aria-label') ?? active?.textContent ?? '')
        },
        null,
        { timeout: 5000 }
      )
      const focus = await activeDescription()
      assert.ok(/Send to/.test(focus ?? ''), `focus after the send menu: ${focus}`)
      return { items, focus }
    })

    await step('select-filter-focus-return', async () => {
      const type = page.getByRole('combobox', { name: 'Type' })
      await type.click()
      await page.getByRole('option').first().waitFor()
      await page.keyboard.press('Escape')
      const focus = await activeDescription()
      assert.ok(/combobox/.test(focus ?? ''), `focus after the Type list: ${focus}`)
      return { focus }
    })

    await step('tab-order', async () => {
      await page.getByRole('combobox', { name: 'Search history' }).focus()
      const order = []
      for (let i = 0; i < 12; i++) {
        await page.keyboard.press('Tab')
        order.push(
          await page.evaluate(() => {
            const el = document.activeElement
            const region = el?.closest('[data-pane], [data-surface], nav')
            return `${region?.getAttribute('data-pane') ?? region?.getAttribute('data-surface') ?? region?.tagName.toLowerCase() ?? 'titlebar'}:${el?.getAttribute('role') ?? el?.tagName.toLowerCase()}:${el?.getAttribute('aria-label') ?? el?.textContent?.trim().slice(0, 24)}`
          })
        )
      }
      assert.ok(!order.some(entry => entry.includes(':body:')), `focus fell to <body>: ${order}`)
      const regions = order.map(entry => entry.split(':')[0])
      const firstIndex = name => regions.findIndex(r => r === name)
      // Title bar, then the navigation pane, the list pane and the detail pane, in drawn order.
      assert.ok(firstIndex('nav') > 0, `order ${order}`)
      assert.ok(firstIndex('list') > firstIndex('nav'), `order ${order}`)
      assert.ok(firstIndex('detail') > firstIndex('list'), `order ${order}`)
      // The detail pane's CommandBar is drawn at its top, so it is reached first.
      assert.match(
        order[firstIndex('detail')],
        /:Copy$/,
        `first detail stop ${order[firstIndex('detail')]}`
      )
      return { order }
    })

    await step('error-feedback-no-false-success', async () => {
      // Any rendered paging entry: a short native window renders fewer rows.
      const target = row('Paging entry')
      await target.click()
      const id = await target.getAttribute('data-item-id')
      // Injected failure (not a real daemon fault): the favorite request answers 500.
      await page.route('**/clipboard/entries/*/favorite**', route =>
        route.fulfill({
          status: 500,
          contentType: 'application/json',
          body: '{"code":"internal","message":"injected"}',
        })
      )
      try {
        await favoriteButton('Pin').click()
        await page
          .getByText('Failed to update favorite status')
          .first()
          .waitFor({ timeout: 10_000 })
        await shot('error-pin-failed')
      } finally {
        await page.unroute('**/clipboard/entries/*/favorite**')
      }
      await page.waitForTimeout(500)
      const shownPinned = await favoriteButton('Unpin').count()
      const read = await listed(id)
      assert.equal(read?.isFavorited, false, 'nothing was pinned in the daemon')
      assert.equal(shownPinned, 0, 'the page does not claim it is pinned')
      return { id, toast: 'Failed to update favorite status', daemonFavorited: read?.isFavorited }
    })

    await step('search-failure-observed', async () => {
      // Injected failure: every search query answers 500. The page must show its error state with a retry, not an
      // empty history with a count of 0.
      await page.route('**/search/query**', route =>
        route.fulfill({
          status: 500,
          contentType: 'application/json',
          body: '{"code":"internal","message":"injected"}',
        })
      )
      const field = page.getByRole('combobox', { name: 'Search history' })
      let observed
      try {
        await field.fill('docker')
        await page.waitForTimeout(2000)
        observed = {
          rows: await rows().count(),
          listPane: (await page.locator('[data-pane="list"]').innerText()).slice(0, 160),
        }
        await shot('error-search-failed')
      } finally {
        await page.unroute('**/search/query**')
      }
      await field.fill('')
      await row('Paging entry').waitFor()
      // Asserted after the page is restored, so a failure here does not leave the injected route behind.
      assert.match(observed.listPane, /History Unavailable/, 'error state shown')
      assert.match(observed.listPane, /Try Again/, 'retry offered')
      assert.ok(!/^All\n0\n/.test(observed.listPane), 'no false count of 0 in the list header')
      return observed
    })

    await step('tag-manager', async () => {
      const more = page.getByRole('button', { name: 'More' }).first()
      await more.click()
      await page.getByRole('menuitem', { name: 'Manage tags' }).click()
      const dialog = page.getByRole('dialog')
      await dialog.waitFor()
      const grid = dialog.getByRole('grid')
      await grid.waitFor()
      const dataRows = grid.getByRole('row').filter({ has: page.getByRole('gridcell') })
      const tagRows = await dataRows.count()
      await shot('tag-manager')
      // Row menu: the context menu of the first row offers Rename, Merge into (submenu) and the colours.
      await dataRows.first().click({ button: 'right' })
      const menu = page.getByRole('menu').first()
      await menu.waitFor()
      const items = await menu.getByRole('menuitem').allTextContents()
      await shot('tag-manager-menu')
      await page.keyboard.press('Escape')
      await page.keyboard.press('Escape')
      await dialog.waitFor({ state: 'hidden' })
      const focus = await activeDescription()
      return { tagRows, items, focus }
    })

    await step('grid-view', async () => {
      // A fresh document loads only the first page.
      await newPage()
      await page.getByRole('radio', { name: 'Grid' }).click()
      await row('git rebase').click()
      await shot('grid-view')
      // Arrows step from the current card. (The page keeps a current card whenever there are results, so the grid's
      // no-current-card path, where the first arrow selects card 0, cannot be reached from here.)
      const ids = await rows().evaluateAll(cards =>
        cards.map(card => card.getAttribute('data-item-id'))
      )
      const from = ids.indexOf(await row('git rebase').getAttribute('data-item-id'))
      await list().focus()
      await page.keyboard.press('ArrowRight')
      await rows()
        .nth(from + 1)
        .and(page.locator('[aria-selected="true"]'))
        .waitFor()
      await page.getByRole('radio', { name: 'List' }).click()
      await row('Paging entry').waitFor()
    })

    await step('quick-look', async () => {
      const target = rows()
        .filter({ has: page.locator('img') })
        .first()
      await target.click()
      await detail()
        .getByRole('button', { name: /Quick Look/ })
        .click()
      await page.waitForTimeout(500)
      await shot('quick-look')
      await page.keyboard.press('Escape')
      await page.waitForTimeout(300)
      return { focus: await activeDescription() }
    })

    await step('paging', async () => {
      // The grid step loaded every page; start the list from the first page again.
      await newPage()
      const before = await rows().count()
      const loaded = page.locator('[data-item-id]').filter({ hasText: 'Paging entry 000' })
      for (let i = 0; i < 40 && !(await loaded.count()); i++) {
        await list().focus()
        await page.keyboard.press('End')
        await page.waitForTimeout(300)
      }
      await loaded.first().waitFor({ timeout: 10_000 })
      return { renderedBefore: before, reachedOldest: true }
    })

    await step('compact-pane', async () => {
      const display = () =>
        page.locator('[data-sidebar-display]').getAttribute('data-sidebar-display')
      await page.getByRole('button', { name: 'Hide sidebar' }).click()
      assert.equal(await display(), 'compact')
      const rail = page
        .getByRole('navigation', { name: /./ })
        .filter({ has: page.getByRole('button', { name: 'Pinned' }) })
      await rail.getByRole('button', { name: 'Pinned' }).click()
      await row('Q4 roadmap').waitFor()
      const pinnedRows = await rowCountBecomes(1)
      await shot('compact-pane')
      // WA3: a section is one rail button whose rows open in a flyout beside the rail.
      await rail.getByRole('button', { name: 'Tags' }).click()
      const flyout = page.getByRole('menu').first()
      await flyout.waitFor()
      const tagItems = await flyout.getByRole('menuitem').allTextContents()
      await shot('compact-pane-flyout')
      await page.keyboard.press('Escape')
      // No pointer moves: rail tooltips open beside the rail, so the one under the pointer covers no other button.
      await rail.getByRole('button', { name: 'All items' }).click()
      await row('Paging entry').waitFor()
      await page.getByRole('button', { name: 'Show sidebar' }).click()
      assert.equal(await display(), 'expanded')
      return { pinnedRows, tagItems }
    })

    await step('overlay-pane', async () => {
      await closePage()
      await newPage({ viewport: { width: 1000, height: 800 } })
      const display = () =>
        page.locator('[data-sidebar-display]').getAttribute('data-sidebar-display')
      assert.equal(await display(), 'compact', 'a narrow window starts with the rail')
      await page.getByRole('button', { name: 'Show sidebar' }).click()
      assert.equal(await display(), 'overlay')
      const focusOpen = await activeDescription()
      await shot('overlay-pane')
      await page.keyboard.press('Escape')
      assert.equal(await display(), 'compact', 'Escape closes the pane')
      const focusClosed = await activeDescription()
      assert.ok(/Show sidebar/.test(focusClosed ?? ''), `focus after Escape: ${focusClosed}`)
      await page.getByRole('button', { name: 'Show sidebar' }).click()
      assert.equal(await display(), 'overlay')
      await page.mouse.click(700, 400)
      assert.equal(await display(), 'compact', 'a click outside closes the pane')
      const overflow = (await geometry()).pageOverflowX
      assert.ok(overflow <= 0, `overflow ${overflow}`)
      return { focusOpen, focusClosed }
    })

    await step('dark', async () => {
      await closePage()
      await newPage({ colorScheme: 'dark' })
      await row('git rebase').click()
      await shot('default-dark')
      const g = await geometry()
      assert.equal(g.colorScheme, 'dark')
      await row('git rebase').click({ button: 'right' })
      await page.getByRole('menu').waitFor()
      await shot('context-menu-dark')
      await page.keyboard.press('Escape')
      return { colorScheme: g.colorScheme }
    })

    await step('narrow-and-scaled', async () => {
      const sizes = [
        { name: 'narrow-1000', viewport: { width: 1000, height: 800 }, deviceScaleFactor: 2 },
        { name: 'narrow-760', viewport: { width: 760, height: 640 }, deviceScaleFactor: 2 },
        { name: 'scaled-150', viewport: { width: 853, height: 533 }, deviceScaleFactor: 1.5 },
      ]
      const out = []
      for (const size of sizes) {
        await closePage()
        await newPage({ viewport: size.viewport, deviceScaleFactor: size.deviceScaleFactor })
        await shot(size.name)
        const g = await geometry()
        assert.ok(g.pageOverflowX <= 0, `${size.name} horizontal overflow ${g.pageOverflowX}`)
        out.push({ name: size.name, geometry: g })
      }
      return out
    })

    // Last: it sets a viewport that a native (CDP) run cannot reliably undo.
    await step('grid-paging', async () => {
      await newPage({ viewport: { width: 1280, height: 12_000 }, deviceScaleFactor: 1 })
      return assertBoundedGrid({ page, rows, list, api })
    })
  }

  // First launch: an initialised space with no history (board WA1), light and dark.
  async function emptyRun() {
    for (const colorScheme of ['light', 'dark']) {
      await step(`empty-${colorScheme}`, async () => {
        if (cdp) {
          const first = !page
          if (first) {
            // A fresh profile's main window was seen to replace its page target shortly after launch in the Windows
            // VM; wait until one main-window target has stayed open for 5 s, then drive that one.
            const end = Date.now() + 90_000
            for (;;) {
              const candidate = await mainWindowPage()
              await new Promise(resolve => setTimeout(resolve, 5000))
              if (!candidate.isClosed()) {
                page = candidate
                break
              }
              results.firstLaunchTargetReplaced = (results.firstLaunchTargetReplaced ?? 0) + 1
              assert.ok(Date.now() < end, 'the main window never settled')
            }
          }
          results.nativeUrl ??= page.url()
          await page.emulateMedia({ colorScheme, reducedMotion: 'reduce' })
          // No reload: the app was just launched for this run, and in the Windows VM navigating a fresh profile's
          // document lost the page target. The dark pass switches the emulated scheme on the same document.
          if (first) {
            await unlockIfLocked()
            await assertNoStoredLayout()
          }
        } else {
          if (page) await closePage()
          const context = await createContext({
            viewport: { width: 1280, height: 800 },
            deviceScaleFactor: 2,
            reducedMotion: 'reduce',
            colorScheme,
          })
          page = await context.newPage()
          await preparePage(page)
        }
        // The real app may start a fresh profile on another page (it opened Devices in the Windows VM); record where
        // it landed and go to History the way a user would, through the sidebar.
        if (cdp) {
          await page.waitForLoadState('domcontentloaded')
          await page.waitForTimeout(3000)
          results.firstLaunchStartUrl ??= page.url()
          await shot(`first-launch-start-${colorScheme}`)
          // A first-time user must land on History and see the getting-started steps without navigating.
          assert.equal(new URL(page.url()).pathname, '/history', 'a fresh profile opens on History')
        }
        // The first-run consent alert is modal (it hides the page from the accessibility tree), so wait on text.
        await page.getByText('Nothing Copied Yet').waitFor({ timeout: 60_000 })
        await dismissStartupPrompts()
        await page.waitForTimeout(1500)
        assert.equal(await rows().count(), 0, 'no rows')
        // Board WA1: the getting-started steps in the list pane, with the pairing command.
        const steps = page.getByRole('list', { name: 'Getting started' }).getByRole('listitem')
        assert.equal(await steps.count(), 3, 'three getting-started steps')
        await page.getByRole('button', { name: 'Pair a Device…' }).first().waitFor()
        await shot(`empty-${colorScheme}`)
        return {
          geometry: await geometry(),
          detailText: (await detail().innerText()).slice(0, 200),
          listPaneText: (await page.locator('[data-pane="list"]').innerText()).slice(0, 200),
          listboxRendered: await list().count(),
        }
      })
    }
  }

  try {
    if (daemon.seeded === 0) await emptyRun()
    else await seededRun()
  } finally {
    results.pageErrors = await page?.evaluate(() => window.__ucPageErrors ?? []).catch(() => null)
    results.nativeCalls = await page?.evaluate(() => window.__ucNativeCalls ?? []).catch(() => null)
    // A run that reached no step proves nothing.
    results.passed = results.steps.length > 0 && results.steps.every(s => s.ok)
    const publicResults = JSON.stringify(
      results,
      (_, value) => {
        if (typeof value !== 'string') return value
        let safe = redactEvidence(value)
        for (const secret of [daemon.fileToken, daemon.sessionToken, daemon.passphrase])
          if (typeof secret === 'string' && secret.length)
            safe = safe.replaceAll(secret, '[redacted]')
        return safe
      },
      2
    )
    await writeFile(path.join(output, 'browser-results.json'), `${publicResults}\n`)
    console.log(
      JSON.stringify({ passed: results.passed, steps: results.steps.map(s => [s.name, s.ok]) })
    )
  }
  assert.ok(results.passed, 'browser checks failed; see browser-results.json')
}
