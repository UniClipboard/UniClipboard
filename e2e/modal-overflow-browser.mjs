// Measures horizontal overflow of shared modals with classic (space-reserving) scrollbars.
// This runs in Chromium, not WebKitGTK: it can prove layout overflow, but it cannot certify
// Ubuntu WebKitGTK rendering.
import { mkdir, writeFile } from 'node:fs/promises'

const playwright = await import(process.env.PLAYWRIGHT_MODULE || 'playwright')
const { chromium } = playwright.default ?? playwright
const base = process.env.MODAL_TEST_URL || 'http://127.0.0.1:1452'
const output = process.env.MODAL_TEST_OUTPUT || '/tmp/uc-modal-overflow'
await mkdir(output, { recursive: true })

const browser = await chromium.launch({
  headless: true,
  channel: process.env.PLAYWRIGHT_CHANNEL,
  executablePath: process.env.CHROMIUM_EXECUTABLE || undefined,
  ignoreDefaultArgs: ['--hide-scrollbars'],
  args: ['--disable-features=OverlayScrollbar', '--disable-smooth-scrolling'],
})
const callSites = process.env.MODAL_FIXTURE === 'call-sites'
const languages = ['en-US', 'zh-CN', 'ru-RU', 'pt-BR', 'ja-JP']
const kinds = callSites
  ? [
      'clear-history',
      'delete',
      'change-passphrase',
      'unpair',
      'rebuild-space',
      'feedback',
      're-pairing',
      'package-manager-update',
      'factory-reset',
    ]
  : ['dialog', 'dialog-body', 'alert', 'sheet']
const contents = callSites ? languages : ['short', 'long', 'longword']
const viewports = [
  { width: 360, height: 640 },
  { width: 420, height: 500 },
  { width: 1100, height: 800 },
]
const scales = [1, 1.25, 1.5]
const rootFontSizes = callSites ? ['16px', '20px'] : ['16px']
const failures = []
const rows = []
try {
  for (const deviceScaleFactor of scales) {
    for (const viewport of viewports) {
      const context = await browser.newContext({
        viewport,
        deviceScaleFactor,
        reducedMotion: 'reduce',
      })
      const page = await context.newPage()
      for (const rootFontSize of rootFontSizes) {
        for (const kind of kinds) {
          for (const content of contents) {
            await page.goto(
              `${base}/?${new URLSearchParams(callSites ? { site: kind, language: content } : { kind, content })}`
            )
            await page.locator('[data-slot$="-content"]').first().waitFor()
            // Assumption: the target engine lets the 10px ::-webkit-scrollbar rule reserve layout
            // space. Chromium prefers scrollbar-width, so neutralise it here to force that layout.
            await page.addStyleTag({
              content: `* { scrollbar-width: auto !important; scrollbar-color: auto !important } html { font-size: ${rootFontSize} }`,
            })
            await page.waitForTimeout(150)
            const result = await page.evaluate(() => {
              const popup = document.querySelector('[data-slot$="-content"]')
              const overflowing = []
              for (const el of [popup, ...popup.querySelectorAll('*')]) {
                const style = getComputedStyle(el)
                if (
                  el.scrollWidth > el.clientWidth &&
                  !el.classList.contains('sr-only') &&
                  style.display !== 'inline'
                ) {
                  overflowing.push({
                    el: `${el.tagName.toLowerCase()}[${el.getAttribute('data-slot') ?? ''}]`,
                    scrollWidth: el.scrollWidth,
                    clientWidth: el.clientWidth,
                    overflowX: style.overflowX,
                  })
                }
              }
              const rect = popup.getBoundingClientRect()
              const root = document.documentElement
              // Sub-pixel excess is rounded away by scrollWidth/clientWidth, so measure rects.
              const popupStyle = getComputedStyle(popup)
              const paddingBoxRight = rect.left + popup.clientLeft + popup.clientWidth
              let worst = { excess: 0, el: '' }
              for (const el of popup.querySelectorAll('*')) {
                if (el.classList.contains('sr-only')) continue
                let clipped = false
                for (let a = el.parentElement; a && a !== popup; a = a.parentElement) {
                  if (getComputedStyle(a).overflowX !== 'visible') clipped = true
                }
                if (clipped) continue
                const box = el.getBoundingClientRect()
                if (!box.width) continue
                const excess = box.right - paddingBoxRight
                if (excess > worst.excess) {
                  worst = {
                    excess,
                    el: `${el.tagName.toLowerCase()}[${el.getAttribute('data-slot') ?? ''}]`,
                  }
                }
              }
              const lock = {
                htmlOverflow:
                  getComputedStyle(root).overflowX + '/' + getComputedStyle(root).overflowY,
                scrollLocked: root.hasAttribute('data-base-ui-scroll-locked'),
                bodyWidth: document.body.style.width,
              }
              return {
                overflowing,
                worst,
                lock,
                popupStyleOverflowY: popupStyle.overflowY,
                popup: {
                  width: rect.width,
                  offsetWidth: popup.offsetWidth,
                  clientWidth: popup.clientWidth,
                  scrollWidth: popup.scrollWidth,
                  overflowX: getComputedStyle(popup).overflowX,
                  verticalScrollbar: popup.offsetWidth - popup.clientWidth - 2 * popup.clientLeft,
                  clientHeight: popup.clientHeight,
                  scrollHeight: popup.scrollHeight,
                },
                document: { scrollWidth: root.scrollWidth, clientWidth: root.clientWidth },
              }
            })
            const name = `${kind}-${content}-${viewport.width}x${viewport.height}@${deviceScaleFactor}-rem${rootFontSize}`
            const popupOverflow = result.popup.scrollWidth > result.popup.clientWidth
            const documentOverflow = result.document.scrollWidth > result.document.clientWidth
            rows.push({ name, ...result })
            // `overflowing` (descendants overflowing their own box) is recorded but not a failure: a form
            // legitimately contains its negative-margin footer. Only overflow past the popup counts.
            if (popupOverflow || documentOverflow || result.worst.excess > 0.01) {
              failures.push(name)
              await page.screenshot({ path: `${output}/${name}.png` })
            }
          }
        }
      }
      await context.close()
    }
  }
} finally {
  await browser.close()
}
await writeFile(`${output}/geometry.json`, JSON.stringify(rows, null, 2))
console.log(`${rows.length} scenarios, ${failures.length} with horizontal overflow`)
for (const failure of failures) console.log(`overflow: ${failure}`)
process.exitCode = failures.length ? 1 : 0
