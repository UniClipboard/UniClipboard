import assert from 'node:assert/strict'
import { test, outputFor } from './page.mjs'

for (const theme of ['light', 'dark']) {
  test(`release notifications ${theme}`, async ({ app, page }) => {
    const output = await outputFor('release-notifications')
    await page.goto(app.baseUrl)
    await page.evaluate(theme => {
      document.documentElement.classList.toggle('dark', theme === 'dark')
      window.__announcementsScript = async () => [
        {
          id: 'notice-1',
          severity: 'critical',
          title: '维护通知',
          content: '**请阅读维护说明**\n\n[详情](https://www.uniclipboard.app/)',
          requireAcknowledgement: true,
          publishedAt: '2026-11-08T00:00:00Z',
          endsAt: null,
        },
      ]
    }, theme)
    const showSidebar = page.getByRole('button', { name: '显示侧边栏', exact: true })
    if (await showSidebar.isVisible()) await showSidebar.click()
    await page.getByRole('button', { name: '通知', exact: true }).click()
    await page.getByRole('heading', { name: '维护通知', exact: true }).waitFor()
    assert.equal(await page.getByText('紧急', { exact: true }).count(), 1)
    await page.getByRole('link', { name: '详情', exact: true }).click()
    assert.deepEqual(await page.evaluate(() => window.__settingsFixtureNative.openedUrls), [
      'https://www.uniclipboard.app/',
    ])
    await page.screenshot({ path: `${output}/${theme}-notices.png` })

    await page.evaluate(() => {
      window.__announcementsScript = async () => []
    })
    await page.getByRole('button', { name: '刷新', exact: true }).click()
    await page.getByText('暂无适用于当前版本的通知。', { exact: true }).waitFor()
    assert.equal(await page.getByRole('heading', { name: '维护通知', exact: true }).count(), 0)

    await page.evaluate(() => {
      window.__announcementsScript = async () => {
        throw new Error('synthetic offline')
      }
    })
    await page.getByRole('button', { name: '刷新', exact: true }).click()
    await page.getByRole('alert').filter({ hasText: '通知加载失败' }).waitFor()
    await page.screenshot({ path: `${output}/${theme}-offline.png` })
    await page.evaluate(() => {
      window.__announcementsScript = async () => []
    })
    await page.getByRole('button', { name: '刷新', exact: true }).click()
    await page.getByText('暂无适用于当前版本的通知。', { exact: true }).waitFor()
  })
}
