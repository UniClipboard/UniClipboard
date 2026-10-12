// Browser checks of the startup, recovery, unlock and setup pages on the platform UI kit
// (e2e/fixtures/app-entry-kit.tsx), for one skin:
//   node e2e/app-entry-kit-browser.mjs --platform windows|macos [--out dir]
// Builds and serves the fixture with UC_UI_PLATFORM, then checks every view in both themes, two
// window sizes and two languages, and the keyboard flows. Playwright comes from PLAYWRIGHT_MODULE
// when it is not installed in the repository. A browser run is not a native WebView run.
import assert from 'node:assert/strict'
import { spawn } from 'node:child_process'
import { mkdir, writeFile } from 'node:fs/promises'
import { createServer } from 'node:net'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const args = process.argv.slice(2)
const option = name => (args.includes(name) ? args[args.indexOf(name) + 1] : undefined)
const platform = option('--platform')
if (!['windows', 'macos'].includes(platform ?? '')) {
  console.error('Usage: node e2e/app-entry-kit-browser.mjs --platform windows|macos [--out dir]')
  process.exit(2)
}
const output = path.resolve(option('--out') ?? `/tmp/uc-app-entry-kit-${platform}`)
// A free port, so a parallel run or another local server never collides with this one.
const port = await new Promise(resolve => {
  const probe = createServer().listen(0, '127.0.0.1', () => {
    const { port: free } = probe.address()
    probe.close(() => resolve(String(free)))
  })
})
const base = `http://127.0.0.1:${port}`
await mkdir(output, { recursive: true })

const frontend = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const server = spawn(process.execPath, ['e2e/visual-effects-server.mjs'], {
  cwd: frontend,
  env: {
    ...process.env,
    UC_UI_PLATFORM: platform,
    UI_FIXTURE_ENTRY: 'e2e/fixtures/app-entry-kit.tsx',
    UI_FIXTURE_PORT: port,
  },
  stdio: ['ignore', 'pipe', 'inherit'],
})
await new Promise((resolve, reject) => {
  server.stdout.on('data', chunk => String(chunk).includes(base) && resolve())
  server.on('exit', code => reject(new Error(`fixture server exited (${code})`)))
})

const views = [
  'startup',
  'upgrade',
  'upgrade-failed',
  'membership',
  'failure',
  'version-too-old',
  'recovery',
  'recovery-restart',
  'admission',
  'unlock',
  'setup-entry',
  'setup-initialize',
  'setup-redeem',
  'setup-invite',
  'setup-invite-expired',
  'setup-pending',
  'setup-processing',
  'setup-ended',
  'setup-ready',
  'setup-paired',
  'setup-import',
]
const sizes = [
  { width: 900, height: 600 },
  { width: 1100, height: 720 },
]

const { chromium } = await import(process.env.PLAYWRIGHT_MODULE || 'playwright')
const browser = await chromium.launch({ headless: true, channel: process.env.PLAYWRIGHT_CHANNEL })
const results = []
const flows = []
const errors = []
try {
  const page = await browser.newPage({ reducedMotion: 'reduce' })
  page.on('pageerror', error => errors.push(error.message))
  page.on('console', message => {
    // The fixture preview has no favicon; Chrome requests it independently of the page.
    if (
      message.type() === 'error' &&
      !message.text().includes('Network access is disabled') &&
      !message.location().url.endsWith('/favicon.ico')
    )
      errors.push(`console: ${message.text()}`)
  })

  async function open(view, { theme = 'light', language = 'zh-CN', material = 'none' } = {}) {
    const query = new URLSearchParams({ view, theme, language, 'window-material': material })
    await page.goto(`${base}/?${query}`)
    await page.locator('[data-testid="entry-kit"] h1').waitFor()
    await page.evaluate(() => document.fonts.ready)
    await page.waitForTimeout(250)
  }

  const measure = () =>
    page.evaluate(() => {
      const main = document.querySelector('[data-testid="entry-kit"] main')
      const overflowing = [...document.querySelectorAll('[data-testid="entry-kit"] *')]
        .filter(element => {
          const rect = element.getBoundingClientRect()
          return rect.width > 1 && (rect.right > innerWidth + 0.5 || rect.left < -0.5)
        })
        .map(element => element.tagName + '.' + String(element.className).slice(0, 40))
      const unnamedButtons = [...document.querySelectorAll('button')]
        .filter(button => !(button.textContent.trim() || button.getAttribute('aria-label')))
        .map(button => button.outerHTML.slice(0, 80))
      // Every `role` attribute
      // in the page must be a real ARIA role.
      const ariaRoles = new Set([
        'alert',
        'alertdialog',
        'button',
        'dialog',
        'group',
        'img',
        'listbox',
        'menu',
        'menuitem',
        'option',
        'progressbar',
        'radio',
        'radiogroup',
        'status',
        'switch',
        'timer',
        'tooltip',
        'presentation',
        'none',
        'separator',
        'list',
        'listitem',
        'region',
        'tab',
        'tablist',
        'tabpanel',
        'checkbox',
        'combobox',
        'textbox',
        'link',
        'navigation',
        'main',
        'banner',
        'form',
        'search',
        'toolbar',
        'tree',
        'treeitem',
        'grid',
        'row',
        'cell',
        'gridcell',
        'columnheader',
        'rowheader',
        'heading',
        'figure',
        'article',
      ])
      const invalidRoles = [...document.querySelectorAll('[role]')]
        .map(element => element.getAttribute('role'))
        .filter(role => !ariaRoles.has(role))
      const sizes = new Set()
      const walker = document.createTreeWalker(document.body, NodeFilter.SHOW_TEXT)
      while (walker.nextNode()) {
        const parent = walker.currentNode.parentElement
        if (!walker.currentNode.textContent.trim() || !parent?.closest('[data-testid="entry-kit"]'))
          continue
        if (parent.closest('[aria-hidden="true"]')) continue
        sizes.add(parseFloat(getComputedStyle(parent).fontSize))
      }
      return {
        skin: document.querySelector('[data-ui-skin]')?.getAttribute('data-ui-skin'),
        h1: document.querySelectorAll('h1').length,
        overflowing,
        horizontalScroll: main ? main.scrollWidth > main.clientWidth + 1 : true,
        unnamedButtons,
        invalidRoles,
        heroText:
          document
            .querySelector('[data-testid="setup-invitation-code"] > span')
            ?.hasAttribute('data-large') ?? null,
        fontSizes: [...sizes].sort((a, b) => a - b),
        entryBackground: getComputedStyle(document.querySelector('.mac-entry') ?? main)
          .backgroundColor,
        titlebarBackground: document
          .elementsFromPoint(innerWidth - 20, 20)
          .map(element => getComputedStyle(element).backgroundColor)
          .find(color => color !== 'rgba(0, 0, 0, 0)' && color !== 'transparent'),
      }
    })

  for (const language of ['zh-CN', 'en-US'])
    for (const theme of ['light', 'dark'])
      for (const size of sizes) {
        await page.setViewportSize(size)
        for (const view of views) {
          await open(view, { theme, language })
          const metrics = await measure()
          const name = `${view}-${language}-${theme}-${size.width}`
          await page.screenshot({ path: path.join(output, `${name}.png`) })
          results.push({ name, ...metrics })
          assert.equal(metrics.skin, platform, `${name}: rendered by the ${platform} skin`)
          assert.equal(metrics.h1, 1, `${name}: exactly one page heading`)
          assert.deepEqual(metrics.overflowing, [], `${name}: nothing crosses the window edge`)
          assert.equal(metrics.horizontalScroll, false, `${name}: no horizontal scroll`)
          assert.deepEqual(metrics.unnamedButtons, [], `${name}: every button has a name`)
          assert.deepEqual(
            metrics.invalidRoles,
            [],
            `${name}: every role attribute is an ARIA role`
          )
          if (view === 'setup-invite' && platform === 'macos')
            assert.equal(
              metrics.heroText,
              true,
              `${name}: the invitation code uses the large text style`
            )
          assert.ok(Math.min(...metrics.fontSizes) >= 11, `${name}: no text under 11px`)
          if (platform === 'macos')
            assert.equal(
              metrics.titlebarBackground,
              metrics.entryBackground,
              `${name}: title bar and page share one background`
            )
        }
      }

  const focused = () =>
    page.evaluate(() => {
      const el = document.activeElement
      return {
        tag: el?.tagName,
        name: el?.getAttribute('aria-label') || el?.textContent?.trim() || '',
        inTestId: el?.closest('[data-testid]')?.getAttribute('data-testid') ?? null,
      }
    })
  const flow = async (name, run) => {
    await run()
    flows.push(name)
  }
  await page.setViewportSize(sizes[0])

  if (platform === 'macos') {
    await flow('entry backgrounds match with and without native material (simulated)', async () => {
      const measurements = []
      for (const theme of ['light', 'dark']) {
        await page.emulateMedia({ colorScheme: theme })
        for (const material of ['none', 'liquid-glass', 'visual-effect']) {
          for (const view of ['startup', 'setup-entry', 'unlock']) {
            await open(view, { theme, material })
            const metrics = await measure()
            measurements.push({ view, theme, material, ...metrics })
            await writeFile(
              path.join(output, 'entry-backgrounds.json'),
              JSON.stringify(measurements, null, 2)
            )
            await page.screenshot({
              path: path.join(output, `background-${view}-${theme}-${material}.png`),
            })
            assert.equal(
              metrics.titlebarBackground,
              metrics.entryBackground,
              `${view}-${theme}-${material}: uniform background`
            )
            assert.equal(
              await page.locator('[data-ui-skin]').getAttribute('data-native-material'),
              material === 'none' ? null : ''
            )
          }
        }
      }
      await page.emulateMedia({ colorScheme: null })
    })
  }

  await flow(
    'unlock: keyring fallback, wrong passphrase, show/hide, right passphrase',
    async () => {
      await open('unlock')
      await page.keyboard.press('Tab')
      assert.equal(
        (await focused()).inTestId,
        'unlock-content',
        'the unlock button is the first stop'
      )
      await page.locator('[data-testid="unlock-content"]').evaluate(button => button.click())
      await page.getByRole('button', { name: '正在解锁...' }).waitFor()
      await page.screenshot({ path: path.join(output, 'flow-unlock-busy.png') })
      const passphrase = page.locator('#unlock-passphrase')
      await passphrase.waitFor()
      assert.equal((await focused()).tag, 'INPUT', 'the passphrase field takes focus')
      await passphrase.fill('wrong')
      await page.keyboard.press('Enter')
      await page.getByRole('alert').waitFor()
      assert.match(await page.getByRole('alert').innerText(), /口令错误/)
      assert.equal(await passphrase.getAttribute('aria-invalid'), 'true')
      await page.screenshot({ path: path.join(output, 'flow-unlock-wrong-passphrase.png') })
      await page.getByRole('button', { name: '显示口令' }).click()
      assert.equal(await passphrase.getAttribute('type'), 'text')
      assert.equal(
        await page.getByRole('button', { name: '隐藏口令' }).getAttribute('aria-pressed'),
        'true'
      )
      await passphrase.fill('fixture-passphrase')
      await page.keyboard.press('Enter')
      await page.waitForFunction(() => document.body.dataset.unlocked === 'true')
    }
  )

  await flow('reset: typed RESET guard, Escape closes, focus returns to the link', async () => {
    await open('unlock')
    const link = page.getByRole('button', { name: '忘记口令？重置并重新开始' })
    await link.focus()
    await page.keyboard.press('Enter')
    const dialog = page.getByRole('alertdialog')
    await dialog.waitFor()
    await page.waitForTimeout(300)
    assert.equal((await focused()).tag, 'INPUT', 'the confirmation field takes focus')
    const confirm = dialog.getByRole('button', { name: '重置', exact: true })
    assert.equal(await confirm.isDisabled(), true)
    await dialog.getByLabel('请输入 RESET 以确认').fill('RESET')
    assert.equal(await confirm.isDisabled(), false)
    await page.screenshot({ path: path.join(output, 'flow-unlock-reset-dialog.png') })
    await page.keyboard.press('Escape')
    await dialog.waitFor({ state: 'detached' })
    await page.waitForTimeout(400)
    assert.equal((await focused()).name, '忘记口令？重置并重新开始', 'focus returns to the link')
  })

  await flow(
    'setup: choice keyboard, field validation, service error, back, join error',
    async () => {
      await open('setup-entry')
      await page.keyboard.press('Tab')
      assert.match(
        (await focused()).name,
        /这是我的第一台设备/,
        'the first choice is the first stop'
      )
      await page.screenshot({ path: path.join(output, 'flow-setup-entry-focus.png') })
      await page.locator('[data-testid="setup-entry-create"]').evaluate(button => button.click())
      await page.locator('#device-name').fill('Fixture Mac')
      await page.locator('#pass1').fill('one')
      await page.locator('#pass2').fill('two')
      await page.keyboard.press('Enter')
      await page.getByRole('alert').filter({ hasText: '两次输入的口令不一致' }).waitFor()
      assert.equal(await page.locator('#pass2').getAttribute('aria-invalid'), 'true')
      await page.screenshot({ path: path.join(output, 'flow-setup-initialize-mismatch.png') })
      await page.locator('#pass2').fill('one')
      await page
        .locator('[data-testid="setup-initialize-submit"]')
        .evaluate(button => button.click())
      await page.getByText('守护进程尚未就绪，请稍后重试。').waitFor()
      await page.screenshot({ path: path.join(output, 'flow-setup-initialize-service-error.png') })
      await page.getByRole('button', { name: '返回' }).click()
      await page.getByRole('button', { name: /我已经在另一台设备上使用/ }).click()
      await page.locator('#join-code').fill('482913')
      await page.locator('#join-pass').waitFor()
      assert.equal((await focused()).tag, 'INPUT')
      assert.equal(
        await page.evaluate(() => document.activeElement?.closest('[data-testid]')?.dataset.testid),
        'join-pass',
        'focus moves to the passphrase once the code is complete'
      )
      await page.keyboard.type('secret')
      await page.keyboard.press('Enter')
      await page.getByText('暂时无法连接对方设备，请确认两台设备都在线。').waitFor()
      await page.screenshot({ path: path.join(output, 'flow-setup-redeem-error.png') })
    }
  )

  await flow('setup: invitation issue failure is reported', async () => {
    await open('setup-ready')
    await page.locator('[data-testid="setup-complete-invite"]').click()
    await page.getByRole('alert').waitFor()
    await page.screenshot({ path: path.join(output, 'flow-setup-invite-issue-error.png') })
  })

  assert.deepEqual(errors, [], 'no page errors')
} finally {
  await writeFile(
    path.join(output, 'results.json'),
    JSON.stringify({ platform, views: results, flows, errors }, null, 2) + '\n'
  )
  await browser.close()
  server.kill()
}
console.log(
  `${platform}: ${results.length} view checks and ${flows.length} flows passed; evidence in ${output}`
)
