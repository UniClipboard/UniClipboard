import assert from 'node:assert/strict'
import { test, outputFor } from './page.mjs'

for (const theme of ['light', 'dark']) {
  test(`sidebar notifications without read action ${theme}`, async ({ app, page }) => {
    const output = await outputFor('main-notifications')
    await page.addInitScript(() => {
      window.__announcementsScript = async () => [
        {
          id: 'notice-main',
          severity: 'warning',
          title: '同步服务维护通知',
          content: '维护期间请保留应用。**本地历史仍可使用。**',
          requireAcknowledgement: false,
          publishedAt: '2026-11-08T00:00:00Z',
          endsAt: null,
        },
      ]
    })
    await page.goto(app.baseUrl)
    await page.evaluate(
      theme => document.documentElement.classList.toggle('dark', theme === 'dark'),
      theme
    )
    const entry = () => page.getByRole('button', { name: '通知（1）', exact: true })
    const devices = () => page.getByRole('button', { name: '管理设备…', exact: true })
    await entry().waitFor()
    assert.equal(await page.getByRole('dialog').count(), 0)
    const order = async () => {
      const notice = await entry().boundingBox()
      const manage = await devices().boundingBox()
      assert.ok(notice.y + notice.height <= manage.y)
    }
    await order()
    await page.screenshot({ path: `${output}/${theme}-expanded.png` })
    await page.getByRole('button', { name: '隐藏侧边栏', exact: true }).click()
    await entry().waitFor()
    await order()
    await page.screenshot({ path: `${output}/${theme}-collapsed.png` })
    await entry().click()
    const heading = page.getByRole('heading', { name: '同步服务维护通知', exact: true })
    await heading.waitFor()
    await page.evaluate(async () => {
      await new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve)))
      await Promise.all(
        document.getAnimations().map(animation => animation.finished.catch(() => {}))
      )
    })
    const refreshBounds = await page
      .getByRole('button', { name: '刷新', exact: true })
      .boundingBox()
    const doneBounds = await page.getByRole('button', { name: '完成', exact: true }).boundingBox()
    assert.equal(await page.getByRole('button', { name: '已读', exact: true }).count(), 0)
    assert.ok(refreshBounds.x < doneBounds.x)
    assert.ok(
      Math.abs(refreshBounds.y + refreshBounds.height / 2 - doneBounds.y - doneBounds.height / 2) <
        2
    )
    assert.ok(refreshBounds.height >= 24)
    await page.screenshot({ path: `${output}/${theme}-sheet.png` })
    await page.getByRole('dialog').screenshot({ path: `${output}/${theme}-sheet-detail.png` })
    await page.getByRole('button', { name: '完成', exact: true }).click()
    await page.getByRole('dialog').waitFor({ state: 'hidden' })
    await entry().click()
    await heading.waitFor()
    assert.equal(await page.getByRole('button', { name: '已读', exact: true }).count(), 0)
    await page.evaluate(() => {
      window.__announcementsScript = async () => [
        {
          id: 'notice-new',
          severity: 'info',
          title: '新的版本通知',
          content: '新的消息',
          requireAcknowledgement: false,
          publishedAt: '2026-11-08T01:00:00Z',
          endsAt: null,
        },
      ]
    })
    await page.getByRole('button', { name: '刷新', exact: true }).click()
    await page.getByRole('heading', { name: '新的版本通知', exact: true }).waitFor()
    await page.getByRole('button', { name: '完成', exact: true }).click()
    await page.getByRole('dialog').waitFor({ state: 'hidden' })
    await entry().waitFor()
    await entry().click()
    await page.getByRole('heading', { name: '新的版本通知', exact: true }).waitFor()
  })
}
