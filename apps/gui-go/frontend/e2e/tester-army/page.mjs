import { mkdir } from 'node:fs/promises'
import path from 'node:path'
import { surfaceOf, test as base } from '@e2e-dev/web'
import { browserEngine } from './engine.mjs'
import { selectedPlatform, selectedSuite } from './suites.mjs'

// Keep the original Playwright assertions where the portable SDK has no equivalent.
// The runner owns this page/context; raw operations are not SDK step events.
export const test = base.extend({
  createContext: async (_, provide) => {
    const owned = []
    try {
      await provide(async options => {
        const context = await surfaceOf(browserEngine).context().browser().newContext(options)
        owned.push(context)
        return context
      })
    } finally {
      let index = 0
      try {
        for (const context of owned) {
          for (const page of context.pages()) {
            if (!page.isClosed())
              await page.screenshot({
                path: path.join(await outputFor(selectedSuite), `context-final-${index++}.png`),
              })
          }
        }
      } finally {
        for (const context of owned) await context.close()
      }
    }
  },
  page: async ({ app }, provide) => {
    await app.open('/runner-blank.html')
    const page = surfaceOf(browserEngine).page()
    page.setDefaultTimeout(15_000)
    await provide(page)
  },
})
export async function outputFor(name) {
  const output = path.resolve(
    'target/tester-army-layout',
    selectedPlatform,
    selectedSuite,
    name === selectedSuite ? '' : name
  )
  await mkdir(output, { recursive: true })
  return output
}
