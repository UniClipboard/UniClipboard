import { $, browser, expect } from '@wdio/globals'
import { waitForSetup } from '../helpers/waitForSetup.js'

async function clickElement(element) {
  await browser.execute(button => button.click(), element)
}

async function expectElement(selector, { timeout = 10000 } = {}) {
  const element = await $(selector)
  await element.waitForExist({ timeout })
  await expect(element).toExist()
  return element
}

describe('首次启动设置窗口', () => {
  it('可以调用 Tauri 后端命令', async () => {
    const pid = await browser.tauri.execute(({ core }) => core.invoke('get_tauri_pid'))

    expect(pid).toBeGreaterThan(0)
  })

  it('可以点击创建和加入入口并看到对应界面', async () => {
    const createEntry = await waitForSetup()

    await clickElement(createEntry)
    await expectElement('#device-name')
    await expectElement('[data-testid="setup-initialize-submit"]')

    await clickElement(await expectElement('[data-testid="setup-initialize-back"]'))
    const joinEntry = await expectElement('[data-testid="setup-entry-join"]')
    await clickElement(joinEntry)

    await expectElement('[data-testid="setup-redeem-code"]')
    await expectElement('[data-testid="setup-redeem-submit"]')

    await browser.closeWindow()
  })
})
