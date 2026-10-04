import { execFileSync } from 'node:child_process'
import { existsSync, mkdirSync, writeFileSync } from 'node:fs'
import { createServer } from 'node:net'
import os from 'node:os'
import path from 'node:path'
import { fileURLToPath } from 'node:url'
import { $, $$, browser, expect } from '@wdio/globals'
import { daemonRequest, readDaemonConnection } from '../helpers/dualPeer.js'
import { waitForSetup } from '../helpers/waitForSetup.js'

// Native macOS evidence for the three-column shell (history-window-exec-plan
// slice 13), on the real Tauri app of an isolated profile. Captures only this
// app's own window (`screencapture -l`), never the screen. Run with
// UC_DISABLE_SYSTEM_CLIPBOARD=1 and UC_GPUI_QUICK_PANEL=0 so neither the
// user's clipboard nor a global shortcut is touched.
const here = path.dirname(fileURLToPath(import.meta.url))
const output = process.env.E2E_ARTIFACT_DIR || '/tmp/uc-history-shell-native'
mkdirSync(output, { recursive: true })
const report = { steps: [], webviewShots: [], nativeShots: [], dragProbe: null, columns: [] }
const saveReport = () =>
  writeFileSync(path.join(output, 'native-result.json'), JSON.stringify(report, null, 2))

// Native window capture is evidence, not an assertion: when the window is not
// capturable (another Space, no Screen Recording grant), record why and carry on
// with the WebView shot.
async function nativeShot(name) {
  await webviewShot(name)
  try {
    const pid = await browser.tauri.execute(({ core }) => core.invoke('get_tauri_pid'))
    const id = execFileSync(
      'swift',
      [path.join(here, '../helpers/native-window-id.swift'), String(pid)],
      { stdio: ['ignore', 'pipe', 'pipe'] }
    )
      .toString()
      .trim()
    const file = path.join(output, `native-${name}.png`)
    execFileSync('screencapture', ['-x', '-o', '-l', id, file])
    report.nativeShots.push(path.basename(file))
  } catch (error) {
    const reason = String(error.stderr ?? error.message).trim()
    report.nativeShotSkipped = report.nativeShotSkipped ?? []
    report.nativeShotSkipped.push(`${name}: ${reason}`)
  }
}

async function webviewShot(name) {
  const file = path.join(output, `webview-${name}.png`)
  await browser.saveScreenshot(file)
  report.webviewShots.push(path.basename(file))
}

const pathname = () => browser.execute(() => location.pathname)
async function waitPath(expected) {
  await browser.waitUntil(async () => (await pathname()) === expected, {
    timeout: 15000,
    timeoutMsg: `never navigated to ${expected}`,
  })
}
async function clickSidebar(text) {
  await browser.execute(wanted => {
    const target = [...document.querySelectorAll('aside a, aside button')].find(
      el =>
        el.closest('aside')?.querySelector('nav[aria-label]') && el.textContent.trim() === wanted
    )
    if (!target) throw new Error(`no sidebar control "${wanted}"`)
    target.click()
  }, text)
}

const profile = process.env.E2E_UC_PROFILE
const repoRoot = path.resolve(here, '../../../..')
const cli = process.env.UC_E2E_DEV_CLI ?? path.join(repoRoot, 'target/debug/uniclip')

function freePort() {
  return new Promise((resolve, reject) => {
    const server = createServer()
    server.once('error', reject)
    server.listen(0, '127.0.0.1', () => {
      const { port } = server.address()
      server.close(() => resolve(port))
    })
  })
}

// Text enters through a real ingestion path: the mobile LAN endpoint, as the
// Rust history-search e2e does. Dispatch without paired peers records nothing.
async function ingestText(text) {
  const port = await freePort()
  const setup = JSON.parse(
    execFileSync(
      cli,
      [
        '--json',
        'mobile',
        'setup',
        '--non-interactive',
        '--label',
        'e2e-phone',
        '--ip',
        '127.0.0.1',
        '--accept-network-risk',
        '--port',
        String(port),
      ],
      { env: { ...process.env, UC_PROFILE: profile, UNICLIPBOARD_ENV: 'development' } }
    )
      .toString()
      .trim()
  )
  const url = `http://127.0.0.1:${port}/SyncClipboard.json`
  const auth = `Basic ${Buffer.from(`${setup.username}:${setup.password}`).toString('base64')}`
  await browser.waitUntil(
    async () => {
      try {
        return (await fetch(url, { headers: { Authorization: auth } })).ok
      } catch {
        return false
      }
    },
    { timeout: 30000, timeoutMsg: 'mobile LAN listener never came up' }
  )
  const put = await fetch(url, {
    method: 'PUT',
    headers: { Authorization: auth, 'Content-Type': 'application/json' },
    body: JSON.stringify({ type: 'Text', text, hasData: false }),
  })
  expect(put.ok).toBe(true)
}

// Layout, virtualization and entrance animations only run while the WebView is
// visible: WebKit pauses frames for a window on another Space. Bring the window
// forward and wait until the page reports itself visible.
async function bringWindowForward() {
  await browser.execute(async () => {
    const label = window.__TAURI_INTERNALS__.metadata.currentWindow.label
    await window.__TAURI_INTERNALS__.invoke('plugin:window|show', { label })
    await window.__TAURI_INTERNALS__.invoke('plugin:window|set_focus', { label })
  })
  try {
    await browser.waitUntil(
      async () => (await browser.execute(() => document.visibilityState)) === 'visible',
      { timeout: 15000 }
    )
    return true
  } catch {
    return false
  }
}

// A window the host keeps on another Space is an environment precondition, not
// a layout regression: skip with the reason instead of failing.
const NOT_VISIBLE =
  'the app window could not be kept visible (another desktop Space?); keep it on the current Space to run the width checks'

// Resize the native window to a logical inner width. The embedded WebDriver
// takes physical outer sizes, so scale by the pixel ratio and correct once for
// window chrome.
async function setInnerWidth(width) {
  const ratio = await browser.execute(() => window.devicePixelRatio)
  for (let attempt = 0; attempt < 2; attempt += 1) {
    const inner = await browser.execute(() => window.innerWidth)
    if (inner === width) break
    const rect = await browser.getWindowRect()
    await browser.setWindowRect(
      null,
      null,
      Math.round(rect.width + (width - inner) * ratio),
      Math.round(800 * ratio)
    )
    await browser.waitUntil(
      async () => (await browser.execute(() => window.innerWidth)) !== inner,
      { timeout: 5000, timeoutMsg: `window never resized toward ${width}px` }
    )
  }
  // Let the panel group settle after the resize observer fires.
  await browser.pause(300)
  return browser.execute(() => window.innerWidth)
}

// Column geometry and horizontal overflow of the three-column History page.
function measureColumns() {
  const width = el => Math.round(el?.getBoundingClientRect().width ?? 0)
  const aside = document.querySelector('aside')
  const overflowing = [
    ...document.querySelectorAll(
      '[data-testid="clipboard-detail"], [data-testid="clipboard-detail"] > *, [data-panel] [role="combobox"]'
    ),
  ]
    .filter(el => el.scrollWidth > el.clientWidth + 1)
    .map(
      el =>
        el.tagName.toLowerCase() + (el.className ? '.' + String(el.className).split(' ')[0] : '')
    )
  return {
    window: window.innerWidth,
    sidebar: width(aside),
    rail: aside?.dataset.sidebar === 'rail',
    list: width(document.getElementById('history-list')),
    detail: width(document.getElementById('history-preview')),
    detailFooter: Boolean(document.querySelector('[data-testid="clipboard-detail"] footer')),
    overflowing,
  }
}

// An unbundled Tauri binary has no bundle id, so WKWebView keys its default
// data store by process name: ~/Library/WebKit/<executable name>. Run a renamed
// copy so the WebView storage cannot be shared with another `uniclipboard`.
const executableName = path.basename(process.env.E2E_TAURI_APP ?? 'uniclipboard')
const webkitStore = path.join(os.homedir(), 'Library/WebKit', executableName)

describe('macOS three-column shell (native)', () => {
  before(() => {
    if (executableName === 'uniclipboard') {
      throw new Error(
        'refusing to run: set E2E_TAURI_APP to a renamed copy (shared WebKit store otherwise)'
      )
    }
  })
  after(saveReport)
  afterEach(async function () {
    if (this.currentTest?.state !== 'failed') return
    const name = this.currentTest.title.replace(/[^a-z0-9]+/gi, '-').toLowerCase()
    await webviewShot(`failed-${name}`).catch(() => {})
    // Enough page state to tell a hidden or throttled window from a broken page.
    report.failureState = await browser
      .execute(() => ({
        visibility: document.visibilityState,
        hasFocus: document.hasFocus(),
        rows: document.querySelectorAll('[data-testid="history-row"]').length,
        virtuosoItems: document.querySelectorAll('[data-item-index]').length,
        previewOpacity: getComputedStyle(
          document.querySelector('[data-testid="history-preview-motion"]') ?? document.body
        ).opacity,
        innerWidth: window.innerWidth,
      }))
      .catch(error => String(error))
    report.steps.push(`FAILED at ${await pathname().catch(() => '?')}: ${this.currentTest.title}`)
  })

  it('creates a space in the isolated profile', async () => {
    const createEntry = await waitForSetup({ timeout: 60000 })
    await browser.execute(button => button.click(), createEntry)
    await (await $('#device-name')).setValue('shell-native')
    await (await $('#pass1')).setValue('shell-native-passphrase')
    await (await $('#pass2')).setValue('shell-native-passphrase')
    await browser.execute(
      button => button.click(),
      await $('[data-testid="setup-initialize-submit"]')
    )
    await browser.waitUntil(
      async () =>
        (await $('[data-testid="setup-complete-done"]').isExisting()) ||
        (await $('[data-testid="setup-complete-later"]').isExisting()) ||
        (await $('nav[aria-label]').isExisting()),
      { timeout: 90000, timeoutMsg: 'setup never completed' }
    )
    // A space creator is offered "connect now / later"; a joiner gets "done".
    for (const id of ['setup-complete-later', 'setup-complete-done']) {
      const button = await $(`[data-testid="${id}"]`)
      if (await button.isExisting()) await browser.execute(el => el.click(), button)
    }
    await (await $('aside nav[aria-label]')).waitForExist({ timeout: 30000 })
    await waitPath('/history')
    report.steps.push('setup completed in the real app; landed on /history')

    expect(existsSync(path.join(webkitStore, 'WebsiteData'))).toBe(true)
    report.webkitStore = webkitStore
    report.steps.push(`WebView data store is the isolated ${webkitStore}`)

    // Pin English and the light scheme (the design's), whatever the host's
    // locale and appearance, so text selectors and screenshots are stable.
    const connection = readDaemonConnection(profile)
    expect(connection).not.toBeNull()
    const saved = await daemonRequest(connection, '/settings', {
      method: 'PUT',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ general: { language: 'en-US', theme: 'light' } }),
    })
    expect(saved.status).toBe(200)
    await browser.execute(() => localStorage.setItem('uniclipboard.language', 'en-US'))
    await browser.refresh()
    await (await $('aside nav[aria-label="Library"]')).waitForExist({ timeout: 30000 })
    report.steps.push('UI pinned to en-US, light scheme')
  })

  it('renders History as sidebar | list | detail with the lights in the sidebar', async () => {
    const layout = await browser.execute(() => ({
      iconRail: document.querySelectorAll('aside.w-12').length,
      sidebars: document.querySelectorAll('aside nav[aria-label]').length,
      sidebarLeft: document
        .querySelector('aside nav[aria-label]')
        ?.closest('aside')
        ?.getBoundingClientRect().left,
      searchInList: Boolean(document.querySelector('[role="combobox"]')),
    }))
    expect(layout.iconRail).toBe(0)
    expect(layout.sidebars).toBe(1)
    expect(layout.sidebarLeft).toBe(0)
    expect(layout.searchInList).toBe(true)

    // Window drag: the sidebar's top strip is a drag region; the search field is not.
    report.dragProbe = await browser.execute(() => {
      const strip = document.querySelector('aside [data-tauri-drag-region]')
      const box = strip?.getBoundingClientRect()
      const atStrip =
        box && document.elementFromPoint(box.left + box.width / 2, box.top + box.height / 2)
      const field = document.querySelector('[role="combobox"]')
      return {
        stripHeight: box?.height ?? null,
        stripIsDragRegion: Boolean(atStrip?.closest('[data-tauri-drag-region]')),
        searchIsDragRegion: Boolean(field?.closest('[data-tauri-drag-region]')),
      }
    })
    expect(report.dragProbe.stripIsDragRegion).toBe(true)
    expect(report.dragProbe.searchIsDragRegion).toBe(false)

    await nativeShot('01-history')
    report.steps.push(
      'History: no icon rail, sidebar at x=0, search in the list column; drag strip present'
    )
  })

  it('opens the list-column search suggestions', async () => {
    const input = await $('[role="combobox"]')
    await input.click()
    await browser.keys('type:'.split(''))
    await browser.waitUntil(async () => (await $$('[role="option"]')).length === 4, {
      timeout: 15000,
      timeoutMsg: 'type: suggestions never showed',
    })
    await nativeShot('02-history-search')
    await browser.keys(Array(5).fill('Backspace'))
    await browser.keys(['Escape'])
    report.steps.push('search field in the list column shows the type: suggestions')
  })

  it('navigates History -> Devices -> Settings -> back -> History', async () => {
    await clickSidebar('Manage')
    await waitPath('/devices')
    await (await $('[data-testid="devices-add-device"]')).waitForExist({ timeout: 15000 })
    await nativeShot('03-devices')
    report.steps.push('sidebar Manage -> /devices; device list and actions in the main content')

    await clickSidebar('Settings')
    await waitPath('/settings')
    await nativeShot('04-settings')
    await browser.execute(() =>
      [...document.querySelectorAll('a, button')]
        .find(el => el.textContent.trim() === 'Back')
        .click()
    )
    await waitPath('/devices')
    report.steps.push('sidebar Settings -> /settings; Back -> /devices')

    await clickSidebar('All items')
    await waitPath('/history')
    await nativeShot('05-history-again')
    report.steps.push('Devices sidebar All items -> /history')
  })

  it('adapts sidebar, list and detail across window widths', async function () {
    const skip = () => {
      report.steps.push(`width checks skipped: ${NOT_VISIBLE}`)
      this.skip()
    }
    if (!(await bringWindowForward())) skip()
    // One real entry, so the detail column renders its full layout.
    await ingestText('docker compose -f compose.prod.yml up -d --build --remove-orphans')
    const row = await $('[data-testid="history-card"] > button')
    await row.waitForExist({ timeout: 30000 })
    await browser.execute(button => button.click(), row)
    await (await $('[data-testid="clipboard-detail"] footer')).waitForExist({ timeout: 15000 })

    const available = await browser.execute(() => window.screen.availWidth)
    for (const target of [900, 1100, 1280, 1680, 2560]) {
      if (target > available) {
        report.steps.push(`${target}px skipped: the screen offers ${available}px`)
        continue
      }
      const reached = await setInnerWidth(target)
      if (!(await bringWindowForward())) skip()
      // Wait for the tier switch (a hidden WebView defers resize events).
      await browser.waitUntil(
        async () =>
          (await browser.execute(
            () => document.querySelector('aside')?.dataset.sidebar === 'rail'
          )) ===
          target < 1100,
        { timeout: 10000, timeoutMsg: `sidebar never matched the ${target}px tier` }
      )
      await browser.pause(300)
      const columns = await browser.execute(measureColumns)
      report.columns.push({ target, ...columns })
      expect(reached).toBe(target)
      expect(columns.rail).toBe(target < 1100)
      expect(columns.sidebar).toBe(target < 1100 ? 52 : 220)
      expect(columns.detail).toBeGreaterThanOrEqual(420)
      expect(columns.list).toBeGreaterThanOrEqual(360)
      expect(columns.detailFooter).toBe(true)
      expect(columns.overflowing).toEqual([])
      if (target === 1280) expect(columns.list).toBe(560)
      await nativeShot(`06-width-${target}`)
    }
    await setInnerWidth(1280)
    report.steps.push('columns adapt at 900/1100/1280/1680/2560 with no horizontal overflow')
  })
})
