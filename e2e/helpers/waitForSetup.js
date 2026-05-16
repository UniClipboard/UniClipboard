import { $, expect } from '@wdio/globals'

// 等首次启动设置窗口出现 create 入口。多个 spec 共用的前置：
// 真窗口启动 + setup 路由 + DOM 锚点 都到位才算通过。
export async function waitForSetup({ timeout = 30000 } = {}) {
  const createEntry = await $('[data-testid="setup-entry-create"]')
  await createEntry.waitForDisplayed({ timeout })
  await expect(createEntry).toBeDisplayed()
  return createEntry
}
