import assert from 'node:assert/strict'
import { test } from '@e2e-dev/web'

test('relay overview preserves all seven legacy scenarios', async ({ app, browser, screen }) => {
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
    window.__relayOverviewScript = async () => {
      const state = JSON.parse(sessionStorage.getItem('relay-script') || '{}')
      if (state.failNext > 0) {
        state.failNext--
        sessionStorage.setItem('relay-script', JSON.stringify(state))
        return { fail: true }
      }
      return { response: state.response }
    }
  })
  await app.open('/')
  const url = new URL('/?category=network', app.baseUrl).href
  const entry = (source, regionId, relayUrl, inEffect, credentialConfigured = false) => ({
    source,
    regionId,
    url: relayUrl,
    credentialConfigured,
    inEffect,
  })
  const BUILT_IN = [
    ['na-east', 'https://use1-1.relay.n0.iroh.link./'],
    ['na-west', 'https://usw1-1.relay.n0.iroh.link./'],
    ['eu', 'https://euc1-1.relay.n0.iroh.link./'],
    ['asia-pacific', 'https://aps1-1.relay.n0.iroh.link./'],
  ]
  const builtIn = inEffect => BUILT_IN.map(([id, u]) => entry('builtIn', id, u, inEffect))
  const script = async (response, failNext = 0) => {
    await browser.evaluate(value => sessionStorage.setItem('relay-script', JSON.stringify(value)), {
      response,
      failNext,
    })
  }
  const builtInOverviewFor = inEffect => ({
    savedMode: 'builtIn',
    appliedMode: 'builtIn',
    changePending: false,
    entries: builtIn(inEffect),
  })
  const rows = () => screen.getByTestId('built-in-relay-row')
  const routing = () => screen.getByTestId('relay-overview-routing')
  const applied = () => screen.getByTestId('relay-overview-applied')

  // 1. Built-in routing, applied, no pending change: four localized rows, no controls.
  await script(builtInOverviewFor(true))
  await browser.goto(url)
  await screen.getByRole('heading', { level: 1, name: '网络设置' }).waitFor()
  await rows().first().waitFor()
  assert.equal(await rows().count(), 4)
  assert.deepEqual(
    await browser.evaluate(() =>
      [...document.querySelectorAll('[data-testid="built-in-relay-row"]')].map(n =>
        n.getAttribute('data-region-id')
      )
    ),
    ['na-east', 'na-west', 'eu', 'asia-pacific']
  )
  const list = screen.getByTestId('built-in-relay-list')
  await list.getByText('北美东部', { exact: true }).waitFor()
  await list.getByText('亚太', { exact: true }).waitFor()
  await list.getByText('https://use1-1.relay.n0.iroh.link./', { exact: true }).waitFor()
  assert.equal(await list.getByText(/已配置到运行中的节点/).count(), 4)
  assert.equal(
    await browser
      .locator('[data-testid="built-in-relay-list"] :is(button, input, [role="switch"])')
      .count(),
    0
  )
  assert.equal(await routing().getAttribute('data-routing-mode'), 'builtIn')
  assert.equal(await applied().getAttribute('data-applied-mode'), 'builtIn')
  const pageText = await browser.evaluate(() => document.body.innerText)
  assert.ok(!/已连接|connected/i.test(pageText.replace('并不表示已经连上某个中继', '')))

  await app.screenshot('01-built-in-applied')
  // 2. Reload with applied mode absent: node not started, nothing claimed as in effect.
  await script({
    savedMode: 'builtIn',
    appliedMode: null,
    changePending: false,
    entries: builtIn(false),
  })
  await browser.reload()
  await rows().first().waitFor()
  await applied().getByText('网络节点尚未启动', { exact: false }).waitFor()
  assert.equal(await applied().getAttribute('data-applied-mode'), 'none')
  assert.equal(await browser.locator('[data-in-effect="true"]').count(), 0)

  await app.screenshot('02-node-not-started')
  // 3. change_pending: the pending notice is shown and in_effect is not flipped locally.
  await script({
    savedMode: 'builtIn',
    appliedMode: 'custom',
    changePending: true,
    entries: builtIn(false),
  })
  await browser.reload()
  await rows().first().waitFor()
  await applied()
    .getByText(/下次重建/)
    .waitFor()
  assert.equal(await applied().getAttribute('data-change-pending'), 'true')
  assert.equal(await browser.locator('[data-in-effect="true"]').count(), 0)

  await app.screenshot('03-change-pending')
  // 4. Custom list replaces built-in: rows dimmed and marked not used; custom rows stay editable.
  await script({
    savedMode: 'custom',
    appliedMode: 'custom',
    changePending: false,
    entries: [
      ...builtIn(false),
      entry('custom', null, 'https://relay-one.example.com/', true, true),
    ],
  })
  await browser.goto(`${url}&relayE2E=1`)
  await rows().first().waitFor()
  assert.equal(await routing().getAttribute('data-routing-mode'), 'custom')
  assert.equal(await list.getByText('未使用', { exact: false }).count(), 4)
  assert.equal(await rows().count(), 4)
  await browser.locator('input[value="https://relay-one.example.com/"]').waitFor()

  await app.screenshot('04-custom-replaces-built-in')
  // 5. Disabled (LAN-only) wins: listed, none in effect, dimmed.
  await script({
    savedMode: 'disabled',
    appliedMode: 'disabled',
    changePending: false,
    entries: builtIn(false),
  })
  await browser.goto(url)
  await rows().first().waitFor()
  assert.equal(await routing().getAttribute('data-routing-mode'), 'disabled')
  await routing()
    .getByText(/中继已关闭/)
    .waitFor()
  assert.equal(await browser.locator('[data-in-effect="true"]').count(), 0)

  await app.screenshot('05-lan-only-disabled')
  // 6. Unknown future region id falls back to the URL.
  await script({
    savedMode: 'builtIn',
    appliedMode: 'builtIn',
    changePending: false,
    entries: [entry('builtIn', 'sa-east', 'https://sae1-1.relay.example.net./', true)],
  })
  await browser.goto(url)
  await rows().first().waitFor()
  assert.equal(
    await browser.evaluate(
      () => document.querySelector('[data-testid="built-in-relay-row"] span').textContent
    ),
    'https://sae1-1.relay.example.net./'
  )

  await app.screenshot('06-unknown-region-url-fallback')
  // 7. Query failure shows a retryable error, never an empty list; retry recovers.
  await script(
    { savedMode: 'builtIn', appliedMode: 'builtIn', changePending: false, entries: builtIn(true) },
    1
  )
  await browser.goto(url)
  await screen.getByText('无法加载内置中继列表。', { exact: true }).waitFor()
  assert.equal(await rows().count(), 0)
  await app.screenshot('06b-load-error-retry')
  await screen.getByRole('button', { name: '重试' }).click()
  await rows().first().waitFor()
  assert.equal(await rows().count(), 4)

  await screen.getByRole('button', { name: '重试' }).waitFor({ state: 'detached' })
  await app.screenshot('07-after-retry')
  assert.deepEqual(
    await browser.evaluate(() => JSON.parse(sessionStorage.getItem('page-errors') || '[]')),
    []
  )
})
