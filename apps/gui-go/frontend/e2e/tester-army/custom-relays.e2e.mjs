import assert from 'node:assert/strict'
import { test } from '@e2e-dev/web'

test('custom relays preserve authoritative mutations and error recovery', async ({
  app,
  browser,
  screen,
}) => {
  await browser.addInitScript(() => {
    window.addEventListener('unhandledrejection', event => {
      const errors = JSON.parse(sessionStorage.getItem('page-errors') || '[]')
      errors.push(String(event.reason))
      sessionStorage.setItem('page-errors', JSON.stringify(errors))
    })
    window.addEventListener('error', event => {
      const errors = JSON.parse(sessionStorage.getItem('page-errors') || '[]')
      errors.push(event.message)
      sessionStorage.setItem('page-errors', JSON.stringify(errors))
    })
  })
  await app.open('/?category=network&relayE2E=1')
  await screen.getByRole('heading', { level: 1, name: '网络设置' }).waitFor()
  await browser.locator('input[value="https://relay-one.example.com/"]').waitFor()
  assert.equal(await screen.getByText('访问令牌已保存', { exact: true }).count(), 1)
  assert.equal(await screen.getByRole('textbox', { name: /自定义中继节点/ }).count(), 2)

  await browser.evaluate(() => {
    window.__customRelayFixture.injectServerRelay({
      url: 'https://remote-change.example.com/',
      credentialConfigured: false,
    })
  })

  const firstRelay = screen.getByRole('textbox', { name: '自定义中继节点 1' })
  await firstRelay.fill('https://relay-one-renamed.example.com')
  const firstEditor = browser.locator(
    'input[aria-label="自定义中继节点 1"] >> xpath=ancestor::section'
  )
  await firstEditor.getByRole('button', { name: '测试可用性' }).click()
  await firstEditor.getByText(/握手成功/).waitFor()
  await firstEditor.getByRole('button', { name: '保存中继节点' }).click()

  await browser.locator('input[value="https://relay-one-renamed.example.com/"]').waitFor()
  await browser.locator('input[value="https://remote-change.example.com/"]').waitFor()
  assert.equal(await screen.getByRole('textbox', { name: /自定义中继节点/ }).count(), 3)
  const editMutation = await browser.evaluate(() => window.__customRelayFixture.getMutations()[0])
  assert.deepEqual(editMutation, {
    action: 'edit',
    previousUrl: 'https://relay-one.example.com/',
    url: 'https://relay-one-renamed.example.com',
    credential: { action: 'keep' },
  })
  assert.equal(await screen.getByText('访问令牌已保存', { exact: true }).count(), 1)

  await browser.evaluate(() => window.__customRelayFixture.setRestartFailure(true))
  await screen.getByRole('button', { name: '立即重启' }).click()
  await screen.getByText('后台服务重启失败，请重试或手动重启应用。', { exact: false }).waitFor()
  assert.equal(await screen.getByRole('textbox', { name: /自定义中继节点/ }).count(), 3)
  await browser.locator('input[value="https://remote-change.example.com/"]').waitFor()

  const secondRelay = screen.getByRole('textbox', { name: '自定义中继节点 2' })
  await secondRelay.fill('https://relay-one-renamed.example.com')
  const secondEditor = browser.locator(
    'input[aria-label="自定义中继节点 2"] >> xpath=ancestor::section'
  )
  await secondEditor.getByRole('button', { name: '测试可用性' }).click()
  await secondEditor.getByText(/握手成功/).waitFor()
  await secondEditor.getByRole('button', { name: '保存中继节点' }).click()
  await screen
    .getByText(/中继 URL 已重复/)
    .first()
    .waitFor()

  await screen.getByRole('button', { name: '添加中继节点' }).click()
  const invalidRelay = screen.getByRole('textbox', { name: '自定义中继节点 4' })
  await invalidRelay.fill('ftp://invalid.example.com')
  const invalidEditor = browser.locator(
    'input[aria-label="自定义中继节点 4"] >> xpath=ancestor::section'
  )
  await invalidEditor.getByRole('button', { name: '测试可用性' }).click()
  await invalidEditor.getByText(/握手成功/).waitFor()
  await invalidEditor.getByRole('button', { name: '保存中继节点' }).click()
  await screen
    .getByText(/无效的中继 URL/)
    .first()
    .waitFor()
  await invalidEditor.getByRole('button', { name: '移除中继节点 4' }).click()

  await browser.evaluate(() => {
    window.__customRelayFixture.removeServerRelay('https://relay-two.example.com/')
  })
  await secondEditor.getByRole('button', { name: '移除中继节点 2' }).click()
  await screen
    .getByText(/此中继已不存在/)
    .first()
    .waitFor()
  await browser.locator('input[value="https://remote-change.example.com/"]').waitFor()
  assert.equal(await screen.getByRole('textbox', { name: /自定义中继节点/ }).count(), 2)

  await screen.getByRole('button', { name: '添加中继节点' }).click()
  const addedRelay = screen.getByRole('textbox', { name: '自定义中继节点 3' })
  await addedRelay.fill('https://added.example.com')
  const addedEditor = browser.locator(
    'input[aria-label="自定义中继节点 3"] >> xpath=ancestor::section'
  )
  await addedEditor.getByRole('textbox', { name: '中继访问令牌 3' }).fill('e2e-placeholder-token')
  await addedEditor.getByRole('button', { name: '测试可用性' }).click()
  await addedEditor.getByText(/握手成功/).waitFor()
  await addedEditor.getByRole('button', { name: '保存中继节点' }).click()
  const savedAddedRelay = browser.locator('input[value="https://added.example.com/"]')
  await savedAddedRelay.waitFor()
  const savedAddedEditor = browser.locator(
    'input[value="https://added.example.com/"] >> xpath=ancestor::section'
  )
  await savedAddedEditor.getByText('访问令牌已保存', { exact: true }).waitFor()
  // Return only emptiness; the runner refuses to expose a password field's value.
  assert.equal(
    await browser.evaluate(
      () => document.querySelector('input[aria-label="中继访问令牌 3"]').value === ''
    ),
    true
  )
  await savedAddedEditor.getByRole('button', { name: '移除中继节点 3' }).click()
  await savedAddedRelay.waitFor({ state: 'detached' })

  await browser.evaluate(() => window.__customRelayFixture.showLoadError())
  await screen.getByText('无法加载自定义中继列表。', { exact: true }).waitFor()
  await screen.getByRole('button', { name: '重试' }).click()
  await browser.locator('input[value="https://relay-one-renamed.example.com/"]').waitFor()

  assert.deepEqual(
    await browser.evaluate(() => JSON.parse(sessionStorage.getItem('page-errors') || '[]')),
    []
  )
  await app.screenshot('custom-relays-complete')
})
