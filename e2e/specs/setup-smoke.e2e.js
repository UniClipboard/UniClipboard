import { $, browser, expect } from '@wdio/globals'
import { waitForSetup } from '../helpers/waitForSetup.js'

// tauri-driver 在 Linux/WebKitWebDriver 下不支持 `POST element/{id}/click`,
// 用 browser.execute 通过 DOM 触发 click 绕过 (本地 click 行为一致).
async function jsClick(el) {
  await browser.execute(node => node.click(), el)
}

describe('首次启动设置窗口', () => {
  it('可以点击创建和加入入口并看到对应界面', async () => {
    const createEntry = await waitForSetup()

    await jsClick(createEntry)
    await expect(await $('#device-name')).toBeDisplayed()
    await expect(await $('[data-testid="setup-initialize-submit"]')).toBeDisplayed()

    await jsClick(await $('[data-testid="setup-initialize-back"]'))
    const joinEntry = await $('[data-testid="setup-entry-join"]')
    await joinEntry.waitForDisplayed({ timeout: 10000 })
    await jsClick(joinEntry)

    await expect(await $('[data-testid="setup-redeem-code"]')).toBeDisplayed()
    await expect(await $('[data-testid="setup-redeem-submit"]')).toBeDisplayed()

    await browser.closeWindow()
  })
})
