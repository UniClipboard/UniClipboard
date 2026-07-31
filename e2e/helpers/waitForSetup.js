import { $, expect } from '@wdio/globals'

// Wait until the real window, setup route, and stable DOM anchor are ready.
export async function waitForSetup({ timeout = 30000 } = {}) {
  const createEntry = await $('[data-testid="setup-entry-create"]')
  await createEntry.waitForExist({ timeout })
  await expect(createEntry).toExist()
  return createEntry
}
